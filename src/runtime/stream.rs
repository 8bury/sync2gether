use super::{Role, Worker};
use crate::{
    player,
    protocol::{Control, Media, Message, StreamAnswer, StreamOffer},
    stream::{
        StreamEvent, StreamPhase, StreamTelemetry, cache::ChunkCache, guest, host,
        http::LocalProxy, webrtc,
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(15);
const STREAM_REQUEST_COOLDOWN: Duration = Duration::from_secs(2);
const MAX_STREAM_RECONNECTS: u32 = 5;
const RECONNECT_BACKOFF_SECS: [u64; 5] = [1, 2, 4, 6, 8];

fn next_stream_id() -> u64 {
    rand::random::<u64>().max(1)
}

pub(super) struct GuestSession {
    pub media: Media,
    pub hash_table_digest: String,
    pub cache: Arc<ChunkCache>,
    pub _proxy: Arc<LocalProxy>,
}

impl Drop for GuestSession {
    fn drop(&mut self) {
        self.cache.close_with_error("Streaming encerrado".into());
    }
}

pub(super) enum StreamPrepResult {
    HostReady {
        id: u64,
        offer: Box<StreamOffer>,
        pending: Box<host::HostPending>,
    },
    GuestReady {
        id: u64,
        answer: Box<StreamAnswer>,
        pending: Box<guest::GuestPending>,
    },
    WebReady {
        id: u64,
        answer: Box<crate::protocol::WebStreamAnswer>,
        host: Box<webrtc::WebHost>,
    },
    Failed {
        id: u64,
        error: String,
    },
}

impl Worker {
    pub(super) fn stop_stream_transport(&mut self) -> (u64, bool) {
        let active_id = self.stream_id;
        let was_active =
            self.stream_task.is_some() || self.stream_pending.is_some() || self.view.stream.active;
        self.stream_id = next_stream_id();
        if let Some(task) = self.stream_task.take() {
            task.abort();
        }
        self.stream_pending = None;
        self.stream_waiting_since = None;
        self.stream_reconnect_at = None;
        self.pending_stream_request = false;
        (active_id, was_active)
    }

    pub(super) fn stop_stream(&mut self, notify_peer: bool) {
        let (active_id, was_active) = self.stop_stream_transport();
        let had_guest_stream = self.guest_stream.take().is_some();
        self.stream_reconnect_attempts = 0;
        self.view.stream = StreamTelemetry::default();
        self.presence.streaming = false;
        self.presence.stream_progress = None;
        if had_guest_stream {
            self.next_generation += 1;
            self.generation = self.next_generation;
            self.reset_operation();
            self.player(player::Command::Stop);
            *self.player.frames.lock().unwrap() = None;
            self.view.player = player::Status::default();
            self.view.file = None;
            self.view.watching = false;
            self.watching_media = None;
            self.buffer_latch = super::sync::BufferLatch::Idle;
            self.presence.file_name = None;
            self.presence.media = None;
            self.presence.ready = false;
            self.presence.blocked = false;
            self.presence.buffering = false;
        }
        if notify_peer && (was_active || had_guest_stream) && self.view.connected {
            self.send(Message::StreamStop { id: active_id });
        }
    }

    pub(super) fn on_control_disconnected(&mut self) {
        self.stream_waiting_since = None;
        self.stream_reconnect_at = None;
        self.pending_stream_request = false;
        if self.stream_pending.take().is_some() {
            self.stream_id = next_stream_id();
            if self.guest_stream.is_none() {
                self.view.stream = StreamTelemetry::default();
                self.presence.streaming = false;
            }
        }
        if self.view.role == Role::Guest
            && self.guest_stream.is_none()
            && self.stream_task.is_none()
        {
            self.view.stream = StreamTelemetry::default();
            self.presence.streaming = false;
        }
    }

    pub(super) fn on_control_reconnected(&mut self) {
        if self.view.role == Role::Guest
            && self.guest_stream.is_some()
            && self.stream_task.is_none()
            && matches!(
                self.view.stream.phase,
                StreamPhase::Reconnecting | StreamPhase::Error(_)
            )
        {
            self.stream_reconnect_attempts = 0;
            self.request_stream_reconnect();
        }
    }

    fn sync_guest_cache_telemetry(&mut self, phase: StreamPhase) {
        if let Some(session) = &self.guest_stream {
            let stats = session.cache.stats();
            self.view.stream.active = matches!(
                phase,
                StreamPhase::Connecting
                    | StreamPhase::Probing
                    | StreamPhase::Streaming
                    | StreamPhase::Reconnecting
                    | StreamPhase::Complete
            );
            self.view.stream.is_sender = false;
            self.view.stream.phase = phase;
            self.view.stream.transfer_bps = 0;
            self.view.stream.cached_bytes = stats.cached_bytes;
            self.view.stream.total_bytes = stats.total_bytes;
            self.view.stream.buffered_ahead_bytes = stats.buffered_ahead_bytes;
            self.view.stream.chunks_cached = stats.chunks_cached;
            self.view.stream.chunks_total = stats.chunks_total;
            self.view.stream.cached_ranges = stats.cached_ranges;
        }
    }

    pub(super) fn refresh_stream_cache_stats(&mut self) {
        if self.stream_task.is_none()
            && let Some(session) = &self.guest_stream
        {
            let stats = session.cache.stats();
            self.view.stream.cached_bytes = stats.cached_bytes;
            self.view.stream.total_bytes = stats.total_bytes;
            self.view.stream.buffered_ahead_bytes = stats.buffered_ahead_bytes;
            self.view.stream.chunks_cached = stats.chunks_cached;
            self.view.stream.chunks_total = stats.chunks_total;
            self.view.stream.cached_ranges = stats.cached_ranges;
        }
    }

    fn request_stream_reconnect(&mut self) {
        if self.view.role != Role::Guest || self.guest_stream.is_none() {
            return;
        }
        let _ = self.stop_stream_transport();
        self.sync_guest_cache_telemetry(StreamPhase::Reconnecting);
        self.presence.streaming = true;
        if self.view.connected {
            self.stream_waiting_since = Some(Instant::now());
            self.send(Message::StreamRequest);
        }
    }

    fn schedule_stream_reconnect(&mut self, reason: String) {
        let _ = self.stop_stream_transport();
        let cache_alive = self
            .guest_stream
            .as_ref()
            .is_some_and(|s| s.cache.closed_reason().is_none());
        if self.view.role == Role::Guest && cache_alive {
            if self.stream_reconnect_attempts < MAX_STREAM_RECONNECTS {
                let idx =
                    (self.stream_reconnect_attempts as usize).min(RECONNECT_BACKOFF_SECS.len() - 1);
                let delay = Duration::from_secs(RECONNECT_BACKOFF_SECS[idx]);
                self.stream_reconnect_attempts += 1;
                self.sync_guest_cache_telemetry(StreamPhase::Reconnecting);
                self.presence.streaming = true;
                if self.view.connected {
                    self.stream_reconnect_at = Some(Instant::now() + delay);
                }
            } else {
                self.sync_guest_cache_telemetry(StreamPhase::Error(reason.clone()));
                self.presence.streaming = true;
                self.view.notice = Some(reason);
            }
        } else {
            self.stop_stream(false);
            self.view.stream.phase = StreamPhase::Error(reason.clone());
            self.view.notice = Some(reason);
        }
    }

    pub(super) fn start_stream(&mut self) {
        if !self.view.connected {
            return;
        }
        match self.view.role {
            Role::Host => {
                let (Some((path, index)), Some(media), Some(file_name)) = (
                    self.local_media.clone(),
                    self.presence.media.clone(),
                    self.presence.file_name.clone(),
                ) else {
                    return;
                };
                let _ = self.stop_stream_transport();
                let id = self.stream_id;
                let duration = self.presence.duration;
                let stun = self.view.internet;
                self.stream_waiting_since = Some(Instant::now());
                self.view.stream = StreamTelemetry {
                    active: true,
                    is_sender: true,
                    phase: StreamPhase::Offering,
                    total_bytes: media.bytes,
                    chunks_total: index.hashes.len() as u64,
                    ..Default::default()
                };
                self.presence.streaming = true;
                let tx = self.stream_prep_tx.clone();
                tokio::spawn(async move {
                    match host::prepare(id, path, media, file_name, duration, index, stun).await {
                        Ok((offer, pending)) => {
                            let _ = tx
                                .send(StreamPrepResult::HostReady {
                                    id,
                                    offer: Box::new(offer),
                                    pending: Box::new(pending),
                                })
                                .await;
                        }
                        Err(err) => {
                            let _ = tx
                                .send(StreamPrepResult::Failed {
                                    id,
                                    error: format!("Não foi possível iniciar streaming P2P: {err}"),
                                })
                                .await;
                        }
                    }
                });
            }
            Role::Guest => {
                if self.guest_stream.is_some() {
                    self.stream_reconnect_attempts = 0;
                    self.view.notice = None;
                    self.request_stream_reconnect();
                } else {
                    self.stop_stream(false);
                    self.stream_waiting_since = Some(Instant::now());
                    self.view.stream = StreamTelemetry {
                        active: true,
                        is_sender: false,
                        phase: StreamPhase::Connecting,
                        ..Default::default()
                    };
                    self.send(Message::StreamRequest);
                }
            }
            Role::Local => {}
        }
    }

    pub(super) fn handle_stream_request(&mut self) {
        if self.view.role != Role::Host || self.local_media.is_none() {
            return;
        }
        let offering =
            self.view.stream.active && matches!(self.view.stream.phase, StreamPhase::Offering);
        let cooling = self
            .last_stream_request
            .is_some_and(|at| at.elapsed() < STREAM_REQUEST_COOLDOWN);
        if offering || cooling {
            self.pending_stream_request = true;
            return;
        }
        self.pending_stream_request = false;
        self.last_stream_request = Some(Instant::now());
        self.start_stream();
    }

    pub(super) fn handle_stream_offer(&mut self, offer: StreamOffer) {
        if self.view.role != Role::Guest || !offer.valid() {
            return;
        }
        let existing_cache = self
            .guest_stream
            .as_ref()
            .filter(|session| {
                session.media == offer.media
                    && session
                        .hash_table_digest
                        .eq_ignore_ascii_case(&offer.hash_table_digest)
                    && session.cache.matches_offer(
                        &offer.media.hash,
                        offer.media.bytes,
                        offer.chunk_size,
                    )
            })
            .map(|session| session.cache.clone());
        let id = offer.id;
        let stun = self.view.internet;
        if existing_cache.is_some() {
            let _ = self.stop_stream_transport();
            self.stream_id = id;
            self.sync_guest_cache_telemetry(StreamPhase::Reconnecting);
        } else {
            self.stop_stream(false);
            self.stream_id = id;
            self.view.stream = StreamTelemetry {
                active: true,
                is_sender: false,
                phase: StreamPhase::Connecting,
                total_bytes: offer.media.bytes,
                ..Default::default()
            };
        }
        let prep_tx = self.stream_prep_tx.clone();
        tokio::spawn(async move {
            match guest::prepare_with_cache(offer, stun, None, existing_cache).await {
                Ok((answer, pending)) => {
                    let _ = prep_tx
                        .send(StreamPrepResult::GuestReady {
                            id,
                            answer: Box::new(answer),
                            pending: Box::new(pending),
                        })
                        .await;
                }
                Err(err) => {
                    let _ = prep_tx
                        .send(StreamPrepResult::Failed {
                            id,
                            error: format!("Não foi possível aceitar streaming P2P: {err}"),
                        })
                        .await;
                }
            }
        });
    }

    pub(super) fn handle_stream_answer(&mut self, answer: StreamAnswer) {
        if self.view.role != Role::Host || !answer.valid() {
            return;
        }
        if let Some(pending) = self.stream_pending.take() {
            if pending.id == answer.id {
                self.stream_waiting_since = None;
                let task = pending.accept(answer, self.stream_tx.clone());
                self.stream_task = Some(task);
            } else {
                self.stream_pending = Some(pending);
            }
        }
    }

    pub(super) fn handle_stream_stop(&mut self, id: u64) {
        if id == self.stream_id {
            if self.view.role == Role::Guest
                && self
                    .guest_stream
                    .as_ref()
                    .is_some_and(|session| session.cache.is_complete())
            {
                let _ = self.stop_stream_transport();
                self.sync_guest_cache_telemetry(StreamPhase::Complete);
                return;
            }
            self.stop_stream(false);
        }
    }

    pub(super) fn check_stream_timeout(&mut self) {
        if self.pending_stream_request
            && self.view.role == Role::Host
            && self.view.connected
            && self.local_media.is_some()
            && !(self.view.stream.active && matches!(self.view.stream.phase, StreamPhase::Offering))
            && self
                .last_stream_request
                .is_none_or(|at| at.elapsed() >= STREAM_REQUEST_COOLDOWN)
        {
            self.pending_stream_request = false;
            self.last_stream_request = Some(Instant::now());
            self.start_stream();
        }
        if let Some(at) = self.stream_reconnect_at
            && Instant::now() >= at
        {
            self.stream_reconnect_at = None;
            if self.view.connected && self.guest_stream.is_some() {
                self.request_stream_reconnect();
            }
        }
        if let Some(since) = self.stream_waiting_since
            && since.elapsed() >= NEGOTIATION_TIMEOUT
        {
            let error =
                "O parceiro não respondeu à negociação de streaming P2P a tempo.".to_owned();
            if self.view.role == Role::Guest && self.guest_stream.is_some() {
                self.schedule_stream_reconnect(error);
            } else {
                let notify = self.view.role == Role::Guest;
                self.stop_stream(notify);
                self.view.stream.phase = StreamPhase::Error(error.clone());
                self.view.notice = Some(error);
            }
        }
    }

    pub(super) fn stream_prep_event(&mut self, result: StreamPrepResult) {
        match result {
            StreamPrepResult::HostReady { id, offer, pending } if id == self.stream_id => {
                self.stream_pending = Some(*pending);
                self.send(Message::StreamOffer(offer));
            }
            StreamPrepResult::GuestReady {
                id,
                answer,
                pending,
            } if id == self.stream_id => {
                self.send(Message::StreamAnswer(answer));
                self.stream_task = Some(pending.connect(self.stream_tx.clone()));
            }
            StreamPrepResult::WebReady { id, answer, host } if id == self.stream_id => {
                self.web_stream_ready(*answer, *host);
            }
            StreamPrepResult::Failed { id, error } if id == self.stream_id => {
                if self.view.role == Role::Guest && self.guest_stream.is_some() {
                    self.schedule_stream_reconnect(error);
                } else {
                    let notify =
                        self.view.role == Role::Guest || self.session.state.guest.web_guest;
                    self.stop_stream(notify);
                    self.view.stream.phase = StreamPhase::Error(error.clone());
                    self.view.notice = Some(error);
                }
            }
            _ => {}
        }
    }

    pub(super) fn stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Telemetry { id, telemetry } if id == self.stream_id => {
                if matches!(
                    telemetry.phase,
                    StreamPhase::Streaming | StreamPhase::Complete
                ) {
                    self.stream_reconnect_attempts = 0;
                    self.stream_reconnect_at = None;
                }
                self.view.stream = telemetry;
            }
            StreamEvent::Ready {
                id,
                url,
                media,
                file_name,
                hash_table_digest,
                cache,
                proxy,
                telemetry,
            } if id == self.stream_id => {
                self.guest_stream = Some(GuestSession {
                    media: media.clone(),
                    hash_table_digest,
                    cache,
                    _proxy: proxy,
                });
                self.stream_reconnect_attempts = 0;
                self.stream_reconnect_at = None;
                self.view.stream = telemetry;
                self.cancel_open();
                self.local_media = None;
                self.control(Control::Pause);
                self.reset_operation();
                self.next_generation += 1;
                self.generation = self.next_generation;
                *self.player.frames.lock().unwrap() = None;
                self.view.file = Some(file_name.clone());
                self.presence.file_name = Some(file_name);
                self.presence.media = Some(media);
                self.presence.ready = false;
                self.presence.loading = false;
                self.presence.streaming = true;
                self.view.player = player::Status::default();
                self.view.error = None;
                self.view.notice = None;
                self.player(player::Command::LoadUrl(url, self.generation));
            }
            StreamEvent::Disconnected { id, reason } if id == self.stream_id => {
                self.schedule_stream_reconnect(reason);
            }
            StreamEvent::Ended { id } if id == self.stream_id => {
                self.stream_task = None;
                let peer_complete = self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.guest.stream_progress.as_ref())
                    .is_some_and(|p| p.complete);
                if self.view.role == Role::Host && peer_complete {
                    self.view.stream.phase = StreamPhase::Complete;
                    self.view.stream.transfer_bps = 0;
                    self.view.stream.cached_bytes = self.view.stream.total_bytes;
                    self.view.stream.chunks_cached = self.view.stream.chunks_total;
                } else {
                    self.stop_stream(false);
                }
            }
            StreamEvent::Failed { id, error } if id == self.stream_id => {
                if self.view.role == Role::Guest
                    && self
                        .guest_stream
                        .as_ref()
                        .is_some_and(|s| s.cache.closed_reason().is_none())
                {
                    self.schedule_stream_reconnect(error);
                } else {
                    let notify =
                        self.view.role == Role::Guest || self.session.state.guest.web_guest;
                    self.stop_stream(notify);
                    self.view.stream.phase = StreamPhase::Error(error.clone());
                    self.view.notice = Some(error);
                }
            }
            _ => {}
        }
    }
}

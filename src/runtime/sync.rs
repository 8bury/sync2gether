use super::{Role, Worker};
use crate::{
    host::{HostNotice, HostOutput, PlayerCommand as HostPlayerCommand, PlayerStatus},
    player,
    protocol::{Control, Message, Playback, Side, Snapshot, StreamProgress},
    stream::{CHUNK_SIZE, StreamPhase},
    sync::Correction,
};
use std::time::{Duration, Instant};

const BUFFER_RESUME_SECS: f64 = 3.0;
const BUFFER_FALLBACK_BYTES: u64 = (CHUNK_SIZE as u64) * 2;
const BUFFER_MIN_CLEAR_LOCAL: Duration = Duration::from_millis(1200);
const BUFFER_STREAM_CLEAR_WAIT: Duration = Duration::from_secs(4);
const BUFFER_MAX_RECOVERY: Duration = Duration::from_secs(12);

pub(super) fn to_host_player_status(status: &player::Status) -> PlayerStatus {
    PlayerStatus {
        generation: status.generation,
        measured_ms: status.measured_ms,
        position: status.position,
        duration: status.duration,
        speed: status.speed,
        paused: status.paused,
        seeking: status.seeking,
        loaded: status.loaded,
        buffering: status.buffering,
        blocked: status.blocked,
        ended: status.ended,
        cache_time: status.cache_time,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum BufferLatch {
    #[default]
    Idle,
    Active,
    Recovering(Instant),
}

impl Worker {
    pub(super) fn sync_host_inputs(&mut self) {
        self.host.set_generation(self.generation);
        self.host.set_fatal_error(self.fatal_error.is_some());
        self.host
            .set_stream_errored(matches!(self.view.stream.phase, StreamPhase::Error(_)));
        let streaming = self.view.stream.active;
        let web_stream = self.local_media.is_some();
        let web_remux = web_stream && crate::stream::ffmpeg::is_available();
        let presence = self.host.presence_mut();
        presence.media = self.presence.media.clone();
        presence.file_name = self.presence.file_name.clone();
        presence.loading = self.presence.loading;
        presence.verification = self.presence.verification;
        presence.streaming = streaming;
        presence.web_stream = web_stream;
        presence.web_remux = web_remux;
    }

    pub(super) fn flush_host(&mut self) {
        let outputs = self.host.take_outputs();
        self.presence = self.host.presence().clone();
        self.snapshot = self.host.snapshot().cloned();
        self.view.watching = self.host.watching();
        self.view.peer_left = self.host.peer_left();
        self.view.scheduled_start_ms = self.host.scheduled_start_ms();
        self.view.last_start_ms = self.host.last_start_ms();
        self.view.last_start_id = self.host.last_start_id();
        if self.view.stream.phase == StreamPhase::Complete
            && let Some(state) = self.host.snapshot()
            && state.guest.media.is_some()
            && state.guest.media != state.host.media
        {
            self.stop_stream(false);
        }
        for output in outputs {
            match output {
                HostOutput::Send(message) => self.send(message),
                HostOutput::Player(command) => match command {
                    HostPlayerCommand::Prepare { token, position } => {
                        self.player(player::Command::Prepare {
                            id: token,
                            position,
                        });
                    }
                    HostPlayerCommand::Schedule { token, at_ms } => {
                        let delay = (at_ms - self.now()).max(0.0);
                        let at = Instant::now() + Duration::from_secs_f64(delay / 1000.0);
                        self.player(player::Command::Schedule { id: token, at });
                    }
                    HostPlayerCommand::Pause { paused } => {
                        self.player(player::Command::Pause(paused));
                    }
                    HostPlayerCommand::Seek { position } => {
                        self.player(player::Command::Seek(position));
                    }
                    HostPlayerCommand::Speed { speed } => {
                        self.player(player::Command::Speed(speed));
                    }
                },
                HostOutput::Notice(notice) => {
                    let errors = &crate::i18n::text().errors;
                    self.view.notice = Some(match notice {
                        HostNotice::Reconnected => errors.reconnected.into(),
                        HostNotice::StartCancelled => errors.start_cancelled.into(),
                        HostNotice::StartTimeout => errors.start_timeout.into(),
                    });
                }
                HostOutput::ClearNotice => self.view.notice = None,
                HostOutput::Disconnect => self.disconnected(),
                HostOutput::WebGuestJoined => {
                    self.queue_shared_subtitle();
                    self.drain_subtitle_outbox();
                }
                HostOutput::StreamRequest => self.handle_stream_request(),
                HostOutput::StreamAnswer(answer) => self.handle_stream_answer(*answer),
                HostOutput::WebStreamOffer(offer) => self.handle_web_stream_offer(*offer),
                HostOutput::StreamStop { id } => self.handle_stream_stop(id),
                HostOutput::SubtitleRequest { track } => self.request_embedded_subtitle(track),
            }
        }
    }

    pub(super) fn control(&mut self, control: Control) {
        self.control_by(control, Side::Host);
    }

    pub(super) fn control_by(&mut self, control: Control, by: Side) {
        if self.view.role == Role::Host {
            self.sync_host_inputs();
            self.host.control(control, by, self.now());
            self.flush_host();
            return;
        }
        if !control.valid() {
            return;
        }
        self.view.notice = None;
        if matches!(control, Control::Play)
            && !self.view.player.buffering
            && matches!(self.buffer_latch, BufferLatch::Recovering(_))
        {
            self.buffer_latch = BufferLatch::Idle;
        }
        match self.view.role {
            Role::Local => match control {
                Control::Play => {
                    self.view.watching = true;
                    self.player(player::Command::Pause(false));
                }
                Control::Pause => self.player(player::Command::Pause(true)),
                Control::Seek(p) => {
                    self.player(player::Command::Seek(p.min(self.view.player.duration)))
                }
            },
            Role::Host => {}
            Role::Guest => {
                if self.view.connected {
                    if matches!(control, Control::Play) {
                        self.refresh_presence();
                        self.send(Message::Presence(Box::new(self.presence.clone())));
                    }
                    self.send(Message::Request(control));
                }
            }
        }
    }

    pub(super) fn player_event(&mut self, event: player::Event) {
        if self.view.role == Role::Host {
            self.sync_host_inputs();
            let now = self.now();
            match event {
                player::Event::Status(status) => {
                    if status.generation == self.generation {
                        self.view.player = status.clone();
                    }
                    self.host.player_status(to_host_player_status(&status), now);
                }
                player::Event::Prepared { id } => self.host.player_prepared(id, now),
                player::Event::Started { id, at } => {
                    let local = at.duration_since(self.origin).as_secs_f64() * 1000.0;
                    self.host.player_started(id, local, now);
                }
                player::Event::Missed { id } => self.host.player_missed(id, now),
                player::Event::Unavailable(error) => {
                    self.fatal_error = Some(error);
                    self.host.player_unavailable(now);
                }
                player::Event::Error(error) => {
                    self.view.error = Some(error);
                    self.host.player_error(now);
                }
            }
            self.flush_host();
            return;
        }
        match event {
            player::Event::Status(status) if status.generation == self.generation => {
                if self.view.role == Role::Local
                    && status.loaded
                    && !self.view.loading
                    && (!self.view.player.loaded
                        || self.view.player.generation != status.generation)
                {
                    self.view.watching = true;
                }
                self.view.player = status;
                self.refresh_presence();
                if self.presence.blocked
                    && let Some(id) = self.scheduled
                {
                    self.operation_failed(id);
                }
            }
            player::Event::Status(_) => {}
            player::Event::Prepared { id }
                if self
                    .player_operation
                    .is_some_and(|(token, wire)| token == id && self.requested == Some(wire)) =>
            {
                let id = self.player_operation.unwrap().1;
                self.prepared = Some(id);
                if self.view.role == Role::Guest {
                    self.send(Message::Prepared { id });
                }
            }
            player::Event::Prepared { .. } => {}
            player::Event::Started { id, at }
                if self
                    .player_operation
                    .is_some_and(|(token, wire)| token == id && self.scheduled == Some(wire)) =>
            {
                let id = self.player_operation.unwrap().1;
                let local = at.duration_since(self.origin).as_secs_f64() * 1000.0;
                self.view.last_start_ms = self.clock.host_now(local);
                self.view.last_start_id = Some(id);
                self.controller.reset(self.now());
            }
            player::Event::Started { .. } => {}
            player::Event::Missed { id } => {
                if let Some((token, wire)) = self.player_operation
                    && token == id
                {
                    self.operation_failed(wire);
                }
            }
            player::Event::Unavailable(error) => {
                self.fatal_error = Some(error);
                self.control(Control::Pause);
            }
            player::Event::Error(error) => {
                self.view.error = Some(error);
                self.presence.ready = false;
                self.presence.blocked = true;
                self.control(Control::Pause);
            }
        }
        self.progress();
    }

    pub(super) fn refresh_presence(&mut self) {
        if self.view.role == Role::Host {
            self.sync_host_inputs();
            self.host.refresh_presence(self.now());
            self.presence = self.host.presence().clone();
            return;
        }
        self.refresh_stream_cache_stats();
        let p = &self.view.player;
        let now = self.now();
        let stream_reconnecting_stalled = self.view.role == Role::Guest
            && self.guest_stream.is_some()
            && self.view.stream.phase == StreamPhase::Reconnecting
            && self.view.stream.buffered_ahead_bytes == 0;
        let stream_errored = matches!(self.view.stream.phase, StreamPhase::Error(_));
        if !p.loaded || p.ended || (stream_errored && !p.buffering) {
            self.buffer_latch = BufferLatch::Idle;
        } else if p.buffering || stream_reconnecting_stalled {
            self.buffer_latch = BufferLatch::Active;
        } else {
            if matches!(self.buffer_latch, BufferLatch::Active) {
                self.buffer_latch = BufferLatch::Recovering(Instant::now());
            }
            if let BufferLatch::Recovering(cleared_at) = self.buffer_latch {
                let demux_ahead = (p.cache_time - p.position).max(0.0);
                let remaining_media = if p.duration > 0.0 {
                    (p.duration - p.position - 0.5).max(0.0)
                } else {
                    BUFFER_RESUME_SECS
                };
                let target_secs = BUFFER_RESUME_SECS.min(remaining_media.max(0.5));
                let recovered = if self.view.role == Role::Guest
                    && let Some(session) = &self.guest_stream
                    && self.view.stream.phase != StreamPhase::Complete
                    && !session.cache.is_complete()
                {
                    let cache_ok = self.view.stream.buffered_seconds().map_or(
                        self.view.stream.buffered_ahead_bytes >= BUFFER_FALLBACK_BYTES,
                        |secs| secs >= target_secs,
                    );
                    self.view.stream.phase != StreamPhase::Reconnecting
                        && (cache_ok
                            || demux_ahead >= target_secs
                            || (cleared_at.elapsed() >= BUFFER_STREAM_CLEAR_WAIT
                                && self.view.stream.buffered_ahead_bytes > 0)
                            || cleared_at.elapsed() >= BUFFER_MAX_RECOVERY)
                } else {
                    demux_ahead >= target_secs.min(2.0)
                        || cleared_at.elapsed() >= BUFFER_MIN_CLEAR_LOCAL
                };
                if recovered {
                    self.buffer_latch = BufferLatch::Idle;
                }
            }
        }
        self.presence.buffering =
            p.loaded && !p.ended && !matches!(self.buffer_latch, BufferLatch::Idle);
        self.presence.ready = p.loaded
            && p.generation == self.generation
            && self.presence.media.is_some()
            && self.fatal_error.is_none()
            && now - p.measured_ms < 1500.0;
        self.presence.blocked = p.blocked || self.presence.buffering || self.fatal_error.is_some();
        self.presence.ended = p.ended;
        self.presence.duration = p.duration;
        self.presence.clock_ready = self.clock.ready(now);
        self.presence.rtt_ms = self.clock.rtt_ms;
        self.presence.uncertainty_ms = self.clock.uncertainty_ms;
        self.presence.streaming = self.view.stream.active || self.guest_stream.is_some();
        self.presence.web_stream = false;
        self.presence.web_remux = false;
        self.presence.stream_progress = if self.view.role == Role::Guest
            && (self.guest_stream.is_some()
                || (self.view.stream.active && !self.view.stream.is_sender))
        {
            let s = &self.view.stream;
            Some(StreamProgress {
                cached_bytes: s.cached_bytes,
                total_bytes: s.total_bytes,
                transfer_bps: s.transfer_bps,
                link_bps: s.link_bps,
                buffered_ahead_bytes: s.buffered_ahead_bytes,
                reconnecting: matches!(
                    s.phase,
                    StreamPhase::Reconnecting | StreamPhase::Connecting | StreamPhase::Probing
                ),
                complete: s.phase == StreamPhase::Complete,
                cached_ranges: s.cached_ranges.clone(),
            })
        } else {
            None
        };
        self.presence.playback = self.clock.host_now(p.measured_ms).map(|host_ms| Playback {
            position: p.position,
            host_ms,
            speed: p.speed,
            paused: p.paused || self.presence.blocked,
            seeking: p.seeking,
        });
    }

    pub(super) fn operation_failed(&mut self, id: u64) {
        if self.requested != Some(id) && self.scheduled != Some(id) {
            return;
        }
        self.reset_operation();
        self.failed = Some(id);
        self.view.notice = Some(crate::i18n::text().errors.start_cancelled.into());
        self.send(Message::Failed { id });
    }

    pub(super) fn apply_snapshot(&mut self, state: Snapshot) {
        if self.view.role == Role::Host {
            self.host.apply_snapshot(state, self.now());
            self.flush_host();
            return;
        }
        if self.view.role == Role::Guest
            && let Some(session) = &self.guest_stream
            && let Some(host_media) = &state.host.media
            && host_media != &session.media
        {
            self.stop_stream(false);
        }
        let media_changed = self.snapshot.as_ref().is_some_and(|old| {
            let host_changed = old.host.media != state.host.media
                && !(old.host.media.is_none()
                    && state.host.media.is_some()
                    && state.host.media == self.watching_media);
            let guest_changed = old.guest.media != state.guest.media
                && !(old.guest.media.is_none()
                    && state.guest.media.is_some()
                    && state.guest.media == self.watching_media);
            host_changed || guest_changed
        });
        let switched_away = self.watching_media.as_ref().is_some_and(|watching| {
            state.host.media.as_ref().is_some_and(|m| m != watching)
                || state.guest.media.as_ref().is_some_and(|m| m != watching)
        });
        if media_changed
            || switched_away
            || (self.view.watching && (state.host.loading || state.guest.loading))
        {
            self.view.watching = false;
            self.watching_media = None;
        }
        if !state.paused || state.transition.as_ref().is_some_and(|t| t.resume) {
            self.view.watching = true;
            self.watching_media = state.host.media.clone();
        }

        self.snapshot = Some(state.clone());
        if let Some(t) = &state.transition {
            if self.failed == Some(t.id) {
                return;
            }
            if self.requested != Some(t.id) {
                self.reset_operation();
                self.requested = Some(t.id);
                self.player_token += 1;
                self.player_operation = Some((self.player_token, t.id));
                self.view.notice = None;
                self.player(player::Command::Prepare {
                    id: self.player_token,
                    position: t.position,
                });
                self.speed = 1.0;
            }
            if let Some(start) = t.start_at_ms {
                if self.prepared != Some(t.id) {
                    self.operation_failed(t.id);
                    return;
                }
                if self.scheduled != Some(t.id) {
                    let Some(host_now) = self.clock.host_now(self.now()) else {
                        self.operation_failed(t.id);
                        return;
                    };
                    if start - host_now < 50.0 {
                        self.operation_failed(t.id);
                        return;
                    }
                    let at = Instant::now() + Duration::from_secs_f64((start - host_now) / 1000.0);
                    self.scheduled = Some(t.id);
                    self.view.scheduled_start_ms = Some(start);
                    self.player(player::Command::Schedule {
                        id: self.player_operation.unwrap().0,
                        at,
                    });
                    self.send(Message::Armed { id: t.id });
                }
            }
        } else if state.paused && self.last_pause != Some(state.revision) {
            self.reset_operation();
            self.last_pause = Some(state.revision);
        }
    }

    pub(super) fn progress(&mut self) {
        if self.view.role == Role::Local {
            return;
        }
        self.check_stream_timeout();
        if self.view.role == Role::Host {
            self.sync_host_inputs();
            self.host.progress(self.now());
            self.flush_host();
            return;
        }
        self.refresh_presence();
        if (!self.clock.ready(self.now()) || !self.presence.ready || self.presence.blocked)
            && let Some(id) = self.scheduled
        {
            self.operation_failed(id);
        }
    }

    pub(super) fn correct(&mut self) {
        if self.view.role == Role::Local {
            return;
        }
        if self.view.role == Role::Host {
            self.sync_host_inputs();
            self.host.correct(self.now());
            self.flush_host();
            return;
        }
        let Some(state) = self.snapshot.clone() else {
            return;
        };
        if state.transition.is_some()
            || !self.presence.ready
            || self.view.player.seeking
            || self.now() - self.view.player.measured_ms > 350.0
        {
            self.set_speed(1.0);
            return;
        }
        let Some(now) = self.clock.host_now(self.now()) else {
            self.set_speed(1.0);
            return;
        };
        let matched = state.host.media.is_some() && state.host.media == state.guest.media;
        if !matched {
            self.set_speed(1.0);
            return;
        }
        let actual = self.view.player.position_at(self.now());
        let target = state.position_at(now);
        match self
            .controller
            .update(actual, target, state.paused, self.now())
        {
            Correction::Seek(p) => {
                self.player(player::Command::Seek(p.min(self.view.player.duration)));
                self.set_speed(1.0);
            }
            Correction::Speed(speed) => self.set_speed(speed),
            Correction::None => self.set_speed(1.0),
        }
    }
}

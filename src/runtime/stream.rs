use super::{Role, Worker};
use crate::{
    player,
    protocol::{Control, Message, StreamAnswer, StreamOffer},
    stream::{StreamEvent, StreamPhase, StreamTelemetry, guest, host},
};
use tokio::task::JoinHandle;

pub(super) enum StreamPrepResult {
    HostReady {
        id: u64,
        offer: Box<StreamOffer>,
        pending: Box<host::HostPending>,
    },
    GuestReady {
        id: u64,
        answer: Box<StreamAnswer>,
        task: JoinHandle<()>,
    },
    Failed {
        id: u64,
        error: String,
    },
}

impl Worker {
    pub(super) fn stop_stream(&mut self, notify_peer: bool) {
        let active_id = self.stream_id;
        let was_active =
            self.stream_task.is_some() || self.stream_pending.is_some() || self.view.stream.active;
        self.stream_id = self.stream_id.wrapping_add(1);
        if let Some(task) = self.stream_task.take() {
            task.abort();
        }
        self.stream_pending = None;
        self.view.stream = StreamTelemetry::default();
        self.presence.streaming = false;
        if notify_peer && was_active && self.view.connected {
            self.send(Message::StreamStop { id: active_id });
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
                self.stop_stream(false);
                let id = self.stream_id;
                let duration = self.presence.duration;
                let stun = self.view.internet;
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
                self.stop_stream(false);
                self.view.stream = StreamTelemetry {
                    active: true,
                    is_sender: false,
                    phase: StreamPhase::Connecting,
                    ..Default::default()
                };
                self.send(Message::StreamRequest);
            }
            Role::Local => {}
        }
    }

    pub(super) fn handle_stream_request(&mut self) {
        if self.view.role == Role::Host && self.local_media.is_some() {
            self.start_stream();
        }
    }

    pub(super) fn handle_stream_offer(&mut self, offer: StreamOffer) {
        if self.view.role != Role::Guest || !offer.valid() {
            return;
        }
        self.stop_stream(false);
        self.stream_id = offer.id;
        let id = offer.id;
        let stun = self.view.internet;
        self.view.stream = StreamTelemetry {
            active: true,
            is_sender: false,
            phase: StreamPhase::Connecting,
            total_bytes: offer.media.bytes,
            ..Default::default()
        };
        let events = self.stream_tx.clone();
        let prep_tx = self.stream_prep_tx.clone();
        tokio::spawn(async move {
            match guest::start(offer, stun, None, events).await {
                Ok((answer, task)) => {
                    let _ = prep_tx
                        .send(StreamPrepResult::GuestReady {
                            id,
                            answer: Box::new(answer),
                            task,
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
                let task = pending.accept(answer, self.stream_tx.clone());
                self.stream_task = Some(task);
            } else {
                self.stream_pending = Some(pending);
            }
        }
    }

    pub(super) fn handle_stream_stop(&mut self, id: u64) {
        if id == self.stream_id {
            self.stop_stream(false);
        }
    }

    pub(super) fn stream_prep_event(&mut self, result: StreamPrepResult) {
        match result {
            StreamPrepResult::HostReady { id, offer, pending } if id == self.stream_id => {
                self.stream_pending = Some(*pending);
                self.send(Message::StreamOffer(offer));
            }
            StreamPrepResult::GuestReady { id, answer, task } if id == self.stream_id => {
                self.stream_task = Some(task);
                self.send(Message::StreamAnswer(answer));
            }
            StreamPrepResult::Failed { id, error } if id == self.stream_id => {
                self.stop_stream(false);
                self.view.stream.phase = StreamPhase::Error(error.clone());
                self.view.notice = Some(error);
            }
            _ => {}
        }
    }

    pub(super) fn stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Telemetry { id, telemetry } if id == self.stream_id => {
                self.view.stream = telemetry;
            }
            StreamEvent::Ready {
                id,
                url,
                media,
                file_name,
                telemetry,
            } if id == self.stream_id => {
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
            StreamEvent::Failed { id, error } if id == self.stream_id => {
                self.stop_stream(false);
                self.view.stream.phase = StreamPhase::Error(error.clone());
                self.view.notice = Some(error);
            }
            _ => {}
        }
    }
}

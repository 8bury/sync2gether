use super::Worker;
use crate::{
    player,
    protocol::{Control, Media, Verification},
    stream::ChunkIndex,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) struct FileResult {
    generation: u64,
    event: FileEvent,
}

enum FileEvent {
    Selected(String),
    Progress(Verification),
    Finished(Result<Option<(PathBuf, Media, ChunkIndex)>, String>),
}

impl Worker {
    pub(super) fn cancel_open(&mut self) {
        if let Some(cancel) = self.file_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        if self.view.loading {
            self.generation = self.view.player.generation;
            self.presence = self.previous.take().unwrap_or_default();
        }
        self.view.loading = false;
        self.view.pending_file = None;
        self.view.verification = None;
    }

    pub(super) fn open(&mut self, path: Option<PathBuf>) {
        if self.view.loading {
            return;
        }
        self.previous = Some(self.presence.clone());
        self.next_generation += 1;
        self.generation = self.next_generation;
        let generation = self.generation;
        self.view.loading = true;
        self.view.watching = false;
        self.watching_media = None;
        self.buffer_latch = super::sync::BufferLatch::Idle;
        self.view.error = None;
        self.view.pending_file = None;
        self.view.verification = None;
        let cancel = Arc::new(AtomicBool::new(false));
        self.file_cancel = Some(cancel.clone());
        let files = self.files.clone();
        tokio::task::spawn_blocking(move || {
            let selected = path.or_else(|| {
                rfd::FileDialog::new()
                    .set_title(crate::i18n::text().errors.pick_title)
                    .pick_file()
            });
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let result = selected
                .map(|path| {
                    let name = crate::media::display_name(&path);
                    let _ = files.blocking_send(FileResult {
                        generation,
                        event: FileEvent::Selected(name),
                    });
                    let mut last_progress = None;
                    path.canonicalize()
                        .and_then(|path| {
                            crate::media::identify_with_chunks_and_progress(
                                &path,
                                |bytes, total| {
                                    if cancel.load(Ordering::Relaxed) {
                                        return Err(std::io::Error::other(
                                            crate::i18n::text().errors.verification_cancelled,
                                        ));
                                    }
                                    if last_progress.is_none_or(|at: Instant| {
                                        at.elapsed() >= Duration::from_millis(100)
                                    }) || bytes == total
                                    {
                                        files
                                            .blocking_send(FileResult {
                                                generation,
                                                event: FileEvent::Progress(Verification {
                                                    bytes,
                                                    total,
                                                }),
                                            })
                                            .map_err(|_| {
                                                std::io::Error::other(
                                                    crate::i18n::text().errors.app_closed,
                                                )
                                            })?;
                                        last_progress = Some(Instant::now());
                                    }
                                    Ok(())
                                },
                            )
                            .map(|(media, index)| (path, media, index))
                        })
                        .map_err(|_| crate::i18n::text().errors.verify_failed.into())
                })
                .transpose();
            if !cancel.load(Ordering::Relaxed) {
                let _ = files.blocking_send(FileResult {
                    generation,
                    event: FileEvent::Finished(result),
                });
            }
        });
        if self.view.role == super::Role::Host {
            self.host.begin_loading(self.generation, self.now());
            self.flush_host();
        } else {
            self.control(Control::Pause);
            self.reset_operation();
            self.presence.ready = false;
            self.presence.media = None;
            self.presence.file_name = None;
            self.presence.loading = true;
            self.presence.verification = None;
        }
    }

    pub(super) fn file_event(&mut self, file: FileResult) {
        if !self.view.loading || file.generation != self.generation {
            return;
        }
        match file.event {
            FileEvent::Selected(name) => {
                self.view.pending_file = Some(name.clone());
                self.presence.file_name = Some(name);
            }
            FileEvent::Progress(progress) => {
                self.view.verification = Some(progress);
                self.presence.verification = Some(progress);
            }
            FileEvent::Finished(result) => {
                match result {
                    Ok(Some((path, media, index))) => {
                        self.previous = None;
                        self.stop_stream(true);
                        self.clear_shared_subtitle(self.view.role == super::Role::Host);
                        self.clear_embedded_subtitles();
                        self.local_media = Some((path.clone(), Arc::new(index)));
                        if self.view.role == super::Role::Host {
                            self.sync_host_inputs();
                            self.host.set_media_loaded(media, self.now());
                            self.flush_host();
                        } else {
                            self.reset_operation();
                            self.presence.media = Some(media);
                            self.presence.ready = false;
                            self.presence.streaming = false;
                        }
                        *self.player.frames.lock().unwrap() = None;
                        self.view.file = self.view.pending_file.take();
                        self.view.player = player::Status::default();
                        self.player(player::Command::Load(path, self.generation));
                    }
                    Ok(None) => {
                        self.generation = self.view.player.generation;
                        self.presence = self.previous.take().unwrap_or_default();
                    }
                    Err(error) => {
                        self.view.error = Some(error);
                        self.generation = self.view.player.generation;
                        self.presence = self.previous.take().unwrap_or_default();
                    }
                }
                self.file_cancel = None;
                self.view.loading = false;
                self.view.pending_file = None;
                self.view.verification = None;
                self.presence.loading = false;
                self.presence.verification = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Presence;

    #[test]
    fn cancel_restores_the_loaded_file_and_ignores_late_events() {
        let (player, commands) = player::Player::test_channel();
        let mut worker = Worker::new(player, Instant::now());
        let original = Presence {
            media: Some(Media {
                hash: "a".repeat(64),
                bytes: 42,
            }),
            file_name: Some("original.mkv".into()),
            ready: true,
            ..Default::default()
        };
        worker.view.file = original.file_name.clone();
        worker.view.player.generation = 4;
        worker.view.player.loaded = true;
        worker.view.loading = true;
        worker.generation = 5;
        worker.previous = Some(original.clone());
        worker.presence.loading = true;
        let cancel = Arc::new(AtomicBool::new(false));
        worker.file_cancel = Some(cancel.clone());
        worker.file_event(FileResult {
            generation: 5,
            event: FileEvent::Selected("new.mkv".into()),
        });
        worker.file_event(FileResult {
            generation: 5,
            event: FileEvent::Progress(Verification {
                bytes: 50,
                total: 100,
            }),
        });
        assert_eq!(worker.view.pending_file.as_deref(), Some("new.mkv"));
        assert_eq!(worker.presence.verification.unwrap().bytes, 50);
        worker.cancel_open();
        assert!(cancel.load(Ordering::Relaxed));
        assert_eq!(worker.presence.file_name, original.file_name);
        assert_eq!(worker.presence.media, original.media);
        assert!(!worker.view.loading);
        assert!(worker.view.pending_file.is_none() && worker.view.verification.is_none());
        worker.file_event(FileResult {
            generation: 5,
            event: FileEvent::Finished(Ok(Some((
                PathBuf::from("unused.mkv"),
                original.media.unwrap(),
                ChunkIndex::from_hashes(crate::stream::CHUNK_SIZE, 42, vec![]),
            )))),
        });
        assert_eq!(worker.view.file.as_deref(), Some("original.mkv"));
        assert!(
            commands.try_recv().is_err(),
            "a cancelled file must never reach the player"
        );
        worker.view.loading = true;
        worker.generation = 6;
        worker.file_event(FileResult {
            generation: 5,
            event: FileEvent::Selected("stale.mkv".into()),
        });
        assert!(worker.view.pending_file.is_none());
    }
}

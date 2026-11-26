use super::{Role, Worker};

impl Worker {
    pub(super) fn update_view(&mut self) {
        if let Some(error) = &self.fatal_error {
            self.view.error = Some(error.clone());
            self.view.player.loaded = false;
        }
        self.view.ready = false;
        self.view.matched = false;
        self.view.peer_ready = false;
        self.view.verified = self.presence.media.is_some() && !self.view.loading;
        self.view.peer_file = None;
        self.view.peer_loading = false;
        self.view.peer_verification = None;
        self.view.peer_verified = false;
        self.view.peer_blocked = false;
        self.view.buffering = self.presence.buffering;
        self.view.peer_buffering = false;
        self.view.waiting_buffer = false;
        self.view.peer_streaming = false;
        self.view.peer_stream = None;
        self.view.can_stream = false;
        self.view.preparing = false;
        self.view.scheduled = false;
        self.view.peer_drift = None;
        self.view.rtt_ms = self.clock.rtt_ms;
        if let Some(state) = &self.snapshot {
            if self.view.connected {
                let peer = if self.view.role == Role::Host {
                    &state.guest
                } else {
                    &state.host
                };
                self.view.peer_file = peer.file_name.clone();
                self.view.peer_loading = peer.loading;
                self.view.peer_verification = peer.verification;
                self.view.peer_verified = peer.media.is_some();
                self.view.peer_blocked = peer.blocked;
                self.view.peer_buffering = peer.buffering;
                self.view.peer_streaming = peer.streaming;
                self.view.peer_stream = peer.stream_progress.clone();
            }
            self.view.waiting_buffer = state.waiting_buffer;
            self.view.can_stream = self.view.connected
                && !self.view.stream.active
                && match self.view.role {
                    Role::Host => {
                        self.local_media.is_some()
                            && self.presence.media.is_some()
                            && !self.view.loading
                            && !state.guest.web_guest
                    }
                    Role::Guest => state.host.media.is_some() && !state.host.loading,
                    Role::Local => false,
                };
            self.view.peer_ready = if self.view.role == Role::Host {
                state.guest.ready
            } else {
                state.host.ready
            };
            self.view.matched = self.view.connected
                && state.host.media.is_some()
                && state.host.media == state.guest.media;
            self.view.ready = self.view.matched
                && state.host.ready
                && state.guest.ready
                && state.host.clock_ready
                && state.guest.clock_ready
                && state.transition.is_none()
                && !state.host.blocked
                && !state.guest.blocked
                && !state.host.buffering
                && !state.guest.buffering
                && self.clock.ready(self.now());
            self.view.preparing = state
                .transition
                .as_ref()
                .is_some_and(|t| t.start_at_ms.is_none());
            self.view.scheduled = state
                .transition
                .as_ref()
                .is_some_and(|t| t.start_at_ms.is_some());
            if let Some(now) = self.clock.host_now(self.now()) {
                self.view.drift = state.position_at(now) - self.view.player.position_at(self.now());
                let peer = if self.view.role == Role::Host {
                    &state.guest
                } else {
                    &state.host
                };
                self.view.peer_drift = peer
                    .playback
                    .as_ref()
                    .filter(|p| (0.0..=350.0).contains(&(now - p.host_ms)))
                    .map(|p| self.view.player.position_at(self.now()) - p.position_at(now));
            }
        }
        self.view.status = match self.view.role {
            Role::Local => "Reprodução local".into(),
            Role::Host if !self.view.connected => "Sala aberta, aguardando parceiro".into(),
            Role::Guest if self.view.waiting_approval => "Aguardando aprovação do anfitrião".into(),
            Role::Guest if !self.view.connected => {
                "Conectando ou reconectando, reprodução pausada".into()
            }
            _ if self.view.loading => "Verificando o arquivo".into(),
            _ if matches!(
                self.view.stream.phase,
                crate::stream::StreamPhase::Offering
                    | crate::stream::StreamPhase::Connecting
                    | crate::stream::StreamPhase::Probing
            ) =>
            {
                "Conectando streaming P2P direto".into()
            }
            _ if self.view.stream.phase == crate::stream::StreamPhase::Reconnecting
                || self
                    .view
                    .peer_stream
                    .as_ref()
                    .is_some_and(|s| s.reconnecting) =>
            {
                "Reconectando streaming P2P".into()
            }
            _ if self.view.buffering || self.view.peer_buffering || self.view.waiting_buffer => {
                "Carregando vídeo".into()
            }
            _ if self.view.preparing => "Preparando os dois players na mesma posição".into(),
            _ if self.view.scheduled => "Início agendado, aguardando o horário comum".into(),
            _ if !self.view.matched && self.presence.media.is_some() && self.view.peer_ready => {
                "Os arquivos são diferentes".into()
            }
            _ if !self.view.ready => "Aguardando os players e a medição da conexão".into(),
            _ if self.view.player.paused => "Sessão pausada".into(),
            _ => "Assistindo juntos".into(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{player, protocol::Verification, session::Session};
    use std::time::Instant;

    #[test]
    fn both_roles_receive_peer_metadata_and_disconnect_clears_it() {
        for role in [Role::Host, Role::Guest] {
            let (player, _commands) = player::Player::test_channel();
            let mut worker = Worker::new(player, Instant::now());
            worker.view.role = role;
            worker.view.connected = true;
            let mut state = Session::default().state;
            state.host.file_name = Some("host.mkv".into());
            state.guest.file_name = Some("guest.mp4".into());
            state.host.loading = true;
            state.guest.loading = true;
            state.host.verification = Some(Verification {
                bytes: 10,
                total: 100,
            });
            state.guest.verification = Some(Verification {
                bytes: 30,
                total: 100,
            });
            worker.snapshot = Some(state);
            worker.update_view();
            assert_eq!(
                worker.view.peer_file.as_deref(),
                Some(if role == Role::Host {
                    "guest.mp4"
                } else {
                    "host.mkv"
                })
            );
            assert!(worker.view.peer_loading);
            assert_eq!(
                worker.view.peer_verification.unwrap().bytes,
                if role == Role::Host { 30 } else { 10 }
            );
            worker.disconnected();
            worker.update_view();
            assert!(worker.view.peer_file.is_none());
            assert!(worker.view.peer_verification.is_none());
            assert!(!worker.view.peer_loading);
        }
    }

    #[test]
    fn reconnect_keeps_watching_while_media_switch_exits_watching() {
        use crate::protocol::{Media, StreamProgress};
        let (player, _commands) = player::Player::test_channel();
        let mut worker = Worker::new(player, Instant::now());
        worker.view.role = Role::Host;
        worker.view.connected = true;
        let movie = Media {
            hash: "a".repeat(64),
            bytes: 1_000_000,
        };
        let other = Media {
            hash: "b".repeat(64),
            bytes: 2_000_000,
        };
        let mut playing = Session::default().state;
        playing.paused = false;
        playing.host.media = Some(movie.clone());
        playing.guest.media = Some(movie.clone());
        worker.apply_snapshot(playing.clone());
        assert!(worker.view.watching);

        worker.disconnected();
        assert!(worker.view.watching);

        let mut reconnect_initial = Session::default().state;
        reconnect_initial.revision = 2;
        reconnect_initial.paused = true;
        reconnect_initial.host.media = Some(movie.clone());
        reconnect_initial.guest.media = None;
        worker.view.connected = true;
        worker.apply_snapshot(reconnect_initial);
        assert!(worker.view.watching);

        let mut reconnect_restored = Session::default().state;
        reconnect_restored.revision = 3;
        reconnect_restored.paused = true;
        reconnect_restored.host.media = Some(movie.clone());
        reconnect_restored.guest.media = Some(movie.clone());
        reconnect_restored.guest.buffering = true;
        reconnect_restored.waiting_buffer = true;
        reconnect_restored.guest.stream_progress = Some(StreamProgress {
            cached_bytes: 250_000,
            total_bytes: 1_000_000,
            transfer_bps: 4_000_000,
            link_bps: Some(8_000_000),
            buffered_ahead_bytes: 100_000,
            reconnecting: false,
            complete: false,
            cached_ranges: vec![(0, 250)],
        });
        worker.apply_snapshot(reconnect_restored);
        assert!(worker.view.watching);
        worker.update_view();
        assert!(worker.view.peer_buffering);
        assert!(worker.view.waiting_buffer);
        assert_eq!(
            worker.view.peer_stream.as_ref().unwrap().cached_ranges,
            vec![(0, 250)]
        );

        let mut switched = Session::default().state;
        switched.revision = 4;
        switched.paused = true;
        switched.host.media = Some(movie);
        switched.guest.media = Some(other);
        worker.apply_snapshot(switched);
        assert!(!worker.view.watching);
    }

    #[tokio::test]
    async fn guest_stop_stream_clears_stale_media_and_presence() {
        use crate::{
            protocol::Media,
            stream::{ChunkIndex, cache::ChunkCache, http},
        };
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".cache/view-stop-stream-test");
        let _ = std::fs::remove_dir_all(&dir);
        let data = vec![7u8; 4096];
        let hash = blake3::hash(&data).to_hex().to_string();
        let chunk_hash = *blake3::hash(&data).as_bytes();
        let index = ChunkIndex::from_hashes(4096, 4096, vec![chunk_hash]);
        let cache = std::sync::Arc::new(
            ChunkCache::open(
                &dir,
                &hash,
                &index.digest,
                4096,
                4096,
                std::sync::Arc::new(index.hashes),
            )
            .unwrap(),
        );
        let proxy = std::sync::Arc::new(http::start(cache.clone(), "movie.mkv").await.unwrap());
        let (player, commands) = player::Player::test_channel();
        let mut worker = Worker::new(player, Instant::now());
        worker.view.role = Role::Guest;
        worker.view.connected = true;
        worker.view.watching = true;
        worker.view.file = Some("movie.mkv".into());
        worker.presence.media = Some(Media {
            hash: hash.clone(),
            bytes: 4096,
        });
        worker.presence.file_name = Some("movie.mkv".into());
        worker.presence.ready = true;
        worker.presence.streaming = true;
        worker.guest_stream = Some(super::super::stream::GuestSession {
            media: Media { hash, bytes: 4096 },
            hash_table_digest: index.digest,
            cache,
            _proxy: proxy,
        });

        worker.stop_stream(false);
        assert!(worker.view.file.is_none());
        assert!(!worker.view.watching);
        assert!(worker.presence.media.is_none());
        assert!(worker.presence.file_name.is_none());
        assert!(!worker.presence.ready);
        assert!(!worker.presence.streaming);
        assert!(worker.presence.stream_progress.is_none());
        let mut saw_stop = false;
        while let Ok(cmd) = commands.try_recv() {
            if matches!(cmd, player::Command::Stop) {
                saw_stop = true;
            }
        }
        assert!(saw_stop);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn host_transient_stream_timeout_does_not_send_stream_stop_and_queues_cooldown_requests() {
        use crate::{
            protocol::{Media, Message},
            stream::{ChunkIndex, StreamPhase},
        };
        let (player, _commands) = player::Player::test_channel();
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let mut worker = Worker::new(player, Instant::now());
        worker.view.role = Role::Host;
        worker.view.connected = true;
        worker.outgoing = Some(tx);
        worker.stream_id = 42;
        worker.view.stream.active = true;
        worker.view.stream.phase = StreamPhase::Offering;
        worker.stream_waiting_since = Some(Instant::now() - std::time::Duration::from_secs(20));

        worker.check_stream_timeout();
        assert!(matches!(worker.view.stream.phase, StreamPhase::Error(_)));
        assert!(
            rx.try_recv().is_err(),
            "host timeout must not send StreamStop that would kill guest playback"
        );

        let index = ChunkIndex::from_hashes(4096, 4096, vec![[1u8; 32]]);
        worker.local_media = Some((
            std::path::PathBuf::from("unused.mkv"),
            std::sync::Arc::new(index),
        ));
        worker.presence.media = Some(Media {
            hash: "a".repeat(64),
            bytes: 4096,
        });
        worker.presence.file_name = Some("movie.mkv".into());
        worker.last_stream_request = Some(Instant::now());
        worker.handle_stream_request();
        assert!(worker.pending_stream_request);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        let _ = Message::StreamRequest;
    }

    #[test]
    fn buffer_latch_measures_from_clear_and_escapes_on_stream_error_or_play() {
        use crate::stream::StreamPhase;
        let (player, _commands) = player::Player::test_channel();
        let mut worker = Worker::new(player, Instant::now());
        worker.view.role = Role::Guest;
        worker.view.connected = true;
        worker.view.player.loaded = true;
        worker.view.player.duration = 120.0;
        worker.view.player.position = 10.0;
        worker.view.player.buffering = true;
        worker.refresh_presence();
        assert!(worker.presence.buffering);

        worker.view.player.buffering = false;
        worker.refresh_presence();
        assert!(
            worker.presence.buffering,
            "latch must stay active right after paused-for-cache clears"
        );

        worker.control(crate::protocol::Control::Play);
        worker.refresh_presence();
        assert!(
            !worker.presence.buffering,
            "manual Play must act as an escape hatch when mpv is no longer buffering"
        );

        worker.view.player.buffering = true;
        worker.refresh_presence();
        worker.view.player.buffering = false;
        worker.view.stream.phase = StreamPhase::Error("offline".into());
        worker.refresh_presence();
        assert!(
            !worker.presence.buffering,
            "stream error must not leave presence.buffering stuck"
        );
    }
}

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
            }
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
}

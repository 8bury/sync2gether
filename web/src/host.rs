use crate::{
    guest::{CORRECT_MS, HEARTBEAT_MS, PlayerStatus},
    host_link::{Event as LinkEvent, HostLink, SocketCommand},
};
use serde::Serialize;
use sync2gether_core::{
    host::{Host, HostNotice, HostOutput, PlayerCommand, PlayerStatus as CorePlayerStatus},
    protocol::{Control, Media, Message, Side, Verification},
};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    ConnectControl { at: f64 },
    CloseControl,
    OpenAccept { id: u64 },
    CloseAccept { id: u64 },
    Prepare { token: u64, position: f64 },
    Schedule { token: u64, at: f64 },
    Pause { paused: bool },
    Seek { position: f64 },
    Speed { speed: f64 },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct View {
    pub role: &'static str,
    pub code: Option<String>,
    pub connected: bool,
    pub relay_offline: bool,
    pub ended: bool,
    pub peer: Option<String>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub clock_ready: bool,
    pub rtt_ms: f64,
    pub host_media: Option<Media>,
    pub host_file_name: Option<String>,
    pub host_duration: f64,
    pub host_ready: bool,
    pub host_loading: bool,
    pub host_buffering: bool,
    pub host_blocked: bool,
    pub host_web_stream: bool,
    pub host_web_remux: bool,
    pub guest_media: Option<Media>,
    pub guest_file_name: Option<String>,
    pub guest_ready: bool,
    pub guest_loading: bool,
    pub guest_buffering: bool,
    pub guest_blocked: bool,
    pub guest_clock_ready: bool,
    pub guest_position: Option<f64>,
    pub matched: bool,
    pub mismatch: bool,
    pub paused: bool,
    pub paused_by: Option<Side>,
    pub playing: bool,
    pub position: f64,
    pub buffered_until: f64,
    pub loaded: bool,
    pub watching: bool,
    pub ready: bool,
    pub blocked: bool,
    pub buffering: bool,
    pub waiting_buffer: bool,
    pub preparing: bool,
    pub scheduled: bool,
    pub starting: bool,
    pub peer_left: bool,
    pub scheduled_start_ms: Option<f64>,
    pub drift_ms: Option<f64>,
    pub generation: u64,
}

fn to_core_player_status(status: &PlayerStatus) -> CorePlayerStatus {
    CorePlayerStatus {
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
        cache_time: status.buffered_until,
    }
}

pub struct BrowserHost {
    link: HostLink,
    host: Host,
    player: PlayerStatus,
    generation: u64,
    peer: Option<String>,
    error: Option<String>,
    notice: Option<String>,
    relay_offline: bool,
    last_heartbeat: f64,
    last_correct: f64,
    now: f64,
    commands: Vec<Command>,
}

impl BrowserHost {
    pub fn new(name: &str) -> Self {
        Self::from_link(HostLink::new(name))
    }

    pub fn restore(
        name: &str,
        secret: String,
        words: [u8; 2],
        room: Option<String>,
        resume_key: Option<[u8; 32]>,
    ) -> Self {
        Self::from_link(HostLink::with_identity(
            name, secret, words, room, resume_key,
        ))
    }

    fn from_link(link: HostLink) -> Self {
        Self {
            link,
            host: Host::new(0.0, 0.0),
            player: PlayerStatus::default(),
            generation: 0,
            peer: None,
            error: None,
            notice: None,
            relay_offline: false,
            last_heartbeat: f64::NEG_INFINITY,
            last_correct: f64::NEG_INFINITY,
            now: 0.0,
            commands: Vec::new(),
        }
    }

    pub fn start(&mut self, now: f64) {
        self.now = now;
        self.link.start(now);
        self.drain_link_commands();
    }

    pub fn code(&self) -> Option<String> {
        self.link.code()
    }

    pub fn room(&self) -> Option<&str> {
        self.link.room()
    }

    pub fn secret(&self) -> &str {
        self.link.secret()
    }

    pub fn words(&self) -> [u8; 2] {
        self.link.words()
    }

    pub fn resume_key(&self) -> Option<[u8; 32]> {
        self.link.resume_key()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_connected(&self) -> bool {
        self.host.is_connected()
    }

    pub fn take_control_output(&mut self) -> Vec<u8> {
        self.link.take_control_output()
    }

    pub fn take_accept_output(&mut self, id: u64) -> Vec<u8> {
        self.link.take_accept_output(id)
    }

    pub fn take_commands(&mut self) -> Vec<Command> {
        self.drain_link_commands();
        std::mem::take(&mut self.commands)
    }

    pub fn control_open(&mut self, now: f64) {
        self.now = now;
        self.link.control_open(now);
        self.drain_link_commands();
    }

    pub fn control_data(&mut self, bytes: &[u8], now: f64) {
        self.now = now;
        for event in self.link.control_data(bytes, now) {
            self.link_event(event);
        }
        self.drain_link_commands();
    }

    pub fn control_closed(&mut self, now: f64) {
        self.now = now;
        if let Some(event) = self.link.control_closed(now) {
            self.link_event(event);
        }
        self.drain_link_commands();
    }

    pub fn accept_open(&mut self, id: u64, now: f64) {
        self.now = now;
        self.link.accept_open(id, now);
        self.drain_link_commands();
    }

    pub fn accept_data(&mut self, id: u64, bytes: &[u8], now: f64) {
        self.now = now;
        for event in self.link.accept_data(id, bytes, now) {
            self.link_event(event);
        }
        self.tick(now);
    }

    pub fn accept_closed(&mut self, id: u64, now: f64) {
        self.now = now;
        if let Some(event) = self.link.accept_closed(id) {
            self.link_event(event);
        }
        self.drain_link_commands();
    }

    pub fn tick(&mut self, now: f64) {
        self.now = now;
        for event in self.link.tick(now) {
            self.link_event(event);
        }
        self.sync_inputs();
        if self.host.is_connected() && now - self.last_heartbeat >= HEARTBEAT_MS {
            self.last_heartbeat = now;
            self.host.heartbeat(now);
            self.flush_host();
        }
        self.host.progress(now);
        self.flush_host();
        if now - self.last_correct >= CORRECT_MS {
            self.last_correct = now;
            self.host.correct(now);
            self.flush_host();
        }
        self.drain_link_commands();
    }

    pub fn leave(&mut self, now: f64) {
        self.now = now;
        self.link.leave();
        self.host.on_disconnected(now);
        self.flush_host();
        self.commands.push(Command::Pause { paused: true });
        self.drain_link_commands();
    }

    pub fn control(&mut self, control: Control, now: f64) {
        self.now = now;
        self.sync_inputs();
        self.host.control(control, Side::Host, now);
        self.flush_host();
        self.drain_link_commands();
    }

    pub fn set_loading(
        &mut self,
        file_name: Option<String>,
        verification: Option<Verification>,
        now: f64,
    ) {
        self.now = now;
        if !self.host.presence().loading {
            self.generation += 1;
            self.host.begin_loading(self.generation, now);
        }
        let presence = self.host.presence_mut();
        presence.file_name =
            file_name.filter(|name| sync2gether_core::protocol::valid_file_name(name));
        presence.verification = verification.filter(Verification::valid);
        presence.web_stream = false;
        presence.web_remux = false;
        if self.host.is_connected() {
            self.host.publish(now);
        }
        self.flush_host();
        self.drain_link_commands();
    }

    pub fn set_media(&mut self, media: Option<Media>, file_name: Option<String>, now: f64) -> u64 {
        self.now = now;
        self.generation += 1;
        self.player = PlayerStatus::default();
        self.error = None;
        self.host.set_fatal_error(false);
        self.host.set_generation(self.generation);
        let valid_name = file_name.filter(|name| sync2gether_core::protocol::valid_file_name(name));
        if let Some(media) = media.filter(Media::valid) {
            self.host.set_media_loaded(media, now);
            self.host.presence_mut().file_name = valid_name;
        } else {
            let presence = self.host.presence_mut();
            presence.media = None;
            presence.file_name = None;
            presence.loading = false;
            presence.verification = None;
            presence.ready = false;
        }
        self.sync_inputs();
        if self.host.is_connected() {
            self.host.publish(now);
        }
        self.flush_host();
        self.drain_link_commands();
        self.generation
    }

    pub fn player_status(&mut self, status: PlayerStatus, now: f64) {
        self.now = now;
        if status.generation == self.generation {
            self.player = status.clone();
        }
        self.sync_inputs();
        self.host.player_status(to_core_player_status(&status), now);
        self.flush_host();
        self.drain_link_commands();
    }

    pub fn player_prepared(&mut self, token: u64, now: f64) {
        self.now = now;
        self.sync_inputs();
        self.host.player_prepared(token, now);
        self.flush_host();
        self.drain_link_commands();
    }

    pub fn player_started(&mut self, token: u64, at: f64, now: f64) {
        self.now = now;
        self.sync_inputs();
        self.host.player_started(token, at, now);
        self.flush_host();
        self.drain_link_commands();
    }

    pub fn player_missed(&mut self, token: u64, now: f64) {
        self.now = now;
        self.sync_inputs();
        self.host.player_missed(token, now);
        self.flush_host();
        self.drain_link_commands();
    }

    pub fn player_error(&mut self, error: String, now: f64) {
        self.now = now;
        self.error = Some(error);
        self.sync_inputs();
        self.host.player_error(now);
        self.flush_host();
        self.drain_link_commands();
    }

    pub fn view(&self) -> View {
        let session = self.host.session();
        let state = self.host.snapshot().unwrap_or(&session.state);
        let presence = self.host.presence();
        let guest = &state.guest;
        let connected = self.host.is_connected();
        let matched = connected && presence.media.is_some() && presence.media == guest.media;
        let mismatch = connected
            && presence.media.is_some()
            && guest.media.is_some()
            && presence.media != guest.media;
        let host_now = self.host.clock().host_now(self.now);
        let guest_position = guest
            .playback
            .as_ref()
            .zip(host_now)
            .filter(|(p, now)| (0.0..=350.0).contains(&(now - p.host_ms)))
            .map(|(p, now)| p.position_at(now));
        let drift_ms = guest_position
            .filter(|_| matched)
            .map(|gp| (self.player.position_at(self.now) - gp) * 1000.0);
        let ready = connected
            && matched
            && presence.ready
            && guest.ready
            && presence.clock_ready
            && guest.clock_ready
            && state.transition.is_none()
            && !presence.blocked
            && !guest.blocked
            && !presence.buffering
            && !guest.buffering;
        View {
            role: "host",
            code: self.link.code(),
            connected,
            relay_offline: self.relay_offline,
            ended: self.link.ended(),
            peer: self.peer.clone(),
            error: self.error.clone(),
            notice: self.notice.clone(),
            clock_ready: guest.clock_ready,
            rtt_ms: guest.rtt_ms,
            host_media: presence.media.clone(),
            host_file_name: presence.file_name.clone(),
            host_duration: presence.duration,
            host_ready: presence.ready,
            host_loading: presence.loading,
            host_buffering: presence.buffering,
            host_blocked: presence.blocked,
            host_web_stream: false,
            host_web_remux: false,
            guest_media: connected.then(|| guest.media.clone()).flatten(),
            guest_file_name: connected.then(|| guest.file_name.clone()).flatten(),
            guest_ready: connected && guest.ready,
            guest_loading: connected && guest.loading,
            guest_buffering: connected && guest.buffering,
            guest_blocked: connected && guest.blocked,
            guest_clock_ready: connected && guest.clock_ready,
            guest_position: connected.then_some(guest_position).flatten(),
            matched,
            mismatch,
            paused: state.paused,
            paused_by: state.paused_by.filter(|_| state.paused),
            playing: connected && host_now.is_some_and(|now| state.playing_at(now)),
            position: host_now
                .filter(|_| connected)
                .map_or(self.player.position, |now| state.position_at(now)),
            buffered_until: self.player.buffered_until,
            loaded: self.player.loaded,
            watching: self.host.watching(),
            ready,
            blocked: presence.blocked,
            buffering: presence.buffering,
            waiting_buffer: connected && state.waiting_buffer,
            preparing: state
                .transition
                .as_ref()
                .is_some_and(|t| t.start_at_ms.is_none()),
            scheduled: state
                .transition
                .as_ref()
                .is_some_and(|t| t.start_at_ms.is_some()),
            starting: state.transition.as_ref().is_some_and(|t| t.resume),
            peer_left: self.host.peer_left(),
            scheduled_start_ms: state
                .transition
                .as_ref()
                .and_then(|t| t.start_at_ms)
                .and(self.host.scheduled_start_ms()),
            drift_ms,
            generation: self.generation,
        }
    }

    fn sync_inputs(&mut self) {
        self.host.set_generation(self.generation);
        let presence = self.host.presence_mut();
        presence.streaming = false;
        presence.web_stream = false;
        presence.web_remux = false;
    }

    fn link_event(&mut self, event: LinkEvent) {
        match event {
            LinkEvent::RoomCode(_) => {
                self.relay_offline = false;
            }
            LinkEvent::RelayOffline => {
                self.relay_offline = true;
            }
            LinkEvent::WrongCode => {
                self.notice = Some("Alguém tentou entrar com o código errado.".into());
            }
            LinkEvent::Connected { peer } => {
                self.peer = Some(peer);
                self.last_heartbeat = f64::NEG_INFINITY;
                self.sync_inputs();
                self.host.on_connected(self.now);
                self.flush_host();
            }
            LinkEvent::Message(message, received_ms) => {
                self.sync_inputs();
                self.host.receive_message(*message, received_ms, self.now);
                self.flush_host();
            }
            LinkEvent::Disconnected => {
                self.host.on_disconnected(self.now);
                self.flush_host();
            }
            LinkEvent::PeerLeft => {
                self.host.on_peer_left(self.now);
                self.flush_host();
            }
            LinkEvent::Fatal(error) => {
                self.error = Some(error);
                self.host.on_disconnected(self.now);
                self.flush_host();
                self.commands.push(Command::Pause { paused: true });
            }
        }
    }

    fn flush_host(&mut self) {
        let outputs = self.host.take_outputs();
        for output in outputs {
            match output {
                HostOutput::Send(message) => {
                    if self.link.send(&message).is_err() {
                        self.link.drop_guest();
                        self.host.on_disconnected(self.now);
                    }
                }
                HostOutput::Player(command) => self.commands.push(match command {
                    PlayerCommand::Prepare { token, position } => {
                        Command::Prepare { token, position }
                    }
                    PlayerCommand::Schedule { token, at_ms } => {
                        Command::Schedule { token, at: at_ms }
                    }
                    PlayerCommand::Pause { paused } => Command::Pause { paused },
                    PlayerCommand::Seek { position } => Command::Seek { position },
                    PlayerCommand::Speed { speed } => Command::Speed { speed },
                }),
                HostOutput::Notice(notice) => {
                    self.notice = Some(match notice {
                        HostNotice::Reconnected => {
                            "Conexão restabelecida. Aperte reproduzir para continuar.".into()
                        }
                        HostNotice::StartCancelled => {
                            "Início cancelado. A sessão está pausada; tente reproduzir novamente."
                                .into()
                        }
                        HostNotice::StartTimeout => {
                            "Não foi possível começar juntos. A sessão está pausada; tente novamente."
                                .into()
                        }
                    });
                }
                HostOutput::ClearNotice => self.notice = None,
                HostOutput::Disconnect => {
                    self.link.drop_guest();
                    self.host.on_disconnected(self.now);
                }
                HostOutput::WebStreamOffer(offer) => {
                    let reply = Message::StreamStop { id: offer.id };
                    if self.host.can_send(&reply) {
                        let _ = self.link.send(&reply);
                    }
                }
                HostOutput::SubtitleRequest { track } => {
                    let reply = Message::SubtitleFailed { track };
                    if self.host.can_send(&reply) {
                        let _ = self.link.send(&reply);
                    }
                }
                HostOutput::WebGuestJoined
                | HostOutput::StreamRequest
                | HostOutput::StreamAnswer(_)
                | HostOutput::StreamStop { .. } => {}
            }
        }
    }

    fn drain_link_commands(&mut self) {
        for command in self.link.take_commands() {
            self.commands.push(match command {
                SocketCommand::ConnectControl { at } => Command::ConnectControl { at },
                SocketCommand::CloseControl => Command::CloseControl,
                SocketCommand::OpenAccept { id } => Command::OpenAccept { id },
                SocketCommand::CloseAccept { id } => Command::CloseAccept { id },
            });
        }
    }
}

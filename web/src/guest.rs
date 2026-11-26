use crate::link::{self, Event, Link};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use sync2gether_core::{
    code::Code,
    protocol::{
        Control, Media, Message, Playback, Presence, Snapshot, StreamProgress, Verification,
        WebStreamAnswer, WebStreamOffer,
    },
    sync::{Clock, Controller, Correction},
};

pub const HEARTBEAT_MS: f64 = 200.0;
pub const CORRECT_MS: f64 = 20.0;
const BUFFER_RESUME_SECS: f64 = 3.0;
const BUFFER_MIN_CLEAR_MS: f64 = 1200.0;
const STATUS_FRESH_MS: f64 = 1500.0;
const CORRECT_FRESH_MS: f64 = 350.0;

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PlayerStatus {
    pub generation: u64,
    pub measured_ms: f64,
    pub position: f64,
    pub duration: f64,
    pub speed: f64,
    pub paused: bool,
    pub seeking: bool,
    pub loaded: bool,
    pub buffering: bool,
    pub blocked: bool,
    pub ended: bool,
    pub buffered_until: f64,
}
impl PlayerStatus {
    pub fn position_at(&self, now: f64) -> f64 {
        (self.position
            + if self.paused || self.seeking || self.blocked {
                0.0
            } else {
                ((now - self.measured_ms) / 1000.0).max(0.0) * self.speed
            })
        .min(self.duration)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    Connect { at: f64 },
    Close,
    Prepare { token: u64, position: f64 },
    Schedule { token: u64, at: f64 },
    Pause { paused: bool },
    Seek { position: f64 },
    Speed { speed: f64 },
    StreamAnswer { answer: Box<WebStreamAnswer> },
    StreamStopped { id: u64 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum BufferLatch {
    #[default]
    Idle,
    Active,
    Recovering(f64),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct View {
    pub code: String,
    pub connected: bool,
    pub reconnecting: bool,
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
    pub host_web_stream: bool,
    pub guest_media: Option<Media>,
    pub matched: bool,
    pub mismatch: bool,
    pub paused: bool,
    pub playing: bool,
    pub position: f64,
    pub watching: bool,
    pub ready: bool,
    pub buffering: bool,
    pub scheduled_start_ms: Option<f64>,
    pub drift_ms: Option<f64>,
    pub generation: u64,
}

pub struct Guest {
    link: Link,
    presence: Presence,
    clock: Clock,
    controller: Controller,
    snapshot: Option<Snapshot>,
    pings: VecDeque<f64>,
    player: PlayerStatus,
    generation: u64,
    requested: Option<u64>,
    prepared: Option<u64>,
    scheduled: Option<u64>,
    failed: Option<u64>,
    last_pause: Option<u64>,
    player_operation: Option<(u64, u64)>,
    player_token: u64,
    speed: f64,
    buffer: BufferLatch,
    watching: bool,
    watching_media: Option<Media>,
    connected: bool,
    reconnecting: bool,
    peer: Option<String>,
    notice: Option<String>,
    error: Option<String>,
    scheduled_start_ms: Option<f64>,
    last_heartbeat: f64,
    last_correct: f64,
    now: f64,
    commands: Vec<Command>,
    stream: Option<(u64, Option<Media>)>,
}

impl Guest {
    pub fn new(code: &str, name: &str) -> Result<Self, String> {
        let code = Code::parse(code)
            .ok_or_else(|| "Link inválido. Peça um link novo para quem criou a sala.".to_owned())?;
        Ok(Self {
            link: Link::new(code, name),
            presence: Presence {
                web_guest: true,
                ..Presence::default()
            },
            clock: Clock::default(),
            controller: Controller::default(),
            snapshot: None,
            pings: VecDeque::new(),
            player: PlayerStatus::default(),
            generation: 0,
            requested: None,
            prepared: None,
            scheduled: None,
            failed: None,
            last_pause: None,
            player_operation: None,
            player_token: 0,
            speed: 1.0,
            buffer: BufferLatch::Idle,
            watching: false,
            watching_media: None,
            connected: false,
            reconnecting: false,
            peer: None,
            notice: None,
            error: None,
            scheduled_start_ms: None,
            last_heartbeat: f64::NEG_INFINITY,
            last_correct: f64::NEG_INFINITY,
            now: 0.0,
            commands: Vec::new(),
            stream: None,
        })
    }

    pub fn start(&mut self, now: f64) {
        self.now = now;
        self.commands.push(Command::Connect { at: now });
    }

    pub fn resume_key(&self) -> Option<[u8; 32]> {
        self.link.key()
    }
    pub fn resume_with(&mut self, key: [u8; 32]) {
        self.link.resume_with(key);
    }

    pub fn take_output(&mut self) -> Vec<u8> {
        self.link.take_output()
    }
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    pub fn socket_open(&mut self, now: f64) {
        self.now = now;
        if self.link.ended() {
            self.commands.push(Command::Close);
            return;
        }
        if self.link.open(now).is_err() {
            self.commands.push(Command::Close);
        }
    }

    pub fn socket_data(&mut self, bytes: &[u8], now: f64) {
        self.now = now;
        for event in self.link.receive(bytes, now) {
            self.link_event(event);
        }
        self.tick(now);
    }

    pub fn socket_closed(&mut self, now: f64) {
        self.now = now;
        if let Some(event) = self.link.closed(now) {
            self.link_event(event);
        }
    }

    pub fn tick(&mut self, now: f64) {
        self.now = now;
        if let Some(event) = self.link.tick(now) {
            self.link_event(event);
        }
        if self.connected && now - self.last_heartbeat >= HEARTBEAT_MS {
            self.last_heartbeat = now;
            self.refresh_presence();
            self.send(Message::Presence(Box::new(self.presence.clone())));
            self.pings.push_back(now);
            while self.pings.len() > 16 {
                self.pings.pop_front();
            }
            self.send(Message::Ping { sent_ms: now });
        }
        self.progress();
        if now - self.last_correct >= CORRECT_MS {
            self.last_correct = now;
            self.correct();
        }
    }

    pub fn leave(&mut self, now: f64) {
        self.now = now;
        if self.link.is_open() {
            self.send(Message::Bye);
        }
        self.link.leave();
        self.connected = false;
        self.commands.push(Command::Pause { paused: true });
        self.commands.push(Command::Close);
    }

    pub fn control(&mut self, control: Control, now: f64) {
        self.now = now;
        if !control.valid() {
            return;
        }
        self.notice = None;
        if matches!(control, Control::Play)
            && !self.player.buffering
            && matches!(self.buffer, BufferLatch::Recovering(_))
        {
            self.buffer = BufferLatch::Idle;
        }
        if self.connected {
            if matches!(control, Control::Play) {
                self.refresh_presence();
                self.send(Message::Presence(Box::new(self.presence.clone())));
            }
            self.send(Message::Request(control));
        }
    }

    pub fn set_media(&mut self, media: Option<Media>, file_name: Option<String>) -> u64 {
        self.generation += 1;
        self.presence.media = media;
        self.presence.file_name =
            file_name.filter(|name| sync2gether_core::protocol::valid_file_name(name));
        self.presence.loading = false;
        self.presence.verification = None;
        self.player = PlayerStatus::default();
        self.generation
    }
    pub fn set_loading(&mut self, file_name: Option<String>, verification: Option<Verification>) {
        self.presence.loading = true;
        self.presence.media = None;
        self.presence.file_name =
            file_name.filter(|name| sync2gether_core::protocol::valid_file_name(name));
        self.presence.verification = verification.filter(Verification::valid);
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn player_status(&mut self, status: PlayerStatus, now: f64) {
        self.now = now;
        if status.generation != self.generation {
            return;
        }
        self.player = status;
        self.refresh_presence();
        if self.presence.blocked
            && let Some(id) = self.scheduled
        {
            self.operation_failed(id);
        }
        self.progress();
    }

    pub fn player_prepared(&mut self, token: u64, now: f64) {
        self.now = now;
        if let Some((current, wire)) = self.player_operation
            && current == token
            && self.requested == Some(wire)
        {
            self.prepared = Some(wire);
            self.send(Message::Prepared { id: wire });
        }
        self.progress();
    }

    pub fn player_started(&mut self, token: u64, at: f64, now: f64) {
        self.now = now;
        if let Some((current, wire)) = self.player_operation
            && current == token
            && self.scheduled == Some(wire)
        {
            self.controller.reset(now.max(at));
        }
        self.progress();
    }

    pub fn player_missed(&mut self, token: u64, now: f64) {
        self.now = now;
        if let Some((current, wire)) = self.player_operation
            && current == token
        {
            self.operation_failed(wire);
        }
        self.progress();
    }

    pub fn player_error(&mut self, error: String, now: f64) {
        self.now = now;
        self.error = Some(error);
        self.presence.ready = false;
        self.presence.blocked = true;
        self.control(Control::Pause, now);
        self.progress();
    }

    pub fn stream_offer(&mut self, id: u64, sdp: String, now: f64) -> bool {
        self.now = now;
        let offer = WebStreamOffer { id, sdp };
        let host_streams = self
            .snapshot
            .as_ref()
            .is_some_and(|s| s.host.web_stream && s.host.media.is_some());
        if !self.connected || !host_streams || !offer.valid() {
            return false;
        }
        self.stream = Some((id, None));
        self.send(Message::WebStreamOffer(Box::new(offer)));
        true
    }

    pub fn stream_started(&mut self, media: Media, file_name: String) -> u64 {
        let generation = self.set_media(Some(media.clone()), Some(file_name));
        if let Some((_, streamed)) = &mut self.stream {
            *streamed = Some(media);
        }
        self.presence.streaming = true;
        generation
    }

    pub fn stream_progress(&mut self, progress: StreamProgress) {
        if self.stream.is_some() && progress.valid() {
            self.presence.stream_progress = Some(progress);
        }
    }

    pub fn stop_stream(&mut self, now: f64) {
        self.now = now;
        if let Some((id, _)) = self.stream.take() {
            self.send(Message::StreamStop { id });
            self.end_stream();
        }
    }

    fn end_stream(&mut self) {
        let was_streaming = self.presence.streaming;
        self.presence.streaming = false;
        self.presence.stream_progress = None;
        if was_streaming {
            self.set_media(None, None);
        }
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }
    pub fn presence(&self) -> &Presence {
        &self.presence
    }
    pub fn is_connected(&self) -> bool {
        self.connected
    }
    pub fn view(&self) -> View {
        let state = self.snapshot.as_ref();
        let host = state.map(|s| &s.host);
        let host_now = self.clock.host_now(self.now);
        let matched =
            state.is_some_and(|s| s.host.media.is_some() && s.host.media == s.guest.media);
        View {
            code: self.link.code().to_string(),
            connected: self.connected,
            reconnecting: self.reconnecting,
            ended: self.link.ended(),
            peer: self.peer.clone(),
            error: self.error.clone(),
            notice: self.notice.clone(),
            clock_ready: self.clock.ready(self.now),
            rtt_ms: self.clock.rtt_ms,
            host_media: host.and_then(|h| h.media.clone()),
            host_file_name: host.and_then(|h| h.file_name.clone()),
            host_duration: host.map_or(0.0, |h| h.duration),
            host_ready: host.is_some_and(|h| h.ready),
            host_loading: host.is_some_and(|h| h.loading),
            host_web_stream: host.is_some_and(|h| h.web_stream),
            guest_media: self.presence.media.clone(),
            matched,
            mismatch: host.is_some_and(|h| {
                h.media.is_some() && self.presence.media.is_some() && h.media != self.presence.media
            }),
            paused: state.is_none_or(|s| s.paused),
            playing: state
                .zip(host_now)
                .is_some_and(|(s, now)| s.playing_at(now)),
            position: state
                .zip(host_now)
                .map_or(self.player.position, |(s, now)| s.position_at(now)),
            watching: self.watching,
            ready: self.presence.ready,
            buffering: self.presence.buffering,
            scheduled_start_ms: self.scheduled_start_ms,
            drift_ms: state
                .zip(host_now)
                .filter(|_| matched)
                .map(|(s, now)| (self.player.position_at(self.now) - s.position_at(now)) * 1000.0),
            generation: self.generation,
        }
    }

    fn link_event(&mut self, event: Event) {
        match event {
            Event::Connected { peer } => {
                if self.watching {
                    self.notice =
                        Some("Conexão restabelecida. Aperte reproduzir para continuar.".into());
                }
                self.disconnected();
                self.connected = true;
                self.reconnecting = false;
                self.error = None;
                self.peer = Some(peer);
                self.last_heartbeat = f64::NEG_INFINITY;
                self.refresh_presence();
                self.send(Message::Presence(Box::new(self.presence.clone())));
            }
            Event::Message(message) => self.message(*message),
            Event::Lost => {
                self.disconnected();
                self.reconnecting = true;
                self.commands.push(Command::Close);
                self.commands.push(Command::Connect {
                    at: self.now + link::RETRY_MS,
                });
            }
            Event::Retry { at } => {
                self.commands.push(Command::Close);
                self.commands.push(Command::Connect { at });
            }
            Event::PeerLeft => {
                self.disconnected();
                self.commands.push(Command::Pause { paused: true });
                self.commands.push(Command::Close);
                self.notice = Some(match &self.peer {
                    Some(peer) => format!("{peer} encerrou a sala."),
                    None => "O anfitrião encerrou a sala.".into(),
                });
            }
            Event::Fatal(error) => {
                self.disconnected();
                self.reconnecting = false;
                self.error = Some(error);
                self.commands.push(Command::Pause { paused: true });
                self.commands.push(Command::Close);
            }
            Event::Closed => self.disconnected(),
        }
    }

    fn message(&mut self, message: Message) {
        match message {
            Message::State(state) if state.valid() => {
                if self.snapshot.as_ref().is_none_or(|old| {
                    state.revision >= old.revision && state.host_ms >= old.host_ms
                }) {
                    self.apply_snapshot(*state);
                }
            }
            Message::WebStreamAnswer(answer) if answer.valid() => {
                if self.stream.as_ref().is_some_and(|(id, _)| *id == answer.id) {
                    self.commands.push(Command::StreamAnswer { answer });
                }
            }
            Message::StreamStop { id } => {
                if self
                    .stream
                    .as_ref()
                    .is_some_and(|(current, _)| *current == id)
                {
                    self.stream = None;
                    self.end_stream();
                    self.commands.push(Command::StreamStopped { id });
                }
            }
            Message::Pong {
                sent_ms,
                received_ms,
                sent_host_ms,
            } => {
                if let Some(index) = self
                    .pings
                    .iter()
                    .position(|sent| (*sent - sent_ms).abs() < 0.001)
                {
                    self.pings.remove(index);
                    self.clock
                        .sample(sent_ms, received_ms, sent_host_ms, self.now);
                    self.refresh_presence();
                }
            }
            _ => {}
        }
    }

    fn send(&mut self, message: Message) {
        if self.connected && self.link.send(&message).is_err() {
            self.link_event(Event::Lost);
        }
    }

    fn command(&mut self, command: Command) {
        self.commands.push(command);
    }

    fn set_speed(&mut self, speed: f64) {
        if (self.speed - speed).abs() > 0.0001 {
            self.command(Command::Speed { speed });
            self.speed = speed;
        }
    }

    fn reset_operation(&mut self) {
        self.requested = None;
        self.player_operation = None;
        self.prepared = None;
        self.scheduled = None;
        self.last_pause = None;
        self.controller.reset(self.now);
        self.command(Command::Pause { paused: true });
        self.set_speed(1.0);
    }

    fn disconnected(&mut self) {
        self.connected = false;
        self.clock = Clock::default();
        self.snapshot = None;
        self.pings.clear();
        self.reset_operation();
        self.failed = None;
        self.scheduled_start_ms = None;
    }

    fn operation_failed(&mut self, id: u64) {
        if self.requested != Some(id) && self.scheduled != Some(id) {
            return;
        }
        self.reset_operation();
        self.failed = Some(id);
        self.notice =
            Some("Início cancelado. A sessão está pausada; tente reproduzir novamente.".into());
        self.send(Message::Failed { id });
    }

    fn refresh_presence(&mut self) {
        let p = &self.player;
        let now = self.now;
        if !p.loaded || p.ended {
            self.buffer = BufferLatch::Idle;
        } else if p.buffering {
            self.buffer = BufferLatch::Active;
        } else {
            if matches!(self.buffer, BufferLatch::Active) {
                self.buffer = BufferLatch::Recovering(now);
            }
            if let BufferLatch::Recovering(cleared_at) = self.buffer {
                let ahead = (p.buffered_until - p.position).max(0.0);
                let remaining = if p.duration > 0.0 {
                    (p.duration - p.position - 0.5).max(0.0)
                } else {
                    BUFFER_RESUME_SECS
                };
                let target = BUFFER_RESUME_SECS.min(remaining.max(0.5)).min(2.0);
                if ahead >= target || now - cleared_at >= BUFFER_MIN_CLEAR_MS {
                    self.buffer = BufferLatch::Idle;
                }
            }
        }
        let buffering = p.loaded && !p.ended && !matches!(self.buffer, BufferLatch::Idle);
        let presence = &mut self.presence;
        presence.buffering = buffering;
        presence.ready = p.loaded
            && p.generation == self.generation
            && presence.media.is_some()
            && now - p.measured_ms < STATUS_FRESH_MS;
        presence.blocked = p.blocked || buffering;
        presence.ended = p.ended;
        presence.duration = if p.duration.is_finite() {
            p.duration.max(0.0)
        } else {
            0.0
        };
        presence.clock_ready = self.clock.ready(now);
        presence.rtt_ms = self.clock.rtt_ms.clamp(0.0, 3000.0);
        presence.uncertainty_ms = self.clock.uncertainty_ms.clamp(0.0, 3000.0);
        presence.web_guest = true;
        presence.playback = self.clock.host_now(p.measured_ms).map(|host_ms| Playback {
            position: p.position.max(0.0),
            host_ms,
            speed: p.speed.clamp(0.5, 2.0),
            paused: p.paused || presence.blocked,
            seeking: p.seeking,
        });
    }

    fn apply_snapshot(&mut self, state: Snapshot) {
        if let Some((id, Some(streamed))) = &self.stream
            && state
                .host
                .media
                .as_ref()
                .is_some_and(|host| host != streamed)
        {
            let id = *id;
            self.stop_stream(self.now);
            self.commands.push(Command::StreamStopped { id });
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
            || (self.watching && (state.host.loading || state.guest.loading))
        {
            self.watching = false;
            self.watching_media = None;
        }
        if !state.paused || state.transition.as_ref().is_some_and(|t| t.resume) {
            self.watching = true;
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
                self.notice = None;
                self.command(Command::Prepare {
                    token: self.player_token,
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
                    let Some(host_now) = self.clock.host_now(self.now) else {
                        self.operation_failed(t.id);
                        return;
                    };
                    if start - host_now < 50.0 {
                        self.operation_failed(t.id);
                        return;
                    }
                    let token = self.player_operation.map_or(0, |(token, _)| token);
                    self.scheduled = Some(t.id);
                    self.scheduled_start_ms = Some(start);
                    self.command(Command::Schedule {
                        token,
                        at: self.now + (start - host_now),
                    });
                    self.send(Message::Armed { id: t.id });
                }
            }
        } else if state.paused && self.last_pause != Some(state.revision) {
            self.reset_operation();
            self.last_pause = Some(state.revision);
        }
    }

    fn progress(&mut self) {
        self.refresh_presence();
        if (!self.clock.ready(self.now) || !self.presence.ready || self.presence.blocked)
            && let Some(id) = self.scheduled
        {
            self.operation_failed(id);
        }
    }

    fn correct(&mut self) {
        let Some(state) = self.snapshot.clone() else {
            return;
        };
        if state.transition.is_some()
            || !self.presence.ready
            || self.player.seeking
            || self.now - self.player.measured_ms > CORRECT_FRESH_MS
        {
            self.set_speed(1.0);
            return;
        }
        let Some(now) = self.clock.host_now(self.now) else {
            self.set_speed(1.0);
            return;
        };
        let matched = state.host.media.is_some() && state.host.media == state.guest.media;
        if !matched {
            self.set_speed(1.0);
            return;
        }
        let actual = self.player.position_at(self.now);
        let target = state.position_at(now);
        match self
            .controller
            .update(actual, target, state.paused, self.now)
        {
            Correction::Seek(p) => {
                self.command(Command::Seek {
                    position: p.min(self.player.duration),
                });
                self.set_speed(1.0);
            }
            Correction::Speed(speed) => self.set_speed(speed),
            Correction::None => self.set_speed(1.0),
        }
    }
}

use crate::{
    protocol::{
        Control, Media, Message, Playback, Presence, Side, Snapshot, StreamAnswer, WebStreamOffer,
    },
    session::Session,
    sync::{Clock, Controller, Correction},
};
use serde::{Deserialize, Serialize};

pub const STATUS_FRESH_MS: f64 = 1500.0;
pub const CORRECT_FRESH_MS: f64 = 350.0;
pub const BUFFER_RESUME_SECS: f64 = 3.0;
pub const BUFFER_MIN_CLEAR_MS: f64 = 1200.0;
pub const MIN_SCHEDULE_LEAD_MS: f64 = 50.0;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
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
    pub cache_time: f64,
}

impl Default for PlayerStatus {
    fn default() -> Self {
        Self {
            generation: 0,
            measured_ms: 0.0,
            position: 0.0,
            duration: 0.0,
            speed: 1.0,
            paused: true,
            seeking: false,
            loaded: false,
            buffering: false,
            blocked: false,
            ended: false,
            cache_time: 0.0,
        }
    }
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: u64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalDecision {
    pub id: u64,
    pub name: String,
    pub accepted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlayerCommand {
    Prepare { token: u64, position: f64 },
    Schedule { token: u64, at_ms: f64 },
    Pause { paused: bool },
    Seek { position: f64 },
    Speed { speed: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostNotice {
    Reconnected,
    StartCancelled,
    StartTimeout,
}

#[derive(Clone, Debug)]
pub enum HostOutput {
    Send(Message),
    Player(PlayerCommand),
    Notice(HostNotice),
    ClearNotice,
    Disconnect,
    WebGuestJoined,
    StreamRequest,
    StreamAnswer(Box<StreamAnswer>),
    WebStreamOffer(Box<WebStreamOffer>),
    StreamStop { id: u64 },
    SubtitleRequest { track: u8 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum BufferLatch {
    #[default]
    Idle,
    Active,
    Recovering(f64),
}

pub struct Host {
    session: Session,
    clock: Clock,
    controller: Controller,
    snapshot: Option<Snapshot>,
    presence: Presence,
    player: PlayerStatus,
    generation: u64,
    speed: f64,
    requested: Option<u64>,
    player_token: u64,
    player_operation: Option<(u64, u64)>,
    prepared: Option<u64>,
    scheduled: Option<u64>,
    last_pause: Option<u64>,
    failed: Option<u64>,
    recover: bool,
    watching: bool,
    watching_media: Option<Media>,
    buffer_latch: BufferLatch,
    fatal_error: bool,
    stream_errored: bool,
    peer_left: bool,
    scheduled_start_ms: Option<f64>,
    last_start_ms: Option<f64>,
    last_start_id: Option<u64>,
    pending_approval: Option<PendingApproval>,
    outputs: Vec<HostOutput>,
}

impl Default for Host {
    fn default() -> Self {
        Self::new(0.0, 0.0)
    }
}

impl Host {
    pub fn new(position: f64, now: f64) -> Self {
        let mut session = Session::default();
        session.state.position = position;
        session.state.host_ms = now;
        Self {
            session,
            clock: Clock::host(),
            controller: Controller::default(),
            snapshot: None,
            presence: Presence::default(),
            player: PlayerStatus::default(),
            generation: 0,
            speed: 1.0,
            requested: None,
            player_token: 0,
            player_operation: None,
            prepared: None,
            scheduled: None,
            last_pause: None,
            failed: None,
            recover: false,
            watching: false,
            watching_media: None,
            buffer_latch: BufferLatch::Idle,
            fatal_error: false,
            stream_errored: false,
            peer_left: false,
            scheduled_start_ms: None,
            last_start_ms: None,
            last_start_id: None,
            pending_approval: None,
            outputs: Vec::new(),
        }
    }

    pub fn take_outputs(&mut self) -> Vec<HostOutput> {
        std::mem::take(&mut self.outputs)
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    pub fn presence(&self) -> &Presence {
        &self.presence
    }

    pub fn presence_mut(&mut self) -> &mut Presence {
        &mut self.presence
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn is_connected(&self) -> bool {
        self.session.connected
    }

    pub fn guest_is_web(&self) -> bool {
        self.session.state.guest.web_guest
    }

    pub fn can_send(&self, message: &Message) -> bool {
        if !self.session.connected {
            return false;
        }
        if matches!(
            message,
            Message::SubtitleChunk(_)
                | Message::SubtitleClear
                | Message::SubtitleFailed { .. }
                | Message::WebStreamAnswer(_)
        ) {
            return self.session.state.guest.web_guest;
        }
        true
    }

    pub fn watching(&self) -> bool {
        self.watching
    }

    pub fn peer_left(&self) -> bool {
        self.peer_left
    }

    pub fn scheduled_start_ms(&self) -> Option<f64> {
        self.scheduled_start_ms
    }

    pub fn last_start_ms(&self) -> Option<f64> {
        self.last_start_ms
    }

    pub fn last_start_id(&self) -> Option<u64> {
        self.last_start_id
    }

    pub fn pending_approval(&self) -> Option<&PendingApproval> {
        self.pending_approval.as_ref()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn set_generation(&mut self, generation: u64) {
        self.generation = generation;
    }

    pub fn set_fatal_error(&mut self, fatal: bool) {
        self.fatal_error = fatal;
    }

    pub fn set_stream_errored(&mut self, errored: bool) {
        self.stream_errored = errored;
    }

    pub fn set_presence(&mut self, presence: Presence) {
        self.presence = presence;
    }

    pub fn reset_buffer_latch(&mut self) {
        self.buffer_latch = BufferLatch::Idle;
    }

    pub fn begin_loading(&mut self, generation: u64, now: f64) {
        self.generation = generation;
        self.watching = false;
        self.watching_media = None;
        self.buffer_latch = BufferLatch::Idle;
        self.control(Control::Pause, Side::Host, now);
        self.reset_operation(now);
        self.presence.ready = false;
        self.presence.media = None;
        self.presence.file_name = None;
        self.presence.loading = true;
        self.presence.verification = None;
    }

    pub fn set_media_loaded(&mut self, media: Media, now: f64) {
        self.session.apply(Control::Pause, now);
        self.session.state.position = 0.0;
        self.session.state.host_ms = now;
        self.reset_operation(now);
        self.presence.media = Some(media);
        self.presence.ready = false;
        self.presence.streaming = false;
        self.presence.loading = false;
        self.presence.verification = None;
        self.player = PlayerStatus::default();
    }

    pub fn request_approval(&mut self, id: u64, name: String) {
        self.pending_approval = Some(PendingApproval { id, name });
    }

    pub fn approval_finished(&mut self, id: u64) -> bool {
        if self.pending_approval.as_ref().is_some_and(|p| p.id == id) {
            self.pending_approval = None;
            true
        } else {
            false
        }
    }

    pub fn approve(&mut self, id: u64, accept: bool) -> Option<ApprovalDecision> {
        if self.pending_approval.as_ref().is_some_and(|p| p.id == id)
            && let Some(pending) = self.pending_approval.take()
        {
            if accept {
                self.watching = false;
                self.watching_media = None;
            }
            Some(ApprovalDecision {
                id: pending.id,
                name: pending.name,
                accepted: accept,
            })
        } else {
            None
        }
    }

    pub fn on_connected(&mut self, now: f64) {
        if self.watching {
            self.outputs
                .push(HostOutput::Notice(HostNotice::Reconnected));
        }
        self.on_disconnected(now);
        self.session.connected = true;
        self.peer_left = false;
        self.recover = true;
        self.clock = Clock::host();
        self.publish(now);
    }

    pub fn on_disconnected(&mut self, now: f64) {
        self.snapshot = None;
        self.session.disconnect(now);
        self.reset_operation(now);
        self.last_start_ms = None;
        self.last_start_id = None;
        self.failed = None;
        self.scheduled_start_ms = None;
    }

    pub fn on_peer_left(&mut self, now: f64) {
        self.on_disconnected(now);
        self.peer_left = true;
    }

    pub fn receive_message(&mut self, message: Message, received_ms: f64, now: f64) {
        match message {
            Message::Presence(p) if p.valid() => {
                let became_web_guest = !self.session.state.guest.web_guest && p.web_guest;
                self.session.state.guest = *p;
                if became_web_guest {
                    self.outputs.push(HostOutput::WebGuestJoined);
                }
                self.progress(now);
            }
            Message::Request(control) => {
                self.control(control, Side::Guest, now);
            }
            Message::Prepared { id } => {
                self.refresh_presence(now);
                self.session.prepared(id, true, now);
                self.publish(now);
            }
            Message::Armed { id } => {
                self.session.armed(id, now);
                self.publish(now);
            }
            Message::Failed { id } => {
                self.session.fail(id, now);
                self.publish(now);
            }
            Message::Ping { sent_ms } if sent_ms.is_finite() => {
                self.send(Message::Pong {
                    sent_ms,
                    received_ms,
                    sent_host_ms: now,
                });
            }
            Message::StreamRequest => {
                self.outputs.push(HostOutput::StreamRequest);
            }
            Message::WebStreamOffer(offer) => {
                if offer.valid() && self.session.state.guest.web_guest {
                    self.outputs.push(HostOutput::WebStreamOffer(offer));
                }
            }
            Message::SubtitleRequest { track } => {
                if self.session.connected && self.session.state.guest.web_guest {
                    self.outputs.push(HostOutput::SubtitleRequest { track });
                }
            }
            Message::StreamAnswer(answer) if answer.valid() => {
                self.outputs.push(HostOutput::StreamAnswer(answer));
            }
            Message::StreamStop { id } => {
                self.outputs.push(HostOutput::StreamStop { id });
            }
            _ => {
                self.on_disconnected(now);
                self.outputs.push(HostOutput::Disconnect);
            }
        }
    }

    pub fn control(&mut self, control: Control, by: Side, now: f64) {
        if !control.valid() {
            return;
        }
        self.outputs.push(HostOutput::ClearNotice);
        if matches!(control, Control::Play)
            && !self.player.buffering
            && matches!(self.buffer_latch, BufferLatch::Recovering(_))
        {
            self.buffer_latch = BufferLatch::Idle;
        }
        self.refresh_presence(now);
        if self.session.request(control, by, now) {
            if matches!(control, Control::Play) {
                self.watching = true;
                self.watching_media = self.presence.media.clone();
            }
            self.publish(now);
        }
    }

    pub fn player_status(&mut self, status: PlayerStatus, now: f64) {
        if status.generation != self.generation {
            self.progress(now);
            return;
        }
        self.player = status;
        self.refresh_presence(now);
        if self.presence.blocked
            && let Some(id) = self.scheduled
        {
            self.operation_failed(id, now);
        }
        self.progress(now);
    }

    pub fn player_prepared(&mut self, token: u64, now: f64) {
        if self
            .player_operation
            .is_some_and(|(current, wire)| current == token && self.requested == Some(wire))
        {
            let id = self.player_operation.unwrap().1;
            self.prepared = Some(id);
            self.session.prepared(id, false, now);
            self.publish(now);
        }
        self.progress(now);
    }

    pub fn player_started(&mut self, token: u64, at_ms: f64, now: f64) {
        if self
            .player_operation
            .is_some_and(|(current, wire)| current == token && self.scheduled == Some(wire))
        {
            let id = self.player_operation.unwrap().1;
            self.last_start_ms = self.clock.host_now(at_ms);
            self.last_start_id = Some(id);
            self.controller.reset(now);
        }
        self.progress(now);
    }

    pub fn player_missed(&mut self, token: u64, now: f64) {
        if let Some((current, wire)) = self.player_operation
            && current == token
        {
            self.operation_failed(wire, now);
        }
        self.progress(now);
    }

    pub fn player_error(&mut self, now: f64) {
        self.presence.ready = false;
        self.presence.blocked = true;
        self.control(Control::Pause, Side::Host, now);
        self.progress(now);
    }

    pub fn player_unavailable(&mut self, now: f64) {
        self.fatal_error = true;
        self.control(Control::Pause, Side::Host, now);
        self.progress(now);
    }

    pub fn heartbeat(&mut self, now: f64) {
        self.refresh_presence(now);
        self.publish(now);
    }

    pub fn refresh_presence(&mut self, now: f64) {
        let p = &self.player;
        if !p.loaded || p.ended || (self.stream_errored && !p.buffering) {
            self.buffer_latch = BufferLatch::Idle;
        } else if p.buffering {
            self.buffer_latch = BufferLatch::Active;
        } else {
            if matches!(self.buffer_latch, BufferLatch::Active) {
                self.buffer_latch = BufferLatch::Recovering(now);
            }
            if let BufferLatch::Recovering(cleared_at) = self.buffer_latch {
                let demux_ahead = (p.cache_time - p.position).max(0.0);
                let remaining_media = if p.duration > 0.0 {
                    (p.duration - p.position - 0.5).max(0.0)
                } else {
                    BUFFER_RESUME_SECS
                };
                let target_secs = BUFFER_RESUME_SECS.min(remaining_media.max(0.5));
                if demux_ahead >= target_secs.min(2.0) || now - cleared_at >= BUFFER_MIN_CLEAR_MS {
                    self.buffer_latch = BufferLatch::Idle;
                }
            }
        }
        self.presence.buffering =
            p.loaded && !p.ended && !matches!(self.buffer_latch, BufferLatch::Idle);
        self.presence.ready = p.loaded
            && p.generation == self.generation
            && self.presence.media.is_some()
            && !self.fatal_error
            && now - p.measured_ms < STATUS_FRESH_MS;
        self.presence.blocked = p.blocked || self.presence.buffering || self.fatal_error;
        self.presence.ended = p.ended;
        self.presence.duration = if p.duration.is_finite() {
            p.duration.max(0.0)
        } else {
            0.0
        };
        self.presence.clock_ready = self.clock.ready(now);
        self.presence.rtt_ms = self.clock.rtt_ms;
        self.presence.uncertainty_ms = self.clock.uncertainty_ms;
        self.presence.stream_progress = None;
        self.presence.playback = self.clock.host_now(p.measured_ms).map(|host_ms| Playback {
            position: p.position.max(0.0),
            host_ms,
            speed: p.speed.clamp(0.5, 2.0),
            paused: p.paused || self.presence.blocked,
            seeking: p.seeking,
        });
        self.session.state.host = self.presence.clone();
    }

    pub fn publish(&mut self, now: f64) {
        self.refresh_presence(now);
        let state = self.session.snapshot(now);
        self.send(Message::State(Box::new(state.clone())));
        self.apply_snapshot(state, now);
    }

    pub fn apply_snapshot(&mut self, state: Snapshot, now: f64) {
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
                self.reset_operation(now);
                self.requested = Some(t.id);
                self.player_token += 1;
                self.player_operation = Some((self.player_token, t.id));
                self.outputs.push(HostOutput::ClearNotice);
                self.player_cmd(PlayerCommand::Prepare {
                    token: self.player_token,
                    position: t.position,
                });
                self.speed = 1.0;
            }
            if let Some(start) = t.start_at_ms {
                if self.prepared != Some(t.id) {
                    self.operation_failed(t.id, now);
                    return;
                }
                if self.scheduled != Some(t.id) && self.session.host_can_arm() {
                    let Some(host_now) = self.clock.host_now(now) else {
                        self.operation_failed(t.id, now);
                        return;
                    };
                    if start - host_now < MIN_SCHEDULE_LEAD_MS {
                        self.operation_failed(t.id, now);
                        return;
                    }
                    let token = self.player_operation.unwrap().0;
                    self.scheduled = Some(t.id);
                    self.scheduled_start_ms = Some(start);
                    self.player_cmd(PlayerCommand::Schedule {
                        token,
                        at_ms: now + (start - host_now),
                    });
                }
            }
        } else if state.paused && self.last_pause != Some(state.revision) {
            self.reset_operation(now);
            self.last_pause = Some(state.revision);
        }
    }

    pub fn progress(&mut self, now: f64) {
        self.refresh_presence(now);
        let before = self.session.state.revision;
        if self.recover && self.session.ready() {
            self.recover = false;
            self.session
                .apply(Control::Seek(self.session.position(now)), now);
        }
        if self.session.advance(now) {
            self.outputs
                .push(HostOutput::Notice(HostNotice::StartTimeout));
        }
        if self.session.state.revision != before {
            self.publish(now);
        }
    }

    pub fn correct(&mut self, now: f64) {
        let Some(state) = self.snapshot.clone() else {
            return;
        };
        if state.transition.is_some()
            || !self.presence.ready
            || self.player.seeking
            || now - self.player.measured_ms > CORRECT_FRESH_MS
        {
            self.set_speed(1.0);
            return;
        }
        let Some(host_now) = self.clock.host_now(now) else {
            self.set_speed(1.0);
            return;
        };
        let matched = state.host.media.is_some() && state.host.media == state.guest.media;
        if !matched {
            self.set_speed(1.0);
            return;
        }
        let actual = self.player.position_at(now);
        let target = state.position_at(host_now);
        match self.controller.update(actual, target, state.paused, now) {
            Correction::Seek(p) => {
                self.player_cmd(PlayerCommand::Seek {
                    position: p.min(self.player.duration),
                });
                self.set_speed(1.0);
            }
            Correction::Speed(speed) => self.set_speed(speed),
            Correction::None => self.set_speed(1.0),
        }
    }

    pub fn reset_operation(&mut self, now: f64) {
        self.requested = None;
        self.player_operation = None;
        self.prepared = None;
        self.scheduled = None;
        self.last_pause = None;
        self.controller.reset(now);
        self.player_cmd(PlayerCommand::Pause { paused: true });
        self.set_speed(1.0);
    }

    fn operation_failed(&mut self, id: u64, now: f64) {
        if self.requested != Some(id) && self.scheduled != Some(id) {
            return;
        }
        self.reset_operation(now);
        self.failed = Some(id);
        self.outputs
            .push(HostOutput::Notice(HostNotice::StartCancelled));
        self.session.fail(id, now);
        self.publish(now);
    }

    fn send(&mut self, message: Message) {
        if self.can_send(&message) {
            self.outputs.push(HostOutput::Send(message));
        }
    }

    fn player_cmd(&mut self, command: PlayerCommand) {
        self.outputs.push(HostOutput::Player(command));
    }

    fn set_speed(&mut self, speed: f64) {
        if (self.speed - speed).abs() > 0.0001 {
            self.player_cmd(PlayerCommand::Speed { speed });
            self.speed = speed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        protocol::{SubtitleChunk, WebStreamAnswer},
        session::{AUTO_RESUME_STABLE_MS, PREPARE_TIMEOUT_MS},
    };

    fn media() -> Media {
        Media {
            hash: "a".repeat(64),
            bytes: 1024,
        }
    }

    fn guest_presence(web_guest: bool) -> Presence {
        Presence {
            media: Some(media()),
            ready: true,
            duration: 120.0,
            clock_ready: true,
            rtt_ms: 40.0,
            uncertainty_ms: 10.0,
            web_guest,
            ..Default::default()
        }
    }

    fn host_player(now: f64, position: f64, paused: bool) -> PlayerStatus {
        PlayerStatus {
            generation: 1,
            measured_ms: now,
            position,
            duration: 120.0,
            speed: 1.0,
            paused,
            loaded: true,
            ..Default::default()
        }
    }

    fn ready_host(now: f64, web_guest: bool) -> Host {
        let mut host = Host::new(0.0, now);
        host.set_generation(1);
        host.presence_mut().media = Some(media());
        host.player_status(host_player(now, 0.0, true), now);
        host.on_connected(now);
        host.take_outputs();
        host.receive_message(
            Message::Presence(Box::new(guest_presence(web_guest))),
            now,
            now,
        );
        let outputs = host.take_outputs();
        let prep_token = outputs.iter().find_map(|o| match o {
            HostOutput::Player(PlayerCommand::Prepare { token, .. }) => Some(*token),
            _ => None,
        });
        if let Some(token) = prep_token {
            let wire = host.snapshot().unwrap().transition.as_ref().unwrap().id;
            host.player_prepared(token, now + 10.0);
            host.receive_message(Message::Prepared { id: wire }, now + 20.0, now + 20.0);
            host.take_outputs();
        }
        host
    }

    #[test]
    fn approval_accept_reject_and_finish_lifecycle() {
        let mut host = Host::new(0.0, 0.0);
        host.request_approval(1, "Alice".into());
        assert_eq!(
            host.pending_approval(),
            Some(&PendingApproval {
                id: 1,
                name: "Alice".into()
            })
        );
        assert!(host.approve(99, true).is_none());
        assert!(host.pending_approval().is_some());

        let decision = host.approve(1, false).unwrap();
        assert_eq!(
            decision,
            ApprovalDecision {
                id: 1,
                name: "Alice".into(),
                accepted: false
            }
        );
        assert!(host.pending_approval().is_none());

        let mut host = ready_host(0.0, false);
        host.control(Control::Play, Side::Host, 100.0);
        assert!(host.watching());
        host.request_approval(2, "Bob".into());
        let accepted = host.approve(2, true).unwrap();
        assert!(accepted.accepted);
        assert_eq!(accepted.name, "Bob");
        assert!(!host.watching());

        host.request_approval(3, "Carol".into());
        assert!(!host.approval_finished(99));
        assert!(host.approval_finished(3));
        assert!(host.pending_approval().is_none());
    }

    #[test]
    fn prepare_arm_and_scheduled_start_flow() {
        let mut host = ready_host(0.0, false);
        assert!(host.snapshot().unwrap().paused);
        assert!(host.snapshot().unwrap().transition.is_none());

        host.receive_message(Message::Request(Control::Play), 100.0, 100.0);
        let outputs = host.take_outputs();
        let (token, wire) = {
            let token = outputs
                .iter()
                .find_map(|o| match o {
                    HostOutput::Player(PlayerCommand::Prepare { token, .. }) => Some(*token),
                    _ => None,
                })
                .unwrap();
            let wire = host.snapshot().unwrap().transition.as_ref().unwrap().id;
            (token, wire)
        };
        assert!(
            outputs
                .iter()
                .any(|o| matches!(o, HostOutput::Send(Message::State(_))))
        );

        host.player_prepared(token, 110.0);
        host.take_outputs();
        host.receive_message(Message::Prepared { id: wire }, 120.0, 120.0);
        let outputs = host.take_outputs();
        let start_at = host
            .snapshot()
            .unwrap()
            .transition
            .as_ref()
            .unwrap()
            .start_at_ms
            .unwrap();
        assert!(
            !outputs
                .iter()
                .any(|o| matches!(o, HostOutput::Player(PlayerCommand::Schedule { .. }))),
            "host must wait for guest Armed before scheduling local playback"
        );

        host.receive_message(Message::Armed { id: wire }, 140.0, 140.0);
        let outputs = host.take_outputs();
        let scheduled_at = outputs
            .iter()
            .find_map(|o| match o {
                HostOutput::Player(PlayerCommand::Schedule { token: t, at_ms }) if *t == token => {
                    Some(*at_ms)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(scheduled_at, start_at);
        assert_eq!(host.scheduled_start_ms(), Some(start_at));

        host.player_started(token, start_at, start_at);
        assert_eq!(host.last_start_ms(), Some(start_at));
        assert_eq!(host.last_start_id(), Some(wire));

        host.progress(start_at + 10.0);
        assert!(!host.snapshot().unwrap().paused);
        assert!(host.snapshot().unwrap().transition.is_none());
    }

    #[test]
    fn prepare_timeout_cancels_and_notifies() {
        let mut host = ready_host(0.0, false);
        host.control(Control::Play, Side::Host, 100.0);
        host.take_outputs();
        host.player_status(
            host_player(100.0 + PREPARE_TIMEOUT_MS, 0.0, true),
            100.0 + PREPARE_TIMEOUT_MS,
        );
        let outputs = host.take_outputs();
        assert!(
            outputs
                .iter()
                .any(|o| matches!(o, HostOutput::Notice(HostNotice::StartTimeout)))
        );
        assert!(host.snapshot().unwrap().paused);
        assert!(host.snapshot().unwrap().transition.is_none());
    }

    #[test]
    fn buffering_pauses_and_auto_resumes_after_stability() {
        let mut host = ready_host(0.0, false);
        host.control(Control::Play, Side::Host, 100.0);
        let outputs = host.take_outputs();
        let token = outputs
            .iter()
            .find_map(|o| match o {
                HostOutput::Player(PlayerCommand::Prepare { token, .. }) => Some(*token),
                _ => None,
            })
            .unwrap();
        let wire = host.snapshot().unwrap().transition.as_ref().unwrap().id;
        host.player_prepared(token, 110.0);
        host.receive_message(Message::Prepared { id: wire }, 120.0, 120.0);
        host.receive_message(Message::Armed { id: wire }, 140.0, 140.0);
        let start_at = host.scheduled_start_ms().unwrap();
        host.progress(start_at + 20.0);
        host.take_outputs();
        assert!(!host.snapshot().unwrap().paused);

        let mut buffering_guest = guest_presence(false);
        buffering_guest.buffering = true;
        buffering_guest.blocked = true;
        host.player_status(host_player(start_at + 50.0, 0.05, false), start_at + 50.0);
        host.receive_message(
            Message::Presence(Box::new(buffering_guest)),
            start_at + 50.0,
            start_at + 50.0,
        );
        let outputs = host.take_outputs();
        assert!(host.snapshot().unwrap().paused);
        assert!(host.snapshot().unwrap().waiting_buffer);
        assert!(
            !outputs
                .iter()
                .any(|o| matches!(o, HostOutput::Notice(HostNotice::StartTimeout)))
        );

        let clear_at = start_at + 200.0;
        host.player_status(host_player(clear_at, 0.05, true), clear_at);
        host.receive_message(
            Message::Presence(Box::new(guest_presence(false))),
            clear_at,
            clear_at,
        );
        host.take_outputs();
        assert!(host.snapshot().unwrap().waiting_buffer);

        let resume_at = clear_at + AUTO_RESUME_STABLE_MS;
        host.player_status(host_player(resume_at, 0.05, true), resume_at);
        host.progress(resume_at);
        assert!(!host.snapshot().unwrap().waiting_buffer);
        assert!(
            host.snapshot()
                .unwrap()
                .transition
                .as_ref()
                .is_some_and(|t| t.resume)
        );

        let mut local_buf = host_player(resume_at + 10.0, 0.05, true);
        local_buf.buffering = true;
        host.player_status(local_buf.clone(), resume_at + 10.0);
        assert!(host.presence().buffering);
        local_buf.buffering = false;
        local_buf.measured_ms = resume_at + 20.0;
        host.player_status(local_buf, resume_at + 20.0);
        assert!(host.presence().buffering);
        host.control(Control::Play, Side::Host, resume_at + 30.0);
        assert!(!host.presence().buffering);
    }

    #[test]
    fn disconnect_and_peer_left_pause_and_reset_state() {
        let mut host = ready_host(0.0, false);
        host.control(Control::Play, Side::Host, 100.0);
        host.take_outputs();
        assert!(host.watching());

        host.on_disconnected(200.0);
        let outputs = host.take_outputs();
        assert!(!host.is_connected());
        assert!(host.snapshot().is_none());
        assert!(host.scheduled_start_ms().is_none());
        assert!(host.watching());
        assert!(
            outputs
                .iter()
                .any(|o| matches!(o, HostOutput::Player(PlayerCommand::Pause { paused: true })))
        );

        host.on_connected(300.0);
        let outputs = host.take_outputs();
        assert!(host.is_connected());
        assert!(
            outputs
                .iter()
                .any(|o| matches!(o, HostOutput::Notice(HostNotice::Reconnected)))
        );

        host.on_peer_left(400.0);
        assert!(!host.is_connected());
        assert!(host.peer_left());
    }

    #[test]
    fn ping_pong_answers_with_host_timestamps_and_invalid_message_disconnects() {
        let mut host = ready_host(0.0, false);
        host.receive_message(Message::Ping { sent_ms: 123.5 }, 500.0, 502.0);
        let outputs = host.take_outputs();
        assert!(outputs.iter().any(|o| matches!(
            o,
            HostOutput::Send(Message::Pong {
                sent_ms,
                received_ms,
                sent_host_ms,
            }) if (*sent_ms - 123.5).abs() < 1e-9
                && (*received_ms - 500.0).abs() < 1e-9
                && (*sent_host_ms - 502.0).abs() < 1e-9
        )));

        host.receive_message(
            Message::Pong {
                sent_ms: 1.0,
                received_ms: 2.0,
                sent_host_ms: 3.0,
            },
            600.0,
            600.0,
        );
        let outputs = host.take_outputs();
        assert!(!host.is_connected());
        assert!(outputs.iter().any(|o| matches!(o, HostOutput::Disconnect)));
    }

    #[test]
    fn web_guest_gating_for_subtitles_and_web_stream() {
        let mut host = ready_host(0.0, false);
        let chunk = SubtitleChunk {
            id: 1,
            name: "sub.vtt".into(),
            index: 0,
            total: 1,
            data: "WEBVTT\n".into(),
            track: None,
        };
        let sdp = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\na=fingerprint:sha-256 AA:BB\r\n".to_owned();
        let answer = WebStreamAnswer {
            id: 1,
            sdp: sdp.clone(),
            media: media(),
            file_name: "movie.mp4".into(),
            duration: 120.0,
            chunk_size: 256 * 1024,
            hash_table_digest: "b".repeat(64),
            mse: None,
            subtitle_tracks: Vec::new(),
        };

        assert!(!host.guest_is_web());
        assert!(!host.can_send(&Message::SubtitleChunk(Box::new(chunk.clone()))));
        assert!(!host.can_send(&Message::SubtitleClear));
        assert!(!host.can_send(&Message::SubtitleFailed { track: 0 }));
        assert!(!host.can_send(&Message::WebStreamAnswer(Box::new(answer.clone()))));
        assert!(host.can_send(&Message::StreamRequest));

        host.receive_message(Message::SubtitleRequest { track: 0 }, 100.0, 100.0);
        host.receive_message(
            Message::WebStreamOffer(Box::new(WebStreamOffer {
                id: 7,
                sdp: sdp.clone(),
            })),
            100.0,
            100.0,
        );
        let outputs = host.take_outputs();
        assert!(host.is_connected());
        assert!(!outputs.iter().any(|o| matches!(
            o,
            HostOutput::SubtitleRequest { .. } | HostOutput::WebStreamOffer(_)
        )));

        host.receive_message(
            Message::Presence(Box::new(guest_presence(true))),
            150.0,
            150.0,
        );
        let outputs = host.take_outputs();
        assert!(host.guest_is_web());
        assert!(
            outputs
                .iter()
                .any(|o| matches!(o, HostOutput::WebGuestJoined))
        );
        assert!(host.can_send(&Message::SubtitleChunk(Box::new(chunk))));
        assert!(host.can_send(&Message::SubtitleClear));
        assert!(host.can_send(&Message::SubtitleFailed { track: 0 }));
        assert!(host.can_send(&Message::WebStreamAnswer(Box::new(answer))));

        host.receive_message(
            Message::Presence(Box::new(guest_presence(true))),
            200.0,
            200.0,
        );
        let outputs = host.take_outputs();
        assert!(
            !outputs
                .iter()
                .any(|o| matches!(o, HostOutput::WebGuestJoined))
        );

        host.receive_message(Message::SubtitleRequest { track: 2 }, 210.0, 210.0);
        host.receive_message(
            Message::WebStreamOffer(Box::new(WebStreamOffer { id: 8, sdp })),
            220.0,
            220.0,
        );
        let outputs = host.take_outputs();
        assert!(
            outputs
                .iter()
                .any(|o| matches!(o, HostOutput::SubtitleRequest { track: 2 }))
        );
        assert!(
            outputs
                .iter()
                .any(|o| matches!(o, HostOutput::WebStreamOffer(o) if o.id == 8))
        );
    }

    #[test]
    fn playback_correction_emits_speed_and_seek() {
        let mut host = ready_host(0.0, false);
        host.player_status(host_player(1000.0, 2.0, true), 1000.0);
        host.take_outputs();
        host.correct(1000.0);
        host.player_status(host_player(1250.0, 2.0, true), 1250.0);
        host.take_outputs();
        host.correct(1250.0);
        let outputs = host.take_outputs();
        assert!(
            outputs
                .iter()
                .any(|o| matches!(o, HostOutput::Player(PlayerCommand::Seek { position }) if *position == 0.0))
        );

        host.control(Control::Play, Side::Host, 2000.0);
        let outputs = host.take_outputs();
        let token = outputs
            .iter()
            .find_map(|o| match o {
                HostOutput::Player(PlayerCommand::Prepare { token, .. }) => Some(*token),
                _ => None,
            })
            .unwrap();
        let wire = host.snapshot().unwrap().transition.as_ref().unwrap().id;
        host.player_prepared(token, 2010.0);
        host.receive_message(Message::Prepared { id: wire }, 2020.0, 2020.0);
        host.receive_message(Message::Armed { id: wire }, 2040.0, 2040.0);
        let start_at = host.scheduled_start_ms().unwrap();
        host.player_started(token, start_at, start_at);
        host.player_status(host_player(start_at + 600.0, 0.6, false), start_at + 600.0);
        host.progress(start_at + 600.0);
        host.take_outputs();
        assert!(!host.snapshot().unwrap().paused);

        let mut saw_speed = false;
        for step in 0..10 {
            let t = start_at + 600.0 + step as f64 * 50.0;
            let expected = (t - start_at) / 1000.0;
            host.player_status(host_player(t, expected - 0.2, false), t);
            host.take_outputs();
            host.correct(t);
            if host.take_outputs().iter().any(
                |o| matches!(o, HostOutput::Player(PlayerCommand::Speed { speed }) if *speed > 1.0),
            ) {
                saw_speed = true;
                break;
            }
        }
        assert!(saw_speed);
    }
}

//! O anfitrião ordena operações e só agenda play após os dois seeks concluírem.
use crate::protocol::{Control, Presence, Snapshot, Transition};

pub const PREPARE_TIMEOUT_MS: f64 = 10_000.0;
pub struct Session {
    pub state: Snapshot,
    pub connected: bool,
    host_prepared: bool,
    guest_prepared: bool,
    guest_armed: bool,
    prepare_since: f64,
    arm_by: f64,
    last_operation: u64,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            state: Snapshot {
                revision: 0,
                position: 0.0,
                paused: true,
                host_ms: 0.0,
                transition: None,
                host: Presence::default(),
                guest: Presence::default(),
            },
            connected: false,
            host_prepared: false,
            guest_prepared: false,
            guest_armed: false,
            prepare_since: 0.0,
            arm_by: 0.0,
            last_operation: 0,
        }
    }
}
impl Session {
    pub fn matched(&self) -> bool {
        self.connected
            && self.state.host.ready
            && self.state.guest.ready
            && self.state.host.media.is_some()
            && self.state.host.media == self.state.guest.media
    }
    pub fn ready(&self) -> bool {
        self.matched()
            && self.state.guest.clock_ready
            && !self.state.host.blocked
            && !self.state.guest.blocked
    }
    pub fn position(&self, now: f64) -> f64 {
        self.state.position_at(now)
    }
    pub fn apply(&mut self, command: Control, now: f64) -> bool {
        if !command.valid()
            || (matches!(command, Control::Play) && !self.ready())
            || (matches!(command, Control::Seek(_)) && !self.matched())
        {
            return false;
        }
        if matches!(command, Control::Play) && self.state.playing_at(now) {
            return false;
        }
        let position = self.position(now);
        let resume = match command {
            Control::Play => true,
            Control::Seek(_) => self
                .state
                .transition
                .as_ref()
                .map_or(!self.state.paused, |t| t.resume),
            Control::Pause => false,
        };
        self.state.position = match command {
            Control::Seek(p) => p.min(self.state.host.duration),
            _ => position,
        };
        self.state.host_ms = now;
        self.state.paused = true;
        self.state.revision += 1;
        self.state.transition = if matches!(command, Control::Pause) {
            None
        } else {
            self.last_operation = self.state.revision;
            Some(Transition {
                id: self.last_operation,
                position: self.state.position,
                resume,
                start_at_ms: None,
            })
        };
        self.prepare_since = now;
        self.host_prepared = false;
        self.guest_prepared = false;
        self.guest_armed = false;
        true
    }
    pub fn prepared(&mut self, id: u64, guest: bool, now: f64) -> bool {
        let Some(t) = &self.state.transition else {
            return false;
        };
        if t.id != id || t.start_at_ms.is_some() {
            return false;
        }
        if guest {
            self.guest_prepared = true;
        } else {
            self.host_prepared = true;
        }
        self.schedule_if_ready(now)
    }
    fn schedule_if_ready(&mut self, now: f64) -> bool {
        let paused_seek_ready = self.state.transition.as_ref().is_some_and(|t| !t.resume)
            && self.matched()
            && self.state.guest.clock_ready
            && (!self.state.host.blocked || self.state.host.ended)
            && (!self.state.guest.blocked || self.state.guest.ended);
        if !self.host_prepared || !self.guest_prepared || (!self.ready() && !paused_seek_ready) {
            return false;
        }
        let Some(t) = &mut self.state.transition else {
            return false;
        };
        if t.start_at_ms.is_some() {
            return false;
        }
        if t.resume {
            let guest = &self.state.guest;
            let lead =
                (guest.rtt_ms * 2.0 + guest.uncertainty_ms * 4.0 + 150.0).clamp(350.0, 5000.0);
            t.start_at_ms = Some(now + lead);
            self.arm_by = now + lead - (guest.rtt_ms + 100.0).min(lead / 2.0);
        } else {
            self.state.transition = None;
        }
        self.state.revision += 1;
        true
    }
    pub fn armed(&mut self, id: u64, now: f64) -> bool {
        if self
            .state
            .transition
            .as_ref()
            .is_some_and(|t| t.id == id && t.start_at_ms.is_some())
            && now < self.arm_by
        {
            self.guest_armed = true;
            true
        } else {
            false
        }
    }
    pub fn host_can_arm(&self) -> bool {
        self.guest_armed
    }
    pub fn fail(&mut self, id: u64, now: f64) {
        let current = self
            .state
            .transition
            .as_ref()
            .map_or(self.last_operation, |t| t.id);
        if current == id {
            self.apply(Control::Pause, now);
        }
    }
    /// Retorna true quando uma operação expirou. O chamador publica a pausa.
    pub fn advance(&mut self, now: f64) -> bool {
        if let Some(t) = self.state.transition.clone() {
            if !self.matched() {
                self.apply(Control::Pause, now);
                return false;
            }
            if let Some(start) = t.start_at_ms {
                if !self.ready() || (!self.guest_armed && now >= self.arm_by) {
                    self.apply(Control::Pause, now);
                    return true;
                }
                if now >= start {
                    self.state.position = t.position;
                    self.state.host_ms = start;
                    self.state.paused = false;
                    self.state.transition = None;
                    self.state.revision += 1;
                }
            } else if now - self.prepare_since >= PREPARE_TIMEOUT_MS {
                self.apply(Control::Pause, now);
                return true;
            } else {
                self.schedule_if_ready(now);
            }
        } else if !self.ready() && !self.state.paused {
            self.apply(Control::Pause, now);
        }
        false
    }
    pub fn disconnect(&mut self, now: f64) {
        self.apply(Control::Pause, now);
        self.connected = false;
        self.state.guest = Presence::default();
    }
    pub fn snapshot(&self, now: f64) -> Snapshot {
        let mut state = self.state.clone();
        state.position = self.position(now);
        state.host_ms = now;
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Media;
    fn session() -> Session {
        let p = Presence {
            media: Some(Media {
                hash: "a".repeat(64),
                bytes: 42,
            }),
            ready: true,
            duration: 120.0,
            clock_ready: true,
            ..Default::default()
        };
        let mut s = Session {
            connected: true,
            ..Default::default()
        };
        s.state.host = p.clone();
        s.state.guest = p;
        s
    }

    #[test]
    fn file_names_never_decide_whether_a_session_can_play() {
        let mut s = session();
        s.state.host.file_name = Some("original.mkv".into());
        s.state.guest.file_name = Some("renamed.mkv".into());
        assert!(s.matched());
        assert!(s.apply(Control::Play, 0.0));
        s.apply(Control::Pause, 1.0);
        s.state.guest.file_name = s.state.host.file_name.clone();
        s.state.guest.media.as_mut().unwrap().hash = "b".repeat(64);
        assert!(!s.matched());
        assert!(!s.apply(Control::Play, 2.0));
    }
    fn start(s: &mut Session) {
        s.apply(Control::Play, 0.0);
        let id = s.state.transition.as_ref().unwrap().id;
        s.prepared(id, false, 10.0);
        s.prepared(id, true, 20.0);
        s.armed(id, 30.0);
        s.advance(370.0);
        assert!(!s.state.paused);
    }
    #[test]
    fn waits_for_both_players_then_starts_at_common_time() {
        let mut s = session();
        s.state.guest.rtt_ms = 100.0;
        assert!(s.apply(Control::Play, 0.0));
        let id = s.state.transition.as_ref().unwrap().id;
        s.prepared(id, false, 10.0);
        assert!(s.state.transition.as_ref().unwrap().start_at_ms.is_none());
        s.advance(900.0);
        assert!(s.state.paused);
        s.prepared(id, true, 1000.0);
        let at = s.state.transition.as_ref().unwrap().start_at_ms.unwrap();
        assert_eq!(at, 1350.0);
        assert!(s.armed(id, 1050.0));
        s.advance(at - 1.0);
        assert!(s.state.paused);
        assert_eq!(s.position(at - 1.0), 0.0);
        s.advance(at + 20.0);
        assert!(!s.state.paused);
        assert!((s.position(at + 20.0) - 0.02).abs() < 1e-9);
    }
    #[test]
    fn superseded_acks_and_pause_cannot_start_old_operation() {
        let mut s = session();
        s.apply(Control::Play, 0.0);
        let old = s.state.transition.as_ref().unwrap().id;
        s.apply(Control::Seek(30.0), 1.0);
        assert!(!s.prepared(old, true, 2.0));
        s.fail(old, 3.0);
        assert!(s.state.transition.is_some());
        s.apply(Control::Pause, 4.0);
        s.prepared(old, false, 5.0);
        s.advance(1000.0);
        assert!(s.state.paused);
        assert!(s.state.transition.is_none());
        assert_eq!(s.position(1000.0), 30.0);
    }
    #[test]
    fn timeout_missing_arm_stall_and_disconnect_cancel_start() {
        let mut s = session();
        s.apply(Control::Play, 0.0);
        assert!(s.advance(PREPARE_TIMEOUT_MS));
        assert!(s.state.transition.is_none());
        s.apply(Control::Play, 11_000.0);
        let id = s.state.transition.as_ref().unwrap().id;
        s.prepared(id, false, 11_010.0);
        s.prepared(id, true, 11_020.0);
        assert!(s.advance(11_300.0));
        assert!(s.state.paused);
        start(&mut s);
        s.state.guest.blocked = true;
        s.advance(500.0);
        assert!(s.state.paused);
        s.state.guest.blocked = false;
        s.apply(Control::Play, 600.0);
        s.disconnect(650.0);
        assert!(s.state.transition.is_none());
        assert!(s.state.paused);
    }
    #[test]
    fn seeking_to_the_end_can_complete_paused() {
        let mut s = session();
        s.apply(Control::Seek(120.0), 0.0);
        s.state.host.blocked = true;
        s.state.host.ended = true;
        s.state.guest.blocked = true;
        s.state.guest.ended = true;
        let id = s.state.transition.as_ref().unwrap().id;
        s.prepared(id, false, 1.0);
        s.prepared(id, true, 2.0);
        assert!(s.state.transition.is_none());
        assert!(s.state.paused);
        assert!(!s.apply(Control::Play, 3.0));
        assert!(s.apply(Control::Seek(0.0), 4.0));
    }
    #[test]
    fn paused_seek_stays_paused_and_eof_can_seek_back() {
        let mut s = session();
        s.state.host.blocked = true;
        s.state.guest.blocked = true;
        assert!(s.apply(Control::Seek(999.0), 0.0));
        assert_eq!(s.state.position, 120.0);
        s.state.host.blocked = false;
        s.state.guest.blocked = false;
        let id = s.state.transition.as_ref().unwrap().id;
        s.prepared(id, true, 1.0);
        s.prepared(id, false, 2.0);
        assert!(s.state.transition.is_none());
        assert!(s.state.paused);
        assert!(!s.apply(Control::Seek(f64::NAN), 3.0));
        s.state.guest.media.as_mut().unwrap().hash = "b".repeat(64);
        assert!(!s.apply(Control::Play, 4.0));
    }
}

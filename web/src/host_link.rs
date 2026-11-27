use serde::Serialize;
use std::{collections::HashMap, io};
use sync2gether_core::{
    code::Code,
    frame,
    protocol::Message,
    relay_wire::{self, MAX_CONTROL, Reply, Request, valid_room, valid_secret},
    secure::{self, Cipher, INTRO_PAKE, INTRO_RESUME, Key, MAX_FRAME, WRONG_KEY},
};

pub const MAX_WRONG_CODES: u32 = 3;
pub const KEEPALIVE_MS: f64 = 15_000.0;
pub const CONTROL_SILENCE_MS: f64 = 45_000.0;
pub const STEP_MS: f64 = 10_000.0;
pub const SILENCE_MS: f64 = 5_000.0;
pub const INITIAL_BACKOFF_MS: f64 = 1_000.0;
pub const MAX_BACKOFF_MS: f64 = 10_000.0;
const MAX_PENDING_ACCEPTS: usize = 8;

pub const OUTDATED: &str = crate::link::OUTDATED;
pub const TOO_MANY_ATTEMPTS: &str = "Muitas tentativas com código incorreto. A sala foi encerrada.";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SocketCommand {
    ConnectControl { at: f64 },
    CloseControl,
    OpenAccept { id: u64 },
    CloseAccept { id: u64 },
}

#[derive(Debug)]
pub enum Event {
    RoomCode(String),
    RelayOffline,
    WrongCode,
    Connected { peer: String },
    Message(Box<Message>, f64),
    Disconnected,
    PeerLeft,
    Fatal(String),
}

enum ControlState {
    Closed,
    Opening,
    Hosting,
}

enum ConnState {
    Connecting,
    Accept,
    Intro,
    Noise {
        psk: Key,
        pake: bool,
    },
    Open {
        cipher: Cipher,
        sent: u64,
        received: u64,
        last_received: f64,
    },
}

struct Conn {
    ticket: u64,
    room: String,
    state: ConnState,
    reader: frame::Reader,
    out: Vec<u8>,
    since: f64,
    holds_slot: bool,
}

pub struct HostLink {
    name: String,
    secret: String,
    words: [u8; 2],
    room: Option<String>,
    resume_key: Option<Key>,
    wrong: u32,
    control: ControlState,
    control_reader: frame::Reader,
    control_out: Vec<u8>,
    control_since: f64,
    control_last_received: f64,
    control_last_alive: f64,
    backoff_ms: f64,
    next_conn: u64,
    conns: HashMap<u64, Conn>,
    pending_out: HashMap<u64, Vec<u8>>,
    active: Option<u64>,
    commands: Vec<SocketCommand>,
    ended: bool,
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    let _ = getrandom::fill(&mut bytes);
    bytes
}

fn random_secret() -> String {
    let bytes = random_bytes::<16>();
    let mut out = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn random_key() -> Key {
    random_bytes::<32>()
}

impl HostLink {
    pub fn new(name: &str) -> Self {
        Self::with_identity(name, random_secret(), random_bytes::<2>(), None, None)
    }

    pub fn with_identity(
        name: &str,
        secret: String,
        words: [u8; 2],
        room: Option<String>,
        resume_key: Option<Key>,
    ) -> Self {
        let secret = if valid_secret(&secret) {
            secret
        } else {
            random_secret()
        };
        let room = room.filter(|r| valid_room(r));
        Self {
            name: sync2gether_core::device_name(name),
            secret,
            words,
            room,
            resume_key,
            wrong: 0,
            control: ControlState::Closed,
            control_reader: frame::Reader::default(),
            control_out: Vec::new(),
            control_since: 0.0,
            control_last_received: 0.0,
            control_last_alive: 0.0,
            backoff_ms: INITIAL_BACKOFF_MS,
            next_conn: 1,
            conns: HashMap::new(),
            pending_out: HashMap::new(),
            active: None,
            commands: Vec::new(),
            ended: false,
        }
    }

    pub fn start(&mut self, now: f64) {
        if self.ended {
            return;
        }
        self.commands
            .push(SocketCommand::ConnectControl { at: now });
    }

    pub fn room(&self) -> Option<&str> {
        self.room.as_deref()
    }

    pub fn code(&self) -> Option<String> {
        self.room
            .as_deref()
            .map(|room| Code::from_indices(room, self.words).to_string())
    }

    pub fn secret(&self) -> &str {
        &self.secret
    }

    pub fn words(&self) -> [u8; 2] {
        self.words
    }

    pub fn resume_key(&self) -> Option<Key> {
        self.resume_key
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn is_guest_connected(&self) -> bool {
        self.active.is_some()
    }

    pub fn take_commands(&mut self) -> Vec<SocketCommand> {
        std::mem::take(&mut self.commands)
    }

    pub fn take_control_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.control_out)
    }

    pub fn take_accept_output(&mut self, id: u64) -> Vec<u8> {
        let mut out = self.pending_out.remove(&id).unwrap_or_default();
        if let Some(conn) = self.conns.get_mut(&id) {
            out.append(&mut conn.out);
        }
        out
    }

    pub fn control_open(&mut self, now: f64) {
        if self.ended {
            self.commands.push(SocketCommand::CloseControl);
            return;
        }
        self.control_reader = frame::Reader::default();
        self.control = ControlState::Opening;
        self.control_since = now;
        let request = Request::Host {
            version: relay_wire::VERSION,
            room: self.room.clone(),
            secret: self.secret.clone(),
        };
        match frame::encode_json(&request, MAX_CONTROL) {
            Ok(bytes) => self.control_out.extend(bytes),
            Err(_) => self.control_lost(now),
        }
    }

    pub fn control_data(&mut self, bytes: &[u8], now: f64) -> Vec<Event> {
        let mut events = Vec::new();
        if matches!(self.control, ControlState::Closed) || self.ended {
            return events;
        }
        self.control_reader.push(bytes);
        loop {
            let frame = match self.control_reader.next(MAX_CONTROL) {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(_) => {
                    self.control_lost(now);
                    events.push(Event::RelayOffline);
                    break;
                }
            };
            let Ok(reply) = serde_json::from_slice::<Reply>(&frame) else {
                self.control_lost(now);
                events.push(Event::RelayOffline);
                break;
            };
            match self.control {
                ControlState::Closed => break,
                ControlState::Opening => match reply {
                    Reply::Hosting { room } if valid_room(&room) => {
                        self.room = Some(room.clone());
                        self.control = ControlState::Hosting;
                        self.control_last_received = now;
                        self.control_last_alive = now;
                        self.backoff_ms = INITIAL_BACKOFF_MS;
                        let code = Code::from_indices(room, self.words).to_string();
                        events.push(Event::RoomCode(code));
                    }
                    Reply::Taken => {
                        self.room = None;
                        self.control = ControlState::Closed;
                        self.commands.push(SocketCommand::CloseControl);
                        self.commands
                            .push(SocketCommand::ConnectControl { at: now });
                        break;
                    }
                    Reply::Outdated { .. } => {
                        self.end_all();
                        events.push(Event::Fatal(OUTDATED.into()));
                        break;
                    }
                    _ => {
                        self.control_lost(now);
                        events.push(Event::RelayOffline);
                        break;
                    }
                },
                ControlState::Hosting => match reply {
                    Reply::Alive => {
                        self.control_last_received = now;
                    }
                    Reply::Incoming { ticket } => {
                        self.control_last_received = now;
                        self.spawn_accept(ticket, now);
                    }
                    _ => {
                        self.control_lost(now);
                        events.push(Event::RelayOffline);
                        break;
                    }
                },
            }
        }
        events
    }

    pub fn control_closed(&mut self, now: f64) -> Option<Event> {
        if matches!(self.control, ControlState::Closed) || self.ended {
            return None;
        }
        self.control_lost(now);
        Some(Event::RelayOffline)
    }

    pub fn accept_open(&mut self, id: u64, now: f64) {
        if self.ended {
            self.commands.push(SocketCommand::CloseAccept { id });
            return;
        }
        let Some(conn) = self.conns.get_mut(&id) else {
            self.commands.push(SocketCommand::CloseAccept { id });
            return;
        };
        let request = Request::Accept {
            version: relay_wire::VERSION,
            room: conn.room.clone(),
            secret: self.secret.clone(),
            ticket: conn.ticket,
        };
        match frame::encode_json(&request, MAX_CONTROL) {
            Ok(bytes) => {
                conn.out.extend(bytes);
                conn.state = ConnState::Accept;
                conn.since = now;
            }
            Err(_) => {
                self.conns.remove(&id);
                self.commands.push(SocketCommand::CloseAccept { id });
            }
        }
    }

    pub fn accept_data(&mut self, id: u64, bytes: &[u8], now: f64) -> Vec<Event> {
        let mut events = Vec::new();
        if self.ended || !self.conns.contains_key(&id) {
            return events;
        }
        if let Some(conn) = self.conns.get_mut(&id) {
            conn.reader.push(bytes);
        }
        loop {
            let max = match self.conns.get(&id) {
                Some(conn) if matches!(conn.state, ConnState::Accept) => MAX_CONTROL,
                Some(_) => MAX_FRAME,
                None => break,
            };
            let frame = match self.conns.get_mut(&id).unwrap().reader.next(max) {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(_) => {
                    if let Some(event) = self.drop_conn(id) {
                        events.push(event);
                    }
                    break;
                }
            };
            if !self.advance_conn(id, &frame, now, &mut events) {
                break;
            }
        }
        events
    }

    pub fn accept_closed(&mut self, id: u64) -> Option<Event> {
        self.conns.remove(&id)?;
        if self.active == Some(id) {
            self.active = None;
            return Some(Event::Disconnected);
        }
        None
    }

    pub fn send(&mut self, message: &Message) -> io::Result<()> {
        let Some(id) = self.active else {
            return Err(io::Error::other("Nenhum convidado conectado"));
        };
        let Some(conn) = self.conns.get_mut(&id) else {
            return Err(io::Error::other("Nenhum convidado conectado"));
        };
        let ConnState::Open { cipher, sent, .. } = &mut conn.state else {
            return Err(io::Error::other("Conexão não está aberta"));
        };
        let sealed = cipher.seal(*sent, message)?;
        *sent += 1;
        conn.out.extend(frame::encode(&sealed, MAX_FRAME)?);
        Ok(())
    }

    pub fn drop_guest(&mut self) {
        if let Some(id) = self.active.take() {
            self.conns.remove(&id);
            self.commands.push(SocketCommand::CloseAccept { id });
        }
    }

    pub fn leave(&mut self) {
        if let Some(id) = self.active {
            let _ = self.send(&Message::Bye);
            if let Some(conn) = self.conns.remove(&id)
                && !conn.out.is_empty()
            {
                self.pending_out.entry(id).or_default().extend(conn.out);
            }
            self.active = None;
            self.commands.push(SocketCommand::CloseAccept { id });
        }
        self.end_all();
    }

    pub fn tick(&mut self, now: f64) -> Vec<Event> {
        let mut events = Vec::new();
        if self.ended {
            return events;
        }
        match self.control {
            ControlState::Closed => {}
            ControlState::Opening => {
                if now - self.control_since > STEP_MS {
                    self.control_lost(now);
                    events.push(Event::RelayOffline);
                }
            }
            ControlState::Hosting => {
                if now - self.control_last_received > CONTROL_SILENCE_MS {
                    self.control_lost(now);
                    events.push(Event::RelayOffline);
                } else if now - self.control_last_alive >= KEEPALIVE_MS {
                    self.control_last_alive = now;
                    if let Ok(bytes) = frame::encode_json(&Request::Alive, MAX_CONTROL) {
                        self.control_out.extend(bytes);
                    }
                }
            }
        }
        let expired: Vec<u64> = self
            .conns
            .iter()
            .filter_map(|(&id, conn)| match &conn.state {
                ConnState::Open { last_received, .. } => {
                    (now - *last_received > SILENCE_MS).then_some(id)
                }
                _ => (now - conn.since > STEP_MS).then_some(id),
            })
            .collect();
        for id in expired {
            if let Some(event) = self.drop_conn(id) {
                events.push(event);
            }
        }
        events
    }

    fn slot_held(&self) -> bool {
        self.conns.values().any(|conn| conn.holds_slot)
    }

    fn spawn_accept(&mut self, ticket: u64, now: f64) {
        if self.ended || self.wrong >= MAX_WRONG_CODES || self.conns.len() >= MAX_PENDING_ACCEPTS {
            return;
        }
        let Some(room) = self.room.clone() else {
            return;
        };
        let id = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(
            id,
            Conn {
                ticket,
                room,
                state: ConnState::Connecting,
                reader: frame::Reader::default(),
                out: Vec::new(),
                since: now,
                holds_slot: false,
            },
        );
        self.commands.push(SocketCommand::OpenAccept { id });
    }

    fn advance_conn(&mut self, id: u64, frame: &[u8], now: f64, events: &mut Vec<Event>) -> bool {
        let Some(conn) = self.conns.get_mut(&id) else {
            return false;
        };
        let state = std::mem::replace(&mut conn.state, ConnState::Connecting);
        match state {
            ConnState::Connecting => {
                self.drop_conn(id);
                false
            }
            ConnState::Accept => {
                let Ok(Reply::Paired) = serde_json::from_slice::<Reply>(frame) else {
                    self.drop_conn(id);
                    return false;
                };
                if self.slot_held() {
                    self.drop_conn(id);
                    return false;
                }
                if let Some(conn) = self.conns.get_mut(&id) {
                    conn.holds_slot = true;
                    conn.state = ConnState::Intro;
                    conn.since = now;
                }
                true
            }
            ConnState::Intro => {
                let room = conn.room.clone();
                match frame.split_first() {
                    Some((&INTRO_PAKE, message)) => {
                        if self.wrong >= MAX_WRONG_CODES {
                            self.drop_conn(id);
                            return false;
                        }
                        let code = Code::from_indices(&room, self.words);
                        let (state, outbound) = secure::pake_start(&room, &code.to_string());
                        let mut answer = vec![INTRO_PAKE];
                        answer.extend(outbound);
                        let Ok(encoded) = frame::encode(&answer, MAX_FRAME) else {
                            self.drop_conn(id);
                            return false;
                        };
                        let Ok(psk) = secure::pake_finish(state, message) else {
                            self.drop_conn(id);
                            return false;
                        };
                        if let Some(conn) = self.conns.get_mut(&id) {
                            conn.out.extend(encoded);
                            conn.state = ConnState::Noise { psk, pake: true };
                            conn.since = now;
                        }
                        true
                    }
                    Some((&INTRO_RESUME, [])) => {
                        let psk = self.resume_key.unwrap_or_else(random_key);
                        if let Some(conn) = self.conns.get_mut(&id) {
                            conn.state = ConnState::Noise { psk, pake: false };
                            conn.since = now;
                        }
                        true
                    }
                    _ => {
                        self.drop_conn(id);
                        false
                    }
                }
            }
            ConnState::Noise { psk, pake } => match secure::respond(&psk, &self.name, frame) {
                Ok(responded) => {
                    let Ok(encoded) = frame::encode(&responded.answer, MAX_FRAME) else {
                        self.drop_conn(id);
                        return false;
                    };
                    if pake {
                        self.resume_key = Some(psk);
                        self.wrong = 0;
                    }
                    if let Some(conn) = self.conns.get_mut(&id) {
                        conn.out.extend(encoded);
                        conn.state = ConnState::Open {
                            cipher: responded.cipher,
                            sent: 0,
                            received: 0,
                            last_received: now,
                        };
                    }
                    self.active = Some(id);
                    events.push(Event::Connected {
                        peer: responded.peer,
                    });
                    true
                }
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    if let Ok(encoded) = frame::encode(&[WRONG_KEY], MAX_FRAME) {
                        self.pending_out.entry(id).or_default().extend(encoded);
                    }
                    self.drop_conn(id);
                    if pake {
                        self.wrong += 1;
                        if self.wrong >= MAX_WRONG_CODES {
                            self.end_all();
                            events.push(Event::Fatal(TOO_MANY_ATTEMPTS.into()));
                        } else {
                            events.push(Event::WrongCode);
                        }
                    }
                    false
                }
                Err(_) => {
                    self.drop_conn(id);
                    false
                }
            },
            ConnState::Open {
                cipher,
                sent,
                mut received,
                ..
            } => match cipher.open(received, frame) {
                Ok(Message::Bye) => {
                    self.conns.remove(&id);
                    if self.active == Some(id) {
                        self.active = None;
                    }
                    self.commands.push(SocketCommand::CloseAccept { id });
                    events.push(Event::PeerLeft);
                    false
                }
                Ok(message) => {
                    received += 1;
                    if let Some(conn) = self.conns.get_mut(&id) {
                        conn.state = ConnState::Open {
                            cipher,
                            sent,
                            received,
                            last_received: now,
                        };
                    }
                    events.push(Event::Message(Box::new(message), now));
                    true
                }
                Err(_) => {
                    if let Some(event) = self.drop_conn(id) {
                        events.push(event);
                    }
                    false
                }
            },
        }
    }

    fn drop_conn(&mut self, id: u64) -> Option<Event> {
        if let Some(conn) = self.conns.remove(&id)
            && !conn.out.is_empty()
        {
            self.pending_out.entry(id).or_default().extend(conn.out);
        }
        self.commands.push(SocketCommand::CloseAccept { id });
        if self.active == Some(id) {
            self.active = None;
            return Some(Event::Disconnected);
        }
        None
    }

    fn control_lost(&mut self, now: f64) {
        self.control = ControlState::Closed;
        self.control_out.clear();
        self.commands.push(SocketCommand::CloseControl);
        if !self.ended {
            let delay = self.backoff_ms;
            self.backoff_ms = (self.backoff_ms * 2.0).min(MAX_BACKOFF_MS);
            self.commands
                .push(SocketCommand::ConnectControl { at: now + delay });
        }
    }

    fn end_all(&mut self) {
        self.ended = true;
        self.control = ControlState::Closed;
        self.control_out.clear();
        self.commands.push(SocketCommand::CloseControl);
        let ids: Vec<u64> = self.conns.keys().copied().collect();
        for id in ids {
            if let Some(conn) = self.conns.remove(&id)
                && !conn.out.is_empty()
            {
                self.pending_out.entry(id).or_default().extend(conn.out);
            }
            self.commands.push(SocketCommand::CloseAccept { id });
        }
        self.active = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sync2gether_core::secure::Initiator;

    fn reply_bytes(reply: &Reply) -> Vec<u8> {
        frame::encode_json(reply, MAX_CONTROL).unwrap()
    }

    fn open_host_room(link: &mut HostLink, room: &str, now: f64) -> String {
        link.start(now);
        let cmds = link.take_commands();
        assert!(matches!(
            cmds.as_slice(),
            [SocketCommand::ConnectControl { .. }]
        ));
        link.control_open(now);
        let out = link.take_control_output();
        let mut reader = frame::Reader::default();
        reader.push(&out);
        let req: Request =
            serde_json::from_slice(&reader.next(MAX_CONTROL).unwrap().unwrap()).unwrap();
        assert!(matches!(
            req,
            Request::Host {
                version: relay_wire::VERSION,
                ..
            }
        ));
        let events = link.control_data(
            &reply_bytes(&Reply::Hosting { room: room.into() }),
            now + 1.0,
        );
        match events.as_slice() {
            [Event::RoomCode(code)] => code.clone(),
            other => panic!("esperado RoomCode, veio {other:?}"),
        }
    }

    #[test]
    fn wrong_codes_close_room_after_three_attempts() {
        let mut link = HostLink::with_identity("Navegador", "a".repeat(32), [1, 2], None, None);
        let code = open_host_room(&mut link, "4821", 0.0);
        let wrong_code = if code.ends_with("agua") {
            "4821-alho-amora"
        } else {
            "4821-agua-agua"
        };
        for attempt in 1..=MAX_WRONG_CODES {
            let now = f64::from(attempt) * 100.0;
            link.control_data(
                &reply_bytes(&Reply::Incoming {
                    ticket: u64::from(attempt),
                }),
                now,
            );
            let cmds = link.take_commands();
            let id = match cmds.as_slice() {
                [SocketCommand::OpenAccept { id }] => *id,
                other => panic!("esperado OpenAccept, veio {other:?}"),
            };
            link.accept_open(id, now);
            let _ = link.take_accept_output(id);
            link.accept_data(id, &reply_bytes(&Reply::Paired), now + 1.0);

            let (guest_pake, guest_msg) = secure::pake_start("4821", wrong_code);
            let mut intro = vec![INTRO_PAKE];
            intro.extend(guest_msg);
            link.accept_data(id, &frame::encode(&intro, MAX_FRAME).unwrap(), now + 2.0);
            let host_out = link.take_accept_output(id);
            let mut reader = frame::Reader::default();
            reader.push(&host_out);
            let host_intro = reader.next(MAX_FRAME).unwrap().unwrap();
            let guest_key = secure::pake_finish(guest_pake, &host_intro[1..]).unwrap();
            let (_init, first) = Initiator::start(&guest_key, "convidado").unwrap();
            let events =
                link.accept_data(id, &frame::encode(&first, MAX_FRAME).unwrap(), now + 3.0);
            let reject_out = link.take_accept_output(id);
            let mut reader = frame::Reader::default();
            reader.push(&reject_out);
            assert_eq!(reader.next(MAX_FRAME).unwrap().unwrap(), vec![WRONG_KEY]);
            if attempt < MAX_WRONG_CODES {
                assert!(matches!(events.as_slice(), [Event::WrongCode]));
                assert!(!link.ended());
            } else {
                assert!(matches!(events.as_slice(), [Event::Fatal(_)]));
                assert!(link.ended());
            }
            let _ = link.take_commands();
        }
    }

    #[test]
    fn control_reconnect_reclaims_room_and_handles_taken() {
        let mut link = HostLink::with_identity("Navegador", "b".repeat(32), [3, 4], None, None);
        let code = open_host_room(&mut link, "4821", 0.0);
        assert!(code.starts_with("4821-"));
        assert!(matches!(
            link.control_closed(100.0),
            Some(Event::RelayOffline)
        ));
        let cmds = link.take_commands();
        assert!(matches!(
            cmds.as_slice(),
            [SocketCommand::CloseControl, SocketCommand::ConnectControl { at }] if *at == 1100.0
        ));
        link.control_open(1100.0);
        let out = link.take_control_output();
        let mut reader = frame::Reader::default();
        reader.push(&out);
        let req: Request =
            serde_json::from_slice(&reader.next(MAX_CONTROL).unwrap().unwrap()).unwrap();
        assert!(matches!(req, Request::Host { room: Some(ref r), .. } if r == "4821"));
        link.control_data(&reply_bytes(&Reply::Taken), 1105.0);
        assert!(link.room().is_none());
        let cmds = link.take_commands();
        assert!(matches!(
            cmds.as_slice(),
            [SocketCommand::CloseControl, SocketCommand::ConnectControl { at }] if *at == 1105.0
        ));
    }
}

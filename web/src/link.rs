use std::io;
use sync2gether_core::{
    code::Code,
    frame,
    protocol::Message,
    relay_wire::{self, MAX_CONTROL, Reply, Request},
    secure::{self, Cipher, INTRO_PAKE, INTRO_RESUME, Initiator, Key, MAX_FRAME, Pake},
};

pub const JOIN_REPLY_MS: f64 = 15_000.0;
pub const STEP_MS: f64 = 10_000.0;
pub const SILENCE_MS: f64 = 5_000.0;
pub const FIRST_ENTRY_MS: f64 = 30_000.0;
pub const GONE_MS: f64 = 45_000.0;
pub const RETRY_MS: f64 = 1_000.0;

pub const OUTDATED: &str = "O servidor exige outra versão do sync2gether. Atualize a página.";

enum Step {
    Closed,
    Join,
    Pake(Pake),
    Noise(Box<Initiator>, Key),
    Open,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Unknown,
    Away,
    WrongKey,
    Outdated,
    Limited,
    Retry,
}

#[derive(Debug)]
pub enum Event {
    Connected { peer: String },
    Message(Box<Message>),
    Lost,
    PeerLeft,
    Retry { at: f64 },
    Fatal(String),
    Closed,
}

pub struct Link {
    code: Code,
    name: String,
    key: Option<Key>,
    reader: frame::Reader,
    step: Step,
    since: f64,
    last_received: f64,
    started: Option<f64>,
    gone_since: Option<f64>,
    cipher: Option<Cipher>,
    sent: u64,
    received: u64,
    out: Vec<u8>,
    ended: bool,
    stored_key: bool,
}

impl Link {
    pub fn new(code: Code, name: &str) -> Self {
        Self {
            code,
            name: sync2gether_core::device_name(name),
            key: None,
            reader: frame::Reader::default(),
            step: Step::Closed,
            since: 0.0,
            last_received: 0.0,
            started: None,
            gone_since: None,
            cipher: None,
            sent: 0,
            received: 0,
            out: Vec::new(),
            ended: false,
            stored_key: false,
        }
    }

    pub fn code(&self) -> &Code {
        &self.code
    }
    pub fn key(&self) -> Option<Key> {
        self.key
    }
    pub fn resume_with(&mut self, key: Key) {
        self.key = Some(key);
        self.stored_key = true;
    }
    pub fn is_open(&self) -> bool {
        matches!(self.step, Step::Open)
    }
    pub fn is_active(&self) -> bool {
        !matches!(self.step, Step::Closed)
    }
    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn open(&mut self, now: f64) -> io::Result<()> {
        self.reader = frame::Reader::default();
        self.cipher = None;
        self.sent = 0;
        self.received = 0;
        self.step = Step::Join;
        self.since = now;
        self.started.get_or_insert(now);
        let join = Request::Join {
            version: relay_wire::VERSION,
            room: self.code.room().to_owned(),
        };
        let bytes = frame::encode_json(&join, MAX_CONTROL)?;
        self.out.extend(bytes);
        Ok(())
    }

    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    pub fn send(&mut self, message: &Message) -> io::Result<()> {
        let Some(cipher) = &self.cipher else {
            return Err(io::Error::other("Sem conexão com o anfitrião"));
        };
        let sealed = cipher.seal(self.sent, message)?;
        self.sent += 1;
        self.out.extend(frame::encode(&sealed, MAX_FRAME)?);
        Ok(())
    }

    pub fn leave(&mut self) {
        self.ended = true;
        self.step = Step::Closed;
        self.cipher = None;
    }

    pub fn receive(&mut self, bytes: &[u8], now: f64) -> Vec<Event> {
        let mut events = Vec::new();
        if matches!(self.step, Step::Closed) {
            return events;
        }
        self.reader.push(bytes);
        loop {
            let max = if matches!(self.step, Step::Join) {
                MAX_CONTROL
            } else {
                MAX_FRAME
            };
            let frame = match self.reader.next(max) {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(_) => {
                    events.push(self.fail(Failure::Retry, now));
                    break;
                }
            };
            match self.advance(&frame, now) {
                Ok(Some(event)) => {
                    let stop = !matches!(event, Event::Connected { .. } | Event::Message(_));
                    events.push(event);
                    if stop {
                        break;
                    }
                }
                Ok(None) => {}
                Err(failure) => {
                    events.push(self.fail(failure, now));
                    break;
                }
            }
        }
        events
    }

    fn advance(&mut self, frame: &[u8], now: f64) -> Result<Option<Event>, Failure> {
        match std::mem::replace(&mut self.step, Step::Closed) {
            Step::Closed => Ok(None),
            Step::Join => {
                let reply: Reply = serde_json::from_slice(frame).map_err(|_| Failure::Retry)?;
                match reply {
                    Reply::Paired => {}
                    Reply::Unknown => return Err(Failure::Unknown),
                    Reply::Away => return Err(Failure::Away),
                    Reply::Limited => return Err(Failure::Limited),
                    Reply::Outdated { .. } => return Err(Failure::Outdated),
                    _ => return Err(Failure::Retry),
                }
                self.since = now;
                match self.key {
                    Some(key) => {
                        self.out.extend(encode(&[INTRO_RESUME])?);
                        self.start_noise(key)
                    }
                    None => {
                        let (state, outbound) =
                            secure::pake_start(self.code.room(), &self.code.to_string());
                        let mut intro = vec![INTRO_PAKE];
                        intro.extend(outbound);
                        self.out.extend(encode(&intro)?);
                        self.step = Step::Pake(state);
                        Ok(None)
                    }
                }
            }
            Step::Pake(state) => match frame.split_first() {
                Some((&INTRO_PAKE, message)) => {
                    let key = secure::pake_finish(state, message).map_err(|_| Failure::Retry)?;
                    self.since = now;
                    self.start_noise(key)
                }
                _ => Err(Failure::Retry),
            },
            Step::Noise(initiator, key) => match initiator.finish(frame) {
                Ok((cipher, peer)) => {
                    self.cipher = Some(cipher);
                    self.key = Some(key);
                    self.step = Step::Open;
                    self.last_received = now;
                    self.gone_since = None;
                    self.stored_key = false;
                    Ok(Some(Event::Connected { peer }))
                }
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    Err(Failure::WrongKey)
                }
                Err(_) => Err(Failure::Retry),
            },
            Step::Open => {
                self.step = Step::Open;
                let cipher = self.cipher.as_ref().ok_or(Failure::Retry)?;
                let Ok(message) = cipher.open(self.received, frame) else {
                    self.step = Step::Closed;
                    self.cipher = None;
                    return Ok(Some(self.lost()));
                };
                self.received += 1;
                self.last_received = now;
                if matches!(message, Message::Bye) {
                    self.leave();
                    return Ok(Some(Event::PeerLeft));
                }
                Ok(Some(Event::Message(Box::new(message))))
            }
        }
    }

    fn start_noise(&mut self, key: Key) -> Result<Option<Event>, Failure> {
        let (initiator, first) = Initiator::start(&key, &self.name).map_err(|_| Failure::Retry)?;
        self.out.extend(encode(&first)?);
        self.step = Step::Noise(Box::new(initiator), key);
        Ok(None)
    }

    fn lost(&mut self) -> Event {
        self.step = Step::Closed;
        self.cipher = None;
        if self.ended {
            Event::Closed
        } else {
            Event::Lost
        }
    }

    pub fn closed(&mut self, now: f64) -> Option<Event> {
        match self.step {
            Step::Closed => None,
            Step::Open => Some(self.lost()),
            _ => Some(self.fail(Failure::Retry, now)),
        }
    }

    pub fn tick(&mut self, now: f64) -> Option<Event> {
        let limit = match self.step {
            Step::Closed => return None,
            Step::Join => JOIN_REPLY_MS,
            Step::Pake(_) | Step::Noise(..) => STEP_MS,
            Step::Open => {
                if now - self.last_received > SILENCE_MS {
                    return Some(self.lost());
                }
                return None;
            }
        };
        (now - self.since > limit).then(|| self.fail(Failure::Retry, now))
    }

    fn fail(&mut self, failure: Failure, now: f64) -> Event {
        self.step = Step::Closed;
        self.cipher = None;
        if self.ended {
            return Event::Closed;
        }
        if self.stored_key && matches!(failure, Failure::WrongKey) {
            self.stored_key = false;
            self.key = None;
            return Event::Retry { at: now };
        }
        let message = if self.key.is_none() {
            let started = self.started.unwrap_or(now);
            match failure {
                Failure::Unknown => {
                    Some("Não há sala com esse código. Confira o número ou peça um código novo.")
                }
                Failure::WrongKey => {
                    Some("Código incorreto. Confira as palavras ou peça um código novo.")
                }
                Failure::Outdated => Some(OUTDATED),
                Failure::Limited => {
                    Some("O servidor está ocupado. Tente de novo em alguns minutos.")
                }
                Failure::Away | Failure::Retry if now - started > FIRST_ENTRY_MS => Some(
                    "Não foi possível falar com o anfitrião. Confira a internet e se a sala continua aberta.",
                ),
                Failure::Away | Failure::Retry => None,
            }
        } else {
            match failure {
                Failure::WrongKey => {
                    Some("A sala não aceita mais esta conexão. Entre de novo com o link.")
                }
                Failure::Outdated => Some(OUTDATED),
                Failure::Unknown if now - *self.gone_since.get_or_insert(now) > GONE_MS => {
                    Some("A sala foi encerrada pelo anfitrião.")
                }
                _ => None,
            }
        };
        match message {
            Some(message) => {
                self.ended = true;
                Event::Fatal(message.into())
            }
            None => Event::Retry { at: now + RETRY_MS },
        }
    }
}

fn encode(bytes: &[u8]) -> Result<Vec<u8>, Failure> {
    frame::encode(bytes, MAX_FRAME).map_err(|_| Failure::Retry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sync2gether_core::secure::{WRONG_KEY, respond};

    fn reply(reply: &Reply) -> Vec<u8> {
        frame::encode_json(reply, MAX_CONTROL).unwrap()
    }

    fn joined(link: &mut Link, now: f64) {
        link.open(now).unwrap();
        let _ = link.take_output();
        assert!(link.receive(&reply(&Reply::Paired), now).is_empty());
    }

    #[test]
    fn a_stale_stored_key_falls_back_to_the_code() {
        let code = Code::parse("4821-gato-lua").unwrap();
        let mut link = Link::new(code, "Firefox");
        link.resume_with([9; 32]);
        joined(&mut link, 0.0);
        let sent = link.take_output();
        assert_eq!(&sent[4..5], &[INTRO_RESUME]);
        let events = link.receive(&frame::encode(&[WRONG_KEY], MAX_FRAME).unwrap(), 10.0);
        assert!(matches!(events.as_slice(), [Event::Retry { at }] if *at == 10.0));
        assert!(link.key().is_none());
        joined(&mut link, 20.0);
        let sent = link.take_output();
        assert_eq!(&sent[4..5], &[INTRO_PAKE]);
    }

    #[test]
    fn first_entry_failures_become_messages_and_handshakes_time_out() {
        let code = Code::parse("4821-gato-lua").unwrap();
        let mut link = Link::new(code.clone(), "Firefox");
        link.open(0.0).unwrap();
        let events = link.receive(&reply(&Reply::Unknown), 1.0);
        assert!(matches!(events.as_slice(), [Event::Fatal(m)] if m.contains("Não há sala")));
        assert!(link.ended());

        let mut link = Link::new(code.clone(), "Firefox");
        link.open(0.0).unwrap();
        let events = link.receive(&reply(&Reply::Away), 1.0);
        assert!(matches!(events.as_slice(), [Event::Retry { .. }]));
        link.open(2.0).unwrap();
        assert!(link.tick(2.0 + JOIN_REPLY_MS + 1.0).is_some());
        link.open(FIRST_ENTRY_MS + 10.0).unwrap();
        let events = link.receive(&reply(&Reply::Away), FIRST_ENTRY_MS + 20.0);
        assert!(matches!(events.as_slice(), [Event::Fatal(m)] if m.contains("anfitrião")));

        let mut link = Link::new(code.clone(), "Firefox");
        joined(&mut link, 0.0);
        let intro = link.take_output();
        let mut reader = frame::Reader::default();
        reader.push(&intro);
        let intro = reader.next(MAX_FRAME).unwrap().unwrap();
        let (host, host_message) = secure::pake_start("4821", "4821-gato-lua");
        let key = secure::pake_finish(host, &intro[1..]).unwrap();
        let mut answer = vec![INTRO_PAKE];
        answer.extend(host_message);
        assert!(
            link.receive(&frame::encode(&answer, MAX_FRAME).unwrap(), 1.0)
                .is_empty()
        );
        let mut reader = frame::Reader::default();
        reader.push(&link.take_output());
        let first = reader.next(MAX_FRAME).unwrap().unwrap();
        let responded = respond(&key, "desktop", &first).unwrap();
        let events = link.receive(&frame::encode(&responded.answer, MAX_FRAME).unwrap(), 2.0);
        assert!(matches!(events.as_slice(), [Event::Connected { peer }] if peer == "desktop"));
        assert!(link.is_open());
        assert!(link.tick(2.0 + SILENCE_MS - 1.0).is_none());
        assert!(matches!(
            link.tick(2.0 + SILENCE_MS + 1.0),
            Some(Event::Lost)
        ));
    }
}

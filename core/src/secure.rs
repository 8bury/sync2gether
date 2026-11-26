use crate::protocol::{MAX_MESSAGE, Message};
use serde::{Deserialize, Serialize};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::{io, sync::Arc};

pub type Key = [u8; 32];
pub type Pake = Spake2<Ed25519Group>;

const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"sync2gether relay 1";
const TAG: usize = 16;
pub const MAX_FRAME: usize = MAX_MESSAGE + 256;

pub const INTRO_PAKE: u8 = 1;
pub const INTRO_RESUME: u8 = 2;
pub const WRONG_KEY: u8 = 0xff;

#[derive(Serialize, Deserialize)]
struct Hello {
    name: String,
}

pub fn rejected() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "Chave da sala não confere")
}
fn noise(error: snow::Error) -> io::Error {
    io::Error::other(error)
}

pub fn pake_start(room: &str, password: &str) -> (Pake, Vec<u8>) {
    Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(password.as_bytes()),
        &Identity::new(format!("sync2gether/{room}").as_bytes()),
    )
}
pub fn pake_finish(state: Pake, inbound: &[u8]) -> io::Result<Key> {
    let key = state
        .finish(inbound)
        .map_err(|_| io::Error::other("Mensagem SPAKE2 inválida"))?;
    key.try_into()
        .map_err(|_| io::Error::other("Chave SPAKE2 com tamanho inesperado"))
}

fn builder(psk: &Key) -> io::Result<snow::Builder<'_>> {
    snow::Builder::new(NOISE.parse().map_err(noise)?)
        .prologue(PROLOGUE)
        .map_err(noise)?
        .psk(0, psk)
        .map_err(noise)
}
fn hello(name: &str) -> io::Result<Vec<u8>> {
    serde_json::to_vec(&Hello { name: name.into() }).map_err(io::Error::other)
}
fn peer_name(payload: &[u8]) -> io::Result<String> {
    let hello: Hello = serde_json::from_slice(payload).map_err(io::Error::other)?;
    Ok(crate::device_name(&hello.name))
}
fn cipher(state: snow::HandshakeState) -> io::Result<Cipher> {
    Ok(Cipher(Arc::new(
        state.into_stateless_transport_mode().map_err(noise)?,
    )))
}

pub struct Initiator(snow::HandshakeState);
impl Initiator {
    pub fn start(psk: &Key, name: &str) -> io::Result<(Self, Vec<u8>)> {
        let mut state = builder(psk)?.build_initiator().map_err(noise)?;
        let mut buffer = vec![0; MAX_FRAME];
        let written = state
            .write_message(&hello(name)?, &mut buffer)
            .map_err(noise)?;
        buffer.truncate(written);
        Ok((Self(state), buffer))
    }
    pub fn finish(mut self, answer: &[u8]) -> io::Result<(Cipher, String)> {
        if answer == [WRONG_KEY] {
            return Err(rejected());
        }
        let mut buffer = vec![0; MAX_FRAME];
        let read = self
            .0
            .read_message(answer, &mut buffer)
            .map_err(|_| rejected())?;
        let peer = peer_name(&buffer[..read])?;
        Ok((cipher(self.0)?, peer))
    }
}

pub struct Responded {
    pub cipher: Cipher,
    pub peer: String,
    pub answer: Vec<u8>,
}
pub fn respond(psk: &Key, name: &str, first: &[u8]) -> io::Result<Responded> {
    let mut state = builder(psk)?.build_responder().map_err(noise)?;
    let mut buffer = vec![0; MAX_FRAME];
    let read = state
        .read_message(first, &mut buffer)
        .map_err(|_| rejected())?;
    let peer = peer_name(&buffer[..read])?;
    let written = state
        .write_message(&hello(name)?, &mut buffer)
        .map_err(noise)?;
    buffer.truncate(written);
    Ok(Responded {
        cipher: cipher(state)?,
        peer,
        answer: buffer,
    })
}

#[derive(Clone)]
pub struct Cipher(Arc<snow::StatelessTransportState>);
impl Cipher {
    pub fn seal(&self, nonce: u64, message: &Message) -> io::Result<Vec<u8>> {
        let plain = serde_json::to_vec(message).map_err(io::Error::other)?;
        if plain.len() > MAX_MESSAGE {
            return Err(io::Error::other("Mensagem excede o limite"));
        }
        let mut frame = vec![0; plain.len() + TAG];
        let written = self
            .0
            .write_message(nonce, &plain, &mut frame)
            .map_err(noise)?;
        frame.truncate(written);
        Ok(frame)
    }
    pub fn open(&self, nonce: u64, frame: &[u8]) -> io::Result<Message> {
        let mut plain = vec![0; frame.len()];
        let read = self
            .0
            .read_message(nonce, frame, &mut plain)
            .map_err(|_| io::Error::other("Mensagem cifrada inválida"))?;
        serde_json::from_slice(&plain[..read]).map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Control;

    fn keys(host_code: &str, guest_code: &str) -> (Key, Key) {
        let (host, host_message) = pake_start("4821", host_code);
        let (guest, guest_message) = pake_start("4821", guest_code);
        (
            pake_finish(host, &guest_message).unwrap(),
            pake_finish(guest, &host_message).unwrap(),
        )
    }

    #[test]
    fn handshake_steps_open_a_channel_and_reject_wrong_codes() {
        let (host_key, guest_key) = keys("4821-gato-lua", "4821-gato-lua");
        assert_eq!(host_key, guest_key);
        let (initiator, first) = Initiator::start(&guest_key, "navegador").unwrap();
        let responded = respond(&host_key, "desktop", &first).unwrap();
        assert_eq!(responded.peer, "navegador");
        let (guest, peer) = initiator.finish(&responded.answer).unwrap();
        assert_eq!(peer, "desktop");
        let sealed = guest
            .seal(0, &Message::Request(Control::Seek(4.5)))
            .unwrap();
        assert!(matches!(
            responded.cipher.open(0, &sealed).unwrap(),
            Message::Request(Control::Seek(4.5))
        ));
        assert!(responded.cipher.open(1, &sealed).is_err());

        let (host_key, guest_key) = keys("4821-gato-lua", "4821-gato-sol");
        let (initiator, first) = Initiator::start(&guest_key, "a").unwrap();
        let error = respond(&host_key, "b", &first).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        let error = initiator.finish(&[WRONG_KEY]).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}

use super::{Inbound, Outbound};
use crate::protocol::{MAX_MESSAGE, Message};
use serde::{Deserialize, Serialize};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::{io, sync::Arc};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub type Key = [u8; 32];

const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"sync2gether relay 1";
const TAG: usize = 16;
const MAX_FRAME: usize = MAX_MESSAGE + 256;

pub const INTRO_PAKE: u8 = 1;
pub const INTRO_RESUME: u8 = 2;
const WRONG_KEY: u8 = 0xff;

#[derive(Serialize, Deserialize)]
struct Hello {
    name: String,
}

pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(io::Error::other("Quadro cifrado com tamanho inválido"));
    }
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(bytes).await?;
    writer.flush().await
}
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Vec<u8>> {
    let size = reader.read_u32().await? as usize;
    if size == 0 || size > MAX_FRAME {
        return Err(io::Error::other("Quadro cifrado com tamanho inválido"));
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).await?;
    Ok(bytes)
}

fn rejected() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "Chave da sala não confere")
}
fn noise(error: snow::Error) -> io::Error {
    io::Error::other(error)
}

pub fn pake_start(room: &str, password: &str) -> (Spake2<Ed25519Group>, Vec<u8>) {
    Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(password.as_bytes()),
        &Identity::new(format!("sync2gether/{room}").as_bytes()),
    )
}
pub fn pake_finish(state: Spake2<Ed25519Group>, inbound: &[u8]) -> io::Result<Key> {
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
    Ok(crate::discovery::device_name(&hello.name))
}

pub struct Channel<R, W> {
    pub reader: SecureReader<R>,
    pub writer: SecureWriter<W>,
    pub peer: String,
}

fn channel<R, W>(
    reader: R,
    writer: W,
    state: snow::HandshakeState,
    peer: String,
) -> io::Result<Channel<R, W>> {
    let transport = Arc::new(state.into_stateless_transport_mode().map_err(noise)?);
    Ok(Channel {
        reader: SecureReader {
            inner: reader,
            transport: transport.clone(),
            nonce: 0,
        },
        writer: SecureWriter {
            inner: writer,
            transport,
            nonce: 0,
        },
        peer,
    })
}

pub async fn initiate<R, W>(
    mut reader: R,
    mut writer: W,
    psk: &Key,
    name: &str,
) -> io::Result<Channel<R, W>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut state = builder(psk)?.build_initiator().map_err(noise)?;
    let mut buffer = vec![0; MAX_FRAME];
    let written = state
        .write_message(&hello(name)?, &mut buffer)
        .map_err(noise)?;
    write_frame(&mut writer, &buffer[..written]).await?;
    let answer = read_frame(&mut reader).await?;
    if answer == [WRONG_KEY] {
        return Err(rejected());
    }
    let read = state
        .read_message(&answer, &mut buffer)
        .map_err(|_| rejected())?;
    let peer = peer_name(&buffer[..read])?;
    channel(reader, writer, state, peer)
}

pub async fn respond<R, W>(
    mut reader: R,
    mut writer: W,
    psk: &Key,
    name: &str,
) -> io::Result<Channel<R, W>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut state = builder(psk)?.build_responder().map_err(noise)?;
    let mut buffer = vec![0; MAX_FRAME];
    let first = read_frame(&mut reader).await?;
    let read = match state.read_message(&first, &mut buffer) {
        Ok(read) => read,
        Err(_) => {
            let _ = write_frame(&mut writer, &[WRONG_KEY]).await;
            return Err(rejected());
        }
    };
    let peer = peer_name(&buffer[..read])?;
    let written = state
        .write_message(&hello(name)?, &mut buffer)
        .map_err(noise)?;
    write_frame(&mut writer, &buffer[..written]).await?;
    channel(reader, writer, state, peer)
}

pub struct SecureReader<R> {
    inner: R,
    transport: Arc<snow::StatelessTransportState>,
    nonce: u64,
}
pub struct SecureWriter<W> {
    inner: W,
    transport: Arc<snow::StatelessTransportState>,
    nonce: u64,
}

impl<R: AsyncRead + Unpin + Send> Inbound for SecureReader<R> {
    async fn recv(&mut self) -> io::Result<Message> {
        let frame = read_frame(&mut self.inner).await?;
        let mut plain = vec![0; frame.len()];
        let read = self
            .transport
            .read_message(self.nonce, &frame, &mut plain)
            .map_err(|_| io::Error::other("Mensagem cifrada inválida"))?;
        self.nonce += 1;
        serde_json::from_slice(&plain[..read]).map_err(io::Error::other)
    }
}
impl<W: AsyncWrite + Unpin + Send> Outbound for SecureWriter<W> {
    async fn send(&mut self, message: &Message) -> io::Result<()> {
        let plain = serde_json::to_vec(message).map_err(io::Error::other)?;
        if plain.len() > MAX_MESSAGE {
            return Err(io::Error::other("Mensagem excede o limite"));
        }
        let mut frame = vec![0; plain.len() + TAG];
        let written = self
            .transport
            .write_message(self.nonce, &plain, &mut frame)
            .map_err(noise)?;
        self.nonce += 1;
        write_frame(&mut self.inner, &frame[..written]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Control;

    fn key(room: &str, host_code: &str, guest_code: &str) -> (Key, Key) {
        let (host, host_message) = pake_start(room, host_code);
        let (guest, guest_message) = pake_start(room, guest_code);
        (
            pake_finish(host, &guest_message).unwrap(),
            pake_finish(guest, &host_message).unwrap(),
        )
    }

    #[tokio::test]
    async fn same_code_opens_an_encrypted_channel_in_both_directions() {
        let (host_key, guest_key) = key("4821", "4821-gato-lua", "4821-gato-lua");
        assert_eq!(host_key, guest_key);
        let (a, b) = tokio::io::duplex(64 * 1024);
        let (ar, aw) = tokio::io::split(a);
        let (br, bw) = tokio::io::split(b);
        let (guest, host) = tokio::join!(
            initiate(ar, aw, &guest_key, "notebook"),
            respond(br, bw, &host_key, "desktop")
        );
        let (mut guest, mut host) = (guest.unwrap(), host.unwrap());
        assert_eq!(guest.peer, "desktop");
        assert_eq!(host.peer, "notebook");
        for _ in 0..3 {
            guest
                .writer
                .send(&Message::Request(Control::Seek(4.5)))
                .await
                .unwrap();
            assert!(matches!(
                host.reader.recv().await.unwrap(),
                Message::Request(Control::Seek(4.5))
            ));
            host.writer.send(&Message::Bye).await.unwrap();
            assert!(matches!(guest.reader.recv().await.unwrap(), Message::Bye));
        }
    }

    #[tokio::test]
    async fn wrong_code_is_reported_to_both_sides_without_a_channel() {
        let (host_key, guest_key) = key("4821", "4821-gato-lua", "4821-gato-sol");
        assert_ne!(host_key, guest_key);
        let (a, b) = tokio::io::duplex(64 * 1024);
        let (ar, aw) = tokio::io::split(a);
        let (br, bw) = tokio::io::split(b);
        let (guest, host) = tokio::join!(
            initiate(ar, aw, &guest_key, "notebook"),
            respond(br, bw, &host_key, "desktop")
        );
        assert_eq!(guest.err().unwrap().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(host.err().unwrap().kind(), io::ErrorKind::PermissionDenied);
    }

    #[tokio::test]
    async fn tampered_or_replayed_frames_are_rejected() {
        let (host_key, guest_key) = key("4821", "4821-gato-lua", "4821-gato-lua");
        let (a, b) = tokio::io::duplex(64 * 1024);
        let (ar, aw) = tokio::io::split(a);
        let (br, bw) = tokio::io::split(b);
        let (guest, host) = tokio::join!(
            initiate(ar, aw, &guest_key, "a"),
            respond(br, bw, &host_key, "b")
        );
        let (mut guest, host) = (guest.unwrap(), host.unwrap());
        let (mut capture, mut source) = tokio::io::duplex(64 * 1024);
        let mut writer = SecureWriter {
            inner: &mut source,
            transport: guest.writer.transport.clone(),
            nonce: 0,
        };
        writer.send(&Message::Bye).await.unwrap();
        let frame = read_frame(&mut capture).await.unwrap();
        let (mut feed, inbox) = tokio::io::duplex(64 * 1024);
        let mut reader = SecureReader {
            inner: inbox,
            transport: host.reader.transport.clone(),
            nonce: 0,
        };
        write_frame(&mut feed, &frame).await.unwrap();
        write_frame(&mut feed, &frame).await.unwrap();
        let mut tampered = frame.clone();
        tampered[0] ^= 1;
        assert!(matches!(reader.recv().await.unwrap(), Message::Bye));
        assert!(reader.recv().await.is_err(), "replayed frame accepted");
        let mut fresh = SecureReader {
            inner: {
                let (mut feed, inbox) = tokio::io::duplex(64 * 1024);
                write_frame(&mut feed, &tampered).await.unwrap();
                inbox
            },
            transport: host.reader.transport.clone(),
            nonce: 0,
        };
        assert!(fresh.recv().await.is_err(), "tampered frame accepted");
        drop(guest.writer.send(&Message::Bye).await);
    }
}

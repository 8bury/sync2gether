use super::{Inbound, Outbound};
use crate::protocol::Message;
use std::io;
pub use sync2gether_core::secure::{
    Cipher, INTRO_PAKE, INTRO_RESUME, Initiator, Key, MAX_FRAME, WRONG_KEY, pake_finish, pake_start,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

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

pub struct Channel<R, W> {
    pub reader: SecureReader<R>,
    pub writer: SecureWriter<W>,
    pub peer: String,
}

fn channel<R, W>(reader: R, writer: W, cipher: Cipher, peer: String) -> Channel<R, W> {
    Channel {
        reader: SecureReader {
            inner: reader,
            cipher: cipher.clone(),
            nonce: 0,
        },
        writer: SecureWriter {
            inner: writer,
            cipher,
            nonce: 0,
        },
        peer,
    }
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
    let (initiator, first) = Initiator::start(psk, name)?;
    write_frame(&mut writer, &first).await?;
    let answer = read_frame(&mut reader).await?;
    let (cipher, peer) = initiator.finish(&answer)?;
    Ok(channel(reader, writer, cipher, peer))
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
    let first = read_frame(&mut reader).await?;
    match sync2gether_core::secure::respond(psk, name, &first) {
        Ok(responded) => {
            write_frame(&mut writer, &responded.answer).await?;
            Ok(channel(reader, writer, responded.cipher, responded.peer))
        }
        Err(error) => {
            if error.kind() == io::ErrorKind::PermissionDenied {
                let _ = write_frame(&mut writer, &[WRONG_KEY]).await;
            }
            Err(error)
        }
    }
}

pub struct SecureReader<R> {
    inner: R,
    cipher: Cipher,
    nonce: u64,
}
pub struct SecureWriter<W> {
    inner: W,
    cipher: Cipher,
    nonce: u64,
}

impl<R: AsyncRead + Unpin + Send> Inbound for SecureReader<R> {
    async fn recv(&mut self) -> io::Result<Message> {
        let frame = read_frame(&mut self.inner).await?;
        let message = self.cipher.open(self.nonce, &frame)?;
        self.nonce += 1;
        Ok(message)
    }
}
impl<W: AsyncWrite + Unpin + Send> Outbound for SecureWriter<W> {
    async fn send(&mut self, message: &Message) -> io::Result<()> {
        let frame = self.cipher.seal(self.nonce, message)?;
        self.nonce += 1;
        write_frame(&mut self.inner, &frame).await
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
            cipher: guest.writer.cipher.clone(),
            nonce: 0,
        };
        writer.send(&Message::Bye).await.unwrap();
        let frame = read_frame(&mut capture).await.unwrap();
        let (mut feed, inbox) = tokio::io::duplex(64 * 1024);
        let mut reader = SecureReader {
            inner: inbox,
            cipher: host.reader.cipher.clone(),
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
            cipher: host.reader.cipher.clone(),
            nonce: 0,
        };
        assert!(fresh.recv().await.is_err(), "tampered frame accepted");
        drop(guest.writer.send(&Message::Bye).await);
    }
}

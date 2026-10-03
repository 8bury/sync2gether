//! TCP enquadrado e limitado. Criptografia depende da rede ou VPN utilizada.
use crate::protocol::{MAX_MESSAGE, Message, VERSION};
use std::{
    io,
    net::SocketAddr,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::{sleep, timeout},
};

pub mod rooms;

pub enum Event {
    ApprovalRequested {
        id: u64,
        name: String,
        address: SocketAddr,
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    ApprovalFinished {
        id: u64,
    },
    WaitingApproval,
    Rejected(String),

    Connected(mpsc::Sender<Message>),
    Message(Box<Message>, Instant),
    Disconnected,
    Error(String),
    DiscoveryUnavailable,
}

pub fn allowed_address(addr: SocketAddr) -> bool {
    addr.port() != 0
        && !addr.ip().is_unspecified()
        && !addr.ip().is_multicast()
        && match addr.ip() {
            std::net::IpAddr::V4(ip) => !ip.is_broadcast() && ip.octets()[0] != 0,
            std::net::IpAddr::V6(_) => true,
        }
}

pub async fn write_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    message: &Message,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(message).map_err(io::Error::other)?;
    if bytes.len() > MAX_MESSAGE {
        return Err(io::Error::other("Mensagem excede o limite"));
    }
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await
}
pub async fn read_message<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Message> {
    let size = reader.read_u32().await? as usize;
    if size == 0 || size > MAX_MESSAGE {
        return Err(io::Error::other("Tamanho de mensagem inválido"));
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}
async fn connection(stream: TcpStream, events: &mpsc::Sender<Event>, origin: Instant) {
    let _ = stream.set_nodelay(true);
    let (mut reader, mut writer) = stream.into_split();
    let (tx, mut rx) = mpsc::channel(32);
    if events.send(Event::Connected(tx)).await.is_err() {
        return;
    }
    let incoming = async {
        loop {
            match timeout(Duration::from_secs(3), read_message(&mut reader)).await {
                Ok(Ok(Message::Bye)) => break,
                Ok(Ok(message)) => {
                    if events
                        .send(Event::Message(Box::new(message), Instant::now()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                _ => break,
            }
        }
    };
    let outgoing = async {
        while let Some(mut message) = rx.recv().await {
            if let Message::Pong { sent_host_ms, .. } = &mut message {
                *sent_host_ms = origin.elapsed().as_secs_f64() * 1000.0;
            }
            if !matches!(
                timeout(Duration::from_secs(2), write_message(&mut writer, &message)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
    };
    tokio::select! { _ = incoming => {}, _ = outgoing => {} }
    let _ = events.send(Event::Disconnected).await;
}

pub async fn host(
    listener: TcpListener,
    key: String,
    events: mpsc::Sender<Event>,
    origin: Instant,
) {
    loop {
        let Ok((mut stream, addr)) = listener.accept().await else {
            break;
        };
        if !allowed_address(addr) {
            continue;
        }
        let hello = timeout(Duration::from_secs(3), read_message(&mut stream)).await;
        if !matches!(hello, Ok(Ok(Message::Hello { version: VERSION, key: ref supplied })) if supplied == &key)
        {
            continue;
        }
        if !matches!(
            timeout(
                Duration::from_secs(2),
                write_message(&mut stream, &Message::Welcome { version: VERSION })
            )
            .await,
            Ok(Ok(()))
        ) {
            continue;
        }
        connection(stream, &events, origin).await;
    }
}
pub async fn guest(addr: SocketAddr, key: String, events: mpsc::Sender<Event>, origin: Instant) {
    loop {
        if let Ok(Ok(mut stream)) = timeout(Duration::from_secs(3), TcpStream::connect(addr)).await
        {
            let hello = Message::Hello {
                version: VERSION,
                key: key.clone(),
            };
            let welcome = async {
                write_message(&mut stream, &hello).await?;
                read_message(&mut stream).await
            };
            match timeout(Duration::from_secs(3), welcome).await {
                Ok(Ok(Message::Welcome { version: VERSION })) => {
                    connection(stream, &events, origin).await
                }
                _ => {
                    let _ = events
                        .send(Event::Error(
                            "Sala recusou a conexão. Verifique o código e a versão do app.".into(),
                        ))
                        .await;
                }
            }
        } else {
            let _ = events.send(Event::Disconnected).await;
        }
        sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn frames_roundtrip_and_reject_oversized_input() {
        let (mut a, mut b) = tokio::io::duplex(MAX_MESSAGE * 2);
        write_message(
            &mut a,
            &Message::Request(crate::protocol::Control::Seek(12.5)),
        )
        .await
        .unwrap();
        assert!(matches!(
            read_message(&mut b).await.unwrap(),
            Message::Request(crate::protocol::Control::Seek(12.5))
        ));
        a.write_u32(MAX_MESSAGE as u32 + 1).await.unwrap();
        assert!(read_message(&mut b).await.is_err());
    }
    #[test]
    fn unicast_addresses_work_without_a_specific_vpn() {
        for address in [
            "127.0.0.1:7842",
            "100.64.1.2:7842",
            "100.127.255.254:7842",
            "[fd7a:115c:a1e0::1]:7842",
            "[::1]:7842",
            "192.168.1.1:7842",
            "26.1.2.3:9000",
            "8.8.8.8:7842",
        ] {
            assert!(allowed_address(address.parse().unwrap()));
        }
        for address in [
            "0.0.0.0:7842",
            "255.255.255.255:7842",
            "239.255.78.42:7842",
            "127.0.0.1:0",
            "[::]:7842",
        ] {
            assert!(!allowed_address(address.parse().unwrap()));
        }
    }
}

use super::{RelayStream, Server};
use futures_util::{Sink, Stream};
use std::{
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::{sleep, timeout},
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Bytes, Message,
        handshake::server::{Callback, ErrorResponse, Request, Response},
        http::StatusCode,
        protocol::WebSocketConfig,
    },
};

pub const PATH: &str = "/relay";
const UPGRADE: Duration = Duration::from_secs(5);
const MAX_WS_MESSAGE: usize = 24 * 1024;
const PENDING_UPGRADES: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub origins: Vec<String>,
    pub client_ip_header: Option<String>,
}

pub(crate) async fn serve(server: Arc<Server>, listener: TcpListener, config: Config) {
    let config = Arc::new(config);
    let upgrades = Arc::new(Semaphore::new(PENDING_UPGRADES));
    loop {
        let (tcp, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(_) => {
                sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let Ok(permit) = upgrades.clone().try_acquire_owned() else {
            continue;
        };
        let server = server.clone();
        let config = config.clone();
        tokio::spawn(async move {
            let upgraded = timeout(UPGRADE, upgrade(tcp, peer, &config)).await;
            drop(permit);
            let Ok(Some((socket, address))) = upgraded else {
                return;
            };
            let Some(admission) = server.admit(address) else {
                return;
            };
            let stream = RelayStream::Ws(Box::new(WsStream::new(socket)));
            let _ = server.handle_web(stream, admission).await;
        });
    }
}

async fn upgrade(
    tcp: TcpStream,
    peer: SocketAddr,
    config: &Config,
) -> Option<(WebSocketStream<TcpStream>, IpAddr)> {
    let _ = tcp.set_nodelay(true);
    let mut address = peer.ip().to_canonical();
    let gate = Gate {
        config,
        peer,
        address: &mut address,
    };
    let ws_config = WebSocketConfig::default()
        .max_message_size(Some(MAX_WS_MESSAGE))
        .max_frame_size(Some(MAX_WS_MESSAGE));
    let socket = tokio_tungstenite::accept_hdr_async_with_config(tcp, gate, Some(ws_config))
        .await
        .ok()?;
    Some((socket, address))
}

struct Gate<'a> {
    config: &'a Config,
    peer: SocketAddr,
    address: &'a mut IpAddr,
}

impl Callback for Gate<'_> {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        match client_address(request, self.config, self.peer) {
            Ok(address) => {
                *self.address = address;
                Ok(response)
            }
            Err(status) => Err(refuse(status)),
        }
    }
}

fn client_address(
    request: &Request,
    config: &Config,
    peer: SocketAddr,
) -> Result<IpAddr, StatusCode> {
    if request.uri().path() != PATH {
        return Err(StatusCode::NOT_FOUND);
    }
    let origin = request
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok());
    if !allowed_origin(&config.origins, origin) {
        return Err(StatusCode::FORBIDDEN);
    }
    let peer = peer.ip().to_canonical();
    match &config.client_ip_header {
        Some(header) if trusted_proxy(peer) => request
            .headers()
            .get(header.as_str())
            .and_then(|value| value.to_str().ok())
            .and_then(forwarded_client)
            .ok_or(StatusCode::BAD_REQUEST),
        _ => Ok(peer),
    }
}

fn refuse(status: StatusCode) -> ErrorResponse {
    let mut response = ErrorResponse::new(None);
    *response.status_mut() = status;
    response
}

pub fn allowed_origin(origins: &[String], origin: Option<&str>) -> bool {
    origins.is_empty()
        || origin.is_some_and(|origin| {
            origins
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(origin))
        })
}

pub fn trusted_proxy(peer: IpAddr) -> bool {
    match peer {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private(),
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local(),
    }
}

pub fn forwarded_client(value: &str) -> Option<IpAddr> {
    value
        .rsplit(',')
        .next()
        .map(str::trim)
        .and_then(|last| last.parse::<IpAddr>().ok())
        .map(|ip| ip.to_canonical())
}

pub(crate) struct WsStream {
    socket: WebSocketStream<TcpStream>,
    pending: Bytes,
}

impl WsStream {
    fn new(socket: WebSocketStream<TcpStream>) -> Self {
        Self {
            socket,
            pending: Bytes::new(),
        }
    }
}

impl AsyncRead for WsStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            if !self.pending.is_empty() {
                let take = self.pending.len().min(buf.remaining());
                let chunk = self.pending.split_to(take);
                buf.put_slice(&chunk);
                return Poll::Ready(Ok(()));
            }
            match ready!(Pin::new(&mut self.socket).poll_next(cx)) {
                Some(Ok(Message::Binary(bytes))) => self.pending = bytes,
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                Some(Ok(Message::Close(_))) | None => return Poll::Ready(Ok(())),
                Some(Ok(Message::Text(_))) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Mensagem de texto no relay",
                    )));
                }
                Some(Err(error)) => return Poll::Ready(Err(io::Error::other(error))),
            }
        }
    }
}

impl AsyncWrite for WsStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let socket = Pin::new(&mut self.socket);
        ready!(socket.poll_ready(cx)).map_err(io::Error::other)?;
        Pin::new(&mut self.socket)
            .start_send(Message::Binary(Bytes::copy_from_slice(buf)))
            .map_err(io::Error::other)?;
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket)
            .poll_flush(cx)
            .map_err(io::Error::other)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket)
            .poll_close(cx)
            .map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_and_forwarded_addresses_are_checked() {
        assert!(allowed_origin(&[], None));
        let origins = vec!["https://assistir.example".to_owned()];
        assert!(allowed_origin(&origins, Some("https://assistir.example")));
        assert!(!allowed_origin(&origins, Some("https://evil.example")));
        assert!(!allowed_origin(&origins, None));
        assert!(trusted_proxy("127.0.0.1".parse().unwrap()));
        assert!(trusted_proxy("10.0.1.2".parse().unwrap()));
        assert!(trusted_proxy("fd00::1".parse().unwrap()));
        assert!(!trusted_proxy("203.0.113.9".parse().unwrap()));
        assert_eq!(
            forwarded_client("198.51.100.1, 203.0.113.9"),
            Some("203.0.113.9".parse().unwrap())
        );
        assert_eq!(
            forwarded_client("::ffff:198.51.100.7"),
            Some("198.51.100.7".parse().unwrap())
        );
        assert_eq!(forwarded_client("unknown"), None);
    }

    #[test]
    fn upgrades_check_path_origin_and_trusted_forwarded_addresses() {
        let config = Config {
            origins: vec!["https://assistir.example".into()],
            client_ip_header: Some("x-real-ip".into()),
        };
        let request = |path: &str, origin: &str, forwarded: Option<&str>| {
            let mut builder = Request::builder().uri(path).header("origin", origin);
            if let Some(forwarded) = forwarded {
                builder = builder.header("x-real-ip", forwarded);
            }
            builder.body(()).unwrap()
        };
        let proxy: SocketAddr = "172.18.0.5:41000".parse().unwrap();
        let public: SocketAddr = "203.0.113.9:41000".parse().unwrap();
        let good = "https://assistir.example";
        assert_eq!(
            client_address(&request(PATH, good, Some("198.51.100.4")), &config, proxy),
            Ok("198.51.100.4".parse().unwrap())
        );
        assert_eq!(
            client_address(&request(PATH, good, Some("198.51.100.4")), &config, public),
            Ok(public.ip())
        );
        assert_eq!(
            client_address(&request(PATH, good, None), &config, proxy),
            Err(StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            client_address(&request("/outro", good, None), &config, public),
            Err(StatusCode::NOT_FOUND)
        );
        assert_eq!(
            client_address(
                &request(PATH, "https://evil.example", None),
                &config,
                public
            ),
            Err(StatusCode::FORBIDDEN)
        );
    }
}

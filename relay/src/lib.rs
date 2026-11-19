use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::HashMap,
    io,
    net::IpAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    time::{sleep, timeout},
};
use tokio_rustls::TlsAcceptor;

pub mod tls;

pub const VERSION: u32 = 1;
pub const PORT: u16 = 7843;
pub const MAX_CONTROL: usize = 1024;
pub const KEEPALIVE: Duration = Duration::from_secs(15);
pub const CONTROL_SILENCE: Duration = Duration::from_secs(45);
const FIRST_REQUEST: Duration = Duration::from_secs(5);
const WRITE: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    Host {
        version: u32,
        room: Option<String>,
        secret: String,
    },
    Alive,
    Accept {
        version: u32,
        room: String,
        secret: String,
        ticket: u64,
    },
    Join {
        version: u32,
        room: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    Hosting { room: String },
    Incoming { ticket: u64 },
    Alive,
    Paired,
    Unknown,
    Away,
    Taken,
    Limited,
    Outdated { version: u32 },
}

pub fn valid_room(room: &str) -> bool {
    (4..=8).contains(&room.len())
        && room.bytes().all(|b| b.is_ascii_digit())
        && !room.starts_with('0')
}
pub fn valid_secret(secret: &str) -> bool {
    secret.len() == 32 && secret.bytes().all(|b| b.is_ascii_hexdigit())
}
fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0, |acc, (x, y)| acc | (x ^ y))
            == 0
}

pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    message: &T,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(message).map_err(io::Error::other)?;
    if bytes.len() > MAX_CONTROL {
        return Err(io::Error::other("Mensagem de controle excede o limite"));
    }
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await
}
pub async fn read_frame<R: AsyncRead + Unpin, T: DeserializeOwned>(
    reader: &mut R,
) -> io::Result<T> {
    let size = reader.read_u32().await? as usize;
    if size == 0 || size > MAX_CONTROL {
        return Err(io::Error::other("Tamanho de mensagem de controle inválido"));
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

#[derive(Clone, Debug)]
pub struct Limits {
    pub connections: usize,
    pub per_address: usize,
    pub rooms: usize,
    pub rooms_per_address: usize,
    pub grace: Duration,
    pub accept: Duration,
    pub idle: Duration,
    pub bytes_per_second: usize,
    pub pending_per_room: usize,
    pub pipes_per_room: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            connections: 4096,
            per_address: 32,
            rooms: 10_000,
            rooms_per_address: 8,
            grace: Duration::from_secs(120),
            accept: Duration::from_secs(10),
            idle: Duration::from_secs(30),
            bytes_per_second: 256 * 1024,
            pending_per_room: 4,
            pipes_per_room: 2,
        }
    }
}

enum RelayStream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::server::TlsStream<TcpStream>>),
}

impl AsyncRead for RelayStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            Self::Tls(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for RelayStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            Self::Tls(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_flush(cx),
            Self::Tls(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            Self::Tls(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

struct Room {
    secret: String,
    owner: IpAddr,
    control: Option<(u64, mpsc::Sender<Reply>)>,
    pending: usize,
    pipes: usize,
}
struct Ticket {
    room: String,
    deliver: oneshot::Sender<(RelayStream, Admission)>,
}
#[derive(Default)]
struct State {
    rooms: HashMap<String, Room>,
    tickets: HashMap<u64, Ticket>,
    addresses: HashMap<IpAddr, usize>,
    connections: usize,
    pipes: usize,
    next: u64,
}
struct Server {
    limits: Limits,
    tls: Option<TlsAcceptor>,
    state: Mutex<State>,
}

struct Admission {
    server: Arc<Server>,
    address: IpAddr,
}
impl Drop for Admission {
    fn drop(&mut self) {
        let mut state = self.server.state.lock().unwrap();
        state.connections -= 1;
        if let Some(count) = state.addresses.get_mut(&self.address) {
            *count -= 1;
            if *count == 0 {
                state.addresses.remove(&self.address);
            }
        }
    }
}

enum End {
    Closed,
    Lost,
    Replaced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    pub connections: usize,
    pub rooms: usize,
    pub pipes: usize,
}

pub async fn serve(listener: TcpListener, limits: Limits) {
    serve_with_stats(listener, limits, |_| {}).await
}

pub async fn serve_with_stats(
    listener: TcpListener,
    limits: Limits,
    report: impl Fn(Stats) + Send + 'static,
) {
    serve_inner(listener, limits, None, report).await
}

pub async fn serve_tls(listener: TcpListener, limits: Limits, tls: TlsAcceptor) {
    serve_tls_with_stats(listener, limits, tls, |_| {}).await
}

pub async fn serve_tls_with_stats(
    listener: TcpListener,
    limits: Limits,
    tls: TlsAcceptor,
    report: impl Fn(Stats) + Send + 'static,
) {
    serve_inner(listener, limits, Some(tls), report).await
}

async fn serve_inner(
    listener: TcpListener,
    limits: Limits,
    tls: Option<TlsAcceptor>,
    report: impl Fn(Stats) + Send + 'static,
) {
    let server = Arc::new(Server {
        limits,
        tls,
        state: Mutex::new(State::default()),
    });
    let reporting = server.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        interval.tick().await;
        loop {
            interval.tick().await;
            report(reporting.stats());
        }
    });
    loop {
        let (stream, address) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(_) => {
                sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let Some(admission) = server.admit(address.ip().to_canonical()) else {
            continue;
        };
        let server = server.clone();
        tokio::spawn(async move {
            let _ = server.handle(stream, admission).await;
        });
    }
}

impl Server {
    fn stats(&self) -> Stats {
        let state = self.state.lock().unwrap();
        Stats {
            connections: state.connections,
            rooms: state.rooms.len(),
            pipes: state.pipes,
        }
    }
    fn admit(self: &Arc<Self>, address: IpAddr) -> Option<Admission> {
        let mut state = self.state.lock().unwrap();
        let from_address = state.addresses.get(&address).copied().unwrap_or(0);
        if state.connections >= self.limits.connections || from_address >= self.limits.per_address {
            return None;
        }
        state.connections += 1;
        *state.addresses.entry(address).or_default() += 1;
        Some(Admission {
            server: self.clone(),
            address,
        })
    }

    async fn handle(self: Arc<Self>, tcp: TcpStream, admission: Admission) -> io::Result<()> {
        self.handle_with_timeout(tcp, admission, FIRST_REQUEST)
            .await
    }

    async fn handle_with_timeout(
        self: Arc<Self>,
        tcp: TcpStream,
        admission: Admission,
        first_request: Duration,
    ) -> io::Result<()> {
        let _ = tcp.set_nodelay(true);
        let mut stream = match &self.tls {
            Some(acceptor) => {
                let tls = timeout(first_request, acceptor.accept(tcp)).await??;
                RelayStream::Tls(Box::new(tls))
            }
            None => RelayStream::Plain(tcp),
        };
        let request: Request = timeout(first_request, read_frame(&mut stream)).await??;
        let version = match &request {
            Request::Host { version, .. }
            | Request::Accept { version, .. }
            | Request::Join { version, .. } => *version,
            Request::Alive => return Ok(()),
        };
        if version != VERSION {
            return reply(&mut stream, &Reply::Outdated { version: VERSION }).await;
        }
        match request {
            Request::Host { room, secret, .. } => self.host(stream, admission, room, secret).await,
            Request::Accept {
                room,
                secret,
                ticket,
                ..
            } => self.accept(stream, admission, room, secret, ticket).await,
            Request::Join { room, .. } => self.join(stream, room).await,
            Request::Alive => Ok(()),
        }
    }

    fn new_room(&self, state: &State) -> Option<String> {
        for (low, high) in [
            (1_000u32, 9_999u32),
            (100_000, 999_999),
            (10_000_000, 99_999_999),
        ] {
            for _ in 0..32 {
                let room = rand::random_range(low..=high).to_string();
                if !state.rooms.contains_key(&room) {
                    return Some(room);
                }
            }
        }
        None
    }

    async fn host(
        self: Arc<Self>,
        mut stream: RelayStream,
        admission: Admission,
        requested: Option<String>,
        secret: String,
    ) -> io::Result<()> {
        if !valid_secret(&secret) || requested.as_deref().is_some_and(|room| !valid_room(room)) {
            return reply(&mut stream, &Reply::Unknown).await;
        }
        let (tx, mut rx) = mpsc::channel(8);
        let weak = tx.downgrade();
        let attached = {
            let mut state = self.state.lock().unwrap();
            state.next += 1;
            let id = state.next;
            let owned = state
                .rooms
                .values()
                .filter(|room| room.owner == admission.address)
                .count();
            let can_create =
                state.rooms.len() < self.limits.rooms && owned < self.limits.rooms_per_address;
            match requested {
                Some(room) => match state.rooms.get_mut(&room) {
                    Some(existing) if same_secret(&existing.secret, &secret) => {
                        existing.control = Some((id, tx));
                        existing.owner = admission.address;
                        Ok((room, id))
                    }
                    Some(_) => Err(Reply::Taken),
                    None if can_create => {
                        state.rooms.insert(
                            room.clone(),
                            Room {
                                secret,
                                owner: admission.address,
                                control: Some((id, tx)),
                                pending: 0,
                                pipes: 0,
                            },
                        );
                        Ok((room, id))
                    }
                    None => Err(Reply::Limited),
                },
                None if can_create => match self.new_room(&state) {
                    Some(room) => {
                        state.rooms.insert(
                            room.clone(),
                            Room {
                                secret,
                                owner: admission.address,
                                control: Some((id, tx)),
                                pending: 0,
                                pipes: 0,
                            },
                        );
                        Ok((room, id))
                    }
                    None => Err(Reply::Limited),
                },
                None => Err(Reply::Limited),
            }
        };
        let (room, id) = match attached {
            Ok(attached) => attached,
            Err(answer) => return reply(&mut stream, &answer).await,
        };
        let end = if reply(&mut stream, &Reply::Hosting { room: room.clone() })
            .await
            .is_err()
        {
            End::Lost
        } else {
            let (mut reader, mut writer) = tokio::io::split(stream);
            let reading = async {
                loop {
                    match timeout(CONTROL_SILENCE, read_frame::<_, Request>(&mut reader)).await {
                        Ok(Ok(Request::Alive)) => {
                            if let Some(tx) = weak.upgrade() {
                                let _ = tx.try_send(Reply::Alive);
                            }
                        }
                        Ok(Err(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
                            return End::Closed;
                        }
                        _ => return End::Lost,
                    }
                }
            };
            let writing = async {
                while let Some(message) = rx.recv().await {
                    if !matches!(
                        timeout(WRITE, write_frame(&mut writer, &message)).await,
                        Ok(Ok(()))
                    ) {
                        return End::Lost;
                    }
                }
                End::Replaced
            };
            tokio::select! { end = reading => end, end = writing => end }
        };
        self.detach(&room, id, end);
        drop(admission);
        Ok(())
    }

    fn detach(self: &Arc<Self>, room: &str, id: u64, end: End) {
        let mut state = self.state.lock().unwrap();
        let Some(existing) = state.rooms.get_mut(room) else {
            return;
        };
        if existing
            .control
            .as_ref()
            .is_none_or(|(current, _)| *current != id)
        {
            return;
        }
        existing.control = None;
        match end {
            End::Closed => {
                state.rooms.remove(room);
            }
            End::Replaced => {}
            End::Lost => {
                let server = self.clone();
                let room = room.to_owned();
                let grace = self.limits.grace;
                tokio::spawn(async move {
                    sleep(grace).await;
                    let mut state = server.state.lock().unwrap();
                    if state
                        .rooms
                        .get(&room)
                        .is_some_and(|existing| existing.control.is_none())
                    {
                        state.rooms.remove(&room);
                    }
                });
            }
        }
    }

    async fn join(self: Arc<Self>, mut stream: RelayStream, room: String) -> io::Result<()> {
        if !valid_room(&room) {
            return reply(&mut stream, &Reply::Unknown).await;
        }
        let ticket = rand::random::<u64>();
        let (deliver, delivered) = oneshot::channel();
        let announced = {
            let mut state = self.state.lock().unwrap();
            let limits = &self.limits;
            match state.rooms.get_mut(&room) {
                None => Err(Reply::Unknown),
                Some(existing) => match &existing.control {
                    None => Err(Reply::Away),
                    Some(_)
                        if existing.pending >= limits.pending_per_room
                            || existing.pipes >= limits.pipes_per_room =>
                    {
                        Err(Reply::Limited)
                    }
                    Some((_, tx)) => {
                        if tx.try_send(Reply::Incoming { ticket }).is_ok() {
                            existing.pending += 1;
                            state.tickets.insert(
                                ticket,
                                Ticket {
                                    room: room.clone(),
                                    deliver,
                                },
                            );
                            Ok(())
                        } else {
                            Err(Reply::Away)
                        }
                    }
                },
            }
        };
        if let Err(answer) = announced {
            return reply(&mut stream, &answer).await;
        }
        let host = timeout(self.limits.accept, delivered).await;
        {
            let mut state = self.state.lock().unwrap();
            state.tickets.remove(&ticket);
            if let Some(existing) = state.rooms.get_mut(&room) {
                existing.pending -= 1;
            }
        }
        let Ok(Ok((mut host, host_admission))) = host else {
            return reply(&mut stream, &Reply::Away).await;
        };
        reply(&mut host, &Reply::Paired).await?;
        reply(&mut stream, &Reply::Paired).await?;
        {
            let mut state = self.state.lock().unwrap();
            state.pipes += 1;
            if let Some(existing) = state.rooms.get_mut(&room) {
                existing.pipes += 1;
            }
        }
        let (guest_read, guest_write) = tokio::io::split(stream);
        let (host_read, host_write) = tokio::io::split(host);
        let limits = self.limits.clone();
        tokio::select! {
            _ = pump(guest_read, host_write, &limits) => {}
            _ = pump(host_read, guest_write, &limits) => {}
        }
        {
            let mut state = self.state.lock().unwrap();
            state.pipes -= 1;
            if let Some(existing) = state.rooms.get_mut(&room) {
                existing.pipes = existing.pipes.saturating_sub(1);
            }
        }
        drop(host_admission);
        Ok(())
    }

    async fn accept(
        self: Arc<Self>,
        mut stream: RelayStream,
        admission: Admission,
        room: String,
        secret: String,
        ticket: u64,
    ) -> io::Result<()> {
        let ticket = {
            let mut state = self.state.lock().unwrap();
            let authorized = state.tickets.get(&ticket).is_some_and(|pending| {
                pending.room == room
                    && state
                        .rooms
                        .get(&room)
                        .is_some_and(|existing| same_secret(&existing.secret, &secret))
            });
            if authorized {
                state.tickets.remove(&ticket)
            } else {
                None
            }
        };
        match ticket {
            Some(ticket) => {
                if let Err((mut stream, _)) = ticket.deliver.send((stream, admission)) {
                    return reply(&mut stream, &Reply::Unknown).await;
                }
                Ok(())
            }
            None => reply(&mut stream, &Reply::Unknown).await,
        }
    }
}

async fn reply(stream: &mut RelayStream, message: &Reply) -> io::Result<()> {
    timeout(WRITE, write_frame(stream, message)).await?
}

async fn pump<R, W>(mut from: R, mut to: W, limits: &Limits) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0; 16 * 1024];
    let rate = limits.bytes_per_second.max(1) as f64;
    let mut allowance = rate;
    let mut last = Instant::now();
    loop {
        let read = timeout(limits.idle, from.read(&mut buffer)).await??;
        if read == 0 {
            return Ok(());
        }
        let now = Instant::now();
        allowance = (allowance + now.duration_since(last).as_secs_f64() * rate).min(rate);
        last = now;
        allowance -= read as f64;
        if allowance < 0.0 {
            sleep(Duration::from_secs_f64(-allowance / rate)).await;
        }
        timeout(limits.idle, async {
            to.write_all(&buffer[..read]).await?;
            to.flush().await
        })
        .await??;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooms_and_secrets_are_validated() {
        for room in ["1000", "4821", "99999999"] {
            assert!(valid_room(room));
        }
        for room in ["", "123", "0123", "123456789", "12a4", " 1234"] {
            assert!(!valid_room(room));
        }
        assert!(valid_secret(&"a".repeat(32)));
        assert!(!valid_secret(&"g".repeat(32)));
        assert!(!valid_secret(&"a".repeat(31)));
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
    }

    #[tokio::test]
    async fn control_frames_roundtrip_and_reject_oversized_input() {
        let (mut a, mut b) = tokio::io::duplex(MAX_CONTROL * 2);
        write_frame(
            &mut a,
            &Request::Join {
                version: VERSION,
                room: "4821".into(),
            },
        )
        .await
        .unwrap();
        let request: Request = read_frame(&mut b).await.unwrap();
        assert_eq!(
            request,
            Request::Join {
                version: VERSION,
                room: "4821".into()
            }
        );
        a.write_u32(MAX_CONTROL as u32 + 1).await.unwrap();
        assert!(read_frame::<_, Request>(&mut b).await.is_err());
    }

    #[tokio::test]
    async fn stalled_tls_handshake_times_out_and_releases_connection_slot() {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let acceptor = tls::acceptor_from_pem(
            cert.cert.pem().as_bytes(),
            cert.signing_key.serialize_pem().as_bytes(),
        )
        .unwrap();
        let server = Arc::new(Server {
            limits: Limits::default(),
            tls: Some(acceptor),
            state: Mutex::new(State::default()),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _stalled_client = TcpStream::connect(addr).await.unwrap();
        let (tcp, peer) = listener.accept().await.unwrap();
        let admission = server.admit(peer.ip().to_canonical()).unwrap();
        assert_eq!(server.stats().connections, 1);
        let result = server
            .clone()
            .handle_with_timeout(tcp, admission, Duration::from_millis(50))
            .await;
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert_eq!(server.stats().connections, 0);
    }
}

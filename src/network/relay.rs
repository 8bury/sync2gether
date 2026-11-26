use super::{
    Ended, Event, exchange,
    secure::{self, Channel, INTRO_PAKE, INTRO_RESUME, Key},
};
use rustls::pki_types::{CertificateDer, ServerName};
use std::{
    fmt,
    io::{self, Cursor},
    net::{IpAddr, Ipv6Addr, SocketAddr},
    pin::Pin,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU32, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};
use sync2gether_relay::{self as wire, Reply, Request};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf, ReadHalf, WriteHalf},
    net::TcpStream,
    sync::{Semaphore, mpsc},
    task::JoinSet,
    time::{sleep, timeout},
};
use tokio_rustls::TlsConnector;

pub const DEFAULT_RELAY: &str = "relay.pedrosales.dev:7843";
pub const MAX_WRONG_CODES: u32 = 3;
const CONNECT: Duration = Duration::from_secs(5);
const STEP: Duration = Duration::from_secs(10);
const JOIN_REPLY: Duration = Duration::from_secs(15);
const FIRST_ENTRY: Duration = Duration::from_secs(30);
const GONE: Duration = Duration::from_secs(45);

const WORDS: [&str; 256] = [
    "agua", "alho", "amora", "anel", "anjo", "arco", "areia", "arroz", "asa", "aveia", "azul",
    "bala", "balde", "banco", "barco", "bico", "bolo", "bola", "bota", "brisa", "cabra", "cacau",
    "caju", "calor", "cama", "campo", "caneca", "canoa", "capa", "carta", "casa", "cavalo",
    "cedro", "cebola", "cereja", "chave", "chuva", "cinema", "circo", "cisne", "coco", "cofre",
    "cola", "cometa", "copo", "coral", "corda", "coruja", "cravo", "cubo", "dado", "dedal",
    "disco", "doce", "dueto", "duna", "eco", "elmo", "escova", "estrela", "faca", "fada", "farol",
    "fava", "feira", "fenda", "ferro", "festa", "figo", "filme", "flauta", "flor", "foca", "fogo",
    "folha", "fonte", "forno", "fruta", "funil", "gaita", "galo", "garfo", "gato", "gelo",
    "girafa", "globo", "goiaba", "gota", "grama", "grilo", "gruta", "guia", "harpa", "hino",
    "horta", "ilha", "janela", "jarra", "jardim", "jipe", "joia", "jogo", "kiwi", "lago", "lama",
    "lanche", "laranja", "lata", "leite", "lenha", "leque", "lima", "lince", "linha", "livro",
    "lobo", "lontra", "lousa", "lua", "luva", "macaco", "mala", "manga", "mapa", "mar", "massa",
    "mel", "menta", "mesa", "milho", "moeda", "mola", "morango", "motor", "mula", "muro", "nabo",
    "navio", "neve", "ninho", "noz", "nuvem", "oca", "ovo", "onda", "ostra", "ouro", "palco",
    "palha", "pano", "papel", "pato", "pedra", "peixe", "pena", "pera", "peteca", "piano", "pilha",
    "pincel", "pinha", "pipa", "pipoca", "pires", "placa", "planta", "pneu", "polvo", "ponte",
    "porta", "pote", "praia", "prato", "queijo", "quiabo", "raio", "ramo", "raposa", "rede",
    "remo", "rio", "roda", "rosa", "roupa", "rua", "sacola", "sal", "salsa", "sapo", "selo",
    "serra", "sino", "sol", "sopa", "tatu", "teia", "telha", "tenda", "terra", "tigre", "tijolo",
    "tinta", "toalha", "tomate", "torre", "trem", "trevo", "trigo", "tucano", "tulipa", "uva",
    "vaca", "vagem", "vale", "vaso", "vela", "vento", "verde", "vidro", "vinho", "viola", "xale",
    "xadrez", "zebra", "baleia", "bambu", "banana", "barro", "batata", "bigode", "bonde", "bosque",
    "broto", "bule", "cacto", "cadeira", "caderno", "camelo", "caneta", "canela", "cesta",
    "chinelo", "colher", "concha", "cupim", "delta", "elefante", "esquilo", "fivela", "formiga",
    "galho", "girassol", "iogurte", "lagarto", "lagoa", "marfim",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    room: String,
    words: [&'static str; 2],
}
impl Code {
    #[cfg(test)]
    fn random(room: String) -> Self {
        Self {
            room,
            words: random_words(),
        }
    }
    pub fn parse(input: &str) -> Option<Self> {
        let folded: String = input.trim().chars().map(fold).collect();
        let parts: Vec<&str> = folded
            .split(|c: char| c == '-' || c == '.' || c.is_whitespace())
            .filter(|part| !part.is_empty())
            .collect();
        let [room, first, second] = parts.as_slice() else {
            return None;
        };
        let word = |input: &str| WORDS.iter().copied().find(|word| *word == input);
        wire::valid_room(room).then_some(())?;
        Some(Self {
            room: (*room).to_owned(),
            words: [word(first)?, word(second)?],
        })
    }
    pub fn room(&self) -> &str {
        &self.room
    }
}
impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.room, self.words[0], self.words[1])
    }
}
fn random_words() -> [&'static str; 2] {
    [
        WORDS[rand::random::<u8>() as usize],
        WORDS[rand::random::<u8>() as usize],
    ]
}
fn fold(c: char) -> char {
    match c.to_lowercase().next().unwrap_or(c) {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        other => other,
    }
}

#[derive(Clone)]
pub struct ClientTls(Arc<rustls::ClientConfig>);

impl ClientTls {
    pub fn webpki() -> Self {
        Self(default_tls_config())
    }

    pub fn from_roots(roots: rustls::RootCertStore) -> io::Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self(Arc::new(config)))
    }

    pub fn from_pem(ca_pem: &[u8]) -> io::Result<Self> {
        let mut reader = Cursor::new(ca_pem);
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
            .collect::<Result<_, _>>()
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    (crate::i18n::text().errors.invalid_ca)(&error.to_string()),
                )
            })?;
        if certs.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                crate::i18n::text().errors.no_ca_found,
            ));
        }
        let mut roots = rustls::RootCertStore::empty();
        let (added, _) = roots.add_parsable_certificates(certs);
        if added == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                crate::i18n::text().errors.no_valid_ca,
            ));
        }
        Self::from_roots(roots)
    }
}

impl Default for ClientTls {
    fn default() -> Self {
        Self::webpki()
    }
}

fn default_tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            Arc::new(
                rustls::ClientConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .expect("versões TLS padrão")
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            )
        })
        .clone()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Endpoint {
    host: String,
    port: u16,
    tls: bool,
    scheme: Option<&'static str>,
}

impl Endpoint {
    fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        let invalid = || crate::i18n::text().errors.relay_format.to_owned();
        let (scheme, rest) = match input.split_once("://") {
            Some((scheme, rest)) => {
                let scheme = if scheme.eq_ignore_ascii_case("tls") {
                    "tls"
                } else if scheme.eq_ignore_ascii_case("tcp") {
                    "tcp"
                } else {
                    return Err(invalid());
                };
                (Some(scheme), rest)
            }
            None => (None, input),
        };
        let (host, port, is_ip) = if let Ok(addr) = rest.parse::<SocketAddr>() {
            if addr.port() == 0 {
                return Err(invalid());
            }
            (addr.ip().to_string(), addr.port(), true)
        } else if let Ok(ip) = rest.parse::<IpAddr>() {
            (ip.to_string(), wire::PORT, true)
        } else if let Some(inner) = rest.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let ip = inner.parse::<Ipv6Addr>().map_err(|_| invalid())?;
            (ip.to_string(), wire::PORT, true)
        } else {
            let (host, port) = match rest.rsplit_once(':') {
                Some((host, port)) => (
                    host,
                    port.parse::<u16>()
                        .ok()
                        .filter(|port| *port != 0)
                        .ok_or_else(invalid)?,
                ),
                None => (rest, wire::PORT),
            };
            if host.is_empty()
                || host.len() > 253
                || host.starts_with(['.', '-'])
                || !host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
                || ServerName::try_from(host.to_owned()).is_err()
            {
                return Err(invalid());
            }
            (host.to_owned(), port, false)
        };
        let is_localhost = host.trim_end_matches('.').eq_ignore_ascii_case("localhost");
        let tls = match scheme {
            Some("tls") => true,
            Some("tcp") => false,
            _ => !is_ip && !is_localhost,
        };
        Ok(Self {
            host,
            port,
            tls,
            scheme,
        })
    }

    fn authority(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    fn canonical(&self) -> String {
        let authority = self.authority();
        match self.scheme {
            Some(scheme) => format!("{scheme}://{authority}"),
            None => authority,
        }
    }

    fn server_name(&self) -> io::Result<ServerName<'static>> {
        ServerName::try_from(self.host.clone()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                crate::i18n::text().errors.invalid_tls_name,
            )
        })
    }
}

pub fn relay_address(input: &str) -> Result<String, String> {
    Endpoint::parse(input).map(|endpoint| endpoint.canonical())
}

enum RelayStream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
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

enum ConnectError {
    Certificate,
    Transient,
}

fn is_certificate_error(error: &io::Error) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = error
        .get_ref()
        .map(|err| err as &(dyn std::error::Error + 'static));
    while let Some(err) = current {
        if matches!(
            err.downcast_ref::<rustls::Error>(),
            Some(rustls::Error::InvalidCertificate(_))
        ) {
            return true;
        }
        current = err.source();
    }
    false
}

async fn connect(endpoint: &Endpoint, tls: &ClientTls) -> Result<RelayStream, ConnectError> {
    let addr = endpoint.authority();
    let stream = timeout(CONNECT, TcpStream::connect(&addr))
        .await
        .map_err(|_| ConnectError::Transient)?
        .map_err(|_| ConnectError::Transient)?;
    let _ = stream.set_nodelay(true);
    if !endpoint.tls {
        return Ok(RelayStream::Plain(stream));
    }
    let name = endpoint
        .server_name()
        .map_err(|_| ConnectError::Certificate)?;
    let connector = TlsConnector::from(tls.0.clone());
    match timeout(CONNECT, connector.connect(name, stream)).await {
        Ok(Ok(stream)) => Ok(RelayStream::Tls(Box::new(stream))),
        Ok(Err(error)) if is_certificate_error(&error) => Err(ConnectError::Certificate),
        Ok(Err(_)) | Err(_) => Err(ConnectError::Transient),
    }
}

struct Host {
    endpoint: Endpoint,
    tls: ClientTls,
    secret: String,
    words: [&'static str; 2],
    name: String,
    events: mpsc::Sender<Event>,
    origin: Instant,
    slot: Arc<Semaphore>,
    key: Mutex<Option<Key>>,
    wrong: AtomicU32,
}
enum Opened {
    Room(RelayStream, String),
    Taken,
    Retry,
    Fatal(String),
}

pub async fn host(relay: String, events: mpsc::Sender<Event>, origin: Instant) {
    host_with_tls(relay, ClientTls::webpki(), events, origin).await
}

pub async fn host_with_tls(
    relay: String,
    tls: ClientTls,
    events: mpsc::Sender<Event>,
    origin: Instant,
) {
    let endpoint = match Endpoint::parse(&relay) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            let _ = events.send(Event::Rejected(error)).await;
            return;
        }
    };
    let host = Arc::new(Host {
        endpoint,
        tls,
        secret: format!("{:032x}", rand::random::<u128>()),
        words: random_words(),
        name: crate::discovery::local_name(),
        events,
        origin,
        slot: Arc::new(Semaphore::new(1)),
        key: Mutex::new(None),
        wrong: AtomicU32::new(0),
    });
    let mut room: Option<String> = None;
    let mut tasks = JoinSet::new();
    let mut backoff = Duration::from_secs(1);
    loop {
        match open(&host, room.as_deref()).await {
            Opened::Room(stream, assigned) => {
                let code = Code {
                    room: assigned.clone(),
                    words: host.words,
                };
                if host
                    .events
                    .send(Event::RoomCode(code.to_string()))
                    .await
                    .is_err()
                {
                    return;
                }
                room = Some(assigned.clone());
                backoff = Duration::from_secs(1);
                control(stream, &host, &assigned, &mut tasks).await;
            }
            Opened::Taken => {
                room = None;
                continue;
            }
            Opened::Fatal(error) => {
                let _ = host.events.send(Event::Rejected(error)).await;
                return;
            }
            Opened::Retry => {}
        }
        if host.events.send(Event::RelayOffline).await.is_err() {
            return;
        }
        sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(10));
    }
}

async fn open(host: &Host, room: Option<&str>) -> Opened {
    let mut stream = match connect(&host.endpoint, &host.tls).await {
        Ok(stream) => stream,
        Err(ConnectError::Certificate) => {
            return Opened::Fatal(crate::i18n::text().errors.tls_error.into());
        }
        Err(ConnectError::Transient) => return Opened::Retry,
    };
    let result = async {
        wire::write_frame(
            &mut stream,
            &Request::Host {
                version: wire::VERSION,
                room: room.map(Into::into),
                secret: host.secret.clone(),
            },
        )
        .await?;
        let reply: Reply = timeout(STEP, wire::read_frame(&mut stream)).await??;
        Ok::<_, io::Error>((stream, reply))
    }
    .await;
    match result {
        Ok((stream, Reply::Hosting { room })) if wire::valid_room(&room) => {
            Opened::Room(stream, room)
        }
        Ok((_, Reply::Taken)) => Opened::Taken,
        Ok((_, Reply::Outdated { .. })) => {
            Opened::Fatal(crate::i18n::text().errors.outdated.into())
        }
        _ => Opened::Retry,
    }
}

async fn control(stream: RelayStream, host: &Arc<Host>, room: &str, tasks: &mut JoinSet<()>) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (tickets, mut incoming) = mpsc::channel(8);
    let reading = async move {
        loop {
            match timeout(
                wire::CONTROL_SILENCE,
                wire::read_frame::<_, Reply>(&mut reader),
            )
            .await
            {
                Ok(Ok(Reply::Incoming { ticket })) => {
                    if tickets.send(ticket).await.is_err() {
                        break;
                    }
                }
                Ok(Ok(Reply::Alive)) => {}
                _ => break,
            }
        }
    };
    let writing = async move {
        let mut alive = tokio::time::interval(wire::KEEPALIVE);
        alive.tick().await;
        loop {
            alive.tick().await;
            if !matches!(
                timeout(STEP, wire::write_frame(&mut writer, &Request::Alive)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
    };
    let accepting = async {
        while let Some(ticket) = incoming.recv().await {
            while tasks.try_join_next().is_some() {}
            if tasks.len() < 8 {
                let host = host.clone();
                let room = room.to_owned();
                tasks.spawn(async move {
                    let _ = admit(&host, &room, ticket).await;
                });
            }
        }
    };
    tokio::select! { _ = reading => {}, _ = writing => {}, _ = accepting => {} }
}

async fn admit(host: &Host, room: &str, ticket: u64) -> io::Result<()> {
    let mut stream = connect(&host.endpoint, &host.tls)
        .await
        .map_err(|_| io::Error::other(crate::i18n::text().errors.relay_connect_failed))?;
    wire::write_frame(
        &mut stream,
        &Request::Accept {
            version: wire::VERSION,
            room: room.into(),
            secret: host.secret.clone(),
            ticket,
        },
    )
    .await?;
    let reply: Reply = timeout(STEP, wire::read_frame(&mut stream)).await??;
    if reply != Reply::Paired {
        return Ok(());
    }
    let Ok(_slot) = host.slot.clone().try_acquire_owned() else {
        return Ok(());
    };
    let (mut reader, mut writer) = tokio::io::split(stream);
    let intro = timeout(STEP, secure::read_frame(&mut reader)).await??;
    let (psk, pake) = match intro.split_first() {
        Some((&INTRO_PAKE, message)) => {
            if host.wrong.load(Ordering::Relaxed) >= MAX_WRONG_CODES {
                return Ok(());
            }
            let code = Code {
                room: room.into(),
                words: host.words,
            };
            let (state, outbound) = secure::pake_start(room, &code.to_string());
            let mut answer = vec![INTRO_PAKE];
            answer.extend(outbound);
            secure::write_frame(&mut writer, &answer).await?;
            (secure::pake_finish(state, message)?, true)
        }
        Some((&INTRO_RESUME, [])) => (
            host.key.lock().unwrap().unwrap_or_else(rand::random::<Key>),
            false,
        ),
        _ => return Ok(()),
    };
    match timeout(STEP, secure::respond(reader, writer, &psk, &host.name)).await {
        Ok(Ok(channel)) => {
            if pake {
                *host.key.lock().unwrap() = Some(psk);
                host.wrong.store(0, Ordering::Relaxed);
            }
            let _ = host.events.send(Event::PeerName(channel.peer)).await;
            exchange(channel.reader, channel.writer, &host.events, host.origin).await;
        }
        Ok(Err(error)) if pake && error.kind() == io::ErrorKind::PermissionDenied => {
            let wrong = host.wrong.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = host
                .events
                .send(if wrong >= MAX_WRONG_CODES {
                    Event::Rejected(crate::i18n::text().errors.too_many_attempts.into())
                } else {
                    Event::WrongCode
                })
                .await;
        }
        _ => {}
    }
    Ok(())
}

enum Failure {
    Unknown,
    Away,
    WrongKey,
    Outdated,
    Limited,
    Certificate,
    Retry,
}
fn retry<T>(_: T) -> Failure {
    Failure::Retry
}

async fn enter(
    endpoint: &Endpoint,
    tls: &ClientTls,
    code: &Code,
    resume: Option<&Key>,
    name: &str,
) -> Result<(Key, Channel<ReadHalf<RelayStream>, WriteHalf<RelayStream>>), Failure> {
    let mut stream = connect(endpoint, tls).await.map_err(|error| match error {
        ConnectError::Certificate => Failure::Certificate,
        ConnectError::Transient => Failure::Retry,
    })?;
    wire::write_frame(
        &mut stream,
        &Request::Join {
            version: wire::VERSION,
            room: code.room.clone(),
        },
    )
    .await
    .map_err(retry)?;
    let reply: Reply = timeout(JOIN_REPLY, wire::read_frame(&mut stream))
        .await
        .map_err(retry)?
        .map_err(retry)?;
    match reply {
        Reply::Paired => {}
        Reply::Unknown => return Err(Failure::Unknown),
        Reply::Away => return Err(Failure::Away),
        Reply::Limited => return Err(Failure::Limited),
        Reply::Outdated { .. } => return Err(Failure::Outdated),
        _ => return Err(Failure::Retry),
    }
    let (mut reader, mut writer) = tokio::io::split(stream);
    let key = match resume {
        Some(key) => {
            secure::write_frame(&mut writer, &[INTRO_RESUME])
                .await
                .map_err(retry)?;
            *key
        }
        None => {
            let (state, outbound) = secure::pake_start(&code.room, &code.to_string());
            let mut intro = vec![INTRO_PAKE];
            intro.extend(outbound);
            secure::write_frame(&mut writer, &intro)
                .await
                .map_err(retry)?;
            let answer = timeout(STEP, secure::read_frame(&mut reader))
                .await
                .map_err(retry)?
                .map_err(retry)?;
            match answer.split_first() {
                Some((&INTRO_PAKE, message)) => {
                    secure::pake_finish(state, message).map_err(retry)?
                }
                _ => return Err(Failure::Retry),
            }
        }
    };
    match timeout(STEP, secure::initiate(reader, writer, &key, name)).await {
        Ok(Ok(channel)) => Ok((key, channel)),
        Ok(Err(error)) if error.kind() == io::ErrorKind::PermissionDenied => Err(Failure::WrongKey),
        _ => Err(Failure::Retry),
    }
}

pub async fn guest(relay: String, code: Code, events: mpsc::Sender<Event>, origin: Instant) {
    guest_with_tls(relay, code, ClientTls::webpki(), events, origin).await
}

pub async fn guest_with_tls(
    relay: String,
    code: Code,
    tls: ClientTls,
    events: mpsc::Sender<Event>,
    origin: Instant,
) {
    let endpoint = match Endpoint::parse(&relay) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            let _ = events.send(Event::Rejected(error)).await;
            return;
        }
    };
    let name = crate::discovery::local_name();
    let started = Instant::now();
    let key = loop {
        match enter(&endpoint, &tls, &code, None, &name).await {
            Ok((key, channel)) => {
                let _ = events.send(Event::PeerName(channel.peer)).await;
                if exchange(channel.reader, channel.writer, &events, origin).await != Ended::Lost {
                    return;
                }
                break key;
            }
            Err(failure) => {
                let error = match failure {
                    Failure::Unknown => Some(crate::i18n::text().errors.no_room),
                    Failure::WrongKey => Some(crate::i18n::text().errors.wrong_code),
                    Failure::Outdated => Some(crate::i18n::text().errors.outdated),
                    Failure::Limited => Some(crate::i18n::text().errors.relay_busy),
                    Failure::Certificate => Some(crate::i18n::text().errors.tls_error),
                    Failure::Away | Failure::Retry if started.elapsed() > FIRST_ENTRY => {
                        Some(crate::i18n::text().errors.host_unreachable)
                    }
                    Failure::Away | Failure::Retry => None,
                };
                if let Some(error) = error {
                    let _ = events.send(Event::Rejected(error.into())).await;
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }
        }
    };
    let mut gone: Option<Instant> = None;
    loop {
        sleep(Duration::from_secs(1)).await;
        match enter(&endpoint, &tls, &code, Some(&key), &name).await {
            Ok((_, channel)) => {
                gone = None;
                let _ = events.send(Event::PeerName(channel.peer)).await;
                if exchange(channel.reader, channel.writer, &events, origin).await != Ended::Lost {
                    return;
                }
            }
            Err(Failure::WrongKey) => {
                let _ = events
                    .send(Event::Rejected(
                        crate::i18n::text().errors.connection_not_accepted.into(),
                    ))
                    .await;
                return;
            }
            Err(Failure::Outdated) => {
                let _ = events
                    .send(Event::Rejected(crate::i18n::text().errors.outdated.into()))
                    .await;
                return;
            }
            Err(Failure::Certificate) => {
                let _ = events
                    .send(Event::Rejected(crate::i18n::text().errors.tls_error.into()))
                    .await;
                return;
            }
            Err(Failure::Unknown) if gone.get_or_insert_with(Instant::now).elapsed() > GONE => {
                let _ = events
                    .send(Event::Rejected(
                        crate::i18n::text().errors.room_closed_by_host.into(),
                    ))
                    .await;
                return;
            }
            Err(_) => {
                if events.send(Event::Disconnected).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_unique_short_and_ascii() {
        let mut sorted = WORDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), WORDS.len());
        assert!(
            WORDS
                .iter()
                .all(|w| (2..=9).contains(&w.len()) && w.bytes().all(|b| b.is_ascii_lowercase()))
        );
    }

    #[test]
    fn codes_roundtrip_and_tolerate_typing_variations() {
        let code = Code::random("4821".into());
        assert_eq!(Code::parse(&code.to_string()), Some(code.clone()));
        let expected = Code::parse("4821-agua-limao").map(|c| c.to_string());
        assert_eq!(expected, None, "palavras fora da lista são recusadas");
        let code = Code::parse("4821-agua-lua").unwrap();
        for typed in [
            "  4821 Água LUA ",
            "4821-agua-lua",
            "4821.agua.lua",
            "4821 - agua - lua",
        ] {
            assert_eq!(Code::parse(typed).as_ref(), Some(&code), "{typed}");
        }
        for invalid in [
            "",
            "4821",
            "4821-agua",
            "0821-agua-lua",
            "4821-agua-lua-sol",
            "abc-agua-lua",
        ] {
            assert!(Code::parse(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn relay_addresses_default_the_port_and_reject_garbage() {
        assert_eq!(
            relay_address("relay.pedrosales.dev").unwrap(),
            "relay.pedrosales.dev:7843"
        );
        assert_eq!(
            relay_address(" relay.example.com:9000 ").unwrap(),
            "relay.example.com:9000"
        );
        assert_eq!(relay_address("10.0.0.2").unwrap(), "10.0.0.2:7843");
        assert_eq!(relay_address("[::1]:7843").unwrap(), "[::1]:7843");
        assert_eq!(relay_address("[::1]").unwrap(), "[::1]:7843");
        assert_eq!(relay_address("::1").unwrap(), "[::1]:7843");
        assert_eq!(relay_address("localhost").unwrap(), "localhost:7843");
        assert_eq!(relay_address(DEFAULT_RELAY).unwrap(), DEFAULT_RELAY);
        assert_eq!(
            relay_address("tls://relay.example.com").unwrap(),
            "tls://relay.example.com:7843"
        );
        assert_eq!(
            relay_address("TLS://relay.example.com:9443").unwrap(),
            "tls://relay.example.com:9443"
        );
        assert_eq!(
            relay_address("tls://127.0.0.1:7843").unwrap(),
            "tls://127.0.0.1:7843"
        );
        assert_eq!(relay_address("tls://[::1]").unwrap(), "tls://[::1]:7843");
        assert_eq!(
            relay_address("tcp://relay.example.com").unwrap(),
            "tcp://relay.example.com:7843"
        );
        assert_eq!(
            relay_address("TCP://relay.example.com:9000").unwrap(),
            "tcp://relay.example.com:9000"
        );
        assert_eq!(
            relay_address("tcp://127.0.0.1:7843").unwrap(),
            "tcp://127.0.0.1:7843"
        );
        for invalid in [
            "",
            "relay:0",
            "relay:abc",
            "re lay",
            "-relay",
            "relay/x:7843",
            "127.0.0.1:0",
            "[::1]:0",
            "http://relay.example.com:7843",
            "https://relay.example.com:7843",
            "://relay.example.com:7843",
            "tls://",
            "tcp://",
            "tls://relay:0",
            "tcp://127.0.0.1:0",
            "tls://relay.example.com/path",
            "relay..example.com:7843",
        ] {
            let error = relay_address(invalid).expect_err(invalid);
            assert!(
                error == crate::i18n::text().errors.relay_format,
                "{invalid}: {error}"
            );
        }
    }

    #[test]
    fn bare_hostnames_default_to_tls_while_ips_and_localhost_default_to_tcp() {
        for tls_input in [
            DEFAULT_RELAY,
            "relay.pedrosales.dev",
            "relay.example.com:9443",
            "tls://relay.example.com",
            "tls://localhost:7843",
            "tls://127.0.0.1:7843",
            "tls://[::1]:7843",
        ] {
            assert!(Endpoint::parse(tls_input).unwrap().tls, "{tls_input}");
        }
        for tcp_input in [
            "127.0.0.1:7843",
            "127.0.0.1",
            "10.0.0.2",
            "[::1]:7843",
            "[::1]",
            "::1",
            "localhost",
            "LOCALHOST:7843",
            "localhost.:7843",
            "tcp://relay.pedrosales.dev:7843",
            "tcp://relay.example.com",
            "tcp://127.0.0.1:7843",
        ] {
            assert!(!Endpoint::parse(tcp_input).unwrap().tls, "{tcp_input}");
        }
    }

    #[test]
    fn client_tls_rejects_empty_or_invalid_pem() {
        assert!(ClientTls::from_pem(b"").is_err());
        assert!(ClientTls::from_pem(b"not a cert").is_err());
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        assert!(ClientTls::from_pem(cert.cert.pem().as_bytes()).is_ok());
    }
}

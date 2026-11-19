use super::{
    Event, exchange,
    secure::{self, Channel, INTRO_PAKE, INTRO_RESUME, Key},
};
use std::{
    fmt, io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use sync2gether_relay::{self as wire, Reply, Request};
use tokio::{
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::{Semaphore, mpsc},
    task::JoinSet,
    time::{sleep, timeout},
};

pub const DEFAULT_RELAY: &str = "relay.pedrosales.dev:7843";
pub const MAX_WRONG_CODES: u32 = 3;
const CONNECT: Duration = Duration::from_secs(5);
const STEP: Duration = Duration::from_secs(10);
const JOIN_REPLY: Duration = Duration::from_secs(15);
const FIRST_ENTRY: Duration = Duration::from_secs(30);
const GONE: Duration = Duration::from_secs(45);
const OUTDATED: &str = "O servidor exige outra versão do sync2gether. Atualize o aplicativo.";

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

pub fn relay_address(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.parse::<std::net::SocketAddr>().is_ok() {
        return Ok(input.to_owned());
    }
    let invalid =
        || "Informe o servidor como nome:porta, por exemplo relay.exemplo.com:7843.".to_owned();
    let (host, port) = match input.rsplit_once(':') {
        Some((host, port)) => (
            host,
            port.parse::<u16>()
                .ok()
                .filter(|port| *port != 0)
                .ok_or_else(invalid)?,
        ),
        None => (input, wire::PORT),
    };
    if host.is_empty()
        || host.len() > 253
        || host.starts_with(['.', '-'])
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err(invalid());
    }
    Ok(format!("{host}:{port}"))
}

async fn connect(relay: &str) -> io::Result<TcpStream> {
    let stream = timeout(CONNECT, TcpStream::connect(relay)).await??;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

struct Host {
    relay: String,
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
    Room(TcpStream, String),
    Taken,
    Retry,
    Fatal(String),
}

pub async fn host(relay: String, events: mpsc::Sender<Event>, origin: Instant) {
    let host = Arc::new(Host {
        relay,
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
    let result = async {
        let mut stream = connect(&host.relay).await?;
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
        Ok((_, Reply::Outdated { .. })) => Opened::Fatal(OUTDATED.into()),
        _ => Opened::Retry,
    }
}

async fn control(stream: TcpStream, host: &Arc<Host>, room: &str, tasks: &mut JoinSet<()>) {
    let (mut reader, mut writer) = stream.into_split();
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
    let mut stream = connect(&host.relay).await?;
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
    let (mut reader, mut writer) = stream.into_split();
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
                    Event::Rejected(
                        "Houve três tentativas com código errado. A sala foi fechada por segurança; crie outra."
                            .into(),
                    )
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
    Retry,
}
fn retry<T>(_: T) -> Failure {
    Failure::Retry
}

async fn enter(
    relay: &str,
    code: &Code,
    resume: Option<&Key>,
    name: &str,
) -> Result<(Key, Channel<OwnedReadHalf, OwnedWriteHalf>), Failure> {
    let mut stream = connect(relay).await.map_err(retry)?;
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
    let (mut reader, mut writer) = stream.into_split();
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
    let name = crate::discovery::local_name();
    let started = Instant::now();
    let key = loop {
        match enter(&relay, &code, None, &name).await {
            Ok((key, channel)) => {
                let _ = events.send(Event::PeerName(channel.peer)).await;
                exchange(channel.reader, channel.writer, &events, origin).await;
                break key;
            }
            Err(failure) => {
                let error = match failure {
                    Failure::Unknown => Some(
                        "Não há sala com esse código. Confira o número ou peça um código novo.",
                    ),
                    Failure::WrongKey => {
                        Some("Código incorreto. Confira as palavras ou peça um código novo.")
                    }
                    Failure::Outdated => Some(OUTDATED),
                    Failure::Limited => {
                        Some("O servidor está ocupado. Tente de novo em alguns minutos.")
                    }
                    Failure::Away | Failure::Retry if started.elapsed() > FIRST_ENTRY => Some(
                        "Não foi possível falar com o anfitrião. Confira a internet e se a sala continua aberta.",
                    ),
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
        match enter(&relay, &code, Some(&key), &name).await {
            Ok((_, channel)) => {
                gone = None;
                let _ = events.send(Event::PeerName(channel.peer)).await;
                exchange(channel.reader, channel.writer, &events, origin).await;
            }
            Err(Failure::WrongKey) => {
                let _ = events
                    .send(Event::Rejected(
                        "A sala não aceita mais esta conexão. Entre de novo com o código.".into(),
                    ))
                    .await;
                return;
            }
            Err(Failure::Outdated) => {
                let _ = events.send(Event::Rejected(OUTDATED.into())).await;
                return;
            }
            Err(Failure::Unknown) if gone.get_or_insert_with(Instant::now).elapsed() > GONE => {
                let _ = events
                    .send(Event::Rejected(
                        "A sala foi encerrada pelo anfitrião.".into(),
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
        assert_eq!(relay_address(DEFAULT_RELAY).unwrap(), DEFAULT_RELAY);
        for invalid in [
            "",
            "relay:0",
            "relay:abc",
            "re lay",
            "-relay",
            "relay/x:7843",
        ] {
            assert!(relay_address(invalid).is_err(), "{invalid}");
        }
    }
}

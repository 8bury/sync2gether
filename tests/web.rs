use futures_util::{SinkExt, StreamExt};
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};
use sync2gether::{
    network::{self, Event, relay::ClientTls},
    protocol::{Media, Message, Presence, Snapshot, Transition},
};
use sync2gether_relay::{Limits, Relay, ws};
use sync2gether_web::guest::{Command, Guest, PlayerStatus};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::timeout,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message as Frame, client::IntoClientRequest},
};

const ORIGIN: &str = "https://assistir.example";

async fn start_relay() -> (SocketAddr, SocketAddr) {
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let web = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addresses = (tcp.local_addr().unwrap(), web.local_addr().unwrap());
    let relay = Relay::new(Limits::default(), None);
    let browsers = relay.clone();
    tokio::spawn(async move { relay.serve(tcp).await });
    tokio::spawn(async move {
        browsers
            .serve_ws(
                web,
                ws::Config {
                    origins: vec![ORIGIN.into()],
                    client_ip_header: None,
                },
            )
            .await
    });
    addresses
}

async fn next(rx: &mut mpsc::Receiver<Event>) -> Event {
    timeout(Duration::from_secs(20), rx.recv())
        .await
        .expect("evento não chegou a tempo")
        .expect("tarefa de rede encerrada")
}

async fn host(relay: SocketAddr) -> (mpsc::Receiver<Event>, String, Instant) {
    let (events, mut rx) = mpsc::channel(64);
    let origin = Instant::now();
    tokio::spawn(network::relay::host_with_tls(
        relay.to_string(),
        ClientTls::webpki(),
        events,
        origin,
    ));
    loop {
        if let Event::RoomCode(code) = next(&mut rx).await {
            return (rx, code, origin);
        }
    }
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Browser {
    guest: Guest,
    url: String,
    origin: Instant,
    socket: Option<Socket>,
    connect_at: Option<f64>,
    prepared: Vec<(u64, f64)>,
    scheduled: Vec<(u64, f64)>,
    connects: usize,
}

impl Browser {
    fn new(code: &str, web: SocketAddr) -> Self {
        let origin = Instant::now();
        let mut guest = Guest::new(code, "Firefox").unwrap();
        guest.start(0.0);
        Self {
            guest,
            url: format!("ws://{web}{}", ws::PATH),
            origin,
            socket: None,
            connect_at: None,
            prepared: Vec::new(),
            scheduled: Vec::new(),
            connects: 0,
        }
    }

    fn now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64() * 1000.0
    }

    async fn flush(&mut self) {
        let output = self.guest.take_output();
        if !output.is_empty()
            && let Some(socket) = &mut self.socket
            && socket.send(Frame::Binary(output.into())).await.is_err()
        {
            self.socket = None;
            let now = self.now();
            self.guest.socket_closed(now);
        }
        for command in self.guest.take_commands() {
            match command {
                Command::Connect { at } => self.connect_at = Some(at),
                Command::Close => {
                    if let Some(mut socket) = self.socket.take() {
                        let _ = socket.close(None).await;
                    }
                }
                Command::Prepare { token, position } => self.prepared.push((token, position)),
                Command::Schedule { token, at } => self.scheduled.push((token, at)),
                _ => {}
            }
        }
    }

    async fn step(&mut self) {
        self.flush().await;
        let now = self.now();
        if self.socket.is_none()
            && let Some(at) = self.connect_at
            && now >= at
        {
            self.connect_at = None;
            let mut request = self.url.as_str().into_client_request().unwrap();
            request
                .headers_mut()
                .insert("origin", ORIGIN.parse().unwrap());
            match tokio_tungstenite::connect_async(request).await {
                Ok((socket, _)) => {
                    self.socket = Some(socket);
                    self.connects += 1;
                    self.guest.socket_open(self.now());
                }
                Err(_) => self.guest.socket_closed(self.now()),
            }
            self.flush().await;
        }
        let Some(socket) = &mut self.socket else {
            tokio::time::sleep(Duration::from_millis(10)).await;
            let now = self.now();
            self.guest.tick(now);
            return;
        };
        match timeout(Duration::from_millis(20), socket.next()).await {
            Ok(Some(Ok(Frame::Binary(bytes)))) => {
                let now = self.now();
                self.guest.socket_data(&bytes, now);
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_)) | None) => {
                self.socket = None;
                let now = self.now();
                self.guest.socket_closed(now);
            }
            Err(_) => {
                let now = self.now();
                self.guest.tick(now);
            }
        }
        self.flush().await;
    }

    async fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(self) {
            assert!(Instant::now() < deadline, "navegador: {what}");
            self.step().await;
        }
    }
}

fn media() -> Media {
    Media {
        hash: "a".repeat(64),
        bytes: 1234,
    }
}

fn snapshot(revision: u64, host_ms: f64, transition: Option<Transition>) -> Snapshot {
    Snapshot {
        revision,
        position: 0.0,
        paused: true,
        waiting_buffer: false,
        host_ms,
        transition,
        host: Presence {
            media: Some(media()),
            file_name: Some("Filme.mp4".into()),
            ready: true,
            duration: 60.0,
            ..Presence::default()
        },
        guest: Presence {
            media: Some(media()),
            web_guest: true,
            ..Presence::default()
        },
    }
}

async fn host_until(
    browser: &mut Browser,
    rx: &mut mpsc::Receiver<Event>,
    tx: &mpsc::Sender<Message>,
    origin: Instant,
    mut wanted: impl FnMut(&Message) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(
            Instant::now() < deadline,
            "anfitrião não recebeu a mensagem"
        );
        browser.step().await;
        while let Ok(event) = rx.try_recv() {
            if let Event::Message(message, received) = event {
                if let Message::Ping { sent_ms } = *message {
                    let received_ms = received.duration_since(origin).as_secs_f64() * 1000.0;
                    let _ = tx.try_send(Message::Pong {
                        sent_ms,
                        received_ms,
                        sent_host_ms: 0.0,
                    });
                } else if wanted(&message) {
                    return;
                }
            }
        }
    }
}

async fn host_connected(
    rx: &mut mpsc::Receiver<Event>,
    browser: &mut Browser,
) -> (mpsc::Sender<Message>, String) {
    let mut peer = None;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < deadline, "anfitrião não conectou");
        browser.step().await;
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::PeerName(name) => peer = Some(name),
                Event::Connected(tx) => return (tx, peer.unwrap()),
                Event::Rejected(error) => panic!("recusado: {error}"),
                _ => {}
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_guest_joins_through_websocket_and_follows_the_host() {
    let (relay, web) = start_relay().await;
    let (mut rx, code, origin) = host(relay).await;
    let mut browser = Browser::new(&code, web);
    let (tx, peer) = host_connected(&mut rx, &mut browser).await;
    assert_eq!(peer, "Firefox");
    browser
        .until("conectado", |b| b.guest.view().connected)
        .await;
    assert!(browser.guest.view().peer.is_some());

    host_until(
        &mut browser,
        &mut rx,
        &tx,
        origin,
        |message| matches!(message, Message::Presence(p) if p.web_guest && p.valid()),
    )
    .await;
    let generation = browser
        .guest
        .set_media(Some(media()), Some("Filme.mp4".into()));
    let host_ms = origin.elapsed().as_secs_f64() * 1000.0;
    tx.send(Message::State(Box::new(snapshot(1, host_ms, None))))
        .await
        .unwrap();
    browser
        .until("estado do anfitrião", |b| {
            b.guest.view().host_media == Some(media())
        })
        .await;
    let view = browser.guest.view();
    assert!(view.matched);
    assert_eq!(view.host_file_name.as_deref(), Some("Filme.mp4"));

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < deadline, "relógio não aqueceu");
        let now = browser.now();
        browser.guest.player_status(
            PlayerStatus {
                generation,
                measured_ms: now,
                duration: 60.0,
                speed: 1.0,
                paused: true,
                loaded: true,
                ..PlayerStatus::default()
            },
            now,
        );
        host_until(&mut browser, &mut rx, &tx, origin, |message| {
            matches!(message, Message::Presence(p) if p.clock_ready && p.ready && p.media == Some(media()))
        })
        .await;
        if browser.guest.view().clock_ready {
            break;
        }
    }

    let host_ms = origin.elapsed().as_secs_f64() * 1000.0;
    let transition = Transition {
        id: 7,
        position: 12.5,
        resume: true,
        start_at_ms: None,
    };
    tx.send(Message::State(Box::new(snapshot(
        2,
        host_ms,
        Some(transition),
    ))))
    .await
    .unwrap();
    browser.until("preparar", |b| !b.prepared.is_empty()).await;
    let (token, position) = browser.prepared[0];
    assert_eq!(position, 12.5);
    let now = browser.now();
    browser.guest.player_prepared(token, now);
    host_until(&mut browser, &mut rx, &tx, origin, |message| {
        matches!(message, Message::Prepared { id: 7 })
    })
    .await;

    let host_ms = origin.elapsed().as_secs_f64() * 1000.0;
    let transition = Transition {
        id: 7,
        position: 12.5,
        resume: true,
        start_at_ms: Some(host_ms + 1500.0),
    };
    let now = browser.now();
    browser.guest.player_status(
        PlayerStatus {
            generation,
            measured_ms: now,
            position: 12.5,
            duration: 60.0,
            speed: 1.0,
            paused: true,
            loaded: true,
            ..PlayerStatus::default()
        },
        now,
    );
    tx.send(Message::State(Box::new(snapshot(
        3,
        host_ms,
        Some(transition),
    ))))
    .await
    .unwrap();
    host_until(&mut browser, &mut rx, &tx, origin, |message| {
        matches!(message, Message::Armed { id: 7 })
    })
    .await;
    let (scheduled_token, at) = browser.scheduled[0];
    assert_eq!(scheduled_token, token);
    let local_start = at;
    let expected = browser.now() + 1500.0;
    assert!(
        (local_start - expected).abs() < 1500.0,
        "início local fora do esperado: {local_start} vs {expected}"
    );

    let first_connects = browser.connects;
    if let Some(mut socket) = browser.socket.take() {
        let _ = socket.close(None).await;
    }
    let now = browser.now();
    browser.guest.socket_closed(now);
    browser
        .until("reconectar", |b| {
            b.connects > first_connects && b.guest.view().connected
        })
        .await;
    let (_tx, peer) = host_connected(&mut rx, &mut browser).await;
    assert_eq!(peer, "Firefox");

    browser.guest.leave(browser.now());
    browser.flush().await;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < deadline, "anfitrião não percebeu a saída");
        match next(&mut rx).await {
            Event::PeerLeft => break,
            _ => continue,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_words_are_rejected_and_other_origins_cannot_connect() {
    let (relay, web) = start_relay().await;
    let (mut rx, code, _) = host(relay).await;
    let (room, words) = code.split_once('-').unwrap();
    let wrong = if words.starts_with("gato-") {
        format!("{room}-lua-lua")
    } else {
        format!("{room}-gato-gato")
    };
    let mut browser = Browser::new(&wrong, web);
    browser
        .until("recusa", |b| b.guest.view().error.is_some())
        .await;
    let view = browser.guest.view();
    assert!(view.ended);
    assert!(view.error.unwrap().contains("Código incorreto"));
    loop {
        if matches!(next(&mut rx).await, Event::WrongCode) {
            break;
        }
    }

    let mut request = format!("ws://{web}{}", ws::PATH)
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", "https://outro.example".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(request).await.is_err());
    let request = format!("ws://{web}/outro").into_client_request().unwrap();
    assert!(tokio_tungstenite::connect_async(request).await.is_err());
}

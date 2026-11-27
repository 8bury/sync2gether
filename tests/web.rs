use futures_util::{SinkExt, StreamExt};
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};
use sync2gether::{
    network::{
        self, Event,
        relay::{ClientTls, Code},
    },
    protocol::{Media, Message, Playback, Presence, Side, Snapshot, Transition},
    runtime::{Command as RuntimeCommand, Runtime, View as RuntimeView},
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
    subtitles: Vec<(String, String)>,
    subtitles_cleared: usize,
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
            subtitles: Vec::new(),
            subtitles_cleared: 0,
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
                Command::SubtitleReceived { name, vtt, .. } => self.subtitles.push((name, vtt)),
                Command::SubtitleCleared => self.subtitles_cleared += 1,
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
        paused_by: None,
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

#[test]
fn web_theme_tokens_match_core_source_of_truth() {
    let expected = sync2gether_core::theme::css_tokens();
    let actual = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("web/static/tokens.css"),
    )
    .expect("web/static/tokens.css ausente");
    assert_eq!(
        actual, expected,
        "web/static/tokens.css divergiu de sync2gether_core::theme::css_tokens()"
    );
}

#[test]
fn web_static_files_do_not_include_preview_code() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let forbidden = [
        "previewFixture",
        "runPreview",
        "preview-backdrop",
        "preview.js",
        "preview.css",
        "get(\"preview\")",
        "Filme.de.demonstracao",
    ];
    for entry in std::fs::read_dir(root.join("web/static")).expect("web/static ausente") {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(
            !name.starts_with("preview"),
            "arquivo de preview não pode ficar em web/static/: {name}"
        );
        if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("html" | "css" | "js")
        ) {
            let content = std::fs::read_to_string(&path).unwrap();
            for pattern in forbidden {
                assert!(
                    !content.contains(pattern),
                    "web/static/{name} contém referência ao modo preview ({pattern})"
                );
            }
        }
    }
    let dockerfile =
        std::fs::read_to_string(root.join("web/Dockerfile")).expect("web/Dockerfile ausente");
    assert!(!dockerfile.contains("--preview"));
    assert!(!dockerfile.contains("web/preview"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_guest_view_projects_pause_author_buffering_and_peer_leave() {
    let (relay, web) = start_relay().await;
    let (mut rx, code, origin) = host(relay).await;
    let mut browser = Browser::new(&code, web);
    let (tx, _peer) = host_connected(&mut rx, &mut browser).await;
    browser
        .until("conectado", |b| b.guest.view().connected)
        .await;
    browser
        .guest
        .set_media(Some(media()), Some("MinhaCopia.mp4".into()));

    let host_ms = origin.elapsed().as_secs_f64() * 1000.0;
    let mut state = snapshot(1, host_ms, None);
    state.paused = true;
    state.paused_by = Some(Side::Host);
    state.waiting_buffer = true;
    state.host.buffering = true;
    state.host.playback = Some(Playback {
        position: 42.0,
        host_ms,
        speed: 1.0,
        paused: true,
        seeking: false,
    });
    tx.send(Message::State(Box::new(state))).await.unwrap();
    browser
        .until("estado com quem pausou", |b| {
            b.guest.view().paused_by == Some(Side::Host)
        })
        .await;
    let view = browser.guest.view();
    assert_eq!(view.paused_by, Some(Side::Host));
    assert!(view.waiting_buffer);
    assert!(view.host_buffering);
    assert_eq!(view.guest_file_name.as_deref(), Some("MinhaCopia.mp4"));

    let host_ms = origin.elapsed().as_secs_f64() * 1000.0;
    let mut state = snapshot(2, host_ms, None);
    state.paused = true;
    state.paused_by = Some(Side::Guest);
    tx.send(Message::State(Box::new(state))).await.unwrap();
    browser
        .until("pausado pelo convidado", |b| {
            b.guest.view().paused_by == Some(Side::Guest)
        })
        .await;
    assert_eq!(browser.guest.view().paused_by, Some(Side::Guest));

    tx.send(Message::Bye).await.unwrap();
    browser
        .until("anfitrião saiu", |b| b.guest.view().peer_left)
        .await;
    let view = browser.guest.view();
    assert!(view.peer_left);
    assert!(view.ended);
    assert!(view.notice.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_shared_subtitle_roundtrips_through_encrypted_relay_channel() {
    let (relay, web) = start_relay().await;
    let (mut rx, code, _origin) = host(relay).await;
    let mut browser = Browser::new(&code, web);
    let (tx, _peer) = host_connected(&mut rx, &mut browser).await;
    browser
        .until("conectado", |b| b.guest.view().connected)
        .await;

    let mut srt = String::from("\u{feff}");
    for i in 1..=260 {
        let start = i * 2;
        let end = start + 1;
        srt.push_str(&format!(
            "{i}\r\n00:{:02}:{:02},000 --> 00:{:02}:{:02},500\r\nLinha compartilhada número {i}\r\n\r\n",
            start / 60,
            start % 60,
            end / 60,
            end % 60,
        ));
    }
    let vtt = sync2gether_core::subtitle::to_webvtt(&srt).unwrap();
    let chunks = sync2gether_core::subtitle::split_chunks(1, "Filme.pt-BR.srt", &vtt).unwrap();
    assert!(chunks.len() > 1);
    for chunk in chunks {
        tx.send(Message::SubtitleChunk(Box::new(chunk)))
            .await
            .unwrap();
    }
    browser
        .until("legenda recebida", |b| !b.subtitles.is_empty())
        .await;
    assert_eq!(browser.subtitles[0].0, "Filme.pt-BR.srt");
    assert_eq!(browser.subtitles[0].1, vtt);

    tx.send(Message::SubtitleClear).await.unwrap();
    browser
        .until("legenda removida", |b| b.subtitles_cleared == 1)
        .await;
}

async fn runtime_until(
    runtime: &Runtime,
    view: &mut RuntimeView,
    what: &str,
    done: impl Fn(&RuntimeView) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        while let Ok(next) = runtime.views.try_recv() {
            *view = next;
        }
        if done(view) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "runtime: {what}; status={} erro={:?}",
            view.status,
            view.error
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn assert_desktop_guest_receives_no_subtitles(rx: &mut mpsc::Receiver<Event>) {
    let mut states = 0usize;
    let deadline = Instant::now() + Duration::from_millis(700);
    while Instant::now() < deadline {
        match timeout(Duration::from_millis(50), rx.recv()).await {
            Ok(Some(Event::Message(message, _))) => match *message {
                Message::SubtitleChunk(_)
                | Message::SubtitleClear
                | Message::SubtitleFailed { .. } => {
                    panic!("convidado desktop recebeu mensagem de legenda");
                }
                Message::State(_) => states += 1,
                _ => {}
            },
            Ok(Some(Event::Disconnected | Event::PeerLeft)) => {
                panic!("convidado desktop desconectou inesperadamente");
            }
            Ok(Some(Event::Rejected(error))) => {
                panic!("convidado desktop recusado: {error}");
            }
            Ok(Some(_)) | Err(_) => {}
            Ok(None) => panic!("tarefa de rede do convidado desktop encerrou"),
        }
    }
    assert!(states > 0, "convidado desktop deixou de receber estados");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_shares_subtitles_only_with_web_guests_and_resends_on_reconnect() {
    let (relay, web) = start_relay().await;
    let dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/web-subtitle-gate-test");
    std::fs::create_dir_all(&dir).unwrap();
    let srt_path = dir.join("legenda.srt");
    std::fs::write(
        &srt_path,
        "1\r\n00:00:01,000 --> 00:00:05,000\r\nLegenda de teste\r\n",
    )
    .unwrap();

    let mut host = Runtime::start();
    let mut view = RuntimeView::default();
    host.commands
        .send(RuntimeCommand::HostInternet(relay.to_string()))
        .await
        .unwrap();
    runtime_until(&host, &mut view, "código da sala desktop", |v| {
        v.room_code.is_some()
    })
    .await;
    let desktop_code = view.room_code.clone().unwrap();

    let (guest_events, mut guest_rx) = mpsc::channel(64);
    let guest_task = tokio::spawn(network::relay::guest_with_tls(
        relay.to_string(),
        Code::parse(&desktop_code).unwrap(),
        ClientTls::webpki(),
        guest_events,
        Instant::now(),
    ));
    let guest_tx = loop {
        match next(&mut guest_rx).await {
            Event::Connected(tx) => break tx,
            Event::Rejected(error) => panic!("recusado: {error}"),
            _ => {}
        }
    };
    guest_tx
        .send(Message::Presence(Box::default()))
        .await
        .unwrap();
    runtime_until(&host, &mut view, "convidado desktop conectado", |v| {
        v.connected
    })
    .await;

    host.commands
        .send(RuntimeCommand::ShareSubtitlePath(srt_path.clone()))
        .await
        .unwrap();
    runtime_until(
        &host,
        &mut view,
        "legenda compartilhada no anfitrião",
        |v| v.shared_subtitle.as_deref() == Some("legenda.srt"),
    )
    .await;
    assert_desktop_guest_receives_no_subtitles(&mut guest_rx).await;

    host.commands
        .send(RuntimeCommand::ClearSharedSubtitle)
        .await
        .unwrap();
    runtime_until(&host, &mut view, "legenda limpa no anfitrião", |v| {
        v.shared_subtitle.is_none()
    })
    .await;
    assert_desktop_guest_receives_no_subtitles(&mut guest_rx).await;

    let _ = guest_tx.send(Message::Bye).await;
    guest_task.abort();

    host.commands
        .send(RuntimeCommand::HostInternet(relay.to_string()))
        .await
        .unwrap();
    runtime_until(&host, &mut view, "código da sala web", |v| {
        v.room_code
            .as_deref()
            .is_some_and(|code| code != desktop_code)
    })
    .await;
    let web_code = view.room_code.clone().unwrap();

    host.commands
        .send(RuntimeCommand::ShareSubtitlePath(srt_path))
        .await
        .unwrap();
    runtime_until(&host, &mut view, "legenda pronta para o navegador", |v| {
        v.shared_subtitle.as_deref() == Some("legenda.srt")
    })
    .await;

    let mut browser = Browser::new(&web_code, web);
    browser
        .until("legenda recebida ao conectar", |b| b.subtitles.len() == 1)
        .await;
    assert_eq!(browser.subtitles[0].0, "legenda.srt");
    assert!(browser.subtitles[0].1.contains("Legenda de teste"));

    let first_connects = browser.connects;
    if let Some(mut socket) = browser.socket.take() {
        let _ = socket.close(None).await;
    }
    let now = browser.now();
    browser.guest.socket_closed(now);
    browser
        .until("legenda reenviada após reconexão", |b| {
            b.connects > first_connects && b.guest.view().connected && b.subtitles.len() >= 2
        })
        .await;
    assert_eq!(browser.subtitles[1], browser.subtitles[0]);

    host.commands
        .send(RuntimeCommand::ClearSharedSubtitle)
        .await
        .unwrap();
    browser
        .until("legenda removida no navegador", |b| {
            b.subtitles_cleared == 1
        })
        .await;

    tokio::task::spawn_blocking(move || host.shutdown())
        .await
        .unwrap();
}

#[test]
fn protocol_stays_compatible_with_older_web_and_desktop_clients() {
    let legacy_presence = r#"{"media":null,"file_name":null,"loading":false,"verification":null,"ready":false,"blocked":false,"ended":false,"duration":0.0,"clock_ready":false,"rtt_ms":0.0,"uncertainty_ms":0.0,"playback":null}"#;
    let parsed: Presence = serde_json::from_str(legacy_presence).unwrap();
    assert!(!parsed.web_guest);
    assert!(!parsed.web_stream);
    assert!(!parsed.web_remux);

    let legacy_answer = r#"{"WebStreamAnswer":{"id":1,"sdp":"v=0\r\na=fingerprint:sha-256 AA:BB\r\n","media":{"hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","bytes":4096},"file_name":"filme.mp4","duration":10.0,"chunk_size":4096,"hash_table_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}"#;
    let message: Message = serde_json::from_str(legacy_answer).unwrap();
    assert!(matches!(
        message,
        Message::WebStreamAnswer(answer) if answer.valid() && answer.mse.is_none()
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guest_view_tracks_preparing_scheduled_and_playing_transitions() {
    let (relay, web) = start_relay().await;
    let (mut rx, code, origin) = host(relay).await;
    let mut browser = Browser::new(&code, web);
    let (tx, _peer) = host_connected(&mut rx, &mut browser).await;
    browser
        .until("conectado", |b| b.guest.view().connected)
        .await;
    let generation = browser
        .guest
        .set_media(Some(media()), Some("Filme.mkv".into()));

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
        host_until(
            &mut browser,
            &mut rx,
            &tx,
            origin,
            |message| matches!(message, Message::Presence(p) if p.clock_ready && p.ready),
        )
        .await;
        if browser.guest.view().clock_ready {
            break;
        }
    }

    let host_ms = origin.elapsed().as_secs_f64() * 1000.0;
    let recovery = Transition {
        id: 1,
        position: 0.0,
        resume: false,
        start_at_ms: None,
    };
    tx.send(Message::State(Box::new(snapshot(
        1,
        host_ms,
        Some(recovery),
    ))))
    .await
    .unwrap();
    browser
        .until("preparando busca inicial", |b| b.guest.view().preparing)
        .await;
    let view = browser.guest.view();
    assert!(view.preparing);
    assert!(!view.scheduled);
    assert!(view.scheduled_start_ms.is_none());

    let (token, _) = browser.prepared[0];
    let now = browser.now();
    browser.guest.player_prepared(token, now);
    host_until(&mut browser, &mut rx, &tx, origin, |message| {
        matches!(message, Message::Prepared { id: 1 })
    })
    .await;

    let host_ms = origin.elapsed().as_secs_f64() * 1000.0;
    tx.send(Message::State(Box::new(snapshot(2, host_ms, None))))
        .await
        .unwrap();
    browser
        .until("pronto após busca inicial", |b| !b.guest.view().preparing)
        .await;
    assert!(!browser.guest.view().scheduled);

    let host_ms = origin.elapsed().as_secs_f64() * 1000.0;
    let play = Transition {
        id: 2,
        position: 0.0,
        resume: true,
        start_at_ms: None,
    };
    tx.send(Message::State(Box::new(snapshot(3, host_ms, Some(play)))))
        .await
        .unwrap();
    browser
        .until("preparando reprodução", |b| b.prepared.len() == 2)
        .await;
    let (token, _) = browser.prepared[1];
    let now = browser.now();
    browser.guest.player_prepared(token, now);
    host_until(&mut browser, &mut rx, &tx, origin, |message| {
        matches!(message, Message::Prepared { id: 2 })
    })
    .await;

    let host_ms = origin.elapsed().as_secs_f64() * 1000.0;
    let scheduled = Transition {
        id: 2,
        position: 0.0,
        resume: true,
        start_at_ms: Some(host_ms + 1500.0),
    };
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
    tx.send(Message::State(Box::new(snapshot(
        4,
        host_ms,
        Some(scheduled),
    ))))
    .await
    .unwrap();
    browser
        .until("início agendado", |b| b.guest.view().scheduled)
        .await;
    assert!(!browser.guest.view().preparing);
    assert!(browser.guest.view().scheduled_start_ms.is_some());

    let host_ms = origin.elapsed().as_secs_f64() * 1000.0;
    let mut playing = snapshot(5, host_ms, None);
    playing.paused = false;
    tx.send(Message::State(Box::new(playing))).await.unwrap();
    browser
        .until("tocando sem agendamento pendente", |b| {
            let v = b.guest.view();
            v.playing && !v.scheduled && !v.preparing
        })
        .await;
    assert!(browser.guest.view().scheduled_start_ms.is_none());
}

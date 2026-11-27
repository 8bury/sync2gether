use futures_util::{SinkExt, StreamExt};
use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};
use sync2gether::{
    network::{
        self, Event as NetEvent,
        relay::{ClientTls, Code},
    },
    protocol::{Control, Media, Message, Presence, WebStreamOffer},
};
use sync2gether_core::{
    frame,
    relay_wire::{MAX_CONTROL, Reply, Request, VERSION},
};
use sync2gether_relay::{Limits, Relay, ws};
use sync2gether_web::{
    guest::{Command as GuestCommand, Guest, PlayerStatus},
    host::{BrowserHost, Command as HostCommand},
};
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

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn start_relay_with(limits: Limits) -> (SocketAddr, SocketAddr) {
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let web = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addresses = (tcp.local_addr().unwrap(), web.local_addr().unwrap());
    let relay = Relay::new(limits, None);
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

async fn start_relay() -> (SocketAddr, SocketAddr) {
    start_relay_with(Limits::default()).await
}

async fn ws_connect(web: SocketAddr) -> Socket {
    let url = format!("ws://{web}{}", ws::PATH);
    let mut request = url.as_str().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", ORIGIN.parse().unwrap());
    tokio_tungstenite::connect_async(request).await.unwrap().0
}

async fn ws_send_req(socket: &mut Socket, req: &Request) {
    let bytes = frame::encode_json(req, MAX_CONTROL).unwrap();
    socket.send(Frame::Binary(bytes.into())).await.unwrap();
}

async fn ws_read_reply(socket: &mut Socket) -> Reply {
    let mut reader = frame::Reader::default();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "sem resposta do relay");
        let frame = timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Frame::Binary(bytes) = frame {
            reader.push(&bytes);
            if let Some(raw) = reader.next(MAX_CONTROL).unwrap() {
                return serde_json::from_slice(&raw).unwrap();
            }
        }
    }
}

struct HostHarness {
    host: BrowserHost,
    url: String,
    origin: Instant,
    control: Option<Socket>,
    control_at: Option<f64>,
    accepts: HashMap<u64, Socket>,
    prepared: Vec<(u64, f64)>,
    scheduled: Vec<(u64, f64)>,
}

impl HostHarness {
    fn new(web: SocketAddr) -> Self {
        let origin = Instant::now();
        let mut host = BrowserHost::new("Firefox Host");
        host.start(0.0);
        Self {
            host,
            url: format!("ws://{web}{}", ws::PATH),
            origin,
            control: None,
            control_at: None,
            accepts: HashMap::new(),
            prepared: Vec::new(),
            scheduled: Vec::new(),
        }
    }

    fn now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64() * 1000.0
    }

    async fn connect(&self) -> Option<Socket> {
        let mut request = self.url.as_str().into_client_request().unwrap();
        request
            .headers_mut()
            .insert("origin", ORIGIN.parse().unwrap());
        tokio_tungstenite::connect_async(request)
            .await
            .ok()
            .map(|(s, _)| s)
    }

    async fn flush(&mut self) {
        let ctrl_out = self.host.take_control_output();
        if !ctrl_out.is_empty()
            && let Some(socket) = &mut self.control
            && socket.send(Frame::Binary(ctrl_out.into())).await.is_err()
        {
            self.control = None;
            let now = self.now();
            self.host.control_closed(now);
        }
        let ids: Vec<u64> = self.accepts.keys().copied().collect();
        for id in ids {
            let out = self.host.take_accept_output(id);
            if !out.is_empty()
                && let Some(socket) = self.accepts.get_mut(&id)
                && socket.send(Frame::Binary(out.into())).await.is_err()
            {
                self.accepts.remove(&id);
                let now = self.now();
                self.host.accept_closed(id, now);
            }
        }
        for command in self.host.take_commands() {
            match command {
                HostCommand::ConnectControl { at } => self.control_at = Some(at),
                HostCommand::CloseControl => {
                    if let Some(mut socket) = self.control.take() {
                        let _ = socket.close(None).await;
                    }
                }
                HostCommand::OpenAccept { id } => match self.connect().await {
                    Some(socket) => {
                        self.accepts.insert(id, socket);
                        let now = self.now();
                        self.host.accept_open(id, now);
                    }
                    None => {
                        let now = self.now();
                        self.host.accept_closed(id, now);
                    }
                },
                HostCommand::CloseAccept { id } => {
                    if let Some(mut socket) = self.accepts.remove(&id) {
                        let tail = self.host.take_accept_output(id);
                        if !tail.is_empty() {
                            let _ = socket.send(Frame::Binary(tail.into())).await;
                        }
                        let _ = socket.close(None).await;
                    }
                }
                HostCommand::Prepare { token, position } => {
                    self.prepared.push((token, position));
                }
                HostCommand::Schedule { token, at } => {
                    self.scheduled.push((token, at));
                }
                _ => {}
            }
        }
    }

    async fn step(&mut self) {
        self.flush().await;
        let now = self.now();
        if self.control.is_none()
            && let Some(at) = self.control_at
            && now >= at
        {
            self.control_at = None;
            match self.connect().await {
                Some(socket) => {
                    self.control = Some(socket);
                    let now = self.now();
                    self.host.control_open(now);
                }
                None => {
                    let now = self.now();
                    self.host.control_closed(now);
                }
            }
            self.flush().await;
        }
        if let Some(socket) = &mut self.control
            && let Ok(item) = timeout(Duration::from_millis(5), socket.next()).await
        {
            match item {
                Some(Ok(Frame::Binary(bytes))) => {
                    let now = self.now();
                    self.host.control_data(&bytes, now);
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => {
                    self.control = None;
                    let now = self.now();
                    self.host.control_closed(now);
                }
            }
        }
        let ids: Vec<u64> = self.accepts.keys().copied().collect();
        for id in ids {
            let Some(socket) = self.accepts.get_mut(&id) else {
                continue;
            };
            if let Ok(item) = timeout(Duration::from_millis(5), socket.next()).await {
                match item {
                    Some(Ok(Frame::Binary(bytes))) => {
                        let now = self.now();
                        self.host.accept_data(id, &bytes, now);
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => {
                        self.accepts.remove(&id);
                        let now = self.now();
                        self.host.accept_closed(id, now);
                    }
                }
            }
        }
        let now = self.now();
        self.host.tick(now);
        self.flush().await;
    }

    fn feed_player(&mut self, generation: u64, position: f64, paused: bool) {
        let now = self.now();
        self.host.player_status(
            PlayerStatus {
                generation,
                measured_ms: now,
                position,
                duration: 120.0,
                speed: 1.0,
                paused,
                loaded: true,
                ..PlayerStatus::default()
            },
            now,
        );
    }

    async fn drop_control(&mut self) {
        if let Some(mut socket) = self.control.take() {
            let _ = socket.close(None).await;
        }
        let now = self.now();
        self.host.control_closed(now);
        self.flush().await;
    }
}

struct GuestHarness {
    guest: Guest,
    url: String,
    origin: Instant,
    socket: Option<Socket>,
    connect_at: Option<f64>,
    prepared: Vec<(u64, f64)>,
    scheduled: Vec<(u64, f64)>,
    stream_stopped: Vec<u64>,
}

impl GuestHarness {
    fn new(code: &str, web: SocketAddr) -> Self {
        let origin = Instant::now();
        let mut guest = Guest::new(code, "Firefox Guest").unwrap();
        guest.start(0.0);
        Self {
            guest,
            url: format!("ws://{web}{}", ws::PATH),
            origin,
            socket: None,
            connect_at: None,
            prepared: Vec::new(),
            scheduled: Vec::new(),
            stream_stopped: Vec::new(),
        }
    }

    fn now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64() * 1000.0
    }

    async fn flush(&mut self) {
        let out = self.guest.take_output();
        if !out.is_empty()
            && let Some(socket) = &mut self.socket
            && socket.send(Frame::Binary(out.into())).await.is_err()
        {
            self.socket = None;
            let now = self.now();
            self.guest.socket_closed(now);
        }
        for command in self.guest.take_commands() {
            match command {
                GuestCommand::Connect { at } => self.connect_at = Some(at),
                GuestCommand::Close => {
                    if let Some(mut socket) = self.socket.take() {
                        let _ = socket.close(None).await;
                    }
                }
                GuestCommand::Prepare { token, position } => {
                    self.prepared.push((token, position));
                }
                GuestCommand::Schedule { token, at } => {
                    self.scheduled.push((token, at));
                }
                GuestCommand::StreamStopped { id } => {
                    self.stream_stopped.push(id);
                }
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
                    let now = self.now();
                    self.guest.socket_open(now);
                }
                Err(_) => {
                    let now = self.now();
                    self.guest.socket_closed(now);
                }
            }
            self.flush().await;
        }
        if let Some(socket) = &mut self.socket
            && let Ok(item) = timeout(Duration::from_millis(5), socket.next()).await
        {
            match item {
                Some(Ok(Frame::Binary(bytes))) => {
                    let now = self.now();
                    self.guest.socket_data(&bytes, now);
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => {
                    self.socket = None;
                    let now = self.now();
                    self.guest.socket_closed(now);
                }
            }
        }
        let now = self.now();
        self.guest.tick(now);
        self.flush().await;
    }

    fn feed_player(&mut self, generation: u64, position: f64, paused: bool) {
        let now = self.now();
        self.guest.player_status(
            PlayerStatus {
                generation,
                measured_ms: now,
                position,
                duration: 120.0,
                speed: 1.0,
                paused,
                loaded: true,
                ..PlayerStatus::default()
            },
            now,
        );
    }

    async fn drop_socket(&mut self) {
        if let Some(mut socket) = self.socket.take() {
            let _ = socket.close(None).await;
        }
        let now = self.now();
        self.guest.socket_closed(now);
        self.flush().await;
    }
}

fn sample_media() -> Media {
    Media {
        hash: "a".repeat(64),
        bytes: 4096,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_websocket_supports_host_alive_accept_and_enforces_limits() {
    let limits = Limits {
        rooms_per_address: 1,
        ..Limits::default()
    };
    let (_tcp, web) = start_relay_with(limits).await;

    let mut bad = ws_connect(web).await;
    ws_send_req(
        &mut bad,
        &Request::Host {
            version: VERSION + 1,
            room: None,
            secret: "a".repeat(32),
        },
    )
    .await;
    assert_eq!(
        ws_read_reply(&mut bad).await,
        Reply::Outdated { version: VERSION }
    );

    let mut host_ctrl = ws_connect(web).await;
    let secret = "1".repeat(32);
    ws_send_req(
        &mut host_ctrl,
        &Request::Host {
            version: VERSION,
            room: None,
            secret: secret.clone(),
        },
    )
    .await;
    let Reply::Hosting { room } = ws_read_reply(&mut host_ctrl).await else {
        panic!("esperado Hosting");
    };

    ws_send_req(&mut host_ctrl, &Request::Alive).await;
    assert_eq!(ws_read_reply(&mut host_ctrl).await, Reply::Alive);

    let mut second_host = ws_connect(web).await;
    ws_send_req(
        &mut second_host,
        &Request::Host {
            version: VERSION,
            room: None,
            secret: "2".repeat(32),
        },
    )
    .await;
    assert_eq!(ws_read_reply(&mut second_host).await, Reply::Limited);

    let mut guest = ws_connect(web).await;
    ws_send_req(
        &mut guest,
        &Request::Join {
            version: VERSION,
            room: room.clone(),
        },
    )
    .await;
    let Reply::Incoming { ticket } = ws_read_reply(&mut host_ctrl).await else {
        panic!("esperado Incoming");
    };

    let mut host_accept = ws_connect(web).await;
    ws_send_req(
        &mut host_accept,
        &Request::Accept {
            version: VERSION,
            room,
            secret,
            ticket,
        },
    )
    .await;
    assert_eq!(ws_read_reply(&mut host_accept).await, Reply::Paired);
    assert_eq!(ws_read_reply(&mut guest).await, Reply::Paired);

    host_accept
        .send(Frame::Binary(b"ping-from-host".to_vec().into()))
        .await
        .unwrap();
    let msg = timeout(Duration::from_secs(5), guest.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(msg, Frame::Binary(b"ping-from-host".to_vec().into()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_host_and_browser_guest_sync_resume_and_reclaim_room() {
    let (_tcp, web) = start_relay().await;
    let mut host = HostHarness::new(web);
    let deadline = Instant::now() + Duration::from_secs(15);
    while host.host.code().is_none() {
        assert!(Instant::now() < deadline, "host não recebeu código");
        host.step().await;
    }
    let code = host.host.code().unwrap();
    let room = host.host.room().unwrap().to_owned();

    let mut guest = GuestHarness::new(&code, web);
    let deadline = Instant::now() + Duration::from_secs(15);
    while !(host.host.is_connected() && guest.guest.is_connected()) {
        assert!(Instant::now() < deadline, "não conectaram");
        host.step().await;
        guest.step().await;
    }
    assert_eq!(host.host.view().peer.as_deref(), Some("Firefox Guest"));
    assert_eq!(guest.guest.view().peer.as_deref(), Some("Firefox Host"));

    let host_gen = host
        .host
        .set_media(Some(sample_media()), Some("Filme.mp4".into()), host.now());
    let guest_gen = guest
        .guest
        .set_media(Some(sample_media()), Some("Copia.mp4".into()));

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        assert!(Instant::now() < deadline, "não ficaram prontos");
        host.feed_player(host_gen, 0.0, true);
        guest.feed_player(guest_gen, 0.0, true);
        host.step().await;
        guest.step().await;
        let host_now = host.now();
        for (tok, _) in host.prepared.drain(..) {
            host.host.player_prepared(tok, host_now);
        }
        let guest_now = guest.now();
        for (tok, _) in guest.prepared.drain(..) {
            guest.guest.player_prepared(tok, guest_now);
        }
        if host.host.view().ready && guest.guest.view().matched && guest.guest.view().clock_ready {
            break;
        }
    }
    assert!(guest.guest.view().host_web_stream);
    assert!(!guest.guest.view().host_web_remux);

    host.host.control(Control::Play, host.now());
    let deadline = Instant::now() + Duration::from_secs(15);
    while host.prepared.is_empty() || guest.prepared.is_empty() {
        assert!(Instant::now() < deadline, "não prepararam");
        host.feed_player(host_gen, 0.0, true);
        guest.feed_player(guest_gen, 0.0, true);
        host.step().await;
        guest.step().await;
    }
    let (h_tok, _) = host.prepared.pop().unwrap();
    let (g_tok, _) = guest.prepared.pop().unwrap();
    host.host.player_prepared(h_tok, host.now());
    guest.guest.player_prepared(g_tok, guest.now());

    let deadline = Instant::now() + Duration::from_secs(15);
    while host.scheduled.is_empty() || guest.scheduled.is_empty() {
        assert!(Instant::now() < deadline, "não agendaram");
        host.feed_player(host_gen, 0.0, true);
        guest.feed_player(guest_gen, 0.0, true);
        host.step().await;
        guest.step().await;
    }

    host.host.control(Control::Pause, host.now());
    let deadline = Instant::now() + Duration::from_secs(15);
    while !guest.guest.view().paused || host.host.view().preparing || host.host.view().scheduled {
        assert!(Instant::now() < deadline, "não pausou");
        host.step().await;
        guest.step().await;
    }

    host.prepared.clear();
    guest.prepared.clear();
    host.host.control(Control::Seek(42.0), host.now());
    let deadline = Instant::now() + Duration::from_secs(15);
    while guest.prepared.is_empty() || host.prepared.is_empty() {
        assert!(Instant::now() < deadline, "seek não chegou");
        host.feed_player(host_gen, 0.0, true);
        guest.feed_player(guest_gen, 0.0, true);
        host.step().await;
        guest.step().await;
    }
    assert_eq!(guest.prepared[0].1, 42.0);
    assert_eq!(host.prepared[0].1, 42.0);

    guest.drop_socket().await;
    let deadline = Instant::now() + Duration::from_secs(15);
    while !host.host.is_connected() || !guest.guest.is_connected() {
        assert!(
            Instant::now() < deadline,
            "convidado não reconectou com INTRO_RESUME"
        );
        host.step().await;
        guest.step().await;
    }

    host.drop_control().await;
    assert!(host.host.view().relay_offline);
    let deadline = Instant::now() + Duration::from_secs(15);
    while host.host.view().relay_offline {
        assert!(Instant::now() < deadline, "controle não reconectou");
        host.step().await;
        guest.step().await;
    }
    assert_eq!(host.host.room(), Some(room.as_str()));
    assert_eq!(host.host.code().as_deref(), Some(code.as_str()));
    assert!(!guest.guest.stream_offer(77, "v=0\r\n".into(), guest.now()));
    let _ = WebStreamOffer {
        id: 77,
        sdp: "v=0\r\n".into(),
    };
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_host_rejects_wrong_codes_and_closes_after_three_attempts() {
    let (_tcp, web) = start_relay().await;
    let mut host = HostHarness::new(web);
    let deadline = Instant::now() + Duration::from_secs(15);
    while host.host.code().is_none() {
        assert!(Instant::now() < deadline, "host não recebeu código");
        host.step().await;
    }
    let code = host.host.code().unwrap();
    let room = host.host.room().unwrap().to_owned();
    let wrong_code = if code.ends_with("agua") {
        format!("{room}-alho-amora")
    } else {
        format!("{room}-agua-agua")
    };

    for attempt in 1..=3 {
        let mut guest = GuestHarness::new(&wrong_code, web);
        let deadline = Instant::now() + Duration::from_secs(15);
        while !guest.guest.view().ended {
            assert!(
                Instant::now() < deadline,
                "tentativa {attempt} não encerrou"
            );
            host.step().await;
            guest.step().await;
        }
        assert!(guest.guest.view().error.is_some());
        if attempt < 3 {
            assert!(!host.host.view().ended);
            assert!(host.host.view().notice.is_some());
        } else {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !host.host.view().ended {
                assert!(Instant::now() < deadline, "sala não encerrou após 3 erros");
                host.step().await;
            }
            assert!(host.host.view().error.is_some());
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn desktop_guest_joins_browser_host_through_relay_and_plays_in_sync() {
    let (tcp, web) = start_relay().await;
    let mut host = HostHarness::new(web);
    let deadline = Instant::now() + Duration::from_secs(15);
    while host.host.code().is_none() {
        assert!(Instant::now() < deadline, "host não recebeu código");
        host.step().await;
    }
    let code = Code::parse(&host.host.code().unwrap()).unwrap();

    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest_origin = Instant::now();
    let guest_task = tokio::spawn(network::relay::guest_with_tls(
        tcp.to_string(),
        code,
        ClientTls::webpki(),
        guest_tx,
        guest_origin,
    ));

    let mut guest_out = None;
    let deadline = Instant::now() + Duration::from_secs(15);
    while guest_out.is_none() || !host.host.is_connected() {
        assert!(Instant::now() < deadline, "desktop guest não conectou");
        host.step().await;
        while let Ok(event) = guest_rx.try_recv() {
            if let NetEvent::Connected(tx) = event {
                guest_out = Some(tx);
            }
        }
    }
    let guest_out = guest_out.unwrap();

    let host_gen = host
        .host
        .set_media(Some(sample_media()), Some("Filme.mp4".into()), host.now());
    host.feed_player(host_gen, 0.0, true);

    guest_out
        .send(Message::Presence(Box::new(Presence {
            media: Some(sample_media()),
            file_name: Some("Filme.mp4".into()),
            ready: true,
            duration: 120.0,
            clock_ready: true,
            rtt_ms: 20.0,
            uncertainty_ms: 5.0,
            web_guest: false,
            ..Presence::default()
        })))
        .await
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(15);
    while !host.host.view().ready {
        assert!(
            Instant::now() < deadline,
            "anfitrião não ficou pronto com convidado desktop"
        );
        host.feed_player(host_gen, 0.0, true);
        host.step().await;
        let host_now = host.now();
        for (tok, _) in host.prepared.drain(..) {
            host.host.player_prepared(tok, host_now);
        }
        while let Ok(NetEvent::Message(msg, _)) = guest_rx.try_recv() {
            if let Message::State(state) = *msg
                && let Some(t) = state.transition
            {
                let _ = guest_out.send(Message::Prepared { id: t.id }).await;
            }
        }
    }

    host.prepared.clear();
    host.host.control(Control::Play, host.now());
    let mut saw_transition = None;
    let deadline = Instant::now() + Duration::from_secs(15);
    while saw_transition.is_none() || host.prepared.is_empty() {
        assert!(
            Instant::now() < deadline,
            "transição não chegou no convidado desktop"
        );
        host.feed_player(host_gen, 0.0, true);
        host.step().await;
        while let Ok(NetEvent::Message(msg, _)) = guest_rx.try_recv() {
            if let Message::State(state) = *msg
                && let Some(t) = state.transition
            {
                saw_transition = Some(t.id);
            }
        }
    }
    let op_id = saw_transition.unwrap();
    let (h_tok, _) = host.prepared[0];
    host.host.player_prepared(h_tok, host.now());
    guest_out
        .send(Message::Prepared { id: op_id })
        .await
        .unwrap();

    let mut saw_schedule = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    while !saw_schedule {
        assert!(
            Instant::now() < deadline,
            "agendamento não chegou no convidado desktop"
        );
        host.feed_player(host_gen, 0.0, true);
        host.step().await;
        while let Ok(NetEvent::Message(msg, _)) = guest_rx.try_recv() {
            if let Message::State(state) = *msg
                && state
                    .transition
                    .as_ref()
                    .is_some_and(|t| t.id == op_id && t.start_at_ms.is_some())
            {
                saw_schedule = true;
            }
        }
    }
    guest_out.send(Message::Armed { id: op_id }).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while host.scheduled.is_empty() {
        assert!(Instant::now() < deadline, "anfitrião não agendou início");
        host.feed_player(host_gen, 0.0, true);
        host.step().await;
    }

    guest_out.send(Message::StreamRequest).await.unwrap();
    host.step().await;
    assert!(host.host.is_connected());

    guest_out.send(Message::Bye).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while host.host.is_connected() || !host.host.view().peer_left {
        assert!(
            Instant::now() < deadline,
            "saída do convidado desktop não detectada"
        );
        host.step().await;
    }
    guest_task.abort();
}

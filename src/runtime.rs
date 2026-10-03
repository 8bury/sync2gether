//! Comandos e eventos ligam a UI à preparação, agendamento e correção dos players.
use crate::{
    discovery, network, player,
    protocol::{Control, Media, Message, PORT, Playback, Presence, Snapshot, Verification},
    session::Session,
    sync::{Clock, Controller, Correction},
};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc,
    },
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Local,
    Host,
    Guest,
}
#[derive(Clone)]
pub struct JoinRequest {
    pub id: u64,
    pub name: String,
    pub address: SocketAddr,
}
#[derive(Clone, Default)]
pub struct View {
    pub role: Role,
    pub hosting: bool,
    pub searching: bool,
    pub rooms: Vec<discovery::Room>,
    pub discovery_error: Option<String>,
    pub room_address: Option<SocketAddr>,
    pub room_addresses: Vec<SocketAddr>,
    pub join_request: Option<JoinRequest>,
    pub waiting_approval: bool,
    pub peer_name: Option<String>,
    pub watching: bool,

    pub connected: bool,
    pub status: String,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub file: Option<String>,
    pub pending_file: Option<String>,
    pub verification: Option<Verification>,
    pub verified: bool,
    pub peer_file: Option<String>,
    pub peer_loading: bool,
    pub peer_verification: Option<Verification>,
    pub peer_verified: bool,
    pub peer_blocked: bool,
    pub loading: bool,
    pub player: player::Status,
    pub matched: bool,
    pub ready: bool,
    pub peer_ready: bool,
    pub rtt_ms: f64,
    pub drift: f64,
    pub peer_drift: Option<f64>,
    pub room_key: String,
    pub preparing: bool,
    pub scheduled: bool,
    pub last_start_ms: Option<f64>,
    pub last_start_id: Option<u64>,
    pub scheduled_start_ms: Option<f64>,
}
pub enum Command {
    Open,
    OpenPath(PathBuf),
    CancelOpen,
    Host(String),
    HostAuto(u16),
    HostRoom(String),
    Discover,
    CancelDiscovery,
    RequestJoin(String),
    Approve { id: u64, accept: bool },
    Join(String, String),
    Leave,
    Control(Control),
    Volume(f64),
    Audio,
    Subtitle,
    AudioTrack(i64),
    SubtitleTrack(Option<i64>),
    Shutdown,
}
pub struct Runtime {
    pub commands: mpsc::Sender<Command>,
    pub views: std_mpsc::Receiver<View>,
    pub frames: player::Frames,
    origin: Instant,
}
impl Runtime {
    /// Interpola a posição para a barra de progresso entre eventos do player.
    pub fn position_now(&self, view: &View) -> f64 {
        view.player
            .position_at(self.origin.elapsed().as_secs_f64() * 1000.0)
    }
    /// Renderização por software, também usada pelos testes sem janela.
    pub fn start() -> Self {
        Self::start_with_renderer(None)
    }
    /// Player com renderização OpenGL no contexto da janela.
    pub fn start_gl(
        cc: &eframe::CreationContext<'_>,
    ) -> Result<(Self, player::GlRenderer), String> {
        let (renderer, bridge) = player::GlRenderer::new(cc)?;
        Ok((Self::start_with_renderer(Some(bridge)), renderer))
    }
    pub(crate) fn start_with_renderer(gl: Option<player::gl::Bridge>) -> Self {
        let (commands, rx) = mpsc::channel(32);
        let (views, output) = std_mpsc::channel();
        let (events, player_rx) = mpsc::channel(32);
        let origin = Instant::now();
        let player = player::Player::start_with_renderer(events, origin, gl);
        let frames = player.frames.clone();
        std::thread::spawn(move || {
            match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => {
                    runtime.block_on(Worker::new(player, origin).run(rx, player_rx, views))
                }
                Err(_) => {
                    let _ = views.send(View {
                        error: Some("Não foi possível iniciar o runtime".into()),
                        ..Default::default()
                    });
                }
            }
        });
        Self {
            commands,
            views: output,
            frames,
            origin,
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.commands.try_send(Command::Shutdown);
    }
}
enum BackgroundResult {
    Host(Result<(tokio::net::TcpListener, String), String>),
    Rooms(Result<Vec<discovery::Room>, String>),
}
struct FileResult {
    generation: u64,
    event: FileEvent,
}
enum FileEvent {
    Selected(String),
    Progress(Verification),
    Finished(Result<Option<(PathBuf, Media)>, String>),
}
struct Worker {
    player: player::Player,
    background: Option<JoinHandle<()>>,
    background_id: u64,
    background_tx: mpsc::Sender<(u64, BackgroundResult)>,
    background_rx: mpsc::Receiver<(u64, BackgroundResult)>,
    approval: Option<(u64, tokio::sync::oneshot::Sender<bool>)>,
    view: View,
    session: Session,
    clock: Clock,
    snapshot: Option<Snapshot>,
    origin: Instant,
    controller: Controller,
    speed: f64,
    requested: Option<u64>,
    player_token: u64,
    player_operation: Option<(u64, u64)>,
    prepared: Option<u64>,
    scheduled: Option<u64>,
    last_pause: Option<u64>,
    failed: Option<u64>,
    network: Option<JoinHandle<()>>,
    outgoing: Option<mpsc::Sender<Message>>,
    presence: Presence,
    generation: u64,
    next_generation: u64,
    previous: Option<Presence>,
    file_cancel: Option<Arc<AtomicBool>>,
    fatal_error: Option<String>,
    recover: bool,
    files: mpsc::Sender<FileResult>,
    file_rx: mpsc::Receiver<FileResult>,
    pings: VecDeque<f64>,
}
impl Worker {
    fn new(player: player::Player, origin: Instant) -> Self {
        let (files, file_rx) = mpsc::channel(4);
        let (background_tx, background_rx) = mpsc::channel(4);
        Self {
            player,
            background: None,
            background_id: 0,
            background_tx,
            background_rx,
            approval: None,
            origin,
            view: View {
                status: "Reprodução local".into(),
                ..Default::default()
            },
            session: Session::default(),
            clock: Clock::default(),
            snapshot: None,
            controller: Controller::default(),
            speed: 1.0,
            requested: None,
            player_token: 0,
            player_operation: None,
            prepared: None,
            scheduled: None,
            last_pause: None,
            failed: None,
            network: None,
            outgoing: None,
            presence: Presence::default(),
            generation: 0,
            next_generation: 0,
            previous: None,
            file_cancel: None,
            fatal_error: None,
            recover: false,
            files,
            file_rx,
            pings: VecDeque::new(),
        }
    }
    fn now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64() * 1000.0
    }
    fn player(&mut self, command: player::Command) {
        if self.player.commands.send(command).is_err() {
            self.fatal_error.get_or_insert_with(|| {
                "Player indisponível. Confira libmpv e reinicie o app.".into()
            });
        }
    }
    fn send(&mut self, message: Message) {
        if self
            .outgoing
            .as_ref()
            .is_some_and(|tx| tx.try_send(message).is_err())
        {
            self.disconnected();
        }
    }
    fn set_speed(&mut self, speed: f64) {
        if (self.speed - speed).abs() > 0.0001 {
            self.player(player::Command::Speed(speed));
            self.speed = speed;
        }
    }
    fn reset_operation(&mut self) {
        self.requested = None;
        self.player_operation = None;
        self.prepared = None;
        self.scheduled = None;
        self.last_pause = None;
        self.controller.reset(self.now());
        self.player(player::Command::Pause(true));
        self.set_speed(1.0);
    }
    fn disconnected(&mut self) {
        self.outgoing = None;
        self.view.connected = false;
        self.clock = Clock::default();
        self.snapshot = None;
        self.pings.clear();
        self.session.disconnect(self.now());
        self.reset_operation();
        self.view.last_start_ms = None;
        self.view.last_start_id = None;
        self.failed = None;
        self.view.scheduled_start_ms = None;
    }
    fn cancel_background(&mut self) {
        self.background_id += 1;
        if let Some(task) = self.background.take() {
            task.abort();
        }
        self.view.searching = false;
        self.view.hosting = false;
    }
    fn background(&mut self, port: Option<u16>) {
        self.cancel_background();
        self.view.error = None;
        self.view.discovery_error = None;
        self.view.hosting = port.is_some();
        self.view.searching = port.is_none();
        self.view.rooms.clear();
        let tx = self.background_tx.clone();
        let id = self.background_id;
        self.background = Some(tokio::spawn(async move {
            if let Some(port) = port {
                let result = if port == 0 {
                    Err("Escolha uma porta entre 1 e 65535.".into())
                } else {
                    tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port))
                        .await
                        .map(|listener| (listener, discovery::local_name()))
                        .map_err(|_| {
                            "Não foi possível abrir a sala. Verifique se a porta está livre.".into()
                        })
                };
                let _ = tx.send((id, BackgroundResult::Host(result))).await;
            } else {
                match discovery::Lan::open() {
                    Ok(mut lan) => loop {
                        let rooms = lan.scan(std::time::Duration::from_secs(2)).await;
                        if tx
                            .send((id, BackgroundResult::Rooms(Ok(rooms))))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    },
                    Err(_) => {
                        let _ = tx.send((id, BackgroundResult::Rooms(Err("Não foi possível buscar na rede. Você pode conectar por endereço.".into())))).await;
                    }
                }
            }
        }));
    }
    fn leave(&mut self) {
        self.cancel_background();
        self.cancel_open();
        self.approval = None;
        self.view.join_request = None;
        self.view.waiting_approval = false;
        self.view.peer_name = None;
        self.view.room_address = None;
        self.view.room_addresses.clear();
        self.view.watching = false;
        if let Some(task) = self.network.take() {
            task.abort();
        }
        self.disconnected();
        self.session = Session::default();
        self.view.role = Role::Local;
        self.view.room_key.clear();
        self.view.error = None;
        self.view.notice = None;
    }

    fn cancel_open(&mut self) {
        if let Some(cancel) = self.file_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        if self.view.loading {
            self.generation = self.view.player.generation;
            self.presence = self.previous.take().unwrap_or_default();
        }
        self.view.loading = false;
        self.view.pending_file = None;
        self.view.verification = None;
    }
    fn control(&mut self, control: Control) {
        if !control.valid() {
            return;
        }
        self.view.notice = None;
        match self.view.role {
            Role::Local => match control {
                Control::Play => {
                    self.view.watching = true;
                    self.player(player::Command::Pause(false));
                }
                Control::Pause => self.player(player::Command::Pause(true)),
                Control::Seek(p) => {
                    self.player(player::Command::Seek(p.min(self.view.player.duration)))
                }
            },
            Role::Host => {
                self.refresh_presence();
                if self.session.apply(control, self.now()) {
                    if matches!(control, Control::Play) {
                        self.view.watching = true;
                    }
                    self.publish();
                }
            }
            Role::Guest => {
                if self.view.connected {
                    self.send(Message::Request(control));
                }
            }
        }
    }
    fn open(&mut self, path: Option<PathBuf>) {
        if self.view.loading {
            return;
        }
        self.previous = Some(self.presence.clone());
        self.next_generation += 1;
        self.generation = self.next_generation;
        let generation = self.generation;
        self.view.loading = true;
        self.view.watching = false;
        self.view.error = None;
        self.view.pending_file = None;
        self.view.verification = None;
        let cancel = Arc::new(AtomicBool::new(false));
        self.file_cancel = Some(cancel.clone());
        let files = self.files.clone();
        tokio::task::spawn_blocking(move || {
            let selected = path.or_else(|| {
                rfd::FileDialog::new()
                    .set_title("Escolher sua cópia do filme")
                    .pick_file()
            });
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let result = selected.map(|path| {
                let name = crate::media::display_name(&path);
                let _ = files.blocking_send(FileResult { generation, event: FileEvent::Selected(name) });
                let mut last_progress = None;
                path.canonicalize().and_then(|path| {
                    crate::media::identify_with_progress(&path, |bytes, total| {
                        if cancel.load(Ordering::Relaxed) { return Err(std::io::Error::other("Verificação cancelada")); }
                        if last_progress.is_none_or(|at: Instant| at.elapsed() >= Duration::from_millis(100)) || bytes == total {
                            files.blocking_send(FileResult { generation, event: FileEvent::Progress(Verification { bytes, total }) })
                                .map_err(|_| std::io::Error::other("Aplicativo encerrado"))?;
                            last_progress = Some(Instant::now());
                        }
                        Ok(())
                    }).map(|media| (path, media))
                }).map_err(|_| "Não foi possível verificar o arquivo. Confira se ele está acessível e escolha novamente.".into())
            }).transpose();
            if !cancel.load(Ordering::Relaxed) {
                let _ = files.blocking_send(FileResult {
                    generation,
                    event: FileEvent::Finished(result),
                });
            }
        });
        self.control(Control::Pause);
        self.reset_operation();
        self.presence.ready = false;
        self.presence.media = None;
        self.presence.file_name = None;
        self.presence.loading = true;
        self.presence.verification = None;
    }
    async fn run(
        mut self,
        mut commands: mpsc::Receiver<Command>,
        mut player_events: mpsc::Receiver<player::Event>,
        views: std_mpsc::Sender<View>,
    ) {
        let (_net_tx, mut net_rx) = mpsc::channel(64);
        let mut heartbeat = tokio::time::interval(Duration::from_millis(200));
        let mut monitor = tokio::time::interval(Duration::from_millis(20));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        monitor.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break; };
                    match command {
                        Command::Shutdown => break,
                        Command::Open => self.open(None), Command::OpenPath(path) => self.open(Some(path)),
                        Command::CancelOpen => self.cancel_open(),
                        Command::HostAuto(port) => { self.leave(); self.background(Some(port)); }
                        Command::Discover => self.background(None),
                        Command::CancelDiscovery => self.cancel_background(),
                        Command::Approve { id, accept } => {
                            if self.approval.as_ref().is_some_and(|(pending, _)| *pending == id)
                                && let Some((_, reply)) = self.approval.take() {
                                if accept {
                                    self.view.peer_name = self.view.join_request.as_ref().map(|r| r.name.clone());
                                    self.view.watching = false;
                                }
                                let _ = reply.send(accept);
                            }
                        }
                        Command::HostRoom(address) => {
                            self.leave(); let (_tx, rx) = mpsc::channel(64); net_rx = rx;
                            match parse_address(&address) {
                                Ok(address) => match tokio::net::TcpListener::bind(address).await {
                                    Ok(listener) => {
                                        self.view.role = Role::Host;
                                        self.view.room_address = Some(address);
                                        self.clock = Clock::host();
                                        self.session.state.position = self.view.player.position;
                                        self.session.state.host_ms = self.now();
                                        let (tx, rx) = mpsc::channel(64); net_rx = rx;
                                        self.network = Some(tokio::spawn(network::rooms::host(listener, "sync2gether".into(), tx, self.origin)));
                                    }
                                    Err(_) => self.view.error = Some("Não foi possível abrir a sala nesse endereço e porta.".into()),
                                }, Err(error) => self.view.error = Some(error),
                            }
                        }
                        Command::RequestJoin(address) => {
                            self.leave(); let (_tx, rx) = mpsc::channel(64); net_rx = rx;
                            match parse_address(&address) {
                                Ok(address) => {
                                    self.view.role = Role::Guest;
                                    self.view.room_address = Some(address);
                                    self.view.peer_name = self.view.rooms.iter().find(|r| r.address == address).map(|r| r.name.clone());
                                    let (tx, rx) = mpsc::channel(64); net_rx = rx;
                                    self.network = Some(tokio::spawn(network::rooms::guest(address, tx, self.origin)));
                                }
                                Err(error) => self.view.error = Some(error),
                            }
                        }
                        Command::Host(address) => {
                            self.leave(); let (_tx, rx) = mpsc::channel(64); net_rx = rx;
                            match parse_address(&address) {
                                Ok(address) => match tokio::net::TcpListener::bind(address).await {
                                    Ok(listener) => {
                                        self.view.room_key = format!("{:032x}", rand::random::<u128>()); self.view.role = Role::Host;
                                        self.clock = Clock::host(); self.session.state.position = self.view.player.position;
                                        self.session.state.host_ms = self.now();
                                        let (tx, rx) = mpsc::channel(64); net_rx = rx;
                                        self.network = Some(tokio::spawn(network::host(listener, self.view.room_key.clone(), tx, self.origin)));
                                    }
                                    Err(_) => self.view.error = Some("Não foi possível abrir a sala nesse endereço e porta.".into()),
                                }, Err(error) => self.view.error = Some(error),
                            }
                        }
                        Command::Join(address, key) => {
                            self.leave(); let (_tx, rx) = mpsc::channel(64); net_rx = rx;
                            match parse_address(&address) {
                                Ok(address) if key.len() == 32 && key.bytes().all(|b| b.is_ascii_hexdigit()) => {
                                    self.view.role = Role::Guest; let (tx, rx) = mpsc::channel(64); net_rx = rx;
                                    self.network = Some(tokio::spawn(network::guest(address, key, tx, self.origin)));
                                }
                                Ok(_) => self.view.error = Some("O código da sala deve ter 32 caracteres hexadecimais.".into()),
                                Err(error) => self.view.error = Some(error),
                            }
                        }
                        Command::Leave => { self.leave(); let (_tx, rx) = mpsc::channel(64); net_rx = rx; }
                        Command::Control(control) => self.control(control),
                        Command::Volume(v) => self.player(player::Command::Volume(v.clamp(0.0, 100.0))),
                        Command::Audio => self.player(player::Command::Audio), Command::Subtitle => self.player(player::Command::Subtitle),
                        Command::AudioTrack(id) => self.player(player::Command::AudioTrack(id)),
                        Command::SubtitleTrack(id) => self.player(player::Command::SubtitleTrack(id)),
                    }
                }
                Some((id, result)) = self.background_rx.recv() => if id == self.background_id {
                    if matches!(&result, BackgroundResult::Host(_) | BackgroundResult::Rooms(Err(_))) { self.background = None; } self.view.searching = false; self.view.hosting = false;
                    match result {
                        BackgroundResult::Rooms(Ok(rooms)) => self.view.rooms = rooms,
                        BackgroundResult::Rooms(Err(error)) => self.view.discovery_error = Some(error),
                        BackgroundResult::Host(Err(error)) => self.view.error = Some(error),
                        BackgroundResult::Host(Ok((listener, net))) => {
                            self.view.room_address = listener.local_addr().ok();
                            self.view.room_addresses = discovery::local_addresses(listener.local_addr().map(|addr| addr.port()).unwrap_or(crate::protocol::PORT));
                            self.view.role = Role::Host; self.clock = Clock::host();
                            self.session.state.position = self.view.player.position;
                            self.session.state.host_ms = self.now();
                            let (tx, rx) = mpsc::channel(64); net_rx = rx;
                            self.network = Some(tokio::spawn(network::rooms::host(listener, net, tx, self.origin)));
                        }
                    }
                },
                Some(event) = player_events.recv() => self.player_event(event),
                Some(file) = self.file_rx.recv() => self.file_event(file),
                Some(event) = net_rx.recv() => self.network_event(event),
                _ = monitor.tick() => { self.progress(); self.correct(); }
                _ = heartbeat.tick() => {
                    self.refresh_presence();
                    if self.view.role == Role::Host { self.publish(); }
                    else if self.view.role == Role::Guest && self.view.connected {
                        self.send(Message::Presence(self.presence.clone()));
                        let sent_ms = self.now(); self.pings.push_back(sent_ms);
                        while self.pings.len() > 16 { self.pings.pop_front(); }
                        self.send(Message::Ping { sent_ms });
                    }
                    self.update_view(); if views.send(self.view.clone()).is_err() { break; }
                }
            }
        }
        self.leave();
        drop(player_events);
    }
    fn file_event(&mut self, file: FileResult) {
        if !self.view.loading || file.generation != self.generation {
            return;
        }
        match file.event {
            FileEvent::Selected(name) => {
                self.view.pending_file = Some(name.clone());
                self.presence.file_name = Some(name);
            }
            FileEvent::Progress(progress) => {
                self.view.verification = Some(progress);
                self.presence.verification = Some(progress);
            }
            FileEvent::Finished(result) => {
                match result {
                    Ok(Some((path, media))) => {
                        self.previous = None;
                        if self.view.role == Role::Host {
                            self.session.apply(Control::Pause, self.now());
                            self.session.state.position = 0.0;
                            self.session.state.host_ms = self.now();
                        }
                        self.reset_operation();
                        *self.player.frames.lock().unwrap() = None;
                        self.view.file = self.view.pending_file.take();
                        self.presence.media = Some(media);
                        self.presence.ready = false;
                        self.view.player = player::Status::default();
                        self.player(player::Command::Load(path, self.generation));
                    }
                    Ok(None) => {
                        self.generation = self.view.player.generation;
                        self.presence = self.previous.take().unwrap_or_default();
                    }
                    Err(error) => {
                        self.view.error = Some(error);
                        self.generation = self.view.player.generation;
                        self.presence = self.previous.take().unwrap_or_default();
                    }
                }
                self.file_cancel = None;
                self.view.loading = false;
                self.view.pending_file = None;
                self.view.verification = None;
                self.presence.loading = false;
                self.presence.verification = None;
            }
        }
    }
    fn player_event(&mut self, event: player::Event) {
        match event {
            player::Event::Status(status) if status.generation == self.generation => {
                if self.view.role == Role::Local
                    && status.loaded
                    && !self.view.loading
                    && (!self.view.player.loaded
                        || self.view.player.generation != status.generation)
                {
                    self.view.watching = true;
                }
                self.view.player = status;
                self.refresh_presence();
                if self.presence.blocked
                    && let Some(id) = self.scheduled
                {
                    self.operation_failed(id);
                }
            }
            player::Event::Status(_) => {}
            player::Event::Prepared { id }
                if self
                    .player_operation
                    .is_some_and(|(token, wire)| token == id && self.requested == Some(wire)) =>
            {
                let id = self.player_operation.unwrap().1;
                self.prepared = Some(id);
                if self.view.role == Role::Host {
                    self.session.prepared(id, false, self.now());
                    self.publish();
                } else if self.view.role == Role::Guest {
                    self.send(Message::Prepared { id });
                }
            }
            player::Event::Prepared { .. } => {}
            player::Event::Started { id, at }
                if self
                    .player_operation
                    .is_some_and(|(token, wire)| token == id && self.scheduled == Some(wire)) =>
            {
                let id = self.player_operation.unwrap().1;
                let local = at.duration_since(self.origin).as_secs_f64() * 1000.0;
                self.view.last_start_ms = self.clock.host_now(local);
                self.view.last_start_id = Some(id);
                self.controller.reset(self.now());
            }
            player::Event::Started { .. } => {}
            player::Event::Missed { id } => {
                if let Some((token, wire)) = self.player_operation
                    && token == id
                {
                    self.operation_failed(wire);
                }
            }
            player::Event::Unavailable(error) => {
                self.fatal_error = Some(error);
                self.control(Control::Pause);
            }
            player::Event::Error(error) => {
                self.view.error = Some(error);
                self.presence.ready = false;
                self.presence.blocked = true;
                self.control(Control::Pause);
            }
        }
        self.progress();
    }
    fn refresh_presence(&mut self) {
        let p = &self.view.player;
        let now = self.now();
        self.presence.ready = p.loaded
            && p.generation == self.generation
            && self.presence.media.is_some()
            && self.fatal_error.is_none()
            && now - p.measured_ms < 1500.0;
        self.presence.blocked = p.blocked || self.fatal_error.is_some();
        self.presence.ended = p.ended;
        self.presence.duration = p.duration;
        self.presence.clock_ready = self.clock.ready(now);
        self.presence.rtt_ms = self.clock.rtt_ms;
        self.presence.uncertainty_ms = self.clock.uncertainty_ms;
        self.presence.playback = self.clock.host_now(p.measured_ms).map(|host_ms| Playback {
            position: p.position,
            host_ms,
            speed: p.speed,
            paused: p.paused || p.blocked,
            seeking: p.seeking,
        });
        if self.view.role == Role::Host {
            self.session.state.host = self.presence.clone();
        }
    }
    fn publish(&mut self) {
        if self.view.role != Role::Host {
            return;
        }
        self.refresh_presence();
        let state = self.session.snapshot(self.now());
        self.send(Message::State(Box::new(state.clone())));
        self.apply_snapshot(state);
    }
    fn network_event(&mut self, event: network::Event) {
        match event {
            network::Event::ApprovalRequested {
                id,
                name,
                address,
                reply,
            } => {
                self.view.join_request = Some(JoinRequest { id, name, address });
                self.approval = Some((id, reply));
            }
            network::Event::ApprovalFinished { id } => {
                if self.view.join_request.as_ref().is_some_and(|r| r.id == id) {
                    self.view.join_request = None;
                    self.approval = None;
                }
            }
            network::Event::WaitingApproval => self.view.waiting_approval = true,
            network::Event::Rejected(error) => {
                self.leave();
                self.view.error = Some(error);
            }
            network::Event::Connected(tx) => {
                self.view.waiting_approval = false;
                if self.view.watching {
                    self.view.notice =
                        Some("Conexão restabelecida. Aperte reproduzir para continuar.".into());
                }
                self.disconnected();
                self.outgoing = Some(tx);
                self.view.connected = true;
                self.view.error = None;
                self.session.connected = true;
                self.recover = self.view.role == Role::Host;
                if self.view.role == Role::Host {
                    self.clock = Clock::host();
                    self.publish();
                } else {
                    self.refresh_presence();
                    self.send(Message::Presence(self.presence.clone()));
                }
            }
            network::Event::Disconnected => self.disconnected(),
            network::Event::Error(error) => self.view.error = Some(error),
            network::Event::DiscoveryUnavailable => self.view.discovery_error = Some("A sala está aberta, mas a descoberta não está disponível. Compartilhe um endereço nos detalhes da conexão.".into()),
            network::Event::Message(message, received_at) => {
                let received_ms = received_at.duration_since(self.origin).as_secs_f64() * 1000.0;
                match (self.view.role, *message) {
                    (Role::Host, Message::Presence(p)) if p.valid() => {
                        self.session.state.guest = p;
                        self.progress();
                    }
                    (Role::Host, Message::Request(c)) => self.control(c),
                    (Role::Host, Message::Prepared { id }) => {
                        self.refresh_presence();
                        self.session.prepared(id, true, self.now());
                        self.publish();
                    }
                    (Role::Host, Message::Armed { id }) => {
                        self.session.armed(id, self.now());
                        self.publish();
                    }
                    (Role::Host, Message::Failed { id }) => {
                        self.session.fail(id, self.now());
                        self.publish();
                    }
                    (Role::Host, Message::Ping { sent_ms }) if sent_ms.is_finite() => {
                        self.send(Message::Pong {
                            sent_ms,
                            received_ms,
                            sent_host_ms: self.now(),
                        })
                    }
                    (Role::Guest, Message::State(state)) if state.valid() => {
                        if self.snapshot.as_ref().is_none_or(|old| {
                            state.revision >= old.revision && state.host_ms >= old.host_ms
                        }) {
                            self.apply_snapshot(*state);
                        }
                    }
                    (
                        Role::Guest,
                        Message::Pong {
                            sent_ms,
                            received_ms: t2,
                            sent_host_ms: t3,
                        },
                    ) => {
                        if let Some(index) = self
                            .pings
                            .iter()
                            .position(|sent| (*sent - sent_ms).abs() < 0.001)
                        {
                            self.pings.remove(index);
                            self.clock.sample(sent_ms, t2, t3, received_ms);
                            self.refresh_presence();
                        }
                    }
                    _ => self.disconnected(),
                }
            }
        }
    }
    fn operation_failed(&mut self, id: u64) {
        if self.requested != Some(id) && self.scheduled != Some(id) {
            return;
        }
        self.reset_operation();
        self.failed = Some(id);
        self.view.notice =
            Some("Início cancelado. A sessão está pausada; tente reproduzir novamente.".into());
        if self.view.role == Role::Host {
            self.session.fail(id, self.now());
            self.publish();
        } else {
            self.send(Message::Failed { id });
        }
    }
    fn apply_snapshot(&mut self, state: Snapshot) {
        if self.snapshot.as_ref().is_some_and(|old| {
            old.host.media != state.host.media || old.guest.media != state.guest.media
        }) {
            self.view.watching = false;
        }
        if !state.paused || state.transition.as_ref().is_some_and(|t| t.resume) {
            self.view.watching = true;
        }

        self.snapshot = Some(state.clone());
        if let Some(t) = &state.transition {
            if self.failed == Some(t.id) {
                return;
            }
            if self.requested != Some(t.id) {
                self.reset_operation();
                self.requested = Some(t.id);
                self.player_token += 1;
                self.player_operation = Some((self.player_token, t.id));
                self.view.notice = None;
                self.player(player::Command::Prepare {
                    id: self.player_token,
                    position: t.position,
                });
                self.speed = 1.0;
            }
            if let Some(start) = t.start_at_ms {
                if self.prepared != Some(t.id) {
                    self.operation_failed(t.id);
                    return;
                }
                if self.scheduled != Some(t.id)
                    && (self.view.role == Role::Guest || self.session.host_can_arm())
                {
                    let Some(host_now) = self.clock.host_now(self.now()) else {
                        self.operation_failed(t.id);
                        return;
                    };
                    if start - host_now < 50.0 {
                        self.operation_failed(t.id);
                        return;
                    }
                    let at = Instant::now() + Duration::from_secs_f64((start - host_now) / 1000.0);
                    self.scheduled = Some(t.id);
                    self.view.scheduled_start_ms = Some(start);
                    self.player(player::Command::Schedule {
                        id: self.player_operation.unwrap().0,
                        at,
                    });
                    if self.view.role == Role::Guest {
                        self.send(Message::Armed { id: t.id });
                    }
                }
            }
        } else if state.paused && self.last_pause != Some(state.revision) {
            self.reset_operation();
            self.last_pause = Some(state.revision);
        }
    }
    fn progress(&mut self) {
        if self.view.role == Role::Local {
            return;
        }
        self.refresh_presence();
        if self.view.role == Role::Host {
            let before = self.session.state.revision;
            if self.recover && self.session.ready() {
                self.recover = false;
                self.session
                    .apply(Control::Seek(self.session.position(self.now())), self.now());
            }
            if self.session.advance(self.now()) {
                self.view.notice = Some(
                    "Os players não confirmaram o início a tempo. A sessão está pausada.".into(),
                );
            }
            if self.session.state.revision != before {
                self.publish();
            }
        } else if (!self.clock.ready(self.now()) || !self.presence.ready || self.presence.blocked)
            && let Some(id) = self.scheduled
        {
            self.operation_failed(id);
        }
    }
    fn correct(&mut self) {
        if self.view.role == Role::Local {
            return;
        }
        let Some(state) = self.snapshot.clone() else {
            return;
        };
        if state.transition.is_some()
            || !self.presence.ready
            || self.view.player.seeking
            || self.now() - self.view.player.measured_ms > 350.0
        {
            self.set_speed(1.0);
            return;
        }
        let Some(now) = self.clock.host_now(self.now()) else {
            self.set_speed(1.0);
            return;
        };
        let matched = state.host.media.is_some() && state.host.media == state.guest.media;
        if !matched {
            self.set_speed(1.0);
            return;
        }
        let actual = self.view.player.position_at(self.now());
        let target = state.position_at(now);
        match self
            .controller
            .update(actual, target, state.paused, self.now())
        {
            Correction::Seek(p) => {
                self.player(player::Command::Seek(p.min(self.view.player.duration)));
                self.set_speed(1.0);
            }
            Correction::Speed(speed) => self.set_speed(speed),
            Correction::None => self.set_speed(1.0),
        }
    }
    fn update_view(&mut self) {
        if let Some(error) = &self.fatal_error {
            self.view.error = Some(error.clone());
            self.view.player.loaded = false;
        }
        self.view.ready = false;
        self.view.matched = false;
        self.view.peer_ready = false;
        self.view.verified = self.presence.media.is_some() && !self.view.loading;
        self.view.peer_file = None;
        self.view.peer_loading = false;
        self.view.peer_verification = None;
        self.view.peer_verified = false;
        self.view.peer_blocked = false;
        self.view.preparing = false;
        self.view.scheduled = false;
        self.view.peer_drift = None;
        self.view.rtt_ms = self.clock.rtt_ms;
        if let Some(state) = &self.snapshot {
            if self.view.connected {
                let peer = if self.view.role == Role::Host {
                    &state.guest
                } else {
                    &state.host
                };
                self.view.peer_file = peer.file_name.clone();
                self.view.peer_loading = peer.loading;
                self.view.peer_verification = peer.verification;
                self.view.peer_verified = peer.media.is_some();
                self.view.peer_blocked = peer.blocked;
            }
            self.view.peer_ready = if self.view.role == Role::Host {
                state.guest.ready
            } else {
                state.host.ready
            };
            self.view.matched = self.view.connected
                && state.host.media.is_some()
                && state.host.media == state.guest.media;
            self.view.ready = self.view.matched
                && state.host.ready
                && state.guest.ready
                && state.host.clock_ready
                && state.guest.clock_ready
                && state.transition.is_none()
                && !state.host.blocked
                && !state.guest.blocked
                && self.clock.ready(self.now());
            self.view.preparing = state
                .transition
                .as_ref()
                .is_some_and(|t| t.start_at_ms.is_none());
            self.view.scheduled = state
                .transition
                .as_ref()
                .is_some_and(|t| t.start_at_ms.is_some());
            if let Some(now) = self.clock.host_now(self.now()) {
                self.view.drift = state.position_at(now) - self.view.player.position_at(self.now());
                let peer = if self.view.role == Role::Host {
                    &state.guest
                } else {
                    &state.host
                };
                self.view.peer_drift = peer
                    .playback
                    .as_ref()
                    .filter(|p| (0.0..=350.0).contains(&(now - p.host_ms)))
                    .map(|p| self.view.player.position_at(self.now()) - p.position_at(now));
            }
        }
        self.view.status = match self.view.role {
            Role::Local => "Reprodução local".into(),
            Role::Host if !self.view.connected => "Sala aberta, aguardando parceiro".into(),
            Role::Guest if self.view.waiting_approval => "Aguardando aprovação do anfitrião".into(),
            Role::Guest if !self.view.connected => {
                "Conectando ou reconectando, reprodução pausada".into()
            }
            _ if self.view.loading => "Verificando o arquivo".into(),
            _ if self.view.preparing => "Preparando os dois players na mesma posição".into(),
            _ if self.view.scheduled => "Início agendado, aguardando o horário comum".into(),
            _ if !self.view.matched && self.presence.media.is_some() && self.view.peer_ready => {
                "Os arquivos são diferentes".into()
            }
            _ if !self.view.ready => "Aguardando os players e a medição da conexão".into(),
            _ if self.view.player.paused => "Sessão pausada".into(),
            _ => "Assistindo juntos".into(),
        };
    }
}
pub fn parse_address(input: &str) -> Result<SocketAddr, String> {
    let input = input.trim();
    let addr = input
        .parse::<SocketAddr>()
        .or_else(|_| {
            input
                .parse::<std::net::IpAddr>()
                .map(|ip| SocketAddr::new(ip, PORT))
        })
        .map_err(|_| {
            "Informe o IP do anfitrião, com porta opcional. Para IPv6 com porta, use [IP]:porta."
                .to_owned()
        })?;
    if addr.port() == 0 || !network::allowed_address(addr) {
        return Err("Use um endereço IP de destino e uma porta válida.".into());
    }
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_restores_the_loaded_file_and_ignores_late_events() {
        let (player, commands) = player::Player::test_channel();
        let mut worker = Worker::new(player, Instant::now());
        let original = Presence {
            media: Some(Media {
                hash: "a".repeat(64),
                bytes: 42,
            }),
            file_name: Some("original.mkv".into()),
            ready: true,
            ..Default::default()
        };
        worker.view.file = original.file_name.clone();
        worker.view.player.generation = 4;
        worker.view.player.loaded = true;
        worker.view.loading = true;
        worker.generation = 5;
        worker.previous = Some(original.clone());
        worker.presence.loading = true;
        let cancel = Arc::new(AtomicBool::new(false));
        worker.file_cancel = Some(cancel.clone());
        worker.file_event(FileResult {
            generation: 5,
            event: FileEvent::Selected("new.mkv".into()),
        });
        worker.file_event(FileResult {
            generation: 5,
            event: FileEvent::Progress(Verification {
                bytes: 50,
                total: 100,
            }),
        });
        assert_eq!(worker.view.pending_file.as_deref(), Some("new.mkv"));
        assert_eq!(worker.presence.verification.unwrap().bytes, 50);
        worker.cancel_open();
        assert!(cancel.load(Ordering::Relaxed));
        assert_eq!(worker.presence.file_name, original.file_name);
        assert_eq!(worker.presence.media, original.media);
        assert!(!worker.view.loading);
        assert!(worker.view.pending_file.is_none() && worker.view.verification.is_none());
        worker.file_event(FileResult {
            generation: 5,
            event: FileEvent::Finished(Ok(Some((
                PathBuf::from("unused.mkv"),
                original.media.unwrap(),
            )))),
        });
        assert_eq!(worker.view.file.as_deref(), Some("original.mkv"));
        assert!(
            commands.try_recv().is_err(),
            "a cancelled file must never reach the player"
        );
        worker.view.loading = true;
        worker.generation = 6;
        worker.file_event(FileResult {
            generation: 5,
            event: FileEvent::Selected("stale.mkv".into()),
        });
        assert!(worker.view.pending_file.is_none());
    }

    #[test]
    fn both_roles_receive_peer_metadata_and_disconnect_clears_it() {
        for role in [Role::Host, Role::Guest] {
            let (player, _commands) = player::Player::test_channel();
            let mut worker = Worker::new(player, Instant::now());
            worker.view.role = role;
            worker.view.connected = true;
            let mut state = Session::default().state;
            state.host.file_name = Some("host.mkv".into());
            state.guest.file_name = Some("guest.mp4".into());
            state.host.loading = true;
            state.guest.loading = true;
            state.host.verification = Some(Verification {
                bytes: 10,
                total: 100,
            });
            state.guest.verification = Some(Verification {
                bytes: 30,
                total: 100,
            });
            worker.snapshot = Some(state);
            worker.update_view();
            assert_eq!(
                worker.view.peer_file.as_deref(),
                Some(if role == Role::Host {
                    "guest.mp4"
                } else {
                    "host.mkv"
                })
            );
            assert!(worker.view.peer_loading);
            assert_eq!(
                worker.view.peer_verification.unwrap().bytes,
                if role == Role::Host { 30 } else { 10 }
            );
            worker.disconnected();
            worker.update_view();
            assert!(worker.view.peer_file.is_none());
            assert!(worker.view.peer_verification.is_none());
            assert!(!worker.view.peer_loading);
        }
    }
}

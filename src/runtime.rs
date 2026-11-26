mod files;
mod rooms;
mod stream;
mod sync;
mod view;
mod web_stream;

use self::{
    files::FileResult,
    rooms::BackgroundResult,
    stream::{GuestSession, StreamPrepResult},
};
use crate::{
    discovery, network, player,
    protocol::{Control, Media, Message, PORT, Presence, Snapshot, StreamProgress, Verification},
    session::Session,
    stream::{ChunkIndex, StreamEvent, StreamTelemetry, host::HostPending},
    sync::{Clock, Controller},
};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool, mpsc as std_mpsc},
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
    pub peer_left: bool,
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
    pub buffering: bool,
    pub peer_buffering: bool,
    pub waiting_buffer: bool,
    pub paused_by: Option<crate::protocol::Side>,
    pub loading: bool,
    pub player: player::Status,
    pub matched: bool,
    pub ready: bool,
    pub peer_ready: bool,
    pub rtt_ms: f64,
    pub drift: f64,
    pub peer_drift: Option<f64>,
    pub room_key: String,
    pub internet: bool,
    pub room_code: Option<String>,
    pub web_link: Option<String>,
    pub relay_offline: bool,
    pub preparing: bool,
    pub scheduled: bool,
    pub last_start_ms: Option<f64>,
    pub last_start_id: Option<u64>,
    pub scheduled_start_ms: Option<f64>,
    pub stream: StreamTelemetry,
    pub peer_stream: Option<StreamProgress>,
    pub can_stream: bool,
    pub peer_streaming: bool,
}

pub enum Command {
    Open,
    OpenPath(PathBuf),
    CancelOpen,
    StartStream,
    StopStream,
    Host(String),
    HostAuto(u16),
    HostRoom(String),
    Discover,
    CancelDiscovery,
    RequestJoin(String),
    Approve { id: u64, accept: bool },
    Join(String, String),
    HostInternet(String),
    JoinCode { relay: String, code: String },
    Leave,
    Control(Control),
    Volume(f64),
    Audio,
    Subtitle,
    AudioTrack(i64),
    SubtitleTrack(Option<i64>),
    SetLanguage(crate::i18n::Language),
    Shutdown,
}

pub struct Runtime {
    pub commands: mpsc::Sender<Command>,
    pub views: std_mpsc::Receiver<View>,
    pub frames: player::Frames,
    origin: Instant,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Runtime {
    pub fn position_now(&self, view: &View) -> f64 {
        view.player
            .position_at(self.origin.elapsed().as_secs_f64() * 1000.0)
    }

    pub fn start() -> Self {
        Self::start_with_renderer(None)
    }

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
        let join = std::thread::spawn(move || {
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
                        error: Some(crate::i18n::text().errors.runtime_start.into()),
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
            join: Some(join),
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = self.commands.blocking_send(Command::Shutdown);
            let _ = join.join();
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.commands.try_send(Command::Shutdown);
    }
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
    farewell: Option<JoinHandle<()>>,
    net_rx: Option<mpsc::Receiver<network::Event>>,
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
    watching_media: Option<Media>,
    buffer_latch: sync::BufferLatch,
    local_media: Option<(PathBuf, Arc<ChunkIndex>)>,
    guest_stream: Option<GuestSession>,
    stream_id: u64,
    stream_pending: Option<HostPending>,
    stream_task: Option<JoinHandle<()>>,
    stream_waiting_since: Option<Instant>,
    stream_reconnect_at: Option<Instant>,
    stream_reconnect_attempts: u32,
    pending_stream_request: bool,
    last_stream_request: Option<Instant>,
    stream_tx: mpsc::Sender<StreamEvent>,
    stream_rx: mpsc::Receiver<StreamEvent>,
    stream_prep_tx: mpsc::Sender<StreamPrepResult>,
    stream_prep_rx: mpsc::Receiver<StreamPrepResult>,
}

impl Worker {
    fn new(player: player::Player, origin: Instant) -> Self {
        let (files, file_rx) = mpsc::channel(4);
        let (background_tx, background_rx) = mpsc::channel(4);
        let (stream_tx, stream_rx) = mpsc::channel(32);
        let (stream_prep_tx, stream_prep_rx) = mpsc::channel(8);
        Self {
            player,
            background: None,
            background_id: 0,
            background_tx,
            background_rx,
            approval: None,
            origin,
            view: View {
                status: crate::i18n::text().common.local_playback.into(),
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
            farewell: None,
            net_rx: None,
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
            watching_media: None,
            buffer_latch: sync::BufferLatch::Idle,
            local_media: None,
            guest_stream: None,
            stream_id: 1,
            stream_pending: None,
            stream_task: None,
            stream_waiting_since: None,
            stream_reconnect_at: None,
            stream_reconnect_attempts: 0,
            pending_stream_request: false,
            last_stream_request: None,
            stream_tx,
            stream_rx,
            stream_prep_tx,
            stream_prep_rx,
        }
    }

    fn now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64() * 1000.0
    }

    fn player(&mut self, command: player::Command) {
        if self.player.commands.send(command).is_err() {
            self.fatal_error
                .get_or_insert_with(|| crate::i18n::text().errors.player_unavailable.into());
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
        self.on_control_disconnected();
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

    async fn handle_command(&mut self, command: Command) -> bool {
        match command {
            Command::Shutdown => return false,
            Command::Open => self.open(None),
            Command::OpenPath(path) => self.open(Some(path)),
            Command::CancelOpen => self.cancel_open(),
            Command::StartStream => self.start_stream(),
            Command::StopStream => self.stop_stream(true),
            Command::HostAuto(port) => {
                self.leave();
                self.background(Some(port));
            }
            Command::Discover => self.background(None),
            Command::CancelDiscovery => self.cancel_background(),
            Command::Approve { id, accept } => self.approve(id, accept),
            Command::HostRoom(address) => self.host_room(address).await,
            Command::RequestJoin(address) => self.request_join(address),
            Command::Host(address) => self.host_direct(address).await,
            Command::Join(address, key) => self.join_direct(address, key),
            Command::HostInternet(relay) => self.host_internet(relay),
            Command::JoinCode { relay, code } => self.join_code(relay, code),
            Command::Leave => self.leave(),
            Command::Control(control) => self.control(control),
            Command::Volume(v) => self.player(player::Command::Volume(v.clamp(0.0, 100.0))),
            Command::Audio => self.player(player::Command::Audio),
            Command::Subtitle => self.player(player::Command::Subtitle),
            Command::AudioTrack(id) => self.player(player::Command::AudioTrack(id)),
            Command::SubtitleTrack(id) => self.player(player::Command::SubtitleTrack(id)),
            Command::SetLanguage(language) => {
                crate::i18n::set(language);
                self.update_view();
                tokio::task::spawn_blocking(move || {
                    let mut preferences = crate::preferences::Preferences::load();
                    preferences.language = Some(language);
                    let _ = preferences.save();
                });
            }
        }
        true
    }

    async fn run(
        mut self,
        mut commands: mpsc::Receiver<Command>,
        mut player_events: mpsc::Receiver<player::Event>,
        views: std_mpsc::Sender<View>,
    ) {
        let mut heartbeat = tokio::time::interval(Duration::from_millis(200));
        let mut monitor = tokio::time::interval(Duration::from_millis(20));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        monitor.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break; };
                    if !self.handle_command(command).await {
                        break;
                    }
                }
                Some((id, result)) = self.background_rx.recv() => self.background_event(id, result),
                Some(event) = player_events.recv() => self.player_event(event),
                Some(file) = self.file_rx.recv() => self.file_event(file),
                Some(prep) = self.stream_prep_rx.recv() => self.stream_prep_event(prep),
                Some(event) = self.stream_rx.recv() => self.stream_event(event),
                event = recv_network(&mut self.net_rx) => self.network_event(event),
                _ = monitor.tick() => {
                    self.progress();
                    self.correct();
                }
                _ = heartbeat.tick() => {
                    self.refresh_presence();
                    if self.view.role == Role::Host {
                        self.publish();
                    } else if self.view.role == Role::Guest && self.view.connected {
                        self.send(Message::Presence(Box::new(self.presence.clone())));
                        let sent_ms = self.now();
                        self.pings.push_back(sent_ms);
                        while self.pings.len() > 16 {
                            self.pings.pop_front();
                        }
                        self.send(Message::Ping { sent_ms });
                    }
                    self.update_view();
                    if views.send(self.view.clone()).is_err() {
                        break;
                    }
                }
            }
        }
        self.leave();
        if let Some(farewell) = self.farewell.take() {
            let _ = farewell.await;
        }
        drop(player_events);
    }
}

async fn recv_network(rx: &mut Option<mpsc::Receiver<network::Event>>) -> network::Event {
    loop {
        match rx {
            Some(channel) => match channel.recv().await {
                Some(event) => return event,
                None => *rx = None,
            },
            None => std::future::pending().await,
        }
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
        .map_err(|_| crate::i18n::text().errors.address_format.to_owned())?;
    if addr.port() == 0 || !network::allowed_address(addr) {
        return Err(crate::i18n::text().errors.invalid_address.into());
    }
    Ok(addr)
}

use super::{
    ChunkIndex, StreamEvent, StreamPhase, StreamTelemetry, file_bitrate_bps, file_io, nat,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::File,
    io,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use str0m::{
    Candidate, Event, IceConnectionState, Input, Output, Rtc,
    change::SdpOffer,
    channel::ChannelId,
    net::{Protocol, Receive},
};
use sync2gether_core::datachannel::{self, Item, Request};
use tokio::{net::UdpSocket, sync::mpsc, task::JoinHandle};

const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
const HIGH_WATER: usize = 1024 * 1024;
const TELEMETRY: Duration = Duration::from_millis(500);
const MAX_QUEUE: usize = 64;

pub struct WebHost {
    rtc: Rtc,
    sockets: HashMap<SocketAddr, Arc<UdpSocket>>,
    path: PathBuf,
    index: Arc<ChunkIndex>,
    duration: f64,
}

fn local_addresses(allow_loopback: bool) -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = if_addrs::get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .map(|iface| iface.ip())
                .filter(|ip| ip.is_ipv4() && !ip.is_loopback() && !ip.is_unspecified())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.dedup();
    out.truncate(nat::MAX_CANDIDATES);
    if allow_loopback || out.is_empty() {
        out.push(IpAddr::from([127, 0, 0, 1]));
    }
    out
}

pub async fn prepare(
    offer: &str,
    path: PathBuf,
    index: Arc<ChunkIndex>,
    duration: f64,
    stun: bool,
    allow_loopback: bool,
) -> Result<(String, WebHost), String> {
    let offer = SdpOffer::from_sdp_string(offer)
        .map_err(|_| "O navegador enviou uma oferta de streaming inválida.".to_owned())?;
    let mut bound = Vec::new();
    for ip in local_addresses(allow_loopback) {
        let Ok(socket) = std::net::UdpSocket::bind(SocketAddr::new(ip, 0)) else {
            continue;
        };
        if socket.set_nonblocking(true).is_err() {
            continue;
        }
        let Ok(local) = socket.local_addr() else {
            continue;
        };
        let reflexive = if stun && !ip.is_loopback() {
            nat::stun_discover(&socket, nat::DEFAULT_STUN_SERVERS).await
        } else {
            None
        };
        bound.push((socket, local, reflexive));
    }
    if bound.is_empty() {
        return Err("Não foi possível abrir uma porta UDP para o streaming.".into());
    }
    let mut host = WebHost {
        rtc: Rtc::builder().build(Instant::now()),
        sockets: HashMap::new(),
        path,
        index,
        duration,
    };
    for (socket, local, reflexive) in bound {
        let socket = UdpSocket::from_std(socket).map_err(|e| e.to_string())?;
        host.sockets.insert(local, Arc::new(socket));
        if let Ok(candidate) = Candidate::host(local, "udp") {
            host.rtc.add_local_candidate(candidate);
            host.drain(&mut Vec::new())?;
        }
        if let Some(mapped) = reflexive.filter(|mapped| *mapped != local && mapped.is_ipv4())
            && let Ok(candidate) = Candidate::server_reflexive(mapped, local, "udp")
        {
            host.rtc.add_local_candidate(candidate);
            host.drain(&mut Vec::new())?;
        }
    }
    let answer =
        host.rtc.sdp_api().accept_offer(offer).map_err(|_| {
            "Não foi possível responder à oferta de streaming do navegador.".to_owned()
        })?;
    host.drain(&mut Vec::new())?;
    Ok((answer.to_sdp_string(), host))
}

enum Happening {
    Opened(ChannelId),
    Data(Vec<u8>),
    Closed,
    Lost,
}

struct Outgoing {
    pieces: VecDeque<Vec<u8>>,
}

impl WebHost {
    pub fn chunk_size(&self) -> u32 {
        self.index.chunk_size
    }
    pub fn digest(&self) -> &str {
        &self.index.digest
    }

    fn drain(&mut self, happenings: &mut Vec<Happening>) -> Result<Instant, String> {
        loop {
            match self.rtc.poll_output() {
                Ok(Output::Timeout(at)) => return Ok(at),
                Ok(Output::Transmit(transmit)) => {
                    if let Some(socket) = self.sockets.get(&transmit.source) {
                        let _ = socket.try_send_to(&transmit.contents, transmit.destination);
                    }
                }
                Ok(Output::Event(event)) => match event {
                    Event::ChannelOpen(id, label) if label == datachannel::LABEL => {
                        happenings.push(Happening::Opened(id))
                    }
                    Event::ChannelData(data) if data.binary => {
                        happenings.push(Happening::Data(data.data))
                    }
                    Event::ChannelClose(_) => happenings.push(Happening::Closed),
                    Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                        happenings.push(Happening::Lost)
                    }
                    _ => {}
                },
                Err(_) => return Err("A conexão de streaming com o navegador falhou.".into()),
            }
        }
    }

    pub fn run(self, id: u64, events: mpsc::Sender<StreamEvent>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let outcome = self.serve(id, &events).await;
            let _ = events
                .send(match outcome {
                    Ok(()) => StreamEvent::Ended { id },
                    Err(error) => StreamEvent::Failed { id, error },
                })
                .await;
        })
    }

    async fn serve(mut self, id: u64, events: &mpsc::Sender<StreamEvent>) -> Result<(), String> {
        let file = Arc::new(
            File::open(&self.path)
                .map_err(|_| "Não foi possível abrir o arquivo para streaming.".to_owned())?,
        );
        let (packets_tx, mut packets) = mpsc::channel::<(SocketAddr, SocketAddr, Vec<u8>)>(256);
        let mut readers = Vec::new();
        for (local, socket) in &self.sockets {
            let (local, socket, tx) = (*local, socket.clone(), packets_tx.clone());
            readers.push(tokio::spawn(async move {
                let mut buffer = vec![0; 2048];
                while let Ok((n, source)) = socket.recv_from(&mut buffer).await {
                    if tx
                        .send((local, source, buffer[..n].to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        let _readers = AbortOnDrop(readers);
        let mut telemetry = Telemetry::new(&self.index, self.duration);
        let _ = events
            .send(StreamEvent::Telemetry {
                id,
                telemetry: telemetry.snapshot(StreamPhase::Connecting),
            })
            .await;
        let started = Instant::now();
        let mut channel: Option<ChannelId> = None;
        let mut queue: VecDeque<Item> = VecDeque::new();
        let mut current: Option<Outgoing> = None;
        let mut reading: Option<JoinHandle<(u32, io::Result<Vec<u8>>)>> = None;
        let mut happenings = Vec::new();
        let mut report = tokio::time::interval(TELEMETRY);
        report.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut deadline = self.drain(&mut happenings)?;
        loop {
            for happening in happenings.drain(..) {
                match happening {
                    Happening::Opened(opened) => channel = Some(opened),
                    Happening::Data(bytes) => match Request::decode(&bytes) {
                        Ok(Request::Cancel) => {
                            queue.clear();
                            current = None;
                            if let Some(task) = reading.take() {
                                task.abort();
                            }
                        }
                        Ok(Request::HashTable) if queue.len() < MAX_QUEUE => {
                            queue.push_back(Item::HashTable)
                        }
                        Ok(Request::Chunk(index)) if queue.len() < MAX_QUEUE => {
                            queue.push_back(Item::Chunk(index))
                        }
                        _ => {
                            return Err(
                                "O navegador enviou um pedido de streaming inválido.".into()
                            );
                        }
                    },
                    Happening::Closed => return Ok(()),
                    Happening::Lost => {
                        return Err("A conexão direta com o navegador caiu.".into());
                    }
                }
            }
            if channel.is_none() && started.elapsed() > OPEN_TIMEOUT {
                return Err(
                    "O navegador não conseguiu se conectar diretamente a este computador.".into(),
                );
            }
            if let Some(open) = channel {
                deadline = self.send_pending(open, &mut current, &mut happenings)?;
                if current.is_none() && reading.is_none() {
                    match queue.pop_front() {
                        Some(Item::HashTable) => {
                            let table = self.index.serialize_hashes();
                            current = Some(Outgoing {
                                pieces: datachannel::fragments(Item::HashTable, &table).collect(),
                            });
                        }
                        Some(Item::Chunk(index)) => {
                            reading = Some(self.read_chunk(&file, index));
                        }
                        None => {}
                    }
                    if current.is_some() {
                        continue;
                    }
                }
            }
            let wait = deadline.saturating_duration_since(Instant::now());
            tokio::select! {
                packet = packets.recv() => {
                    let Some((local, source, bytes)) = packet else {
                        return Err("A conexão de streaming com o navegador falhou.".into());
                    };
                    if let Ok(receive) = Receive::new(Protocol::Udp, source, local, &bytes) {
                        self.rtc
                            .handle_input(Input::Receive(Instant::now(), receive))
                            .map_err(|_| "A conexão de streaming com o navegador falhou.".to_owned())?;
                    }
                    deadline = self.drain(&mut happenings)?;
                }
                _ = tokio::time::sleep(wait) => {
                    self.rtc
                        .handle_input(Input::Timeout(Instant::now()))
                        .map_err(|_| "A conexão de streaming com o navegador falhou.".to_owned())?;
                    deadline = self.drain(&mut happenings)?;
                }
                done = async { reading.as_mut().unwrap().await }, if reading.is_some() => {
                    reading = None;
                    let pieces = match done {
                        Ok((index, Ok(data))) => {
                            telemetry.sent_chunk(index);
                            datachannel::fragments(Item::Chunk(index), &data).collect()
                        }
                        Ok((index, Err(_))) => VecDeque::from([datachannel::failure(Item::Chunk(index))]),
                        Err(_) => VecDeque::new(),
                    };
                    current = Some(Outgoing { pieces });
                }
                _ = report.tick() => {
                    let phase = if channel.is_some() { StreamPhase::Streaming } else { StreamPhase::Connecting };
                    if events.send(StreamEvent::Telemetry { id, telemetry: telemetry.snapshot(phase) }).await.is_err() {
                        return Ok(());
                    }
                }
            }
            if !self.rtc.is_alive() {
                return Err("A conexão direta com o navegador caiu.".into());
            }
        }
    }

    fn read_chunk(&self, file: &Arc<File>, index: u32) -> JoinHandle<(u32, io::Result<Vec<u8>>)> {
        let file = file.clone();
        let chunks = &self.index;
        let valid = (index as usize) < chunks.hashes.len();
        let chunk_size = u64::from(chunks.chunk_size);
        let offset = u64::from(index) * chunk_size;
        let len = chunks.file_size.saturating_sub(offset).min(chunk_size) as usize;
        tokio::task::spawn_blocking(move || {
            if !valid {
                return (index, Err(io::Error::other("Bloco inexistente")));
            }
            let mut buffer = vec![0; len];
            let result = file_io::read_exact_at(&file, &mut buffer, offset).map(|()| buffer);
            (index, result)
        })
    }

    fn send_pending(
        &mut self,
        open: ChannelId,
        current: &mut Option<Outgoing>,
        happenings: &mut Vec<Happening>,
    ) -> Result<Instant, String> {
        while let Some(outgoing) = current.as_mut() {
            let Some(piece) = outgoing.pieces.front() else {
                *current = None;
                break;
            };
            let Some(mut handle) = self.rtc.channel(open) else {
                break;
            };
            if handle.buffered_amount() >= HIGH_WATER {
                break;
            }
            let accepted = handle
                .write(true, piece)
                .map_err(|_| "A conexão de streaming com o navegador falhou.".to_owned())?;
            self.drain(happenings)?;
            if !accepted {
                break;
            }
            outgoing.pieces.pop_front();
        }
        self.drain(happenings)
    }
}

struct AbortOnDrop(Vec<JoinHandle<()>>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

struct Telemetry {
    chunk_size: u64,
    total_bytes: u64,
    chunks_total: u64,
    bitrate: Option<u64>,
    unique: HashSet<u32>,
    sent: u64,
    previous: (u64, Instant),
}
impl Telemetry {
    fn new(index: &ChunkIndex, duration: f64) -> Self {
        Self {
            chunk_size: u64::from(index.chunk_size),
            total_bytes: index.file_size,
            chunks_total: index.hashes.len() as u64,
            bitrate: file_bitrate_bps(index.file_size, duration),
            unique: HashSet::new(),
            sent: 0,
            previous: (0, Instant::now()),
        }
    }
    fn sent_chunk(&mut self, index: u32) {
        self.unique.insert(index);
        self.sent += self.chunk_size;
    }
    fn snapshot(&mut self, phase: StreamPhase) -> StreamTelemetry {
        let now = Instant::now();
        let elapsed = now.duration_since(self.previous.1).as_secs_f64().max(0.001);
        let bps = ((self.sent.saturating_sub(self.previous.0)) as f64 * 8.0 / elapsed) as u64;
        self.previous = (self.sent, now);
        let unique = self.unique.len() as u64;
        StreamTelemetry {
            active: true,
            is_sender: true,
            phase,
            file_bitrate_bps: self.bitrate,
            transfer_bps: bps,
            cached_bytes: unique.saturating_mul(self.chunk_size).min(self.total_bytes),
            total_bytes: self.total_bytes,
            chunks_cached: unique.min(self.chunks_total),
            chunks_total: self.chunks_total,
            ..Default::default()
        }
    }
}

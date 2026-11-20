use super::{
    ChunkIndex, StreamEvent, StreamPhase, StreamTelemetry, cache::chunk_len_for, file_bitrate_bps,
    nat, tls, wire,
};
use crate::protocol::{Media, StreamAnswer, StreamOffer};
use std::{
    fs::File,
    io,
    net::{SocketAddr, UdpSocket},
    os::unix::fs::FileExt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinHandle};

const ACCEPT_TIMEOUT: Duration = Duration::from_secs(12);
const PUNCH_INTERVAL: Duration = Duration::from_millis(200);

pub struct HostPending {
    pub id: u64,
    socket: UdpSocket,
    identity: tls::Identity,
    path: PathBuf,
    index: Arc<ChunkIndex>,
    duration: f64,
}

pub async fn prepare(
    id: u64,
    path: PathBuf,
    media: Media,
    file_name: String,
    duration: f64,
    index: Arc<ChunkIndex>,
    stun_enabled: bool,
) -> io::Result<(StreamOffer, HostPending)> {
    let socket = nat::bind_udp()?;
    let identity = tls::Identity::generate()?;
    let candidates = nat::gather_candidates(&socket, stun_enabled).await;
    let offer = StreamOffer {
        id,
        media,
        file_name,
        duration,
        chunk_size: index.chunk_size,
        hash_table_digest: index.digest.clone(),
        cert_fingerprint: identity.fingerprint.clone(),
        candidates: candidates.into_iter().map(|a| a.to_string()).collect(),
    };
    Ok((
        offer,
        HostPending {
            id,
            socket,
            identity,
            path,
            index,
            duration,
        },
    ))
}

impl HostPending {
    pub fn accept(self, answer: StreamAnswer, events: mpsc::Sender<StreamEvent>) -> JoinHandle<()> {
        tokio::spawn(async move {
            if let Err(error) = run_host(self, answer, events.clone()).await {
                let _ = events
                    .send(StreamEvent::Failed {
                        id: error.0,
                        error: error.1,
                    })
                    .await;
            }
        })
    }
}

async fn run_host(
    pending: HostPending,
    answer: StreamAnswer,
    events: mpsc::Sender<StreamEvent>,
) -> Result<(), (u64, String)> {
    let id = pending.id;
    let remote_candidates: Vec<SocketAddr> = answer
        .candidates
        .iter()
        .filter_map(|c| c.parse::<SocketAddr>().ok())
        .filter(|a| crate::network::allowed_address(*a))
        .collect();
    if remote_candidates.is_empty() {
        return Err((id, "Parceiro não enviou endereços P2P válidos.".into()));
    }

    let punch_sock = pending
        .socket
        .try_clone()
        .map_err(|_| (id, "Falha ao preparar socket UDP para streaming.".into()))?;
    let server_config = tls::server_config(&pending.identity, &answer.cert_fingerprint)
        .map_err(|e| (id, format!("Falha ao configurar TLS P2P: {e}")))?;

    let endpoint = quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(server_config),
        pending.socket,
        Arc::new(quinn::TokioRuntime),
    )
    .map_err(|e| (id, format!("Falha ao abrir endpoint QUIC: {e}")))?;

    let file_bitrate = file_bitrate_bps(pending.index.file_size, pending.duration);
    let _ = events
        .send(StreamEvent::Telemetry {
            id,
            telemetry: StreamTelemetry {
                active: true,
                is_sender: true,
                phase: StreamPhase::Connecting,
                file_bitrate_bps: file_bitrate,
                total_bytes: pending.index.file_size,
                chunks_total: pending.index.hashes.len() as u64,
                ..Default::default()
            },
        })
        .await;

    let puncher = tokio::spawn(async move {
        let mut interval = tokio::time::interval(PUNCH_INTERVAL);
        for _ in 0..60 {
            interval.tick().await;
            nat::send_hole_punches(&punch_sock, &remote_candidates);
        }
    });

    let conn = match tokio::time::timeout(ACCEPT_TIMEOUT, endpoint.accept()).await {
        Ok(Some(incoming)) => {
            puncher.abort();
            incoming.await.map_err(|_| {
                (
                    id,
                    "Handshake P2P direto falhou. Verifique firewall ou NAT.".into(),
                )
            })?
        }
        _ => {
            puncher.abort();
            return Err((
                id,
                "Não foi possível abrir túnel P2P direto (NAT restrito). Tente na rede local, IPv6 ou VPN."
                    .into(),
            ));
        }
    };

    let peer_addr = conn.remote_address();
    let file = Arc::new(File::open(&pending.path).map_err(|_| {
        (
            id,
            "Não foi possível abrir o arquivo para streaming.".into(),
        )
    })?);
    let hash_table = Arc::new(pending.index.serialize_hashes());
    let bytes_sent = Arc::new(AtomicU64::new(0));
    let chunks_sent = Arc::new(AtomicU64::new(0));

    let telemetry_events = events.clone();
    let telemetry_bytes = bytes_sent.clone();
    let telemetry_chunks = chunks_sent.clone();
    let total_bytes = pending.index.file_size;
    let chunks_total = pending.index.hashes.len() as u64;
    let telemetry_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        let mut prev_bytes = 0u64;
        let mut prev_at = Instant::now();
        loop {
            interval.tick().await;
            let now = Instant::now();
            let cur_bytes = telemetry_bytes.load(Ordering::Relaxed);
            let dt = now.duration_since(prev_at).as_secs_f64().max(0.001);
            let bps = ((cur_bytes.saturating_sub(prev_bytes) as f64 * 8.0) / dt) as u64;
            prev_bytes = cur_bytes;
            prev_at = now;
            let _ = telemetry_events
                .send(StreamEvent::Telemetry {
                    id,
                    telemetry: StreamTelemetry {
                        active: true,
                        is_sender: true,
                        phase: StreamPhase::Streaming,
                        peer_addr: Some(peer_addr),
                        file_bitrate_bps: file_bitrate,
                        transfer_bps: bps,
                        cached_bytes: cur_bytes.min(total_bytes),
                        total_bytes,
                        chunks_cached: telemetry_chunks.load(Ordering::Relaxed).min(chunks_total),
                        chunks_total,
                        ..Default::default()
                    },
                })
                .await;
        }
    });

    while let Ok((mut send, mut recv)) = conn.accept_bi().await {
        let file = file.clone();
        let index = pending.index.clone();
        let hash_table = hash_table.clone();
        let bytes_sent = bytes_sent.clone();
        let chunks_sent = chunks_sent.clone();
        tokio::spawn(async move {
            let Ok(request) = wire::read_request(&mut recv).await else {
                return;
            };
            match request {
                wire::Request::HashTable => {
                    let _ = wire::write_ok_payload(&mut send, &hash_table).await;
                }
                wire::Request::Probe(size) => {
                    let probe = vec![0xA5u8; size as usize];
                    let _ = wire::write_ok_payload(&mut send, &probe).await;
                }
                wire::Request::Chunk(idx) => {
                    if idx as usize >= index.hashes.len() {
                        let _ = wire::write_error(&mut send).await;
                        return;
                    }
                    let offset = u64::from(idx) * u64::from(index.chunk_size);
                    let len = chunk_len_for(index.file_size, index.chunk_size, idx);
                    let read_result = tokio::task::spawn_blocking(move || {
                        let mut buf = vec![0u8; len];
                        file.read_exact_at(&mut buf, offset).map(|()| buf)
                    })
                    .await;
                    match read_result {
                        Ok(Ok(buf)) => {
                            if wire::write_ok_payload(&mut send, &buf).await.is_ok() {
                                bytes_sent.fetch_add(buf.len() as u64, Ordering::Relaxed);
                                chunks_sent.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        _ => {
                            let _ = wire::write_error(&mut send).await;
                        }
                    }
                }
            }
            let _ = send.finish();
        });
    }

    telemetry_task.abort();
    endpoint.close(0u32.into(), b"done");
    Ok(())
}

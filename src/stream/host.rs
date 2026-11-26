use super::{
    ChunkIndex, StreamEvent, StreamPhase, StreamTelemetry, cache::chunk_len_for, file_bitrate_bps,
    file_io, nat, tls, wire,
};
use crate::protocol::{Media, StreamAnswer, StreamOffer};
use std::{
    collections::HashSet,
    fs::File,
    io,
    net::UdpSocket,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};

const ACCEPT_TIMEOUT: Duration = Duration::from_secs(14);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(4);
const PUNCH_INTERVAL: Duration = Duration::from_millis(200);

struct EndpointGuard(quinn::Endpoint);
impl Drop for EndpointGuard {
    fn drop(&mut self) {
        self.0.close(0u32.into(), b"stop");
    }
}

pub struct HostPending {
    pub id: u64,
    socket: UdpSocket,
    identity: tls::Identity,
    path: PathBuf,
    index: Arc<ChunkIndex>,
    duration: f64,
    stun_enabled: bool,
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
            stun_enabled,
        },
    ))
}

impl HostPending {
    pub fn accept(self, answer: StreamAnswer, events: mpsc::Sender<StreamEvent>) -> JoinHandle<()> {
        let id = self.id;
        tokio::spawn(async move {
            match run_host(self, answer, events.clone()).await {
                Ok(()) => {
                    let _ = events.send(StreamEvent::Ended { id }).await;
                }
                Err(error) => {
                    let _ = events.send(StreamEvent::Failed { id, error }).await;
                }
            }
        })
    }
}

async fn run_host(
    pending: HostPending,
    answer: StreamAnswer,
    events: mpsc::Sender<StreamEvent>,
) -> Result<(), String> {
    let id = pending.id;
    let remote_candidates =
        nat::filter_remote_candidates(&answer.candidates, !pending.stun_enabled);
    if remote_candidates.is_empty() {
        return Err(crate::i18n::text().stream_errors.peer_no_addresses.into());
    }

    let punch_sock = pending
        .socket
        .try_clone()
        .map_err(|_| crate::i18n::text().stream_errors.udp_prepare.to_owned())?;
    let server_config = tls::server_config(&pending.identity, &answer.cert_fingerprint)
        .map_err(|e| (crate::i18n::text().stream_errors.tls_host)(&e.to_string()))?;

    let endpoint = quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(server_config),
        pending.socket,
        Arc::new(quinn::TokioRuntime),
    )
    .map_err(|e| (crate::i18n::text().stream_errors.quic_endpoint)(&e.to_string()))?;
    let _endpoint_guard = EndpointGuard(endpoint.clone());

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

    let mut children = JoinSet::new();
    children.spawn(async move {
        let mut interval = tokio::time::interval(PUNCH_INTERVAL);
        for _ in 0..70 {
            interval.tick().await;
            nat::send_hole_punches(&punch_sock, &remote_candidates);
        }
    });

    let (conn, first_bi) = accept_authenticated(&endpoint).await?;
    children.abort_all();

    let peer_addr = conn.remote_address();
    let file = Arc::new(
        File::open(&pending.path)
            .map_err(|_| crate::i18n::text().stream_errors.open_file.to_owned())?,
    );
    let hash_table = Arc::new(pending.index.serialize_hashes());
    let bytes_sent = Arc::new(AtomicU64::new(0));
    let unique_chunks = Arc::new(Mutex::new(HashSet::<u32>::new()));

    let telemetry_events = events.clone();
    let telemetry_bytes = bytes_sent.clone();
    let telemetry_chunks = unique_chunks.clone();
    let total_bytes = pending.index.file_size;
    let chunk_size = pending.index.chunk_size;
    let chunks_total = pending.index.hashes.len() as u64;
    children.spawn(async move {
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
            let unique_count = telemetry_chunks.lock().unwrap().len() as u64;
            let unique_bytes = unique_count
                .saturating_mul(u64::from(chunk_size))
                .min(total_bytes);
            if telemetry_events
                .send(StreamEvent::Telemetry {
                    id,
                    telemetry: StreamTelemetry {
                        active: true,
                        is_sender: true,
                        phase: StreamPhase::Streaming,
                        peer_addr: Some(peer_addr),
                        file_bitrate_bps: file_bitrate,
                        transfer_bps: bps,
                        cached_bytes: unique_bytes,
                        total_bytes,
                        chunks_cached: unique_count.min(chunks_total),
                        chunks_total,
                        ..Default::default()
                    },
                })
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let mut next_bi = Some(first_bi);
    loop {
        let (mut send, mut recv) = match next_bi.take() {
            Some(bi) => bi,
            None => match conn.accept_bi().await {
                Ok(bi) => bi,
                Err(_) => break,
            },
        };
        let file = file.clone();
        let index = pending.index.clone();
        let hash_table = hash_table.clone();
        let bytes_sent = bytes_sent.clone();
        let unique_chunks = unique_chunks.clone();
        children.spawn(async move {
            let Ok(request) = wire::read_request(&mut recv).await else {
                return;
            };
            match request {
                wire::Request::HashTable => {
                    let _ = wire::write_ok_payload(&mut send, &hash_table).await;
                }
                wire::Request::Probe(size) => {
                    let _ = wire::write_ok_probe(&mut send, size).await;
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
                        file_io::read_exact_at(&file, &mut buf, offset).map(|()| buf)
                    })
                    .await;
                    match read_result {
                        Ok(Ok(buf)) => {
                            if wire::write_ok_payload(&mut send, &buf).await.is_ok() {
                                bytes_sent.fetch_add(buf.len() as u64, Ordering::Relaxed);
                                unique_chunks.lock().unwrap().insert(idx);
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

    children.abort_all();
    Ok(())
}

type BidiStream = (quinn::SendStream, quinn::RecvStream);

async fn accept_authenticated(
    endpoint: &quinn::Endpoint,
) -> Result<(quinn::Connection, BidiStream), String> {
    let deadline = tokio::time::Instant::now() + ACCEPT_TIMEOUT;
    let mut handshakes = JoinSet::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            handshakes.abort_all();
            return Err(crate::i18n::text().stream_errors.host_nat.into());
        }
        tokio::select! {
            incoming = tokio::time::timeout(remaining, endpoint.accept()) => {
                match incoming {
                    Ok(Some(incoming)) => {
                        handshakes.spawn(async move {
                            let conn = tokio::time::timeout(HANDSHAKE_TIMEOUT, incoming).await.ok()?.ok()?;
                            let bi = tokio::time::timeout(HANDSHAKE_TIMEOUT, conn.accept_bi()).await.ok()?.ok()?;
                            Some((conn, bi))
                        });
                    }
                    Ok(None) => {
                        handshakes.abort_all();
                        return Err(crate::i18n::text().stream_errors.endpoint_closed.into());
                    }
                    Err(_) => {
                        handshakes.abort_all();
                        return Err(
                            crate::i18n::text().stream_errors.host_nat
                                .into(),
                        );
                    }
                }
            }
            Some(result) = handshakes.join_next(), if !handshakes.is_empty() => {
                if let Ok(Some(winner)) = result {
                    handshakes.abort_all();
                    return Ok(winner);
                }
            }
        }
    }
}

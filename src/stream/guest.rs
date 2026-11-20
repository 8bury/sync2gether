use super::{
    ChunkIndex, StreamEvent, StreamPhase, StreamTelemetry,
    cache::{ChunkCache, default_cache_dir},
    compute_bitrate_advice, file_bitrate_bps, http, nat, tls, wire,
};
use crate::protocol::{StreamAnswer, StreamOffer};
use std::{
    io,
    net::{SocketAddr, UdpSocket},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinHandle};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
const FETCH_WORKERS: usize = 6;
const PROBE_BYTES: u32 = 1024 * 1024;

pub async fn start(
    offer: StreamOffer,
    stun_enabled: bool,
    cache_dir: Option<PathBuf>,
    events: mpsc::Sender<StreamEvent>,
) -> io::Result<(StreamAnswer, JoinHandle<()>)> {
    let socket = nat::bind_udp()?;
    let identity = tls::Identity::generate()?;
    let candidates = nat::gather_candidates(&socket, stun_enabled).await;
    let answer = StreamAnswer {
        id: offer.id,
        cert_fingerprint: identity.fingerprint.clone(),
        candidates: candidates.into_iter().map(|a| a.to_string()).collect(),
    };
    let task = tokio::spawn(async move {
        let id = offer.id;
        if let Err(error) = run_guest(offer, socket, identity, cache_dir, events.clone()).await {
            let _ = events.send(StreamEvent::Failed { id, error }).await;
        }
    });
    Ok((answer, task))
}

async fn run_guest(
    offer: StreamOffer,
    socket: UdpSocket,
    identity: tls::Identity,
    cache_dir: Option<PathBuf>,
    events: mpsc::Sender<StreamEvent>,
) -> Result<(), String> {
    let id = offer.id;
    let local_addr = socket
        .local_addr()
        .map_err(|_| "Não foi possível obter porta UDP local.".to_owned())?;
    let remote_candidates: Vec<SocketAddr> = offer
        .candidates
        .iter()
        .filter_map(|c| c.parse::<SocketAddr>().ok())
        .filter(|a| crate::network::allowed_address(*a))
        .collect();
    if remote_candidates.is_empty() {
        return Err("Anfitrião não enviou endereços P2P válidos.".into());
    }

    let file_bitrate = file_bitrate_bps(offer.media.bytes, offer.duration);
    let _ = events
        .send(StreamEvent::Telemetry {
            id,
            telemetry: StreamTelemetry {
                active: true,
                is_sender: false,
                phase: StreamPhase::Connecting,
                file_bitrate_bps: file_bitrate,
                total_bytes: offer.media.bytes,
                ..Default::default()
            },
        })
        .await;

    let punch_sock = socket
        .try_clone()
        .map_err(|_| "Falha ao clonar socket UDP para hole-punching.".to_owned())?;
    nat::send_hole_punches(&punch_sock, &remote_candidates);

    let client_config = tls::client_config(&identity, &offer.cert_fingerprint)
        .map_err(|e| format!("Falha ao configurar cliente TLS P2P: {e}"))?;
    let mut endpoint = quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        None,
        socket,
        Arc::new(quinn::TokioRuntime),
    )
    .map_err(|e| format!("Falha ao abrir endpoint QUIC: {e}"))?;
    endpoint.set_default_client_config(client_config);

    let conn = connect_any(&endpoint, &punch_sock, local_addr, &remote_candidates).await?;
    let peer_addr = conn.remote_address();

    let _ = events
        .send(StreamEvent::Telemetry {
            id,
            telemetry: StreamTelemetry {
                active: true,
                is_sender: false,
                phase: StreamPhase::Probing,
                peer_addr: Some(peer_addr),
                file_bitrate_bps: file_bitrate,
                total_bytes: offer.media.bytes,
                ..Default::default()
            },
        })
        .await;

    let hashes = Arc::new(fetch_hash_table(&conn, &offer).await?);
    let link_bps = measure_throughput(&conn, offer.media.bytes).await.ok();

    let dir = cache_dir.unwrap_or_else(|| default_cache_dir(&offer.media.hash));
    let cache = Arc::new(
        ChunkCache::open(&dir, &offer.media.hash, offer.media.bytes, offer.chunk_size)
            .map_err(|e| format!("Não foi possível abrir o cache local: {e}"))?,
    );
    let proxy = http::start(cache.clone(), &offer.file_name)
        .await
        .map_err(|e| format!("Não foi possível iniciar o proxy local do player: {e}"))?;

    let stats = cache.stats();
    let initial_telemetry = StreamTelemetry {
        active: true,
        is_sender: false,
        phase: StreamPhase::Streaming,
        peer_addr: Some(peer_addr),
        link_bps,
        file_bitrate_bps: file_bitrate,
        transfer_bps: link_bps.unwrap_or(0),
        cached_bytes: stats.cached_bytes,
        total_bytes: stats.total_bytes,
        buffered_ahead_bytes: stats.buffered_ahead_bytes,
        chunks_cached: stats.chunks_cached,
        chunks_total: stats.chunks_total,
        advice: compute_bitrate_advice(
            link_bps,
            file_bitrate,
            stats.total_bytes.saturating_sub(stats.cached_bytes),
        ),
    };

    if events
        .send(StreamEvent::Ready {
            id,
            url: proxy.url.clone(),
            media: offer.media.clone(),
            file_name: offer.file_name.clone(),
            telemetry: initial_telemetry,
        })
        .await
        .is_err()
    {
        return Ok(());
    }

    let bytes_received = Arc::new(AtomicU64::new(0));
    let mut workers = Vec::with_capacity(FETCH_WORKERS);
    for _ in 0..FETCH_WORKERS {
        let conn = conn.clone();
        let cache = cache.clone();
        let hashes = hashes.clone();
        let bytes_received = bytes_received.clone();
        workers.push(tokio::spawn(async move {
            fetch_worker(conn, cache, hashes, bytes_received).await;
        }));
    }

    let mut interval = tokio::time::interval(Duration::from_millis(400));
    let mut prev_bytes = 0u64;
    let mut prev_at = Instant::now();
    let mut ewma_bps = link_bps.unwrap_or(0) as f64;

    loop {
        tokio::select! {
            _ = conn.closed() => {
                if !cache.is_complete() {
                    cache.close_with_error("Conexão P2P encerrada".into());
                    for w in workers {
                        w.abort();
                    }
                    drop(proxy);
                    return Err("A conexão P2P direta foi interrompida.".into());
                }
                break;
            }
            _ = interval.tick() => {
                let now = Instant::now();
                let cur = bytes_received.load(Ordering::Relaxed);
                let dt = now.duration_since(prev_at).as_secs_f64().max(0.001);
                let instant_bps = ((cur.saturating_sub(prev_bytes) as f64) * 8.0) / dt;
                if cur > prev_bytes {
                    ewma_bps = if ewma_bps <= 1.0 {
                        instant_bps
                    } else {
                        ewma_bps * 0.65 + instant_bps * 0.35
                    };
                }
                prev_bytes = cur;
                prev_at = now;
                let stats = cache.stats();
                let effective_link = if ewma_bps > 1.0 {
                    Some(ewma_bps as u64)
                } else {
                    link_bps
                };
                let telemetry = StreamTelemetry {
                    active: true,
                    is_sender: false,
                    phase: StreamPhase::Streaming,
                    peer_addr: Some(peer_addr),
                    link_bps: effective_link,
                    file_bitrate_bps: file_bitrate,
                    transfer_bps: instant_bps as u64,
                    cached_bytes: stats.cached_bytes,
                    total_bytes: stats.total_bytes,
                    buffered_ahead_bytes: stats.buffered_ahead_bytes,
                    chunks_cached: stats.chunks_cached,
                    chunks_total: stats.chunks_total,
                    advice: compute_bitrate_advice(
                        effective_link,
                        file_bitrate,
                        stats.total_bytes.saturating_sub(stats.cached_bytes),
                    ),
                };
                if events.send(StreamEvent::Telemetry { id, telemetry }).await.is_err() {
                    break;
                }
            }
        }
    }

    for w in workers {
        w.abort();
    }
    drop(proxy);
    endpoint.close(0u32.into(), b"done");
    Ok(())
}

async fn connect_any(
    endpoint: &quinn::Endpoint,
    punch_sock: &UdpSocket,
    local_addr: SocketAddr,
    candidates: &[SocketAddr],
) -> Result<quinn::Connection, String> {
    let targets: Vec<SocketAddr> = candidates
        .iter()
        .filter_map(|&c| nat::normalize_for_socket(local_addr, c))
        .collect();
    if targets.is_empty() {
        return Err("Nenhum endereço P2P compatível com a pilha de rede local.".into());
    }

    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    let mut attempt = 0u32;
    while tokio::time::Instant::now() < deadline {
        nat::send_hole_punches(punch_sock, candidates);
        let (tx, mut rx) = mpsc::channel(targets.len());
        let mut handles = Vec::with_capacity(targets.len());
        for &target in &targets {
            let endpoint = endpoint.clone();
            let tx = tx.clone();
            handles.push(tokio::spawn(async move {
                if let Ok(connecting) = endpoint.connect(target, "sync2gether.peer")
                    && let Ok(conn) = connecting.await
                {
                    let _ = tx.send(conn).await;
                }
            }));
        }
        drop(tx);
        let wait = Duration::from_millis(if attempt == 0 { 2500 } else { 2000 });
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if let Ok(Some(conn)) = tokio::time::timeout(wait.min(remaining), rx.recv()).await {
            for h in handles {
                h.abort();
            }
            return Ok(conn);
        }
        for h in handles {
            h.abort();
        }
        attempt += 1;
    }
    Err(
        "Não foi possível conectar P2P direto ao anfitrião (NAT restrito ou firewall). Tente na rede local, IPv6 ou VPN."
            .into(),
    )
}

async fn fetch_hash_table(
    conn: &quinn::Connection,
    offer: &StreamOffer,
) -> Result<Vec<[u8; 32]>, String> {
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|_| "Falha ao abrir canal QUIC para tabela de blocos.".to_owned())?;
    wire::write_request(&mut send, wire::Request::HashTable)
        .await
        .map_err(|_| "Falha ao solicitar tabela de blocos.".to_owned())?;
    let _ = send.finish();
    let raw = wire::read_ok_payload(&mut recv, wire::MAX_HASH_TABLE_BYTES)
        .await
        .map_err(|e| format!("Falha ao receber tabela de blocos: {e}"))?;
    ChunkIndex::deserialize_verified(
        &raw,
        &offer.hash_table_digest,
        offer.media.bytes,
        offer.chunk_size,
    )
    .map_err(|e| format!("Tabela de blocos inválida: {e}"))
}

async fn measure_throughput(conn: &quinn::Connection, file_bytes: u64) -> io::Result<u64> {
    let probe_len = (file_bytes.min(u64::from(PROBE_BYTES)) as u32).max(64 * 1024);
    let started = Instant::now();
    let (mut send, mut recv) = conn.open_bi().await.map_err(io::Error::other)?;
    wire::write_request(&mut send, wire::Request::Probe(probe_len)).await?;
    let _ = send.finish();
    let data = wire::read_ok_payload(&mut recv, wire::MAX_PROBE_BYTES as usize).await?;
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    Ok(((data.len() as f64 * 8.0) / elapsed) as u64)
}

async fn fetch_worker(
    conn: quinn::Connection,
    cache: Arc<ChunkCache>,
    hashes: Arc<Vec<[u8; 32]>>,
    bytes_received: Arc<AtomicU64>,
) {
    while let Some(idx) = cache.next_chunk_to_fetch().await {
        let Some(expected_hash) = hashes.get(idx as usize) else {
            cache.release_in_flight(idx);
            break;
        };
        let result = fetch_one_chunk(&conn, idx, cache.chunk_size() as usize).await;
        match result {
            Ok(data) => {
                let len = data.len() as u64;
                if cache.complete_chunk(idx, &data, expected_hash).is_ok() {
                    bytes_received.fetch_add(len, Ordering::Relaxed);
                }
            }
            Err(_) => {
                cache.release_in_flight(idx);
                if conn.close_reason().is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn fetch_one_chunk(
    conn: &quinn::Connection,
    idx: u32,
    max_chunk_size: usize,
) -> io::Result<Vec<u8>> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(io::Error::other)?;
    wire::write_request(&mut send, wire::Request::Chunk(idx)).await?;
    let _ = send.finish();
    wire::read_ok_payload(&mut recv, max_chunk_size).await
}

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
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
const CHUNK_TIMEOUT: Duration = Duration::from_secs(8);
const FETCH_WORKERS: usize = 6;
const PROBE_BYTES: u32 = 1024 * 1024;

struct EndpointGuard(quinn::Endpoint);
impl Drop for EndpointGuard {
    fn drop(&mut self) {
        self.0.close(0u32.into(), b"stop");
    }
}

struct CacheGuard {
    cache: Arc<ChunkCache>,
    proxy: Option<Weak<http::LocalProxy>>,
}
impl Drop for CacheGuard {
    fn drop(&mut self) {
        if let Some(proxy) = &self.proxy
            && proxy.strong_count() == 0
        {
            self.cache.close_with_error("Streaming encerrado".into());
        }
    }
}

pub struct GuestPending {
    pub id: u64,
    offer: StreamOffer,
    socket: UdpSocket,
    identity: tls::Identity,
    stun_enabled: bool,
    cache_dir: Option<PathBuf>,
    existing_cache: Option<Arc<ChunkCache>>,
}

pub async fn prepare(
    offer: StreamOffer,
    stun_enabled: bool,
    cache_dir: Option<PathBuf>,
) -> io::Result<(StreamAnswer, GuestPending)> {
    prepare_with_cache(offer, stun_enabled, cache_dir, None).await
}

pub async fn prepare_with_cache(
    offer: StreamOffer,
    stun_enabled: bool,
    cache_dir: Option<PathBuf>,
    existing_cache: Option<Arc<ChunkCache>>,
) -> io::Result<(StreamAnswer, GuestPending)> {
    let socket = nat::bind_udp()?;
    let identity = tls::Identity::generate()?;
    let candidates = nat::gather_candidates(&socket, stun_enabled).await;
    let answer = StreamAnswer {
        id: offer.id,
        cert_fingerprint: identity.fingerprint.clone(),
        candidates: candidates.into_iter().map(|a| a.to_string()).collect(),
    };
    let pending = GuestPending {
        id: offer.id,
        offer,
        socket,
        identity,
        stun_enabled,
        cache_dir,
        existing_cache,
    };
    Ok((answer, pending))
}

enum GuestOutcome {
    Ended,
    Disconnected(String),
}

impl GuestPending {
    pub fn connect(self, events: mpsc::Sender<StreamEvent>) -> JoinHandle<()> {
        let id = self.id;
        tokio::spawn(async move {
            match run_guest(self, events.clone()).await {
                Ok(GuestOutcome::Ended) => {
                    let _ = events.send(StreamEvent::Ended { id }).await;
                }
                Ok(GuestOutcome::Disconnected(reason)) => {
                    let _ = events.send(StreamEvent::Disconnected { id, reason }).await;
                }
                Err(error) => {
                    let _ = events.send(StreamEvent::Failed { id, error }).await;
                }
            }
        })
    }
}

async fn run_guest(
    pending: GuestPending,
    events: mpsc::Sender<StreamEvent>,
) -> Result<GuestOutcome, String> {
    let GuestPending {
        id,
        offer,
        socket,
        identity,
        stun_enabled,
        cache_dir,
        existing_cache,
    } = pending;

    let reconnecting = existing_cache.is_some();
    let local_addr = socket
        .local_addr()
        .map_err(|_| "Não foi possível obter porta UDP local.".to_owned())?;
    let remote_candidates = nat::filter_remote_candidates(&offer.candidates, !stun_enabled);
    if remote_candidates.is_empty() {
        return Err("Anfitrião não enviou endereços P2P válidos.".into());
    }

    let file_bitrate = file_bitrate_bps(offer.media.bytes, offer.duration);
    let initial_stats = existing_cache.as_ref().map(|c| c.stats());
    let _ = events
        .send(StreamEvent::Telemetry {
            id,
            telemetry: StreamTelemetry {
                active: true,
                is_sender: false,
                phase: if reconnecting {
                    StreamPhase::Reconnecting
                } else {
                    StreamPhase::Connecting
                },
                file_bitrate_bps: file_bitrate,
                cached_bytes: initial_stats.as_ref().map_or(0, |s| s.cached_bytes),
                total_bytes: offer.media.bytes,
                buffered_ahead_bytes: initial_stats.as_ref().map_or(0, |s| s.buffered_ahead_bytes),
                chunks_cached: initial_stats.as_ref().map_or(0, |s| s.chunks_cached),
                chunks_total: initial_stats.as_ref().map_or(0, |s| s.chunks_total),
                cached_ranges: initial_stats
                    .as_ref()
                    .map(|s| s.cached_ranges.clone())
                    .unwrap_or_default(),
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
    let endpoint_guard = EndpointGuard(endpoint.clone());

    tokio::time::sleep(Duration::from_millis(80)).await;
    let conn = connect_any(&endpoint, &punch_sock, local_addr, &remote_candidates).await?;
    let peer_addr = conn.remote_address();

    let probe_stats = existing_cache.as_ref().map(|c| c.stats());
    let _ = events
        .send(StreamEvent::Telemetry {
            id,
            telemetry: StreamTelemetry {
                active: true,
                is_sender: false,
                phase: if reconnecting {
                    StreamPhase::Reconnecting
                } else {
                    StreamPhase::Probing
                },
                peer_addr: Some(peer_addr),
                file_bitrate_bps: file_bitrate,
                cached_bytes: probe_stats.as_ref().map_or(0, |s| s.cached_bytes),
                total_bytes: offer.media.bytes,
                buffered_ahead_bytes: probe_stats.as_ref().map_or(0, |s| s.buffered_ahead_bytes),
                chunks_cached: probe_stats.as_ref().map_or(0, |s| s.chunks_cached),
                chunks_total: probe_stats.as_ref().map_or(0, |s| s.chunks_total),
                cached_ranges: probe_stats
                    .as_ref()
                    .map(|s| s.cached_ranges.clone())
                    .unwrap_or_default(),
                ..Default::default()
            },
        })
        .await;

    let hashes = if existing_cache.is_some() {
        None
    } else {
        Some(Arc::new(fetch_hash_table(&conn, &offer).await?))
    };
    let link_bps = measure_throughput(&conn, offer.media.bytes).await.ok();

    let (cache, proxy) = if let Some(existing) = existing_cache {
        existing.release_in_flight();
        (existing, None)
    } else {
        let dir = cache_dir.unwrap_or_else(|| default_cache_dir(&offer.media.hash));
        let cache = ChunkCache::open_async(
            dir,
            offer.media.hash.clone(),
            offer.hash_table_digest.clone(),
            offer.media.bytes,
            offer.chunk_size,
            hashes.unwrap(),
        )
        .await
        .map_err(|e| format!("Não foi possível abrir o cache local: {e}"))?;
        let proxy = Arc::new(
            http::start(cache.clone(), &offer.file_name)
                .await
                .map_err(|e| format!("Não foi possível iniciar o proxy local do player: {e}"))?,
        );
        (cache, Some(proxy))
    };
    let _cache_guard = CacheGuard {
        cache: cache.clone(),
        proxy: proxy.as_ref().map(Arc::downgrade),
    };
    let _keep_proxy = proxy;

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
        cached_ranges: stats.cached_ranges,
    };

    if let Some(proxy) = &_keep_proxy {
        if events
            .send(StreamEvent::Ready {
                id,
                url: proxy.url.clone(),
                media: offer.media.clone(),
                file_name: offer.file_name.clone(),
                hash_table_digest: offer.hash_table_digest.clone(),
                cache: cache.clone(),
                proxy: proxy.clone(),
                telemetry: initial_telemetry,
            })
            .await
            .is_err()
        {
            return Ok(GuestOutcome::Ended);
        }
    } else if events
        .send(StreamEvent::Telemetry {
            id,
            telemetry: initial_telemetry,
        })
        .await
        .is_err()
    {
        return Ok(GuestOutcome::Ended);
    }

    let bytes_received = Arc::new(AtomicU64::new(0));
    let mut workers = JoinSet::new();
    for _ in 0..FETCH_WORKERS {
        let conn = conn.clone();
        let cache = cache.clone();
        let bytes_received = bytes_received.clone();
        workers.spawn(async move {
            fetch_worker(conn, cache, bytes_received).await;
        });
    }

    let mut interval = tokio::time::interval(Duration::from_millis(400));
    let mut prev_bytes = 0u64;
    let mut prev_at = Instant::now();
    let mut ewma_bps = link_bps.unwrap_or(0) as f64;
    let mut full_verified = false;

    loop {
        tokio::select! {
            _ = conn.closed() => {
                if !cache.is_complete() {
                    workers.abort_all();
                    cache.release_in_flight();
                    if let Some(reason) = cache.closed_reason() {
                        return Err(reason);
                    }
                    return Ok(GuestOutcome::Disconnected(
                        "A conexão P2P direta foi interrompida.".into(),
                    ));
                }
                break;
            }
            _ = interval.tick() => {
                if let Some(reason) = cache.closed_reason() {
                    workers.abort_all();
                    return Err(reason);
                }
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
                if cache.is_complete() && !full_verified {
                    verify_complete_or_wipe(&cache).await?;
                    full_verified = true;
                }
                let effective_link = if ewma_bps > 1.0 {
                    Some(ewma_bps as u64)
                } else {
                    link_bps
                };
                let phase = if full_verified {
                    StreamPhase::Complete
                } else {
                    StreamPhase::Streaming
                };
                let telemetry = StreamTelemetry {
                    active: true,
                    is_sender: false,
                    phase,
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
                    cached_ranges: stats.cached_ranges,
                };
                if events.send(StreamEvent::Telemetry { id, telemetry }).await.is_err() {
                    workers.abort_all();
                    return Ok(GuestOutcome::Ended);
                }
                if full_verified {
                    break;
                }
            }
        }
    }

    workers.abort_all();
    if !full_verified {
        verify_complete_or_wipe(&cache).await?;
    }
    drop(endpoint_guard);

    let stats = cache.stats();
    let _ = events
        .send(StreamEvent::Telemetry {
            id,
            telemetry: StreamTelemetry {
                active: true,
                is_sender: false,
                phase: StreamPhase::Complete,
                peer_addr: Some(peer_addr),
                link_bps,
                file_bitrate_bps: file_bitrate,
                transfer_bps: 0,
                cached_bytes: stats.cached_bytes,
                total_bytes: stats.total_bytes,
                buffered_ahead_bytes: stats.buffered_ahead_bytes,
                chunks_cached: stats.chunks_cached,
                chunks_total: stats.chunks_total,
                advice: super::BitrateAdvice::Ready,
                cached_ranges: stats.cached_ranges,
            },
        })
        .await;

    std::future::pending::<()>().await;
    Ok(GuestOutcome::Ended)
}

async fn verify_complete_or_wipe(cache: &ChunkCache) -> Result<(), String> {
    match cache.verify_full_file_hash().await {
        Ok(true) => Ok(()),
        Ok(false) => {
            cache.close_with_error("Hash do arquivo não confere.".into());
            cache.wipe_cache().await;
            Err("O conteúdo recebido não confere com o hash informado pelo anfitrião.".into())
        }
        Err(err) => {
            cache.close_with_error(format!("Erro ao verificar cache: {err}"));
            Err(format!("Falha ao verificar integridade do filme: {err}"))
        }
    }
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
        let mut handles = JoinSet::new();
        for &target in &targets {
            let endpoint = endpoint.clone();
            let tx = tx.clone();
            handles.spawn(async move {
                if let Ok(connecting) = endpoint.connect(target, "sync2gether.peer")
                    && let Ok(conn) = connecting.await
                {
                    let _ = tx.send(conn).await;
                }
            });
        }
        drop(tx);
        let wait = Duration::from_millis(if attempt == 0 { 2500 } else { 2000 });
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if let Ok(Some(conn)) = tokio::time::timeout(wait.min(remaining), rx.recv()).await {
            handles.abort_all();
            return Ok(conn);
        }
        handles.abort_all();
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
    let raw = tokio::time::timeout(
        Duration::from_secs(10),
        wire::read_ok_payload(&mut recv, wire::MAX_HASH_TABLE_BYTES),
    )
    .await
    .map_err(|_| "Tempo esgotado ao baixar tabela de blocos.".to_owned())?
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
    let data = tokio::time::timeout(
        Duration::from_secs(8),
        wire::read_ok_payload(&mut recv, wire::MAX_PROBE_BYTES as usize),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Sonda de velocidade expirou"))??;
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    Ok(((data.len() as f64 * 8.0) / elapsed) as u64)
}

async fn fetch_worker(
    conn: quinn::Connection,
    cache: Arc<ChunkCache>,
    bytes_received: Arc<AtomicU64>,
) {
    while let Some(idx) = cache.next_chunk_to_fetch().await {
        let result = fetch_one_chunk(&conn, idx, cache.chunk_size() as usize).await;
        match result {
            Ok(data) => {
                let len = data.len() as u64;
                match cache.complete_chunk(idx, data).await {
                    Ok(()) => {
                        bytes_received.fetch_add(len, Ordering::Relaxed);
                    }
                    Err(err) => match cache.record_chunk_failure(idx, &err.to_string()) {
                        Ok(attempt) => {
                            let delay = Duration::from_millis(150 * (1u64 << (attempt - 1)));
                            tokio::time::sleep(delay).await;
                        }
                        Err(_) => break,
                    },
                }
            }
            Err(err) => {
                if conn.close_reason().is_some() {
                    cache.release_chunk(idx);
                    break;
                }
                match cache.record_chunk_failure(idx, &err.to_string()) {
                    Ok(attempt) => {
                        let delay = Duration::from_millis(150 * (1u64 << (attempt - 1)));
                        tokio::time::sleep(delay).await;
                    }
                    Err(_) => break,
                }
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
    let fut = async {
        wire::write_request(&mut send, wire::Request::Chunk(idx)).await?;
        let _ = send.finish();
        wire::read_ok_payload(&mut recv, max_chunk_size).await
    };
    match tokio::time::timeout(CHUNK_TIMEOUT, fut).await {
        Ok(result) => result,
        Err(_) => {
            let _ = send.reset(1u32.into());
            let _ = recv.stop(1u32.into());
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Tempo esgotado ao ler bloco",
            ))
        }
    }
}

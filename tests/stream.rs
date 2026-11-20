use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use sync2gether::stream::{CHUNK_SIZE, ChunkIndex, StreamEvent, StreamPhase, guest, host};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};

fn write_fixture(name: &str, size: usize) -> (PathBuf, Vec<u8>) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/stream-e2e-test");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let data: Vec<u8> = (0..size)
        .map(|i| ((i.wrapping_mul(31) ^ (i >> 7)) & 0xff) as u8)
        .collect();
    fs::write(&path, &data).unwrap();
    (path, data)
}

async fn http_range(url: &str, start: u64, end: u64) -> Vec<u8> {
    let stripped = url.strip_prefix("http://").unwrap();
    let (addr, path) = stripped.split_once('/').unwrap();
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let req = format!("GET /{path} HTTP/1.1\r\nHost: {addr}\r\nRange: bytes={start}-{end}\r\n\r\n");
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut resp = Vec::new();
    stream.read_to_end(&mut resp).await.unwrap();
    let split = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let headers = std::str::from_utf8(&resp[..split]).unwrap();
    assert!(
        headers.starts_with("HTTP/1.1 206 Partial Content"),
        "unexpected status: {headers}"
    );
    resp[split + 4..].to_vec()
}

#[tokio::test]
async fn quic_p2p_streams_random_ranges_probes_bandwidth_and_persists_cache() {
    let size = (CHUNK_SIZE as usize) * 6 + 13_371;
    let (path, original) = write_fixture("movie.mkv", size);
    let (media, index) = ChunkIndex::compute_file(&path, CHUNK_SIZE).unwrap();
    let index = Arc::new(index);

    let (host_tx, mut host_rx) = mpsc::channel(32);
    let (guest_tx, mut guest_rx) = mpsc::channel(32);

    let (offer, pending) = host::prepare(
        1,
        path.clone(),
        media.clone(),
        "movie.mkv".into(),
        120.0,
        index,
        false,
    )
    .await
    .unwrap();
    assert!(offer.valid());

    let cache_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/stream-e2e-cache");
    let _ = fs::remove_dir_all(&cache_dir);

    let (answer, guest_task) = guest::start(offer, false, Some(cache_dir.clone()), guest_tx)
        .await
        .unwrap();
    assert!(answer.valid());
    let host_task = pending.accept(answer, host_tx);

    let ready_url = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match guest_rx.recv().await.unwrap() {
                StreamEvent::Ready {
                    url,
                    media: ready_media,
                    telemetry,
                    ..
                } => {
                    assert_eq!(ready_media, media);
                    assert_eq!(telemetry.phase, StreamPhase::Streaming);
                    assert!(telemetry.link_bps.unwrap_or(0) > 0);
                    return url;
                }
                StreamEvent::Failed { error, .. } => panic!("guest stream failed: {error}"),
                StreamEvent::Telemetry { .. } => {}
            }
        }
    })
    .await
    .unwrap();

    let head = http_range(&ready_url, 0, 4095).await;
    assert_eq!(head, original[0..4096]);

    let tail_start = (size - 8192) as u64;
    let tail = http_range(&ready_url, tail_start, (size - 1) as u64).await;
    assert_eq!(tail, original[tail_start as usize..]);

    let mid_start = (CHUNK_SIZE as usize * 3 - 500) as u64;
    let mid_end = mid_start + 50_000;
    let mid = http_range(&ready_url, mid_start, mid_end).await;
    assert_eq!(mid, original[mid_start as usize..=mid_end as usize]);

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(StreamEvent::Telemetry { telemetry, .. }) = guest_rx.recv().await
                && telemetry.cached_bytes == size as u64
            {
                break;
            }
        }
    })
    .await
    .unwrap();

    assert!(
        tokio::time::timeout(Duration::from_secs(2), host_rx.recv())
            .await
            .is_ok()
    );

    guest_task.abort();
    host_task.abort();

    let resumed = sync2gether::stream::cache::ChunkCache::open(
        &cache_dir,
        &media.hash,
        size as u64,
        CHUNK_SIZE,
    )
    .unwrap();
    assert!(resumed.is_complete());
    assert_eq!(resumed.stats().cached_bytes, size as u64);

    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_file(&path);
}

#[tokio::test]
async fn mismatched_tls_fingerprint_rejects_p2p_connection() {
    let (path, _) = write_fixture("reject.mkv", 64 * 1024);
    let (media, index) = ChunkIndex::compute_file(&path, CHUNK_SIZE).unwrap();
    let (host_tx, _host_rx) = mpsc::channel(16);
    let (guest_tx, mut guest_rx) = mpsc::channel(16);

    let (mut offer, pending) = host::prepare(
        2,
        path.clone(),
        media,
        "reject.mkv".into(),
        10.0,
        Arc::new(index),
        false,
    )
    .await
    .unwrap();

    offer.cert_fingerprint = "0".repeat(64);
    let (answer, guest_task) = guest::start(offer, false, None, guest_tx).await.unwrap();
    let host_task = pending.accept(answer, host_tx);

    let failed = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match guest_rx.recv().await {
                Some(StreamEvent::Failed { .. }) => return true,
                Some(StreamEvent::Ready { .. }) => {
                    panic!("tampered fingerprint must never connect")
                }
                Some(StreamEvent::Telemetry { .. }) => {}
                None => return false,
            }
        }
    })
    .await
    .unwrap();
    assert!(failed);

    guest_task.abort();
    host_task.abort();
    let _ = fs::remove_file(&path);
}

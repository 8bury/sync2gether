use super::cache::ChunkCache;
use std::{io, net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};

const MAX_HEADER_BYTES: usize = 8 * 1024;
const READ_BUF_BYTES: usize = 64 * 1024;

pub struct LocalProxy {
    pub url: String,
    pub addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for LocalProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn start(cache: Arc<ChunkCache>, file_name: &str) -> io::Result<LocalProxy> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let token = format!("{:032x}", rand::random::<u128>());
    let safe_name = sanitize_url_name(file_name);
    let expected_path = format!("/stream/{token}/{safe_name}");
    let url = format!("http://{addr}{expected_path}");

    let task = tokio::spawn(async move {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else {
                        break;
                    };
                    let _ = stream.set_nodelay(true);
                    let cache = cache.clone();
                    let path = expected_path.clone();
                    connections.spawn(async move {
                        let _ = serve_connection(stream, cache, &path).await;
                    });
                }
                Some(_) = connections.join_next(), if !connections.is_empty() => {}
            }
        }
    });

    Ok(LocalProxy { url, addr, task })
}

fn sanitize_url_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(128)
        .collect();
    if clean.is_empty() {
        "media.bin".into()
    } else {
        clean
    }
}

async fn wait_client_disconnect(stream: &TcpStream) {
    let mut probe = [0u8; 1];
    loop {
        if stream.readable().await.is_err() {
            return;
        }
        match stream.try_read(&mut probe) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(_) => return,
        }
    }
}

async fn serve_connection(
    mut stream: TcpStream,
    cache: Arc<ChunkCache>,
    expected_path: &str,
) -> io::Result<()> {
    let header_bytes = read_headers(&mut stream).await?;
    let header_str = std::str::from_utf8(&header_bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            crate::i18n::text().stream_errors.invalid_http_header,
        )
    })?;
    let mut lines = header_str.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    let path_only = path.split('?').next().unwrap_or(path);

    if path_only != expected_path {
        write_status(&mut stream, "404 Not Found", &[]).await?;
        return Ok(());
    }
    if method != "GET" && method != "HEAD" {
        write_status(&mut stream, "405 Method Not Allowed", &[]).await?;
        return Ok(());
    }

    let mut range_header = None;
    for line in lines {
        if let Some((key, val)) = line.split_once(':')
            && key.trim().eq_ignore_ascii_case("range")
        {
            range_header = Some(val.trim().to_owned());
        }
    }

    let total = cache.file_size();
    let (status, start, end) = match range_header.filter(|r| r.starts_with("bytes=")) {
        Some(raw) => match parse_range(&raw, total) {
            Some((s, e)) => ("206 Partial Content", s, e),
            None => {
                let cr = format!("bytes */{total}");
                write_status(
                    &mut stream,
                    "416 Range Not Satisfiable",
                    &[("Content-Range", cr.as_str())],
                )
                .await?;
                return Ok(());
            }
        },
        None => {
            if total == 0 {
                write_status(
                    &mut stream,
                    "200 OK",
                    &[
                        ("Content-Length", "0"),
                        ("Accept-Ranges", "bytes"),
                        ("Content-Type", "application/octet-stream"),
                    ],
                )
                .await?;
                return Ok(());
            }
            ("200 OK", 0, total - 1)
        }
    };

    let length = end - start + 1;
    let len_str = length.to_string();
    let cr_str = format!("bytes {start}-{end}/{total}");
    let mut headers = vec![
        ("Content-Length", len_str.as_str()),
        ("Accept-Ranges", "bytes"),
        ("Content-Type", "application/octet-stream"),
    ];
    if status.starts_with("206") {
        headers.push(("Content-Range", cr_str.as_str()));
    }
    write_status(&mut stream, status, &headers).await?;

    if method == "HEAD" {
        return Ok(());
    }

    let mut pos = start;
    while pos <= end {
        let remaining = (end - pos + 1).min(READ_BUF_BYTES as u64) as usize;
        let data = tokio::select! {
            biased;
            _ = wait_client_disconnect(&stream) => return Ok(()),
            res = cache.read_at_blocking(pos, remaining) => res?,
        };
        if data.is_empty() {
            break;
        }
        stream.write_all(&data).await?;
        pos += data.len() as u64;
    }
    let _ = stream.flush().await;
    Ok(())
}

async fn read_headers(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(512);
    let mut tmp = [0u8; 512];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            return Ok(buf);
        }
        if buf.len() >= MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                crate::i18n::text().stream_errors.http_header_too_large,
            ));
        }
    }
}

async fn write_status(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
) -> io::Result<()> {
    let mut response = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for (k, v) in headers {
        response.push_str(k);
        response.push_str(": ");
        response.push_str(v);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes()).await
}

pub fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let spec = header.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (start_s, end_s) = spec.split_once('-')?;
    if start_s.is_empty() {
        let suffix: u64 = end_s.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        let start = total.saturating_sub(suffix);
        return Some((start, total - 1));
    }
    let start: u64 = start_s.parse().ok()?;
    if start >= total {
        return None;
    }
    let end = if end_s.is_empty() {
        total - 1
    } else {
        end_s.parse::<u64>().ok()?.min(total - 1)
    };
    if start > end {
        return None;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::ChunkIndex;
    use std::path::PathBuf;

    #[test]
    fn parses_standard_http_byte_ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=950-2000", 1000), Some((950, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=500-400", 1000), None);
    }

    #[tokio::test]
    async fn proxy_serves_range_requests_and_rejects_wrong_token() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/stream-http-test");
        let _ = std::fs::remove_dir_all(&dir);
        let payload: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
        let hash = blake3::hash(&payload).to_hex().to_string();
        let hashes: Vec<[u8; 32]> = payload
            .chunks(1024)
            .map(|c| *blake3::hash(c).as_bytes())
            .collect();
        let index = ChunkIndex::from_hashes(1024, payload.len() as u64, hashes);
        let cache = Arc::new(
            ChunkCache::open(
                &dir,
                &hash,
                &index.digest,
                payload.len() as u64,
                1024,
                Arc::new(index.hashes),
            )
            .unwrap(),
        );
        for idx in 0..4u32 {
            let slice = payload[(idx as usize * 1024)..((idx as usize + 1) * 1024)].to_vec();
            cache.complete_chunk(idx, slice).await.unwrap();
        }
        let proxy = start(cache, "Filme Teste.mkv").await.unwrap();
        let path = proxy
            .url
            .strip_prefix(&format!("http://{}", proxy.addr))
            .unwrap();

        let mut client = TcpStream::connect(proxy.addr).await.unwrap();
        client
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nRange: bytes=100-199\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.unwrap();
        let split = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let headers = std::str::from_utf8(&resp[..split]).unwrap();
        assert!(headers.starts_with("HTTP/1.1 206 Partial Content"));
        assert!(headers.contains("Content-Range: bytes 100-199/4096"));
        assert_eq!(&resp[split + 4..], &payload[100..200]);

        let mut bad = TcpStream::connect(proxy.addr).await.unwrap();
        bad.write_all(b"GET /stream/wrong/media.mkv HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut bad_resp = Vec::new();
        bad.read_to_end(&mut bad_resp).await.unwrap();
        assert!(
            std::str::from_utf8(&bad_resp)
                .unwrap()
                .starts_with("HTTP/1.1 404")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

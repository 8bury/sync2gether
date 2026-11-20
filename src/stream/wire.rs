use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const OP_HASH_TABLE: u8 = 1;
pub const OP_CHUNK: u8 = 2;
pub const OP_PROBE: u8 = 3;

pub const STATUS_OK: u8 = 0;
pub const STATUS_ERR: u8 = 1;

pub const MAX_PROBE_BYTES: u32 = 1024 * 1024;
pub const MAX_HASH_TABLE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    HashTable,
    Chunk(u32),
    Probe(u32),
}

pub async fn write_request<W: AsyncWrite + Unpin>(writer: &mut W, req: Request) -> io::Result<()> {
    match req {
        Request::HashTable => writer.write_u8(OP_HASH_TABLE).await?,
        Request::Chunk(index) => {
            writer.write_u8(OP_CHUNK).await?;
            writer.write_u32(index).await?;
        }
        Request::Probe(bytes) => {
            writer.write_u8(OP_PROBE).await?;
            writer.write_u32(bytes.min(MAX_PROBE_BYTES)).await?;
        }
    }
    writer.flush().await
}

pub async fn read_request<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Request> {
    match reader.read_u8().await? {
        OP_HASH_TABLE => Ok(Request::HashTable),
        OP_CHUNK => Ok(Request::Chunk(reader.read_u32().await?)),
        OP_PROBE => {
            let bytes = reader.read_u32().await?;
            if bytes == 0 || bytes > MAX_PROBE_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Tamanho de teste inválido",
                ));
            }
            Ok(Request::Probe(bytes))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Operação de streaming desconhecida",
        )),
    }
}

pub async fn write_ok_payload<W: AsyncWrite + Unpin>(
    writer: &mut W,
    data: &[u8],
) -> io::Result<()> {
    writer.write_u8(STATUS_OK).await?;
    writer.write_u32(data.len() as u32).await?;
    writer.write_all(data).await?;
    writer.flush().await
}

pub async fn write_ok_probe<W: AsyncWrite + Unpin>(
    writer: &mut W,
    total_bytes: u32,
) -> io::Result<()> {
    writer.write_u8(STATUS_OK).await?;
    writer.write_u32(total_bytes).await?;
    let block = [0xA5u8; 64 * 1024];
    let mut remaining = total_bytes as usize;
    while remaining > 0 {
        let n = remaining.min(block.len());
        writer.write_all(&block[..n]).await?;
        remaining -= n;
    }
    writer.flush().await
}

pub async fn write_error<W: AsyncWrite + Unpin>(writer: &mut W) -> io::Result<()> {
    writer.write_u8(STATUS_ERR).await?;
    writer.flush().await
}

pub async fn read_ok_payload<R: AsyncRead + Unpin>(
    reader: &mut R,
    max_len: usize,
) -> io::Result<Vec<u8>> {
    let status = reader.read_u8().await?;
    if status != STATUS_OK {
        return Err(io::Error::other("Peer recusou a solicitação do bloco"));
    }
    let len = reader.read_u32().await? as usize;
    if len > max_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Resposta excede o limite esperado",
        ));
    }
    let mut data = vec![0u8; len];
    reader.read_exact(&mut data).await?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wire_requests_and_payloads_roundtrip() {
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        write_request(&mut a, Request::HashTable).await.unwrap();
        assert_eq!(read_request(&mut b).await.unwrap(), Request::HashTable);

        write_request(&mut a, Request::Chunk(42)).await.unwrap();
        assert_eq!(read_request(&mut b).await.unwrap(), Request::Chunk(42));

        write_request(&mut a, Request::Probe(1024)).await.unwrap();
        assert_eq!(read_request(&mut b).await.unwrap(), Request::Probe(1024));

        write_ok_payload(&mut a, b"hello chunk").await.unwrap();
        assert_eq!(read_ok_payload(&mut b, 1024).await.unwrap(), b"hello chunk");

        write_error(&mut a).await.unwrap();
        assert!(read_ok_payload(&mut b, 1024).await.is_err());
    }
}

use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use sync2gether_core::{
    chunks::{self, Identifier},
    datachannel::{self, Request, decode_mse_index},
    protocol::{Media, Message, Presence, WebStreamAnswer, WebStreamOffer},
};
use sync2gether_web::{
    host::{BrowserHost, Command as HostCommand},
    host_stream::{ServerAction, StreamServer},
    remux::{MkvRemuxer, RemuxError},
    stream::StreamClient,
};

const CHUNK: u32 = 4096;
const SDP: &str = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\na=fingerprint:sha-256 AA:BB\r\n";

fn pump_server(server: &mut StreamServer, data: &[u8], client: &mut StreamClient) {
    loop {
        while let Some(frag) = server.take_fragment() {
            client.on_message(&frag);
        }
        for req in client.take_requests() {
            server.on_request(&req).unwrap();
        }
        match server.next_action() {
            Some(ServerAction::ReadChunk {
                index,
                offset,
                length,
            }) => {
                let start = offset as usize;
                let end = (start + length as usize).min(data.len());
                server.provide_chunk(index, &data[start..end]);
            }
            Some(_) => panic!("ação MSE inesperada no teste direto"),
            None => {
                if !server.has_outgoing() {
                    break;
                }
            }
        }
    }
}

#[test]
fn stream_server_serves_hash_table_chunks_and_honors_cancel() {
    let data: Vec<u8> = (0..(CHUNK as usize * 12 + 333))
        .map(|n| (n * 19 % 251) as u8)
        .collect();
    let mut id = Identifier::new(CHUNK);
    id.update(&data);
    let (media, hashes) = id.finish();
    let digest = chunks::table_digest(&hashes);
    let table = hashes.concat();

    let mut server = StreamServer::new(media.bytes, CHUNK, table, false);
    let mut client = StreamClient::new(media.bytes, CHUNK, &digest, 256 * 1024);

    pump_server(&mut server, &data, &mut client);
    assert!(client.ready());
    assert!(client.error().is_none());

    assert_eq!(client.read(0, 127).unwrap(), data[..128]);

    let tail_start = (CHUNK as u64) * 12;
    for _ in 0..8 {
        if let Some(bytes) = client.read(tail_start, media.bytes) {
            assert_eq!(bytes, data[tail_start as usize..]);
            break;
        }
        pump_server(&mut server, &data, &mut client);
    }
    assert!(server.sent_bytes() > 0);
    assert!(server.unique_chunks() >= 2);

    server.on_request(&Request::Chunk(0).encode()).unwrap();
    let action = server.next_action().unwrap();
    assert!(matches!(action, ServerAction::ReadChunk { index: 0, .. }));
    server.on_request(&Request::Cancel.encode()).unwrap();
    server.provide_chunk(0, &data[..CHUNK as usize]);
    assert!(!server.has_outgoing());

    server.on_request(&Request::Chunk(999).encode()).unwrap();
    assert!(server.next_action().is_none());
    let fail_frag = server.take_fragment().unwrap();
    let decoded = datachannel::decode(&fail_frag).unwrap();
    assert!(!decoded.ok);

    assert!(server.on_request(&[99]).is_err());
}

#[test]
fn browser_host_handles_web_stream_offer_answer_and_stop() {
    let mut host = BrowserHost::new("Navegador Host");
    let media = Media {
        hash: "a".repeat(64),
        bytes: 8192,
    };
    host.set_media(Some(media.clone()), Some("Filme.mp4".into()), 10.0);
    assert!(host.view().host_web_stream);
    assert!(!host.view().host_web_remux);

    host.set_stream_capability(true, true, 20.0);
    assert!(host.view().host_web_stream);
    assert!(host.view().host_web_remux);

    host.set_stream_capability(false, false, 30.0);
    assert!(!host.view().host_web_stream);
    assert!(!host.view().host_web_remux);

    host.set_stream_capability(true, false, 40.0);
    let invalid_answer = WebStreamAnswer {
        id: 1,
        sdp: SDP.into(),
        media,
        file_name: "Filme.mp4".into(),
        duration: 10.0,
        chunk_size: 256 * 1024,
        hash_table_digest: "b".repeat(64),
        mse: None,
        subtitle_tracks: vec![],
    };
    assert!(!host.stream_answer(invalid_answer, 50.0));
    let _ = (
        HostCommand::StreamStop { id: 1 },
        Message::WebStreamOffer(Box::new(WebStreamOffer {
            id: 1,
            sdp: SDP.into(),
        })),
        Presence::default(),
    );
}

fn cache_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/web-remux-test");
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn has_ffmpeg_cli() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn generate_aac_mkv(path: &Path) {
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000",
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-map",
            "2:a:0",
            "-t",
            "12",
            "-c:v",
            "libx264",
            "-profile:v",
            "baseline",
            "-pix_fmt",
            "yuv420p",
            "-g",
            "24",
            "-c:a:0",
            "aac",
            "-b:a:0",
            "96k",
            "-metadata:s:a:0",
            "language=por",
            "-metadata:s:a:0",
            "title=Portugues",
            "-disposition:a:0",
            "default",
            "-c:a:1",
            "aac",
            "-b:a:1",
            "96k",
            "-metadata:s:a:1",
            "language=eng",
            "-metadata:s:a:1",
            "title=English",
            "-disposition:a:1",
            "0",
        ])
        .arg(path)
        .status()
        .expect("ffmpeg não encontrado");
    assert!(status.success(), "ffmpeg falhou ao gerar MKV AAC");
}

fn generate_ac3_mkv(path: &Path) {
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "4",
            "-c:v",
            "libx264",
            "-profile:v",
            "baseline",
            "-pix_fmt",
            "yuv420p",
            "-g",
            "24",
            "-c:a",
            "ac3",
            "-b:a",
            "192k",
        ])
        .arg(path)
        .status()
        .expect("ffmpeg não encontrado");
    assert!(status.success(), "ffmpeg falhou ao gerar MKV AC-3");
}

fn has_box(bytes: &[u8], tag: &[u8; 4]) -> bool {
    bytes.windows(4).any(|w| w == tag)
}

#[test]
fn wasm_mkv_remuxer_produces_valid_fmp4_init_and_segments_for_aac_mkv() {
    if !has_ffmpeg_cli() {
        eprintln!("ffmpeg indisponível; pulando teste do remuxer MKV");
        return;
    }
    let dir = cache_dir();
    let aac_path = dir.join("sample-aac.mkv");
    generate_aac_mkv(&aac_path);

    let file = BufReader::new(File::open(&aac_path).unwrap());
    let mut remuxer = MkvRemuxer::open(file).unwrap();
    let mse = remuxer.web_stream_mse();
    assert!(mse.valid(), "WebStreamMse inválido: {mse:?}");
    assert!(mse.mime.starts_with("video/mp4; codecs=\"avc1."));
    assert!(mse.mime.contains("mp4a."));
    assert!(!mse.audio_transcoded);
    assert!(mse.duration >= 11.0 && mse.duration <= 13.0);
    assert_eq!(mse.audio_tracks.len(), 2);
    assert_eq!(mse.default_audio, Some(0));

    let index = decode_mse_index(&remuxer.encoded_index()).unwrap();
    assert_eq!(index.len(), mse.segments as usize);
    assert!(index.len() >= 2);
    let covered_ms: u64 = index.iter().map(|s| u64::from(s.duration_ms)).sum();
    assert!(covered_ms >= 11_000);

    for track in [0u8, 1u8] {
        let init = remuxer.init_segment(track).unwrap();
        assert!(has_box(&init, b"ftyp"));
        assert!(has_box(&init, b"moov"));
        assert!(has_box(&init, b"mvex"));
        for seg_idx in 0..mse.segments {
            let seg = remuxer.media_segment(track, seg_idx).unwrap_or_else(|e| {
                panic!(
                    "track={track} seg={seg_idx}/{}: {e} index={index:?}",
                    mse.segments
                )
            });
            assert!(has_box(&seg, b"moof"));
            assert!(has_box(&seg, b"tfdt"));
            assert!(has_box(&seg, b"mdat"));
            assert!(seg.len() > 128);
        }
    }

    let ac3_path = dir.join("sample-ac3.mkv");
    generate_ac3_mkv(&ac3_path);
    let ac3_file = BufReader::new(File::open(&ac3_path).unwrap());
    let err = MkvRemuxer::open(ac3_file).err().unwrap();
    assert_eq!(
        err,
        RemuxError::UnsupportedAudio {
            codecs: vec!["AC-3".into()]
        }
    );
}

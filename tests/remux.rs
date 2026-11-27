use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use str0m::{
    Candidate, Event, Input, Output, Rtc,
    change::SdpAnswer,
    net::{Protocol, Receive},
};
use sync2gether::stream::{
    CHUNK_SIZE, ChunkIndex,
    ffmpeg::{self, Ffmpeg},
    remux::RemuxSession,
    webrtc,
};
use sync2gether_core::datachannel::{self, Assembly, Item, Request, decode_mse_index};
use tokio::{net::UdpSocket, sync::mpsc};

fn cache_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/remux-test");
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

fn generate_mkv(path: &Path) {
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
            "48",
            "-c:a:0",
            "ac3",
            "-b:a:0",
            "192k",
            "-metadata:s:a:0",
            "language=por",
            "-metadata:s:a:0",
            "title=Portugues",
            "-disposition:a:0",
            "default",
            "-c:a:1",
            "ac3",
            "-b:a:1",
            "192k",
            "-metadata:s:a:1",
            "language=eng",
            "-metadata:s:a:1",
            "title=English",
            "-disposition:a:1",
            "0",
        ])
        .arg(path)
        .status()
        .expect("falha ao executar ffmpeg");
    assert!(status.success(), "ffmpeg falhou ao gerar fixture MKV");
}

fn generate_native_mp4(path: &Path) {
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
            "aac",
            "-b:a",
            "96k",
        ])
        .arg(path)
        .status()
        .expect("falha ao executar ffmpeg");
    assert!(status.success(), "ffmpeg falhou ao gerar fixture MP4");
}

fn contains_box(bytes: &[u8], fourcc: &[u8; 4]) -> bool {
    bytes.windows(4).any(|w| w == fourcc)
}

fn find_tfdt_values(bytes: &[u8]) -> Vec<u64> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 16 <= bytes.len() {
        if &bytes[i + 4..i + 8] == b"tfdt" {
            let size = u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
            if size >= 16 && i + size <= bytes.len() {
                let version = bytes[i + 8];
                if version == 1 && size >= 20 {
                    out.push(u64::from_be_bytes(
                        bytes[i + 12..i + 20].try_into().unwrap(),
                    ));
                } else if version == 0 {
                    out.push(u64::from(u32::from_be_bytes(
                        bytes[i + 12..i + 16].try_into().unwrap(),
                    )));
                }
            }
        }
        i += 1;
    }
    out
}

#[test]
fn missing_ffmpeg_libraries_fail_cleanly_without_panic() {
    let err = Ffmpeg::load_from_dir(Some(Path::new("/nonexistent/ffmpeg-dir")))
        .err()
        .expect("diretório inexistente deve falhar");
    assert!(!err.is_empty());
}

#[test]
fn unsupported_or_mixed_major_versions_fail_cleanly() {
    assert!(ffmpeg::AbiOffsets::for_versions(59, 59, 57, 3).is_err());
    assert!(ffmpeg::AbiOffsets::for_versions(64, 64, 62, 8).is_err());
    assert!(ffmpeg::AbiOffsets::for_versions(60, 61, 58, 4).is_err());
    assert!(ffmpeg::AbiOffsets::for_versions(61, 61, 58, 5).is_err());
    assert!(ffmpeg::AbiOffsets::for_versions(62, 62, 60, 5).is_err());
    assert!(ffmpeg::AbiOffsets::for_versions(63, 62, 61, 7).is_err());
}

#[test]
fn runtime_frame_abi_self_check_aborts_before_write() {
    if !has_ffmpeg_cli() || !ffmpeg::is_available() {
        return;
    }
    let dir = cache_dir();
    let mkv = dir.join("abi-self-check.mkv");
    generate_mkv(&mkv);
    let mut ff = Ffmpeg::load().expect("FFmpeg deve carregar");
    ff.abi.frame_sample_rate += 4;
    let err = RemuxSession::open_with_owned(ff, &mkv)
        .err()
        .expect("desalinhamento de AVFrame deve abortar remux antes de escrever");
    assert!(err.contains("ABI"), "erro inesperado: {err}");
}

#[test]
fn mkv_with_ac3_remuxes_keyframe_segments_and_switches_audio_track() {
    if !has_ffmpeg_cli() || !ffmpeg::is_available() {
        return;
    }
    let dir = cache_dir();
    let mkv = dir.join("sample-ac3.mkv");
    generate_mkv(&mkv);

    let session = RemuxSession::open(&mkv)
        .expect("RemuxSession::open falhou")
        .expect("MKV com AC-3 deve ativar remux MSE");
    let v = session.ffmpeg_versions();
    println!(
        "FFmpeg carregado: lavf={} lavc={} lavu={} swr={}",
        v.lavf, v.lavc, v.lavu, v.swr
    );
    assert!(ffmpeg::AbiOffsets::for_versions(v.lavf, v.lavc, v.lavu, v.swr).is_ok());
    if let Ok(expected) = std::env::var("SYNC2GETHER_EXPECT_FFMPEG_MAJORS")
        && !expected.is_empty()
    {
        let got = format!("{},{},{},{}", v.lavf, v.lavc, v.lavu, v.swr);
        assert_eq!(
            got, expected,
            "versão carregada do FFmpeg difere da esperada"
        );
    }
    let probe = session.probe();
    assert!(probe.needs_remux);
    assert_eq!(probe.audio_streams.len(), 2);
    assert_eq!(probe.default_audio, Some(0));
    assert_eq!(probe.audio_streams[0].info.codec, "ac3");
    assert_eq!(probe.audio_streams[0].info.language.as_deref(), Some("por"));
    assert_eq!(probe.audio_streams[1].info.language.as_deref(), Some("eng"));

    let mse = probe.web_stream_mse();
    assert!(mse.valid());
    assert!(mse.mime.contains("avc1."));
    assert!(mse.mime.contains("mp4a.40.2"));
    assert!(mse.audio_transcoded);
    assert!(mse.segments >= 2);

    let decoded = decode_mse_index(&probe.encoded_index()).unwrap();
    assert_eq!(decoded.len(), mse.segments as usize);
    assert_eq!(decoded[0].start_ms, 0);
    for pair in decoded.windows(2) {
        assert!(pair[1].start_ms > pair[0].start_ms);
    }

    let init0 = session.init_segment(0).unwrap();
    assert!(contains_box(&init0, b"ftyp"));
    assert!(contains_box(&init0, b"moov"));

    let seg0 = session.media_segment(0, 0).unwrap();
    assert!(contains_box(&seg0, b"moof"));
    assert!(contains_box(&seg0, b"mdat"));
    assert!(!contains_box(&seg0, b"ftyp"));

    let last = mse.segments - 1;
    let seg_last = session.media_segment(0, last).unwrap();
    assert!(contains_box(&seg_last, b"moof"));
    assert!(contains_box(&seg_last, b"mdat"));
    let tfdts = find_tfdt_values(&seg_last);
    assert!(!tfdts.is_empty());
    assert!(
        tfdts.iter().all(|&v| v > 0),
        "segmento posterior deve manter tfdt > 0, obtido {tfdts:?}"
    );

    let init1 = session.init_segment(1).unwrap();
    assert!(contains_box(&init1, b"ftyp"));
    assert!(contains_box(&init1, b"moov"));
    let seg1 = session.media_segment(1, 0).unwrap();
    assert!(contains_box(&seg1, b"moof"));
    assert!(contains_box(&seg1, b"mdat"));

    assert!(session.media_segment(0, mse.segments + 5).is_err());

    let mp4 = dir.join("native.mp4");
    generate_native_mp4(&mp4);
    let native = RemuxSession::open(&mp4).expect("probe de MP4 nativo falhou");
    assert!(
        native.is_none(),
        "MP4 H.264/AAC nativo deve manter fluxo direto"
    );
}

struct Peer {
    rtc: Rtc,
    socket: UdpSocket,
    channel: Option<str0m::channel::ChannelId>,
    received: VecDeque<Vec<u8>>,
}

impl Peer {
    fn drain(&mut self) -> Instant {
        loop {
            match self.rtc.poll_output().unwrap() {
                Output::Timeout(at) => return at,
                Output::Transmit(transmit) => {
                    let _ = self
                        .socket
                        .try_send_to(&transmit.contents, transmit.destination);
                }
                Output::Event(Event::ChannelOpen(id, _)) => self.channel = Some(id),
                Output::Event(Event::ChannelData(data)) => self.received.push_back(data.data),
                Output::Event(_) => {}
            }
        }
    }

    async fn step(&mut self) {
        let deadline = self.drain();
        let mut buffer = vec![0; 2048];
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(50));
        let local = self.socket.local_addr().unwrap();
        tokio::select! {
            received = self.socket.recv_from(&mut buffer) => {
                let (n, source) = received.unwrap();
                if let Ok(receive) = Receive::new(Protocol::Udp, source, local, &buffer[..n]) {
                    self.rtc.handle_input(Input::Receive(Instant::now(), receive)).unwrap();
                }
            }
            _ = tokio::time::sleep(wait) => {
                self.rtc.handle_input(Input::Timeout(Instant::now())).unwrap();
            }
        }
        self.drain();
    }

    fn request(&mut self, request: Request) {
        let id = self.channel.unwrap();
        assert!(
            self.rtc
                .channel(id)
                .unwrap()
                .write(true, &request.encode())
                .unwrap()
        );
        self.drain();
    }

    async fn fetch(&mut self, item: Item, max: usize) -> Vec<u8> {
        let mut assembly = Assembly::new(item);
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            while let Some(message) = self.received.pop_front() {
                let fragment = datachannel::decode(&message).unwrap();
                if fragment.item != item {
                    continue;
                }
                assert!(fragment.ok, "anfitrião recusou {item:?}");
                if let Some(done) = assembly.push(&fragment, max).unwrap() {
                    return done;
                }
            }
            assert!(Instant::now() < deadline, "resposta não chegou: {item:?}");
            self.step().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webrtc_host_serves_mse_index_init_and_remuxed_segments() {
    if !has_ffmpeg_cli() || !ffmpeg::is_available() {
        return;
    }
    let mkv = cache_dir().join("webrtc-ac3.mkv");
    generate_mkv(&mkv);
    let (_, index) = ChunkIndex::compute_file(&mkv, CHUNK_SIZE).unwrap();
    let index = Arc::new(index);

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let local = socket.local_addr().unwrap();
    let mut rtc = Rtc::builder().build(Instant::now());
    rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
    let mut peer = Peer {
        rtc,
        socket,
        channel: None,
        received: VecDeque::new(),
    };
    peer.drain();
    let mut change = peer.rtc.sdp_api();
    change.add_channel(datachannel::LABEL.into());
    let (offer, pending) = change.apply().unwrap();
    peer.drain();

    let (answer, host) = webrtc::prepare(&offer.to_sdp_string(), mkv, index, 12.0, false, true)
        .await
        .unwrap();
    let mse = host.mse_info().expect("MKV deve expor metadados MSE");
    assert_eq!(mse.audio_tracks.len(), 2);

    let (events, _rx) = mpsc::channel(64);
    let task = host.run(7, events);
    peer.rtc
        .sdp_api()
        .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
        .unwrap();
    peer.drain();

    let deadline = Instant::now() + Duration::from_secs(20);
    while peer.channel.is_none() {
        assert!(Instant::now() < deadline, "canal de dados não abriu");
        peer.step().await;
    }

    peer.request(Request::MseIndex);
    let raw_index = peer.fetch(Item::MseIndex, 1 << 20).await;
    let segments = decode_mse_index(&raw_index).unwrap();
    assert_eq!(segments.len(), mse.segments as usize);

    peer.request(Request::MseInit(0));
    let init = peer.fetch(Item::MseInit(0), 1 << 20).await;
    assert!(contains_box(&init, b"ftyp"));

    let last = mse.segments - 1;
    peer.request(Request::MseSegment {
        track: 0,
        segment: last,
    });
    let seg = peer
        .fetch(
            Item::MseSegment {
                track: 0,
                segment: last,
            },
            16 << 20,
        )
        .await;
    assert!(contains_box(&seg, b"moof"));
    assert!(contains_box(&seg, b"mdat"));

    peer.request(Request::MseInit(1));
    let init1 = peer.fetch(Item::MseInit(1), 1 << 20).await;
    assert!(contains_box(&init1, b"moov"));

    task.abort();
}

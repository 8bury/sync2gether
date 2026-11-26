use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use str0m::{
    Candidate, Event, Input, Output, Rtc,
    change::SdpAnswer,
    net::{Protocol, Receive},
};
use sync2gether::stream::{CHUNK_SIZE, ChunkIndex, StreamEvent, StreamPhase, webrtc};
use sync2gether_core::{
    chunks,
    datachannel::{self, Assembly, Item, Request},
};
use tokio::{net::UdpSocket, sync::mpsc};

fn write_fixture(name: &str, size: usize) -> (PathBuf, Vec<u8>) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/web-stream-test");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let data: Vec<u8> = (0..size)
        .map(|i| ((i.wrapping_mul(17) ^ (i >> 5)) & 0xff) as u8)
        .collect();
    fs::write(&path, &data).unwrap();
    (path, data)
}

struct Browser {
    rtc: Rtc,
    socket: UdpSocket,
    channel: Option<str0m::channel::ChannelId>,
    received: VecDeque<Vec<u8>>,
}

impl Browser {
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
        let accepted = self
            .rtc
            .channel(id)
            .unwrap()
            .write(true, &request.encode())
            .unwrap();
        assert!(accepted);
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
async fn data_channel_streams_verified_chunks_and_cancels_queued_requests() {
    let size = CHUNK_SIZE as usize * 5 + 4_321;
    let (path, original) = write_fixture("filme.mp4", size);
    let (media, index) = ChunkIndex::compute_file(&path, CHUNK_SIZE).unwrap();
    let index = Arc::new(index);

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let local = socket.local_addr().unwrap();
    let mut rtc = Rtc::builder().build(Instant::now());
    rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
    let mut browser = Browser {
        rtc,
        socket,
        channel: None,
        received: VecDeque::new(),
    };
    browser.drain();
    let mut change = browser.rtc.sdp_api();
    change.add_channel(datachannel::LABEL.into());
    let (offer, pending) = change.apply().unwrap();
    browser.drain();

    let offer = offer.to_sdp_string();
    assert!(sync2gether::protocol::valid_sdp(&offer));
    let (answer, host) = webrtc::prepare(&offer, path, index.clone(), 30.0, false, true)
        .await
        .unwrap();
    assert!(sync2gether::protocol::valid_sdp(&answer));
    assert_eq!(host.chunk_size(), CHUNK_SIZE);
    assert_eq!(host.digest(), index.digest);
    let (events, mut events_rx) = mpsc::channel(64);
    let task = host.run(42, events);
    browser
        .rtc
        .sdp_api()
        .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
        .unwrap();
    browser.drain();

    let deadline = Instant::now() + Duration::from_secs(20);
    while browser.channel.is_none() {
        assert!(Instant::now() < deadline, "canal de dados não abriu");
        browser.step().await;
    }

    browser.request(Request::HashTable);
    let table = browser.fetch(Item::HashTable, 1 << 20).await;
    let hashes = chunks::verify_table(&table, &index.digest, media.bytes, CHUNK_SIZE).unwrap();
    assert_eq!(hashes.len(), 6);

    for chunk in [5u32, 0, 3] {
        browser.request(Request::Chunk(chunk));
        let data = browser.fetch(Item::Chunk(chunk), CHUNK_SIZE as usize).await;
        let start = chunk as usize * CHUNK_SIZE as usize;
        assert_eq!(data, original[start..(start + data.len())]);
        assert!(chunks::chunk_matches(&hashes[chunk as usize], &data));
    }

    browser.request(Request::Chunk(99));
    let deadline = Instant::now() + Duration::from_secs(20);
    'refused: loop {
        while let Some(message) = browser.received.pop_front() {
            let fragment = datachannel::decode(&message).unwrap();
            if fragment.item == Item::Chunk(99) {
                assert!(!fragment.ok);
                break 'refused;
            }
        }
        assert!(Instant::now() < deadline, "recusa não chegou");
        browser.step().await;
    }

    for chunk in [1u32, 2, 4] {
        browser.request(Request::Chunk(chunk));
    }
    browser.request(Request::Cancel);
    browser.request(Request::Chunk(2));
    let data = browser.fetch(Item::Chunk(2), CHUNK_SIZE as usize).await;
    assert!(chunks::chunk_matches(&hashes[2], &data));

    let mut streaming = false;
    while let Ok(event) = events_rx.try_recv() {
        if let StreamEvent::Telemetry { id: 42, telemetry } = event {
            streaming |= telemetry.phase == StreamPhase::Streaming && telemetry.is_sender;
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while !streaming && Instant::now() < deadline {
        browser.step().await;
        while let Ok(event) = events_rx.try_recv() {
            if let StreamEvent::Telemetry { id: 42, telemetry } = event {
                streaming |= telemetry.phase == StreamPhase::Streaming;
            }
        }
    }
    assert!(streaming, "telemetria do anfitrião não indicou streaming");
    task.abort();
}

#[tokio::test]
async fn offers_that_are_not_sdp_are_refused() {
    let (path, _) = write_fixture("recusa.mp4", 4096);
    let (_, index) = ChunkIndex::compute_file(&path, CHUNK_SIZE).unwrap();
    let result = webrtc::prepare("bobagem", path, Arc::new(index), 1.0, false, true).await;
    assert!(result.is_err());
}

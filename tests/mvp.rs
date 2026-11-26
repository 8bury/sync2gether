use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use sync2gether::{
    protocol::Control,
    runtime::{Command, Role, Runtime, View},
};

#[track_caller]
fn wait(runtime: &Runtime, predicate: impl Fn(&View) -> bool) -> View {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut last = String::new();
    while Instant::now() < deadline {
        if let Ok(view) = runtime.views.recv_timeout(Duration::from_millis(300)) {
            assert!(view.error.is_none(), "runtime error: {:?}", view.error);
            last = format!(
                "{}: pos={}, loaded={}, paused={}, ready={}, blocked={}, connected={}, preparing={}, scheduled={}, last_start={:?}",
                view.status,
                view.player.position,
                view.player.loaded,
                view.player.paused,
                view.ready,
                view.player.blocked,
                view.connected,
                view.preparing,
                view.scheduled,
                view.last_start_id,
            );
            if predicate(&view) {
                return view;
            }
        }
    }
    panic!("condition timed out: {last}");
}
fn send(runtime: &Runtime, command: Command) {
    runtime.commands.blocking_send(command).unwrap();
}
fn open_both(host: &Runtime, host_file: PathBuf, guest: &Runtime, guest_file: PathBuf) {
    send(host, Command::OpenPath(host_file));
    wait(host, |v| v.player.loaded);
    send(guest, Command::OpenPath(guest_file));
    wait(guest, |v| v.player.loaded);
}
fn shutdown(mut runtime: Runtime) {
    runtime.shutdown();
    runtime.shutdown();
    while runtime.views.try_recv().is_ok() {}
    assert!(matches!(
        runtime.views.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
#[ignore = "requer libmpv e vídeo sintético gerado por scripts/test-mvp.sh"]
fn two_players_sync_mismatch_disconnect_and_resume() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/mvp-fixture.mkv");
    assert!(fixture.is_file(), "execute scripts/test-mvp.sh");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    let host = Runtime::start();
    let guest = Runtime::start();
    open_both(
        &host,
        fixture.clone(),
        &guest,
        fixture.with_file_name("mvp-renamed.mkv"),
    );
    send(&host, Command::Host(address.clone()));
    let host_view = wait(&host, |v| v.role == Role::Host);
    let key = host_view.room_key;
    send(&guest, Command::Join(address.clone(), key.clone()));
    let h = wait(&host, |v| v.ready);
    let g = wait(&guest, |v| v.ready);
    assert_eq!(h.file.as_deref(), Some("mvp-fixture.mkv"));
    assert_eq!(h.peer_file.as_deref(), Some("mvp-renamed.mkv"));
    assert_eq!(g.peer_file, h.file);
    assert_eq!(g.file, h.peer_file);
    send(&guest, Command::Control(Control::Play));
    wait(&host, |v| !v.player.paused && v.player.position > 1.0);
    wait(&guest, |v| !v.player.paused && v.player.position > 1.0);
    assert!(
        host.frames
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|f| f.iter().any(|b| *b != 0))
    );
    send(&guest, Command::Control(Control::Seek(8.0)));
    wait(&host, |v| (8.0..10.0).contains(&v.player.position));
    wait(&guest, |v| (8.0..10.0).contains(&v.player.position));
    send(&host, Command::Control(Control::Pause));
    let h = wait(&host, |v| v.player.paused);
    let g = wait(&guest, |v| {
        v.player.paused && (v.player.position - h.player.position).abs() < 0.4
    });
    assert!((h.player.position - g.player.position).abs() < 0.4);
    send(&guest, Command::Control(Control::Seek(20.0)));
    wait(&host, |v| v.player.ended && !v.preparing && v.player.paused);
    wait(&guest, |v| {
        v.player.ended && !v.preparing && v.player.paused
    });
    send(&host, Command::Control(Control::Seek(4.0)));
    wait(&host, |v| v.ready && v.player.position < 4.2);
    wait(&guest, |v| v.ready && v.player.position < 4.2);
    send(&guest, Command::Control(Control::Play));
    wait(&host, |v| !v.player.paused);
    send(&guest, Command::Leave);
    wait(&host, |v| !v.connected && v.peer_left && v.player.paused);
    send(&guest, Command::Join(address, key));
    let h = wait(&host, |v| v.ready && v.player.paused);
    assert!(!h.peer_left);
    let g = wait(&guest, |v| v.ready && v.player.paused);
    assert_eq!(h.peer_file.as_deref(), Some("mvp-renamed.mkv"));
    assert_eq!(g.peer_file.as_deref(), Some("mvp-fixture.mkv"));
    send(
        &guest,
        Command::OpenPath(fixture.with_file_name("mvp-different.mkv")),
    );
    wait(&guest, |v| {
        !v.loading && v.player.loaded && v.file.as_deref() == Some("mvp-different.mkv")
    });
    wait(&host, |v| !v.matched && v.peer_ready);
    send(&guest, Command::Control(Control::Play));
    wait(&host, |v| !v.matched && v.player.paused);
    send(&guest, Command::OpenPath(fixture));
    wait(&host, |v| v.ready);
    wait(&guest, |v| v.ready);
    send(&host, Command::Control(Control::Seek(19.9)));
    send(&host, Command::Control(Control::Play));
    wait(&host, |v| v.player.blocked && v.player.paused);
    send(&guest, Command::Control(Control::Seek(0.0)));
    wait(&host, |v| v.ready && v.player.position < 0.5);
    wait(&guest, |v| v.ready && v.player.position < 0.5);
    shutdown(host);
    host_closed_the_room(&guest);
    shutdown(guest);
}

fn host_closed_the_room(guest: &Runtime) {
    let view = wait(guest, |v| v.role == Role::Local && v.player.paused);
    assert!(
        view.notice
            .as_deref()
            .is_some_and(|notice| notice.ends_with(&(sync2gether::i18n::text().room.closed_by)(""))),
        "{:?}",
        view.notice
    );
    assert!(!view.connected);
}

#[derive(Clone)]
struct Link {
    min_ms: u64,
    jitter_ms: u64,
    hold_prepared: Arc<AtomicBool>,
    stalled: Arc<AtomicBool>,
}
impl Link {
    fn new(min_ms: u64, jitter_ms: u64) -> Self {
        Self {
            min_ms,
            jitter_ms,
            hold_prepared: Arc::default(),
            stalled: Arc::default(),
        }
    }
}

async fn delayed_forward<R, W>(mut read: R, mut write: W, link: Link, mut seed: u64)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use sync2gether::network::{read_message, write_message};
    use tokio::time::{Instant, sleep, sleep_until};
    let (queue, mut queued) = tokio::sync::mpsc::unbounded_channel();
    let (min_ms, jitter_ms) = (link.min_ms, link.jitter_ms);
    let receive = async move {
        let mut release = Instant::now();
        while let Ok(message) = read_message(&mut read).await {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let delay = min_ms + (seed >> 33) % (jitter_ms + 1);
            release = release.max(Instant::now() + Duration::from_millis(delay));
            if queue.send((release, message)).is_err() {
                break;
            }
        }
    };
    let deliver = async move {
        while let Some((release, message)) = queued.recv().await {
            sleep_until(release).await;
            while link.stalled.load(Ordering::SeqCst)
                || (matches!(message, sync2gether::protocol::Message::Prepared { .. })
                    && link.hold_prepared.load(Ordering::SeqCst))
            {
                sleep(Duration::from_millis(5)).await;
            }
            if write_message(&mut write, &message).await.is_err() {
                break;
            }
        }
    };
    tokio::select! { _ = receive => {}, _ = deliver => {} }
}

fn proxy(
    io: &tokio::runtime::Runtime,
    host: std::net::SocketAddr,
    link: Link,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = io
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let address = listener.local_addr().unwrap();
    let task = io.spawn(async move {
        let mut seed = 1;
        while let Ok((client, _)) = listener.accept().await {
            let Ok(server) = tokio::net::TcpStream::connect(host).await else {
                continue;
            };
            let (cr, cw) = client.into_split();
            let (sr, sw) = server.into_split();
            tokio::select! {
                _ = delayed_forward(cr, sw, link.clone(), seed) => {},
                _ = delayed_forward(sr, cw, link.clone(), seed + 1) => {},
            }
            seed += 2;
        }
    });
    (address, task)
}

#[test]
#[ignore = "requer libmpv e vídeo sintético gerado por scripts/test-mvp.sh"]
fn schedules_two_real_players_under_network_delay_and_jitter() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/mvp-fixture.mkv");
    let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reserve.local_addr().unwrap();
    drop(reserve);
    let io = tokio::runtime::Runtime::new().unwrap();
    let link = Link::new(40, 25);
    let hold_prepared = link.hold_prepared.clone();
    let (proxy_address, proxy) = proxy(&io, address, link);
    let host = Runtime::start();
    let guest = Runtime::start();
    open_both(&host, fixture.clone(), &guest, fixture);
    send(&host, Command::Host(address.to_string()));
    let key = wait(&host, |v| v.role == Role::Host).room_key;
    send(&guest, Command::Join(proxy_address.to_string(), key));
    wait(&host, |v| v.ready);
    wait(&guest, |v| v.ready);
    send(&guest, Command::Control(Control::Play));
    let scheduled = wait(&host, |v| v.scheduled);
    assert!(scheduled.player.paused);
    let h = wait(&host, |v| v.last_start_id.is_some());
    let g = wait(&guest, |v| v.last_start_id == h.last_start_id);
    let host_late = h.last_start_ms.unwrap() - h.scheduled_start_ms.unwrap();
    let guest_late = g.last_start_ms.unwrap() - g.scheduled_start_ms.unwrap();
    assert!(
        (0.0..100.0).contains(&host_late),
        "host deadline missed: {host_late}"
    );
    assert!(
        guest_late.abs() < 100.0,
        "guest deadline missed: {guest_late}"
    );
    assert!((h.last_start_ms.unwrap() - g.last_start_ms.unwrap()).abs() < 100.0);
    let v = wait(&guest, |v| {
        v.player.position > 1.0 && v.peer_drift.is_some()
    });
    assert!(
        v.peer_drift.unwrap().abs() < 0.15,
        "peer drift {}",
        v.peer_drift.unwrap()
    );
    send(&guest, Command::Control(Control::Seek(6.0)));
    let h2 = wait(&host, |v| {
        v.last_start_id
            .is_some_and(|id| Some(id) != h.last_start_id)
    });
    let g2 = wait(&guest, |v| v.last_start_id == h2.last_start_id);
    assert!((h2.last_start_ms.unwrap() - g2.last_start_ms.unwrap()).abs() < 100.0);
    hold_prepared.store(true, Ordering::SeqCst);
    send(&guest, Command::Control(Control::Seek(10.0)));
    wait(&host, |v| v.preparing);
    send(&host, Command::Control(Control::Pause));
    wait(&host, |v| !v.preparing && !v.scheduled && v.player.paused);
    hold_prepared.store(false, Ordering::SeqCst);
    wait(&guest, |v| !v.preparing && !v.scheduled && v.player.paused);
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        let v = guest
            .views
            .recv_timeout(Duration::from_millis(500))
            .unwrap();
        assert!(v.player.paused, "a canceled operation restarted playback");
    }
    shutdown(host);
    shutdown(guest);
    proxy.abort();
}

#[track_caller]
fn hold(runtime: &Runtime, holds: impl Fn(&View) -> bool, until: impl Fn(&View) -> bool) -> View {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Ok(view) = runtime.views.recv_timeout(Duration::from_millis(300)) {
            assert!(view.error.is_none(), "runtime error: {:?}", view.error);
            assert!(
                holds(&view),
                "condition broken: {}, pos={}, paused={}, connected={}, notice={:?}",
                view.status,
                view.player.position,
                view.player.paused,
                view.connected,
                view.notice,
            );
            if until(&view) {
                return view;
            }
        }
    }
    panic!("condition timed out");
}

#[test]
#[ignore = "requer libmpv e vídeo sintético gerado por scripts/test-mvp.sh"]
fn intercontinental_delay_survives_stalls_and_pauses_when_the_link_is_lost() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/mvp-fixture.mkv");
    let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reserve.local_addr().unwrap();
    drop(reserve);
    let io = tokio::runtime::Runtime::new().unwrap();
    let link = Link::new(90, 60);
    let stalled = link.stalled.clone();
    let (proxy_address, proxy) = proxy(&io, address, link);
    let host = Runtime::start();
    let guest = Runtime::start();
    open_both(&host, fixture.clone(), &guest, fixture);
    send(&host, Command::Host(address.to_string()));
    let key = wait(&host, |v| v.role == Role::Host).room_key;
    send(&guest, Command::Join(proxy_address.to_string(), key));
    wait(&host, |v| v.ready);
    let g = wait(&guest, |v| v.ready);
    assert!(
        (180.0..300.0).contains(&g.rtt_ms),
        "measured round trip {}",
        g.rtt_ms
    );
    send(&guest, Command::Control(Control::Play));
    let h = wait(&host, |v| v.last_start_id.is_some());
    let g = wait(&guest, |v| v.last_start_id == h.last_start_id);
    let start_gap = h.last_start_ms.unwrap() - g.last_start_ms.unwrap();
    assert!(start_gap.abs() < 100.0, "start gap {start_gap}");
    wait(&host, |v| !v.player.paused && v.player.position > 1.0);
    wait(&guest, |v| !v.player.paused && v.player.position > 1.0);

    stalled.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_secs(2));
    stalled.store(false, Ordering::SeqCst);
    let started = h.last_start_id;
    let playing = |v: &View| !v.player.paused && v.connected && v.last_start_id == started;
    hold(&host, playing, |v| v.player.position > 5.0);
    let g = hold(&guest, playing, |v| {
        v.player.position > 5.0 && v.peer_drift.is_some()
    });
    assert!(g.drift.abs() < 0.15, "drift after stall {}", g.drift);
    assert!(
        g.peer_drift.unwrap().abs() < 0.15,
        "peer drift after stall {}",
        g.peer_drift.unwrap()
    );
    println!(
        "intercontinental: rtt={:.0} ms, start gap={start_gap:.1} ms, drift={:.0} ms, peer drift={:.0} ms",
        g.rtt_ms,
        g.drift * 1000.0,
        g.peer_drift.unwrap() * 1000.0
    );

    stalled.store(true, Ordering::SeqCst);
    wait(&guest, |v| !v.connected && v.player.paused);
    stalled.store(false, Ordering::SeqCst);
    wait(&host, |v| !v.connected && v.player.paused);
    wait(&host, |v| v.ready && v.player.paused);
    wait(&guest, |v| v.ready && v.player.paused);
    send(&host, Command::Control(Control::Play));
    let h = wait(&host, |v| {
        v.last_start_id.is_some() && v.last_start_id != started
    });
    let g = wait(&guest, |v| v.last_start_id == h.last_start_id);
    let start_gap = h.last_start_ms.unwrap() - g.last_start_ms.unwrap();
    assert!(
        start_gap.abs() < 100.0,
        "start gap after reconnect {start_gap}"
    );
    shutdown(host);
    shutdown(guest);
    proxy.abort();
}

#[test]
#[ignore = "requer libmpv e vídeo sintético gerado por scripts/test-mvp.sh"]
fn approved_room_waits_for_explicit_play_and_returns_to_preparation_on_file_change() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/mvp-fixture.mkv");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    let host = Runtime::start();
    let guest = Runtime::start();
    send(&host, Command::HostRoom(address.clone()));
    wait(&host, |v| v.role == Role::Host);
    send(&guest, Command::RequestJoin(address));
    wait(&guest, |v| v.waiting_approval && !v.connected);
    let pending = wait(&host, |v| v.join_request.is_some());
    send(
        &host,
        Command::Approve {
            id: pending.join_request.unwrap().id,
            accept: true,
        },
    );
    wait(&guest, |v| v.connected);
    open_both(&host, fixture.clone(), &guest, fixture.clone());
    let h = wait(&host, |v| v.ready);
    let g = wait(&guest, |v| v.ready);
    assert!(h.player.paused && g.player.paused && !h.watching && !g.watching);
    send(&guest, Command::Control(Control::Play));
    wait(&host, |v| v.watching && !v.player.paused);
    wait(&guest, |v| v.watching && !v.player.paused);
    send(
        &guest,
        Command::OpenPath(fixture.with_file_name("mvp-different.mkv")),
    );
    wait(&host, |v| {
        !v.watching && v.player.paused && v.peer_ready && !v.matched
    });
    wait(&guest, |v| {
        !v.watching && v.player.paused && !v.loading && !v.matched
    });
    send(&guest, Command::OpenPath(fixture));
    wait(&host, |v| v.ready && !v.watching);
    wait(&guest, |v| v.ready && !v.watching);
    send(&guest, Command::Leave);
    wait(&host, |v| !v.connected && v.peer_left && v.player.paused);
    let guest_view = wait(&guest, |v| v.role == Role::Local);
    assert!(!guest_view.watching);
    shutdown(host);
    shutdown(guest);
}

#[test]
#[ignore = "requer libmpv, execute scripts/test-mvp.sh"]
fn automatic_host_and_live_discovery_work_without_a_vpn() {
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let host = Runtime::start();
    let guest = Runtime::start();
    send(&host, Command::HostAuto(port));
    let view = wait(&host, |v| v.role == Role::Host);
    assert_eq!(view.room_address.unwrap().port(), port);
    send(&guest, Command::Discover);
    let view = wait(&guest, |v| {
        v.rooms.iter().any(|room| room.address.port() == port)
    });
    let room = view
        .rooms
        .iter()
        .find(|room| room.address.port() == port)
        .unwrap();
    send(&guest, Command::RequestJoin(room.address.to_string()));
    let view = wait(&host, |v| v.join_request.is_some());
    send(
        &host,
        Command::Approve {
            id: view.join_request.unwrap().id,
            accept: true,
        },
    );
    wait(&host, |v| v.connected);
    wait(&guest, |v| v.connected);
    send(&guest, Command::Leave);
    wait(&guest, |v| v.role == Role::Local);
    send(&guest, Command::Discover);
    wait(&guest, |v| {
        v.rooms.iter().any(|room| room.address.port() == port)
    });
    send(&host, Command::Leave);
    wait(&host, |v| v.role == Role::Local);
    wait(&guest, |v| {
        !v.rooms.iter().any(|room| room.address.port() == port)
    });
    send(&guest, Command::CancelDiscovery);
    shutdown(host);
    shutdown(guest);
}

async fn delayed_bytes<R, W>(mut read: R, mut write: W, min_ms: u64, jitter_ms: u64, mut seed: u64)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::time::{Instant, sleep_until};
    let (queue, mut queued) = tokio::sync::mpsc::unbounded_channel();
    let receive = async move {
        let mut release = Instant::now();
        let mut buffer = vec![0; 16 * 1024];
        while let Ok(read) = read.read(&mut buffer).await {
            if read == 0 {
                break;
            }
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let delay = min_ms + (seed >> 33) % (jitter_ms + 1);
            release = release.max(Instant::now() + Duration::from_millis(delay));
            if queue.send((release, buffer[..read].to_vec())).is_err() {
                break;
            }
        }
    };
    let deliver = async move {
        while let Some((release, bytes)) = queued.recv().await {
            sleep_until(release).await;
            if write.write_all(&bytes).await.is_err() {
                break;
            }
        }
    };
    tokio::select! { _ = receive => {}, _ = deliver => {} }
}

#[test]
#[ignore = "requer libmpv e vídeo sintético gerado por scripts/test-mvp.sh"]
fn internet_room_syncs_two_real_players_through_the_relay_with_intercontinental_delay() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/mvp-fixture.mkv");
    let io = tokio::runtime::Runtime::new().unwrap();
    let listener = io
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let relay = listener.local_addr().unwrap();
    let server = io.spawn(sync2gether_relay::serve(
        listener,
        sync2gether_relay::Limits::default(),
    ));
    let entry = io
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let delayed = entry.local_addr().unwrap();
    let proxy = io.spawn(async move {
        let mut seed = 7;
        while let Ok((client, _)) = entry.accept().await {
            let Ok(server) = tokio::net::TcpStream::connect(relay).await else {
                continue;
            };
            seed += 2;
            tokio::spawn(async move {
                let (cr, cw) = client.into_split();
                let (sr, sw) = server.into_split();
                tokio::select! {
                    _ = delayed_bytes(cr, sw, 110, 30, seed) => {},
                    _ = delayed_bytes(sr, cw, 110, 30, seed + 1) => {},
                }
            });
        }
    });
    let host = Runtime::start();
    let guest = Runtime::start();
    open_both(&host, fixture.clone(), &guest, fixture);
    send(&host, Command::HostInternet(relay.to_string()));
    let code = wait(&host, |v| v.room_code.is_some()).room_code.unwrap();
    send(
        &guest,
        Command::JoinCode {
            relay: delayed.to_string(),
            code,
        },
    );
    let h = wait(&host, |v| v.ready);
    let g = wait(&guest, |v| v.ready && v.rtt_ms > 200.0);
    assert!(h.internet && g.internet);
    assert!(h.peer_name.is_some() && g.peer_name.is_some());
    send(&guest, Command::Control(Control::Play));
    let h = wait(&host, |v| v.last_start_id.is_some());
    let g = wait(&guest, |v| v.last_start_id == h.last_start_id);
    assert!(
        (h.last_start_ms.unwrap() - g.last_start_ms.unwrap()).abs() < 100.0,
        "starts apart: {} ms",
        h.last_start_ms.unwrap() - g.last_start_ms.unwrap()
    );
    let v = wait(&guest, |v| {
        v.player.position > 2.0 && v.peer_drift.is_some()
    });
    assert!(
        v.peer_drift.unwrap().abs() < 0.15,
        "peer drift {}",
        v.peer_drift.unwrap()
    );
    send(&host, Command::Control(Control::Pause));
    let h = wait(&host, |v| v.player.paused);
    wait(&guest, |v| {
        v.player.paused && (v.player.position - h.player.position).abs() < 0.4
    });
    send(&host, Command::Leave);
    host_closed_the_room(&guest);
    shutdown(host);
    shutdown(guest);
    proxy.abort();
    server.abort();
}

#[test]
#[ignore = "requer libmpv e vídeo sintético gerado por scripts/test-mvp.sh"]
fn two_players_sync_via_p2p_streaming_without_guest_local_file() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/mvp-fixture.mkv");
    assert!(fixture.is_file(), "execute scripts/test-mvp.sh");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    let host = Runtime::start();
    let guest = Runtime::start();

    send(&host, Command::OpenPath(fixture));
    wait(&host, |v| v.player.loaded && v.verified);

    send(&host, Command::Host(address.clone()));
    let host_view = wait(&host, |v| v.role == Role::Host);
    let key = host_view.room_key;
    send(&guest, Command::Join(address, key));

    wait(&host, |v| v.connected && v.can_stream);
    wait(&guest, |v| v.connected && v.can_stream && v.file.is_none());

    send(&guest, Command::StartStream);
    let h = wait(&host, |v| v.ready);
    let g = wait(&guest, |v| v.ready && v.stream.active && v.player.loaded);
    assert_eq!(g.file.as_deref(), Some("mvp-fixture.mkv"));
    assert_eq!(h.peer_file.as_deref(), Some("mvp-fixture.mkv"));
    assert!(g.stream.link_bps.unwrap_or(0) > 0);

    send(&guest, Command::Control(Control::Play));
    wait(&host, |v| !v.player.paused && v.player.position > 1.2);
    wait(&guest, |v| !v.player.paused && v.player.position > 1.2);

    send(&guest, Command::Control(Control::Seek(8.0)));
    wait(&host, |v| (8.0..10.5).contains(&v.player.position));
    wait(&guest, |v| (8.0..10.5).contains(&v.player.position));

    send(&host, Command::Control(Control::Pause));
    let h = wait(&host, |v| v.player.paused);
    let g = wait(&guest, |v| {
        v.player.paused && (v.player.position - h.player.position).abs() < 0.4
    });
    assert!((h.player.position - g.player.position).abs() < 0.4);

    send(&guest, Command::StopStream);
    wait(&guest, |v| !v.stream.active);
    shutdown(host);
    shutdown(guest);
}

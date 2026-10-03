//! Integração real libmpv/TCP. Execute com scripts/test-mvp.sh.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use sync2gether::{
    protocol::Control,
    runtime::{Command, Role, Runtime, View},
};

fn wait(runtime: &Runtime, predicate: impl Fn(&View) -> bool) -> View {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut last = String::new();
    while Instant::now() < deadline {
        if let Ok(view) = runtime.views.recv_timeout(Duration::from_millis(300)) {
            assert!(view.error.is_none(), "runtime error: {:?}", view.error);
            last = format!(
                "{}: pos={}, paused={}, ready={}, blocked={}",
                view.status,
                view.player.position,
                view.player.paused,
                view.ready,
                view.player.blocked
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
    send(&host, Command::OpenPath(fixture.clone()));
    send(
        &guest,
        Command::OpenPath(fixture.with_file_name("mvp-renamed.mkv")),
    );
    wait(&host, |v| v.player.loaded);
    wait(&guest, |v| v.player.loaded);
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
    wait(&host, |v| !v.connected && v.player.paused);
    send(&guest, Command::Join(address, key));
    let h = wait(&host, |v| v.ready && v.player.paused);
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
    send(&host, Command::Shutdown);
    send(&guest, Command::Shutdown);
}

/// Um proxy por direção acrescenta atraso e jitter sem precisar de privilégios.
async fn delayed_forward<R, W>(mut read: R, mut write: W)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use sync2gether::network::{read_message, write_message};
    let mut n = 0;
    while let Ok(message) = read_message(&mut read).await {
        let delay = [40, 65, 45, 55][n % 4];
        n += 1;
        tokio::time::sleep(Duration::from_millis(delay)).await;
        if write_message(&mut write, &message).await.is_err() {
            break;
        }
    }
}

#[test]
#[ignore = "requer libmpv e vídeo sintético gerado por scripts/test-mvp.sh"]
fn schedules_two_real_players_under_network_delay_and_jitter() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/mvp-fixture.mkv");
    let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reserve.local_addr().unwrap();
    drop(reserve);
    let io = tokio::runtime::Runtime::new().unwrap();
    let listener = io
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let proxy_address = listener.local_addr().unwrap();
    let proxy = io.spawn(async move {
        let (client, _) = listener.accept().await.unwrap();
        let server = tokio::net::TcpStream::connect(address).await.unwrap();
        let (cr, cw) = client.into_split();
        let (sr, sw) = server.into_split();
        tokio::select! { _ = delayed_forward(cr, sw) => {}, _ = delayed_forward(sr, cw) => {} }
    });
    let host = Runtime::start();
    let guest = Runtime::start();
    send(&host, Command::OpenPath(fixture.clone()));
    send(&guest, Command::OpenPath(fixture));
    wait(&host, |v| v.player.loaded);
    wait(&guest, |v| v.player.loaded);
    send(&host, Command::Host(address.to_string()));
    let key = wait(&host, |v| v.role == Role::Host).room_key;
    send(&guest, Command::Join(proxy_address.to_string(), key));
    wait(&host, |v| v.ready);
    wait(&guest, |v| v.ready);
    send(&guest, Command::Control(Control::Play));
    // Captura a fase agendada: nenhum player deve tocar antes do seu deadline.
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
    // Pause cancela uma preparação em andamento, mesmo com mensagens no proxy.
    send(&guest, Command::Control(Control::Seek(10.0)));
    wait(&host, |v| v.preparing);
    send(&host, Command::Control(Control::Pause));
    wait(&host, |v| !v.preparing && !v.scheduled && v.player.paused);
    wait(&guest, |v| !v.preparing && !v.scheduled && v.player.paused);
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        let v = guest
            .views
            .recv_timeout(Duration::from_millis(500))
            .unwrap();
        assert!(v.player.paused, "a canceled operation restarted playback");
    }
    send(&host, Command::Shutdown);
    send(&guest, Command::Shutdown);
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
    send(&host, Command::OpenPath(fixture.clone()));
    send(&guest, Command::OpenPath(fixture.clone()));
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
    wait(&host, |v| !v.connected && v.player.paused);
    let guest_view = wait(&guest, |v| v.role == Role::Local);
    assert!(!guest_view.watching);
    send(&host, Command::Shutdown);
    send(&guest, Command::Shutdown);
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
    send(&host, Command::Shutdown);
    send(&guest, Command::Shutdown);
}

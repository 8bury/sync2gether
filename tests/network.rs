//! Dois clientes TCP reais em localhost, sem Tailscale ou vídeos pessoais.
use std::time::{Duration, Instant};
use sync2gether::{
    network::{self, Event},
    protocol::{Control, Message, VERSION},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::timeout,
};

async fn next(rx: &mut mpsc::Receiver<Event>) -> Event {
    timeout(Duration::from_secs(6), rx.recv())
        .await
        .unwrap()
        .unwrap()
}
async fn connected(rx: &mut mpsc::Receiver<Event>) -> mpsc::Sender<Message> {
    loop {
        if let Event::Connected(tx) = next(rx).await {
            return tx;
        }
    }
}
#[tokio::test]
async fn authenticates_orders_messages_and_reconnects() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (host_tx, mut host_rx) = mpsc::channel(64);
    let key = "a".repeat(32);
    let host = tokio::spawn(network::host(
        listener,
        key.clone(),
        host_tx,
        Instant::now(),
    ));
    for (version, bad_key) in [(VERSION, "b".repeat(32)), (VERSION + 1, key.clone())] {
        let mut rejected = TcpStream::connect(address).await.unwrap();
        network::write_message(
            &mut rejected,
            &Message::Hello {
                version,
                key: bad_key,
            },
        )
        .await
        .unwrap();
        assert!(
            timeout(Duration::from_secs(4), network::read_message(&mut rejected))
                .await
                .unwrap()
                .is_err()
        );
        assert!(host_rx.try_recv().is_err());
    }
    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::guest(address, key, guest_tx, Instant::now()));
    let host_out = connected(&mut host_rx).await;
    let guest_out = connected(&mut guest_rx).await;
    for n in 0..20 {
        guest_out
            .send(Message::Request(Control::Seek(n as f64)))
            .await
            .unwrap();
        match next(&mut host_rx).await {
            Event::Message(message, _) => {
                let Message::Request(Control::Seek(position)) = *message else {
                    panic!("unexpected message")
                };
                assert_eq!(position, n as f64)
            }
            _ => panic!("unexpected host event"),
        }
        host_out
            .send(Message::Pong {
                sent_ms: n as f64,
                received_ms: n as f64 + 100.0,
                sent_host_ms: n as f64 + 100.0,
            })
            .await
            .unwrap();
        assert!(matches!(
            next(&mut guest_rx).await,
            Event::Message(message, _) if matches!(*message, Message::Pong { .. })
        ));
    }
    drop(host_out);
    assert!(matches!(next(&mut host_rx).await, Event::Disconnected));
    assert!(matches!(next(&mut guest_rx).await, Event::Disconnected));
    let _reconnected_host = connected(&mut host_rx).await;
    let _reconnected_guest = connected(&mut guest_rx).await;
    host.abort();
    guest.abort();
}

#[tokio::test]
async fn approved_peers_exchange_file_names_and_verification_progress() {
    use sync2gether::protocol::{Presence, Verification};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::rooms::host(
        listener,
        "host".into(),
        tx,
        Instant::now(),
    ));
    let (tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::rooms::guest(address, tx, Instant::now()));
    assert!(matches!(next(&mut guest_rx).await, Event::WaitingApproval));
    let Event::ApprovalRequested { reply, .. } = next(&mut host_rx).await else {
        panic!("missing approval")
    };
    reply.send(true).unwrap();
    let host_out = connected(&mut host_rx).await;
    let guest_out = connected(&mut guest_rx).await;
    let presence = Presence {
        file_name: Some("Filme.1080p.mkv".into()),
        loading: true,
        verification: Some(Verification {
            bytes: 42,
            total: 100,
        }),
        ..Default::default()
    };
    guest_out
        .send(Message::Presence(presence.clone()))
        .await
        .unwrap();
    let Event::Message(message, _) = next(&mut host_rx).await else {
        panic!("missing presence")
    };
    let Message::Presence(received) = *message else {
        panic!("unexpected message")
    };
    assert!(received.valid());
    assert_eq!(received.file_name, presence.file_name);
    assert_eq!(received.verification, presence.verification);
    let mut state = sync2gether::session::Session::default().state;
    state.host = Presence {
        file_name: Some("Outro.nome.mkv".into()),
        ..presence
    };
    state.guest = received;
    host_out
        .send(Message::State(Box::new(state)))
        .await
        .unwrap();
    let Event::Message(message, _) = next(&mut guest_rx).await else {
        panic!("missing state")
    };
    let Message::State(state) = *message else {
        panic!("unexpected message")
    };
    assert!(state.valid());
    assert_eq!(state.host.file_name.as_deref(), Some("Outro.nome.mkv"));
    assert_eq!(state.guest.file_name.as_deref(), Some("Filme.1080p.mkv"));
    host.abort();
    guest.abort();
}

#[tokio::test]
async fn heartbeat_timeout_detects_a_silent_peer() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, mut rx) = mpsc::channel(8);
    let key = "a".repeat(32);
    let host = tokio::spawn(network::host(listener, key.clone(), tx, Instant::now()));
    let mut silent = TcpStream::connect(address).await.unwrap();
    network::write_message(
        &mut silent,
        &Message::Hello {
            version: VERSION,
            key,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        network::read_message(&mut silent).await.unwrap(),
        Message::Welcome { .. }
    ));
    let _keep_outgoing_alive = connected(&mut rx).await;
    assert!(matches!(next(&mut rx).await, Event::Disconnected));
    host.abort();
}

async fn request_entry(
    address: std::net::SocketAddr,
    rx: &mut mpsc::Receiver<Event>,
) -> (TcpStream, u64, tokio::sync::oneshot::Sender<bool>) {
    let mut stream = TcpStream::connect(address).await.unwrap();
    network::write_message(
        &mut stream,
        &Message::JoinRequest {
            version: VERSION,
            name: "untrusted-name".into(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        network::read_message(&mut stream).await.unwrap(),
        Message::Pending
    ));
    match next(rx).await {
        Event::ApprovalRequested {
            id, name, reply, ..
        } => {
            assert_ne!(name, "untrusted-name");
            (stream, id, reply)
        }
        _ => panic!("approval expected"),
    }
}

#[tokio::test]
async fn discovered_rooms_require_approval_and_reconnect_with_private_tokens() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let host = tokio::spawn(network::rooms::host(
        listener,
        "host-device".into(),
        tx,
        Instant::now(),
    ));
    assert_eq!(
        network::rooms::probe(address).await.as_deref(),
        Some("host-device")
    );
    assert!(
        rx.try_recv().is_err(),
        "discovery must not join or interrupt a room"
    );
    let (mut stream, id, reply) = request_entry(address, &mut rx).await;
    assert!(
        network::rooms::probe(address).await.is_none(),
        "pending room has no free slot"
    );
    assert!(
        timeout(
            Duration::from_millis(50),
            network::read_message(&mut stream)
        )
        .await
        .is_err(),
        "no acceptance before host approval"
    );
    let mut third = TcpStream::connect(address).await.unwrap();
    network::write_message(
        &mut third,
        &Message::JoinRequest {
            version: VERSION,
            name: "third".into(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        network::read_message(&mut third).await.unwrap(),
        Message::Busy
    ));
    reply.send(true).unwrap();
    assert!(
        matches!(next(&mut rx).await, Event::ApprovalFinished { id: finished } if finished == id)
    );
    let Message::Approved {
        version: VERSION,
        token,
    } = network::read_message(&mut stream).await.unwrap()
    else {
        panic!("expected private token");
    };
    assert_eq!(token.len(), 32);
    let keep = connected(&mut rx).await;
    assert!(network::rooms::probe(address).await.is_none());
    drop(stream);
    assert!(matches!(next(&mut rx).await, Event::Disconnected));
    drop(keep);
    let mut stream = TcpStream::connect(address).await.unwrap();
    network::write_message(
        &mut stream,
        &Message::Hello {
            version: VERSION,
            key: token,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        network::read_message(&mut stream).await.unwrap(),
        Message::Welcome { version: VERSION }
    ));
    let keep = connected(&mut rx).await;
    drop(stream);
    assert!(matches!(next(&mut rx).await, Event::Disconnected));
    drop(keep);
    // Um token inventado nunca abre a sessão.
    let mut unauthorized = TcpStream::connect(address).await.unwrap();
    network::write_message(
        &mut unauthorized,
        &Message::Hello {
            version: VERSION,
            key: "0".repeat(32),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        network::read_message(&mut unauthorized).await.unwrap(),
        Message::Rejected
    ));
    host.abort();
}

#[tokio::test]
async fn rejection_cancellation_and_room_shutdown_clear_pending_requests() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let host = tokio::spawn(network::rooms::host(
        listener,
        "host".into(),
        tx,
        Instant::now(),
    ));
    let (mut stream, id, reply) = request_entry(address, &mut rx).await;
    reply.send(false).unwrap();
    assert!(matches!(
        network::read_message(&mut stream).await.unwrap(),
        Message::Rejected
    ));
    assert!(
        matches!(next(&mut rx).await, Event::ApprovalFinished { id: finished } if finished == id)
    );
    let (stream, second_id, second_reply) = request_entry(address, &mut rx).await;
    assert_ne!(id, second_id);
    drop(stream);
    assert!(matches!(next(&mut rx).await, Event::ApprovalFinished { id } if id == second_id));
    assert!(
        second_reply.send(true).is_err(),
        "cancelled request cannot be approved later"
    );
    let (mut stream, _, reply) = request_entry(address, &mut rx).await;
    host.abort();
    let _ = host.await;
    assert!(reply.send(true).is_err());
    assert!(
        timeout(Duration::from_secs(1), network::read_message(&mut stream))
            .await
            .unwrap()
            .is_err()
    );
    assert!(network::rooms::probe(address).await.is_none());
}

#[tokio::test]
async fn approved_guest_reconnects_without_another_approval() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let host = tokio::spawn(network::rooms::host(
        listener,
        "host".into(),
        tx,
        Instant::now(),
    ));
    let (tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::rooms::guest(address, tx, Instant::now()));
    assert!(matches!(next(&mut guest_rx).await, Event::WaitingApproval));
    let Event::ApprovalRequested { reply, .. } = next(&mut rx).await else {
        panic!("approval missing");
    };
    reply.send(true).unwrap();
    let host_out = connected(&mut rx).await;
    let _guest_out = connected(&mut guest_rx).await;
    drop(host_out);
    assert!(matches!(next(&mut rx).await, Event::Disconnected));
    assert!(matches!(next(&mut guest_rx).await, Event::Disconnected));
    // connected() ignoraria pedidos: verifique que o primeiro evento já é a conexão.
    assert!(matches!(next(&mut rx).await, Event::Connected(_)));
    assert!(matches!(next(&mut guest_rx).await, Event::Connected(_)));
    host.abort();
    guest.abort();
}

#[tokio::test]
async fn room_search_ignores_unrelated_services_and_incompatible_versions() {
    use sync2gether::discovery::{Room, find_rooms};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, _rx) = mpsc::channel(64);
    let host = tokio::spawn(network::rooms::host(
        listener,
        "host".into(),
        tx,
        Instant::now(),
    ));
    let unrelated = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let wrong_address = unrelated.local_addr().unwrap();
    let wrong = tokio::spawn(async move {
        let (mut stream, _) = unrelated.accept().await.unwrap();
        let _ = network::read_message(&mut stream).await;
        network::write_message(
            &mut stream,
            &Message::RoomInfo {
                version: VERSION + 1,
                name: "fake".into(),
                available: true,
            },
        )
        .await
        .unwrap();
    });
    let rooms = find_rooms(vec![
        Room {
            name: "known-device".into(),
            address,
        },
        Room {
            name: "other".into(),
            address: wrong_address,
        },
    ])
    .await;
    assert_eq!(
        rooms,
        vec![Room {
            name: "host".into(),
            address
        }]
    );
    host.abort();
    wrong.await.unwrap();
}

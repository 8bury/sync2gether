//! Descoberta com sockets UDP multicast e TCP reais, sem VPN.
use std::{
    net::{Ipv4Addr, UdpSocket},
    time::{Duration, Instant},
};
use sync2gether::{
    discovery::Lan,
    network,
    protocol::{Message, VERSION},
};
use tokio::{net::TcpListener, sync::mpsc};

fn pair() -> (Lan, Lan) {
    let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let interfaces = [(Ipv4Addr::LOCALHOST, None)];
    (
        Lan::on_interfaces(port, &interfaces).unwrap(),
        Lan::on_interfaces(port, &interfaces).unwrap(),
    )
}

#[tokio::test]
async fn multicast_finds_custom_ports_and_hides_pending_and_closed_rooms() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let host = tokio::spawn(network::rooms::host(
        listener,
        "Cinema local".into(),
        tx,
        Instant::now(),
    ));
    let (announcer, mut browser) = pair();
    announcer
        .announce("Nome não confirmado", address.port(), true)
        .await;
    let rooms = browser.scan(Duration::from_millis(150)).await;
    assert_eq!(rooms.len(), 1);
    assert_eq!(rooms[0].address, address);
    assert_eq!(
        rooms[0].name, "Cinema local",
        "TCP confirms the advertised service and name"
    );
    assert!(rx.try_recv().is_err(), "discovery never requests entry");

    let mut guest = tokio::net::TcpStream::connect(address).await.unwrap();
    network::write_message(
        &mut guest,
        &Message::JoinRequest {
            version: VERSION,
            name: "ignored".into(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        network::read_message(&mut guest).await.unwrap(),
        Message::Pending
    ));
    let approval = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(approval, network::Event::ApprovalRequested { .. }));
    // Mesmo com anúncio antigo de vaga, a confirmação TCP elimina a sala ocupada.
    assert!(browser.scan(Duration::from_millis(100)).await.is_empty());
    drop(approval);
    drop(guest);
    host.abort();
    let _ = host.await;
    assert!(browser.scan(Duration::from_millis(100)).await.is_empty());
}

#[tokio::test]
async fn unavailable_and_expired_announcements_are_removed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, _rx) = mpsc::channel(64);
    let host = tokio::spawn(network::rooms::host(
        listener,
        "host".into(),
        tx,
        Instant::now(),
    ));
    let (announcer, mut browser) = pair();
    announcer.announce("host", port, true).await;
    assert_eq!(browser.scan(Duration::from_millis(100)).await.len(), 1);
    announcer.announce("host", port, false).await;
    assert!(browser.scan(Duration::from_millis(100)).await.is_empty());
    announcer.announce("host", port, true).await;
    assert_eq!(browser.scan(Duration::from_millis(100)).await.len(), 1);
    // O TCP continua disponível; a remoção depende da expiração do anúncio.
    assert!(browser.scan(Duration::from_millis(5200)).await.is_empty());
    host.abort();
}

#[tokio::test]
async fn one_room_on_multiple_interfaces_appears_once() {
    let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, _rx) = mpsc::channel(64);
    let host = tokio::spawn(network::rooms::host(
        listener,
        "host".into(),
        tx,
        Instant::now(),
    ));
    let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let announcer = Lan::on_interfaces(
        udp_port,
        &[
            (Ipv4Addr::LOCALHOST, None),
            (Ipv4Addr::new(127, 0, 0, 2), None),
        ],
    )
    .unwrap();
    let mut browser = Lan::on_interfaces(udp_port, &[(Ipv4Addr::LOCALHOST, None)]).unwrap();
    announcer.announce("host", port, true).await;
    assert_eq!(browser.scan(Duration::from_millis(100)).await.len(), 1);
    host.abort();
}

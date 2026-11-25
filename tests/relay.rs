use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};
use sync2gether::{
    network::{
        self, Event,
        relay::{ClientTls, Code, MAX_WRONG_CODES},
    },
    protocol::{Control, Message},
};
use sync2gether_relay::{Limits, Reply, Request, VERSION};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinSet,
    time::timeout,
};

struct Relay(Option<tokio::runtime::Runtime>);
impl Relay {
    fn stop(mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}
fn start_relay(address: SocketAddr) -> Relay {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let listener = bind_retrying(address);
    runtime.spawn(async move {
        let listener = TcpListener::from_std(listener).unwrap();
        sync2gether_relay::serve(listener, Limits::default()).await
    });
    Relay(Some(runtime))
}
fn bind_retrying(address: SocketAddr) -> std::net::TcpListener {
    let mut attempts = 0;
    let listener = loop {
        match std::net::TcpListener::bind(address) {
            Ok(listener) => break listener,
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse && attempts < 40 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(err) => panic!("failed to bind {address}: {err}"),
        }
    };
    listener.set_nonblocking(true).unwrap();
    listener
}
fn free_address() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

async fn next(rx: &mut mpsc::Receiver<Event>) -> Event {
    timeout(Duration::from_secs(20), rx.recv())
        .await
        .expect("evento não chegou a tempo")
        .expect("tarefa de rede encerrada")
}
async fn room_code(rx: &mut mpsc::Receiver<Event>) -> String {
    loop {
        if let Event::RoomCode(code) = next(rx).await {
            return code;
        }
    }
}
async fn connected(rx: &mut mpsc::Receiver<Event>) -> (mpsc::Sender<Message>, Option<String>) {
    let mut name = None;
    loop {
        match next(rx).await {
            Event::PeerName(peer) => name = Some(peer),
            Event::Connected(tx) => return (tx, name),
            Event::Rejected(error) => panic!("recusado: {error}"),
            _ => {}
        }
    }
}
async fn rejected(rx: &mut mpsc::Receiver<Event>) -> String {
    loop {
        match next(rx).await {
            Event::Rejected(error) => return error,
            Event::Connected(_) => panic!("conectou sem o código certo"),
            _ => {}
        }
    }
}
async fn exchanged(from: &mpsc::Sender<Message>, to: &mut mpsc::Receiver<Event>, position: f64) {
    from.send(Message::Request(Control::Seek(position)))
        .await
        .unwrap();
    loop {
        if let Event::Message(message, _) = next(to).await {
            assert!(matches!(*message, Message::Request(Control::Seek(p)) if p == position));
            return;
        }
    }
}
fn wrong_words(code: &str) -> String {
    let (room, words) = code.split_once('-').unwrap();
    let replacement = if words.starts_with("gato-") {
        "lua-lua"
    } else {
        "gato-gato"
    };
    format!("{room}-{replacement}")
}

struct Link {
    address: SocketAddr,
    connections: std::sync::Arc<tokio::sync::Mutex<JoinSet<()>>>,
    _task: tokio::task::JoinHandle<()>,
}
impl Link {
    async fn new(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let connections = std::sync::Arc::new(tokio::sync::Mutex::new(JoinSet::new()));
        let tracked = connections.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                tracked.lock().await.spawn(async move {
                    if let Ok(mut outbound) = TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                });
            }
        });
        Self {
            address,
            connections,
            _task: task,
        }
    }
    async fn cut(&self) {
        self.connections.lock().await.abort_all();
    }
}

#[tokio::test]
async fn code_opens_an_encrypted_room_that_survives_a_dropped_link() {
    let relay = free_address();
    let _relay = start_relay(relay);
    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host(
        relay.to_string(),
        host_tx,
        Instant::now(),
    ));
    let code = room_code(&mut host_rx).await;
    let parsed = Code::parse(&code).expect("código no formato documentado");
    assert_eq!(parsed.to_string(), code);

    let link = Link::new(relay).await;
    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::relay::guest(
        link.address.to_string(),
        parsed,
        guest_tx,
        Instant::now(),
    ));
    let (host_out, guest_name) = connected(&mut host_rx).await;
    let (guest_out, host_name) = connected(&mut guest_rx).await;
    let local = sync2gether::discovery::local_name();
    assert_eq!(guest_name.as_deref(), Some(local.as_str()));
    assert_eq!(host_name.as_deref(), Some(local.as_str()));
    for n in 0..10 {
        exchanged(&guest_out, &mut host_rx, n as f64).await;
        exchanged(&host_out, &mut guest_rx, n as f64 + 0.5).await;
    }

    link.cut().await;
    let (host_out, _) = connected(&mut host_rx).await;
    let (guest_out, _) = connected(&mut guest_rx).await;
    exchanged(&guest_out, &mut host_rx, 42.0).await;
    exchanged(&host_out, &mut guest_rx, 43.0).await;
    host.abort();
    guest.abort();
}

#[tokio::test]
async fn wrong_codes_are_refused_and_the_third_closes_the_room() {
    let relay = free_address();
    let _relay = start_relay(relay);
    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host(
        relay.to_string(),
        host_tx,
        Instant::now(),
    ));
    let code = room_code(&mut host_rx).await;
    for attempt in 1..=MAX_WRONG_CODES {
        let (guest_tx, mut guest_rx) = mpsc::channel(64);
        let guest = tokio::spawn(network::relay::guest(
            relay.to_string(),
            Code::parse(&wrong_words(&code)).unwrap(),
            guest_tx,
            Instant::now(),
        ));
        assert!(rejected(&mut guest_rx).await.contains("Código incorreto"));
        if attempt < MAX_WRONG_CODES {
            loop {
                match next(&mut host_rx).await {
                    Event::WrongCode => break,
                    Event::Connected(_) => panic!("anfitrião aceitou código errado"),
                    _ => {}
                }
            }
        }
        guest.abort();
    }
    assert!(rejected(&mut host_rx).await.contains("três tentativas"));
    host.abort();
}

#[tokio::test]
async fn unknown_rooms_and_incompatible_versions_are_reported() {
    let relay = free_address();
    let _relay = start_relay(relay);
    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::relay::guest(
        relay.to_string(),
        Code::parse("9876-gato-lua").unwrap(),
        guest_tx,
        Instant::now(),
    ));
    assert!(rejected(&mut guest_rx).await.contains("Não há sala"));
    guest.abort();

    let mut stream = TcpStream::connect(relay).await.unwrap();
    sync2gether_relay::write_frame(
        &mut stream,
        &Request::Join {
            version: VERSION + 1,
            room: "9876".into(),
        },
    )
    .await
    .unwrap();
    let reply: Reply = sync2gether_relay::read_frame(&mut stream).await.unwrap();
    assert_eq!(reply, Reply::Outdated { version: VERSION });

    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host(
        relay.to_string(),
        host_tx,
        Instant::now(),
    ));
    let code = room_code(&mut host_rx).await;
    let room = Code::parse(&code).unwrap().room().to_owned();
    let mut intruder = TcpStream::connect(relay).await.unwrap();
    sync2gether_relay::write_frame(
        &mut intruder,
        &Request::Host {
            version: VERSION,
            room: Some(room.clone()),
            secret: "0".repeat(32),
        },
    )
    .await
    .unwrap();
    let reply: Reply = sync2gether_relay::read_frame(&mut intruder).await.unwrap();
    assert_eq!(reply, Reply::Taken);
    let mut intruder = TcpStream::connect(relay).await.unwrap();
    sync2gether_relay::write_frame(
        &mut intruder,
        &Request::Accept {
            version: VERSION,
            room,
            secret: "0".repeat(32),
            ticket: 1,
        },
    )
    .await
    .unwrap();
    let reply: Reply = sync2gether_relay::read_frame(&mut intruder).await.unwrap();
    assert_eq!(reply, Reply::Unknown);
    host.abort();
}

#[tokio::test]
async fn relay_restart_keeps_the_code_and_the_guest_resumes() {
    let relay = free_address();
    let first = start_relay(relay);
    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host(
        relay.to_string(),
        host_tx,
        Instant::now(),
    ));
    let code = room_code(&mut host_rx).await;
    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::relay::guest(
        relay.to_string(),
        Code::parse(&code).unwrap(),
        guest_tx,
        Instant::now(),
    ));
    connected(&mut host_rx).await;
    connected(&mut guest_rx).await;

    first.stop();
    let _second = start_relay(relay);
    let mut offline = false;
    loop {
        match next(&mut host_rx).await {
            Event::RelayOffline => offline = true,
            Event::RoomCode(again) => {
                assert_eq!(again, code, "o número da sala mudou após o reinício");
                break;
            }
            _ => {}
        }
    }
    assert!(offline);
    let (host_out, _) = connected(&mut host_rx).await;
    let (guest_out, _) = connected(&mut guest_rx).await;
    exchanged(&guest_out, &mut host_rx, 7.0).await;
    exchanged(&host_out, &mut guest_rx, 8.0).await;
    host.abort();
    guest.abort();
}

#[tokio::test]
async fn leaving_host_closes_the_room_for_new_entries() {
    let relay = free_address();
    let _relay = start_relay(relay);
    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host(
        relay.to_string(),
        host_tx,
        Instant::now(),
    ));
    let code = room_code(&mut host_rx).await;
    host.abort();
    let _ = host.await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::relay::guest(
        relay.to_string(),
        Code::parse(&code).unwrap(),
        guest_tx,
        Instant::now(),
    ));
    assert!(rejected(&mut guest_rx).await.contains("Não há sala"));
    guest.abort();
}

async fn left(rx: &mut mpsc::Receiver<Event>) {
    loop {
        match next(rx).await {
            Event::PeerLeft => return,
            Event::Connected(_) | Event::Disconnected => panic!("saída tratada como queda"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn leaving_is_announced_to_the_other_side() {
    let relay = free_address();
    let _relay = start_relay(relay);
    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host(
        relay.to_string(),
        host_tx,
        Instant::now(),
    ));
    let code = Code::parse(&room_code(&mut host_rx).await).unwrap();
    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::relay::guest(
        relay.to_string(),
        code.clone(),
        guest_tx,
        Instant::now(),
    ));
    let _host_out = connected(&mut host_rx).await;
    let (guest_out, _) = connected(&mut guest_rx).await;
    guest_out.send(Message::Bye).await.unwrap();
    left(&mut host_rx).await;
    timeout(Duration::from_secs(5), guest)
        .await
        .expect("quem saiu não tenta reconectar")
        .unwrap();

    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::relay::guest(
        relay.to_string(),
        code,
        guest_tx,
        Instant::now(),
    ));
    let (host_out, _) = connected(&mut host_rx).await;
    let _guest_out = connected(&mut guest_rx).await;
    host_out.send(Message::Bye).await.unwrap();
    left(&mut guest_rx).await;
    timeout(Duration::from_secs(5), guest)
        .await
        .expect("o convidado não tenta reconectar")
        .unwrap();
    host.abort();
}

struct TestPki {
    ca_pem: Vec<u8>,
    cert_pem: Vec<u8>,
    key_pem: Vec<u8>,
}

fn generate_pki(sans: &[&str]) -> TestPki {
    let mut ca_params = rcgen::CertificateParams::default();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = rcgen::Issuer::new(ca_params, ca_key);

    let leaf_params =
        rcgen::CertificateParams::new(sans.iter().map(|san| (*san).to_owned()).collect::<Vec<_>>())
            .unwrap();
    let leaf_key = rcgen::KeyPair::generate().unwrap();
    let leaf_cert = leaf_params.signed_by(&leaf_key, &issuer).unwrap();

    TestPki {
        ca_pem: ca_cert.pem().into_bytes(),
        cert_pem: leaf_cert.pem().into_bytes(),
        key_pem: leaf_key.serialize_pem().into_bytes(),
    }
}

fn start_tls_relay(address: SocketAddr, cert_pem: &[u8], key_pem: &[u8]) -> Relay {
    let acceptor = sync2gether_relay::tls::acceptor_from_pem(cert_pem, key_pem).unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let listener = bind_retrying(address);
    runtime.spawn(async move {
        let listener = TcpListener::from_std(listener).unwrap();
        sync2gether_relay::serve_tls(listener, Limits::default(), acceptor).await
    });
    Relay(Some(runtime))
}

#[tokio::test]
async fn tls_room_exchanges_encrypted_messages_and_survives_dropped_link_and_restart() {
    let pki = generate_pki(&["127.0.0.1", "localhost"]);
    let client_tls = ClientTls::from_pem(&pki.ca_pem).unwrap();
    let relay = free_address();
    let first = start_tls_relay(relay, &pki.cert_pem, &pki.key_pem);

    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host_with_tls(
        format!("tls://{relay}"),
        client_tls.clone(),
        host_tx,
        Instant::now(),
    ));
    let code = room_code(&mut host_rx).await;
    let parsed = Code::parse(&code).expect("código no formato documentado");

    let link = Link::new(relay).await;
    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::relay::guest_with_tls(
        format!("tls://{}", link.address),
        parsed,
        client_tls,
        guest_tx,
        Instant::now(),
    ));
    let (host_out, guest_name) = connected(&mut host_rx).await;
    let (guest_out, host_name) = connected(&mut guest_rx).await;
    let local = sync2gether::discovery::local_name();
    assert_eq!(guest_name.as_deref(), Some(local.as_str()));
    assert_eq!(host_name.as_deref(), Some(local.as_str()));
    for n in 0..10 {
        exchanged(&guest_out, &mut host_rx, n as f64).await;
        exchanged(&host_out, &mut guest_rx, n as f64 + 0.5).await;
    }

    link.cut().await;
    let (host_out, _) = connected(&mut host_rx).await;
    let (guest_out, _) = connected(&mut guest_rx).await;
    exchanged(&guest_out, &mut host_rx, 42.0).await;
    exchanged(&host_out, &mut guest_rx, 43.0).await;

    first.stop();
    let _second = start_tls_relay(relay, &pki.cert_pem, &pki.key_pem);
    let mut offline = false;
    loop {
        match next(&mut host_rx).await {
            Event::RelayOffline => offline = true,
            Event::RoomCode(again) => {
                assert_eq!(again, code, "o número da sala mudou após o reinício TLS");
                break;
            }
            _ => {}
        }
    }
    assert!(offline);
    let (host_out, _) = connected(&mut host_rx).await;
    let (guest_out, _) = connected(&mut guest_rx).await;
    exchanged(&guest_out, &mut host_rx, 77.0).await;
    exchanged(&host_out, &mut guest_rx, 78.0).await;
    host.abort();
    guest.abort();
}

#[tokio::test]
async fn tls_rejects_untrusted_certificate_and_wrong_hostname_without_downgrade() {
    let valid_pki = generate_pki(&["127.0.0.1", "localhost"]);
    let untrusted_relay = free_address();
    let _untrusted_server =
        start_tls_relay(untrusted_relay, &valid_pki.cert_pem, &valid_pki.key_pem);

    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host(
        format!("tls://{untrusted_relay}"),
        host_tx,
        Instant::now(),
    ));
    assert!(rejected(&mut host_rx).await.contains("certificado"));
    host.abort();

    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::relay::guest(
        format!("tls://{untrusted_relay}"),
        Code::parse("4821-gato-lua").unwrap(),
        guest_tx,
        Instant::now(),
    ));
    assert!(rejected(&mut guest_rx).await.contains("certificado"));
    guest.abort();

    let wrong_host_pki = generate_pki(&["outro-relay.exemplo.invalid"]);
    let wrong_host_tls = ClientTls::from_pem(&wrong_host_pki.ca_pem).unwrap();
    let wrong_host_relay = free_address();
    let _wrong_host_server = start_tls_relay(
        wrong_host_relay,
        &wrong_host_pki.cert_pem,
        &wrong_host_pki.key_pem,
    );

    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host_with_tls(
        format!("tls://{wrong_host_relay}"),
        wrong_host_tls.clone(),
        host_tx,
        Instant::now(),
    ));
    assert!(rejected(&mut host_rx).await.contains("certificado"));
    host.abort();

    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::relay::guest_with_tls(
        format!("tls://{wrong_host_relay}"),
        Code::parse("4821-gato-lua").unwrap(),
        wrong_host_tls.clone(),
        guest_tx,
        Instant::now(),
    ));
    assert!(rejected(&mut guest_rx).await.contains("certificado"));
    guest.abort();

    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host_with_tls(
        format!("tls://localhost:{}", wrong_host_relay.port()),
        wrong_host_tls,
        host_tx,
        Instant::now(),
    ));
    assert!(rejected(&mut host_rx).await.contains("certificado"));
    host.abort();

    let plain_relay = free_address();
    let _plain_server = start_relay(plain_relay);

    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host_with_tls(
        format!("tls://{plain_relay}"),
        ClientTls::from_pem(&valid_pki.ca_pem).unwrap(),
        host_tx,
        Instant::now(),
    ));
    match next(&mut host_rx).await {
        Event::RelayOffline => {}
        Event::RoomCode(_) => panic!("cliente TLS fez downgrade silencioso para TCP puro"),
        Event::Rejected(_) => {}
        _ => {}
    }
    assert!(
        timeout(Duration::from_millis(500), host_rx.recv())
            .await
            .is_err()
            || !matches!(host_rx.try_recv(), Ok(Event::RoomCode(_)))
    );
    host.abort();

    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host(
        format!("tcp://{plain_relay}"),
        host_tx,
        Instant::now(),
    ));
    let _ = room_code(&mut host_rx).await;
    host.abort();
}

#[tokio::test]
#[ignore = "usa a internet e um relay publicado"]
async fn published_relay_pairs_two_apps() {
    let relay = std::env::var("SYNC2GETHER_RELAY_E2E").expect("defina SYNC2GETHER_RELAY_E2E");
    let (host_tx, mut host_rx) = mpsc::channel(64);
    let host = tokio::spawn(network::relay::host(relay.clone(), host_tx, Instant::now()));
    let code = room_code(&mut host_rx).await;
    let started = Instant::now();
    let (guest_tx, mut guest_rx) = mpsc::channel(64);
    let guest = tokio::spawn(network::relay::guest(
        relay,
        Code::parse(&code).unwrap(),
        guest_tx,
        Instant::now(),
    ));
    let (host_out, _) = connected(&mut host_rx).await;
    let (guest_out, _) = connected(&mut guest_rx).await;
    let joined = started.elapsed();
    let exchange = Instant::now();
    for n in 0..20 {
        exchanged(&guest_out, &mut host_rx, n as f64).await;
        exchanged(&host_out, &mut guest_rx, n as f64).await;
    }
    eprintln!(
        "entrada em {} ms; ida e volta média {} ms",
        joined.as_millis(),
        exchange.elapsed().as_millis() / 20
    );
    host.abort();
    guest.abort();
}

//! Descoberta e entrada aprovada pelo anfitrião. Tokens ficam apenas na memória.
use super::{Event, allowed_address, connection, read_message, write_message};
use crate::{
    discovery::{Lan, device_name},
    protocol::{Message, VERSION},
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Semaphore, mpsc, oneshot},
    task::JoinSet,
    time::{sleep, timeout},
};

struct HostState {
    name: String,
    slot: Arc<Semaphore>,
    authorized: Mutex<Option<(IpAddr, String)>>,
    next_id: AtomicU64,
    events: mpsc::Sender<Event>,
    origin: Instant,
}

pub async fn host(
    listener: TcpListener,
    name: String,
    events: mpsc::Sender<Event>,
    origin: Instant,
) {
    let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0);
    let state = Arc::new(HostState {
        name: device_name(&name),
        slot: Arc::new(Semaphore::new(1)),
        authorized: Mutex::new(None),
        next_id: AtomicU64::new(rand::random()),
        events,
        origin,
    });
    // JoinSet aborta conexões e solicitações pendentes quando a sala é fechada.
    let mut tasks = JoinSet::new();
    let advertised = state.clone();
    tasks.spawn(async move {
        if let Ok(lan) = Lan::open() {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                interval.tick().await;
                lan.announce(
                    &advertised.name,
                    port,
                    advertised.slot.available_permits() > 0,
                )
                .await;
            }
        } else {
            let _ = advertised.events.send(Event::DiscoveryUnavailable).await;
        }
    });
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, address)) = accepted else { break; };
                if !allowed_address(address) || tasks.len() >= 32 { continue; }
                let state = state.clone();
                tasks.spawn(async move { let _ = handle(stream, address, state).await; });
            }
            _ = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
}
async fn handle(
    mut stream: TcpStream,
    address: SocketAddr,
    state: Arc<HostState>,
) -> std::io::Result<()> {
    let hello = timeout(Duration::from_secs(3), read_message(&mut stream)).await??;
    if let Message::Discover { version: VERSION } = hello {
        return timeout(
            Duration::from_secs(2),
            write_message(
                &mut stream,
                &Message::RoomInfo {
                    version: VERSION,
                    name: state.name.clone(),
                    available: state.slot.available_permits() > 0,
                },
            ),
        )
        .await?;
    }
    let Ok(_slot) = state.slot.clone().try_acquire_owned() else {
        return timeout(
            Duration::from_secs(2),
            write_message(&mut stream, &Message::Busy),
        )
        .await?;
    };
    match hello {
        Message::JoinRequest {
            version: VERSION, ..
        } => {
            let id = state.next_id.fetch_add(1, Ordering::Relaxed);
            let name = address.ip().to_string();
            let (reply, decision) = oneshot::channel();
            timeout(
                Duration::from_secs(2),
                write_message(&mut stream, &Message::Pending),
            )
            .await??;
            if state
                .events
                .send(Event::ApprovalRequested {
                    id,
                    name,
                    address,
                    reply,
                })
                .await
                .is_err()
            {
                return Ok(());
            }
            // EOF ou qualquer mensagem inesperada cancela a solicitação antes da aprovação.
            let approved = tokio::select! {
                result = timeout(Duration::from_secs(60), decision) => matches!(result, Ok(Ok(true))),
                _ = read_message(&mut stream) => false,
            };
            let _ = state.events.send(Event::ApprovalFinished { id }).await;
            if !approved {
                return timeout(
                    Duration::from_secs(2),
                    write_message(&mut stream, &Message::Rejected),
                )
                .await?;
            }
            let token = format!("{:032x}", rand::random::<u128>());
            *state.authorized.lock().unwrap() = Some((address.ip(), token.clone()));
            timeout(
                Duration::from_secs(2),
                write_message(
                    &mut stream,
                    &Message::Approved {
                        version: VERSION,
                        token,
                    },
                ),
            )
            .await??;
        }
        Message::Hello {
            version: VERSION,
            key,
        } => {
            let authorized = state
                .authorized
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|(ip, token)| *ip == address.ip() && *token == key);
            if !authorized {
                return timeout(
                    Duration::from_secs(2),
                    write_message(&mut stream, &Message::Rejected),
                )
                .await?;
            }
            timeout(
                Duration::from_secs(2),
                write_message(&mut stream, &Message::Welcome { version: VERSION }),
            )
            .await??;
        }
        _ => {
            return timeout(
                Duration::from_secs(2),
                write_message(&mut stream, &Message::Rejected),
            )
            .await?;
        }
    }
    connection(stream, &state.events, state.origin).await;
    Ok(())
}
pub async fn probe(address: SocketAddr) -> Option<String> {
    if !allowed_address(address) {
        return None;
    }
    timeout(Duration::from_millis(800), async {
        let mut stream = TcpStream::connect(address).await.ok()?;
        write_message(&mut stream, &Message::Discover { version: VERSION })
            .await
            .ok()?;
        match read_message(&mut stream).await.ok()? {
            Message::RoomInfo {
                version: VERSION,
                name,
                available: true,
            } => Some(device_name(&name)),
            _ => None,
        }
    })
    .await
    .ok()
    .flatten()
}
pub async fn guest(address: SocketAddr, events: mpsc::Sender<Event>, origin: Instant) {
    let result = request(address, &events).await;
    let (stream, token) = match result {
        Ok(Some(accepted)) => accepted,
        Ok(None) => {
            let _ = events
                .send(Event::Rejected(
                    "A solicitação foi recusada, expirou ou a sala está ocupada.".into(),
                ))
                .await;
            return;
        }
        Err(_) => {
            let _ = events.send(Event::Rejected("Não foi possível entrar. Confira o endereço, a conexão e a versão dos aplicativos.".into())).await;
            return;
        }
    };
    connection(stream, &events, origin).await;
    // Só participantes já aprovados reconectam automaticamente.
    loop {
        sleep(Duration::from_secs(1)).await;
        let result = timeout(Duration::from_secs(3), async {
            let mut stream = TcpStream::connect(address).await?;
            write_message(
                &mut stream,
                &Message::Hello {
                    version: VERSION,
                    key: token.clone(),
                },
            )
            .await?;
            let welcome = read_message(&mut stream).await?;
            Ok::<_, std::io::Error>((stream, welcome))
        })
        .await;
        match result {
            Ok(Ok((stream, Message::Welcome { version: VERSION }))) => {
                connection(stream, &events, origin).await
            }
            Ok(Ok((_, Message::Rejected))) => {
                let _ = events
                    .send(Event::Rejected(
                        "A sala foi encerrada ou está ocupada. Solicite entrada novamente.".into(),
                    ))
                    .await;
                return;
            }
            _ => {
                let _ = events.send(Event::Disconnected).await;
            }
        }
    }
}
async fn request(
    address: SocketAddr,
    events: &mpsc::Sender<Event>,
) -> std::io::Result<Option<(TcpStream, String)>> {
    let mut stream = timeout(Duration::from_secs(3), TcpStream::connect(address)).await??;
    timeout(
        Duration::from_secs(3),
        write_message(
            &mut stream,
            &Message::JoinRequest {
                version: VERSION,
                name: String::new(),
            },
        ),
    )
    .await??;
    if !matches!(
        timeout(Duration::from_secs(3), read_message(&mut stream)).await??,
        Message::Pending
    ) {
        return Ok(None);
    }
    let _ = events.send(Event::WaitingApproval).await;
    match timeout(Duration::from_secs(65), read_message(&mut stream)).await?? {
        Message::Approved {
            version: VERSION,
            token,
        } if token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(Some((stream, token)))
        }
        _ => Ok(None),
    }
}

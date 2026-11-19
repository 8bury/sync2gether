use super::{CODE_HINT, JoinRequest, Role, Worker, parse_address};
use crate::{discovery, network, protocol::Message, session::Session, sync::Clock};
use std::future::Future;
use tokio::sync::mpsc;

pub(super) enum BackgroundResult {
    Host(Result<(tokio::net::TcpListener, String), String>),
    Rooms(Result<Vec<discovery::Room>, String>),
}

impl Worker {
    pub(super) fn cancel_background(&mut self) {
        self.background_id += 1;
        if let Some(task) = self.background.take() {
            task.abort();
        }
        self.view.searching = false;
        self.view.hosting = false;
    }

    pub(super) fn background(&mut self, port: Option<u16>) {
        self.cancel_background();
        self.view.error = None;
        self.view.discovery_error = None;
        self.view.hosting = port.is_some();
        self.view.searching = port.is_none();
        self.view.rooms.clear();
        let tx = self.background_tx.clone();
        let id = self.background_id;
        self.background = Some(tokio::spawn(async move {
            if let Some(port) = port {
                let result = if port == 0 {
                    Err("Escolha uma porta entre 1 e 65535.".into())
                } else {
                    tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port))
                        .await
                        .map(|listener| (listener, discovery::local_name()))
                        .map_err(|_| {
                            "Não foi possível abrir a sala. Verifique se a porta está livre.".into()
                        })
                };
                let _ = tx.send((id, BackgroundResult::Host(result))).await;
            } else {
                match discovery::Lan::open() {
                    Ok(mut lan) => loop {
                        let rooms = lan.scan(std::time::Duration::from_secs(2)).await;
                        if tx
                            .send((id, BackgroundResult::Rooms(Ok(rooms))))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    },
                    Err(_) => {
                        let _ = tx
                            .send((
                                id,
                                BackgroundResult::Rooms(Err(
                                    "Não foi possível buscar na rede. Você pode conectar por endereço."
                                        .into(),
                                )),
                            ))
                            .await;
                    }
                }
            }
        }));
    }

    pub(super) fn leave(&mut self) {
        self.cancel_background();
        self.cancel_open();
        self.approval = None;
        self.view.join_request = None;
        self.view.waiting_approval = false;
        self.view.peer_name = None;
        self.view.room_address = None;
        self.view.room_addresses.clear();
        self.view.watching = false;
        if let Some(task) = self.network.take() {
            task.abort();
        }
        self.net_rx = None;
        self.disconnected();
        self.session = Session::default();
        self.view.role = Role::Local;
        self.view.room_key.clear();
        self.view.internet = false;
        self.view.room_code = None;
        self.view.relay_offline = false;
        self.view.error = None;
        self.view.notice = None;
    }

    pub(super) fn approve(&mut self, id: u64, accept: bool) {
        if self
            .approval
            .as_ref()
            .is_some_and(|(pending, _)| *pending == id)
            && let Some((_, reply)) = self.approval.take()
        {
            if accept {
                self.view.peer_name = self.view.join_request.as_ref().map(|r| r.name.clone());
                self.view.watching = false;
            }
            let _ = reply.send(accept);
        }
    }

    fn begin_host(&mut self) {
        self.view.role = Role::Host;
        self.clock = Clock::host();
        self.session.state.position = self.view.player.position;
        self.session.state.host_ms = self.now();
    }

    fn spawn_network<F>(&mut self, task: impl FnOnce(mpsc::Sender<network::Event>) -> F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel(64);
        self.net_rx = Some(rx);
        self.network = Some(tokio::spawn(task(tx)));
    }

    pub(super) async fn host_room(&mut self, address: String) {
        self.leave();
        match parse_address(&address) {
            Ok(address) => match tokio::net::TcpListener::bind(address).await {
                Ok(listener) => {
                    self.view.room_address = Some(address);
                    self.begin_host();
                    let origin = self.origin;
                    self.spawn_network(|tx| {
                        network::rooms::host(listener, "sync2gether".into(), tx, origin)
                    });
                }
                Err(_) => {
                    self.view.error =
                        Some("Não foi possível abrir a sala nesse endereço e porta.".into());
                }
            },
            Err(error) => self.view.error = Some(error),
        }
    }

    pub(super) fn request_join(&mut self, address: String) {
        self.leave();
        match parse_address(&address) {
            Ok(address) => {
                self.view.role = Role::Guest;
                self.view.room_address = Some(address);
                self.view.peer_name = self
                    .view
                    .rooms
                    .iter()
                    .find(|r| r.address == address)
                    .map(|r| r.name.clone());
                let origin = self.origin;
                self.spawn_network(|tx| network::rooms::guest(address, tx, origin));
            }
            Err(error) => self.view.error = Some(error),
        }
    }

    pub(super) async fn host_direct(&mut self, address: String) {
        self.leave();
        match parse_address(&address) {
            Ok(address) => match tokio::net::TcpListener::bind(address).await {
                Ok(listener) => {
                    self.view.room_key = format!("{:032x}", rand::random::<u128>());
                    self.begin_host();
                    let key = self.view.room_key.clone();
                    let origin = self.origin;
                    self.spawn_network(|tx| network::host(listener, key, tx, origin));
                }
                Err(_) => {
                    self.view.error =
                        Some("Não foi possível abrir a sala nesse endereço e porta.".into());
                }
            },
            Err(error) => self.view.error = Some(error),
        }
    }

    pub(super) fn join_direct(&mut self, address: String, key: String) {
        self.leave();
        match parse_address(&address) {
            Ok(address) if key.len() == 32 && key.bytes().all(|b| b.is_ascii_hexdigit()) => {
                self.view.role = Role::Guest;
                let origin = self.origin;
                self.spawn_network(|tx| network::guest(address, key, tx, origin));
            }
            Ok(_) => {
                self.view.error =
                    Some("O código da sala deve ter 32 caracteres hexadecimais.".into());
            }
            Err(error) => self.view.error = Some(error),
        }
    }

    pub(super) fn host_internet(&mut self, relay: String) {
        self.leave();
        match network::relay::relay_address(&relay) {
            Ok(relay) => {
                self.view.internet = true;
                self.begin_host();
                let origin = self.origin;
                self.spawn_network(|tx| network::relay::host(relay, tx, origin));
            }
            Err(error) => self.view.error = Some(error),
        }
    }

    pub(super) fn join_code(&mut self, relay: String, code: String) {
        self.leave();
        match (
            network::relay::relay_address(&relay),
            network::relay::Code::parse(&code),
        ) {
            (Ok(relay), Some(code)) => {
                self.view.role = Role::Guest;
                self.view.internet = true;
                self.view.room_code = Some(code.to_string());
                let origin = self.origin;
                self.spawn_network(|tx| network::relay::guest(relay, code, tx, origin));
            }
            (Err(error), _) => self.view.error = Some(error),
            (_, None) => self.view.error = Some(CODE_HINT.into()),
        }
    }

    pub(super) fn background_event(&mut self, id: u64, result: BackgroundResult) {
        if id != self.background_id {
            return;
        }
        if matches!(
            &result,
            BackgroundResult::Host(_) | BackgroundResult::Rooms(Err(_))
        ) {
            self.background = None;
        }
        self.view.searching = false;
        self.view.hosting = false;
        match result {
            BackgroundResult::Rooms(Ok(rooms)) => self.view.rooms = rooms,
            BackgroundResult::Rooms(Err(error)) => self.view.discovery_error = Some(error),
            BackgroundResult::Host(Err(error)) => self.view.error = Some(error),
            BackgroundResult::Host(Ok((listener, net))) => {
                self.view.room_address = listener.local_addr().ok();
                self.view.room_addresses = discovery::local_addresses(
                    listener
                        .local_addr()
                        .map(|addr| addr.port())
                        .unwrap_or(crate::protocol::PORT),
                );
                self.begin_host();
                let origin = self.origin;
                self.spawn_network(|tx| network::rooms::host(listener, net, tx, origin));
            }
        }
    }

    pub(super) fn network_event(&mut self, event: network::Event) {
        match event {
            network::Event::ApprovalRequested {
                id,
                name,
                address,
                reply,
            } => {
                self.view.join_request = Some(JoinRequest { id, name, address });
                self.approval = Some((id, reply));
            }
            network::Event::ApprovalFinished { id } => {
                if self.view.join_request.as_ref().is_some_and(|r| r.id == id) {
                    self.view.join_request = None;
                    self.approval = None;
                }
            }
            network::Event::WaitingApproval => self.view.waiting_approval = true,
            network::Event::RoomCode(code) => {
                self.view.room_code = Some(code);
                self.view.relay_offline = false;
            }
            network::Event::RelayOffline => self.view.relay_offline = true,
            network::Event::WrongCode => {
                self.view.notice = Some(
                    "Alguém tentou entrar com um código errado. Confira o código enviado.".into(),
                )
            }
            network::Event::PeerName(name) => self.view.peer_name = Some(name),
            network::Event::Rejected(error) => {
                self.leave();
                self.view.error = Some(error);
            }
            network::Event::Connected(tx) => {
                self.view.waiting_approval = false;
                if self.view.watching {
                    self.view.notice =
                        Some("Conexão restabelecida. Aperte reproduzir para continuar.".into());
                }
                self.disconnected();
                self.outgoing = Some(tx);
                self.view.connected = true;
                self.view.error = None;
                self.session.connected = true;
                self.recover = self.view.role == Role::Host;
                if self.view.role == Role::Host {
                    self.clock = Clock::host();
                    self.publish();
                } else {
                    self.refresh_presence();
                    self.send(Message::Presence(self.presence.clone()));
                }
            }
            network::Event::Disconnected => self.disconnected(),
            network::Event::Error(error) => self.view.error = Some(error),
            network::Event::DiscoveryUnavailable => {
                self.view.discovery_error = Some(
                    "A sala está aberta, mas a descoberta não está disponível. Compartilhe um endereço nos detalhes da conexão."
                        .into(),
                )
            }
            network::Event::Message(message, received_at) => {
                let received_ms = received_at.duration_since(self.origin).as_secs_f64() * 1000.0;
                match (self.view.role, *message) {
                    (Role::Host, Message::Presence(p)) if p.valid() => {
                        self.session.state.guest = p;
                        self.progress();
                    }
                    (Role::Host, Message::Request(c)) => self.control(c),
                    (Role::Host, Message::Prepared { id }) => {
                        self.refresh_presence();
                        self.session.prepared(id, true, self.now());
                        self.publish();
                    }
                    (Role::Host, Message::Armed { id }) => {
                        self.session.armed(id, self.now());
                        self.publish();
                    }
                    (Role::Host, Message::Failed { id }) => {
                        self.session.fail(id, self.now());
                        self.publish();
                    }
                    (Role::Host, Message::Ping { sent_ms }) if sent_ms.is_finite() => {
                        self.send(Message::Pong {
                            sent_ms,
                            received_ms,
                            sent_host_ms: self.now(),
                        })
                    }
                    (Role::Guest, Message::State(state)) if state.valid() => {
                        if self.snapshot.as_ref().is_none_or(|old| {
                            state.revision >= old.revision && state.host_ms >= old.host_ms
                        }) {
                            self.apply_snapshot(*state);
                        }
                    }
                    (
                        Role::Guest,
                        Message::Pong {
                            sent_ms,
                            received_ms: t2,
                            sent_host_ms: t3,
                        },
                    ) => {
                        if let Some(index) = self
                            .pings
                            .iter()
                            .position(|sent| (*sent - sent_ms).abs() < 0.001)
                        {
                            self.pings.remove(index);
                            self.clock.sample(sent_ms, t2, t3, received_ms);
                            self.refresh_presence();
                        }
                    }
                    _ => self.disconnected(),
                }
            }
        }
    }
}

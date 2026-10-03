//! Descoberta IPv4 por multicast e broadcast, sem depender de um provedor de VPN.
use crate::{network, protocol::VERSION};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    net::{Ipv4Addr, SocketAddr},
    time::{Duration, Instant},
};
use tokio::{net::UdpSocket, task::JoinSet};

pub const DISCOVERY_PORT: u16 = 7841;
const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 78, 42);
const MAGIC: &str = "sync2gether-lan";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Room {
    pub name: String,
    pub address: SocketAddr,
}
pub fn device_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(64).collect();
    if name.trim().is_empty() {
        "Outro computador".into()
    } else {
        name
    }
}
pub fn local_name() -> String {
    device_name(
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .unwrap_or_else(|_| "sync2gether".into())
            .trim(),
    )
}
pub fn local_addresses(port: u16) -> Vec<SocketAddr> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|iface| SocketAddr::new(iface.ip(), port))
        .filter(|addr| {
            addr.is_ipv4() && !addr.ip().is_loopback() && network::allowed_address(*addr)
        })
        .collect()
}
#[derive(Serialize, Deserialize)]
struct Announcement {
    id: u64,
    magic: String,
    version: u32,
    name: String,
    port: u16,
    available: bool,
}
/// Socket e interfaces são criados apenas no trabalho em segundo plano.
pub struct Lan {
    receiver: UdpSocket,
    senders: Vec<(UdpSocket, Option<Ipv4Addr>)>,
    port: u16,
    id: u64,
    seen: BTreeMap<SocketAddr, (u64, String, Instant)>,
}
impl Lan {
    pub fn open() -> io::Result<Self> {
        let interfaces = if_addrs::get_if_addrs()?
            .into_iter()
            .filter_map(|iface| match iface.addr {
                if_addrs::IfAddr::V4(addr) => Some((addr.ip, addr.broadcast)),
                _ => None,
            })
            .collect::<Vec<_>>();
        Self::on_interfaces(DISCOVERY_PORT, &interfaces)
    }
    /// Permite isolar portas e interfaces nos testes de sockets reais.
    pub fn on_interfaces(
        port: u16,
        interfaces: &[(Ipv4Addr, Option<Ipv4Addr>)],
    ) -> io::Result<Self> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        #[cfg(unix)]
        socket.set_reuse_port(true)?;
        socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)).into())?;
        socket.set_nonblocking(true)?;
        let mut senders = Vec::new();
        for &(ip, broadcast) in interfaces {
            // Algumas redes virtuais aceitam broadcast sem multicast.
            let _ = socket.join_multicast_v4(&GROUP, &ip);
            let sender = (|| -> io::Result<UdpSocket> {
                let sender = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
                sender.bind(&SocketAddr::from((ip, 0)).into())?;
                let _ = sender.set_multicast_if_v4(&ip);
                sender.set_multicast_ttl_v4(1)?;
                sender.set_multicast_loop_v4(true)?;
                sender.set_broadcast(true)?;
                sender.set_nonblocking(true)?;
                UdpSocket::from_std(sender.into())
            })();
            if let Ok(sender) = sender {
                senders.push((sender, broadcast));
            }
        }
        if senders.is_empty() {
            return Err(io::Error::other(
                "Nenhuma interface disponível para descoberta.",
            ));
        }
        let receiver = UdpSocket::from_std(socket.into())?;
        let port = receiver.local_addr()?.port();
        Ok(Self {
            receiver,
            senders,
            port,
            id: rand::random(),
            seen: BTreeMap::new(),
        })
    }
    pub async fn announce(&self, name: &str, port: u16, available: bool) {
        let packet = Announcement {
            id: self.id,
            magic: MAGIC.into(),
            version: VERSION,
            name: device_name(name),
            port,
            available,
        };
        let Ok(bytes) = serde_json::to_vec(&packet) else {
            return;
        };
        for (sender, broadcast) in &self.senders {
            let _ = sender.send_to(&bytes, (GROUP, self.port)).await;
            if let Some(ip) = broadcast {
                let _ = sender.send_to(&bytes, (*ip, self.port)).await;
            }
        }
    }
    pub async fn scan(&mut self, duration: Duration) -> Vec<Room> {
        let deadline = tokio::time::Instant::now() + duration;
        let mut bytes = [0; 1025];
        loop {
            let Ok(Ok((len, source))) =
                tokio::time::timeout_at(deadline, self.receiver.recv_from(&mut bytes)).await
            else {
                break;
            };
            if len > 1024 {
                continue;
            }
            let Ok(packet) = serde_json::from_slice::<Announcement>(&bytes[..len]) else {
                continue;
            };
            let address = SocketAddr::new(source.ip(), packet.port);
            if packet.magic != MAGIC
                || packet.version != VERSION
                || !network::allowed_address(address)
            {
                continue;
            }
            if !packet.available {
                self.seen.remove(&address);
                continue;
            }
            if self.seen.len() < 128 || self.seen.contains_key(&address) {
                self.seen.insert(
                    address,
                    (packet.id, device_name(&packet.name), Instant::now()),
                );
            }
        }
        self.seen
            .retain(|_, (_, _, seen)| seen.elapsed() < Duration::from_secs(5));
        let rooms = find_rooms(
            self.seen
                .iter()
                .map(|(&address, (_, name, _))| Room {
                    name: name.clone(),
                    address,
                })
                .collect(),
        )
        .await;
        let mut ids = BTreeSet::new();
        rooms
            .into_iter()
            .filter(|room| {
                self.seen
                    .get(&room.address)
                    .is_some_and(|(id, _, _)| ids.insert(*id))
            })
            .collect()
    }
}
/// Somente respostas válidas do sync2gether viram salas, sem carregar mídias.
pub async fn find_rooms(peers: Vec<Room>) -> Vec<Room> {
    let mut peers = peers.into_iter();
    let mut tasks = JoinSet::new();
    let mut rooms = Vec::new();
    loop {
        while tasks.len() < 16 {
            let Some(peer) = peers.next() else {
                break;
            };
            tasks.spawn(async move {
                network::rooms::probe(peer.address).await.map(|name| Room {
                    name,
                    address: peer.address,
                })
            });
        }
        let Some(result) = tasks.join_next().await else {
            break;
        };
        if let Ok(Some(room)) = result {
            rooms.push(room);
        }
    }
    rooms.sort_by(|a, b| a.name.cmp(&b.name).then(a.address.cmp(&b.address)));
    rooms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn malformed_foreign_oversized_and_incompatible_packets_are_ignored() {
        let mut lan = Lan::on_interfaces(0, &[(Ipv4Addr::LOCALHOST, None)]).unwrap();
        let address = SocketAddr::from((
            Ipv4Addr::LOCALHOST,
            lan.receiver.local_addr().unwrap().port(),
        ));
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        for bytes in [b"not-json".to_vec(), vec![b' '; 1025]] {
            sender.send_to(&bytes, address).await.unwrap();
        }
        for (magic, version, port) in [
            ("other-app", VERSION, 9000),
            (MAGIC, VERSION + 1, 9000),
            (MAGIC, VERSION, 0),
        ] {
            let packet = Announcement {
                id: 1,
                magic: magic.into(),
                version,
                name: "ignored".into(),
                port,
                available: true,
            };
            sender
                .send_to(&serde_json::to_vec(&packet).unwrap(), address)
                .await
                .unwrap();
        }
        assert!(lan.scan(Duration::from_millis(50)).await.is_empty());
        assert!(
            lan.seen.is_empty(),
            "invalid packets must never cause TCP probes"
        );
        assert_eq!(device_name("a\nb"), "ab");
        assert_eq!(device_name("\n\t"), "Outro computador");
    }
}

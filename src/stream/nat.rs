use rand::Rng;
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    collections::HashSet,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket},
    time::Duration,
};
use tokio::net::lookup_host;

pub const DEFAULT_STUN_SERVERS: &[&str] = &["stun.cloudflare.com:3478", "stun.l.google.com:19302"];
pub const MAX_CANDIDATES: usize = 16;
const STUN_MAGIC: u32 = 0x2112_A442;
const STUN_BINDING_REQUEST: u16 = 0x0001;
const STUN_BINDING_RESPONSE: u16 = 0x0101;
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
const PUNCH_PACKET: &[u8] = b"S2GPUNCH";

pub fn bind_udp() -> io::Result<UdpSocket> {
    if let Ok(socket) = bind_dual_stack() {
        return Ok(socket);
    }
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)).into())?;
    Ok(socket.into())
}

fn bind_dual_stack() -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_only_v6(false)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)).into())?;
    Ok(socket.into())
}

pub fn normalize_for_socket(local: SocketAddr, remote: SocketAddr) -> Option<SocketAddr> {
    match (local, remote) {
        (SocketAddr::V6(_), SocketAddr::V4(v4)) => Some(SocketAddr::V6(SocketAddrV6::new(
            v4.ip().to_ipv6_mapped(),
            v4.port(),
            0,
            0,
        ))),
        (SocketAddr::V4(_), SocketAddr::V6(v6)) => v6
            .ip()
            .to_ipv4_mapped()
            .map(|ip| SocketAddr::V4(SocketAddrV4::new(ip, v6.port()))),
        (_, other) => Some(other),
    }
}

pub async fn gather_candidates(socket: &UdpSocket, stun_enabled: bool) -> Vec<SocketAddr> {
    let Ok(local) = socket.local_addr() else {
        return Vec::new();
    };
    let port = local.port();
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    let mut push = |addr: SocketAddr| {
        if out.len() < MAX_CANDIDATES && crate::network::allowed_address(addr) && seen.insert(addr)
        {
            out.push(addr);
        }
    };

    if stun_enabled && let Some(reflexive) = stun_discover(socket, DEFAULT_STUN_SERVERS).await {
        push(reflexive);
    }

    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for iface in interfaces {
            let ip = iface.ip();
            if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
                continue;
            }
            if let IpAddr::V6(v6) = ip
                && (v6.segments()[0] & 0xffc0) == 0xfe80
            {
                continue;
            }
            push(SocketAddr::new(ip, port));
        }
    }

    let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    if (!stun_enabled || out.is_empty())
        && out.len() < MAX_CANDIDATES
        && crate::network::allowed_address(loopback)
        && seen.insert(loopback)
    {
        out.push(loopback);
    }
    out
}

pub fn filter_remote_candidates(raw: &[String], allow_loopback: bool) -> Vec<SocketAddr> {
    raw.iter()
        .filter_map(|c| c.parse::<SocketAddr>().ok())
        .filter(|a| {
            crate::network::allowed_address(*a) && (allow_loopback || !a.ip().is_loopback())
        })
        .collect()
}

pub fn send_hole_punches(socket: &UdpSocket, candidates: &[SocketAddr]) {
    let Ok(local) = socket.local_addr() else {
        return;
    };
    for &candidate in candidates {
        if let Some(target) = normalize_for_socket(local, candidate) {
            let _ = socket.send_to(PUNCH_PACKET, target);
        }
    }
}

fn build_stun_request(tx_id: [u8; 12]) -> [u8; 20] {
    let mut buf = [0u8; 20];
    buf[0..2].copy_from_slice(&STUN_BINDING_REQUEST.to_be_bytes());
    buf[2..4].copy_from_slice(&0u16.to_be_bytes());
    buf[4..8].copy_from_slice(&STUN_MAGIC.to_be_bytes());
    buf[8..20].copy_from_slice(&tx_id);
    buf
}

pub fn parse_stun_response(packet: &[u8], expected_tx: &[u8; 12]) -> Option<SocketAddr> {
    if packet.len() < 20 {
        return None;
    }
    let msg_type = u16::from_be_bytes([packet[0], packet[1]]);
    let msg_len = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    let magic = u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]);
    if msg_type != STUN_BINDING_RESPONSE
        || magic != STUN_MAGIC
        || &packet[8..20] != expected_tx
        || packet.len() < 20 + msg_len
    {
        return None;
    }
    let mut pos = 20;
    let end = 20 + msg_len;
    let mut fallback = None;
    while pos + 4 <= end {
        let attr_type = u16::from_be_bytes([packet[pos], packet[pos + 1]]);
        let attr_len = u16::from_be_bytes([packet[pos + 2], packet[pos + 3]]) as usize;
        pos += 4;
        if pos + attr_len > end {
            break;
        }
        let val = &packet[pos..pos + attr_len];
        if attr_type == ATTR_XOR_MAPPED_ADDRESS
            && let Some(addr) = parse_xor_mapped(val, expected_tx)
        {
            return Some(addr);
        }
        if attr_type == ATTR_MAPPED_ADDRESS
            && let Some(addr) = parse_mapped(val)
        {
            fallback = Some(addr);
        }
        pos += (attr_len + 3) & !3;
    }
    fallback
}

fn parse_xor_mapped(val: &[u8], tx_id: &[u8; 12]) -> Option<SocketAddr> {
    if val.len() < 8 {
        return None;
    }
    let family = val[1];
    let xport = u16::from_be_bytes([val[2], val[3]]) ^ ((STUN_MAGIC >> 16) as u16);
    match family {
        0x01 if val.len() >= 8 => {
            let magic = STUN_MAGIC.to_be_bytes();
            let ip = Ipv4Addr::new(
                val[4] ^ magic[0],
                val[5] ^ magic[1],
                val[6] ^ magic[2],
                val[7] ^ magic[3],
            );
            Some(SocketAddr::V4(SocketAddrV4::new(ip, xport)))
        }
        0x02 if val.len() >= 20 => {
            let mut mask = [0u8; 16];
            mask[0..4].copy_from_slice(&STUN_MAGIC.to_be_bytes());
            mask[4..16].copy_from_slice(tx_id);
            let mut octets = [0u8; 16];
            for i in 0..16 {
                octets[i] = val[4 + i] ^ mask[i];
            }
            Some(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(octets),
                xport,
                0,
                0,
            )))
        }
        _ => None,
    }
}

fn parse_mapped(val: &[u8]) -> Option<SocketAddr> {
    if val.len() < 8 {
        return None;
    }
    let family = val[1];
    let port = u16::from_be_bytes([val[2], val[3]]);
    match family {
        0x01 if val.len() >= 8 => Some(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(val[4], val[5], val[6], val[7]),
            port,
        ))),
        0x02 if val.len() >= 20 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&val[4..20]);
            Some(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(octets),
                port,
                0,
                0,
            )))
        }
        _ => None,
    }
}

pub(crate) async fn stun_discover(socket: &UdpSocket, servers: &[&str]) -> Option<SocketAddr> {
    let cloned = socket.try_clone().ok()?;
    let local = cloned.local_addr().ok()?;
    let async_sock = tokio::net::UdpSocket::from_std(cloned).ok()?;
    let mut tx_id = [0u8; 12];
    rand::rng().fill_bytes(&mut tx_id);
    let request = build_stun_request(tx_id);

    for &server in servers {
        let Ok(Ok(addrs)) =
            tokio::time::timeout(Duration::from_millis(350), lookup_host(server)).await
        else {
            continue;
        };
        for addr in addrs.take(2) {
            if let Some(target) = normalize_for_socket(local, addr) {
                let _ = async_sock.send_to(&request, target).await;
            }
        }
    }

    let mut buf = [0u8; 512];
    let deadline = tokio::time::Instant::now() + Duration::from_millis(700);
    while let Ok(Ok((n, _))) =
        tokio::time::timeout_at(deadline, async_sock.recv_from(&mut buf)).await
    {
        if let Some(mapped) = parse_stun_response(&buf[..n], &tx_id) {
            return Some(mapped);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv4_and_ipv6_xor_mapped_stun_responses() {
        let tx_id = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let port: u16 = 54321;
        let ip = Ipv4Addr::new(203, 0, 113, 42);
        let magic = STUN_MAGIC.to_be_bytes();
        let xport = port ^ ((STUN_MAGIC >> 16) as u16);

        let mut packet = Vec::new();
        packet.extend_from_slice(&STUN_BINDING_RESPONSE.to_be_bytes());
        packet.extend_from_slice(&12u16.to_be_bytes());
        packet.extend_from_slice(&magic);
        packet.extend_from_slice(&tx_id);
        packet.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
        packet.extend_from_slice(&8u16.to_be_bytes());
        packet.push(0);
        packet.push(0x01);
        packet.extend_from_slice(&xport.to_be_bytes());
        let octets = ip.octets();
        for i in 0..4 {
            packet.push(octets[i] ^ magic[i]);
        }

        assert_eq!(
            parse_stun_response(&packet, &tx_id),
            Some(SocketAddr::V4(SocketAddrV4::new(ip, port)))
        );
        assert!(parse_stun_response(&packet, &[0u8; 12]).is_none());
    }

    #[tokio::test]
    async fn binds_udp_and_gathers_local_candidates_offline() {
        let sock = bind_udp().unwrap();
        let candidates = gather_candidates(&sock, false).await;
        assert!(!candidates.is_empty());
        assert!(
            candidates
                .iter()
                .all(|c| c.port() == sock.local_addr().unwrap().port())
        );
    }
}

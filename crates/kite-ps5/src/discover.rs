//! Finding Kite helpers on the home network: a UDP broadcast that each helper answers.

use serde::Serialize;
use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

/// The helper's TCP port, and the UDP port it answers discovery on.
pub const DISCOVERY_PORT: u16 = 9130;
const ASK: &[u8] = b"KITE-DISCOVER 1";
const ANSWER: &str = "KITE-HELPER 1 ";

/// A helper that answered.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Found {
    pub address: String,
    pub port: u16,
    pub version: String,
}

/// Asks the whole home network: the limited broadcast and every interface's own broadcast
/// address. Waits `wait` for answers.
pub fn discover(wait: Duration) -> Vec<Found> {
    let mut targets = vec![SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, DISCOVERY_PORT))];
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for interface in interfaces {
            if let if_addrs::IfAddr::V4(v4) = interface.addr
                && !v4.ip.is_loopback()
                && let Some(broadcast) = v4.broadcast
            {
                targets.push(SocketAddr::V4(SocketAddrV4::new(broadcast, DISCOVERY_PORT)));
            }
        }
    }
    discover_at(&targets, wait)
}

/// Asks the given addresses and collects every answer within `wait`.
pub fn discover_at(targets: &[SocketAddr], wait: Duration) -> Vec<Found> {
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else { return Vec::new() };
    let _ = socket.set_broadcast(true);
    for target in targets {
        let _ = socket.send_to(ASK, target);
    }
    let deadline = Instant::now() + wait;
    let mut found = BTreeSet::new();
    let mut buf = [0u8; 128];
    while let Some(left) = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) {
        let _ = socket.set_read_timeout(Some(left));
        let Ok((n, from)) = socket.recv_from(&mut buf) else { break };
        let Some(rest) = std::str::from_utf8(&buf[..n]).ok().and_then(|s| s.strip_prefix(ANSWER)) else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        if let (Some(Ok(port)), Some(version)) = (parts.next().map(str::parse::<u16>), parts.next()) {
            found.insert(Found { address: from.ip().to_string(), port, version: version.to_string() });
        }
    }
    found.into_iter().collect()
}

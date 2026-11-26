//! Send magic packets over the Console's local IPv4 broadcast interfaces.

use std::collections::HashSet;
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use if_addrs::{IfAddr, Interface};
use socket2::{Domain, Protocol, Socket, Type};

const WAKE_PORT: u16 = 9;
const DEVICE_INTERVAL: Duration = Duration::from_secs(15);
const SERVER_INTERVAL: Duration = Duration::from_secs(60);
const SERVER_BURST: usize = 20;

#[derive(Clone, Copy, Debug)]
pub(crate) struct MacAddress([u8; 6]);

impl MacAddress {
    pub(crate) fn manufacturer(self) -> Option<String> {
        // Locally administered addresses do not identify an IEEE-assigned vendor.
        if self.0[0] & 2 != 0 {
            return None;
        }
        rsmanuf::lookup(self.to_string())
            .ok()
            .filter(|name| !name.trim().is_empty() && !name.eq_ignore_ascii_case("unknown"))
    }
}

impl std::fmt::Display for MacAddress {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [a, b, c, d, e, f] = self.0;
        write!(formatter, "{a:02X}:{b:02X}:{c:02X}:{d:02X}:{e:02X}:{f:02X}")
    }
}

impl FromStr for MacAddress {
    type Err = ();

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let separator = match input.as_bytes().get(2) {
            Some(b':') => ':',
            Some(b'-') => '-',
            _ => return Err(()),
        };
        let mut octets = input.split(separator);
        let mut bytes = [0; 6];
        for byte in &mut bytes {
            let octet = octets.next().ok_or(())?;
            if octet.len() != 2 || !octet.bytes().all(|digit| digit.is_ascii_hexdigit()) {
                return Err(());
            }
            *byte = u8::from_str_radix(octet, 16).map_err(|_| ())?;
        }
        if octets.next().is_some() || bytes == [0; 6] || bytes[0] & 1 != 0 {
            return Err(());
        }
        Ok(Self(bytes))
    }
}

#[derive(Clone, Debug)]
struct Network {
    interface: Option<String>,
    source: SocketAddrV4,
    destination: SocketAddrV4,
}

impl Network {
    fn from_interface(interface: &Interface) -> Option<Self> {
        if !interface.is_oper_up() || interface.is_loopback() || interface.is_p2p() {
            return None;
        }
        let IfAddr::V4(address) = &interface.addr else {
            return None;
        };
        if !(1..=30).contains(&address.prefixlen) || address.ip.is_unspecified() {
            return None;
        }
        let broadcast = address.broadcast?;
        // Linux can pin a limited broadcast to the exact NIC, even if two NICs
        // have addresses in the same subnet. Elsewhere use its directed broadcast.
        let destination = if cfg!(any(target_os = "linux", target_os = "android")) {
            Ipv4Addr::BROADCAST
        } else {
            broadcast
        };
        Some(Self {
            interface: Some(interface.name.clone()),
            source: SocketAddrV4::new(address.ip, 0),
            destination: SocketAddrV4::new(destination, WAKE_PORT),
        })
    }

    fn send(&self, packet: &wake_on_lan::MagicPacket) -> io::Result<()> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if let Some(interface) = &self.interface {
            socket.bind_device(Some(interface.as_bytes()))?;
        }
        socket.bind(&self.source.into())?;
        socket.set_broadcast(true)?;
        socket.send_to(packet.magic_bytes(), &self.destination.into())?;
        Ok(())
    }
}

fn broadcast_networks(interfaces: &[Interface]) -> Vec<Network> {
    let mut names = HashSet::new();
    interfaces
        .iter()
        .filter_map(|interface| {
            let network = Network::from_interface(interface)?;
            names.insert(interface.name.as_str()).then_some(network)
        })
        .collect()
}

#[derive(Debug)]
pub(crate) struct WakeOnLan {
    attempts: Mutex<Vec<(Instant, i32)>>,
    #[cfg(test)]
    test_networks: Option<Vec<Network>>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SendError {
    #[error("No local interface supports IPv4 broadcast")]
    NoBroadcastInterface,
    #[error("Wake packet rate limit reached")]
    RateLimited,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Task(#[from] tokio::task::JoinError),
}

impl WakeOnLan {
    pub(crate) fn new() -> Self {
        Self {
            attempts: Mutex::new(Vec::new()),
            #[cfg(test)]
            test_networks: None,
        }
    }

    async fn networks(&self) -> Result<Vec<Network>, SendError> {
        #[cfg(test)]
        if let Some(networks) = &self.test_networks {
            return Ok(networks.clone());
        }
        Ok(tokio::task::spawn_blocking(|| {
            if_addrs::get_if_addrs().map(|interfaces| broadcast_networks(&interfaces))
        })
        .await??)
    }

    pub(crate) async fn available(&self) -> bool {
        self.networks()
            .await
            .is_ok_and(|networks| !networks.is_empty())
    }

    pub(crate) async fn send(&self, row_id: i32, mac: MacAddress) -> Result<(), SendError> {
        let networks = self.networks().await?;
        if networks.is_empty() {
            return Err(SendError::NoBroadcastInterface);
        }
        if !self.reserve(row_id) {
            return Err(SendError::RateLimited);
        }
        tokio::task::spawn_blocking(move || {
            let packet = wake_on_lan::MagicPacket::new(&mac.0);
            let mut sent = false;
            let mut last_error = None;
            for network in networks {
                match network.send(&packet) {
                    Ok(()) => sent = true,
                    Err(error) => {
                        tracing::debug!(
                            interface = network.interface.as_deref(),
                            %error,
                            "Wake-on-LAN send failed on interface"
                        );
                        last_error = Some(error);
                    }
                }
            }
            if sent {
                Ok(())
            } else {
                Err(last_error.map_or(SendError::NoBroadcastInterface, SendError::Io))
            }
        })
        .await?
    }

    fn reserve(&self, row_id: i32) -> bool {
        let mut attempts = self.attempts.lock().expect("wake attempt gate poisoned");
        let now = Instant::now();
        attempts.retain(|(when, _)| now.duration_since(*when) < SERVER_INTERVAL);
        if attempts.len() >= SERVER_BURST
            || attempts.iter().any(|(when, device)| {
                *device == row_id && now.duration_since(*when) < DEVICE_INTERVAL
            })
        {
            return false;
        }
        attempts.push((now, row_id));
        true
    }

    #[cfg(test)]
    pub(crate) fn with_loopback_destinations(ports: &[u16]) -> Self {
        Self {
            attempts: Mutex::new(Vec::new()),
            test_networks: Some(
                ports
                    .iter()
                    .map(|port| Network {
                        interface: None,
                        source: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                        destination: SocketAddrV4::new(Ipv4Addr::LOCALHOST, *port),
                    })
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use if_addrs::{IfOperStatus, Ifv4Addr};

    #[test]
    fn parses_only_complete_unicast_mac_addresses() {
        let globally_administered: MacAddress = " 00-1b-63-00-00-01 ".parse().unwrap();
        let canonical = globally_administered.to_string();
        assert_eq!(canonical, "00:1B:63:00:00:01");
        assert!(
            globally_administered
                .manufacturer()
                .is_some_and(|name| name.contains("Apple"))
        );
        let locally_administered: MacAddress = "02:1b:63:00:00:01".parse().unwrap();
        assert_eq!(locally_administered.manufacturer(), None,);
        let unknown: MacAddress = "FC:FF:FF:00:00:01".parse().unwrap();
        assert_eq!(unknown.manufacturer(), None);
        for invalid in [
            "",
            "00:00:00:00:00:00",
            "ff:ff:ff:ff:ff:ff",
            "01:00:5e:00:00:01",
            "00:11-22:33:44:55",
            "00:11:22:33:44:+5",
            "00:11:22:33:44:55:66",
        ] {
            assert!(invalid.parse::<MacAddress>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn selects_each_up_broadcast_interface_once() {
        fn interface(name: &str, ip: &str, broadcast: Option<Ipv4Addr>) -> Interface {
            Interface {
                name: name.to_string(),
                index: None,
                addr: IfAddr::V4(Ifv4Addr {
                    ip: ip.parse().unwrap(),
                    netmask: Ipv4Addr::new(255, 255, 255, 0),
                    prefixlen: 24,
                    broadcast,
                }),
                oper_status: IfOperStatus::Up,
                is_p2p: false,
                #[cfg(windows)]
                adapter_name: name.to_string(),
            }
        }
        let broadcast = Some(Ipv4Addr::new(192, 168, 16, 255));
        let mut down = interface("down", "192.168.16.3", broadcast);
        down.oper_status = IfOperStatus::Down;
        let mut tunnel = interface("tunnel", "192.168.16.4", broadcast);
        tunnel.is_p2p = true;
        let networks = broadcast_networks(&[
            interface("lo", "127.0.0.1", None),
            interface("ethernet", "192.168.16.5", broadcast),
            interface("ethernet", "192.168.16.6", broadcast),
            interface("wifi", "192.168.16.7", broadcast),
            interface("no-broadcast", "192.168.16.8", None),
            down,
            tunnel,
        ]);
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].source.ip(), &Ipv4Addr::new(192, 168, 16, 5));
        assert_eq!(networks[1].source.ip(), &Ipv4Addr::new(192, 168, 16, 7));
        assert_eq!(networks[1].destination.port(), WAKE_PORT);
    }

    #[test]
    fn bounds_each_device_and_total_attempts() {
        let transport = WakeOnLan::new();
        assert!(transport.reserve(1));
        assert!(!transport.reserve(1));
        for row in 2..=SERVER_BURST as i32 {
            assert!(transport.reserve(row));
        }
        assert!(!transport.reserve(101));
    }
}

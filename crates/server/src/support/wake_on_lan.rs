//! Wake packet transport restricted to one operator-configured IPv4 subnet.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

#[derive(Clone, Copy, Debug)]
pub(crate) struct Network {
    source: SocketAddrV4,
    destination: SocketAddrV4,
}

impl FromStr for Network {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        const INVALID: &str = "wake-on-lan.source-cidr must be a usable host IPv4 address with a subnet prefix between /1 and /30";
        let (address, prefix) = value
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!(INVALID))?;
        let (source, prefix): (Ipv4Addr, u32) = match (address.parse(), prefix.parse()) {
            (Ok(source), Ok(prefix)) => (source, prefix),
            _ => anyhow::bail!(INVALID),
        };
        anyhow::ensure!(
            (1..=30).contains(&prefix)
                && (1..224).contains(&source.octets()[0])
                && !source.is_loopback(),
            INVALID
        );
        let host_mask = u32::MAX >> prefix;
        let host_bits = u32::from(source) & host_mask;
        anyhow::ensure!(host_bits != 0 && host_bits != host_mask, INVALID);
        Ok(Self {
            source: SocketAddrV4::new(source, 0),
            destination: SocketAddrV4::new(
                Ipv4Addr::from(u32::from(source) | host_mask),
                WAKE_PORT,
            ),
        })
    }
}

#[derive(Debug)]
pub(crate) struct WakeOnLan {
    network: Option<Network>,
    attempts: Mutex<Vec<(Instant, i32)>>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SendError {
    #[error("Wake-on-LAN is disabled")]
    NotConfigured,
    #[error("Wake packet rate limit reached")]
    RateLimited,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Task(#[from] tokio::task::JoinError),
}

impl WakeOnLan {
    pub(crate) fn new(network: Option<Network>) -> Self {
        Self {
            network,
            attempts: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn configured(&self) -> bool {
        self.network.is_some()
    }

    pub(crate) async fn send(&self, row_id: i32, mac: MacAddress) -> Result<(), SendError> {
        let network = self.network.ok_or(SendError::NotConfigured)?;
        if !self.reserve(row_id) {
            return Err(SendError::RateLimited);
        }
        tokio::task::spawn_blocking(move || {
            wake_on_lan::MagicPacket::new(&mac.0).send_to(network.destination, network.source)
        })
        .await??;
        Ok(())
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
    pub(crate) fn with_loopback_destination(port: u16) -> Self {
        Self::new(Some(Network {
            source: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            destination: SocketAddrV4::new(Ipv4Addr::LOCALHOST, port),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_complete_unicast_mac_addresses() {
        let globally_administered: MacAddress = " 00-1b-63-00-00-01 ".parse().unwrap();
        let canonical = globally_administered.to_string();
        assert_eq!(canonical, "00:1B:63:00:00:01");
        assert!(globally_administered
            .manufacturer()
            .is_some_and(|name| name.contains("Apple")));
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
    fn derives_the_destination_from_the_configured_local_subnet() {
        let network: Network = "192.168.16.140/26".parse().unwrap();
        assert_eq!(network.source, "192.168.16.140:0".parse().unwrap());
        assert_eq!(network.destination, "192.168.16.191:9".parse().unwrap());
        for invalid in [
            "192.168.16.140",
            "192.168.16.140/0",
            "192.168.16.140/31",
            "192.168.16.128/26",
            "192.168.16.191/26",
            "0.0.0.0/24",
            "230.1.0.1/24",
            "127.0.0.1/8",
            "::1/64",
        ] {
            assert!(invalid.parse::<Network>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn bounds_each_device_and_total_attempts() {
        let transport = WakeOnLan::new(None);
        assert!(transport.reserve(1));
        assert!(!transport.reserve(1));
        for row in 2..=SERVER_BURST as i32 {
            assert!(transport.reserve(row));
        }
        assert!(!transport.reserve(101));
    }
}

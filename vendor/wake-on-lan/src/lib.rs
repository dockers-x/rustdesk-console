//! A library for creating and sending Wake-on-LAN magic packets.
//!
//! # Usage
//!
//! ```no_run
//! # fn main() -> std::io::Result<()> {
//! use wake_on_lan;
//!
//! // The MAC address of the target device
//! let mac_address: [u8; 6] = [0x0F, 0x1E, 0x2D, 0x3C, 0x4B, 0x5A];
//!
//! // Create a magic packet (but don't send it yet)
//! let magic_packet = wake_on_lan::MagicPacket::new(&mac_address);
//!
//! // Send the magic packet via UDP to the broadcast address 255.255.255.255:9 from 0.0.0.0:0
//! magic_packet.send()?;
//! # Ok(())
//! # }
//! ```
//!
//! To choose the source and destination IPs and ports, use `send_to()`. If you want to access the
//! contents of the magic packet, use `magic_bytes()`.

use std::net::{Ipv4Addr, ToSocketAddrs, UdpSocket};

/// A Wake-on-LAN magic packet.
pub struct MagicPacket {
    magic_bytes: [u8; 102],
}

impl MagicPacket {
    /// Creates a new `MagicPacket` intended for `mac_address` (but doesn't send it yet).
    pub fn new(mac_address: &[u8; 6]) -> MagicPacket {
        let mut magic_bytes = [0; 102];
        let (header, body) = magic_bytes.split_at_mut(MAGIC_BYTES_HEADER.len());
        header.copy_from_slice(&MAGIC_BYTES_HEADER);
        for repeat in body.chunks_exact_mut(mac_address.len()) {
            repeat.copy_from_slice(mac_address);
        }
        MagicPacket { magic_bytes }
    }

    /// Sends the magic packet via UDP to the broadcast address `255.255.255.255:9`.
    /// Lets the operating system choose the source port and network interface.
    pub fn send(&self) -> std::io::Result<()> {
        self.send_to(
            (Ipv4Addr::new(255, 255, 255, 255), 9),
            (Ipv4Addr::new(0, 0, 0, 0), 0),
        )
    }

    /// Sends the magic packet via UDP to/from an IP address and port number of your choosing.
    pub fn send_to<A: ToSocketAddrs>(&self, to_addr: A, from_addr: A) -> std::io::Result<()> {
        let socket = UdpSocket::bind(from_addr)?;
        socket.set_broadcast(true)?;
        socket.send_to(&self.magic_bytes, to_addr)?;

        Ok(())
    }

    /// Returns the magic packet's payload (6 repetitions of `0xFF` and 16 repetitions of the
    /// target device's MAC address). Send these bytes yourself over the network if you want to do
    /// something more advanced (like reuse a single UDP socket when sending a large number of
    /// magic packets).
    pub fn magic_bytes(&self) -> &[u8; 102] {
        &self.magic_bytes
    }
}

const MAGIC_BYTES_HEADER: [u8; 6] = [0xFF; 6];

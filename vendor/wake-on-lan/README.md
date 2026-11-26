wake-on-lan
===========

A Rust library for creating and sending Wake-on-LAN magic packets.

# Usage

```rust
use wake_on_lan;

// The MAC address of the target device
let mac_address: [u8; 6] = [0x0F, 0x1E, 0x2D, 0x3C, 0x4B, 0x5A];

// Create a magic packet (but don't send it yet)
let magic_packet = wake_on_lan::MagicPacket::new(&mac_address);

// Send the magic packet via UDP to the broadcast address 255.255.255.255:9
// from 0.0.0.0:0
magic_packet.send()?;
```

To choose the source and destination IPs and ports, use `send_to()`. If you want
to access the contents of the magic packet, use `magic_bytes()`.

## Console patch

This is wake-on-lan 0.2.0 from crates.io. The patch replaces the original
uninitialized array and raw pointer copies in `MagicPacket::new` with safe
array initialization and slice copies. The packet format and UDP send API
follow the upstream crate.

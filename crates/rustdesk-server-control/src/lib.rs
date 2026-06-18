//! Loopback command-channel client for RustDesk hbbs/hbbr.
//!
//! RustDesk server exposes a small local TCP command channel. `hbbs` listens on
//! id-server-port - 1, while `hbbr` listens on relay-server-port. This crate
//! keeps that protocol independent from the console service layer.

use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const TARGET_ID_SERVER: &str = "21115";
pub const TARGET_RELAY_SERVER: &str = "21117";

#[derive(Debug, Clone, Copy, Default)]
pub struct ControlPorts {
    pub id_server_cmd_port: Option<u16>,
    pub relay_server_cmd_port: Option<u16>,
}

impl ControlPorts {
    pub fn from_config_ports(id_server_port: i32, relay_server_port: i32) -> Self {
        Self {
            id_server_cmd_port: normalize_port(id_server_port - 1),
            relay_server_cmd_port: normalize_port(relay_server_port),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetStatus {
    pub target: String,
    pub label: String,
    pub port: Option<u16>,
    pub available: bool,
    pub message: String,
}

pub fn target_port(ports: ControlPorts, target: &str) -> Option<u16> {
    if target == TARGET_ID_SERVER {
        ports.id_server_cmd_port
    } else if target == TARGET_RELAY_SERVER {
        ports.relay_server_cmd_port
    } else {
        None
    }
}

pub fn target_label(target: &str) -> &'static str {
    if target == TARGET_ID_SERVER {
        "ID"
    } else if target == TARGET_RELAY_SERVER {
        "Relay"
    } else {
        "Unknown"
    }
}

pub async fn target_status(ports: ControlPorts, target: &str) -> TargetStatus {
    let port = target_port(ports, target);
    let Some(port) = port else {
        return TargetStatus {
            target: target.to_string(),
            label: target_label(target).to_string(),
            port,
            available: false,
            message: "port is not configured".to_string(),
        };
    };

    match send_cmd(port, "h", "").await {
        Ok(_) => TargetStatus {
            target: target.to_string(),
            label: target_label(target).to_string(),
            port: Some(port),
            available: true,
            message: "command endpoint is available".to_string(),
        },
        Err(e) => TargetStatus {
            target: target.to_string(),
            label: target_label(target).to_string(),
            port: Some(port),
            available: false,
            message: e,
        },
    }
}

pub async fn send_target_cmd(
    ports: ControlPorts,
    target: &str,
    cmd: &str,
    arg: &str,
) -> Result<String, String> {
    let Some(port) = target_port(ports, target) else {
        return Err("target port is not configured".to_string());
    };
    send_cmd(port, cmd, arg).await
}

/// Send a command to the local id/relay server, trying IPv6 then IPv4
/// (mirrors RustDesk server's loopback command channel).
pub async fn send_cmd(port: u16, cmd: &str, arg: &str) -> Result<String, String> {
    let full = format!("{cmd} {arg}");
    match send_socket_cmd("[::1]", port, &full).await {
        Ok(r) => Ok(r),
        Err(_) => send_socket_cmd("127.0.0.1", port, &full).await,
    }
}

fn normalize_port(port: i32) -> Option<u16> {
    if (1..=u16::MAX as i32).contains(&port) {
        Some(port as u16)
    } else {
        None
    }
}

async fn send_socket_cmd(addr: &str, port: u16, cmd: &str) -> Result<String, String> {
    let target = format!("{addr}:{port}");
    let mut conn = TcpStream::connect(&target)
        .await
        .map_err(|e| format!("connect {target} failed: {e}"))?;
    conn.write_all(cmd.as_bytes())
        .await
        .map_err(|e| format!("send cmd failed: {e}"))?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut buf = vec![0u8; 4096];
    let n = match tokio::time::timeout(Duration::from_secs(3), conn.read(&mut buf)).await {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => return Err(format!("read response failed: {e}")),
        Err(_) => 0,
    };
    Ok(String::from_utf8_lossy(&buf[..n]).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_label_maps_known_targets() {
        assert_eq!(target_label(TARGET_ID_SERVER), "ID");
        assert_eq!(target_label(TARGET_RELAY_SERVER), "Relay");
        assert_eq!(target_label("x"), "Unknown");
    }

    #[test]
    fn target_port_uses_rustdesk_command_ports() {
        let ports = ControlPorts::from_config_ports(21116, 21117);
        assert_eq!(target_port(ports, TARGET_ID_SERVER), Some(21115));
        assert_eq!(target_port(ports, TARGET_RELAY_SERVER), Some(21117));
    }
}

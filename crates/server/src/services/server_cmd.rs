//! Server-command service — ports `service/serverCmd.go`. CRUD over stored
//! commands plus live dispatch to the RustDesk id/relay server over TCP.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use sea_orm::*;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;

use ::entity::{server_cmd, system_setting};
use rustdesk_server_control as control;

use crate::config::Config;
use crate::services::{now, paginate};

const RELAY_POOL_SETTING_KEY: &str = "rustdesk_relay_pool";
const DEFAULT_RELAY_PORT: u16 = crate::config::DEFAULT_RELAY_SERVER_PORT as u16;

pub struct ServerCmdListResult {
    pub list: Vec<server_cmd::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RelayPoolSetting {
    pub servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelayPoolView {
    pub servers: Vec<String>,
    pub value: String,
    pub persisted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelayServerCheck {
    pub server: String,
    pub ok: bool,
    pub message: String,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
) -> Result<ServerCmdListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let q = server_cmd::Entity::find();
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(ServerCmdListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn info(db: &DatabaseConnection, id: i32) -> Result<Option<server_cmd::Model>, DbErr> {
    server_cmd::Entity::find_by_id(id).one(db).await
}

pub async fn create(
    db: &DatabaseConnection,
    cmd: &str,
    alias: &str,
    option: &str,
    explain: &str,
    target: &str,
) -> Result<server_cmd::Model, DbErr> {
    let am = server_cmd::ActiveModel {
        cmd: Set(cmd.to_string()),
        alias: Set(alias.to_string()),
        option: Set(option.to_string()),
        explain: Set(explain.to_string()),
        target: Set(target.to_string()),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await
}

pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    cmd: &str,
    alias: &str,
    option: &str,
    explain: &str,
    target: &str,
) -> Result<server_cmd::Model, DbErr> {
    let Some(existing) = info(db, id).await? else {
        return Err(DbErr::RecordNotFound(format!("server_cmd {id}")));
    };
    let mut am: server_cmd::ActiveModel = existing.into();
    am.cmd = Set(cmd.to_string());
    am.alias = Set(alias.to_string());
    am.option = Set(option.to_string());
    am.explain = Set(explain.to_string());
    am.target = Set(target.to_string());
    am.updated_at = Set(now());
    am.update(db).await
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    server_cmd::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub fn normalize_relay_pool(input: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in input
        .split(|ch| ch == ',' || ch == '\n' || ch == '\r' || ch == ';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let server = normalize_relay_server(raw)?;
        if seen.insert(server.to_lowercase()) {
            out.push(server);
        }
    }
    if out.len() > 50 {
        return Err("relay server count must not exceed 50".to_string());
    }
    Ok(out)
}

fn normalize_relay_server(raw: &str) -> Result<String, String> {
    if raw.contains("://") || raw.contains('/') || raw.chars().any(char::is_whitespace) {
        return Err(format!("invalid relay server: {raw}"));
    }
    if raw.starts_with('[') {
        return normalize_bracketed_host(raw);
    }

    let colon_count = raw.matches(':').count();
    if colon_count > 1 {
        return Ok(format!("[{raw}]:{DEFAULT_RELAY_PORT}"));
    }
    if colon_count == 1 {
        let (host, port) = raw.rsplit_once(':').unwrap_or((raw, ""));
        validate_host(host, raw)?;
        validate_port(port, raw)?;
        return Ok(format!("{host}:{}", port.trim()));
    }
    validate_host(raw, raw)?;
    Ok(format!("{raw}:{DEFAULT_RELAY_PORT}"))
}

fn normalize_bracketed_host(raw: &str) -> Result<String, String> {
    let Some(close) = raw.find(']') else {
        return Err(format!("invalid relay server: {raw}"));
    };
    let host = &raw[1..close];
    validate_host(host, raw)?;
    let rest = &raw[close + 1..];
    if rest.is_empty() {
        return Ok(format!("[{host}]:{DEFAULT_RELAY_PORT}"));
    }
    let Some(port) = rest.strip_prefix(':') else {
        return Err(format!("invalid relay server: {raw}"));
    };
    validate_port(port, raw)?;
    Ok(format!("[{host}]:{}", port.trim()))
}

fn validate_host(host: &str, raw: &str) -> Result<(), String> {
    let host = host.trim();
    if host.is_empty() || host.len() > 253 {
        return Err(format!("invalid relay server: {raw}"));
    }
    if host
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':')))
    {
        return Err(format!("invalid relay server: {raw}"));
    }
    Ok(())
}

fn validate_port(port: &str, raw: &str) -> Result<(), String> {
    let Ok(port) = port.trim().parse::<u16>() else {
        return Err(format!("invalid relay server port: {raw}"));
    };
    if port == 0 {
        return Err(format!("invalid relay server port: {raw}"));
    }
    Ok(())
}

pub async fn load_relay_pool(db: &DatabaseConnection) -> Result<RelayPoolView, DbErr> {
    let row = system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(RELAY_POOL_SETTING_KEY))
        .one(db)
        .await?;
    let Some(row) = row else {
        return Ok(RelayPoolView {
            servers: Vec::new(),
            value: String::new(),
            persisted: false,
        });
    };
    let setting = serde_json::from_str::<RelayPoolSetting>(&row.value).unwrap_or_default();
    if setting.servers.is_empty() {
        return Ok(RelayPoolView {
            servers: Vec::new(),
            value: String::new(),
            persisted: false,
        });
    }
    Ok(RelayPoolView {
        value: setting.servers.join(","),
        servers: setting.servers,
        persisted: true,
    })
}

pub async fn save_relay_pool(
    db: &DatabaseConnection,
    input: &str,
) -> Result<RelayPoolView, String> {
    let servers = normalize_relay_pool(input)?;
    if servers.is_empty() {
        system_setting::Entity::delete_many()
            .filter(system_setting::Column::Key.eq(RELAY_POOL_SETTING_KEY))
            .exec(db)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(RelayPoolView {
            value: String::new(),
            servers,
            persisted: false,
        });
    }
    let encoded = serde_json::to_string(&RelayPoolSetting {
        servers: servers.clone(),
    })
    .map_err(|e| e.to_string())?;
    match system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(RELAY_POOL_SETTING_KEY))
        .one(db)
        .await
        .map_err(|e| e.to_string())?
    {
        Some(row) => {
            let mut am: system_setting::ActiveModel = row.into();
            am.value = Set(encoded);
            am.updated_at = Set(now());
            am.update(db).await.map_err(|e| e.to_string())?;
        }
        None => {
            system_setting::ActiveModel {
                key: Set(RELAY_POOL_SETTING_KEY.to_string()),
                value: Set(encoded),
                created_at: Set(now()),
                updated_at: Set(now()),
                ..Default::default()
            }
            .insert(db)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(RelayPoolView {
        value: servers.join(","),
        servers,
        persisted: true,
    })
}

pub async fn save_and_sync_relay_pool(
    db: &DatabaseConnection,
    config: &Config,
    input: &str,
) -> Result<(RelayPoolView, String), String> {
    let view = save_relay_pool(db, input).await?;
    if !view.persisted {
        return Ok((
            view,
            "relay pool override removed; hbbs runtime configuration was not changed".to_string(),
        ));
    }
    let response = sync_relay_pool(config, &view.servers).await?;
    Ok((view, response))
}

pub async fn sync_saved_relay_pool(db: &DatabaseConnection, config: &Config) -> Result<(), String> {
    let pool = load_relay_pool(db).await.map_err(|e| e.to_string())?;
    if !pool.persisted {
        return Ok(());
    }
    sync_relay_pool(config, &pool.servers).await.map(|_| ())
}

pub fn spawn_saved_relay_pool_sync(db: DatabaseConnection, config: Arc<Config>) {
    tokio::spawn(async move {
        for attempt in 1..=10 {
            match sync_saved_relay_pool(&db, &config).await {
                Ok(()) => {
                    tracing::info!("synced persisted relay pool to hbbs");
                    return;
                }
                Err(e) => {
                    tracing::warn!(
                        "failed to sync persisted relay pool to hbbs (attempt {attempt}): {e}"
                    );
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
        }
    });
}

pub async fn check_relay_pool(input: &str) -> Result<Vec<RelayServerCheck>, String> {
    let servers = normalize_relay_pool(input)?;
    let mut checks = Vec::with_capacity(servers.len());
    for server in servers {
        checks.push(check_one_relay_server(server).await);
    }
    Ok(checks)
}

async fn check_one_relay_server(server: String) -> RelayServerCheck {
    let target = normalize_tcp_target(&server);
    match tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(&target)).await {
        Ok(Ok(_)) => RelayServerCheck {
            server,
            ok: true,
            message: "tcp reachable".to_string(),
        },
        Ok(Err(e)) => RelayServerCheck {
            server,
            ok: false,
            message: format!("connect {target} failed: {e}"),
        },
        Err(_) => RelayServerCheck {
            server,
            ok: false,
            message: format!("connect {target} timed out"),
        },
    }
}

fn normalize_tcp_target(server: &str) -> String {
    if server.starts_with('[') {
        server.to_string()
    } else if server.matches(':').count() > 1 {
        format!("[{server}]")
    } else {
        server.to_string()
    }
}

/// The built-in system commands `CmdList` prepends (mirrors `model.Sys*Cmds`).
pub fn system_commands() -> Vec<server_cmd::Model> {
    let id_t = server_cmd::TARGET_ID_SERVER;
    let relay_t = server_cmd::TARGET_RELAY_SERVER;
    let mk =
        |cmd: &str, alias: &str, option: &str, explain: &str, target: &str| server_cmd::Model {
            id: 0,
            cmd: cmd.into(),
            alias: alias.into(),
            option: option.into(),
            explain: explain.into(),
            target: target.into(),
            created_at: None,
            updated_at: None,
        };
    vec![
        mk("h", "", "", "show help", id_t),
        mk(
            "relay-servers",
            "rs",
            "<separated by ,>",
            "set or show relay servers",
            id_t,
        ),
        mk(
            "ip-blocker",
            "ib",
            "[<ip>|<number>] [-]",
            "block or unblock ip or show blocked ip",
            id_t,
        ),
        mk(
            "ip-changes",
            "ic",
            "[<id>|<number>] [-]",
            "ip-changes(ic) [<id>|<number>] [-]",
            id_t,
        ),
        mk("always-use-relay", "aur", "[y|n]", "always use relay", id_t),
        mk("test-geo", "tg", "<ip1> <ip2>", "test geo", id_t),
        mk("h", "", "", "show help", relay_t),
        mk(
            "blacklist-add",
            "ba",
            "<ip>",
            "blacklist-add(ba) <ip>",
            relay_t,
        ),
        mk(
            "blacklist-remove",
            "br",
            "<ip>",
            "blacklist-remove(br) <ip>",
            relay_t,
        ),
        mk("blacklist", "b", "<ip>", "blacklist(b) <ip>", relay_t),
        mk(
            "blocklist-add",
            "Ba",
            "<ip>",
            "blocklist-add(Ba) <ip>",
            relay_t,
        ),
        mk(
            "blocklist-remove",
            "Br",
            "<ip>",
            "blocklist-remove(Br) <ip>",
            relay_t,
        ),
        mk("blocklist", "B", "<ip>", "blocklist(B) <ip>", relay_t),
        mk(
            "downgrade-threshold",
            "dt",
            "[value]",
            "downgrade-threshold(dt) [value]",
            relay_t,
        ),
        mk(
            "downgrade-start-check",
            "t",
            "[value(second)]",
            "downgrade-start-check(t) [value(second)]",
            relay_t,
        ),
        mk(
            "limit-speed",
            "ls",
            "[value(Mb/s)]",
            "limit-speed(ls) [value(Mb/s)]",
            relay_t,
        ),
        mk(
            "total-bandwidth",
            "tb",
            "[value(Mb/s)]",
            "total-bandwidth(tb) [value(Mb/s)]",
            relay_t,
        ),
        mk(
            "single-bandwidth",
            "sb",
            "[value(Mb/s)]",
            "single-bandwidth(sb) [value(Mb/s)]",
            relay_t,
        ),
        mk("usage", "u", "", "usage(u)", relay_t),
    ]
}

#[derive(Debug, Clone, Serialize)]
pub struct RustdeskTargetStatus {
    pub target: String,
    pub label: String,
    pub port: Option<u16>,
    pub available: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RustdeskStructuredStatus {
    pub targets: Vec<RustdeskTargetStatus>,
    pub relay_servers: Option<String>,
    pub always_use_relay: Option<String>,
}

pub fn target_port(config: &Config, target: &str) -> Option<u16> {
    control::target_port(control_ports(config), target)
}

pub fn target_label(target: &str) -> &'static str {
    control::target_label(target)
}

pub async fn structured_status(config: &Config) -> RustdeskStructuredStatus {
    let targets = vec![
        target_status(config, server_cmd::TARGET_ID_SERVER).await,
        target_status(config, server_cmd::TARGET_RELAY_SERVER).await,
    ];
    let relay_servers = send_target_cmd(config, server_cmd::TARGET_ID_SERVER, "relay-servers", "")
        .await
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let always_use_relay =
        send_target_cmd(config, server_cmd::TARGET_ID_SERVER, "always-use-relay", "")
            .await
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

    RustdeskStructuredStatus {
        targets,
        relay_servers,
        always_use_relay,
    }
}

async fn target_status(config: &Config, target: &str) -> RustdeskTargetStatus {
    let status = control::target_status(control_ports(config), target).await;
    RustdeskTargetStatus {
        target: status.target,
        label: status.label,
        port: status.port,
        available: status.available,
        message: status.message,
    }
}

pub async fn send_target_cmd(
    config: &Config,
    target: &str,
    cmd: &str,
    arg: &str,
) -> Result<String, String> {
    control::send_target_cmd(control_ports(config), target, cmd, arg).await
}

async fn sync_relay_pool(config: &Config, servers: &[String]) -> Result<String, String> {
    if servers.is_empty() {
        return Ok(String::new());
    }
    let arg = servers.join(",");
    send_target_cmd(config, server_cmd::TARGET_ID_SERVER, "relay-servers", &arg).await
}

/// Send a command to the local id/relay server, trying IPv6 then IPv4
/// (mirrors `SendCmd` / `SendSocketCmd`).
pub async fn send_cmd(port: u16, cmd: &str, arg: &str) -> Result<String, String> {
    control::send_cmd(port, cmd, arg).await
}

fn control_ports(config: &Config) -> control::ControlPorts {
    control::ControlPorts::from_config_ports(
        config.admin.id_server_port,
        config.admin.relay_server_port,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_label_maps_known_targets() {
        assert_eq!(target_label(server_cmd::TARGET_ID_SERVER), "ID");
        assert_eq!(target_label(server_cmd::TARGET_RELAY_SERVER), "Relay");
        assert_eq!(target_label("x"), "Unknown");
    }

    #[test]
    fn relay_pool_normalization_adds_default_port_and_deduplicates() {
        let servers =
            normalize_relay_pool("relay.example.com\nrelay.example.com:21117,10.0.0.2").unwrap();
        assert_eq!(servers, vec!["relay.example.com:21117", "10.0.0.2:21117"]);
    }

    #[test]
    fn relay_pool_rejects_urls() {
        assert!(normalize_relay_pool("https://relay.example.com:21117").is_err());
    }
}

//! Server-command service — ports `service/serverCmd.go`. CRUD over stored
//! commands plus live dispatch to the RustDesk id/relay server over TCP.

use std::time::Duration;

use sea_orm::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use ::entity::server_cmd;

use crate::services::{now, paginate};

pub struct ServerCmdListResult {
    pub list: Vec<server_cmd::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
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

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    server_cmd::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

/// The built-in system commands `CmdList` prepends (mirrors `model.Sys*Cmds`).
pub fn system_commands() -> Vec<server_cmd::Model> {
    let id_t = server_cmd::TARGET_ID_SERVER;
    let relay_t = server_cmd::TARGET_RELAY_SERVER;
    let mk = |cmd: &str, alias: &str, option: &str, explain: &str, target: &str| server_cmd::Model {
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
        mk("relay-servers", "rs", "<separated by ,>", "set or show relay servers", id_t),
        mk("ip-blocker", "ib", "[<ip>|<number>] [-]", "block or unblock ip or show blocked ip", id_t),
        mk("ip-changes", "ic", "[<id>|<number>] [-]", "ip-changes(ic) [<id>|<number>] [-]", id_t),
        mk("always-use-relay", "aur", "[y|n]", "always use relay", id_t),
        mk("test-geo", "tg", "<ip1> <ip2>", "test geo", id_t),
        mk("h", "", "", "show help", relay_t),
        mk("blacklist-add", "ba", "<ip>", "blacklist-add(ba) <ip>", relay_t),
        mk("blacklist-remove", "br", "<ip>", "blacklist-remove(br) <ip>", relay_t),
        mk("blacklist", "b", "<ip>", "blacklist(b) <ip>", relay_t),
        mk("blocklist-add", "Ba", "<ip>", "blocklist-add(Ba) <ip>", relay_t),
        mk("blocklist-remove", "Br", "<ip>", "blocklist-remove(Br) <ip>", relay_t),
        mk("blocklist", "B", "<ip>", "blocklist(B) <ip>", relay_t),
        mk("downgrade-threshold", "dt", "[value]", "downgrade-threshold(dt) [value]", relay_t),
        mk("downgrade-start-check", "t", "[value(second)]", "downgrade-start-check(t) [value(second)]", relay_t),
        mk("limit-speed", "ls", "[value(Mb/s)]", "limit-speed(ls) [value(Mb/s)]", relay_t),
        mk("total-bandwidth", "tb", "[value(Mb/s)]", "total-bandwidth(tb) [value(Mb/s)]", relay_t),
        mk("single-bandwidth", "sb", "[value(Mb/s)]", "single-bandwidth(sb) [value(Mb/s)]", relay_t),
        mk("usage", "u", "", "usage(u)", relay_t),
    ]
}

/// Send a command to the local id/relay server, trying IPv6 then IPv4
/// (mirrors `SendCmd` / `SendSocketCmd`).
pub async fn send_cmd(port: u16, cmd: &str, arg: &str) -> Result<String, String> {
    let full = format!("{cmd} {arg}");
    match send_socket_cmd("[::1]", port, &full).await {
        Ok(r) => Ok(r),
        Err(_) => send_socket_cmd("127.0.0.1", port, &full).await,
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
    let mut buf = vec![0u8; 1024];
    let n = match tokio::time::timeout(Duration::from_secs(3), conn.read(&mut buf)).await {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => return Err(format!("read response failed: {e}")),
        Err(_) => 0, // timeout: treat as empty response
    };
    Ok(String::from_utf8_lossy(&buf[..n]).to_string())
}

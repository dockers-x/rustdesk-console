//! Admin overview aggregation for the console landing page.

use std::collections::BTreeMap;

use chrono::Utc;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Serialize;

use entity::{active_connection, address_book, audit_conn, audit_file, login_log, peer, share_record, user};

const ONLINE_WINDOW_SECONDS: i64 = 90;
const RECENT_LIMIT: u64 = 6;

#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub generated_at: String,
    pub version: String,
    pub start_time: String,
    pub uptime_seconds: i64,
    pub totals: OverviewTotals,
    pub platforms: Vec<NameCount>,
    pub recent_logins: Vec<LoginSummary>,
    pub recent_connections: Vec<ConnectionSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverviewTotals {
    pub users: u64,
    pub admins: u64,
    pub devices: u64,
    pub online_devices: u64,
    pub active_connections: u64,
    pub address_books: u64,
    pub share_records: u64,
    pub login_logs: u64,
    pub audit_connections: u64,
    pub audit_files: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NameCount {
    pub name: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginSummary {
    pub id: i32,
    pub user_id: i32,
    pub client: String,
    pub ip: String,
    pub r#type: String,
    pub platform: String,
    pub created_at: Option<sea_orm::prelude::DateTime>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionSummary {
    pub id: i32,
    pub action: String,
    pub conn_id: i64,
    pub peer_id: String,
    pub from_peer: String,
    pub ip: String,
    pub created_at: Option<sea_orm::prelude::DateTime>,
}

pub async fn load(
    db: &DatabaseConnection,
    version: &str,
    start_time: &str,
) -> Result<Overview, sea_orm::DbErr> {
    let now = Utc::now().timestamp();
    let online_since = now - ONLINE_WINDOW_SECONDS;
    let peers = peer::Entity::find().all(db).await?;

    let recent_logins = login_log::Entity::find()
        .order_by_desc(login_log::Column::Id)
        .limit(RECENT_LIMIT)
        .all(db)
        .await?
        .into_iter()
        .map(|row| LoginSummary {
            id: row.id,
            user_id: row.user_id,
            client: row.client,
            ip: row.ip,
            r#type: row.r#type,
            platform: row.platform,
            created_at: row.created_at,
        })
        .collect();

    let recent_connections = audit_conn::Entity::find()
        .order_by_desc(audit_conn::Column::Id)
        .limit(RECENT_LIMIT)
        .all(db)
        .await?
        .into_iter()
        .map(|row| ConnectionSummary {
            id: row.id,
            action: row.action,
            conn_id: row.conn_id,
            peer_id: row.peer_id,
            from_peer: row.from_peer,
            ip: row.ip,
            created_at: row.created_at,
        })
        .collect();

    Ok(Overview {
        generated_at: Utc::now().to_rfc3339(),
        version: version.to_string(),
        start_time: start_time.to_string(),
        uptime_seconds: uptime_seconds(start_time),
        totals: OverviewTotals {
            users: user::Entity::find().count(db).await?,
            admins: user::Entity::find()
                .filter(user::Column::IsAdmin.eq(true))
                .count(db)
                .await?,
            devices: peers.len() as u64,
            online_devices: peers
                .iter()
                .filter(|p| p.last_online_time >= online_since)
                .count() as u64,
            active_connections: active_connection::Entity::find().count(db).await?,
            address_books: address_book::Entity::find().count(db).await?,
            share_records: share_record::Entity::find().count(db).await?,
            login_logs: login_log::Entity::find().count(db).await?,
            audit_connections: audit_conn::Entity::find().count(db).await?,
            audit_files: audit_file::Entity::find().count(db).await?,
        },
        platforms: platform_counts(peers.iter().map(|p| p.os.as_str())),
        recent_logins,
        recent_connections,
    })
}

pub fn platform_name(os: &str) -> String {
    let lower = os.to_lowercase();
    if lower.contains("windows") {
        "Windows".to_string()
    } else if lower.contains("android") {
        "Android".to_string()
    } else if lower.contains("linux") {
        "Linux".to_string()
    } else if lower.contains("mac") || lower.contains("darwin") || lower.contains("osx") {
        "macOS".to_string()
    } else if os.trim().is_empty() {
        "Unknown".to_string()
    } else {
        "Other".to_string()
    }
}

pub fn platform_counts<'a>(items: impl Iterator<Item = &'a str>) -> Vec<NameCount> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for item in items {
        *counts.entry(platform_name(item)).or_insert(0) += 1;
    }
    let mut out: Vec<NameCount> = counts
        .into_iter()
        .map(|(name, count)| NameCount { name, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    out
}

pub fn uptime_seconds(start_time: &str) -> i64 {
    let Ok(started_at) = chrono::DateTime::parse_from_rfc3339(start_time) else {
        return 0;
    };
    (Utc::now() - started_at.with_timezone(&Utc))
        .num_seconds()
        .max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_name_normalizes_common_os_strings() {
        assert_eq!(platform_name("Windows 11"), "Windows");
        assert_eq!(platform_name("android"), "Android");
        assert_eq!(platform_name("Ubuntu Linux"), "Linux");
        assert_eq!(platform_name("Darwin 23"), "macOS");
        assert_eq!(platform_name(""), "Unknown");
        assert_eq!(platform_name("FreeBSD"), "Other");
    }

    #[test]
    fn platform_counts_orders_by_count_then_name() {
        let counts = platform_counts(["Windows", "linux", "Windows", "", "FreeBSD"].into_iter());
        assert_eq!(
            counts,
            vec![
                NameCount {
                    name: "Windows".to_string(),
                    count: 2,
                },
                NameCount {
                    name: "Linux".to_string(),
                    count: 1,
                },
                NameCount {
                    name: "Other".to_string(),
                    count: 1,
                },
                NameCount {
                    name: "Unknown".to_string(),
                    count: 1,
                },
            ]
        );
    }
}

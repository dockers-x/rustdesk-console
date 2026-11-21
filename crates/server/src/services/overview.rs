//! Admin overview aggregation for the console landing page.

use std::collections::BTreeMap;

use chrono::Utc;
use sea_orm::sea_query::{Expr, Func};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};
use serde::Serialize;

use entity::{address_book, audit_conn, audit_file, login_log, peer, share_record, user};

use super::active_connection;

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
    let fleet = fleet_summary(db, online_since).await?;

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
            devices: fleet.devices,
            online_devices: fleet.online,
            active_connections: active_connection::count(db).await?,
            address_books: address_book::Entity::find().count(db).await?,
            share_records: share_record::Entity::find().count(db).await?,
            login_logs: login_log::Entity::find().count(db).await?,
            audit_connections: audit_conn::Entity::find().count(db).await?,
            audit_files: audit_file::Entity::find().count(db).await?,
        },
        platforms: fleet.platforms,
        recent_logins,
        recent_connections,
    })
}

fn platform_name(os: &str) -> &'static str {
    let lower = os.to_lowercase();
    if lower.contains("windows") {
        "Windows"
    } else if lower.contains("android") {
        "Android"
    } else if lower.contains("linux") {
        "Linux"
    } else if lower.contains("mac") || lower.contains("darwin") || lower.contains("osx") {
        "macOS"
    } else if os.trim().is_empty() {
        "Unknown"
    } else {
        "Other"
    }
}

struct FleetSummary {
    devices: u64,
    online: u64,
    platforms: Vec<NameCount>,
}

async fn fleet_summary(
    db: &DatabaseConnection,
    online_since: i64,
) -> Result<FleetSummary, sea_orm::DbErr> {
    // Keep both totals in the same snapshot. MySQL's default text collation
    // can merge OS strings that the Rust platform classifier treats differently.
    let os_key = match db.get_database_backend() {
        DatabaseBackend::MySql => Expr::cust("BINARY os"),
        _ => Expr::col(peer::Column::Os).into(),
    };
    let online =
        Expr::case(peer::Column::LastOnlineTime.gte(online_since), 1).finally(Option::<i32>::None);
    let groups = peer::Entity::find()
        .select_only()
        .column_as(peer::Column::Os.min(), "os_name")
        .column_as(peer::Column::RowId.count(), "devices")
        .expr_as(Func::count(online), "online")
        .group_by(os_key)
        .into_tuple::<(String, i64, i64)>()
        .all(db)
        .await?;
    let mut devices = 0;
    let mut online = 0;
    let mut counts = BTreeMap::new();
    for (os, total, active) in groups {
        devices += total as u64;
        online += active as u64;
        *counts.entry(platform_name(&os)).or_insert(0) += total as u64;
    }
    let mut platforms: Vec<_> = counts
        .into_iter()
        .map(|(name, count)| NameCount {
            name: name.to_owned(),
            count,
        })
        .collect();
    platforms.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    Ok(FleetSummary {
        devices,
        online,
        platforms,
    })
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

    #[tokio::test]
    async fn fleet_summary_preserves_platform_counts_and_inclusive_online_boundary() {
        use sea_orm::{Database, Schema};

        let db = Database::connect("sqlite::memory:").await.unwrap();
        let backend = db.get_database_backend();
        db.execute(backend.build(&Schema::new(backend).create_table_from_entity(peer::Entity)))
            .await
            .unwrap();
        let empty = fleet_summary(&db, 100).await.unwrap();
        assert_eq!(
            (empty.devices, empty.online, empty.platforms.len()),
            (0, 0, 0)
        );
        db.execute_unprepared("INSERT INTO peers (os, last_online_time) VALUES ('Windows 10', 99), ('Windows 10', 100), ('Windows 11', 101), ('linux', 0), (' ', 0), ('FreeBSD', 0)").await.unwrap();
        let summary = fleet_summary(&db, 100).await.unwrap();
        assert_eq!((summary.devices, summary.online), (6, 2));
        assert_eq!(
            summary.platforms,
            vec![
                NameCount {
                    name: "Windows".to_string(),
                    count: 3,
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

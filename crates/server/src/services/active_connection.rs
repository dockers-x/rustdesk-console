//! Active connection tracking from RustDesk heartbeat `conns`.

use std::collections::BTreeSet;

use chrono::{TimeDelta, Utc};
use sea_orm::*;

use ::entity::active_connection;

use crate::services::{now, paginate};

const ACTIVE_CONNECTION_WINDOW: TimeDelta = TimeDelta::seconds(90);

pub struct ActiveConnectionListResult {
    pub list: Vec<active_connection::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

fn current_connections() -> Select<active_connection::Entity> {
    let cutoff = Utc::now().naive_utc() - ACTIVE_CONNECTION_WINDOW;
    active_connection::Entity::find().filter(active_connection::Column::UpdatedAt.gte(cutoff))
}

pub async fn count(db: &DatabaseConnection) -> Result<u64, DbErr> {
    current_connections().count(db).await
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    peer_id: Option<String>,
    uuid: Option<String>,
) -> Result<ActiveConnectionListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = current_connections();
    if let Some(peer_id) = peer_id.filter(|s| !s.is_empty()) {
        q = q.filter(active_connection::Column::PeerId.contains(&peer_id));
    }
    if let Some(uuid) = uuid.filter(|s| !s.is_empty()) {
        q = q.filter(active_connection::Column::Uuid.contains(&uuid));
    }

    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(active_connection::Column::UpdatedAt)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;

    Ok(ActiveConnectionListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn sync_for_device(
    db: &DatabaseConnection,
    peer_id: &str,
    uuid: &str,
    conn_ids: &[i64],
) -> Result<(), DbErr> {
    if peer_id.is_empty() && uuid.is_empty() {
        return Ok(());
    }

    let conn_ids: BTreeSet<i64> = conn_ids.iter().copied().filter(|id| *id > 0).collect();
    let mut previous = active_connection::Entity::delete_many();
    if !peer_id.is_empty() {
        previous = previous.filter(active_connection::Column::PeerId.eq(peer_id));
    }
    if !uuid.is_empty() {
        previous = previous.filter(active_connection::Column::Uuid.eq(uuid));
    }
    if conn_ids.is_empty() {
        previous.exec(db).await?;
        return Ok(());
    }

    let txn = db.begin().await?;
    previous.exec(&txn).await?;
    let timestamp = now();
    for conn_id in conn_ids {
        let am = active_connection::ActiveModel {
            conn_id: Set(conn_id),
            peer_id: Set(peer_id.to_string()),
            uuid: Set(uuid.to_string()),
            created_at: Set(timestamp),
            updated_at: Set(timestamp),
            ..Default::default()
        };
        am.insert(&txn).await?;
    }
    txn.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectOptions, ConnectionTrait, Database, EntityTrait, PaginatorTrait, Schema};

    async fn setup_db() -> DatabaseConnection {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.unwrap();
        let backend = db.get_database_backend();
        let schema = Schema::new(backend);
        let stmt = schema
            .create_table_from_entity(active_connection::Entity)
            .to_owned();
        db.execute(backend.build(&stmt)).await.unwrap();
        db
    }

    async fn all_rows(db: &DatabaseConnection) -> Vec<active_connection::Model> {
        active_connection::Entity::find()
            .order_by_asc(active_connection::Column::ConnId)
            .all(db)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn sync_for_device_replaces_existing_connections_for_same_device() {
        let db = setup_db().await;

        sync_for_device(&db, "peer-1", "uuid-1", &[100, 101])
            .await
            .unwrap();
        assert_eq!(
            active_connection::Entity::find().count(&db).await.unwrap(),
            2
        );

        sync_for_device(&db, "peer-1", "uuid-1", &[101, 102, 102, 0])
            .await
            .unwrap();

        let rows = all_rows(&db).await;
        let conn_ids: Vec<i64> = rows.iter().map(|row| row.conn_id).collect();
        assert_eq!(conn_ids, vec![101, 102]);
        assert!(rows.iter().all(|row| row.peer_id == "peer-1"));
        assert!(rows.iter().all(|row| row.uuid == "uuid-1"));
    }

    #[tokio::test]
    async fn sync_for_device_empty_connection_list_clears_device_rows() {
        let db = setup_db().await;

        sync_for_device(&db, "peer-1", "uuid-1", &[100])
            .await
            .unwrap();
        sync_for_device(&db, "peer-1", "uuid-1", &[]).await.unwrap();

        assert_eq!(
            active_connection::Entity::find().count(&db).await.unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn active_list_excludes_expired_and_undated_snapshots() {
        let db = setup_db().await;
        let current = chrono::Utc::now().naive_utc();
        for (conn_id, timestamp) in [
            (101, Some(current)),
            (102, Some(current - chrono::TimeDelta::days(100))),
            (103, None),
        ] {
            active_connection::ActiveModel {
                conn_id: Set(conn_id),
                peer_id: Set("peer-1".into()),
                uuid: Set("uuid-1".into()),
                updated_at: Set(timestamp),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }

        let result = list(&db, 1, 1, Some("peer-1".into()), None).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.list[0].conn_id, 101);
        assert_eq!(count(&db).await.unwrap(), 1);
    }
}

//! Active connection tracking from RustDesk heartbeat `conns`.

use sea_orm::*;

use ::entity::active_connection;

use crate::services::{now, paginate};

pub struct ActiveConnectionListResult {
    pub list: Vec<active_connection::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    peer_id: Option<String>,
    uuid: Option<String>,
) -> Result<ActiveConnectionListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = active_connection::Entity::find();
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

    delete_for_device(db, peer_id, uuid).await?;

    let conn_ids: Vec<i64> = conn_ids.iter().copied().filter(|id| *id > 0).collect();
    for conn_id in conn_ids {
        let am = active_connection::ActiveModel {
            conn_id: Set(conn_id),
            peer_id: Set(peer_id.to_string()),
            uuid: Set(uuid.to_string()),
            created_at: Set(now()),
            updated_at: Set(now()),
            ..Default::default()
        };
        am.insert(db).await?;
    }
    Ok(())
}

async fn delete_for_device(
    db: &DatabaseConnection,
    peer_id: &str,
    uuid: &str,
) -> Result<(), DbErr> {
    let mut q = active_connection::Entity::delete_many();
    if !uuid.is_empty() {
        q = q.filter(active_connection::Column::Uuid.eq(uuid));
    } else {
        q = q.filter(active_connection::Column::PeerId.eq(peer_id));
    }
    q.exec(db).await?;
    Ok(())
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
    async fn sync_for_device_replaces_existing_connections_for_same_uuid() {
        let db = setup_db().await;

        sync_for_device(&db, "peer-1", "uuid-1", &[100, 101])
            .await
            .unwrap();
        assert_eq!(
            active_connection::Entity::find().count(&db).await.unwrap(),
            2
        );

        sync_for_device(&db, "peer-1", "uuid-1", &[101, 102])
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
}

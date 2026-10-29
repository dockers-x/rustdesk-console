use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::Deserialize;
use serde_json::Value;

use ::entity::audit_alarm;

use crate::services::{now, paginate};

#[derive(Default, Deserialize)]
#[serde(default)]
struct AlarmEvent {
    id: String,
    uuid: String,
    typ: i32,
    info: String,
    conn_id: Option<i64>,
    nonce: Option<String>,
    conn_audit_ref: Option<String>,
}

pub async fn record_event<C: ConnectionTrait>(db: &C, body: &Value) -> Result<(), DbErr> {
    let event = AlarmEvent::deserialize(body)
        .map_err(|error| DbErr::Custom(format!("Invalid alarm audit: {error}")))?;
    audit_alarm::ActiveModel {
        id: Set(event.id),
        uuid: Set(event.uuid),
        typ: Set(event.typ),
        info: Set(event.info),
        conn_id: Set(event.conn_id),
        nonce: Set(event.nonce),
        conn_audit_ref: Set(event.conn_audit_ref),
        created_at: Set(now()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

pub struct AlarmListResult {
    pub list: Vec<audit_alarm::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    id: Option<String>,
) -> Result<AlarmListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut query = audit_alarm::Entity::find();
    if let Some(id) = id.filter(|id| !id.is_empty()) {
        query = query.filter(audit_alarm::Column::Id.contains(&id));
    }
    let total = query.clone().count(db).await? as i64;
    let list = query
        .order_by_desc(audit_alarm::Column::RowId)
        .offset((page - 1).saturating_mul(page_size))
        .limit(page_size)
        .all(db)
        .await?;
    Ok(AlarmListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectOptions, Database, Schema};
    use serde_json::json;

    async fn setup_db() -> DatabaseConnection {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.unwrap();
        let backend = db.get_database_backend();
        let schema = Schema::new(backend);
        db.execute(backend.build(&schema.create_table_from_entity(audit_alarm::Entity)))
            .await
            .unwrap();
        db
    }

    #[tokio::test]
    async fn persists_current_client_alarm_without_changing_wire_fields() {
        let db = setup_db().await;
        let body = json!({
            "id": "123456789", "uuid": "device-uuid", "typ": 3,
            "info": "{\"message\":\"scope violation\"}", "conn_id": 42,
            "nonce": "alarm-nonce", "conn_audit_ref": "connection-ref"
        });
        record_event(&db, &body).await.unwrap();
        let result = list(&db, 0, 0, None).await.unwrap();
        assert_eq!((result.total, result.page, result.page_size), (1, 1, 10));
        let saved = serde_json::to_value(&result.list[0]).unwrap();
        for (field, value) in body.as_object().unwrap() {
            assert_eq!(&saved[field], value, "{field}");
        }
        assert!(saved["created_at"].is_string());
    }

    #[tokio::test]
    async fn accepts_legacy_alarms_without_optional_fields_or_nonce_deduplication() {
        let db = setup_db().await;
        let body = json!({
            "id": "legacy-device", "uuid": "legacy-uuid", "typ": 0,
            "info": "{}", "future_field": true
        });
        record_event(&db, &body).await.unwrap();
        record_event(&db, &body).await.unwrap();
        let result = list(&db, 1, 10, None).await.unwrap();
        assert_eq!(result.total, 2);
        assert_eq!(result.list[0].conn_id, None);
        assert_eq!(result.list[0].nonce, None);
        assert_eq!(result.list[0].conn_audit_ref, None);
    }

    #[tokio::test]
    async fn lists_filtered_alarms_newest_first_with_total_before_pagination() {
        let db = setup_db().await;
        for id in ["target-1", "other", "target-2", "target-3"] {
            record_event(&db, &json!({"id": id, "typ": 1, "info": "{}"}))
                .await
                .unwrap();
        }
        let result = list(&db, 2, 2, Some("target".to_string())).await.unwrap();
        assert_eq!((result.total, result.page, result.page_size), (3, 2, 2));
        assert_eq!(result.list.len(), 1);
        assert_eq!(result.list[0].id, "target-1");
        let first = list(&db, 1, 2, Some("target".to_string())).await.unwrap();
        assert_eq!(first.list[0].id, "target-3");
        assert_eq!(first.list[1].id, "target-2");
    }
}

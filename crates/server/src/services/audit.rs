//! Audit service — ports `service/audit.go` (conn + file logs).

use sea_orm::sea_query::Expr;
use sea_orm::*;
use serde_json::Value as JsonValue;

use ::entity::{audit_conn, audit_file};

use crate::services::paginate;

// ---- AuditConn ----

pub struct AuditConnListResult {
    pub list: Vec<audit_conn::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn conn_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    peer_id: Option<String>,
    from_peer: Option<String>,
) -> Result<AuditConnListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = audit_conn::Entity::find();
    if let Some(p) = peer_id.filter(|s| !s.is_empty()) {
        q = q.filter(audit_conn::Column::PeerId.contains(&p));
    }
    if let Some(p) = from_peer.filter(|s| !s.is_empty()) {
        q = q.filter(audit_conn::Column::FromPeer.contains(&p));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(audit_conn::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(AuditConnListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn conn_info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<audit_conn::Model>, DbErr> {
    audit_conn::Entity::find_by_id(id).one(db).await
}

pub async fn delete_conn(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    audit_conn::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn batch_delete_conn(db: &DatabaseConnection, ids: &[i32]) -> Result<(), DbErr> {
    audit_conn::Entity::delete_many()
        .filter(audit_conn::Column::Id.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnPeer {
    id: String,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnEvent {
    action: Option<String>,
    conn_id: i64,
    peer_id: String,
    from_peer: String,
    from_name: String,
    ip: String,
    session_id: String,
    r#type: i32,
    uuid: String,
    close_time: i64,
    note: Option<String>,
    peer: Option<ConnPeer>,
}

pub async fn record_conn_event(db: &DatabaseConnection, body: &JsonValue) -> Result<(), DbErr> {
    let event = ConnEvent::from_json(body);

    if let Some(note) = &event.note {
        update_conn_note(db, &event.session_id, note).await?;
        return Ok(());
    }

    if let Some(action) = &event.action {
        match action.as_str() {
            "new" => insert_conn_event(db, &event).await?,
            "close" => close_conn_event(db, &event).await?,
            _ => {}
        }
        return Ok(());
    }

    if event.peer.is_some() {
        update_conn_peer(db, &event).await?;
    }

    Ok(())
}

async fn insert_conn_event(db: &DatabaseConnection, event: &ConnEvent) -> Result<(), DbErr> {
    let now = crate::services::now();
    let am = audit_conn::ActiveModel {
        action: Set(event.action.clone().unwrap_or_default()),
        conn_id: Set(event.conn_id),
        peer_id: Set(event.peer_id.clone()),
        from_peer: Set(event.from_peer.clone()),
        from_name: Set(event.from_name.clone()),
        ip: Set(event.ip.clone()),
        session_id: Set(event.session_id.clone()),
        r#type: Set(event.r#type),
        uuid: Set(event.uuid.clone()),
        note: Set(String::new()),
        close_time: Set(event.close_time),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    am.insert(db).await?;
    Ok(())
}

async fn update_conn_note(
    db: &DatabaseConnection,
    session_id: &str,
    note: &str,
) -> Result<(), DbErr> {
    audit_conn::Entity::update_many()
        .col_expr(audit_conn::Column::Note, Expr::value(note.to_string()))
        .col_expr(
            audit_conn::Column::UpdatedAt,
            Expr::value(crate::services::now().unwrap()),
        )
        .filter(audit_conn::Column::SessionId.eq(session_id))
        .exec(db)
        .await?;
    Ok(())
}

async fn close_conn_event(db: &DatabaseConnection, event: &ConnEvent) -> Result<(), DbErr> {
    let close_time = if event.close_time > 0 {
        event.close_time
    } else {
        chrono::Utc::now().timestamp()
    };
    audit_conn::Entity::update_many()
        .col_expr(audit_conn::Column::Action, Expr::value("close"))
        .col_expr(audit_conn::Column::CloseTime, Expr::value(close_time))
        .col_expr(
            audit_conn::Column::UpdatedAt,
            Expr::value(crate::services::now().unwrap()),
        )
        .filter(audit_conn::Column::ConnId.eq(event.conn_id))
        .exec(db)
        .await?;
    Ok(())
}

async fn update_conn_peer(db: &DatabaseConnection, event: &ConnEvent) -> Result<(), DbErr> {
    let peer = event.peer.as_ref();
    let from_peer = peer.map(|p| p.id.as_str()).unwrap_or(&event.from_peer);
    let from_name = peer.map(|p| p.name.as_str()).unwrap_or(&event.from_name);

    audit_conn::Entity::update_many()
        .col_expr(
            audit_conn::Column::PeerId,
            Expr::value(event.peer_id.clone()),
        )
        .col_expr(
            audit_conn::Column::FromPeer,
            Expr::value(from_peer.to_string()),
        )
        .col_expr(
            audit_conn::Column::FromName,
            Expr::value(from_name.to_string()),
        )
        .col_expr(
            audit_conn::Column::SessionId,
            Expr::value(event.session_id.clone()),
        )
        .col_expr(audit_conn::Column::Type, Expr::value(event.r#type))
        .col_expr(audit_conn::Column::Uuid, Expr::value(event.uuid.clone()))
        .col_expr(
            audit_conn::Column::UpdatedAt,
            Expr::value(crate::services::now().unwrap()),
        )
        .filter(audit_conn::Column::ConnId.eq(event.conn_id))
        .exec(db)
        .await?;
    Ok(())
}

impl ConnEvent {
    fn from_json(body: &JsonValue) -> Self {
        let peer = parse_peer(body.get("peer"));
        let peer_id = first_non_empty(&[string_field(body, "peer_id"), string_field(body, "id")]);
        let from_peer = first_non_empty(&[
            string_field(body, "from_peer"),
            peer.as_ref().map(|p| p.id.clone()).unwrap_or_default(),
        ]);
        let from_name = first_non_empty(&[
            string_field(body, "from_name"),
            peer.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
        ]);

        Self {
            action: body.get("action").map(value_to_string),
            conn_id: i64_field(body, "conn_id"),
            peer_id,
            from_peer,
            from_name,
            ip: string_field(body, "ip"),
            session_id: string_field(body, "session_id"),
            r#type: i32_field(body, "type"),
            uuid: string_field(body, "uuid"),
            close_time: i64_field(body, "close_time"),
            note: body.get("note").map(value_to_string),
            peer,
        }
    }
}

fn parse_peer(value: Option<&JsonValue>) -> Option<ConnPeer> {
    match value? {
        JsonValue::Array(items) => Some(ConnPeer {
            id: items.first().map(value_to_string).unwrap_or_default(),
            name: items.get(1).map(value_to_string).unwrap_or_default(),
        }),
        value => Some(ConnPeer {
            id: value_to_string(value),
            name: String::new(),
        }),
    }
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find(|v| !v.is_empty())
        .cloned()
        .unwrap_or_default()
}

fn string_field(body: &JsonValue, key: &str) -> String {
    body.get(key).map(value_to_string).unwrap_or_default()
}

fn i64_field(body: &JsonValue, key: &str) -> i64 {
    body.get(key).map(value_to_i64).unwrap_or(0)
}

fn i32_field(body: &JsonValue, key: &str) -> i32 {
    i32::try_from(i64_field(body, key)).unwrap_or(0)
}

fn value_to_string(value: &JsonValue) -> String {
    match value {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn value_to_i64(value: &JsonValue) -> i64 {
    match value {
        JsonValue::Number(n) => n
            .as_i64()
            .or_else(|| n.as_u64().and_then(|v| i64::try_from(v).ok()))
            .unwrap_or(0),
        JsonValue::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{
        ConnectOptions, ConnectionTrait, Database, EntityTrait, PaginatorTrait, QueryOrder, Schema,
    };
    use serde_json::json;

    async fn setup_audit_db() -> DatabaseConnection {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.unwrap();
        let backend = db.get_database_backend();
        let schema = Schema::new(backend);
        let stmt = schema
            .create_table_from_entity(audit_conn::Entity)
            .to_owned();
        db.execute(backend.build(&stmt)).await.unwrap();
        db
    }

    async fn count_conn_rows(db: &DatabaseConnection) -> u64 {
        audit_conn::Entity::find().count(db).await.unwrap()
    }

    async fn only_conn_row(db: &DatabaseConnection) -> audit_conn::Model {
        audit_conn::Entity::find()
            .order_by_asc(audit_conn::Column::Id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn conn_event_updates_a_single_lifecycle_record() {
        let db = setup_audit_db().await;

        record_conn_event(
            &db,
            &json!({
                "action": "new",
                "conn_id": 762,
                "id": "182921366",
                "ip": "103.156.242.225",
                "session_id": 0,
                "uuid": "u-1"
            }),
        )
        .await
        .unwrap();
        assert_eq!(count_conn_rows(&db).await, 1);

        let row = only_conn_row(&db).await;
        assert_eq!(row.action, "new");
        assert_eq!(row.conn_id, 762);
        assert_eq!(row.peer_id, "182921366");
        assert_eq!(row.session_id, "0");

        record_conn_event(
            &db,
            &json!({
                "conn_id": 762,
                "id": "182921366",
                "peer": ["1139987256", "SYSTEM"],
                "session_id": 17409556129324805845u64,
                "type": 0,
                "uuid": "u-1"
            }),
        )
        .await
        .unwrap();
        assert_eq!(count_conn_rows(&db).await, 1);

        let row = only_conn_row(&db).await;
        assert_eq!(row.peer_id, "182921366");
        assert_eq!(row.from_peer, "1139987256");
        assert_eq!(row.from_name, "SYSTEM");
        assert_eq!(row.session_id, "17409556129324805845");
        assert_eq!(row.r#type, 0);

        record_conn_event(
            &db,
            &json!({
                "id": "1139987256",
                "note": "operator note",
                "session_id": 17409556129324805845u64
            }),
        )
        .await
        .unwrap();
        assert_eq!(count_conn_rows(&db).await, 1);

        let row = only_conn_row(&db).await;
        assert_eq!(row.note, "operator note");

        record_conn_event(
            &db,
            &json!({
                "action": "close",
                "conn_id": 762,
                "id": "182921366",
                "session_id": 17409556129324805845u64,
                "uuid": "u-1"
            }),
        )
        .await
        .unwrap();
        assert_eq!(count_conn_rows(&db).await, 1);

        let row = only_conn_row(&db).await;
        assert_eq!(row.action, "close");
        assert!(row.close_time > 0);
    }

    #[test]
    fn conn_event_keeps_large_numeric_session_id_as_string() {
        let event = ConnEvent::from_json(&json!({
            "session_id": 17409556129324805845u64
        }));

        assert_eq!(event.session_id, "17409556129324805845");
    }
}

// ---- AuditFile ----

pub struct AuditFileListResult {
    pub list: Vec<audit_file::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn file_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    peer_id: Option<String>,
    from_peer: Option<String>,
) -> Result<AuditFileListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = audit_file::Entity::find();
    if let Some(p) = peer_id.filter(|s| !s.is_empty()) {
        q = q.filter(audit_file::Column::PeerId.contains(&p));
    }
    if let Some(p) = from_peer.filter(|s| !s.is_empty()) {
        q = q.filter(audit_file::Column::FromPeer.contains(&p));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(audit_file::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(AuditFileListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn file_info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<audit_file::Model>, DbErr> {
    audit_file::Entity::find_by_id(id).one(db).await
}

pub async fn delete_file(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    audit_file::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn batch_delete_file(db: &DatabaseConnection, ids: &[i32]) -> Result<(), DbErr> {
    audit_file::Entity::delete_many()
        .filter(audit_file::Column::Id.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

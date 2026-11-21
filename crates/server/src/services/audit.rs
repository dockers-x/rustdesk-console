//! Audit service — ports `service/audit.go` (conn + file logs).

use sea_orm::sea_query::Expr;
use sea_orm::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use ::entity::{audit_conn, audit_file, peer};

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

pub async fn record_conn_event(db: &impl ConnectionTrait, body: &JsonValue) -> Result<(), DbErr> {
    let event = ConnEvent::from_json(body);

    if let Some(note) = &event.note {
        update_conn_note(db, &event.session_id, note, &event.peer_id).await?;
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

pub async fn active_conn_guid(
    db: &DatabaseConnection,
    peer_id: &str,
    session_id: &str,
    conn_type: Option<i32>,
) -> Result<Option<String>, DbErr> {
    let mut query = audit_conn::Entity::find()
        .filter(audit_conn::Column::PeerId.eq(peer_id))
        .filter(audit_conn::Column::SessionId.eq(session_id));
    if let Some(conn_type) = conn_type {
        query = query.filter(audit_conn::Column::Type.eq(conn_type));
    }
    let row = query.order_by_desc(audit_conn::Column::Id).one(db).await?;
    Ok(row.map(|row| row.id.to_string()))
}

pub async fn update_conn_note_by_guid(
    db: &DatabaseConnection,
    guid: &str,
    note: &str,
    owner_id: Option<i32>,
) -> Result<(), DbErr> {
    let mut query = audit_conn::Entity::update_many()
        .col_expr(audit_conn::Column::Note, Expr::value(note.to_string()))
        .col_expr(
            audit_conn::Column::UpdatedAt,
            Expr::value(crate::services::now()),
        );
    query = match guid.parse::<i32>() {
        Ok(id) => query.filter(audit_conn::Column::Id.eq(id)),
        Err(_) => query.filter(audit_conn::Column::SessionId.eq(guid)),
    };
    if let Some(owner_id) = owner_id {
        let devices = peer::Entity::find()
            .select_only()
            .column(peer::Column::Id)
            .filter(peer::Column::UserId.eq(owner_id))
            .into_tuple::<String>()
            .all(db)
            .await?;
        query = query.filter(
            Condition::any()
                .add(audit_conn::Column::PeerId.is_in(devices.clone()))
                .add(audit_conn::Column::FromPeer.is_in(devices)),
        );
    }
    if query.exec(db).await?.rows_affected == 0 {
        return Err(DbErr::Custom(
            "audit not found or not authorized".to_string(),
        ));
    }
    Ok(())
}

async fn insert_conn_event(db: &impl ConnectionTrait, event: &ConnEvent) -> Result<(), DbErr> {
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
    db: &impl ConnectionTrait,
    session_id: &str,
    note: &str,
    peer_id: &str,
) -> Result<(), DbErr> {
    audit_conn::Entity::update_many()
        .col_expr(audit_conn::Column::Note, Expr::value(note.to_string()))
        .col_expr(
            audit_conn::Column::UpdatedAt,
            Expr::value(crate::services::now().unwrap()),
        )
        .filter(audit_conn::Column::SessionId.eq(session_id))
        .filter(
            Condition::any()
                .add(audit_conn::Column::PeerId.eq(peer_id))
                .add(audit_conn::Column::FromPeer.eq(peer_id)),
        )
        .exec(db)
        .await?;
    Ok(())
}

async fn close_conn_event(db: &impl ConnectionTrait, event: &ConnEvent) -> Result<(), DbErr> {
    if event.peer_id.is_empty() {
        return Err(DbErr::Custom("missing audit device identity".to_string()));
    }
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
        .filter(audit_conn::Column::PeerId.eq(&event.peer_id))
        .filter(
            Condition::all().add_option(
                (!event.uuid.is_empty()).then(|| audit_conn::Column::Uuid.eq(&event.uuid)),
            ),
        )
        .exec(db)
        .await?;
    Ok(())
}

async fn update_conn_peer(db: &impl ConnectionTrait, event: &ConnEvent) -> Result<(), DbErr> {
    if event.peer_id.is_empty() {
        return Err(DbErr::Custom("missing audit device identity".to_string()));
    }
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
        .col_expr(
            audit_conn::Column::Uuid,
            if event.uuid.is_empty() {
                Expr::col(audit_conn::Column::Uuid).into()
            } else {
                Expr::value(event.uuid.clone())
            },
        )
        .col_expr(
            audit_conn::Column::UpdatedAt,
            Expr::value(crate::services::now().unwrap()),
        )
        .filter(audit_conn::Column::ConnId.eq(event.conn_id))
        .filter(audit_conn::Column::PeerId.eq(&event.peer_id))
        .filter(
            Condition::all().add_option(
                (!event.uuid.is_empty()).then(|| audit_conn::Column::Uuid.eq(&event.uuid)),
            ),
        )
        .exec(db)
        .await?;
    Ok(())
}

impl ConnEvent {
    fn from_json(body: &JsonValue) -> Self {
        let peer = parse_peer(body.get("peer"));
        let peer_id = first_non_empty(&[string_field(body, "id"), string_field(body, "peer_id")]);
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

fn bool_field(body: &JsonValue, key: &str) -> bool {
    match body.get(key) {
        Some(JsonValue::Bool(v)) => *v,
        Some(JsonValue::String(v)) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "true" | "y" | "yes" | "1"
        ),
        Some(JsonValue::Number(v)) => v.as_i64().unwrap_or(0) != 0,
        _ => false,
    }
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
        let stmt = schema
            .create_table_from_entity(audit_file::Entity)
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

    async fn count_file_rows(db: &DatabaseConnection) -> u64 {
        audit_file::Entity::find().count(db).await.unwrap()
    }

    async fn only_file_row(db: &DatabaseConnection) -> audit_file::Model {
        audit_file::Entity::find()
            .order_by_asc(audit_file::Column::Id)
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

    #[tokio::test]
    async fn connection_events_only_update_the_reporting_device() {
        let db = setup_audit_db().await;
        for (id, uuid) in [("device-a", "uuid-a"), ("device-b", "uuid-b")] {
            record_conn_event(
                &db,
                &json!({"action": "new", "conn_id": 1, "id": id, "uuid": uuid}),
            )
            .await
            .unwrap();
        }
        record_conn_event(&db, &json!({"conn_id": 1, "id": "device-a", "peer": ["remote", "operator"], "session_id": "session-a"}))
            .await.unwrap();
        record_conn_event(
            &db,
            &json!({"action": "close", "conn_id": 1, "id": "device-a", "uuid": "uuid-a"}),
        )
        .await
        .unwrap();
        let rows = audit_conn::Entity::find()
            .order_by_asc(audit_conn::Column::Id)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(rows[0].action, "close");
        assert_eq!(rows[0].from_peer, "remote");
        assert_eq!(rows[0].session_id, "session-a");
        assert_eq!(rows[1].action, "new");
        assert_eq!(rows[1].uuid, "uuid-b");
        assert!(rows[1].from_peer.is_empty());
        assert!(rows[1].session_id.is_empty());
        for body in [
            json!({"action": "close", "conn_id": 1}),
            json!({"peer": ["remote", "operator"], "conn_id": 1}),
        ] {
            assert!(record_conn_event(&db, &body).await.is_err());
        }
    }

    #[tokio::test]
    async fn active_conn_guid_can_update_note_by_guid() {
        let db = setup_audit_db().await;

        record_conn_event(
            &db,
            &json!({
                "action": "new",
                "conn_id": 762,
                "id": "182921366",
                "session_id": "17409556129324805845",
                "type": 0,
                "uuid": "u-1"
            }),
        )
        .await
        .unwrap();

        let guid = active_conn_guid(&db, "182921366", "17409556129324805845", Some(0))
            .await
            .unwrap()
            .unwrap();
        update_conn_note_by_guid(&db, &guid, "operator note", None)
            .await
            .unwrap();

        let row = only_conn_row(&db).await;
        assert_eq!(row.note, "operator note");
    }

    #[test]
    fn conn_event_keeps_large_numeric_session_id_as_string() {
        let event = ConnEvent::from_json(&json!({
            "session_id": 17409556129324805845u64
        }));

        assert_eq!(event.session_id, "17409556129324805845");
    }

    #[tokio::test]
    async fn file_event_maps_rustdesk_client_fields_like_go_api() {
        let db = setup_audit_db().await;

        record_file_event(
            &db,
            &json!({
                "id": "182921366",
                "uuid": "u-1",
                "peer_id": "1139987256",
                "type": 1,
                "path": "/tmp/report.txt",
                "is_file": true,
                "info": serde_json::json!({
                    "ip": "103.156.242.225",
                    "name": "SYSTEM",
                    "num": 3,
                    "files": [["report.txt", 1024]]
                }).to_string()
            }),
        )
        .await
        .unwrap();

        assert_eq!(count_file_rows(&db).await, 1);
        let row = only_file_row(&db).await;
        assert_eq!(row.peer_id, "182921366");
        assert_eq!(row.from_peer, "1139987256");
        assert_eq!(row.from_name, "SYSTEM");
        assert_eq!(row.ip, "103.156.242.225");
        assert_eq!(row.num, 3);
        assert_eq!(row.path, "/tmp/report.txt");
        assert!(row.is_file);
        assert_eq!(row.r#type, 1);
        let detail = serde_json::to_value(FileAuditRow::from_row(row)).unwrap();
        assert_eq!(detail["direction"], "received");
        assert_eq!(detail["file_count"], 3);
        assert_eq!(detail["size_bytes"], "1024");
        assert_eq!(detail["size_is_partial"], true);
        assert_eq!(
            detail["files"],
            json!([{ "name": "report.txt", "size_bytes": "1024" }])
        );
    }

    #[tokio::test]
    async fn file_audit_distinguishes_reported_zero_and_sparse_legacy_rows() {
        let db = setup_audit_db().await;
        audit_file::ActiveModel {
            peer_id: Set("legacy-device".to_owned()),
            path: Set("/home/legacy.txt".to_owned()),
            r#type: Set(0),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        record_file_event(&db, &json!({ "peer_id": "sparse", "type": "invalid" }))
            .await
            .unwrap();
        record_file_event(
            &db,
            &json!({
                "id": "host",
                "peer_id": "remote",
                "conn_id": 325,
                "type": 0,
                "path": "/home/complete.txt",
                "info": {
                    "num": 1,
                    "files": [["", 9007199254740993u64]],
                },
            }),
        )
        .await
        .unwrap();

        let reports = file_list_with_details(&db, FileSearch::default())
            .await
            .unwrap();
        assert_eq!(reports.total, 3);
        let report = serde_json::to_value(&reports.list[0]).unwrap();
        assert_eq!(report["direction"], "sent");
        assert_eq!(report["conn_id"], 325);
        assert_eq!(report["peer_id"], "host");
        assert_eq!(report["from_peer"], "remote");
        assert_eq!(report["size_bytes"], "9007199254740993");
        assert_eq!(report["size_is_partial"], false);
        assert_eq!(report["files"][0]["size_bytes"], "9007199254740993");
        assert_eq!(reports.list[1].row.peer_id, "sparse");
        assert_eq!(reports.list[1].row.from_peer, "");
        for legacy in &reports.list[1..] {
            let legacy = serde_json::to_value(legacy).unwrap();
            assert!(legacy["direction"].is_null());
            assert!(legacy["file_count"].is_null());
            assert!(legacy["size_bytes"].is_null());
        }
        let filtered = file_list_with_details(
            &db,
            FileSearch {
                path: Some("/home".to_owned()),
                direction: Some(FileDirection::Sent),
                ..FileSearch::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.list[0].row.peer_id, "host");

        let pro = file_list(&db, 1, 10, Some("host".to_owned()), None)
            .await
            .unwrap();
        let raw = serde_json::to_value(&pro.list[0]).unwrap();
        assert_eq!(raw["type"], 0);
        assert!(raw["info"].is_string());
        assert!(raw.get("conn_id").is_none());
        assert!(raw.get("direction").is_none());
    }
}

// ---- AuditFile ----

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileDirection {
    Sent,
    Received,
}

#[derive(Debug, Default, Deserialize)]
pub struct FileSearch {
    page: Option<u64>,
    page_size: Option<u64>,
    peer_id: Option<String>,
    from_peer: Option<String>,
    path: Option<String>,
    direction: Option<FileDirection>,
}

#[derive(Serialize)]
pub struct FileAuditRow {
    #[serde(flatten)]
    row: audit_file::Model,
    direction: Option<FileDirection>,
    conn_id: Option<i32>,
    file_count: Option<u64>,
    size_bytes: Option<String>,
    size_is_partial: bool,
    files: Vec<FileAuditEntry>,
}

#[derive(Serialize)]
struct FileAuditEntry {
    name: String,
    size_bytes: Option<String>,
}

impl FileAuditRow {
    pub(crate) fn peer_ids(&self) -> [&str; 2] {
        [&self.row.peer_id, &self.row.from_peer]
    }

    fn from_row(row: audit_file::Model) -> Self {
        let info = serde_json::from_str::<JsonValue>(&row.info).unwrap_or(JsonValue::Null);
        let file_count = if row.num > 0 {
            u64::try_from(row.num).ok()
        } else {
            unsigned_json_number(&info["num"])
        };
        let direction = match (row.reported_type, row.r#type) {
            (Some(0), _) => Some(FileDirection::Sent),
            (Some(1), _) | (None, 1) => Some(FileDirection::Received),
            // Older rows use zero both for sent files and a missing type.
            _ => None,
        };
        let entries = info["files"].as_array();
        let mut files = Vec::new();
        let mut sizes = 0u64;
        let mut count_with_size = 0;
        let mut size_overflow = false;
        if let Some(entries) = entries {
            for entry in entries.iter().take(10) {
                let Some(name) = entry.get(0).and_then(JsonValue::as_str) else {
                    continue;
                };
                let size_bytes = entry.get(1).and_then(unsigned_json_number);
                if let Some(size) = size_bytes {
                    if let Some(sum) = sizes.checked_add(size) {
                        sizes = sum;
                    } else {
                        size_overflow = true;
                    }
                    count_with_size += 1;
                }
                files.push(FileAuditEntry {
                    name: name.to_owned(),
                    size_bytes: size_bytes.map(|size| size.to_string()),
                });
            }
        }
        let size_is_partial = file_count != Some(count_with_size)
            || entries.is_none_or(|raw| raw.len() != files.len());
        let size_bytes = (!size_overflow
            && (count_with_size > 0 || (!size_is_partial && file_count == Some(0))))
        .then(|| sizes.to_string());
        Self {
            conn_id: row.conn_id,
            row,
            direction,
            file_count,
            size_bytes,
            size_is_partial,
            files,
        }
    }
}

fn unsigned_json_number(value: &JsonValue) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

pub async fn file_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    peer_id: Option<String>,
    from_peer: Option<String>,
) -> Result<::entity::ListResult<audit_file::Model>, DbErr> {
    find_file_reports(
        db,
        FileSearch {
            page: Some(page),
            page_size: Some(page_size),
            peer_id,
            from_peer,
            ..FileSearch::default()
        },
    )
    .await
}

pub async fn file_list_with_details(
    db: &DatabaseConnection,
    search: FileSearch,
) -> Result<::entity::ListResult<FileAuditRow>, DbErr> {
    let rows = find_file_reports(db, search).await?;
    let list = rows.list.into_iter().map(FileAuditRow::from_row).collect();
    Ok(::entity::ListResult::new(
        list,
        rows.page,
        rows.page_size,
        rows.total,
    ))
}

async fn find_file_reports(
    db: &DatabaseConnection,
    search: FileSearch,
) -> Result<::entity::ListResult<audit_file::Model>, DbErr> {
    let (page, page_size) = paginate(
        search.page.unwrap_or_default(),
        search.page_size.unwrap_or_default(),
    );
    let mut q = audit_file::Entity::find();
    if let Some(p) = search.peer_id.filter(|s| !s.is_empty()) {
        q = q.filter(audit_file::Column::PeerId.contains(&p));
    }
    if let Some(p) = search.from_peer.filter(|s| !s.is_empty()) {
        q = q.filter(audit_file::Column::FromPeer.contains(&p));
    }
    if let Some(path) = search.path.filter(|s| !s.is_empty()) {
        q = q.filter(audit_file::Column::Path.contains(&path));
    }
    if let Some(direction) = search.direction {
        q = match direction {
            FileDirection::Sent => q.filter(audit_file::Column::ReportedType.eq(0)),
            FileDirection::Received => q.filter(
                Condition::any()
                    .add(audit_file::Column::ReportedType.eq(1))
                    .add(
                        Condition::all()
                            .add(audit_file::Column::ReportedType.is_null())
                            .add(audit_file::Column::Type.eq(1)),
                    ),
            ),
        };
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(audit_file::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(::entity::ListResult::new(
        list,
        page as i64,
        page_size as i64,
        total,
    ))
}

pub async fn record_file_event(db: &impl ConnectionTrait, body: &JsonValue) -> Result<(), DbErr> {
    let event = FileEvent::from_json(body);
    let now = crate::services::now();
    let am = audit_file::ActiveModel {
        from_peer: Set(event.from_peer),
        info: Set(event.info),
        is_file: Set(event.is_file),
        path: Set(event.path),
        peer_id: Set(event.peer_id),
        r#type: Set(event.r#type),
        reported_type: Set(event.reported_type),
        conn_id: Set(event.conn_id),
        uuid: Set(event.uuid),
        ip: Set(event.ip),
        num: Set(event.num),
        from_name: Set(event.from_name),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    am.insert(db).await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileEvent {
    peer_id: String,
    from_peer: String,
    info: String,
    is_file: bool,
    path: String,
    r#type: i32,
    reported_type: Option<i32>,
    conn_id: Option<i32>,
    uuid: String,
    ip: String,
    num: i32,
    from_name: String,
}

impl FileEvent {
    fn from_json(body: &JsonValue) -> Self {
        let info = match &body["info"] {
            info @ JsonValue::Object(_) => info.to_string(),
            info => value_to_string(info),
        };
        let info_json = serde_json::from_str::<JsonValue>(&info).unwrap_or(JsonValue::Null);
        let body_num = i32_field(body, "num");
        let info_num = i32_field(&info_json, "num");
        let report_id = string_field(body, "id");
        let peer_id = string_field(body, "peer_id");
        let from_peer = string_field(body, "from_peer");
        // Stock RustDesk supplies a reporting ID in `id` and a remote ID in `peer_id`.
        // Older callers can supply the reporting ID as `peer_id` and the remote as `from_peer`.
        let (peer_id, from_peer) = if report_id.is_empty() {
            (peer_id, from_peer)
        } else if from_peer.is_empty() {
            (report_id, peer_id)
        } else {
            (report_id, from_peer)
        };
        let optional_i32 = |field: &JsonValue| {
            field
                .as_i64()
                .or_else(|| field.as_str()?.parse().ok())
                .and_then(|value| i32::try_from(value).ok())
        };
        let reported_type = optional_i32(&body["type"]);

        Self {
            peer_id,
            from_peer,
            info,
            is_file: bool_field(body, "is_file"),
            path: string_field(body, "path"),
            r#type: reported_type.unwrap_or_default(),
            reported_type,
            conn_id: optional_i32(&body["conn_id"]),
            uuid: string_field(body, "uuid"),
            ip: first_non_empty(&[string_field(body, "ip"), string_field(&info_json, "ip")]),
            num: if body_num != 0 { body_num } else { info_num },
            from_name: first_non_empty(&[
                string_field(body, "from_name"),
                string_field(&info_json, "name"),
            ]),
        }
    }
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

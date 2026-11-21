//! Audit service — ports `service/audit.go` (conn + file logs).

use chrono::{LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::{GapInfo, Tz};
use futures_util::{Stream, StreamExt};
use sea_orm::sea_query::{Alias, Expr, LikeExpr, Query, SimpleExpr};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use ::entity::{active_connection, audit_conn, audit_file, peer};

use crate::services::paginate;

// ---- AuditConn ----

pub struct AuditConnListResult {
    pub list: Vec<audit_conn::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ConnSearch {
    page: Option<u64>,
    page_size: Option<u64>,
    peer_id: Option<String>,
    from_peer: Option<String>,
    q: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

/// The site uses an IANA time zone from TZ. Standalone installs default to UTC.
pub(crate) fn audit_timezone() -> Tz {
    std::env::var("TZ")
        .ok()
        .and_then(|name| name.trim().parse().ok())
        .unwrap_or(Tz::UTC)
}

fn parse_date(value: Option<&str>) -> Result<Option<NaiveDate>, &'static str> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        if value.len() == 10 && value == date.format("%Y-%m-%d").to_string() {
            return Ok(Some(date));
        }
    }
    Err("AuditDateFormat")
}

fn start_of_day(date: NaiveDate, zone: Tz) -> Result<NaiveDateTime, &'static str> {
    let midnight = date.and_time(chrono::NaiveTime::MIN);
    match zone.from_local_datetime(&midnight) {
        LocalResult::Single(time) => Some(time),
        LocalResult::Ambiguous(first, second) => Some(first.min(second)),
        LocalResult::None => GapInfo::new(&midnight, &zone).and_then(|gap| gap.end),
    }
    .map(|time| time.naive_utc())
    .ok_or("AuditDateFormat")
}

impl ConnSearch {
    pub(crate) fn query(&self, zone: Tz) -> Result<Select<audit_conn::Entity>, &'static str> {
        let from = parse_date(self.from.as_deref())?;
        let to = parse_date(self.to.as_deref())?;
        if matches!((from, to), (Some(from), Some(to)) if from > to) {
            return Err("AuditDateOrder");
        }
        let mut query = conn_query(self.peer_id.as_deref(), self.from_peer.as_deref());
        if let Some(from) = from {
            query = query.filter(audit_conn::Column::CreatedAt.gte(start_of_day(from, zone)?));
        }
        if let Some(to) = to {
            let next = to.succ_opt().ok_or("AuditDateFormat")?;
            query = query.filter(audit_conn::Column::CreatedAt.lt(start_of_day(next, zone)?));
        }
        if let Some(keyword) = self.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
            let literal = keyword
                .replace('!', "!!")
                .replace('%', "!%")
                .replace('_', "!_");
            let pattern = LikeExpr::new(format!("%{literal}%")).escape('!');
            let matching = [
                audit_conn::Column::PeerId,
                audit_conn::Column::FromPeer,
                audit_conn::Column::FromName,
                audit_conn::Column::Ip,
                audit_conn::Column::Note,
            ]
            .into_iter()
            .fold(Condition::any(), |condition, column| {
                condition.add(column.like(pattern.clone()))
            });
            query = query.filter(matching);
        }
        Ok(query)
    }
}

fn conn_query(peer_id: Option<&str>, from_peer: Option<&str>) -> Select<audit_conn::Entity> {
    let mut query = audit_conn::Entity::find();
    if let Some(peer_id) = peer_id.filter(|id| !id.is_empty()) {
        query = query.filter(audit_conn::Column::PeerId.contains(peer_id));
    }
    if let Some(from_peer) = from_peer.filter(|id| !id.is_empty()) {
        query = query.filter(audit_conn::Column::FromPeer.contains(from_peer));
    }
    query
}

#[derive(Debug)]
struct ConnReport {
    row: audit_conn::Model,
    has_recent_snapshot: i32,
}

impl FromQueryResult for ConnReport {
    fn from_query_result(result: &QueryResult, prefix: &str) -> Result<Self, DbErr> {
        Ok(Self {
            row: audit_conn::Model::from_query_result(result, prefix)?,
            has_recent_snapshot: result.try_get(prefix, "has_recent_snapshot")?,
        })
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ConnAuditRow {
    #[serde(flatten)]
    pub row: audit_conn::Model,
    end_status: &'static str,
    duration_seconds: Option<i64>,
    #[serde(skip)]
    closed_at: Option<NaiveDateTime>,
}

impl From<ConnReport> for ConnAuditRow {
    fn from(report: ConnReport) -> Self {
        let row = report.row;
        let closed_at = chrono::DateTime::from_timestamp(row.close_time, 0)
            .filter(|time| time.timestamp() > 0)
            .map(|time| time.naive_utc());
        let duration_seconds = row.created_at.and_then(|start| {
            // Close events have second precision. A same-second close lasts zero seconds.
            let seconds = closed_at?.and_utc().timestamp() - start.and_utc().timestamp();
            (seconds >= 0).then_some(seconds)
        });
        let end_status = if closed_at.is_some() {
            "closed"
        } else if report.has_recent_snapshot == 1 {
            "active"
        } else {
            "unknown"
        };
        Self {
            row,
            end_status,
            duration_seconds,
            closed_at,
        }
    }
}

fn conn_reports(query: Select<audit_conn::Entity>) -> Select<audit_conn::Entity> {
    use active_connection::Column as Snapshot;
    use audit_conn::Column as Audit;

    let prior = Alias::new("newer_audit_conn");
    let source = |column| (audit_conn::Entity, column);
    let newer = Query::select()
        .from_as(audit_conn::Entity, prior.clone())
        .expr(Expr::value(1))
        .and_where(Expr::col((prior.clone(), Audit::PeerId)).equals(source(Audit::PeerId)))
        .and_where(Expr::col((prior.clone(), Audit::Uuid)).equals(source(Audit::Uuid)))
        .and_where(Expr::col((prior.clone(), Audit::ConnId)).equals(source(Audit::ConnId)))
        .and_where(Expr::col((prior, Audit::Id)).gt(Expr::col(source(Audit::Id))))
        .to_owned();
    let fresh_snapshot = Query::select()
        .from(active_connection::Entity)
        .expr(Expr::value(1))
        .and_where(
            Expr::col((active_connection::Entity, Snapshot::PeerId)).equals(source(Audit::PeerId)),
        )
        .and_where(
            Expr::col((active_connection::Entity, Snapshot::Uuid)).equals(source(Audit::Uuid)),
        )
        .and_where(
            Expr::col((active_connection::Entity, Snapshot::ConnId)).equals(source(Audit::ConnId)),
        )
        .and_where(Snapshot::UpdatedAt.gte(super::active_connection::freshness_cutoff()))
        .and_where(
            Expr::col((active_connection::Entity, Snapshot::UpdatedAt))
                .gte(Expr::col(source(Audit::CreatedAt))),
        )
        .to_owned();
    let recent = Condition::all()
        .add(Audit::CloseTime.lte(0))
        .add(Audit::ConnId.gt(0))
        .add(Audit::PeerId.ne(""))
        .add(Audit::Uuid.ne(""))
        .add(Expr::exists(fresh_snapshot))
        .add(Expr::exists(newer).not());
    let has_recent_snapshot: SimpleExpr = Expr::case(recent, 1_i32).finally(0_i32).into();
    query.column_as(has_recent_snapshot, "has_recent_snapshot")
}

pub(crate) async fn conn_list_with_details(
    db: &DatabaseConnection,
    query: Select<audit_conn::Entity>,
    search: &ConnSearch,
) -> Result<::entity::ListResult<ConnAuditRow>, DbErr> {
    let (page, page_size) = paginate(
        search.page.unwrap_or_default(),
        search.page_size.unwrap_or_default(),
    );
    let total = query.clone().count(db).await? as i64;
    let reports = conn_reports(query)
        .order_by_desc(audit_conn::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .into_model::<ConnReport>()
        .all(db)
        .await?;
    Ok(::entity::ListResult::new(
        reports.into_iter().map(ConnAuditRow::from).collect(),
        page as i64,
        page_size as i64,
        total,
    ))
}

pub async fn conn_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    peer_id: Option<String>,
    from_peer: Option<String>,
) -> Result<AuditConnListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let q = conn_query(peer_id.as_deref(), from_peer.as_deref());
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
        let stmt = schema
            .create_table_from_entity(active_connection::Entity)
            .to_owned();
        db.execute(backend.build(&stmt)).await.unwrap();
        db
    }

    #[tokio::test]
    async fn connection_list_and_csv_share_keyword_and_inclusive_local_day_filters() {
        let db = setup_audit_db().await;
        let note = " =中文,\"原文\"\n第二行";
        for (id, timestamp, target, source, name) in [
            (1, "2026-03-08 04:59:59", "target-a", "source-a", "Operator"),
            (2, "2026-03-08 05:00:00", "target-a", "source-a", "Operator"),
            (3, "2026-03-09 03:59:59", "target-a", "source-a", "Operator"),
            (4, "2026-03-09 04:00:00", "target-a", "source-a", "Operator"),
            (5, "2026-03-08 16:00:00", "target-b", "source-a", "Operator"),
            (6, "2026-03-08 16:00:00", "target-a", "source-b", "Operator"),
            (7, "2026-03-08 16:00:00", "target-a", "source-a", "Other"),
        ] {
            let start =
                NaiveDateTime::parse_from_str(timestamp, ::entity::datetime::FORMAT).unwrap();
            audit_conn::ActiveModel {
                id: Set(id),
                peer_id: Set(target.to_owned()),
                from_peer: Set(source.to_owned()),
                from_name: Set(name.to_owned()),
                ip: Set("203.0.113.14".to_owned()),
                created_at: Set(Some(start)),
                close_time: Set(start.and_utc().timestamp() + 60),
                note: Set(if id != 7 { note } else { "other 50%_!" }.to_owned()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let zone = chrono_tz::America::New_York;
        let search = ConnSearch {
            page: Some(2),
            page_size: Some(1),
            peer_id: Some("target-a".into()),
            from_peer: Some("source-a".into()),
            q: Some("中文".into()),
            from: Some("2026-03-08".into()),
            to: Some("2026-03-08".into()),
        };
        let query = search.query(zone).unwrap();
        let listed = conn_list_with_details(&db, query.clone(), &search)
            .await
            .unwrap();
        assert_eq!(listed.total, 2);
        assert_eq!(listed.list[0].row.id, 2);
        assert_eq!(listed.list[0].duration_seconds, Some(60));
        assert_eq!(listed.list[0].end_status, "closed");
        let exported = conn_export(&db, query).await.unwrap();
        let exported = exported.split_once("\r\n").unwrap().1;
        assert!(exported.starts_with("\"3\","));
        assert!(exported.contains("\r\n\"2\","));
        assert!(exported.contains("\"2026-03-08T05:00:00Z\",\"2026-03-08T05:01:00Z\",\"60\",\"closed\",\"' =中文,\"\"原文\"\"\n第二行\""));
        assert_eq!(exported.matches("\r\n").count(), 2);

        for (keyword, total) in [
            ("target-b", 1),
            ("source-b", 1),
            ("Operator", 6),
            ("203.0.113.14", 7),
            ("%_!", 1),
        ] {
            let search = ConnSearch {
                q: Some(keyword.into()),
                ..ConnSearch::default()
            };
            let query = search.query(zone).unwrap();
            assert_eq!(query.count(&db).await.unwrap(), total, "{keyword}");
        }
    }

    #[test]
    fn connection_date_filters_validate_dates_and_handle_a_skipped_day() {
        let zone = chrono_tz::Pacific::Apia;
        for invalid in ["2026-1-01", "2026-01-32", "2026-01-01T00:00:00Z"] {
            let search = ConnSearch {
                from: Some(invalid.into()),
                ..ConnSearch::default()
            };
            assert_eq!(search.query(zone).unwrap_err(), "AuditDateFormat");
        }
        let backwards = ConnSearch {
            from: Some("2026-04-01".into()),
            to: Some("2026-03-31".into()),
            ..ConnSearch::default()
        };
        assert_eq!(backwards.query(zone).unwrap_err(), "AuditDateOrder");
        let skipped = NaiveDate::from_ymd_opt(2011, 12, 30).unwrap();
        assert_eq!(
            start_of_day(skipped, zone).unwrap(),
            start_of_day(skipped.succ_opt().unwrap(), zone).unwrap()
        );
    }

    #[tokio::test]
    async fn connection_list_requires_a_fresh_matching_snapshot_for_active_status() {
        let db = setup_audit_db().await;
        for (conn_id, uuid) in [
            (1, "peer-uuid"),
            (1, "peer-uuid"),
            (2, "peer-uuid"),
            (3, "wrong-uuid"),
        ] {
            record_conn_event(
                &db,
                &json!({"action":"new","conn_id":conn_id,"id":"target","uuid":uuid}),
            )
            .await
            .unwrap();
        }
        super::super::active_connection::sync_for_device(&db, "target", "peer-uuid", &[1, 2, 3])
            .await
            .unwrap();
        active_connection::Entity::update_many()
            .filter(active_connection::Column::ConnId.eq(2))
            .col_expr(
                active_connection::Column::UpdatedAt,
                Expr::value(
                    super::super::active_connection::freshness_cutoff()
                        - chrono::TimeDelta::seconds(1),
                ),
            )
            .exec(&db)
            .await
            .unwrap();
        let search = ConnSearch::default();
        let listed = conn_list_with_details(&db, search.query(Tz::UTC).unwrap(), &search)
            .await
            .unwrap();
        let status: Vec<_> = listed.list.iter().map(|report| report.end_status).collect();
        assert_eq!(status, ["unknown", "unknown", "active", "unknown"]);
    }

    #[tokio::test]
    async fn csv_rejects_exports_above_the_row_or_byte_limit() {
        let rows = futures_util::stream::iter((0..=EXPORT_MAX_ROWS).map(Ok));
        assert!(matches!(
            export_csv(rows, &["test"], |csv, _| csv.write_row(["row"])).await,
            Err(ExportError::Limit)
        ));
        let rows = futures_util::stream::iter([Ok("a".repeat(EXPORT_MAX_BYTES))]);
        assert!(matches!(
            export_csv(rows, &["test"], |csv, data| csv.write_row([data.as_str()])).await,
            Err(ExportError::Limit)
        ));
    }

    #[test]
    fn csv_quotes_columns_and_protects_spreadsheet_formula_prefixes() {
        let mut csv = CsvDocument(String::new());
        csv.write_row([
            "=1",
            "+1",
            "-1",
            "@1",
            "\tword",
            "\rword",
            "\nword",
            " \u{feff}＝1",
            "中文,\"引号\"\n下一行",
        ])
        .unwrap();
        assert_eq!(csv.0, "\"'=1\",\"'+1\",\"'-1\",\"'@1\",\"'\tword\",\"'\rword\",\"'\nword\",\"' \u{feff}＝1\",\"中文,\"\"引号\"\"\n下一行\"\r\n");
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

        let mut row = only_conn_row(&db).await;
        assert_eq!(row.action, "close");
        assert!(row.close_time > 0);
        row.created_at = chrono::DateTime::from_timestamp(row.close_time, 500_000_000)
            .map(|time| time.naive_utc());
        let report = ConnAuditRow::from(ConnReport {
            row,
            has_recent_snapshot: 0,
        });
        assert_eq!(report.duration_seconds, Some(0));
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
        let search = FileSearch {
            direction: Some(FileDirection::Received),
            peer_id: Some("182921".to_owned()),
            from_peer: Some("113998".to_owned()),
            path: Some("report".to_owned()),
            ..FileSearch::default()
        };
        let exported = file_export(&db, &search).await.unwrap();
        assert!(exported
            .contains("\"/tmp/report.txt\",\"true\",\"received\",\"3\",\"1024\",\"partial\""));
        assert_eq!(file_list_with_details(&db, search).await.unwrap().total, 1);
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
        let export = file_export(&db, &FileSearch::default()).await.unwrap();
        assert!(export.contains("\"unknown\",\"\",\"\",\"unknown\""));
        assert!(export.contains("\"sent\",\"1\",\"9007199254740993\",\"complete\""));
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

impl FileSearch {
    fn query(&self) -> Select<audit_file::Entity> {
        let mut query = audit_file::Entity::find();
        if let Some(id) = self.peer_id.as_deref().filter(|id| !id.is_empty()) {
            query = query.filter(audit_file::Column::PeerId.contains(id));
        }
        if let Some(id) = self.from_peer.as_deref().filter(|id| !id.is_empty()) {
            query = query.filter(audit_file::Column::FromPeer.contains(id));
        }
        if let Some(path) = self.path.as_deref().filter(|path| !path.is_empty()) {
            query = query.filter(audit_file::Column::Path.contains(path));
        }
        match self.direction {
            Some(FileDirection::Sent) => query.filter(audit_file::Column::ReportedType.eq(0)),
            Some(FileDirection::Received) => query.filter(
                Condition::any()
                    .add(audit_file::Column::ReportedType.eq(1))
                    .add(
                        Condition::all()
                            .add(audit_file::Column::ReportedType.is_null())
                            .add(audit_file::Column::Type.eq(1)),
                    ),
            ),
            None => query,
        }
    }
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
    let q = search.query();
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

const EXPORT_MAX_ROWS: u64 = 1_000;
const EXPORT_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ExportError {
    #[error(transparent)]
    Database(#[from] DbErr),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("audit export exceeds its size limit")]
    Limit,
}

struct CsvDocument(String);

impl CsvDocument {
    fn write_row<'a>(
        &mut self,
        fields: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), ExportError> {
        for (index, value) in fields.into_iter().enumerate() {
            let significant = value.trim_start_matches(|c: char| {
                c.is_whitespace() || c.is_control() || c == '\u{feff}'
            });
            let protect = value.starts_with(['\t', '\r', '\n'])
                || significant.starts_with(['=', '+', '-', '@', '＝', '＋', '－', '＠']);
            let quoted_length =
                value.len() + value.as_bytes().iter().filter(|&&b| b == b'"').count();
            if self.0.len() + quoted_length + usize::from(protect) + 5 > EXPORT_MAX_BYTES {
                return Err(ExportError::Limit);
            }
            if index != 0 {
                self.0.push(',');
            }
            self.0.push('"');
            if protect {
                self.0.push('\'');
            }
            for part in value.split_inclusive('"') {
                self.0.push_str(part);
                if part.ends_with('"') {
                    self.0.push('"');
                }
            }
            self.0.push('"');
        }
        self.0.push_str("\r\n");
        Ok(())
    }
}

async fn export_csv<T>(
    rows: impl Stream<Item = Result<T, DbErr>>,
    header: &[&str],
    write: impl Fn(&mut CsvDocument, T) -> Result<(), ExportError>,
) -> Result<String, ExportError> {
    futures_util::pin_mut!(rows);
    let mut csv = CsvDocument(String::from("\u{feff}"));
    csv.write_row(header.iter().copied())?;
    let mut count = 0;
    while let Some(row) = rows.next().await {
        let row = row?;
        if count == EXPORT_MAX_ROWS {
            return Err(ExportError::Limit);
        }
        write(&mut csv, row)?;
        count += 1;
    }
    Ok(csv.0)
}

fn csv_time(value: Option<NaiveDateTime>) -> String {
    value
        .map(|time| {
            time.and_utc()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        })
        .unwrap_or_default()
}

pub(crate) async fn conn_export(
    db: &DatabaseConnection,
    query: Select<audit_conn::Entity>,
) -> Result<String, ExportError> {
    let rows = conn_reports(query)
        .order_by_desc(audit_conn::Column::Id)
        .limit(EXPORT_MAX_ROWS + 1)
        .into_model::<ConnReport>()
        .stream(db)
        .await?;
    export_csv(
        rows,
        &[
            "id",
            "peer_id",
            "from_peer",
            "from_name",
            "ip",
            "created_at_utc",
            "closed_at_utc",
            "duration_seconds",
            "end_status",
            "note",
            "action",
            "conn_id",
            "session_id",
            "type",
            "uuid",
            "updated_at_utc",
        ],
        |csv, report| {
            let report = ConnAuditRow::from(report);
            let row = report.row;
            csv.write_row([
                row.id.to_string().as_str(),
                &row.peer_id,
                &row.from_peer,
                &row.from_name,
                &row.ip,
                &csv_time(row.created_at),
                &csv_time(report.closed_at),
                &report
                    .duration_seconds
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                report.end_status,
                &row.note,
                &row.action,
                &row.conn_id.to_string(),
                &row.session_id,
                &row.r#type.to_string(),
                &row.uuid,
                &csv_time(row.updated_at),
            ])
        },
    )
    .await
}

pub(crate) async fn file_export(
    db: &DatabaseConnection,
    search: &FileSearch,
) -> Result<String, ExportError> {
    let rows = search
        .query()
        .order_by_desc(audit_file::Column::Id)
        .limit(EXPORT_MAX_ROWS + 1)
        .stream(db)
        .await?;
    export_csv(
        rows,
        &[
            "id",
            "peer_id",
            "from_peer",
            "from_name",
            "ip",
            "created_at_utc",
            "path",
            "is_file",
            "direction",
            "file_count",
            "size_bytes",
            "size_status",
            "reported_files",
            "conn_id",
            "uuid",
            "updated_at_utc",
        ],
        |csv, row| {
            let report = FileAuditRow::from_row(row);
            let row = report.row;
            let direction = match report.direction {
                Some(FileDirection::Sent) => "sent",
                Some(FileDirection::Received) => "received",
                None => "unknown",
            };
            let size_status = match &report.size_bytes {
                None => "unknown",
                Some(_) if report.size_is_partial => "partial",
                Some(_) => "complete",
            };
            csv.write_row([
                row.id.to_string().as_str(),
                &row.peer_id,
                &row.from_peer,
                &row.from_name,
                &row.ip,
                &csv_time(row.created_at),
                &row.path,
                &row.is_file.to_string(),
                direction,
                &report.file_count.map(|v| v.to_string()).unwrap_or_default(),
                report.size_bytes.as_deref().unwrap_or_default(),
                size_status,
                &serde_json::to_string(&report.files)?,
                &report.conn_id.map(|v| v.to_string()).unwrap_or_default(),
                &row.uuid,
                &csv_time(row.updated_at),
            ])
        },
    )
    .await
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

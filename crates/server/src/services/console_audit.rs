//! Persisted management history and protected, bounded retention cleanup.

use ::entity::{
    active_connection, audit_conn, audit_file, console_audit, login_log, record_file, user_token,
};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use sea_orm::{
    sea_query::{Expr, IntoCondition, Query},
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};

use super::{console_roles, paginate};

const SETTING_KEY: &str = "console_audit_retention";
const BATCH_SIZE: u64 = 100;

pub(crate) async fn record(
    db: &impl ConnectionTrait,
    action: &'static str,
    result: &'static str,
    actor_id: i32,
    subject_id: Option<i32>,
) -> Result<(), DbErr> {
    console_audit::ActiveModel {
        action: Set(action.to_owned()),
        result: Set(result.to_owned()),
        actor_id: Set(actor_id),
        subject_id: Set(subject_id.filter(|id| *id > 0)),
        created_at: Set(Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Search {
    #[serde(default)]
    page: u64,
    #[serde(default)]
    page_size: u64,
    action: Option<String>,
    result: Option<String>,
    actor_id: Option<i32>,
    subject_id: Option<i32>,
}

pub(crate) async fn list(
    db: &impl ConnectionTrait,
    search: Search,
) -> Result<::entity::ListResult<console_audit::Model>, DbErr> {
    let (page, size) = paginate(search.page, search.page_size.min(100));
    let mut query = console_audit::Entity::find();
    if let Some(action) = search.action.filter(|s| !s.trim().is_empty()) {
        query = query.filter(console_audit::Column::Action.contains(action.trim()));
    }
    if let Some(result) = search.result.filter(|s| !s.is_empty()) {
        query = query.filter(console_audit::Column::Result.eq(result));
    }
    if let Some(id) = search.actor_id {
        query = query.filter(console_audit::Column::ActorId.eq(id));
    }
    if let Some(id) = search.subject_id {
        query = query.filter(console_audit::Column::SubjectId.eq(id));
    }
    let total = query.clone().count(db).await? as i64;
    let rows = query
        .order_by_desc(console_audit::Column::CreatedAt)
        .order_by_desc(console_audit::Column::Id)
        .offset((page - 1) * size)
        .limit(size)
        .all(db)
        .await?;
    Ok(::entity::ListResult::new(
        rows,
        page as i64,
        size as i64,
        total,
    ))
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Retention {
    /// None disables cleanup entirely.
    pub days: Option<u16>,
}

pub(crate) async fn retention(db: &impl ConnectionTrait) -> Result<Retention, DbErr> {
    Ok(console_roles::setting(db, SETTING_KEY)
        .await?
        .unwrap_or_default())
}

pub(crate) async fn set_retention(
    db: &sea_orm::DatabaseConnection,
    settings: Retention,
) -> Result<(), DbErr> {
    if settings
        .days
        .is_some_and(|days| !(1..=3650).contains(&days))
    {
        return Err(DbErr::Custom(
            "Retention days must be between 1 and 3650".to_owned(),
        ));
    }
    let transaction = console_roles::authority_transaction(db).await?;
    let value = serde_json::to_string(&settings).map_err(|err| DbErr::Custom(err.to_string()))?;
    console_roles::write_setting(&transaction, SETTING_KEY.to_owned(), value).await?;
    transaction.commit().await
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct RetentionResult {
    cutoff: Option<DateTime<Utc>>,
    console: u64,
    login: u64,
    connection: u64,
    file: u64,
}

fn linked_peer_events<T: ColumnTrait>(peer_id: T, from_peer: T) -> Condition {
    // Recordings only carry a peer ID. Preserve all rows for a known
    // participant while any recording or active connection refers to it.
    let mut condition = Condition::all()
        .add(peer_id.ne(""))
        .add(from_peer.is_not_null());
    for column in [peer_id, from_peer] {
        let active = Query::select()
            .from(active_connection::Entity)
            .expr(Expr::value(1))
            .and_where(
                Expr::col((active_connection::Entity, active_connection::Column::PeerId))
                    .equals((column.entity_name(), column)),
            )
            .to_owned();
        let recordings = Query::select()
            .from(record_file::Entity)
            .expr(Expr::value(1))
            .and_where(
                Expr::col((record_file::Entity, record_file::Column::PeerId))
                    .equals((column.entity_name(), column)),
            )
            .to_owned();
        condition = condition
            .add(Expr::exists(active).not())
            .add(Expr::exists(recordings).not());
    }
    condition
}

fn unclosed_connections() -> Condition {
    let mut protection = Condition::all();
    for column in [audit_file::Column::PeerId, audit_file::Column::FromPeer] {
        let file_side = Expr::col((audit_file::Entity, column));
        let open = Query::select()
            .from(audit_conn::Entity)
            .expr(Expr::value(1))
            .and_where(audit_conn::Column::CloseTime.eq(0))
            .cond_where(
                Condition::any()
                    .add(
                        file_side
                            .clone()
                            .equals((audit_conn::Entity, audit_conn::Column::PeerId)),
                    )
                    .add(file_side.equals((audit_conn::Entity, audit_conn::Column::FromPeer))),
            )
            .to_owned();
        protection = protection.add(
            Condition::any()
                .add(column.eq(""))
                .add(Expr::exists(open).not()),
        );
    }
    protection
}

fn active_logins(now: i64) -> sea_orm::sea_query::SelectStatement {
    Query::select()
        .from(user_token::Entity)
        .column(user_token::Column::Id)
        .and_where(user_token::Column::ExpiredAt.gt(now))
        .to_owned()
}

struct Candidates {
    console: Condition,
    login: Condition,
    connection: Condition,
    file: Condition,
}

fn candidates(cutoff: NaiveDateTime, now: i64) -> Candidates {
    Candidates {
        console: console_audit::Column::CreatedAt.lt(cutoff).into_condition(),
        login: Condition::all()
            .add(login_log::Column::CreatedAt.lt(cutoff))
            .add(login_log::Column::UserTokenId.not_in_subquery(active_logins(now))),
        connection: linked_peer_events(audit_conn::Column::PeerId, audit_conn::Column::FromPeer)
            .add(audit_conn::Column::UpdatedAt.lt(cutoff))
            .add(audit_conn::Column::CloseTime.gt(0))
            .add(audit_conn::Column::CloseTime.lt(cutoff.and_utc().timestamp())),
        file: linked_peer_events(audit_file::Column::PeerId, audit_file::Column::FromPeer)
            .add(unclosed_connections())
            .add(audit_file::Column::CreatedAt.lt(cutoff)),
    }
}

pub(crate) async fn preview(
    db: &impl ConnectionTrait,
    now: DateTime<Utc>,
) -> Result<RetentionResult, DbErr> {
    let Some(days) = retention(db).await?.days else {
        return Ok(RetentionResult::default());
    };
    let cutoff = now - Duration::days(i64::from(days));
    let eligible = candidates(cutoff.naive_utc(), now.timestamp());
    Ok(RetentionResult {
        cutoff: Some(cutoff),
        console: console_audit::Entity::find()
            .filter(eligible.console)
            .count(db)
            .await?,
        login: login_log::Entity::find()
            .filter(eligible.login)
            .count(db)
            .await?,
        connection: audit_conn::Entity::find()
            .filter(eligible.connection)
            .count(db)
            .await?,
        file: audit_file::Entity::find()
            .filter(eligible.file)
            .count(db)
            .await?,
    })
}

pub(crate) async fn cleanup(
    db: &sea_orm::DatabaseConnection,
    now: DateTime<Utc>,
) -> Result<RetentionResult, DbErr> {
    let transaction = console_roles::authority_transaction(db).await?;
    let Some(days) = retention(&transaction).await?.days else {
        return Err(DbErr::Custom("Audit retention is disabled".to_owned()));
    };
    let cutoff = now - Duration::days(i64::from(days));
    let eligible = candidates(cutoff.naive_utc(), now.timestamp());
    // One bounded batch per log. Reapply eligibility on DELETE, since an
    // active session or recording may have appeared after selecting IDs.
    macro_rules! remove {
        ($module:ident, $eligible:expr, $id_type:ty) => {{
            let ids = $module::Entity::find()
                .select_only()
                .column($module::Column::Id)
                .filter($eligible.clone())
                .order_by_asc($module::Column::Id)
                .limit(BATCH_SIZE)
                .into_tuple::<$id_type>()
                .all(&transaction)
                .await?;
            $module::Entity::delete_many()
                .filter($module::Column::Id.is_in(ids))
                .filter($eligible)
                .exec(&transaction)
                .await?
                .rows_affected
        }};
    }
    let result = RetentionResult {
        cutoff: Some(cutoff),
        console: remove!(console_audit, eligible.console, i64),
        login: remove!(login_log, eligible.login, i32),
        connection: remove!(audit_conn, eligible.connection, i32),
        file: remove!(audit_file, eligible.file, i32),
    };
    transaction.commit().await?;
    Ok(result)
}

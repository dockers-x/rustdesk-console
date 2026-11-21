//! Persisted management history and protected, bounded retention cleanup.

use ::entity::{
    active_connection, audit_conn, audit_file, console_audit, login_log, record_file, user_token,
};
use chrono::{DateTime, Duration, Utc};
use sea_orm::{
    sea_query::{Expr, IntoCondition, Query},
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbErr,
    EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use super::{console_roles, paginate};

const SETTING_KEY: &str = "console_audit_retention";
const RUN_KEY: &str = "console_audit_retention_last_run";
const BATCH_SIZE: u64 = 100;
const RUN_INTERVAL: Duration = Duration::minutes(15);

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
    pub days: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_days: Option<LogValues<Option<u16>>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scheduled: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LogValues<T> {
    console: T,
    login: T,
    connection: T,
    file: T,
}

impl<T> LogValues<T> {
    fn map<U>(self, f: impl Fn(T) -> U) -> LogValues<U> {
        LogValues {
            console: f(self.console),
            login: f(self.login),
            connection: f(self.connection),
            file: f(self.file),
        }
    }

    fn into_values(self) -> [T; 4] {
        [self.console, self.login, self.connection, self.file]
    }
}

impl Retention {
    fn effective_days(&self) -> LogValues<Option<u16>> {
        self.log_days.unwrap_or(LogValues {
            console: self.days,
            login: self.days,
            connection: self.days,
            file: self.days,
        })
    }

    fn enabled(&self) -> bool {
        self.effective_days()
            .into_values()
            .iter()
            .any(Option::is_some)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct RetentionStatus {
    #[serde(flatten)]
    settings: Retention,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_run: Option<ScheduledRun>,
}

#[derive(Debug, Serialize)]
struct ScheduledRun {
    at: DateTime<Utc>,
    result: Option<RetentionResult>,
}

// Persist the periods rather than four formatted cutoff timestamps to fit
// the existing MySQL system_settings value column.
#[derive(Debug, Deserialize, Serialize)]
struct StoredRun {
    at: DateTime<Utc>,
    policy: Retention,
    counts: Option<LogValues<u64>>,
}

async fn retention(db: &impl ConnectionTrait) -> Result<Retention, DbErr> {
    Ok(console_roles::setting(db, SETTING_KEY)
        .await?
        .unwrap_or_default())
}

pub(crate) async fn retention_status(db: &impl ConnectionTrait) -> Result<RetentionStatus, DbErr> {
    let settings = retention(db).await?;
    let last_run = if settings.scheduled {
        console_roles::setting::<StoredRun>(db, RUN_KEY)
            .await?
            .map(|run| ScheduledRun {
                at: run.at,
                result: run.counts.map(|counts| RetentionResult {
                    cutoff: cutoff_for(run.at, run.policy.days),
                    cutoffs: run
                        .policy
                        .log_days
                        .map(|days| days.map(|days| cutoff_for(run.at, days))),
                    counts,
                }),
            })
    } else {
        None
    };
    Ok(RetentionStatus { settings, last_run })
}

pub(crate) async fn set_retention(
    db: &DatabaseConnection,
    settings: Retention,
) -> Result<(), DbErr> {
    if settings.days.is_some() && settings.log_days.is_some() {
        return Err(DbErr::Custom(
            "Specify unified retention days or per-log retention days".to_owned(),
        ));
    }
    if settings
        .effective_days()
        .into_values()
        .into_iter()
        .flatten()
        .any(|days| !(1..=3650).contains(&days))
    {
        return Err(DbErr::Custom(
            "Retention days must be between 1 and 3650".to_owned(),
        ));
    }
    if settings.scheduled && !settings.enabled() {
        return Err(DbErr::Custom(
            "Scheduled cleanup requires at least one retention rule".to_owned(),
        ));
    }
    let transaction = console_roles::authority_transaction(db).await?;
    let value = serde_json::to_string(&settings).map_err(|err| DbErr::Custom(err.to_string()))?;
    console_roles::write_setting(&transaction, SETTING_KEY.to_owned(), value).await?;
    transaction.commit().await
}

#[derive(Debug, Serialize)]
pub(crate) struct RetentionResult {
    cutoff: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cutoffs: Option<LogValues<Option<DateTime<Utc>>>>,
    #[serde(flatten)]
    counts: LogValues<u64>,
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

fn cutoff_for(now: DateTime<Utc>, days: Option<u16>) -> Option<DateTime<Utc>> {
    days.map(|days| now - Duration::days(i64::from(days)))
}

struct CleanupPlan {
    cutoff: Option<DateTime<Utc>>,
    cutoffs: Option<LogValues<Option<DateTime<Utc>>>>,
    eligible: LogValues<Option<Condition>>,
}

impl CleanupPlan {
    fn new(settings: &Retention, now: DateTime<Utc>) -> Self {
        let cutoffs = settings.effective_days().map(|days| cutoff_for(now, days));
        Self {
            cutoff: cutoff_for(now, settings.days),
            cutoffs: settings.log_days.map(|_| cutoffs),
            eligible: LogValues {
                console: cutoffs.console.map(|cutoff| {
                    console_audit::Column::CreatedAt
                        .lt(cutoff.naive_utc())
                        .into_condition()
                }),
                login: cutoffs.login.map(|cutoff| {
                    Condition::all()
                        .add(login_log::Column::CreatedAt.lt(cutoff.naive_utc()))
                        .add(
                            login_log::Column::UserTokenId
                                .not_in_subquery(active_logins(now.timestamp())),
                        )
                }),
                connection: cutoffs.connection.map(|cutoff| {
                    linked_peer_events(audit_conn::Column::PeerId, audit_conn::Column::FromPeer)
                        .add(audit_conn::Column::UpdatedAt.lt(cutoff.naive_utc()))
                        .add(audit_conn::Column::CloseTime.gt(0))
                        .add(audit_conn::Column::CloseTime.lt(cutoff.timestamp()))
                }),
                file: cutoffs.file.map(|cutoff| {
                    linked_peer_events(audit_file::Column::PeerId, audit_file::Column::FromPeer)
                        .add(unclosed_connections())
                        .add(audit_file::Column::CreatedAt.lt(cutoff.naive_utc()))
                }),
            },
        }
    }
}

pub(crate) async fn preview(
    db: &impl ConnectionTrait,
    now: DateTime<Utc>,
) -> Result<RetentionResult, DbErr> {
    let plan = CleanupPlan::new(&retention(db).await?, now);
    macro_rules! count {
        ($module:ident, $eligible:expr) => {
            match $eligible {
                Some(eligible) => $module::Entity::find().filter(eligible).count(db).await?,
                None => 0,
            }
        };
    }
    Ok(RetentionResult {
        cutoff: plan.cutoff,
        cutoffs: plan.cutoffs,
        counts: LogValues {
            console: count!(console_audit, plan.eligible.console),
            login: count!(login_log, plan.eligible.login),
            connection: count!(audit_conn, plan.eligible.connection),
            file: count!(audit_file, plan.eligible.file),
        },
    })
}

pub(crate) async fn cleanup(
    db: &DatabaseConnection,
    now: DateTime<Utc>,
) -> Result<RetentionResult, DbErr> {
    let transaction = console_roles::authority_transaction(db).await?;
    let settings = retention(&transaction).await?;
    if !settings.enabled() {
        return Err(DbErr::Custom("Audit retention is disabled".to_owned()));
    }
    let result = remove_batch(&transaction, CleanupPlan::new(&settings, now)).await?;
    transaction.commit().await?;
    Ok(result)
}

async fn remove_batch(
    db: &impl ConnectionTrait,
    plan: CleanupPlan,
) -> Result<RetentionResult, DbErr> {
    // Reapply eligibility on DELETE to check for new active connections and recordings.
    macro_rules! remove {
        ($module:ident, $eligible:expr, $id_type:ty) => {
            match $eligible {
                Some(eligible) => {
                    let ids = $module::Entity::find()
                        .select_only()
                        .column($module::Column::Id)
                        .filter(eligible.clone())
                        .order_by_asc($module::Column::Id)
                        .limit(BATCH_SIZE)
                        .into_tuple::<$id_type>()
                        .all(db)
                        .await?;
                    if ids.is_empty() {
                        0
                    } else {
                        $module::Entity::delete_many()
                            .filter($module::Column::Id.is_in(ids))
                            .filter(eligible)
                            .exec(db)
                            .await?
                            .rows_affected
                    }
                }
                None => 0,
            }
        };
    }
    Ok(RetentionResult {
        cutoff: plan.cutoff,
        cutoffs: plan.cutoffs,
        counts: LogValues {
            console: remove!(console_audit, plan.eligible.console, i64),
            login: remove!(login_log, plan.eligible.login, i32),
            connection: remove!(audit_conn, plan.eligible.connection, i32),
            file: remove!(audit_file, plan.eligible.file, i32),
        },
    })
}

async fn run_due(db: &impl ConnectionTrait, now: DateTime<Utc>) -> Result<bool, DbErr> {
    let last: Option<StoredRun> = console_roles::setting(db, RUN_KEY).await?;
    Ok(last.is_none_or(|run| now - run.at >= RUN_INTERVAL))
}

pub(crate) async fn run_scheduled(
    db: &DatabaseConnection,
    now: DateTime<Utc>,
) -> Result<Option<RetentionResult>, DbErr> {
    let settings = retention(db).await?;
    if !settings.scheduled || !settings.enabled() || !run_due(db, now).await? {
        return Ok(None);
    }

    // Check again after taking the same database lock as policy changes and manual cleanup.
    let transaction = console_roles::authority_transaction(db).await?;
    let settings = retention(&transaction).await?;
    if !settings.scheduled || !settings.enabled() || !run_due(&transaction, now).await? {
        transaction.rollback().await?;
        return Ok(None);
    }
    // A savepoint rolls back the whole batch on failure while retaining the authority
    // lock to publish the failed attempt. A crash rolls back both deletion and status.
    let batch = transaction.begin().await?;
    let result = match remove_batch(&batch, CleanupPlan::new(&settings, now)).await {
        Ok(result) => {
            batch.commit().await?;
            Ok(result)
        }
        Err(err) => {
            batch.rollback().await?;
            Err(err)
        }
    };
    let status = StoredRun {
        at: now,
        policy: settings,
        counts: result.as_ref().ok().map(|result| result.counts),
    };
    let value = serde_json::to_string(&status).map_err(|err| DbErr::Custom(err.to_string()))?;
    console_roles::write_setting(&transaction, RUN_KEY.to_owned(), value).await?;
    transaction.commit().await?;
    result.map(Some)
}

pub(crate) fn spawn_retention_worker(db: DatabaseConnection) {
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(60));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            match run_scheduled(&db, Utc::now()).await {
                Ok(Some(result)) => tracing::info!(
                    target: "security_audit",
                    event = "console.audit_retention_completed",
                    console = result.counts.console,
                    login = result.counts.login,
                    connection = result.counts.connection,
                    file = result.counts.file,
                ),
                Ok(None) => {}
                Err(err) => tracing::warn!(%err, "Scheduled audit cleanup failed"),
            }
        }
    });
}

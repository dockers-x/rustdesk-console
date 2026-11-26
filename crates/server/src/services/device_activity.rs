//! Bounded presence and connection history for one enrolled device.

use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use entity::{device_presence_event, peer};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::Serialize;

use super::audit::{self, DeviceConnectionReport};

pub(crate) const DEFAULT_RANGE_DAYS: i64 = 7;
const RETENTION_DAYS: i64 = 90;
const MAX_RESULTS: usize = 25;
const PRUNE_BATCH_SIZE: u64 = 500;

#[derive(Debug, Serialize)]
pub(crate) struct Activity {
    presence: Recent<device_presence_event::Model>,
    connections: Recent<DeviceConnectionReport>,
}

#[derive(Debug, Serialize)]
struct Recent<T> {
    entries: Vec<T>,
    has_more: bool,
}

impl<T> Recent<T> {
    fn new(mut entries: Vec<T>) -> Self {
        let has_more = entries.len() > MAX_RESULTS;
        entries.truncate(MAX_RESULTS);
        Self { entries, has_more }
    }
}

/// Accept the ranges offered in the drawer and bound the largest scan.
pub(crate) fn range(days: i64) -> Option<TimeDelta> {
    matches!(days, DEFAULT_RANGE_DAYS | 30 | RETENTION_DAYS).then(|| TimeDelta::days(days))
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Presence {
    Online,
    Offline,
}

/// Record a presence transition inside the transaction holding the peer lock.
pub(crate) async fn record(
    db: &sea_orm::DatabaseTransaction,
    device: &peer::Model,
    presence: Presence,
    observed_at: i64,
) -> Result<(), DbErr> {
    device_presence_event::ActiveModel {
        peer_row_id: Set(device.row_id),
        online: Set(matches!(presence, Presence::Online)),
        observed_at: Set(observed_at),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

/// Read the latest presence transitions and audit reports within the selected window.
pub(crate) async fn recent(
    db: &DatabaseConnection,
    device: &peer::Model,
    now: DateTime<Utc>,
    range: TimeDelta,
) -> Result<Activity, DbErr> {
    let since = device
        .created_at
        .map_or(now - range, |created| created.and_utc().max(now - range));
    let limit = MAX_RESULTS as u64 + 1;
    let presence = device_presence_event::Entity::find()
        .filter(device_presence_event::Column::PeerRowId.eq(device.row_id))
        .filter(
            device_presence_event::Column::ObservedAt.between(since.timestamp(), now.timestamp()),
        )
        .order_by_desc(device_presence_event::Column::ObservedAt)
        .order_by_desc(device_presence_event::Column::Id)
        .limit(limit)
        .all(db)
        .await?;
    let connections =
        audit::conn_recent_for_device(db, device, since.naive_utc(), now.naive_utc(), limit)
            .await?;
    Ok(Activity {
        presence: Recent::new(presence),
        connections: Recent::new(connections),
    })
}

async fn prune(db: &impl ConnectionTrait, now: DateTime<Utc>) -> Result<(), DbErr> {
    use device_presence_event::{Column, Entity};

    let cutoff = (now - TimeDelta::days(RETENTION_DAYS)).timestamp();
    let expired = Entity::find()
        .select_only()
        .column(Column::Id)
        .filter(Column::ObservedAt.lt(cutoff))
        .order_by_asc(Column::ObservedAt)
        .order_by_asc(Column::Id)
        .limit(PRUNE_BATCH_SIZE)
        .into_tuple::<i32>()
        .all(db)
        .await?;
    if !expired.is_empty() {
        Entity::delete_many()
            .filter(Column::Id.is_in(expired))
            .filter(Column::ObservedAt.lt(cutoff))
            .exec(db)
            .await?;
    }
    Ok(())
}

/// Remove expired presence events in small batches without touching audit reports.
pub(crate) fn spawn_retention_worker(db: DatabaseConnection) {
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(60));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            if let Err(error) = prune(&db, Utc::now()).await {
                tracing::warn!(%error, "Could not prune device presence history");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, DbBackend, Schema};

    #[tokio::test]
    async fn device_activity_prunes_a_bounded_batch_of_expired_transitions() {
        use device_presence_event::{Column, Entity};

        let db = Database::connect("sqlite::memory:").await.unwrap();
        let backend = DbBackend::Sqlite;
        let table = Schema::new(backend).create_table_from_entity(Entity);
        db.execute(backend.build(&table)).await.unwrap();
        let now = Utc::now();
        let cutoff = (now - TimeDelta::days(RETENTION_DAYS)).timestamp();
        for age in (1..=PRUNE_BATCH_SIZE + 1).map(|age| age as i64).chain([0]) {
            device_presence_event::ActiveModel {
                peer_row_id: Set(1),
                online: Set(true),
                observed_at: Set(cutoff - age),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        prune(&db, now).await.unwrap();
        let remaining = Entity::find()
            .select_only()
            .column(Column::ObservedAt)
            .order_by_asc(Column::ObservedAt)
            .into_tuple::<i64>()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(remaining, [cutoff - 1, cutoff]);
    }
}

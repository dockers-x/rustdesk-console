//! Persistent fan-out and bounded delivery for device notifications.

use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use sea_orm::{sea_query::Expr, *};
use tokio::task::JoinSet;

use ::entity::{webhook_delivery as delivery, webhook_subscription as subscription};

use super::notification_channel::{self, DeliveryError, Destination, Event};
use super::now;

const WORKERS: u64 = 4;
const LEASE_SECONDS: i64 = 45;
const MAX_ATTEMPTS: i32 = 3;
const TEST_EVENT: &str = "webhook.test";

pub(super) struct Pending<'a> {
    pub sub: &'a subscription::Model,
    pub body: String,
}

pub(super) async fn enqueue(
    db: &DatabaseTransaction,
    jobs: &[Pending<'_>],
    event_id: &str,
    event_type: &str,
) -> Result<(), DbErr> {
    let scheduled = Utc::now().timestamp();
    for batch in jobs.chunks(100) {
        delivery::Entity::insert_many(batch.iter().map(|job| {
            let event = Event {
                id: event_id,
                kind: event_type,
                body: &job.body,
            };
            new_job(job.sub, &event, delivery::STATUS_PENDING, 0, scheduled)
        }))
        .exec_without_returning(db)
        .await?;
    }
    Ok(())
}

fn new_job(
    sub: &subscription::Model,
    event: &Event<'_>,
    status: &str,
    attempt: i32,
    execute_at: i64,
) -> delivery::ActiveModel {
    delivery::ActiveModel {
        subscription_id: Set(sub.id),
        channel: Set(sub.channel.clone()),
        destination_revision: Set(sub.destination_revision.clone()),
        event_id: Set(event.id.to_owned()),
        event_type: Set(event.kind.to_owned()),
        request_body: Set(event.body.to_owned()),
        status: Set(status.to_owned()),
        attempt: Set(attempt),
        execute_at: Set(execute_at),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    }
}

fn client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .timeout(Duration::from_secs(6))
        .connect_timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

pub(super) async fn test(
    db: &DatabaseConnection,
    sub: &subscription::Model,
    event_id: &str,
    body: &str,
) -> Result<(), String> {
    let client = client().map_err(|_| "Notification HTTP client unavailable".to_owned())?;
    let job = new_job(
        sub,
        &Event {
            id: event_id,
            kind: TEST_EVENT,
            body,
        },
        delivery::STATUS_SENDING,
        1,
        Utc::now().timestamp() + LEASE_SECONDS,
    )
    .insert(db)
    .await
    .map_err(|_| "Failed to record test delivery".to_owned())?;
    let error = deliver(db, &client, &job, Some(sub))
        .await
        .map_err(|_| "Failed to record test delivery outcome".to_owned())?;
    error.map_or(Ok(()), Err)
}

pub(super) fn spawn_worker(db: DatabaseConnection) {
    tokio::spawn(async move {
        let Ok(client) = client() else {
            tracing::error!("failed to start notification HTTP client");
            return;
        };
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(error) = process_ready(&db, &client).await {
                tracing::warn!(%error, "notification delivery queue unavailable");
            }
        }
    });
}

async fn process_ready(db: &DatabaseConnection, client: &Client) -> Result<(), DbErr> {
    let jobs = delivery::Entity::find()
        .filter(
            delivery::Column::Status.is_in([delivery::STATUS_PENDING, delivery::STATUS_SENDING]),
        )
        // Legacy pending rows have no schedule and an unknown prior outcome.
        .filter(delivery::Column::ExecuteAt.between(1, Utc::now().timestamp()))
        .order_by_asc(delivery::Column::ExecuteAt)
        .order_by_asc(delivery::Column::Id)
        .limit(WORKERS)
        .all(db)
        .await?;
    let mut active = JoinSet::new();
    for job in jobs {
        let (db, client) = (db.clone(), client.clone());
        active.spawn(async move {
            let id = job.id;
            (id, process(&db, &client, job).await)
        });
    }
    while let Some(result) = active.join_next().await {
        match result {
            Ok((id, Err(error))) => {
                tracing::warn!(%error, delivery_id = id, "notification job failed")
            }
            Err(error) => tracing::warn!(%error, "notification worker task stopped"),
            Ok((_, Ok(()))) => {}
        }
    }
    Ok(())
}

async fn process(
    db: &DatabaseConnection,
    client: &Client,
    mut job: delivery::Model,
) -> Result<(), DbErr> {
    if job.attempt >= attempts(&job) {
        finish(db, &job, Outcome::lease_expired()).await?;
        return Ok(());
    }
    // A competing worker must see a different attempt or schedule after our claim.
    let claimed = delivery::Entity::update_many()
        .col_expr(
            delivery::Column::Status,
            Expr::value(delivery::STATUS_SENDING),
        )
        .col_expr(delivery::Column::Attempt, Expr::value(job.attempt + 1))
        .col_expr(
            delivery::Column::ExecuteAt,
            Expr::value(Utc::now().timestamp() + LEASE_SECONDS),
        )
        .col_expr(delivery::Column::UpdatedAt, Expr::value(now()))
        .filter(delivery::Column::Id.eq(job.id))
        .filter(delivery::Column::Status.eq(&job.status))
        .filter(delivery::Column::Attempt.eq(job.attempt))
        .filter(delivery::Column::ExecuteAt.eq(job.execute_at))
        .exec(db)
        .await?;
    if claimed.rows_affected != 1 {
        return Ok(());
    }
    job.status = delivery::STATUS_SENDING.to_owned();
    job.attempt += 1;
    let sub = subscription::Entity::find_by_id(job.subscription_id)
        .one(db)
        .await?
        .filter(|sub| {
            sub.enabled
                && sub.channel == job.channel
                && sub.destination_revision == job.destination_revision
        });
    deliver(db, client, &job, sub.as_ref()).await?;
    Ok(())
}

async fn deliver(
    db: &DatabaseConnection,
    client: &Client,
    job: &delivery::Model,
    sub: Option<&subscription::Model>,
) -> Result<Option<String>, DbErr> {
    let result = match sub {
        Some(sub) => match sub.channel.parse() {
            Ok(channel) => {
                notification_channel::deliver(
                    client,
                    Destination {
                        channel,
                        url: &sub.url,
                        topic: &sub.topic,
                        secret: &sub.secret,
                    },
                    Event {
                        id: &job.event_id,
                        kind: &job.event_type,
                        body: &job.request_body,
                    },
                )
                .await
            }
            Err(_) => Err(failure("unsupported notification channel")),
        },
        None => Err(failure(
            "notification destination disabled, deleted or changed",
        )),
    };
    let outcome = match result {
        Ok(response) => Outcome {
            code: Some(response.status),
            body: response.body,
            error: None,
            retry: false,
        },
        Err(error) => Outcome {
            code: error.status,
            body: error.body,
            error: Some(error.reason),
            retry: error.retryable && job.attempt < attempts(job),
        },
    };
    let error = outcome.error.clone();
    finish(db, job, outcome).await?;
    Ok(error)
}

fn attempts(job: &delivery::Model) -> i32 {
    if job.event_type == TEST_EVENT {
        1
    } else {
        MAX_ATTEMPTS
    }
}

fn failure(reason: &str) -> DeliveryError {
    DeliveryError {
        status: None,
        body: String::new(),
        reason: reason.to_owned(),
        retryable: false,
    }
}

struct Outcome {
    code: Option<u16>,
    body: String,
    error: Option<String>,
    retry: bool,
}

impl Outcome {
    fn lease_expired() -> Self {
        Self {
            code: None,
            body: String::new(),
            error: Some("notification lease expired before outcome was recorded".to_owned()),
            retry: false,
        }
    }
}

async fn finish(
    db: &DatabaseConnection,
    job: &delivery::Model,
    outcome: Outcome,
) -> Result<(), DbErr> {
    let ts = Utc::now().timestamp();
    let (status, execute_at, delivered_at) = if outcome.retry {
        // Stagger retries from notifications sharing the same original event.
        let delay = 5 * 3_i64.pow((job.attempt - 1).max(0) as u32) + i64::from(job.id % 5);
        (delivery::STATUS_PENDING, ts + delay, 0)
    } else if outcome.error.is_some() {
        (delivery::STATUS_FAILED, 0, ts)
    } else {
        (delivery::STATUS_SUCCESS, 0, ts)
    };
    delivery::Entity::update_many()
        .col_expr(delivery::Column::Status, Expr::value(status))
        .col_expr(
            delivery::Column::StatusCode,
            Expr::value(i32::from(outcome.code.unwrap_or(0))),
        )
        .col_expr(delivery::Column::ResponseBody, Expr::value(outcome.body))
        .col_expr(
            delivery::Column::Error,
            Expr::value(outcome.error.unwrap_or_default()),
        )
        .col_expr(delivery::Column::DeliveredAt, Expr::value(delivered_at))
        .col_expr(delivery::Column::ExecuteAt, Expr::value(execute_at))
        .col_expr(delivery::Column::UpdatedAt, Expr::value(now()))
        .filter(delivery::Column::Id.eq(job.id))
        .filter(delivery::Column::Status.eq(&job.status))
        .filter(delivery::Column::Attempt.eq(job.attempt))
        .exec(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::notification_channel::Channel;

    async fn deliver_fixture(
        status: axum::http::StatusCode,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handler =
            axum::Router::new().route("/", axum::routing::post(move || async move { status }));
        let server = tokio::spawn(async move { axum::serve(listener, handler).await.unwrap() });
        (format!("http://{addr}"), server)
    }

    #[tokio::test]
    async fn successful_channel_records_delivery_while_throttled_channel_retries() {
        let (ok_url, ok_server) = deliver_fixture(axum::http::StatusCode::OK).await;
        let (retry_url, retry_server) =
            deliver_fixture(axum::http::StatusCode::TOO_MANY_REQUESTS).await;
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for table in [
            schema.create_table_from_entity(subscription::Entity),
            schema.create_table_from_entity(delivery::Entity),
        ] {
            db.execute(db.get_database_backend().build(&table))
                .await
                .unwrap();
        }
        let mut subscriptions = Vec::new();
        for (channel, url) in [(Channel::Webhook, ok_url), (Channel::Ntfy, retry_url)] {
            subscriptions.push(
                subscription::ActiveModel {
                    name: Set("fixture".to_owned()),
                    enabled: Set(true),
                    channel: Set(channel.as_str().to_owned()),
                    topic: Set("status-fixture".to_owned()),
                    url: Set(url),
                    ..Default::default()
                }
                .insert(&db)
                .await
                .unwrap(),
            );
        }
        let transaction = db.begin().await.unwrap();
        let jobs = subscriptions
            .iter()
            .map(|sub| Pending {
                sub,
                body: r#"{"resource":{"id":"42","name":"Fixture"}}"#.to_owned(),
            })
            .collect::<Vec<_>>();
        enqueue(&transaction, &jobs, "event-fixture", "device.online")
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let client = client().unwrap();
        let (first, competitor) =
            tokio::join!(process_ready(&db, &client), process_ready(&db, &client));
        first.unwrap();
        competitor.unwrap();
        let rows = delivery::Entity::find()
            .order_by_asc(delivery::Column::Id)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].status, delivery::STATUS_SUCCESS);
        assert_eq!(rows[1].status, delivery::STATUS_PENDING);
        assert_eq!(rows[1].status_code, 429);
        assert!(rows[1].execute_at > Utc::now().timestamp());
        assert!(rows.iter().all(|row| row.attempt == 1));
        ok_server.abort();
        retry_server.abort();
    }
}

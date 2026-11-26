use std::time::Duration;

use chrono::Utc;
use sea_orm::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use ::entity::{
    device_group, device_presence_state, peer, user, webhook_delivery, webhook_subscription,
};

use super::device_activity::{self, Presence};
use super::notification_channel::{self, Channel};
use super::{notification_delivery, notification_template, now, paginate};

pub const EVENT_DEVICE_ONLINE: &str = "device.online";
pub const EVENT_DEVICE_OFFLINE: &str = "device.offline";
const ONLINE_WINDOW_SECONDS: i64 = 90;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WebhookSubscriptionInput {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub channel: Option<Channel>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub template_id: Option<i32>,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub clear_secret: bool,
    #[serde(default)]
    pub event_types: Vec<String>,
    #[serde(default)]
    pub device_ids: Vec<String>,
    #[serde(default)]
    pub device_group_ids: Vec<i32>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookSubscriptionView {
    pub id: i32,
    pub name: String,
    pub url: String,
    pub channel: String,
    pub topic: String,
    pub template_id: Option<i32>,
    pub event_types: Vec<String>,
    pub device_ids: Vec<String>,
    pub device_group_ids: Vec<i32>,
    pub enabled: bool,
    pub secret_set: bool,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookSubscriptionList {
    pub list: Vec<WebhookSubscriptionView>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookDeliveryList {
    pub list: Vec<webhook_delivery::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Clone)]
struct DeviceEvent {
    event_id: String,
    event_type: &'static str,
    peer: peer::Model,
    previous_state: &'static str,
    current_state: &'static str,
    previous_seen_at: i64,
    current_seen_at: i64,
}

pub async fn list_subscriptions(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
) -> Result<WebhookSubscriptionList, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let q = webhook_subscription::Entity::find();
    let total = q.clone().count(db).await? as i64;
    let rows = q
        .order_by_desc(webhook_subscription::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    let templates =
        notification_template::for_subscriptions(db, rows.iter().map(|row| row.id).collect())
            .await?;
    Ok(WebhookSubscriptionList {
        list: rows
            .into_iter()
            .map(|row| {
                let template_id = templates.get(&row.id).map(|template| template.id);
                subscription_view(row, template_id)
            })
            .collect(),
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn save_subscription(
    db: &DatabaseConnection,
    input: WebhookSubscriptionInput,
) -> Result<WebhookSubscriptionView, String> {
    let transaction = db
        .begin()
        .await
        .map_err(|_| "Notification config storage unavailable")?;
    if input.id > 0 {
        // Resolve "keep current credential" after serializing concurrent edits.
        webhook_subscription::Entity::update_many()
            .col_expr(
                webhook_subscription::Column::Id,
                sea_orm::sea_query::Expr::col(webhook_subscription::Column::Id).into(),
            )
            .filter(webhook_subscription::Column::Id.eq(input.id))
            .exec(&transaction)
            .await
            .map_err(|_| "Failed to lock notification destination")?;
    }
    let selected = input.template_id;
    let draft = prepare_subscription(&transaction, input).await?;
    let id = draft.id;
    let mut active: webhook_subscription::ActiveModel = draft.into();
    active = active.reset_all();
    let row = if id > 0 {
        active.update(&transaction).await
    } else {
        active.id = NotSet;
        active.insert(&transaction).await
    }
    .map_err(|_| "Failed to save notification destination".to_owned())?;
    notification_template::set_binding(&transaction, row.id, selected).await?;
    let selected = selection(&transaction, row.id, selected)
        .await?
        .map(|template| template.id);
    transaction
        .commit()
        .await
        .map_err(|_| "Failed to save notification destination")?;
    Ok(subscription_view(row, selected))
}

async fn prepare_subscription(
    db: &impl ConnectionTrait,
    input: WebhookSubscriptionInput,
) -> Result<webhook_subscription::Model, String> {
    let existing = if input.id > 0 {
        Some(find_subscription(db, input.id).await?)
    } else {
        None
    };
    let old_channel = existing.as_ref().map(|row| row.channel.as_str());
    let channel = match (input.channel, old_channel) {
        (Some(channel), _) => channel,
        (None, Some(stored)) => stored.parse()?,
        (None, None) => Channel::default(),
    };
    let same_channel = old_channel == Some(channel.as_str());
    let name = input.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 255 {
        return Err("Enter a notification name between 1 and 255 characters".to_owned());
    }
    let topic = if channel == Channel::Ntfy {
        input
            .topic
            .as_deref()
            .or_else(|| {
                existing
                    .as_ref()
                    .filter(|_| same_channel)
                    .map(|row| row.topic.as_str())
            })
            .unwrap_or_default()
            .trim()
            .to_owned()
    } else {
        String::new()
    };
    let url = notification_channel::normalize_endpoint(channel, input.url.trim(), &topic)?;
    let can_reuse_secret = same_channel
        && existing
            .as_ref()
            .is_some_and(|row| channel == Channel::Webhook || row.url == url);
    let provided_secret = input.secret.trim();
    let secret = if input.clear_secret {
        String::new()
    } else if !provided_secret.is_empty() {
        provided_secret.to_owned()
    } else if let Some(row) = existing.as_ref().filter(|_| can_reuse_secret) {
        row.secret.clone()
    } else if channel == Channel::Webhook {
        crate::support::random::random_string(32)
    } else {
        String::new()
    };
    notification_channel::validate_secret(channel, &secret)?;
    let event_types = normalize_event_types(channel, input.event_types)?;
    let device_ids = normalize_string_list(input.device_ids);
    let device_group_ids = normalize_i32_list(input.device_group_ids);
    let destination_revision = existing
        .as_ref()
        .filter(|row| {
            same_channel
                && row.url == url
                && row.topic == topic
                && row.secret == secret
                && row.enabled == input.enabled
        })
        .map_or_else(
            || Uuid::new_v4().to_string(),
            |row| row.destination_revision.clone(),
        );
    let created_at = existing.as_ref().map_or_else(now, |row| row.created_at);
    Ok(webhook_subscription::Model {
        id: input.id.max(0),
        name,
        url,
        channel: channel.as_str().to_owned(),
        topic,
        destination_revision,
        secret,
        event_types: json_string(&event_types)?,
        device_ids: json_string(&device_ids)?,
        device_group_ids: json_string(&device_group_ids)?,
        tags: existing.map_or_else(|| "[]".to_owned(), |row| row.tags),
        enabled: input.enabled,
        created_at,
        updated_at: now(),
    })
}

async fn find_subscription(
    db: &impl ConnectionTrait,
    id: i32,
) -> Result<webhook_subscription::Model, String> {
    webhook_subscription::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(|_| "Failed to load notification destination".to_owned())?
        .ok_or_else(|| "Notification destination not found".to_owned())
}

pub async fn delete_subscription(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    webhook_subscription::Entity::delete_by_id(id)
        .exec(db)
        .await?;
    Ok(())
}

pub async fn list_deliveries(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    subscription_id: Option<i32>,
) -> Result<WebhookDeliveryList, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = webhook_delivery::Entity::find();
    if let Some(id) = subscription_id.filter(|id| *id > 0) {
        q = q.filter(webhook_delivery::Column::SubscriptionId.eq(id));
    }
    let total = q.clone().count(db).await? as i64;
    let rows = q
        .order_by_desc(webhook_delivery::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(WebhookDeliveryList {
        list: rows,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn test_subscription(db: &DatabaseConnection, id: i32) -> Result<(), String> {
    let rule = find_subscription(db, id).await?;
    let template = selection(db, id, None).await?;
    send_test(db, &rule, template.as_ref()).await
}

pub async fn test_configuration(
    db: &DatabaseConnection,
    input: WebhookSubscriptionInput,
) -> Result<(), String> {
    let template = selection(db, input.id, input.template_id).await?;
    let rule = prepare_subscription(db, input).await?;
    send_test(db, &rule, template.as_ref()).await
}

async fn selection(
    db: &impl ConnectionTrait,
    id: i32,
    selected: Option<i32>,
) -> Result<Option<notification_template::View>, String> {
    match selected {
        Some(0) => Ok(None),
        Some(id) => notification_template::load(db, id)
            .await
            .ok()
            .flatten()
            .map(Some)
            .ok_or_else(|| "Selected notification template is unavailable".into()),
        None if id > 0 => notification_template::for_subscriptions(db, vec![id])
            .await
            .map(|mut templates| templates.remove(&id))
            .map_err(|_| "Failed to load notification template selection".into()),
        None => Ok(None),
    }
}

async fn send_test(
    db: &DatabaseConnection,
    sub: &webhook_subscription::Model,
    template: Option<&notification_template::View>,
) -> Result<(), String> {
    let event_id = format!("evt_{}", Uuid::new_v4());
    let occurred_at = Utc::now();
    let mut payload = json!({
        "event_id": event_id,
        "event_type": "webhook.test",
        "occurred_at": occurred_at.to_rfc3339(),
        "source": "rustdesk-console",
        "test": true,
    });
    if let Some(template) = template {
        let sample = json!({
            "resource": {
                "id": "123456789", "name": "Test device", "hostname": "test-desktop",
                "platform": "Linux", "username": "sample"
            },
            "device": {"group_name": "Test group"},
            "transition": {"from":"offline", "to":"online", "current_seen_at":occurred_at.timestamp()},
            "event_type": EVENT_DEVICE_ONLINE,
            "occurred_at": payload["occurred_at"],
        });
        payload["notification"] = notification_template::rendered(template, &sample);
    }
    notification_delivery::test(db, sub, &event_id, &payload.to_string()).await
}

pub fn spawn_device_seen(db: DatabaseConnection, p: peer::Model) {
    tokio::spawn(async move {
        if let Err(e) = mark_device_seen(&db, p).await {
            tracing::warn!("failed to update device presence: {e}");
        }
    });
}

pub async fn mark_device_seen(db: &DatabaseConnection, p: peer::Model) -> Result<(), DbErr> {
    let transaction = db.begin().await?;
    let Some(p) = lock_presence_peer(&transaction, p.row_id).await? else {
        return Ok(());
    };
    let now_ts = Utc::now().timestamp();
    let existing = latest_presence(&transaction, p.row_id).await?;
    let previous_seen_at = existing.as_ref().map_or(0, |row| row.last_seen_at);
    let missed_heartbeats = existing
        .as_ref()
        .is_some_and(|row| row.online && row.last_seen_at < now_ts - ONLINE_WINDOW_SECONDS);
    let should_emit = existing
        .as_ref()
        .is_none_or(|row| !row.online || missed_heartbeats);
    if let Some(row) = existing {
        let mut active: device_presence_state::ActiveModel = row.into();
        active.peer_id = Set(p.id.clone());
        active.online = Set(true);
        active.last_seen_at = Set(now_ts);
        if should_emit {
            active.last_changed_at = Set(now_ts);
        }
        active.updated_at = Set(now());
        active.update(&transaction).await?;
    } else {
        device_presence_state::ActiveModel {
            peer_row_id: Set(p.row_id),
            peer_id: Set(p.id.clone()),
            online: Set(true),
            last_seen_at: Set(now_ts),
            last_changed_at: Set(now_ts),
            created_at: Set(now()),
            updated_at: Set(now()),
            ..Default::default()
        }
        .insert(&transaction)
        .await?;
    }
    if should_emit {
        // A returning heartbeat can detect the timeout before the scan runs.
        if missed_heartbeats {
            device_activity::record(&transaction, &p, Presence::Offline, now_ts).await?;
        }
        device_activity::record(&transaction, &p, Presence::Online, now_ts).await?;
        queue_device_event(
            &transaction,
            DeviceEvent {
                event_id: format!("evt_{}", Uuid::new_v4()),
                event_type: EVENT_DEVICE_ONLINE,
                peer: p,
                previous_state: "offline",
                current_state: "online",
                previous_seen_at,
                current_seen_at: now_ts,
            },
        )
        .await?;
    }
    transaction.commit().await
}

pub fn spawn_presence_worker(db: DatabaseConnection) {
    notification_delivery::spawn_worker(db.clone());
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(30));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(e) = scan_offline_devices(&db).await {
                tracing::warn!("failed to scan offline devices: {e}");
            }
        }
    });
}

async fn scan_offline_devices(db: &DatabaseConnection) -> Result<(), DbErr> {
    let now_ts = Utc::now().timestamp();
    let cutoff = now_ts - ONLINE_WINDOW_SECONDS;
    let rows = device_presence_state::Entity::find()
        .filter(device_presence_state::Column::Online.eq(true))
        .filter(device_presence_state::Column::LastSeenAt.lt(cutoff))
        .order_by_asc(device_presence_state::Column::LastSeenAt)
        .limit(100)
        .all(db)
        .await?;
    for scheduled in rows {
        let transaction = db.begin().await?;
        let device = lock_presence_peer(&transaction, scheduled.peer_row_id).await?;
        let Some(row) = latest_presence(&transaction, scheduled.peer_row_id).await? else {
            continue;
        };
        if row.id != scheduled.id {
            // Older releases allowed duplicate shadow rows. Retire them without emitting.
            let mut obsolete: device_presence_state::ActiveModel = scheduled.into();
            obsolete.online = Set(false);
            obsolete.updated_at = Set(now());
            obsolete.update(&transaction).await?;
            transaction.commit().await?;
            continue;
        }
        if !row.online || row.last_seen_at >= cutoff {
            continue;
        }
        let previous_seen_at = row.last_seen_at;
        let mut active: device_presence_state::ActiveModel = row.into();
        active.online = Set(false);
        active.last_changed_at = Set(now_ts);
        active.updated_at = Set(now());
        active.update(&transaction).await?;
        if let Some(p) = device {
            device_activity::record(&transaction, &p, Presence::Offline, now_ts).await?;
            queue_device_event(
                &transaction,
                DeviceEvent {
                    event_id: format!("evt_{}", Uuid::new_v4()),
                    event_type: EVENT_DEVICE_OFFLINE,
                    peer: p,
                    previous_state: "online",
                    current_state: "offline",
                    previous_seen_at,
                    current_seen_at: previous_seen_at,
                },
            )
            .await?;
        }
        transaction.commit().await?;
    }
    Ok(())
}

async fn latest_presence(
    db: &DatabaseTransaction,
    peer_id: i32,
) -> Result<Option<device_presence_state::Model>, DbErr> {
    device_presence_state::Entity::find()
        .filter(device_presence_state::Column::PeerRowId.eq(peer_id))
        .order_by_desc(device_presence_state::Column::LastSeenAt)
        .order_by_desc(device_presence_state::Column::Id)
        .one(db)
        .await
}

async fn lock_presence_peer(
    db: &DatabaseTransaction,
    peer_id: i32,
) -> Result<Option<peer::Model>, DbErr> {
    // Serialize the first observation and later transitions for this device.
    peer::Entity::update_many()
        .col_expr(
            peer::Column::RowId,
            sea_orm::sea_query::Expr::col(peer::Column::RowId).into(),
        )
        .filter(peer::Column::RowId.eq(peer_id))
        .exec(db)
        .await?;
    peer::Entity::find_by_id(peer_id).one(db).await
}

async fn queue_device_event(db: &DatabaseTransaction, event: DeviceEvent) -> Result<(), DbErr> {
    let mut subscriptions = webhook_subscription::Entity::find()
        .filter(webhook_subscription::Column::Enabled.eq(true))
        .all(db)
        .await?;
    subscriptions.retain(|sub| subscription_matches(sub, &event));
    if subscriptions.is_empty() {
        return Ok(());
    }
    let event_payload = device_event_payload(db, &event).await;
    let templates = notification_template::for_subscriptions(
        db,
        subscriptions.iter().map(|sub| sub.id).collect(),
    )
    .await?;
    let default_body = event_payload.to_string();
    let jobs = subscriptions
        .iter()
        .map(|sub| {
            let body = if let Some(template) = templates.get(&sub.id) {
                let mut payload = event_payload.clone();
                payload["notification"] = notification_template::rendered(template, &event_payload);
                payload.to_string()
            } else {
                default_body.clone()
            };
            notification_delivery::Pending { sub, body }
        })
        .collect::<Vec<_>>();
    notification_delivery::enqueue(db, &jobs, &event.event_id, event.event_type).await
}

fn subscription_matches(sub: &webhook_subscription::Model, event: &DeviceEvent) -> bool {
    let event_types = parse_string_vec(&sub.event_types);
    if !event_types.iter().any(|kind| kind == event.event_type) {
        return false;
    }
    let device_ids = parse_string_vec(&sub.device_ids);
    if !device_ids.is_empty() && !device_ids.iter().any(|id| id == &event.peer.id) {
        return false;
    }
    let group_ids = parse_i32_vec(&sub.device_group_ids);
    group_ids.is_empty() || group_ids.contains(&event.peer.group_id)
}

async fn device_event_payload(db: &DatabaseTransaction, event: &DeviceEvent) -> Value {
    let group = if event.peer.group_id > 0 {
        device_group::Entity::find_by_id(event.peer.group_id)
            .one(db)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let owner = if event.peer.user_id > 0 {
        user::Entity::find_by_id(event.peer.user_id)
            .one(db)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    json!({
        "event_id": event.event_id,
        "event_type": event.event_type,
        "occurred_at": Utc::now().to_rfc3339(),
        "dedupe_key": format!("device:{}:{}", event.peer.id, event.event_type),
        "source": "rustdesk-console",
        "resource": {
            "type": "device",
            "id": event.peer.id,
            "uuid": event.peer.uuid,
            "name": if event.peer.alias.is_empty() { &event.peer.hostname } else { &event.peer.alias },
            "hostname": event.peer.hostname,
            "username": event.peer.username,
            "platform": event.peer.os,
            "version": event.peer.version,
        },
        "device": {
            "row_id": event.peer.row_id,
            "rustdesk_id": event.peer.id,
            "group_id": event.peer.group_id,
            "group_name": group.map(|g| g.name).unwrap_or_default(),
            "owner_user_id": event.peer.user_id,
            "owner_username": owner.map(|u| u.username).unwrap_or_default(),
            "last_online_time": event.peer.last_online_time,
            "last_online_ip": event.peer.last_online_ip,
        },
        "transition": {
            "from": event.previous_state,
            "to": event.current_state,
            "previous_seen_at": event.previous_seen_at,
            "current_seen_at": event.current_seen_at,
        }
    })
}

fn subscription_view(
    row: webhook_subscription::Model,
    template_id: Option<i32>,
) -> WebhookSubscriptionView {
    WebhookSubscriptionView {
        id: row.id,
        name: row.name,
        url: row.url,
        channel: row.channel,
        topic: row.topic,
        template_id,
        event_types: parse_string_vec(&row.event_types),
        device_ids: parse_string_vec(&row.device_ids),
        device_group_ids: parse_i32_vec(&row.device_group_ids),
        enabled: row.enabled,
        secret_set: !row.secret.is_empty(),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn normalize_event_types(channel: Channel, types: Vec<String>) -> Result<Vec<String>, String> {
    let types = normalize_string_list(types);
    if types.is_empty() {
        let mut defaults = vec![EVENT_DEVICE_ONLINE.to_owned()];
        if channel == Channel::Webhook {
            defaults.push(EVENT_DEVICE_OFFLINE.to_owned());
        }
        return Ok(defaults);
    }
    for kind in &types {
        if !matches!(kind.as_str(), EVENT_DEVICE_ONLINE | EVENT_DEVICE_OFFLINE) {
            return Err(format!("unsupported event type: {kind}"));
        }
    }
    Ok(types)
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty() && !out.iter().any(|item| item == value) {
            out.push(value.to_string());
        }
    }
    out
}

fn normalize_i32_list(values: Vec<i32>) -> Vec<i32> {
    let mut out = Vec::new();
    for value in values.into_iter().filter(|value| *value > 0) {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

fn json_string<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

fn parse_string_vec(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
}

fn parse_i32_vec(value: &str) -> Vec<i32> {
    serde_json::from_str::<Vec<i32>>(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(event_types: &str, device_ids: &str, group_ids: &str) -> webhook_subscription::Model {
        webhook_subscription::Model {
            id: 1,
            name: "s".to_string(),
            url: "https://example.com".to_string(),
            channel: Channel::Webhook.as_str().to_owned(),
            topic: String::new(),
            destination_revision: String::new(),
            secret: String::new(),
            event_types: event_types.to_string(),
            device_ids: device_ids.to_string(),
            device_group_ids: group_ids.to_string(),
            tags: "[]".to_string(),
            enabled: true,
            created_at: None,
            updated_at: None,
        }
    }

    fn event() -> DeviceEvent {
        DeviceEvent {
            event_id: "evt_1".to_string(),
            event_type: EVENT_DEVICE_ONLINE,
            peer: peer::Model {
                row_id: 1,
                id: "100".to_string(),
                cpu: String::new(),
                hostname: String::new(),
                memory: String::new(),
                os: String::new(),
                username: String::new(),
                uuid: String::new(),
                pk: String::new(),
                guid: String::new(),
                version: String::new(),
                user_id: 0,
                last_online_time: 0,
                last_online_ip: String::new(),
                group_id: 2,
                alias: String::new(),
                status: 1,
                force_sysinfo_refresh: false,
                created_at: None,
                updated_at: None,
            },
            previous_state: "offline",
            current_state: "online",
            previous_seen_at: 0,
            current_seen_at: 1,
        }
    }

    async fn fixture() -> (DatabaseConnection, peer::Model) {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for table in [
            schema.create_table_from_entity(peer::Entity),
            schema.create_table_from_entity(device_presence_state::Entity),
            schema.create_table_from_entity(::entity::device_presence_event::Entity),
            schema.create_table_from_entity(webhook_subscription::Entity),
            schema.create_table_from_entity(::entity::notification_template::Entity),
            schema.create_table_from_entity(::entity::notification_template_binding::Entity),
            schema.create_table_from_entity(webhook_delivery::Entity),
        ] {
            db.execute(db.get_database_backend().build(&table))
                .await
                .unwrap();
        }
        let device = peer::ActiveModel {
            id: Set("100".to_owned()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        (db, device)
    }

    #[tokio::test]
    async fn concurrent_observations_enqueue_one_event_for_all_matching_channels() {
        let (db, device) = fixture().await;
        for (channel, url, topic) in [
            (Channel::Webhook, "https://example.org/test", None),
            (
                Channel::Ntfy,
                "https://ntfy.sh",
                Some("device-notify-fixture".to_owned()),
            ),
        ] {
            save_subscription(
                &db,
                WebhookSubscriptionInput {
                    id: 0,
                    name: "fixture".to_owned(),
                    url: url.to_owned(),
                    channel: Some(channel),
                    topic,
                    template_id: None,
                    secret: String::new(),
                    clear_secret: false,
                    event_types: vec![],
                    device_ids: vec!["100".to_owned()],
                    device_group_ids: vec![],
                    enabled: true,
                },
            )
            .await
            .unwrap();
        }
        let (first, repeat) = tokio::join!(
            mark_device_seen(&db, device.clone()),
            mark_device_seen(&db, device),
        );
        first.unwrap();
        repeat.unwrap();
        assert_eq!(
            ::entity::device_presence_event::Entity::find()
                .count(&db)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            device_presence_state::Entity::find()
                .count(&db)
                .await
                .unwrap(),
            1
        );
        let queued = webhook_delivery::Entity::find()
            .order_by_asc(webhook_delivery::Column::Id)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(queued.len(), 2);
        assert_eq!(queued[0].event_id, queued[1].event_id);
        assert_eq!(queued[0].channel, "webhook");
        assert_eq!(queued[1].channel, "ntfy");
        assert!(queued
            .iter()
            .all(|row| row.status == webhook_delivery::STATUS_PENDING
                && row.event_type == EVENT_DEVICE_ONLINE
                && row.attempt == 0
                && row.execute_at > 0));
    }

    #[tokio::test]
    async fn device_activity_records_timeout_once_and_observes_recovery() {
        use ::entity::device_presence_event;

        let (db, device) = fixture().await;
        let started_at = Utc::now().timestamp();
        let age_presence = || async {
            device_presence_state::Entity::update_many()
                .col_expr(
                    device_presence_state::Column::LastSeenAt,
                    (started_at - 120).into(),
                )
                .filter(device_presence_state::Column::PeerRowId.eq(device.row_id))
                .exec(&db)
                .await
                .unwrap();
        };

        mark_device_seen(&db, device.clone()).await.unwrap();
        age_presence().await;
        scan_offline_devices(&db).await.unwrap();
        scan_offline_devices(&db).await.unwrap();
        mark_device_seen(&db, device.clone()).await.unwrap();
        age_presence().await;
        // This heartbeat observes the timeout before the worker can scan.
        mark_device_seen(&db, device.clone()).await.unwrap();
        mark_device_seen(&db, device.clone()).await.unwrap();
        scan_offline_devices(&db).await.unwrap();

        let events = device_presence_event::Entity::find()
            .order_by_asc(device_presence_event::Column::Id)
            .all(&db)
            .await
            .unwrap();
        let states: Vec<_> = events.iter().map(|event| event.online).collect();
        assert_eq!(states, [true, false, true, false, true]);
        assert!(
            events
                .iter()
                .all(|event| event.peer_row_id == device.row_id && event.observed_at >= started_at)
        );
        assert_eq!(events[3].observed_at, events[4].observed_at);
    }

    #[tokio::test]
    async fn device_activity_record_failure_rolls_back_presence_change() {
        let (db, device) = fixture().await;
        db.execute_unprepared(
            "CREATE TRIGGER reject_presence_history BEFORE INSERT ON device_presence_events
             BEGIN SELECT RAISE(FAIL, 'record failed'); END",
        )
        .await
        .unwrap();
        assert!(mark_device_seen(&db, device).await.is_err());
        assert_eq!(
            device_presence_state::Entity::find()
                .count(&db)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn subscription_edits_never_reuse_another_channels_secret() {
        let (db, _) = fixture().await;
        let saved = save_subscription(
            &db,
            WebhookSubscriptionInput {
                id: 0,
                name: "fixture".to_owned(),
                url: "https://example.org/hook".to_owned(),
                channel: None,
                topic: None,
                template_id: None,
                secret: "fixture-hmac-key".to_owned(),
                clear_secret: false,
                event_types: vec![],
                device_ids: vec![],
                device_group_ids: vec![],
                enabled: true,
            },
        )
        .await
        .unwrap();
        let updated = save_subscription(
            &db,
            WebhookSubscriptionInput {
                id: saved.id,
                name: "fixture".to_owned(),
                url: "https://ntfy.sh".to_owned(),
                channel: Some(Channel::Ntfy),
                topic: Some("device-notify-fixture".to_owned()),
                template_id: None,
                secret: String::new(),
                clear_secret: false,
                event_types: vec![],
                device_ids: vec![],
                device_group_ids: vec![],
                enabled: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.channel, "ntfy");
        assert_eq!(updated.event_types, vec![EVENT_DEVICE_ONLINE]);
        assert!(!updated.secret_set);
        assert!(!serde_json::to_string(&updated)
            .unwrap()
            .contains("fixture-hmac-key"));
    }

    #[tokio::test]
    async fn stale_duplicate_presence_rows_do_not_emit_offline_for_an_online_device() {
        let (db, device) = fixture().await;
        let ts = Utc::now().timestamp();
        for last_seen in [ts - 200, ts] {
            device_presence_state::ActiveModel {
                peer_row_id: Set(device.row_id),
                peer_id: Set(device.id.clone()),
                online: Set(true),
                last_seen_at: Set(last_seen),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        save_subscription(
            &db,
            WebhookSubscriptionInput {
                name: "fixture".to_owned(),
                url: "https://example.org".to_owned(),
                enabled: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        scan_offline_devices(&db).await.unwrap();
        assert_eq!(
            webhook_delivery::Entity::find().count(&db).await.unwrap(),
            0
        );
        assert_eq!(
            device_presence_state::Entity::find()
                .filter(device_presence_state::Column::Online.eq(true))
                .count(&db)
                .await
                .unwrap(),
            1
        );
        device_presence_state::Entity::update_many()
            .col_expr(
                device_presence_state::Column::LastSeenAt,
                sea_orm::sea_query::Expr::value(ts - 100),
            )
            .filter(device_presence_state::Column::Online.eq(true))
            .exec(&db)
            .await
            .unwrap();
        scan_offline_devices(&db).await.unwrap();
        let event = webhook_delivery::Entity::find()
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.event_type, EVENT_DEVICE_OFFLINE);
        assert_eq!(
            webhook_delivery::Entity::find().count(&db).await.unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn template_binding_is_reusable_and_outbox_keeps_the_rendered_snapshot() {
        use notification_template::{Input, Part, Variable};
        let (db, device) = fixture().await;
        let input = |id, text: &str| Input {
            id,
            name: "fixture template".to_owned(),
            title: vec![
                Part::Text {
                    text: text.to_owned(),
                },
                Part::Variable {
                    name: Variable::DeviceId,
                },
            ],
            body: vec![Part::Text {
                text: "Hello {{device_id}}".to_owned(),
            }],
        };
        let template = notification_template::save(&db, input(0, "Initial "))
            .await
            .unwrap();
        let mut subscriptions = Vec::new();
        for (channel, topic) in [
            (Channel::Webhook, None),
            (Channel::Ntfy, Some("device".to_owned())),
        ] {
            subscriptions.push(
                save_subscription(
                    &db,
                    WebhookSubscriptionInput {
                        name: "fixture".to_owned(),
                        url: "https://example.org".to_owned(),
                        channel: Some(channel),
                        topic,
                        enabled: true,
                        template_id: Some(template.id),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
            );
        }
        assert!(notification_template::delete(&db, template.id)
            .await
            .is_err());
        assert!(subscriptions
            .iter()
            .all(|rule| rule.template_id == Some(template.id)));
        mark_device_seen(&db, device).await.unwrap();
        notification_template::save(&db, input(template.id, "Changed "))
            .await
            .unwrap();
        let jobs = webhook_delivery::Entity::find().all(&db).await.unwrap();
        assert_eq!(jobs.len(), 2);
        for job in jobs {
            let payload: Value = serde_json::from_str(&job.request_body).unwrap();
            assert_eq!(payload["notification"]["title"], "Initial 100");
            assert_eq!(payload["notification"]["body"], "Hello {{device_id}}");
        }
        for rule in subscriptions {
            delete_subscription(&db, rule.id).await.unwrap();
        }
        notification_template::delete(&db, template.id)
            .await
            .unwrap();
    }

    #[test]
    fn subscription_filter_matches_event_device_and_group() {
        assert!(subscription_matches(
            &sub(r#"["device.online"]"#, r#"["100"]"#, "[2]"),
            &event()
        ));
        assert!(!subscription_matches(
            &sub(r#"["device.offline"]"#, r#"["100"]"#, "[2]"),
            &event()
        ));
        assert!(!subscription_matches(
            &sub(r#"["device.online"]"#, r#"["200"]"#, "[2]"),
            &event()
        ));
        assert!(!subscription_matches(
            &sub(r#"["device.online"]"#, r#"["100"]"#, "[3]"),
            &event()
        ));
    }
}

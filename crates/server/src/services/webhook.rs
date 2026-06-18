use std::time::Duration;

use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::Client;
use sea_orm::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use uuid::Uuid;

use ::entity::{
    device_group, device_presence_state, peer, user, webhook_delivery, webhook_subscription,
};

use crate::services::{now, paginate};

pub const EVENT_DEVICE_ONLINE: &str = "device.online";
pub const EVENT_DEVICE_OFFLINE: &str = "device.offline";
const ONLINE_WINDOW_SECONDS: i64 = 90;

#[derive(Debug, Clone, Deserialize)]
pub struct WebhookSubscriptionInput {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
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
    Ok(WebhookSubscriptionList {
        list: rows.into_iter().map(subscription_view).collect(),
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn save_subscription(
    db: &DatabaseConnection,
    input: WebhookSubscriptionInput,
) -> Result<WebhookSubscriptionView, String> {
    let name = input.name.trim().to_string();
    let url = input.url.trim().to_string();
    if name.is_empty() || url.is_empty() {
        return Err("name and webhook URL are required".to_string());
    }
    let parsed = url::Url::parse(&url).map_err(|_| "invalid webhook URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("webhook URL must start with http:// or https://".to_string());
    }
    let event_types = normalize_event_types(input.event_types)?;
    let device_ids = normalize_string_list(input.device_ids);
    let device_group_ids = normalize_i32_list(input.device_group_ids);

    let row = if input.id > 0 {
        let row = webhook_subscription::Entity::find_by_id(input.id)
            .one(db)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("webhook subscription {} not found", input.id))?;
        let current_secret = row.secret.clone();
        let mut am: webhook_subscription::ActiveModel = row.into();
        am.name = Set(name);
        am.url = Set(url);
        am.secret = Set(if input.clear_secret {
            String::new()
        } else if input.secret.trim().is_empty() {
            current_secret
        } else {
            input.secret.trim().to_string()
        });
        am.event_types = Set(json_string(&event_types)?);
        am.device_ids = Set(json_string(&device_ids)?);
        am.device_group_ids = Set(json_string(&device_group_ids)?);
        am.enabled = Set(input.enabled);
        am.updated_at = Set(now());
        am.update(db).await.map_err(|e| e.to_string())?
    } else {
        webhook_subscription::ActiveModel {
            name: Set(name),
            url: Set(url),
            secret: Set(if input.secret.trim().is_empty() {
                crate::support::random::random_string(32)
            } else {
                input.secret.trim().to_string()
            }),
            event_types: Set(json_string(&event_types)?),
            device_ids: Set(json_string(&device_ids)?),
            device_group_ids: Set(json_string(&device_group_ids)?),
            tags: Set("[]".to_string()),
            enabled: Set(input.enabled),
            created_at: Set(now()),
            updated_at: Set(now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .map_err(|e| e.to_string())?
    };

    Ok(subscription_view(row))
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
    let sub = webhook_subscription::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("webhook subscription {id} not found"))?;
    let event_id = format!("evt_{}", Uuid::new_v4());
    let payload = json!({
        "event_id": event_id,
        "event_type": "webhook.test",
        "occurred_at": Utc::now().to_rfc3339(),
        "source": "rustdesk-console",
        "test": true,
    });
    deliver_to_subscription(db, &sub, &event_id, "webhook.test", &payload).await
}

pub fn spawn_device_seen(db: DatabaseConnection, p: peer::Model) {
    tokio::spawn(async move {
        if let Err(e) = mark_device_seen(&db, p).await {
            tracing::warn!("failed to update device presence: {e}");
        }
    });
}

pub async fn mark_device_seen(db: &DatabaseConnection, p: peer::Model) -> Result<(), DbErr> {
    let now_ts = Utc::now().timestamp();
    let existing = device_presence_state::Entity::find()
        .filter(device_presence_state::Column::PeerRowId.eq(p.row_id))
        .one(db)
        .await?;
    match existing {
        Some(row) => {
            let should_emit = !row.online;
            let previous_seen_at = row.last_seen_at;
            let mut am: device_presence_state::ActiveModel = row.into();
            am.peer_id = Set(p.id.clone());
            am.online = Set(true);
            am.last_seen_at = Set(now_ts);
            if should_emit {
                am.last_changed_at = Set(now_ts);
            }
            am.updated_at = Set(now());
            am.update(db).await?;
            if should_emit {
                emit_device_event(
                    db,
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
                .await;
            }
        }
        None => {
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
            .insert(db)
            .await?;
            emit_device_event(
                db,
                DeviceEvent {
                    event_id: format!("evt_{}", Uuid::new_v4()),
                    event_type: EVENT_DEVICE_ONLINE,
                    peer: p,
                    previous_state: "offline",
                    current_state: "online",
                    previous_seen_at: 0,
                    current_seen_at: now_ts,
                },
            )
            .await;
        }
    }
    Ok(())
}

pub fn spawn_presence_worker(db: DatabaseConnection) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(30));
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
        .limit(100)
        .all(db)
        .await?;
    for row in rows {
        let Some(p) = peer::Entity::find_by_id(row.peer_row_id).one(db).await? else {
            let mut am: device_presence_state::ActiveModel = row.into();
            am.online = Set(false);
            am.updated_at = Set(now());
            let _ = am.update(db).await;
            continue;
        };
        let previous_seen_at = row.last_seen_at;
        let mut am: device_presence_state::ActiveModel = row.into();
        am.online = Set(false);
        am.last_changed_at = Set(now_ts);
        am.updated_at = Set(now());
        am.update(db).await?;
        emit_device_event(
            db,
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
        .await;
    }
    Ok(())
}

async fn emit_device_event(db: &DatabaseConnection, event: DeviceEvent) {
    let subscriptions = match matching_subscriptions(db, &event).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("failed to load webhook subscriptions: {e}");
            return;
        }
    };
    if subscriptions.is_empty() {
        return;
    }
    let payload = device_event_payload(db, &event).await;
    for sub in subscriptions {
        if let Err(e) =
            deliver_to_subscription(db, &sub, &event.event_id, event.event_type, &payload).await
        {
            tracing::warn!("webhook delivery failed for subscription {}: {e}", sub.id);
        }
    }
}

async fn matching_subscriptions(
    db: &DatabaseConnection,
    event: &DeviceEvent,
) -> Result<Vec<webhook_subscription::Model>, DbErr> {
    let rows = webhook_subscription::Entity::find()
        .filter(webhook_subscription::Column::Enabled.eq(true))
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .filter(|sub| subscription_matches(sub, event))
        .collect())
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
    if !group_ids.is_empty() && !group_ids.iter().any(|id| *id == event.peer.group_id) {
        return false;
    }
    true
}

async fn device_event_payload(db: &DatabaseConnection, event: &DeviceEvent) -> Value {
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

async fn deliver_to_subscription(
    db: &DatabaseConnection,
    sub: &webhook_subscription::Model,
    event_id: &str,
    event_type: &str,
    payload: &Value,
) -> Result<(), String> {
    let body = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    let created = webhook_delivery::ActiveModel {
        subscription_id: Set(sub.id),
        event_id: Set(event_id.to_string()),
        event_type: Set(event_type.to_string()),
        status: Set(webhook_delivery::STATUS_PENDING.to_string()),
        request_body: Set(body.clone()),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .map_err(|e| e.to_string())?;

    let mut request = Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|e| e.to_string())?
        .post(&sub.url)
        .header("content-type", "application/json")
        .header("x-rustdesk-event", event_type)
        .header("x-rustdesk-event-id", event_id);
    if !sub.secret.is_empty() {
        request = request.header("x-rustdesk-signature", signature(&sub.secret, &body));
    }

    let result = request.body(body).send().await;
    let mut am: webhook_delivery::ActiveModel = created.into();
    am.delivered_at = Set(Utc::now().timestamp());
    am.updated_at = Set(now());
    match result {
        Ok(resp) => {
            let status = resp.status().as_u16() as i32;
            let text = resp.text().await.unwrap_or_default();
            am.status_code = Set(status);
            am.response_body = Set(truncate_text(&text, 4000));
            if (200..300).contains(&status) {
                am.status = Set(webhook_delivery::STATUS_SUCCESS.to_string());
                am.update(db).await.map_err(|e| e.to_string())?;
                Ok(())
            } else {
                am.status = Set(webhook_delivery::STATUS_FAILED.to_string());
                am.error = Set(format!("HTTP {status}"));
                am.update(db).await.map_err(|e| e.to_string())?;
                Err(format!("HTTP {status}"))
            }
        }
        Err(e) => {
            am.status = Set(webhook_delivery::STATUS_FAILED.to_string());
            am.error = Set(e.to_string());
            am.update(db).await.map_err(|e| e.to_string())?;
            Err(e.to_string())
        }
    }
}

fn subscription_view(row: webhook_subscription::Model) -> WebhookSubscriptionView {
    WebhookSubscriptionView {
        id: row.id,
        name: row.name,
        url: row.url,
        event_types: parse_string_vec(&row.event_types),
        device_ids: parse_string_vec(&row.device_ids),
        device_group_ids: parse_i32_vec(&row.device_group_ids),
        enabled: row.enabled,
        secret_set: !row.secret.is_empty(),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn normalize_event_types(types: Vec<String>) -> Result<Vec<String>, String> {
    let types = normalize_string_list(types);
    if types.is_empty() {
        return Ok(vec![
            EVENT_DEVICE_ONLINE.to_string(),
            EVENT_DEVICE_OFFLINE.to_string(),
        ]);
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

fn signature(secret: &str, body: &str) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body.as_bytes());
    format!("sha256={}", hex_encode(&mac.finalize().into_bytes()))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn truncate_text(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        value.chars().take(max).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(event_types: &str, device_ids: &str, group_ids: &str) -> webhook_subscription::Model {
        webhook_subscription::Model {
            id: 1,
            name: "s".to_string(),
            url: "https://example.com".to_string(),
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

//! Wake-on-LAN controls for Console administrators managing an existing device.

use axum::{
    Json,
    extract::{Path, State},
    response::Response,
};
use entity::{peer, user};
use sea_orm::{ActiveModelTrait, Set};
use serde::{Deserialize, Serialize};

use crate::{
    services,
    state::AppState,
    support::wake_on_lan::{MacAddress, SendError},
};

use super::{
    middleware::{AcceptLang, AdminUser},
    response as resp,
    security_audit::AuditContext,
};

#[derive(Debug, Serialize)]
struct DeviceSettings {
    mac_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    manufacturer: Option<String>,
    network_configured: bool,
    device_enabled: bool,
}

impl DeviceSettings {
    async fn read(state: &AppState, peer: peer::Model) -> Self {
        let manufacturer = peer
            .wake_mac_address
            .as_deref()
            .and_then(|text| text.parse::<MacAddress>().ok()?.manufacturer());
        Self {
            mac_address: peer.wake_mac_address,
            manufacturer,
            network_configured: state.wake_on_lan.available().await,
            device_enabled: peer.status == user::STATUS_ENABLE,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SaveSettings {
    mac_address: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendRequest {}

#[derive(Debug, Serialize)]
struct PacketSent {
    packet_sent: bool,
    sent_at: i64,
}

async fn find_device(state: &AppState, id: i32) -> Result<peer::Model, &'static str> {
    match services::peer::info_by_row_id(&state.db, id).await {
        Ok(Some(peer)) => Ok(peer),
        Ok(None) => Err("ItemNotFound"),
        Err(error) => {
            tracing::error!(%error, row_id = id, "Wake-on-LAN device lookup failed");
            Err("OperationFailed")
        }
    }
}

pub(crate) async fn settings(
    State(state): State<AppState>,
    _admin: AdminUser,
    AcceptLang(lang): AcceptLang,
    Path(id): Path<i32>,
) -> Response {
    match find_device(&state, id).await {
        Ok(peer) => resp::no_store(resp::success(DeviceSettings::read(&state, peer).await)),
        Err(message) => resp::fail(101, state.tr(&lang, message)),
    }
}

pub(crate) async fn save(
    State(state): State<AppState>,
    _admin: AdminUser,
    AcceptLang(lang): AcceptLang,
    Path(id): Path<i32>,
    audit: AuditContext,
    Json(input): Json<SaveSettings>,
) -> Response {
    audit.subject(id);
    let mac_address = if input.mac_address.trim().is_empty() {
        None
    } else {
        match input.mac_address.parse::<MacAddress>() {
            Ok(mac_address) => Some(mac_address.to_string()),
            Err(()) => return resp::fail(101, state.tr(&lang, "WakeOnLanInvalidMac")),
        }
    };

    let mut peer = match find_device(&state, id).await {
        Ok(peer) => peer,
        Err(message) => return resp::fail(101, state.tr(&lang, message)),
    };
    if peer.wake_mac_address != mac_address {
        let update = peer::ActiveModel {
            row_id: Set(id),
            wake_mac_address: Set(mac_address),
            ..Default::default()
        };
        peer = match update.update(&state.db).await {
            Ok(peer) => peer,
            Err(error) => {
                tracing::error!(%error, row_id = id, "Wake-on-LAN MAC update failed");
                return resp::fail(101, state.tr(&lang, "OperationFailed"));
            }
        };
    }
    resp::no_store(resp::success(DeviceSettings::read(&state, peer).await))
}

pub(crate) async fn send(
    State(state): State<AppState>,
    _admin: AdminUser,
    AcceptLang(lang): AcceptLang,
    Path(id): Path<i32>,
    audit: AuditContext,
    Json(_): Json<SendRequest>,
) -> Response {
    audit.subject(id);
    let peer = match find_device(&state, id).await {
        Ok(peer) => peer,
        Err(message) => return resp::fail(101, state.tr(&lang, message)),
    };
    if peer.status != user::STATUS_ENABLE {
        return resp::fail(101, state.tr(&lang, "WakeOnLanDeviceDisabled"));
    }
    let Some(mac_address) = peer
        .wake_mac_address
        .as_deref()
        .and_then(|text| text.parse::<MacAddress>().ok())
    else {
        return resp::fail(101, state.tr(&lang, "WakeOnLanMissingMac"));
    };
    match state.wake_on_lan.send(id, mac_address).await {
        Ok(()) => resp::no_store(resp::success(PacketSent {
            packet_sent: true,
            sent_at: chrono::Utc::now().timestamp(),
        })),
        Err(SendError::NoBroadcastInterface) => {
            resp::fail(101, state.tr(&lang, "WakeOnLanNoBroadcastInterface"))
        }
        Err(SendError::RateLimited) => resp::fail(429, state.tr(&lang, "WakeOnLanRateLimited")),
        Err(error) => {
            tracing::warn!(%error, row_id = id, "Wake-on-LAN packet send failed");
            resp::fail(101, state.tr(&lang, "WakeOnLanSendFailed"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use entity::{console_audit, device_group, system_setting, user_token};
    use sea_orm::{ConnectionTrait, Database, DbBackend, EntityTrait, Schema};
    use serde_json::{Value, json};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::net::UdpSocket;
    use tower::ServiceExt;

    use crate::services::console_roles::{self, ConsoleRole};
    use crate::support::wake_on_lan::WakeOnLan;

    const SETTINGS: &str = "/api/admin/peer/wake-on-lan/1";
    const SEND: &str = "/api/admin/peer/wake-on-lan/1/send";

    async fn fixture(transport: WakeOnLan) -> (AppState, Router) {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for statement in [
            schema.create_table_from_entity(user::Entity),
            schema.create_table_from_entity(user_token::Entity),
            schema.create_table_from_entity(device_group::Entity),
            schema.create_table_from_entity(peer::Entity),
            schema.create_table_from_entity(console_audit::Entity),
            schema.create_table_from_entity(system_setting::Entity),
        ] {
            db.execute(DbBackend::Sqlite.build(&statement))
                .await
                .unwrap();
        }
        for (id, admin) in [(1, true), (2, false)] {
            user::ActiveModel {
                id: Set(id),
                username: Set(format!("test-user-{id}")),
                is_admin: Set(Some(admin)),
                tfa_enabled: Set(true),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
            user_token::ActiveModel {
                user_id: Set(id),
                token: Set(format!("test-token-{id}")),
                auth_level: Set(services::user::AUTH_TOTP),
                expired_at: Set(chrono::Utc::now().timestamp() + 600),
                created_at: Set(services::now()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        device_group::ActiveModel {
            id: Set(1),
            name: Set("Test group".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        console_roles::assign(
            &db,
            2,
            ConsoleRole::DeviceGroupOperator { group_ids: vec![1] },
        )
        .await
        .unwrap();
        peer::ActiveModel {
            row_id: Set(1),
            id: Set("123456789".to_string()),
            group_id: Set(1),
            last_online_time: Set(100),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let mut state = crate::http::api::deployment_tests::test_state(db);
        state.wake_on_lan = Arc::new(transport);
        let router = crate::http::router::build(state.clone());
        (state, router)
    }

    async fn request(app: &Router, method: &str, path: &str, actor: i32, body: Value) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if actor > 0 {
            request = request.header("api-token", format!("test-token-{actor}"));
        }
        app.clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }

    async fn body(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn sends_the_stored_mac_to_each_endpoint_and_audits_the_device() {
        let first = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let second = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (state, app) = fixture(WakeOnLan::with_loopback_destinations(&[
            first.local_addr().unwrap().port(),
            second.local_addr().unwrap().port(),
        ]))
        .await;

        let saved = body(
            request(
                &app,
                "PUT",
                SETTINGS,
                1,
                json!({"mac_address": "00-1b-63-12-34-56"}),
            )
            .await,
        )
        .await;
        assert_eq!(saved["code"], 0, "{saved}");
        assert_eq!(saved["data"]["mac_address"], "00:1B:63:12:34:56");
        assert!(
            saved["data"]["manufacturer"]
                .as_str()
                .is_some_and(|name| name.contains("Apple"))
        );

        let injection = request(
            &app,
            "POST",
            SEND,
            1,
            json!({"destination": "203.0.113.4:1234", "mac_address": "02:00:00:00:00:01"}),
        )
        .await;
        assert_eq!(injection.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let sent = body(request(&app, "POST", SEND, 1, json!({})).await).await;
        assert_eq!(sent["data"]["packet_sent"], true);
        assert!(sent["data"]["sent_at"].as_i64().unwrap() > 100);
        let mut bytes = [0; 256];
        for socket in [&first, &second] {
            let count = tokio::time::timeout(Duration::from_secs(2), socket.recv(&mut bytes))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(count, 102);
            assert_eq!(&bytes[..6], &[0xff; 6]);
            let (repeats, remainder) = bytes[6..count].as_chunks::<6>();
            assert!(remainder.is_empty());
            assert_eq!(repeats, &[[0x00, 0x1B, 0x63, 0x12, 0x34, 0x56]; 16]);
        }
        let detail =
            body(request(&app, "GET", "/api/admin/peer/detail/1", 1, Value::Null).await).await;
        assert_eq!(detail["data"]["last_online_time"], 100);
        assert!(detail["data"].get("wake_mac_address").is_none());
        let limited = body(request(&app, "POST", SEND, 1, json!({})).await).await;
        assert_eq!(limited["code"], 429);
        for socket in [&first, &second] {
            assert_eq!(
                socket.try_recv(&mut bytes).unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        }

        let entries = console_audit::Entity::find().all(&state.db).await.unwrap();
        for (action, result) in [
            ("devices.wake_mac_update", "success"),
            ("devices.wake", "success"),
            ("devices.wake", "failed"),
        ] {
            assert!(entries.iter().any(|entry| entry.action == action
                && entry.result == result
                && entry.actor_id == 1
                && entry.subject_id == Some(1)));
        }
    }

    #[tokio::test]
    async fn rejects_requests_outside_admin_scope_and_without_wake_prerequisites() {
        let (state, app) = fixture(WakeOnLan::with_loopback_destinations(&[])).await;
        for endpoint in [("GET", SETTINGS), ("PUT", SETTINGS), ("POST", SEND)] {
            for (actor, code) in [(0, resp::CODE_NEED_LOGIN), (2, resp::CODE_NO_ACCESS)] {
                let result =
                    body(request(&app, endpoint.0, endpoint.1, actor, json!({})).await).await;
                assert_eq!(result["code"], code, "{endpoint:?} actor {actor}: {result}");
            }
        }

        let nonexistent = body(
            request(
                &app,
                "PUT",
                "/api/admin/peer/wake-on-lan/999",
                1,
                json!({"mac_address": "02:00:00:00:00:01"}),
            )
            .await,
        )
        .await;
        assert_eq!(nonexistent["code"], 101);
        assert!(
            peer::Entity::find_by_id(999)
                .one(&state.db)
                .await
                .unwrap()
                .is_none()
        );

        let missing = body(request(&app, "POST", SEND, 1, json!({})).await).await;
        assert_eq!(missing["message"], state.tr("en", "WakeOnLanMissingMac"));
        let invalid = body(
            request(
                &app,
                "PUT",
                SETTINGS,
                1,
                json!({"mac_address": "FF:FF:FF:FF:FF:FF"}),
            )
            .await,
        )
        .await;
        assert_eq!(invalid["message"], state.tr("en", "WakeOnLanInvalidMac"));
        let saved = body(
            request(
                &app,
                "PUT",
                SETTINGS,
                1,
                json!({"mac_address": "02:00:00:00:00:01"}),
            )
            .await,
        )
        .await;
        assert_eq!(saved["data"]["network_configured"], false);
        assert!(saved["data"].get("manufacturer").is_none());

        let disabled_transport = body(request(&app, "POST", SEND, 1, json!({})).await).await;
        assert_eq!(
            disabled_transport["message"],
            state.tr("en", "WakeOnLanNoBroadcastInterface")
        );
        peer::ActiveModel {
            row_id: Set(1),
            status: Set(user::STATUS_DISABLED),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        let disabled_peer = body(request(&app, "POST", SEND, 1, json!({})).await).await;
        assert_eq!(
            disabled_peer["message"],
            state.tr("en", "WakeOnLanDeviceDisabled")
        );

        let removed =
            body(request(&app, "PUT", SETTINGS, 1, json!({"mac_address": ""})).await).await;
        assert_eq!(removed["code"], 0);
        assert_eq!(removed["data"]["mac_address"], Value::Null);
        assert!(
            peer::Entity::find_by_id(1)
                .one(&state.db)
                .await
                .unwrap()
                .unwrap()
                .wake_mac_address
                .is_none()
        );
    }
}

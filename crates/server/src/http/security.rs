//! Authorization at the boundary between untrusted clients and device management.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use entity::user;
use serde::Deserialize;
use serde_json::json;

use super::middleware::RustClientUser;
use crate::{services, state::AppState};

/// Permit a report only for an already enrolled device owned by the account.
/// Legacy mode supports stock clients but never grants management authority.
pub(crate) async fn authorize_report(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
    uuid: &str,
) -> Result<(), StatusCode> {
    let denied = || StatusCode::UNAUTHORIZED;
    if id.is_empty() || uuid.is_empty() {
        return Err(denied());
    }
    let account = match headers.get("authorization") {
        Some(header) => {
            let token = header
                .to_str()
                .ok()
                .and_then(|v| v.strip_prefix("Bearer "))
                .ok_or_else(denied)?;
            let (account, _) = services::user::info_by_access_token(&state.db, token)
                .await
                .map_err(|_| denied())?
                .ok_or_else(denied)?;
            if account.must_change_password {
                return Err(denied());
            }
            Some(account)
        }
        None if state.config.security.allow_legacy_device_reports => None,
        None => return Err(denied()),
    };
    let device = services::peer::find_by_id(&state.db, id)
        .await
        .map_err(|_| denied())?;
    match device {
        Some(device) if device.status == user::STATUS_ENABLE && device.uuid == uuid => {
            if account
                .as_ref()
                .is_some_and(|u| !u.is_admin() && u.id != device.user_id)
            {
                return Err(denied());
            }
            Ok(())
        }
        // Only legacy inventory discovery may create an unowned record.
        None if account.is_none() => Ok(()),
        _ => Err(denied()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionRequest {
    id: String,
}

/// Used by hbbs on every new punch/relay request; database state is authoritative.
pub async fn authorize_connection(
    State(state): State<AppState>,
    caller: RustClientUser,
    Json(request): Json<ConnectionRequest>,
) -> Response {
    let allowed =
        if caller.user.must_change_password || request.id.is_empty() || request.id.len() > 128 {
            false
        } else {
            connection_allowed(&state, &caller.user, &request.id)
                .await
                .unwrap_or(false)
        };
    Json(json!({"allowed": allowed})).into_response()
}

async fn connection_allowed(
    state: &AppState,
    caller: &user::Model,
    id: &str,
) -> Result<bool, sea_orm::DbErr> {
    let Some(device) = services::peer::find_by_id(&state.db, id).await? else {
        return Ok(false);
    };
    if device.status != user::STATUS_ENABLE {
        return Ok(false);
    }
    if caller.is_admin() {
        return Ok(true);
    }
    if device.user_id <= 0 {
        return Ok(false);
    }
    let Some(owner) = services::user::info_by_id(&state.db, device.user_id).await? else {
        return Ok(false);
    };
    if !owner.is_enabled() {
        return Ok(false);
    }
    // Address books are editable inventory. Their membership or sharing rules
    // cannot establish a device-level connection grant.
    Ok(device.user_id == caller.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use entity::{
        address_book, address_book_collection, address_book_collection_rule, peer, user_token,
    };
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, ConnectionTrait, Database, DbBackend, EntityTrait,
        QueryFilter, Schema, Set,
    };
    use tower::ServiceExt;

    async fn fixture() -> AppState {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for table in [
            schema.create_table_from_entity(user::Entity),
            schema.create_table_from_entity(user_token::Entity),
            schema.create_table_from_entity(peer::Entity),
            schema.create_table_from_entity(address_book::Entity),
            schema.create_table_from_entity(address_book_collection::Entity),
            schema.create_table_from_entity(address_book_collection_rule::Entity),
            schema.create_table_from_entity(entity::system_setting::Entity),
            schema.create_table_from_entity(entity::login_log::Entity),
            schema.create_table_from_entity(entity::strategy_assignment::Entity),
            schema.create_table_from_entity(entity::trusted_login_device::Entity),
            schema.create_table_from_entity(entity::audit_conn::Entity),
            schema.create_table_from_entity(entity::share_record::Entity),
        ] {
            db.execute(db.get_database_backend().build(&table))
                .await
                .unwrap();
        }
        for id in [1, 2, 3] {
            user::ActiveModel {
                id: Set(id),
                username: Set(format!("user-{id}")),
                group_id: Set(id),
                is_admin: Set(Some(id == 3)),
                status: Set(1),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
            user_token::ActiveModel {
                user_id: Set(id),
                token: Set(format!("token-{id}")),
                expired_at: Set(chrono::Utc::now().timestamp() + 3600),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        peer::ActiveModel {
            id: Set("device-1".into()),
            uuid: Set("device-uuid".into()),
            user_id: Set(1),
            status: Set(1),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        crate::http::api::deployment_tests::test_state(db)
    }

    async fn allowed(state: &AppState, token: &str, id: &str) -> bool {
        let response = super::super::router::build(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/server/authorize-connection")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"id":id}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        body["allowed"].as_bool().unwrap_or(false)
    }

    #[tokio::test]
    async fn connection_acl_checks_current_owner_status_and_session() {
        let state = fixture().await;
        assert!(allowed(&state, "token-1", "device-1").await);
        assert!(allowed(&state, "token-3", "device-1").await);
        assert!(!allowed(&state, "token-2", "device-1").await);
        assert!(!allowed(&state, "token-1", "missing").await);
        let device = services::peer::find_by_id(&state.db, "device-1")
            .await
            .unwrap()
            .unwrap();
        peer::ActiveModel {
            row_id: Set(device.row_id),
            status: Set(2),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        assert!(!allowed(&state, "token-3", "device-1").await);
        peer::ActiveModel {
            row_id: Set(device.row_id),
            status: Set(1),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        user::ActiveModel {
            id: Set(1),
            status: Set(2),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        assert!(!allowed(&state, "token-1", "device-1").await);
        user::ActiveModel {
            id: Set(1),
            status: Set(1),
            must_change_password: Set(true),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        assert!(!allowed(&state, "token-1", "device-1").await);
        user::ActiveModel {
            id: Set(1),
            must_change_password: Set(false),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        user_token::Entity::delete_many()
            .filter(user_token::Column::Token.eq("token-1"))
            .exec(&state.db)
            .await
            .unwrap();
        assert!(!allowed(&state, "token-1", "device-1").await);
        user_token::ActiveModel {
            user_id: Set(1),
            token: Set("expired".into()),
            expired_at: Set(1),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        assert!(!allowed(&state, "expired", "device-1").await);
    }

    #[tokio::test]
    async fn address_books_and_shared_collections_never_confer_device_access() {
        let state = fixture().await;
        address_book::ActiveModel {
            id: Set("device-1".into()),
            tags: Set(json!([])),
            user_id: Set(2),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        assert!(!allowed(&state, "token-2", "device-1").await);
        let collection = address_book_collection::ActiveModel {
            user_id: Set(1),
            name: Set("Published".into()),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        address_book::ActiveModel {
            id: Set("device-1".into()),
            tags: Set(json!([])),
            user_id: Set(1),
            collection_id: Set(collection.id),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        let rule = address_book_collection_rule::ActiveModel {
            user_id: Set(1),
            collection_id: Set(collection.id),
            to_id: Set(2),
            r#type: Set(address_book_collection_rule::RULE_TYPE_PERSONAL),
            rule: Set(1),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        assert!(!allowed(&state, "token-2", "device-1").await);
        address_book_collection_rule::Entity::delete_by_id(rule.id)
            .exec(&state.db)
            .await
            .unwrap();
        assert!(!allowed(&state, "token-2", "device-1").await);
    }

    #[tokio::test]
    async fn stock_client_heartbeat_refreshes_online_time_by_default() {
        let state = fixture().await;
        let app = super::super::router::build(state.clone());
        let before = chrono::Utc::now().timestamp();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/heartbeat")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"id": "device-1", "uuid": "device-uuid", "version": "1.4.6"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert!(body.get("strategy").is_some());
        let device = services::peer::find_by_id(&state.db, "device-1")
            .await
            .unwrap()
            .unwrap();
        assert!(device.last_online_time >= before);
        assert_eq!(device.user_id, 1);
    }

    #[tokio::test]
    async fn stock_client_sysinfo_discovers_unowned_devices_by_default() {
        let state = fixture().await;
        let response = super::super::router::build(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sysinfo")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "id": "new-device",
                            "uuid": "new-device-uuid",
                            "hostname": "stock-client",
                            "preset-user-name": "user-2",
                            "preset-device-group-name": "injected-group"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.unwrap(),
            "SYSINFO_UPDATED"
        );
        let device = services::peer::find_by_id(&state.db, "new-device")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(device.hostname, "stock-client");
        assert_eq!(device.uuid, "new-device-uuid");
        assert_eq!(device.user_id, 0);
        assert_eq!(device.group_id, 0);
    }

    #[tokio::test]
    async fn stock_client_audit_reports_are_accepted_by_default() {
        let state = fixture().await;
        let response = super::super::router::build(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/audit/conn")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "id": "device-1", "uuid": "device-uuid", "action": "new",
                            "peer_id": "remote-device", "session_id": "stock-client-session"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let record = entity::audit_conn::Entity::find()
            .one(&state.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.peer_id, "device-1");
        assert_eq!(record.session_id, "stock-client-session");
    }

    #[tokio::test]
    async fn reports_reject_anonymous_and_cross_owner_writes_and_ignore_presets() {
        let mut state = fixture().await;
        Arc::make_mut(&mut state.config)
            .security
            .allow_legacy_device_reports = false;
        let empty = HeaderMap::new();
        assert!(authorize_report(&state, &empty, "device-1", "device-uuid")
            .await
            .is_err());
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer token-2".parse().unwrap());
        assert!(
            authorize_report(&state, &headers, "device-1", "device-uuid")
                .await
                .is_err()
        );
        headers.insert("authorization", "Bearer token-1".parse().unwrap());
        assert!(authorize_report(&state, &headers, "device-1", "wrong-uuid")
            .await
            .is_err());
        assert!(
            authorize_report(&state, &headers, "device-1", "device-uuid")
                .await
                .is_ok()
        );
        Arc::make_mut(&mut state.config)
            .security
            .allow_legacy_device_reports = true;
        let response = super::super::router::build(state.clone()).oneshot(Request::builder().method("POST").uri("/api/sysinfo")
            .header("content-type", "application/json").body(Body::from(json!({"id":"device-1", "uuid":"device-uuid",
                "preset-user-name":"user-2", "preset-strategy-name":"attacker-policy", "preset-device-group-name":"attacker-group",
                "preset-address-book-name":"Injected", "preset-address-book-password":"attacker-password"}).to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let device = services::peer::find_by_id(&state.db, "device-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(device.user_id, 1);
        assert_eq!(device.group_id, 0);
        assert!(address_book_collection::Entity::find()
            .all(&state.db)
            .await
            .unwrap()
            .is_empty());
        assert!(entity::strategy_assignment::Entity::find()
            .all(&state.db)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn anonymous_routes_are_closed_and_login_uuid_does_not_change_ownership() {
        let mut state = fixture().await;
        Arc::make_mut(&mut state.config)
            .security
            .allow_legacy_device_reports = false;
        for path in [
            "/api/heartbeat",
            "/api/sysinfo",
            "/api/audit/conn",
            "/api/audit/file",
            "/api/audit/alarm",
            "/api/record?type=remove&file=dummy.webm",
        ] {
            let response = super::super::router::build(state.clone())
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"id":"device-1","uuid":"device-uuid"}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
        let attacker = services::user::info_by_id(&state.db, 2)
            .await
            .unwrap()
            .unwrap();
        let session = services::user::login(
            &state.db,
            &state.jwt,
            &state.config,
            &attacker,
            services::user::LoginEvent {
                client: "app".into(),
                device_id: "device-1".into(),
                uuid: "device-uuid".into(),
                ip: "192.0.2.1".into(),
                login_type: "account".into(),
                platform: "test".into(),
            },
        )
        .await
        .unwrap();
        services::user::logout(&state.db, attacker.id, &session.token)
            .await
            .unwrap();
        assert_eq!(
            services::peer::find_by_id(&state.db, "device-1")
                .await
                .unwrap()
                .unwrap()
                .user_id,
            1
        );
    }

    #[tokio::test]
    async fn audit_reports_and_notes_cannot_target_another_device() {
        let state = fixture().await;
        services::audit::record_conn_event(
            &state.db,
            &json!({"action":"new", "id":"device-1",
            "peer_id":"victim-device", "uuid":"device-uuid", "session_id":"same-session"}),
        )
        .await
        .unwrap();
        let record = entity::audit_conn::Entity::find()
            .one(&state.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.peer_id, "device-1");
        assert!(services::audit::update_conn_note_by_guid(
            &state.db,
            &record.id.to_string(),
            "forged",
            Some(2)
        )
        .await
        .is_err());
        services::audit::record_conn_event(&state.db, &json!({"id":"unrelated", "uuid":"other", "note":"forged", "session_id":"same-session"})).await.unwrap();
        assert!(entity::audit_conn::Entity::find_by_id(record.id)
            .one(&state.db)
            .await
            .unwrap()
            .unwrap()
            .note
            .is_empty());
        services::audit::update_conn_note_by_guid(
            &state.db,
            &record.id.to_string(),
            "owner note",
            Some(1),
        )
        .await
        .unwrap();
        assert_eq!(
            entity::audit_conn::Entity::find_by_id(record.id)
                .one(&state.db)
                .await
                .unwrap()
                .unwrap()
                .note,
            "owner note"
        );
    }

    #[tokio::test]
    async fn webclient_shares_follow_live_owner_and_target_authority() {
        let state = fixture().await;
        address_book::ActiveModel {
            id: Set("device-1".into()),
            user_id: Set(1),
            tags: Set(json!([])),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        let token = services::share_record::share_by_web_client(
            &state.db,
            1,
            "device-1",
            "temporary",
            "dummy-password",
            3600,
        )
        .await
        .unwrap();
        let query = json!({"share_token": token});
        let response =
            super::super::api::shared_peer(State(state.clone()), Json(query.clone())).await;
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(body["data"]["peer"]["tmppwd"], "dummy-password");
        user::ActiveModel {
            id: Set(1),
            status: Set(2),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        let response =
            super::super::api::shared_peer(State(state.clone()), Json(query.clone())).await;
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(body["code"], 101);
        user::ActiveModel {
            id: Set(1),
            status: Set(1),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        let device = services::peer::find_by_id(&state.db, "device-1")
            .await
            .unwrap()
            .unwrap();
        peer::ActiveModel {
            row_id: Set(device.row_id),
            user_id: Set(2),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        let response = super::super::api::shared_peer(State(state.clone()), Json(query)).await;
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(body["code"], 101);
    }

    #[tokio::test]
    async fn public_device_identifiers_never_exempt_login_verification() {
        let state = fixture().await;
        let mut account = services::user::info_by_id(&state.db, 1)
            .await
            .unwrap()
            .unwrap();
        account.login_device_verification_enabled = true;
        entity::trusted_login_device::ActiveModel {
            user_id: Set(1),
            device_id: Set("device-1".into()),
            device_uuid: Set("device-uuid".into()),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        let device = services::login_security::DeviceLoginInfo {
            id: "device-1".into(),
            uuid: "device-uuid".into(),
            name: String::new(),
            os: String::new(),
            client_type: "app".into(),
            ip: "192.0.2.1".into(),
            auto_login: true,
        };
        assert_eq!(
            services::login_security::required_verification(&state.db, &account, &device)
                .await
                .unwrap(),
            Some(services::login_security::VerificationKind::Email)
        );
    }

    #[tokio::test]
    async fn registration_switch_is_persistent_and_admin_only() {
        let state = fixture().await;
        assert!(
            services::login_security::registration_enabled(&state.db, true)
                .await
                .unwrap()
        );
        services::login_security::save_login_settings(
            &state.db,
            services::login_security::LoginSecuritySettings {
                allow_registration: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(
            !services::login_security::registration_enabled(&state.db, true)
                .await
                .unwrap()
        );
        let app = super::super::router::build(state.clone());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/user/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"new-user","password":"test-password"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(body["code"], 101);
        let response = app.oneshot(Request::builder().method("PATCH").uri("/api/admin/config/login-security")
            .header("api-token", "token-2").header("content-type", "application/json")
            .body(Body::from(r#"{"login":{"require_totp":false,"require_email_verification":false,"require_device_verification":false,"allow_trusted_login_devices":false,"allow_registration":true}}"#)).unwrap()).await.unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_ne!(body["code"], 0);
        assert!(
            !services::login_security::registration_enabled(&state.db, true)
                .await
                .unwrap()
        );
    }

    use std::sync::Arc;
}

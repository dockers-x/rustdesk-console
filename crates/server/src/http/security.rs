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
    if !allowed {
        return Json(json!({"allowed": false})).into_response();
    }
    match services::device_admission::control_role(&state.db, caller.user.id).await {
        Ok(role) if role.mask() == 0 => Json(json!({"allowed": true})).into_response(),
        Ok(role) => Json(json!({"allowed": true, "control_permissions": role.mask()})).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
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
            schema.create_table_from_entity(entity::ldap_identity::Entity),
            schema.create_table_from_entity(user_token::Entity),
            schema.create_table_from_entity(peer::Entity),
            schema.create_table_from_entity(address_book::Entity),
            schema.create_table_from_entity(address_book_collection::Entity),
            schema.create_table_from_entity(address_book_collection_rule::Entity),
            schema.create_table_from_entity(entity::system_setting::Entity),
            schema.create_table_from_entity(entity::login_log::Entity),
            schema.create_table_from_entity(entity::login_verification::Entity),
            schema.create_table_from_entity(entity::control_role::Entity),
            schema.create_table_from_entity(entity::device_admission::Entity),
            schema.create_table_from_entity(entity::routing_settings::Entity),
            schema.create_table_from_entity(entity::strategy_delivery::Entity),
            schema.create_table_from_entity(entity::address_book_trash::Entity),
            schema.create_table_from_entity(entity::strategy_assignment::Entity),
            schema.create_table_from_entity(entity::trusted_login_device::Entity),
            schema.create_table_from_entity(entity::audit_conn::Entity),
            schema.create_table_from_entity(entity::share_record::Entity),
            schema.create_table_from_entity(entity::tag::Entity),
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
                tfa_enabled: Set(id == 3),
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
                created_at: Set(services::now()),
                auth_level: Set(if id == 3 {
                    services::user::AUTH_TOTP
                } else {
                    0
                }),
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
                proof: services::user::LoginProof::SingleFactor,
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

    async fn json_request(
        state: &AppState,
        method: &str,
        path: &str,
        body: serde_json::Value,
        token: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request
                .header("api-token", token)
                .header("authorization", format!("Bearer {token}"));
        }
        let response = super::super::router::build(state.clone())
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let json =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn address_book_rules_control_console_and_native_access_and_recycle() {
        let state = fixture().await;
        let request = |method: &'static str, path: String, data: serde_json::Value,
                       token: &'static str, native: bool| {
            let state = state.clone();
            async move {
                let request = Request::builder().method(method).uri(path)
                    .header("content-type", "application/json");
                let request = if native {
                    request.header("authorization", format!("Bearer {token}"))
                } else {
                    request.header("api-token", token)
                };
                let response = super::super::router::build(state)
                    .oneshot(request.body(Body::from(data.to_string())).unwrap())
                    .await.unwrap();
                let status = response.status();
                let bytes = to_bytes(response.into_body(), 32768).await.unwrap();
                (status, serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_default())
            }
        };
        let (status, book) = request("POST", "/api/ab/shared/add".into(),
            json!({"name":"Operations","note":"Test address book"}), "token-1", false).await;
        assert_eq!(status, StatusCode::OK);
        let guid = book["guid"].as_str().unwrap();
        let (status, _) = request("POST", format!("/api/ab/peer/add/{guid}"),
            json!({"id":"device-1","alias":"Notebook","password":"test-sync-value"}), "token-1", false).await;
        assert_eq!(status, StatusCode::OK);
        let (_, rule) = request("POST", "/api/ab/rule".into(),
            json!({"guid":guid,"rule":1,"user":"user-2"}), "token-1", false).await;
        let rule_id = rule["guid"].as_str().unwrap();
        let peer_list = format!("/api/ab/peers?ab={guid}");
        let (_, visible) = request("GET", peer_list.clone(), json!(null), "token-2", false).await;
        assert_eq!(visible["data"][0]["alias"], "Notebook");
        assert_eq!(visible["data"][0]["password"], "");
        let (_, native) = request("GET", peer_list.clone(), json!(null), "token-2", true).await;
        assert_eq!(native["data"][0]["password"], "test-sync-value");

        for (level, edit_status, trash_status) in [
            (1, StatusCode::BAD_REQUEST, StatusCode::BAD_REQUEST),
            (2, StatusCode::OK, StatusCode::BAD_REQUEST),
            (3, StatusCode::OK, StatusCode::OK),
        ] {
            let (status, _) = request("PATCH", "/api/ab/rule".into(),
                json!({"guid":rule_id,"rule":level}), "token-1", false).await;
            assert_eq!(status, StatusCode::OK);
            let (status, _) = request("PUT", format!("/api/ab/peer/update/{guid}"),
                json!({"id":"device-1","note":"Updated"}), "token-2", false).await;
            assert_eq!(status, edit_status);
            let (status, _) = request("DELETE", format!("/api/ab/peer/{guid}"),
                json!(["device-1"]), "token-2", false).await;
            assert_eq!(status, trash_status);
        }
        let (_, trash) = request("GET", format!("/api/ab/trash/{guid}"), json!(null), "token-2", false).await;
        assert_eq!(trash["total"], 1);
        assert!(!trash.to_string().contains("test-sync-value"));
        assert_eq!(trash["data"][0]["note"], "Updated");
        let row_id = trash["data"][0]["row_id"].as_i64().unwrap();
        let (status, _) = request("POST", format!("/api/ab/trash/restore/{guid}"),
            json!({"id":row_id}), "token-1", false).await;
        assert_eq!(status, StatusCode::OK);
        let (_, visible) = request("GET", peer_list.clone(), json!(null), "token-2", true).await;
        assert_eq!(visible["data"][0]["password"], "test-sync-value");

        let (status, _) = request("DELETE", "/api/ab/rules".into(),
            json!([rule_id, "999999"]), "token-1", false).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = request("GET", peer_list.clone(), json!(null), "token-2", false).await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = request("DELETE", "/api/ab/rules".into(),
            json!([rule_id]), "token-1", false).await;
        assert_eq!(status, StatusCode::OK);
        for native in [false, true] {
            let (status, _) = request("GET", peer_list.clone(), json!(null), "token-2", native).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }
        assert!(!allowed(&state, "token-2", "device-1").await);
    }

    #[tokio::test]
    async fn device_admission_requires_pairing_and_matches_registered_key() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let mut state = fixture().await;
        const TOKEN: &str = "local-fixture-token-with-32-characters";
        Arc::make_mut(&mut state.config).security.hbbs_admission_token = TOKEN.into();
        let (_, before) = json_request(&state, "PUT", "/api/admin/device-admission/settings",
            json!({"require_deployment":true}), Some("token-3")).await;
        assert_ne!(before["code"], 0, "{before}");
        let (_, paired) = json_request(&state, "POST", "/api/server/device-admission/hello",
            json!(null), Some(TOKEN)).await;
        assert_eq!(paired["allowed"], true);
        let (status, _) = json_request(
            &state, "PUT", "/api/admin/device-admission/settings",
            json!({"require_deployment":true}), Some("token-3"),
        ).await;
        assert_eq!(status, StatusCode::OK);
        let device = json!({
            "id": "new-device-1", "uuid": STANDARD.encode([1; 16]),
            "public_key": STANDARD.encode([2; 32]), "observe": true,
        });
        let check = |key: serde_json::Value, token| {
            let state = state.clone();
            async move {
                json_request(&state, "POST", "/api/server/device-admission", key, token).await
            }
        };
        assert_eq!(check(device.clone(), Some("wrong-token")).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(check(device.clone(), Some(TOKEN)).await.1["allowed"], false);
        let (_, list) = json_request(
            &state, "GET", "/api/admin/device-admission/list", json!(null), Some("token-3")
        ).await;
        let fingerprint = list["data"]["list"][0]["fingerprint"].as_str().unwrap();
        let mut form = json!({"device_id":"new-device-1", "fingerprint":fingerprint, "approved":true});
        let (_, saved) = json_request(&state, "POST", "/api/admin/device-admission/review",
            form.clone(), Some("token-3")).await;
        assert_eq!(saved["code"], 0, "{saved}");
        assert_eq!(check(device.clone(), Some(TOKEN)).await.1["allowed"], true);
        let mut changed = device.clone();
        changed["public_key"] = json!(STANDARD.encode([3; 32]));
        assert_eq!(check(changed, Some(TOKEN)).await.1["allowed"], false);
        form["approved"] = json!(false);
        let (_, saved) = json_request(&state, "POST", "/api/admin/device-admission/review",
            form, Some("token-3")).await;
        assert_eq!(saved["code"], 0, "{saved}");
        assert_eq!(check(device, Some(TOKEN)).await.1["allowed"], false);

        let (_, changed) = json_request(&state, "PUT", "/api/admin/user/control-role/1",
            json!({"deny_keyboard":true,"deny_clipboard":true,"deny_file":true}), Some("token-3")).await;
        assert_eq!(changed["code"], 0, "{changed}");
        let (_, policy) = json_request(&state, "POST", "/api/server/authorize-connection",
            json!({"id":"device-1"}), Some("token-1")).await;
        assert_eq!(policy, json!({"allowed":true,"control_permissions":81}));
        let (_, no_policy) = json_request(&state, "POST", "/api/server/authorize-connection",
            json!({"id":"device-1"}), Some("token-2")).await;
        assert_eq!(no_policy, json!({"allowed":false}));
    }

    #[tokio::test]
    async fn legacy_address_book_push_accepts_stock_client_peer_records() {
        let state = fixture().await;
        let other = address_book::ActiveModel {
            id: Set("other-account-peer".into()),
            user_id: Set(2),
            collection_id: Set(0),
            tags: Set(json!([])),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        let mut peer = json!({
            "id": "stock-client-42",
            "username": "remote-user",
            "hostname": "desktop",
            "platform": "Linux",
            "alias": "Office",
            "tags": ["Team"],
            "hash": "",
        });
        for with_server_row in [false, true] {
            if with_server_row {
                peer["row_id"] = json!(other.row_id);
                peer["alias"] = json!("New alias");
            }
            let inner = json!({
                "peers": [&peer],
                "tags": ["Team"],
                "tag_colors": r#"{"Team":4290123456}"#,
            });
            let (status, body) = json_request(
                &state,
                "POST",
                "/api/ab",
                json!({"data": inner.to_string()}),
                Some("token-1"),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert!(body.is_null());
        }
        let (_, pulled) =
            json_request(&state, "GET", "/api/ab", json!(null), Some("token-1")).await;
        let contents: serde_json::Value =
            serde_json::from_str(pulled["data"].as_str().unwrap()).unwrap();
        assert_eq!(contents["peers"][0]["id"], "stock-client-42");
        assert_eq!(contents["peers"][0]["alias"], "New alias");
        assert_ne!(contents["peers"][0]["row_id"], other.row_id);
        assert_eq!(contents["tags"], json!(["Team"]));
        assert_eq!(
            address_book::Entity::find_by_id(other.row_id)
                .one(&state.db)
                .await
                .unwrap(),
            Some(other),
        );
    }

    #[tokio::test]
    async fn unbound_admin_receives_a_console_only_session() {
        let state = fixture().await;
        user::ActiveModel {
            id: Set(3),
            password: Set(crate::support::password::md5_hex(
                "local-passwordrustdesk-api",
            )),
            tfa_enabled: Set(false),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        user_token::Entity::update_many()
            .col_expr(user_token::Column::AuthLevel, sea_orm::sea_query::Expr::value(0))
            .filter(user_token::Column::UserId.eq(3))
            .exec(&state.db)
            .await
            .unwrap();

        let (_, legacy) = json_request(
            &state, "GET", "/api/admin/user/list", json!(null), Some("token-3"),
        )
        .await;
        assert_ne!(legacy["code"], 0, "{legacy}");

        let (_, login) = json_request(
            &state, "POST", "/api/admin/login",
            json!({"username":"user-3", "password":"local-password"}), None,
        )
        .await;
        assert_eq!(login["data"]["must_enroll_totp"], false, "{login}");
        assert_eq!(login["data"]["tfa_enabled"], false);
        let token = login["data"]["token"].as_str().unwrap();
        let (_, users) = json_request(
            &state, "GET", "/api/admin/user/list", json!(null), Some(token),
        )
        .await;
        assert_eq!(users["code"], 0, "{users}");
        let (native, _) =
            json_request(&state, "GET", "/api/user/info", json!(null), Some(token)).await;
        assert_eq!(native, StatusCode::UNAUTHORIZED);
        let (_, reset) = json_request(
            &state, "POST", "/api/admin/user/tfa/reset", json!({"id": 3}), Some(token),
        )
        .await;
        assert_eq!(reset["code"], crate::http::response::CODE_NO_ACCESS, "{reset}");

        services::login_security::save_login_settings(
            &state.db,
            services::login_security::LoginSecuritySettings {
                require_totp: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let (_, denied) = json_request(
            &state, "GET", "/api/admin/user/list", json!(null), Some(token),
        )
        .await;
        assert_ne!(denied["code"], 0, "{denied}");
    }

    #[tokio::test]
    async fn administrator_enrollment_and_totp_are_enforced_across_session_entry_points() {
        let state = fixture().await;
        user::ActiveModel {
            id: Set(3),
            password: Set(crate::support::password::md5_hex(
                "local-passwordrustdesk-api",
            )),
            tfa_enabled: Set(false),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        services::login_security::save_login_settings(
            &state.db,
            services::login_security::LoginSecuritySettings {
                require_totp: true,
                totp_issuer: "Example IT".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let credentials = json!({"username":"user-3", "password":"local-password"});
        let (status, native) =
            json_request(&state, "POST", "/api/login", credentials.clone(), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(native["access_token"].is_null());

        let (_, admin) = json_request(
            &state,
            "POST",
            "/api/admin/login",
            credentials.clone(),
            None,
        )
        .await;
        assert_eq!(admin["data"]["must_enroll_totp"], true, "{admin}");
        let limited = admin["data"]["token"].as_str().unwrap();
        let (_, blocked) = json_request(
            &state,
            "GET",
            "/api/admin/user/list",
            json!(null),
            Some(limited),
        )
        .await;
        assert_eq!(blocked["code"], 114);
        let (blocked, _) =
            json_request(&state, "GET", "/api/user/info", json!(null), Some(limited)).await;
        assert_eq!(blocked, StatusCode::UNAUTHORIZED);

        let (_, setup) = json_request(
            &state,
            "POST",
            "/api/admin/user/myTfaSetup",
            json!(null),
            Some(limited),
        )
        .await;
        assert!(setup["data"]["uri"].as_str().unwrap().contains("issuer=Example%20IT"));
        let secret = setup["data"]["secret"].as_str().unwrap();
        let code = services::login_security::test_totp_code(secret);
        let (_, enrolled) = json_request(
            &state,
            "POST",
            "/api/admin/user/myTfaEnable",
            json!({"secret": secret, "code": code}),
            Some(limited),
        )
        .await;
        assert_eq!(enrolled["code"], 0, "{enrolled}");
        for token in [limited, "token-3"] {
            assert!(services::user::session_by_token(&state.db, token)
                .await
                .unwrap()
                .is_none());
        }

        let (_, challenge) =
            json_request(&state, "POST", "/api/admin/login", credentials, None).await;
        assert_eq!(challenge["data"]["tfa_type"], "tfa_check");
        assert!(challenge["data"]["token"].is_null());
        let completion = json!({
            "secret": challenge["data"]["secret"],
            "tfaCode": services::login_security::test_totp_code(secret),
        });
        let (_, login) =
            json_request(&state, "POST", "/api/admin/login", completion.clone(), None).await;
        assert_eq!(login["data"]["must_enroll_totp"], false, "{login}");
        let verified = login["data"]["token"].as_str().unwrap();
        let (_, list) = json_request(
            &state,
            "GET",
            "/api/admin/user_token/list",
            json!(null),
            Some(verified),
        )
        .await;
        assert_eq!(list["code"], 0, "{list}");
        assert!(!list.to_string().contains(verified));
        let (_, replay) = json_request(&state, "POST", "/api/admin/login", completion, None).await;
        assert_ne!(replay["code"], 0);
    }

    use std::sync::Arc;
}

mod store;

use std::collections::BTreeMap;

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    http::{middleware::AdminUser, response},
    state::AppState,
};

pub use store::{FeatureStore, SettingsUpdate};

pub const MAX_CONTENT_SIZE: usize = 1024 * 1024;
const HARD_SETTINGS: &[&str] = &[
    "disable-settings",
    "disable-ab",
    "disable-account",
    "disable-installation",
    "disable-tcp-listen",
    "conn-type",
];

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SignRequest {
    pub app_name: String,
    #[serde(default)]
    pub branding: BTreeMap<String, String>,
    #[serde(default)]
    pub default_settings: BTreeMap<String, String>,
    #[serde(default)]
    pub override_settings: BTreeMap<String, String>,
}

#[derive(Serialize)]
pub struct SignedArtifact {
    filename: &'static str,
    content: String,
    public_key: String,
    fingerprint: String,
    source_hint: &'static str,
}

impl SignRequest {
    fn payload(self) -> Result<Vec<u8>, &'static str> {
        if self.app_name.trim().is_empty() || self.app_name.len() > 128 {
            return Err("app_name must contain 1 to 128 bytes");
        }
        if self
            .app_name
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\'))
            || matches!(self.app_name.trim(), "." | "..")
        {
            return Err("app_name must be a name, not a path");
        }
        if self
            .branding
            .keys()
            .any(|key| !HARD_SETTINGS.contains(&key.as_str()))
        {
            return Err("unsupported branding key; logos must be compiled as client assets");
        }
        let mut payload = serde_json::Map::new();
        payload.insert(
            "app-name".to_string(),
            serde_json::Value::String(self.app_name),
        );
        payload.insert(
            "default-settings".to_string(),
            serde_json::json!(self.default_settings),
        );
        payload.insert(
            "override-settings".to_string(),
            serde_json::json!(self.override_settings),
        );
        for (key, value) in self.branding {
            payload.insert(key, serde_json::Value::String(value));
        }
        serde_json::to_vec(&payload).map_err(|_| "could not serialize client settings")
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/features/self-signed-client",
            get(settings).put(update),
        )
        .route(
            "/api/admin/features/self-signed-client/keys/generate",
            post(generate),
        )
        .route("/api/admin/features/self-signed-client/sign", post(sign))
        .route(
            "/api/admin/features/self-signed-client/catalog",
            get(catalog),
        )
        .route(
            "/api/admin/features/self-signed-client/profile",
            get(profile).put(save_profile),
        )
        .layer(DefaultBodyLimit::max(MAX_CONTENT_SIZE))
}

async fn settings(State(state): State<AppState>, _user: AdminUser) -> Response {
    match state.features.settings().await {
        Ok(settings) => response::success(settings),
        Err(_) => storage_error(),
    }
}

async fn profile(State(state): State<AppState>, _user: AdminUser) -> Response {
    match state.features.profile().await {
        Ok(profile) => response::success(profile),
        Err(error) => operation_error(error),
    }
}

async fn save_profile(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(profile): Json<SignRequest>,
) -> Response {
    match state.features.save_profile(profile).await {
        Ok(profile) => response::success(profile),
        Err(error) => operation_error(error),
    }
}

async fn update(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(update): Json<SettingsUpdate>,
) -> Response {
    match state.features.update(update).await {
        Ok(settings) => response::success(settings),
        Err(error) => operation_error(error),
    }
}

async fn generate(State(state): State<AppState>, _user: AdminUser) -> Response {
    let mut response = match state.features.generate().await {
        Ok(settings) => response::success(settings),
        Err(error) => operation_error(error),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn sign(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(request): Json<SignRequest>,
) -> Response {
    let payload = match request.payload() {
        Ok(payload) => payload,
        Err(error) => return response::error(error),
    };
    match state.features.sign(payload).await {
        Ok(artifact) => response::success(artifact),
        Err(error) => operation_error(error),
    }
}

async fn catalog(State(state): State<AppState>, _user: AdminUser) -> Response {
    match state.features.settings().await {
        Ok(settings) if settings.enabled => {
            match serde_json::from_str::<serde_json::Value>(include_str!("catalog.json")) {
                Ok(catalog) => response::success(catalog),
                Err(_) => storage_error(),
            }
        }
        Ok(_) => operation_error(store::OperationError::Disabled),
        Err(_) => storage_error(),
    }
}

fn operation_error(error: store::OperationError) -> Response {
    match error {
        store::OperationError::Disabled => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "self-signed client feature is disabled"})),
        )
            .into_response(),
        store::OperationError::Invalid(message) => response::error(message),
        store::OperationError::Storage => storage_error(),
    }
}

pub(super) fn storage_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "could not access self-signed client feature storage"})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use sea_orm::{ActiveModelTrait, ConnectionTrait, Database, DbBackend, Schema, Set};
    use tower::ServiceExt;

    #[test]
    fn disabled_response_and_client_schema_are_explicit() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("catalog.json")).unwrap();
        assert_eq!(catalog["branding"], serde_json::json!(HARD_SETTINGS));
        assert!(catalog["local"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("enable-webrtc")));
        assert_eq!(
            operation_error(store::OperationError::Disabled).status(),
            StatusCode::FORBIDDEN
        );
        assert!(
            serde_json::from_str::<SignRequest>(r#"{"app_name":"OwnDesk","unknown":true}"#)
                .is_err()
        );
        let request = SignRequest {
            app_name: "OwnDesk".to_string(),
            branding: BTreeMap::from([("logo".to_string(), "unsupported".to_string())]),
            ..Default::default()
        };
        assert!(request.payload().is_err());
        let request = SignRequest {
            app_name: "OwnDesk".to_string(),
            branding: BTreeMap::from([("disable-ab".to_string(), "Y".to_string())]),
            ..Default::default()
        };
        let payload: serde_json::Value =
            serde_json::from_slice(&request.payload().unwrap()).unwrap();
        assert_eq!(payload["app-name"], "OwnDesk");
        assert_eq!(payload["disable-ab"], "Y");
        assert!(payload["default-settings"].is_object());
        assert!(payload["override-settings"].is_object());
    }

    #[tokio::test]
    async fn real_routes_enforce_admin_disabled_gate_and_body_limit() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let backend = db.get_database_backend();
        let schema = Schema::new(DbBackend::Sqlite);
        db.execute(backend.build(&schema.create_table_from_entity(entity::user::Entity)))
            .await
            .unwrap();
        db.execute(backend.build(&schema.create_table_from_entity(entity::user_token::Entity)))
            .await
            .unwrap();
        for (user_id, is_admin, token) in [(1, true, "feature-admin"), (2, false, "feature-user")] {
            entity::user::ActiveModel {
                id: Set(user_id),
                username: Set(token.to_string()),
                status: Set(entity::user::STATUS_ENABLE),
                is_admin: Set(Some(is_admin)),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
            entity::user_token::ActiveModel {
                user_id: Set(user_id),
                token: Set(token.to_string()),
                expired_at: Set(chrono::Utc::now().timestamp() + 86400),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let root =
            std::env::temp_dir().join(format!("console-feature-routes-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let mut state = crate::http::api::deployment_tests::test_state(db);
        state.features = Arc::new(FeatureStore::new(&root.join("config.yaml")));
        let app = routes().with_state(state);
        let request = |token: &str, uri: &str, body: String| {
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("api-token", token)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap()
        };
        let response = app
            .clone()
            .oneshot(request(
                "feature-user",
                "/api/admin/features/self-signed-client/sign",
                r#"{"app_name":"OwnDesk"}"#.to_string(),
            ))
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), MAX_CONTENT_SIZE)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["code"], crate::http::response::CODE_NO_ACCESS);
        for uri in [
            "/api/admin/features/self-signed-client/sign",
            "/api/admin/features/self-signed-client/keys/generate",
        ] {
            let response = app
                .clone()
                .oneshot(request(
                    "feature-admin",
                    uri,
                    r#"{"app_name":"OwnDesk"}"#.to_string(),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        for body in [
            r#"{"enabled":false,"private_key":""}"#,
            r#"{"enabled":false,"public_key":""}"#,
        ] {
            let mut request = request(
                "feature-admin",
                "/api/admin/features/self-signed-client",
                body.to_string(),
            );
            *request.method_mut() = axum::http::Method::PUT;
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        for body in [
            serde_json::json!({"enabled": false}),
            serde_json::json!({"enabled": true, "private_key": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [7u8; 32])}),
        ] {
            let mut request = request(
                "feature-admin",
                "/api/admin/features/self-signed-client",
                body.to_string(),
            );
            *request.method_mut() = axum::http::Method::PUT;
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body: serde_json::Value = serde_json::from_slice(
                &to_bytes(response.into_body(), MAX_CONTENT_SIZE)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["code"], 0);
            assert!(body["data"].get("private_key").is_none());
        }
        let response = app
            .clone()
            .oneshot(request(
                "feature-admin",
                "/api/admin/features/self-signed-client/keys/generate",
                String::new(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let body: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), MAX_CONTENT_SIZE)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["code"], 0);
        let encoded_secret = body["data"]["private_key"].as_str().unwrap();
        let secret =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded_secret)
                .unwrap();
        let public = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            body["data"]["public_key"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(secret.len(), 64);
        assert_eq!(public.len(), 32);
        let pair =
            ring::signature::Ed25519KeyPair::from_seed_and_public_key(&secret[..32], &secret[32..])
                .unwrap();
        assert_eq!(ring::signature::KeyPair::public_key(&pair).as_ref(), public);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/features/self-signed-client")
                    .header("api-token", "feature-admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let response_body = to_bytes(response.into_body(), MAX_CONTENT_SIZE)
            .await
            .unwrap();
        let settings: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        assert!(settings["data"].get("private_key").is_none());
        assert!(!String::from_utf8_lossy(&response_body).contains(encoded_secret));
        let response = app
            .oneshot(request(
                "feature-admin",
                "/api/admin/features/self-signed-client/sign",
                "x".repeat(MAX_CONTENT_SIZE + 1),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        std::fs::remove_dir_all(root).unwrap();
    }
}

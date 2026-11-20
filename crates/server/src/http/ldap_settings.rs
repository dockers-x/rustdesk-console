use axum::Json;
use axum::extract::{State, rejection::JsonRejection};
use axum::response::Response;

use super::middleware::{AcceptLang, AdminUser};
use super::response::{fail, no_store, success};
use crate::services::ldap_settings::{self, SettingsError, Update, View};
use crate::state::AppState;

fn failure(state: &AppState, lang: &str, error: SettingsError) -> Response {
    let code = if matches!(error, SettingsError::Storage(_)) {
        500
    } else {
        400
    };
    no_store(fail(code, state.tr(lang, &error.to_string())))
}

pub async fn read(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _admin: AdminUser,
) -> Response {
    match ldap_settings::read(&state.db, &state.config.ldap).await {
        Ok(cfg) => no_store(success(View::from(&cfg))),
        Err(error) => failure(&state, &lang, error),
    }
}

pub async fn save(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _admin: AdminUser,
    body: Result<Json<Update>, JsonRejection>,
) -> Response {
    let Ok(Json(update)) = body else {
        return failure(&state, &lang, SettingsError::Invalid);
    };
    match ldap_settings::save(&state.db, &state.config.ldap, update).await {
        Ok(view) => no_store(success(view)),
        Err(error) => failure(&state, &lang, error),
    }
}

pub async fn test(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _admin: AdminUser,
    body: Result<Json<Update>, JsonRejection>,
) -> Response {
    let Ok(Json(update)) = body else {
        return failure(&state, &lang, SettingsError::Invalid);
    };
    match ldap_settings::test(&state.db, &state.config.ldap, update).await {
        Ok(()) => no_store(success(())),
        Err(error) => failure(&state, &lang, error),
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use sea_orm::{ActiveModelTrait, Set};
    use serde_json::{Value, json};
    use std::sync::Arc;
    use tower::ServiceExt;

    use crate::http::{api::deployment_tests::test_state, router};
    use crate::services::{ldap_settings::tests::test_db, now, user::AUTH_TOTP};

    async fn request(
        app: &axum::Router,
        method: &str,
        path: &str,
        token: &str,
        body: Value,
    ) -> (Value, String) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("api-token", token)
                    .header("Accept-Language", "ru")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let cache = response
            .headers()
            .get("cache-control")
            .map(|value| value.to_str().unwrap().to_owned())
            .unwrap_or_default();
        let bytes = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
        (serde_json::from_slice(&bytes).unwrap(), cache)
    }

    #[tokio::test]
    async fn ldap_http_routes_require_admin_and_keep_passwords_write_only() {
        let mut state = test_state(test_db().await);
        let cfg = Arc::make_mut(&mut state.config);
        cfg.ldap.bind_dn = "cn=config,dc=example".to_owned();
        cfg.ldap.bind_password = "initial-file-secret".to_owned();
        for (token, admin) in [("admin-token", true), ("regular-token", false)] {
            let account = ::entity::user::ActiveModel {
                username: Set(token.to_owned()),
                status: Set(::entity::user::STATUS_ENABLE),
                is_admin: Set(Some(admin)),
                tfa_enabled: Set(admin),
                ..Default::default()
            }
            .insert(&state.db)
            .await
            .unwrap();
            ::entity::user_token::ActiveModel {
                token: Set(token.to_owned()),
                user_id: Set(account.id),
                created_at: Set(now()),
                expired_at: Set(chrono::Utc::now().timestamp() + 60),
                auth_level: Set(if admin { AUTH_TOTP } else { 0 }),
                ..Default::default()
            }
            .insert(&state.db)
            .await
            .unwrap();
        }

        let app = router::build(state);
        let settings = "/api/admin/ldap/settings";
        let test = "/api/admin/ldap/test";
        for (method, path) in [("GET", settings), ("PUT", settings), ("POST", test)] {
            let (body, _) = request(&app, method, path, "regular-token", json!({})).await;
            assert_ne!(body["code"], 0);
        }
        let (body, cache) = request(&app, "GET", settings, "admin-token", json!(null)).await;
        assert_eq!(body["code"], 0);
        assert_eq!(cache, "no-store");
        assert_eq!(body["data"]["has_bind_password"], true);
        assert!(!body.to_string().contains("initial-file-secret"));

        let (body, cache) = request(
            &app,
            "PUT",
            settings,
            "admin-token",
            json!({
                "settings":{"user":{"scope":"one"}},
                "bind_password":"write-only-secret"
            }),
        )
        .await;
        assert_eq!(body["code"], 0, "{body}");
        assert_eq!(cache, "no-store");
        assert_eq!(body["data"]["settings"]["user"]["scope"], "one");
        assert!(!body.to_string().contains("write-only-secret"));

        let (body, _) = request(
            &app,
            "POST",
            test,
            "admin-token",
            json!({
                "settings":{"user":{"scope":"not-a-valid-scope"}},
                "bind_password":"rejected-secret"
            }),
        )
        .await;
        assert_eq!(body["code"], 400);
        assert_eq!(
            body["message"],
            "Проверьте настройки LDAP и область поиска."
        );
        assert!(!body.to_string().contains("rejected-secret"));
    }
}

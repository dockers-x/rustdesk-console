use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue},
    response::Response,
    Json,
};
use entity::user;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{middleware::BackendUser, response as resp, security_audit::AuditContext};
use crate::{
    config::Config, services::browser_trust, state::AppState, support::totp_vault::TotpVault,
};

const COOKIE: &str = "__Secure-rustdesk-console-trust";

pub(super) fn issue(
    config: &Config,
    account: &user::Model,
    id: Uuid,
    seconds: i64,
) -> anyhow::Result<HeaderValue> {
    let sealed = TotpVault::load(config, false)?
        .seal_bound(browser_trust::binding(account).as_bytes(), &id.to_string())?;
    Ok(HeaderValue::from_str(&format!(
        "{COOKIE}={sealed}; Max-Age={seconds}; Path=/api/admin/login; Secure; HttpOnly; SameSite=Strict"
    ))?)
}

pub(super) fn browser_id(
    headers: &HeaderMap,
    config: &Config,
    account: &user::Model,
) -> Option<String> {
    let mut values = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter_map(|(name, value)| (name == COOKIE).then_some(value));
    let value = values.next()?;
    if value.len() > 128 || values.next().is_some() {
        return None;
    }
    let vault = TotpVault::load(config, false).ok()?;
    let id = vault
        .open_bound(browser_trust::binding(account).as_bytes(), value)
        .ok()?;
    Some(Uuid::parse_str(&id).ok()?.to_string())
}

pub(super) fn browser_info(headers: &HeaderMap) -> (&'static str, &'static str) {
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let browser = [
        ("Edg", "Edge"),
        ("OPR/", "Opera"),
        ("Firefox/", "Firefox"),
        ("FxiOS/", "Firefox"),
        ("SamsungBrowser/", "Samsung Internet"),
        ("Chrome/", "Chrome"),
        ("CriOS/", "Chrome"),
        ("Safari/", "Safari"),
    ]
    .into_iter()
    .find_map(|(marker, label)| ua.contains(marker).then_some(label))
    .unwrap_or("");
    let platform = [
        ("iPhone", "iOS"),
        ("iPad", "iPadOS"),
        ("Android", "Android"),
        ("Windows", "Windows"),
        ("Macintosh", "macOS"),
        ("CrOS", "ChromeOS"),
        ("Linux", "Linux"),
    ]
    .into_iter()
    .find_map(|(marker, label)| ua.contains(marker).then_some(label))
    .unwrap_or("");
    (browser, platform)
}

#[derive(Serialize)]
struct BrowserView {
    id: String,
    browser: String,
    platform: String,
    created_at: i64,
    last_used_at: i64,
    expires_at: i64,
    current: bool,
}

pub(super) async fn list(
    State(state): State<AppState>,
    backend: BackendUser,
    headers: HeaderMap,
) -> Response {
    let current = browser_id(&headers, &state.config, &backend.user);
    match browser_trust::list(&state.db, &backend.user).await {
        Ok(rows) => resp::no_store(resp::success(
            rows.into_iter()
                .map(|row| BrowserView {
                    current: current.as_deref() == Some(row.id.as_str()),
                    id: row.id,
                    browser: row.browser,
                    platform: row.platform,
                    created_at: row.created_at,
                    last_used_at: row.last_used_at,
                    expires_at: row.expires_at,
                })
                .collect::<Vec<_>>(),
        )),
        Err(error) => {
            tracing::warn!(user_id = backend.user.id, %error, "Could not list trusted browsers");
            resp::fail(101, "Could not load trusted browsers")
        }
    }
}

#[derive(Deserialize)]
pub(super) struct RevokeOptions {
    #[serde(default)]
    end_sessions: bool,
}

pub(super) async fn revoke_one(
    State(state): State<AppState>,
    backend: BackendUser,
    audit: AuditContext,
    Path(id): Path<Uuid>,
    Json(options): Json<RevokeOptions>,
) -> Response {
    revoke(
        &state,
        backend.user.id,
        Some(&id.to_string()),
        options,
        &audit,
    )
    .await
}

pub(super) async fn revoke_all(
    State(state): State<AppState>,
    backend: BackendUser,
    audit: AuditContext,
    Json(options): Json<RevokeOptions>,
) -> Response {
    revoke(&state, backend.user.id, None, options, &audit).await
}

async fn revoke(
    state: &AppState,
    user_id: i32,
    id: Option<&str>,
    options: RevokeOptions,
    audit: &AuditContext,
) -> Response {
    audit.subject(user_id);
    audit.management(if id.is_some() {
        "login_security.revoke_browser"
    } else {
        "login_security.revoke_all_browsers"
    });
    match browser_trust::revoke(&state.db, user_id, id, options.end_sessions).await {
        Ok(()) => {
            audit.decision(
                "browser_trust",
                "accepted",
                if options.end_sessions {
                    "trust_and_sessions_revoked"
                } else {
                    "trust_revoked"
                },
            );
            resp::success(serde_json::Value::Null)
        }
        Err(error) => {
            tracing::warn!(user_id, %error, "Could not revoke trusted browsers");
            resp::fail(101, "Could not revoke browser trust")
        }
    }
}

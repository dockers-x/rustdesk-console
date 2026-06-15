//! Auth extractors + request helpers, mirroring `http/middleware/*.go`.
//!
//! - [`RustClientUser`]: `Authorization: Bearer <token>` (RustDesk PC client).
//! - [`BackendUser`]: `api-token` header (admin panel); auto-refreshes the token.
//! - [`AdminUser`]: [`BackendUser`] + `is_admin` check.
//! - [`ClientIp`] / [`AcceptLang`]: request metadata helpers.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::response::Response;
use async_trait::async_trait;

use entity::{user, user_token};

use crate::http::response;
use crate::state::AppState;

/// `Accept-Language` header value (empty string when absent).
pub struct AcceptLang(pub String);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AcceptLang {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let lang = parts
            .headers
            .get("Accept-Language")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        Ok(AcceptLang(lang))
    }
}

/// Best-effort client IP: `X-Forwarded-For` (first hop), `X-Real-IP`, else the
/// peer socket address.
pub struct ClientIp(pub String);

#[async_trait]
impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(extract_client_ip(parts)))
    }
}

pub fn extract_client_ip(parts: &Parts) -> String {
    if let Some(xff) = parts.headers.get("X-Forwarded-For").and_then(|v| v.to_str().ok()) {
        if let Some(first) = xff.split(',').next() {
            let ip = first.trim();
            if !ip.is_empty() {
                return ip.to_string();
            }
        }
    }
    if let Some(xrip) = parts.headers.get("X-Real-IP").and_then(|v| v.to_str().ok()) {
        if !xrip.is_empty() {
            return xrip.to_string();
        }
    }
    parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn header(parts: &Parts, name: &str) -> Option<String> {
    parts
        .headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// RustDesk PC client auth: `Authorization: Bearer <access_token>`.
pub struct RustClientUser {
    pub user: user::Model,
    pub token: String,
}

#[async_trait]
impl FromRequestParts<AppState> for RustClientUser {
    type Rejection = Response;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth = header(parts, "Authorization").ok_or_else(response::unauthorized)?;
        if auth.len() <= 7 {
            return Err(response::unauthorized());
        }
        let token = auth[7..].to_string();
        match crate::services::user::info_by_access_token(&state.db, &token).await {
            Ok(Some((user, _ut))) => Ok(RustClientUser { user, token }),
            _ => Err(response::unauthorized()),
        }
    }
}

/// Admin-panel auth: `api-token` header -> user_tokens lookup + token refresh.
pub struct BackendUser {
    pub user: user::Model,
    pub token: String,
    #[allow(dead_code)]
    pub user_token: user_token::Model,
}

#[async_trait]
impl FromRequestParts<AppState> for BackendUser {
    type Rejection = Response;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let lang = header(parts, "Accept-Language").unwrap_or_default();
        let token = header(parts, "api-token")
            .filter(|t| !t.is_empty())
            .ok_or_else(|| response::fail(403, state.tr(&lang, "NeedLogin")))?;
        let (user, ut) = match crate::services::user::info_by_access_token(&state.db, &token).await
        {
            Ok(Some(pair)) => pair,
            _ => return Err(response::fail(403, state.tr(&lang, "NeedLogin"))),
        };
        if !user.is_enabled() {
            return Err(response::unauthorized());
        }
        // auto-refresh token if close to expiry
        let _ = crate::services::user::auto_refresh_access_token(&state.db, &state.config, &ut).await;
        Ok(BackendUser {
            user,
            token,
            user_token: ut,
        })
    }
}

/// Admin-only: a [`BackendUser`] that is also an admin.
pub struct AdminUser {
    pub user: user::Model,
    pub token: String,
}

#[async_trait]
impl FromRequestParts<AppState> for AdminUser {
    type Rejection = Response;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let lang = header(parts, "Accept-Language").unwrap_or_default();
        let backend = BackendUser::from_request_parts(parts, state).await?;
        if !backend.user.is_admin() {
            return Err(response::fail(403, state.tr(&lang, "NoAccess")));
        }
        Ok(AdminUser {
            user: backend.user,
            token: backend.token,
        })
    }
}

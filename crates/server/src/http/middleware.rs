//! Auth extractors + request helpers, mirroring `http/middleware/*.go`.
//!
//! - [`RustClientUser`]: `Authorization: Bearer <token>` (RustDesk PC client).
//! - [`BackendUser`]: `api-token` header (admin panel); auto-refreshes the token.
//! - [`AdminUser`]: [`BackendUser`] + `is_admin` check.
//! - [`ClientIp`] / [`AcceptLang`]: request metadata helpers.

use std::net::{IpAddr, SocketAddr};

use async_trait::async_trait;
use axum::extract::{ConnectInfo, FromRequestParts, MatchedPath};
use axum::http::{request::Parts, Method};
use axum::response::Response;

use entity::{deployment_token, user, user_token};

use crate::http::response;
use crate::state::AppState;
use crate::services::console_roles::{self, ConsoleRole};

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
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(extract_client_ip(
            parts,
            &state.config.gin.trust_proxy,
        )))
    }
}

pub fn extract_client_ip(parts: &Parts, trusted_proxies: &str) -> String {
    let Some(peer) = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
    else {
        return "unknown".to_string();
    };
    if !trusted_proxy(peer, trusted_proxies) {
        return peer.to_string();
    }
    if let Some(value) = parts
        .headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        let chain = value
            .split(',')
            .map(|v| v.trim().parse::<IpAddr>())
            .collect::<Result<Vec<_>, _>>();
        let Ok(chain) = chain else {
            return peer.to_string();
        };
        for ip in chain.into_iter().rev() {
            if !trusted_proxy(ip, trusted_proxies) {
                return ip.to_string();
            }
        }
        return peer.to_string();
    }
    parts
        .headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<IpAddr>().ok())
        .unwrap_or(peer)
        .to_string()
}

fn trusted_proxy(ip: IpAddr, configured: &str) -> bool {
    configured.split(',').any(|entry| {
        let (network, prefix) = entry.trim().split_once('/').unwrap_or((entry.trim(), ""));
        let Ok(network) = network.parse::<IpAddr>() else {
            return false;
        };
        let (address, network, bits) = match (ip, network) {
            (IpAddr::V4(ip), IpAddr::V4(network)) => {
                (u32::from(ip) as u128, u32::from(network) as u128, 32)
            }
            (IpAddr::V6(ip), IpAddr::V6(network)) => (u128::from(ip), u128::from(network), 128),
            _ => return false,
        };
        let prefix = if prefix.is_empty() {
            bits
        } else {
            match prefix.parse::<u32>() {
                Ok(prefix) if prefix <= bits => prefix,
                _ => return false,
            }
        };
        prefix == 0 || address >> (bits - prefix) == network >> (bits - prefix)
    })
}

fn audit_principal(parts: &Parts, actor_id: i32, session_id: i32) {
    if let Some(audit) = parts
        .extensions
        .get::<super::security_audit::AuditContext>()
    {
        audit.principal(actor_id, session_id);
    }
}

fn header(parts: &Parts, name: &str) -> Option<String> {
    parts
        .headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// RustDesk deployment CLI auth: accepts a normal user token or a deployment
/// token created by the admin console.
pub struct DeploymentAuth {
    pub user: Option<user::Model>,
    pub token: String,
    pub deployment_token: Option<deployment_token::Model>,
}

impl DeploymentAuth {
    pub fn is_deployment_token(&self) -> bool {
        self.deployment_token.is_some()
    }

    pub fn deployment_token_id(&self) -> i32 {
        self.deployment_token.as_ref().map(|t| t.id).unwrap_or(0)
    }

    pub fn has_scope(&self, scope: &str) -> bool {
        match &self.deployment_token {
            Some(token) => crate::services::deployment::has_scope(token, scope),
            None => true,
        }
    }
}

#[async_trait]
impl FromRequestParts<AppState> for DeploymentAuth {
    type Rejection = Response;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth = header(parts, "Authorization").ok_or_else(response::unauthorized)?;
        let token = auth
            .strip_prefix("Bearer ")
            .filter(|v| !v.is_empty())
            .ok_or_else(response::unauthorized)?
            .to_string();
        if let Ok(Some((user, ut))) =
            crate::services::user::info_by_access_token(&state.db, &token).await
        {
            if user.must_change_password {
                let lang = header(parts, "Accept-Language").unwrap_or_default();
                return Err(response::error(state.tr(&lang, "PasswordChangeRequired")));
            }
            audit_principal(parts, user.id, ut.id);
            return Ok(DeploymentAuth {
                user: Some(user),
                token,
                deployment_token: None,
            });
        }
        match crate::services::deployment::verify_token(&state.db, &token).await {
            Ok(Some(deployment_token)) => {
                if let Some(audit) = parts
                    .extensions
                    .get::<super::security_audit::AuditContext>()
                {
                    audit.deployment_token(deployment_token.id);
                }
                Ok(DeploymentAuth {
                    user: None,
                    token,
                    deployment_token: Some(deployment_token),
                })
            }
            _ => Err(response::unauthorized()),
        }
    }
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
        let token = auth
            .strip_prefix("Bearer ")
            .filter(|v| !v.is_empty())
            .ok_or_else(response::unauthorized)?
            .to_string();
        match crate::services::user::info_by_access_token(&state.db, &token).await {
            Ok(Some((user, ut))) => {
                audit_principal(parts, user.id, ut.id);
                if user.must_change_password
                    && !matches!(
                        parts.uri.path(),
                        "/api/currentUser" | "/api/user/info" | "/api/logout"
                    )
                {
                    let lang = header(parts, "Accept-Language").unwrap_or_default();
                    return Err(response::error(state.tr(&lang, "PasswordChangeRequired")));
                }
                Ok(RustClientUser { user, token })
            }
            _ => Err(response::unauthorized()),
        }
    }
}

/// Address books are available to authenticated clients and the Console.
pub struct AddressBookUser {
    pub user: user::Model,
    pub console: bool,
}

#[async_trait]
impl FromRequestParts<AppState> for AddressBookUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if parts.headers.contains_key("api-token") && !parts.headers.contains_key("authorization") {
            let backend = BackendUser::from_request_parts(parts, state).await?;
            return Ok(Self { user: backend.user, console: true });
        }
        let native = RustClientUser::from_request_parts(parts, state).await?;
        Ok(Self { user: native.user, console: false })
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
            .ok_or_else(|| {
                response::fail(response::CODE_NEED_LOGIN, state.tr(&lang, "NeedLogin"))
            })?;
        use crate::services::user::{self, SessionLifetime, AUTH_CONSOLE_ADMIN, AUTH_ENROLLMENT, AUTH_TOTP};

        let need_login = || response::fail(response::CODE_NEED_LOGIN, state.tr(&lang, "NeedLogin"));
        let (user, ut) = user::session_by_token(&state.db, &token)
            .await
            .ok()
            .flatten()
            .ok_or_else(need_login)?;
        audit_principal(parts, user.id, ut.id);
        if user.must_change_password && !password_change_allowed_path(parts.uri.path()) {
            return Err(response::fail(
                112,
                state.tr(&lang, "PasswordChangeRequired"),
            ));
        }
        if !user.is_admin() {
            let role = console_roles::for_user(&state.db, user.id)
                .await
                .map_err(|_| response::fail(101, state.tr(&lang, "OperationFailed")))?;
            let path = parts
                .extensions
                .get::<MatchedPath>()
                .map_or(parts.uri.path(), MatchedPath::as_str);
            if !restricted_console_access(&role, &parts.method, path) {
                return Err(response::fail(
                    response::CODE_NO_ACCESS,
                    state.tr(&lang, "NoAccess"),
                ));
            }
            parts.extensions.insert(role);
        }
        if ut.auth_level == AUTH_ENROLLMENT {
            if !enrollment_allowed_path(parts.uri.path()) {
                return Err(response::fail(114, "TOTP enrollment required"));
            }
        } else {
            let permitted = match ut.auth_level {
                AUTH_TOTP => true,
                AUTH_CONSOLE_ADMIN => !user::console_requires_totp(&state.db, &user)
                    .await
                    .unwrap_or(true),
                0 => !user::requires_totp(&state.db, &user).await.unwrap_or(true),
                _ => false,
            };
            if !permitted {
                return Err(need_login());
            }
            let lifetime = if user.is_admin() {
                SessionLifetime::Admin
            } else {
                SessionLifetime::User
            };
            if user::auto_refresh_access_token(&state.db, &state.config, &ut, lifetime)
                .await
                .is_err()
            {
                tracing::warn!(session_id = ut.id, "session renewal failed");
            }
        }
        Ok(BackendUser {
            user,
            token,
            user_token: ut,
        })
    }
}

fn restricted_console_access(role: &ConsoleRole, method: &Method, path: &str) -> bool {
    if *role == ConsoleRole::Member {
        return true;
    }
    match (*method == Method::GET, path) {
        (true, "/api/admin/user/current" | "/api/admin/user/mySecurity") => return true,
        (
            false,
            "/api/admin/logout"
            | "/api/admin/user/changeCurPwd"
            | "/api/admin/user/myTfaSetup"
            | "/api/admin/user/myTfaEnable",
        ) => {
            return *method == Method::POST;
        }
        _ => {}
    }
    match role {
        ConsoleRole::AuditReader => {
            *method == Method::GET
                && matches!(
                    path,
                    "/api/admin/login_log/list"
                        | "/api/admin/audit_conn/list"
                        | "/api/admin/audit_file/list"
                        | "/api/admin/console-audit/list"
                )
        }
        ConsoleRole::DeviceGroupOperator { .. } => match (*method == Method::GET, path) {
            (true, "/api/admin/peer/list" | "/api/admin/peer/detail/:id") => true,
            (false, "/api/admin/peer/update") => *method == Method::POST,
            _ => false,
        },
        ConsoleRole::ScopedAdministrator { .. } => matches!(
            (method, path),
            (&Method::GET, "/api/admin/delegated/scopes"
                | "/api/admin/delegated/groups/:id/users"
                | "/api/admin/delegated/collections/:id/entries")
            | (&Method::PUT, "/api/admin/delegated/groups/:id/users/status"
                | "/api/admin/delegated/collections/:id"
                | "/api/admin/delegated/collections/:id/entries/:row_id")
        ),
        ConsoleRole::Member => true,
    }
}

/// Authorizes audit list operations for a superadmin or assigned audit reader.
pub(crate) enum AuditReader {
    Admin,
    Reader,
}

impl AuditReader {
    pub(crate) fn can_lookup_devices(&self) -> bool {
        matches!(self, Self::Admin)
    }
}

#[async_trait]
impl FromRequestParts<AppState> for AuditReader {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let backend = BackendUser::from_request_parts(parts, state).await?;
        match console_access(parts, &backend) {
            Some(ConsoleAccess::Admin) => Ok(Self::Admin),
            Some(ConsoleAccess::AuditReader) => Ok(Self::Reader),
            _ => Err(no_access(parts, state)),
        }
    }
}

/// Authorizes scoped peer operations, carrying the operator and assigned group IDs.
pub(crate) enum DeviceAccess {
    Admin,
    Operator { user_id: i32, group_ids: Vec<i32> },
}

#[async_trait]
impl FromRequestParts<AppState> for DeviceAccess {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let backend = BackendUser::from_request_parts(parts, state).await?;
        match console_access(parts, &backend) {
            Some(ConsoleAccess::Admin) => Ok(Self::Admin),
            Some(ConsoleAccess::DeviceGroupOperator(group_ids)) => Ok(Self::Operator {
                user_id: backend.user.id,
                group_ids,
            }),
            _ => Err(no_access(parts, state)),
        }
    }
}

#[async_trait]
impl FromRequestParts<AppState> for crate::services::delegated_admin::Actor {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let backend = BackendUser::from_request_parts(parts, state).await?;
        match parts.extensions.remove::<ConsoleRole>() {
            Some(ConsoleRole::ScopedAdministrator { .. }) if !backend.user.is_admin() => Ok(Self {
                user_id: backend.user.id,
                session_id: backend.user_token.id,
            }),
            _ => Err(no_access(parts, state)),
        }
    }
}

enum ConsoleAccess {
    Admin,
    AuditReader,
    DeviceGroupOperator(Vec<i32>),
}

fn no_access(parts: &Parts, state: &AppState) -> Response {
    let lang = header(parts, "Accept-Language").unwrap_or_default();
    response::fail(response::CODE_NO_ACCESS, state.tr(&lang, "NoAccess"))
}

fn console_access(parts: &mut Parts, backend: &BackendUser) -> Option<ConsoleAccess> {
    use crate::services::user::{AUTH_CONSOLE_ADMIN, AUTH_TOTP};
    if backend.user.is_admin() {
        if !matches!(
            backend.user_token.auth_level,
            AUTH_TOTP | AUTH_CONSOLE_ADMIN
        ) {
            return None;
        }
        if let Some(audit) = parts
            .extensions
            .get::<super::security_audit::AuditContext>()
        {
            audit.admin_access();
        }
        return Some(ConsoleAccess::Admin);
    }
    match parts.extensions.remove::<ConsoleRole>()? {
        ConsoleRole::AuditReader => Some(ConsoleAccess::AuditReader),
        ConsoleRole::DeviceGroupOperator { group_ids } => {
            Some(ConsoleAccess::DeviceGroupOperator(group_ids))
        }
        ConsoleRole::Member | ConsoleRole::ScopedAdministrator { .. } => None,
    }
}

fn password_change_allowed_path(path: &str) -> bool {
    matches!(
        path,
        "/api/admin/user/current" | "/api/admin/user/changeCurPwd" | "/api/admin/logout"
    )
}

fn enrollment_allowed_path(path: &str) -> bool {
    password_change_allowed_path(path)
        || matches!(
            path,
            "/api/admin/user/mySecurity"
                | "/api/admin/user/myTfaSetup"
                | "/api/admin/user/myTfaEnable"
        )
}

/// Admin-only: a [`BackendUser`] that is also an admin.
pub struct AdminUser {
    pub auth_level: i32,
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
        use crate::services::user::{AUTH_CONSOLE_ADMIN, AUTH_TOTP};
        if !backend.user.is_admin()
            || !matches!(backend.user_token.auth_level, AUTH_TOTP | AUTH_CONSOLE_ADMIN)
        {
            return Err(response::fail(
                response::CODE_NO_ACCESS,
                state.tr(&lang, "NoAccess"),
            ));
        }
        if let Some(audit) = parts
            .extensions
            .get::<super::security_audit::AuditContext>()
        {
            audit.admin_access();
        }
        Ok(AdminUser {
            auth_level: backend.user_token.auth_level,
            user: backend.user,
            token: backend.token,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use sea_orm::{ActiveModelTrait, ConnectionTrait, Database, DbBackend, Schema, Set};
    use serde_json::Value;
    use tower::ServiceExt;

    #[test]
    fn forwarded_addresses_require_trusted_peers_and_skip_trusted_hops() {
        let request = Request::builder()
            .header("x-forwarded-for", "198.51.100.9, 192.0.2.1")
            .body(Body::empty())
            .unwrap();
        let (mut parts, _) = request.into_parts();
        parts
            .extensions
            .insert(ConnectInfo("127.0.0.1:9000".parse::<SocketAddr>().unwrap()));
        assert_eq!(extract_client_ip(&parts, ""), "127.0.0.1");
        assert_eq!(extract_client_ip(&parts, "127.0.0.1"), "192.0.2.1");
        assert_eq!(
            extract_client_ip(&parts, "127.0.0.1,192.0.2.0/24"),
            "198.51.100.9"
        );
        assert_eq!(extract_client_ip(&parts, "192.0.2.0/24"), "127.0.0.1");
        parts
            .headers
            .insert("x-forwarded-for", "invalid".parse().unwrap());
        assert_eq!(extract_client_ip(&parts, "127.0.0.1"), "127.0.0.1");
        assert!(trusted_proxy("::1".parse().unwrap(), "::1/128"));
        assert!(!trusted_proxy("::2".parse().unwrap(), "::1/128"));
    }

    #[tokio::test]
    async fn review_bearer_extractors_enforce_required_password_changes() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for statement in [
            schema.create_table_from_entity(user::Entity),
            schema.create_table_from_entity(user_token::Entity),
            schema.create_table_from_entity(entity::system_setting::Entity),
        ] {
            db.execute(db.get_database_backend().build(&statement))
                .await
                .unwrap();
        }
        user::ActiveModel {
            id: Set(1),
            username: Set("temporary".into()),
            status: Set(user::STATUS_ENABLE),
            must_change_password: Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        user_token::ActiveModel {
            user_id: Set(1),
            token: Set("temporary-token".into()),
            expired_at: Set(chrono::Utc::now().timestamp() + 3600),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let state = crate::http::api::deployment_tests::test_state(db);
        let app = crate::http::router::build(state.clone());
        for path in ["/api/ab", "/api/devices/cli"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .header("authorization", "Bearer temporary-token")
                        .header("content-type", "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                    .unwrap();
            assert_eq!(
                body["error"],
                state.tr("", "PasswordChangeRequired"),
                "{path}"
            );
        }
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/user/info")
                    .header("authorization", "Bearer temporary-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        user::ActiveModel {
            id: Set(1),
            must_change_password: Set(false),
            ..Default::default()
        }
        .update(&state.db)
        .await
        .unwrap();
        let (mut parts, _) = Request::builder()
            .uri("/api/ab")
            .header("authorization", "Bearer temporary-token")
            .body(Body::empty())
            .unwrap()
            .into_parts();
        assert!(RustClientUser::from_request_parts(&mut parts, &state)
            .await
            .is_ok());
        assert!(DeploymentAuth::from_request_parts(&mut parts, &state)
            .await
            .is_ok());
    }
}

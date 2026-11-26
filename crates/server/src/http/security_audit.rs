//! Correlate HTTP requests with credential decisions without logging request data.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{ConnectInfo, MatchedPath, Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

use crate::state::AppState;

use super::middleware::extract_client_ip;

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Clone, Debug, Default)]
struct Decision {
    actor_id: Option<i32>,
    admin_access: bool,
    management_action: Option<&'static str>,
    session_id: Option<i32>,
    api_key_id: Option<i32>,
    deployment_token_id: Option<i32>,
    subject_id: Option<i32>,
    method: Option<&'static str>,
    result: Option<&'static str>,
    reason: Option<&'static str>,
}

#[derive(Debug, Clone, Default)]
pub struct AuditContext(Arc<Mutex<Decision>>);

#[async_trait::async_trait]
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for AuditContext {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(parts.extensions.get::<Self>().cloned().unwrap_or_default())
    }
}

impl AuditContext {
    pub fn principal(&self, actor_id: i32, session_id: i32) {
        let mut decision = self.0.lock().expect("security audit context poisoned");
        decision.actor_id = Some(actor_id);
        decision.session_id = Some(session_id);
    }

    pub fn admin_access(&self) {
        self.0
            .lock()
            .expect("security audit context poisoned")
            .admin_access = true;
    }

    pub fn api_key(&self, actor_id: i32, key_id: i32) {
        let mut decision = self.0.lock().expect("security audit context poisoned");
        decision.actor_id = Some(actor_id);
        decision.api_key_id = Some(key_id);
        decision.method = Some("api_key");
    }

    pub fn deployment_token(&self, token_id: i32) {
        let mut decision = self.0.lock().expect("security audit context poisoned");
        decision.deployment_token_id = Some(token_id);
    }

    /// Set a code-defined management action for this request.
    pub(crate) fn management(&self, action: &'static str) {
        self.0
            .lock()
            .expect("security audit context poisoned")
            .management_action = Some(action);
    }

    pub fn subject(&self, subject_id: i32) {
        self.0
            .lock()
            .expect("security audit context poisoned")
            .subject_id = Some(subject_id);
    }

    pub fn decision(&self, method: &'static str, result: &'static str, reason: &'static str) {
        let mut decision = self.0.lock().expect("security audit context poisoned");
        decision.method = Some(method);
        decision.result = Some(result);
        decision.reason = Some(reason);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BusinessCode(pub i32);

pub async fn trace(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let (mut parts, body) = request.into_parts();
    let context = AuditContext::default();
    parts.extensions.insert(context.clone());
    let method = parts.method.clone();
    let route = parts
        .extensions
        .get::<MatchedPath>()
        .map_or("<unmatched>", MatchedPath::as_str)
        .to_owned();
    let peer_socket = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0);
    let peer_ip = peer_socket.map(|peer| peer.ip());
    let client_ip = extract_client_ip(&parts, &state.config.gin.trust_proxy);
    let started = Instant::now();
    let mut response = next.run(Request::from_parts(parts, body)).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert(REQUEST_ID, value);
    }
    let status = response.status().as_u16();
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let code = response
        .extensions()
        .get::<BusinessCode>()
        .map(|code| code.0);
    let decision = context
        .0
        .lock()
        .expect("security audit context poisoned")
        .clone();
    let failed = status >= StatusCode::BAD_REQUEST.as_u16() || code.is_some_and(|n| n != 0);
    let denied = matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) || matches!(code, Some(401 | 403));
    if let (Some(actor_id), Some(action)) = (
        decision.actor_id,
        decision
            .management_action
            .or_else(|| management_action(&route, &method)),
    ) {
        let outcome = if denied {
            "denied"
        } else if failed {
            "failed"
        } else {
            "success"
        };
        if let Err(error) = crate::services::console_audit::record(
            &state.db,
            action,
            outcome,
            actor_id,
            decision.subject_id,
        )
        .await
        {
            tracing::error!(%request_id, action, %error, "Failed to persist Console management audit");
        }
    }
    if decision.admin_access
        || decision.result.is_some()
        || audited_route(&route, &method)
        || denied
    {
        tracing::info!(
            target: "security_audit",
            event = "security.request",
            %request_id,
            %route,
            %method,
            peer_socket = peer_socket.map(tracing::field::display),
            peer_ip = peer_ip.map(tracing::field::display),
            %client_ip,
            status,
            elapsed_ms,
            business_code = code,
            actor_id = decision.actor_id,
            session_id = decision.session_id,
            api_key_id = decision.api_key_id,
            deployment_token_id = decision.deployment_token_id,
            subject_id = decision.subject_id,
            auth_method = decision.method,
            outcome = decision.result.unwrap_or(if failed { "rejected" } else { "completed" }),
            reason = decision.reason.unwrap_or(if denied { "authorization_denied" } else if failed { "request_rejected" } else { "handler_completed" }),
        );
    } else {
        tracing::debug!(%request_id, %route, %method, status, elapsed_ms, "HTTP request");
    }
    response
}

fn management_action(route: &str, method: &Method) -> Option<&'static str> {
    match (method.clone(), route) {
        (Method::PUT, "/api/admin/console-role/:id") => Some("roles.assign"),
        (Method::PUT, "/api/admin/delegated/groups/:id/users/status") => Some("delegation.user_status"),
        (Method::PUT, "/api/admin/delegated/collections/:id") => Some("delegation.collection_update"),
        (Method::PUT, "/api/admin/delegated/collections/:id/entries/:row_id") => Some("delegation.entry_update"),
        (Method::PUT, "/api/admin/user/control-role/:id") => Some("roles.control_assign"),
        (Method::PUT, "/api/admin/device-admission/settings") => Some("admission.settings"),
        (Method::POST, "/api/admin/device-admission/review") => Some("admission.review"),
        (Method::POST, "/api/admin/enrollment-invitations") => Some("admission.invite"),
        (Method::DELETE, "/api/admin/enrollment-invitations/:id") => Some("admission.revoke_invitation"),
        (Method::POST, "/api/admin/storage/profiles") => Some("storage.profile_create"),
        (Method::PUT, "/api/admin/storage/profiles/:id") => Some("storage.profile_update"),
        (Method::DELETE, "/api/admin/storage/profiles/:id") => Some("storage.profile_archive"),
        (Method::PUT, "/api/admin/storage/recording") => Some("storage.recording_select"),
        (Method::PUT, "/api/admin/storage/backup") => Some("storage.backup_select"),
        (Method::POST, "/api/admin/backups/run") => Some("backup.run"),
        (Method::PUT, "/api/admin/connection-grants/:group/:device_group") => Some("connections.grant"),
        (Method::DELETE, "/api/admin/connection-grants/:group/:device_group") => Some("connections.revoke"),
        (Method::POST, "/api/ab/rule") => Some("address_book.grant_create"),
        (Method::PATCH, "/api/ab/rule") => Some("address_book.grant_update"),
        (Method::DELETE, "/api/ab/rules") => Some("address_book.grant_delete"),
        (Method::PUT, "/api/admin/console-audit/retention") => Some("audit.retention"),
        (Method::POST, "/api/admin/console-audit/cleanup") => Some("audit.cleanup"),
        (Method::PUT, "/api/admin/ldap/settings") => Some("login_security.ldap"),
        (Method::PATCH, "/api/admin/config/login-security") => Some("login_security.update"),
        (Method::POST, "/api/admin/config/login-security/test-email") => {
            Some("login_security.test_email")
        }
        (Method::POST, "/api/admin/config/smtp") => Some("login_security.smtp_save"),
        (Method::POST, "/api/admin/user/create") => Some("users.create"),
        (Method::POST, "/api/admin/user/update") => Some("users.update"),
        (Method::POST, "/api/admin/user/delete") => Some("users.delete"),
        (Method::POST, "/api/admin/user/changePwd") => Some("login_security.reset_password"),
        (Method::POST, "/api/admin/user/password-reset/email") => {
            Some("login_security.email_password_reset")
        }
        (Method::POST, "/api/admin/user/tfa/reset") => Some("login_security.reset_totp"),
        (Method::POST, "/api/admin/user/security") => Some("login_security.account"),
        (Method::POST, "/api/admin/peer/update") => Some("devices.update"),
        (Method::POST, "/api/admin/strategy/create") => Some("strategies.create"),
        (Method::POST, "/api/admin/strategy/update") => Some("strategies.update"),
        (Method::POST, "/api/admin/strategy/delete") => Some("strategies.delete"),
        (Method::POST, "/api/admin/strategy/assign") => Some("strategies.assign"),
        (Method::POST, "/api/admin/strategy_assignment/create") => {
            Some("strategy_assignments.create")
        }
        (Method::POST, "/api/admin/strategy_assignment/update") => {
            Some("strategy_assignments.update")
        }
        (Method::POST, "/api/admin/strategy_assignment/delete") => {
            Some("strategy_assignments.delete")
        }
        (Method::POST, "/api/strategies/assign") => Some("strategies.assign"),
        (Method::PUT, "/api/strategies/:guid/status") => Some("strategies.update"),
        (Method::POST, "/api/users" | "/api/users/invite") => Some("users.create"),
        (Method::POST, "/api/users/:guid/enable" | "/api/users/:guid/disable") => Some("users.update"),
        (Method::DELETE, "/api/users/:guid") => Some("users.delete"),
        (Method::POST, "/api/users/force-logout") => Some("login_security.account"),
        (Method::PUT, "/api/users/tfa/totp/enforce" | "/api/users/disable_login_verification") => Some("login_security.account"),
        (Method::POST, "/api/devices/:guid/assign" | "/api/devices/:guid/enable" | "/api/devices/:guid/disable") => Some("devices.update"),
        (Method::DELETE, "/api/devices/:guid") => Some("devices.delete"),
        (Method::POST, "/api/device-groups") | (Method::POST | Method::PATCH | Method::DELETE, "/api/device-groups/:guid")
        | (Method::DELETE, "/api/device-groups/:guid/devices") => Some("roles.device_group"),
        (Method::POST, "/api/user-groups") | (Method::POST | Method::PATCH | Method::DELETE, "/api/user-groups/:guid") => Some("roles.user_group"),
        (Method::POST, _) if route.starts_with("/api/admin/user/trusted-login-device/") => {
            Some("login_security.account")
        }
        (Method::POST, _) if route.starts_with("/api/admin/config/smtp/") => {
            Some("login_security.smtp")
        }
        (Method::POST, _) if route.starts_with("/api/admin/device_group/") => {
            Some("roles.device_group")
        }
        (Method::POST, _) if route.starts_with("/api/admin/group/") => Some("roles.user_group"),
        _ => None,
    }
}

fn audited_route(route: &str, method: &axum::http::Method) -> bool {
    route == "/api/login"
        || route == "/api/logout"
        || route.starts_with("/api/oidc/")
        || route.starts_with("/api/oauth/")
        || route.starts_with("/api/admin/oidc/")
        || route.starts_with("/api/admin/oauth/")
        || route.starts_with("/api/admin/login")
        || route.starts_with("/api/admin/password-reset/")
        || route.starts_with("/api/admin/user_token/")
        || (route.starts_with("/api/admin/") && method != axum::http::Method::GET)
        || (method != axum::http::Method::GET
            && [
                "/api/users",
                "/api/devices",
                "/api/device-groups",
                "/api/user-groups",
                "/api/strategies",
                "/api/v1",
            ]
            .iter()
            .any(|prefix| {
                route == *prefix
                    || route
                        .strip_prefix(prefix)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use axum::body::Body;
    use sea_orm::Database;
    use tower::ServiceExt;

    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn audit_records_business_rejection_and_trusted_socket_without_credentials() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let state = crate::http::api::deployment_tests::test_state(db);
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::clone(&bytes);
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(move || Capture(Arc::clone(&writer)))
            .finish();
        tracing::subscriber::set_global_default(subscriber).unwrap();
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/admin/login?code=query-secret-do-not-log")
            .header("content-type", "application/json")
            .header("x-request-id", "untrusted-request-id")
            .header("x-forwarded-for", "192.0.2.99")
            .header("api-token", "header-secret-do-not-log")
            .body(Body::from(
                r#"{"username":"x","password":"body-secret-do-not-log"}"#,
            ))
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
        let response = crate::http::router::build(state)
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let id = response.headers()[REQUEST_ID].to_str().unwrap();
        assert_ne!(id, "untrusted-request-id");
        let bytes = bytes.lock().unwrap();
        let output = std::str::from_utf8(&bytes).unwrap();
        let (line, event) = output
            .lines()
            .filter_map(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .map(|event| (line, event))
            })
            .find(|(_, event)| event["fields"]["request_id"] == id)
            .unwrap();
        assert!(!line.contains("secret-do-not-log"), "{line}");
        assert!(!line.contains("192.0.2.99"), "{line}");
        let fields = &event["fields"];
        assert_eq!(fields["event"], "security.request");
        assert_eq!(fields["request_id"], id);
        assert_eq!(fields["client_ip"], "127.0.0.1");
        assert_eq!(fields["peer_ip"], "127.0.0.1");
        assert_eq!(fields["peer_socket"], "127.0.0.1:12345");
        assert_eq!(fields["route"], "/api/admin/login");
        assert_eq!(fields["business_code"], 101);
        assert_eq!(fields["outcome"], "rejected");
        assert_eq!(fields["reason"], "invalid_input");
    }
}

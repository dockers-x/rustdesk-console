//! OAuth/OIDC login endpoints, ports of `http/controller/api/ouath.go` and the
//! admin `Login` OAuth methods + `admin/oauth.go` bind/confirm/info. Drives the
//! authorize → callback → poll(auth-query) handshake via the in-memory cache.

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::http::api::request_public_base;
use crate::http::middleware::{AcceptLang, BackendUser, ClientIp};
use crate::http::response as resp;
use crate::http::security_audit::AuditContext;
use crate::services;
use crate::services::login_security::{DeviceLoginInfo, LoginChallenge};
use crate::services::user::LoginProof;
use crate::state::AppState;
use crate::support::oauth_cache::{OauthCacheItem, ACTION_BIND, ACTION_LOGIN};

#[derive(Deserialize, Default)]
pub struct DeviceInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub os: String,
    #[serde(default, rename = "type")]
    pub r#type: String,
}

#[derive(Deserialize, Default)]
pub struct OidcAuthRequest {
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default, rename = "deviceInfo")]
    pub device_info: DeviceInfo,
}

async fn begin(state: &AppState, f: &OidcAuthRequest, device_type: &str) -> Response {
    match begin_value(state, f, device_type).await {
        Ok(v) => resp::no_store(Json(v).into_response()),
        Err(message) => resp::error(message),
    }
}

async fn begin_value(
    state: &AppState,
    f: &OidcAuthRequest,
    device_type: &str,
) -> Result<serde_json::Value, String> {
    let auth = services::oauth::begin_auth(&state.config, &state.db, &f.op).await?;
    let poll_secret = crate::support::random::random_string(40);
    let code = format!("{}.{poll_secret}", auth.state);
    state.oauth_cache.set(
        &auth.state,
        OauthCacheItem {
            action: ACTION_LOGIN.into(),
            poll_secret,
            id: f.id.clone(),
            op: f.op.clone(),
            uuid: f.uuid.clone(),
            device_name: f.device_info.name.clone(),
            device_os: f.device_info.os.clone(),
            device_type: device_type.to_string(),
            verifier: auth.verifier,
            nonce: auth.nonce,
            provider_identity: auth.provider_identity,
            ..Default::default()
        },
        5 * 60,
    );
    Ok(json!({ "code": code, "url": auth.url }))
}

// ---------- client ----------

pub async fn oidc_auth(State(state): State<AppState>, Json(f): Json<OidcAuthRequest>) -> Response {
    begin(&state, &f, "").await
}

#[derive(Deserialize, Default)]
pub struct OidcAuthQuery {
    #[serde(default)]
    pub code: String,
}

#[derive(Clone, Copy, Debug)]
enum OidcClient {
    Console,
    Native,
}

enum OidcLogin {
    Session(entity::user::Model, entity::user_token::Model, &'static str),
    Challenge(entity::user::Model, LoginChallenge),
}

#[derive(Clone, Copy, Debug)]
enum PollError {
    Expired,
    Invalid,
    Pending,
    Failed,
    NativeMfa,
    Provider(&'static str),
    ConsolePolicy(&'static str),
}

impl PollError {
    fn response(self, state: &AppState, lang: &str) -> Response {
        match self {
            Self::Expired => resp::error(state.tr(lang, "OauthExpired")),
            Self::Invalid => resp::error(state.tr(lang, "ParamsError")),
            Self::Pending => Json(json!({
                "message": "Authorization in progress, please login and bind",
                "error": "No authed oidc is found"
            }))
            .into_response(),
            Self::Failed => resp::error(state.tr(lang, "LoginFailed")),
            Self::Provider(message) => resp::fail(101, state.tr(lang, message)),
            Self::NativeMfa => {
                resp::error("Use password sign-in or approve sign-in through the console")
            }
            Self::ConsolePolicy(message) => resp::fail(101, message),
        }
    }
}

/// Redeem an approved attempt once, then satisfy local authentication policy.
async fn auth_query_pre(
    state: &AppState,
    ip: &str,
    code: &str,
    client: OidcClient,
    audit: &AuditContext,
) -> Result<OidcLogin, PollError> {
    let Some(pending) = state.oauth_cache.get_poll(code) else {
        audit.decision("federated", "rejected", "attempt_expired");
        return Err(PollError::Expired);
    };
    let console = matches!(client, OidcClient::Console);
    if pending.action != ACTION_LOGIN
        || (pending.device_type == entity::login_log::CLIENT_WEB_ADMIN) != console
    {
        audit.decision("federated", "rejected", "invalid_attempt");
        return Err(PollError::Invalid);
    }
    if pending.user_id == 0 && pending.failure.is_none() {
        audit.decision("federated", "pending", "awaiting_provider");
        return Err(PollError::Pending);
    }
    let Some(v) = state.oauth_cache.take_ready_login(code, console) else {
        audit.decision("federated", "rejected", "attempt_expired");
        return Err(PollError::Expired);
    };
    if let Some(message) = v.failure {
        return Err(PollError::Provider(message));
    }
    let Some(account) = services::user::info_by_id(&state.db, v.user_id)
        .await
        .map_err(|_| PollError::Failed)?
        .filter(entity::user::Model::is_enabled)
    else {
        audit.decision("federated", "rejected", "account_unavailable");
        return Err(PollError::Failed);
    };
    audit.subject(account.id);
    let device = DeviceLoginInfo {
        id: v.id,
        uuid: v.uuid,
        name: v.device_name,
        os: v.device_os,
        client_type: v.device_type,
        ip: ip.to_owned(),
        auto_login: false,
    };
    let webauth = v.op == entity::oauth::TYPE_WEBAUTH;
    let proof = if webauth {
        let Some(session_id) = v.source_session_id.filter(|_| state.config.app.web_sso) else {
            audit.decision("webauth", "rejected", "approval_unavailable");
            return Err(PollError::Failed);
        };
        LoginProof::WebSso { session_id }
    } else {
        let binding_id = v.binding_id.ok_or(PollError::Failed)?;
        if !services::oauth::binding_active(&state.db, &state.config, account.id, binding_id)
            .await
            .map_err(|_| PollError::Failed)?
        {
            return Err(PollError::Provider("OauthBindingRequired"));
        }
        let kind = services::login_security::required_verification(&state.db, &account, &device)
            .await
            .map_err(|_| PollError::Failed)?;
        if let Some(kind) = kind {
            if !console {
                audit.decision("federated", "rejected", "native_challenge_unsupported");
                return Err(PollError::NativeMfa);
            }
            let challenge = services::login_security::create_federated_challenge(
                &state.db,
                &account,
                &device,
                kind,
                &state.config.lang,
                binding_id,
            )
            .await
            .map_err(|_| PollError::Failed)?;
            audit.decision("federated", "pending", "challenge_issued");
            return Ok(OidcLogin::Challenge(account, challenge));
        }
        if console {
            super::admin::console_login_proof(state, &account)
                .await
                .map_err(PollError::ConsolePolicy)?
        } else {
            LoginProof::SingleFactor
        }
    };
    let method = if webauth { "webauth" } else { "federated" };
    let session = services::user::login_with_binding(
        &state.db,
        &state.jwt,
        &state.config,
        &account,
        services::user::LoginEvent {
            proof,
            client: device.client_type,
            device_id: device.id,
            uuid: device.uuid,
            ip: device.ip,
            login_type: entity::login_log::TYPE_OAUTH.to_owned(),
            platform: device.os,
        },
        v.binding_id,
    )
    .await
    .map_err(|_| {
        audit.decision(method, "rejected", "session_issue_denied");
        PollError::Failed
    })?;
    Ok(OidcLogin::Session(account, session, method))
}

pub async fn oidc_auth_query(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    audit: AuditContext,
    Query(q): Query<OidcAuthQuery>,
) -> Response {
    let avatar_base_url = request_public_base(&headers, &state);
    match auth_query_pre(&state, &ip, &q.code, OidcClient::Native, &audit).await {
        Ok(OidcLogin::Session(u, ut, method)) => {
            super::api::client_session_response(&audit, method, &u, ut, &avatar_base_url)
        }
        Ok(OidcLogin::Challenge(_, _)) => resp::error(state.tr(&lang, "LoginFailed")),
        Err(PollError::Provider(message)) => resp::error(state.tr(&lang, message)),
        Err(error) => error.response(&state, &lang),
    }
}

#[derive(Deserialize, Default)]
pub struct CallbackQuery {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub code: String,
}

fn render_template(state: &AppState, name: &str, message: &str, sub: &str) -> Response {
    let tpl = crate::assets::Resources::read_string(&format!("templates/{name}"))
        .unwrap_or_else(|| "<html><body>{{.message}}</body></html>".to_string());
    let message = json!(message).to_string().replace('<', "\\u003c");
    let html = tpl
        .replace("{{.message}}", &message)
        .replace("{{.sub_message}}", sub);
    let _ = state;
    Html(html).into_response()
}

pub async fn oauth_callback(
    State(state): State<AppState>,
    audit: AuditContext,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let Some(mut attempt) = state.oauth_cache.claim_callback(&q.state) else {
        return resp::no_store(render_template(
            &state,
            "oauth_fail.html",
            "OauthExpired",
            "",
        ));
    };
    let outcome = complete_provider_callback(&state, &q, &mut attempt).await;
    let return_to_console = attempt.device_type == entity::login_log::CLIENT_WEB_ADMIN;
    let binding = attempt.action == ACTION_BIND;
    let user_id = attempt.user_id;
    if let Err(message) = outcome {
        attempt.failure = Some(message);
        audit.decision("federated", "rejected", message);
    } else {
        if user_id != 0 {
            audit.subject(user_id);
        }
        audit.decision("federated", "pending", "provider_identity_verified");
    }
    if !state.oauth_cache.complete_callback(&q.state, attempt) {
        return resp::no_store(render_template(
            &state,
            "oauth_fail.html",
            "OauthExpired",
            "",
        ));
    }

    let response = if binding {
        Redirect::to("/_admin/#/my?oidc=bind").into_response()
    } else if return_to_console {
        Redirect::to("/_admin/#/login?oidc=return").into_response()
    } else if let Err(message) = outcome {
        render_template(&state, "oauth_fail.html", message, "")
    } else {
        render_template(&state, "oauth_success.html", "OauthSuccess", "")
    };
    resp::no_store(response)
}

async fn complete_provider_callback(
    state: &AppState,
    q: &CallbackQuery,
    attempt: &mut OauthCacheItem,
) -> Result<(), &'static str> {
    if q.code.is_empty() {
        return Err("OauthAuthorizationCancelled");
    }
    let provider = services::oauth::login_provider(&state.db, &state.config, &attempt.op)
        .await
        .map_err(|_| "OperationFailed")?
        .filter(|provider| {
            services::oauth::provider_identity(provider) == attempt.provider_identity
        })
        .ok_or("ConfigNotFound")?;
    let identity = services::oauth::callback(
        &state.config, &provider, &q.code, &attempt.verifier, &attempt.nonce,
    ).await.map_err(|error| {
        tracing::warn!(provider = %provider.op, %error, "OIDC callback could not verify the identity");
        "OauthFailed"
    })?;

    if attempt.action == ACTION_BIND {
        attempt.identity = Some(identity);
        return Ok(());
    }
    let allow_registration = provider.auto_register == Some(true)
        && services::login_security::registration_enabled(&state.db, state.config.app.register)
            .await
            .map_err(|_| "OperationFailed")?;
    let binding = services::oauth::resolve_identity(
        &state.db,
        &provider,
        &identity,
        allow_registration,
    )
    .await
    .map_err(|error| {
        tracing::warn!(provider = %provider.op, %error, "Could not resolve the OAuth binding");
        "OperationFailed"
    })?
    .ok_or("OauthBindingRequired")?;
    attempt.user_id = binding.user_id;
    attempt.binding_id = Some(binding.id);
    Ok(())
}

#[derive(Deserialize, Default)]
pub struct MessageParams {
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub msg: String,
}

pub async fn message(State(state): State<AppState>, Query(mp): Query<MessageParams>) -> Response {
    let mut res = String::new();
    if !mp.title.is_empty() {
        let t = state.i18n.translate(&mp.lang, &mp.title);
        res.push_str(&format!(";title={};", json!(t)));
    }
    if !mp.msg.is_empty() {
        let m = state.i18n.translate(&mp.lang, &mp.msg);
        res.push_str(&format!("msg = {};", json!(m)));
    }
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        res,
    )
        .into_response()
}

// ---------- admin ----------

pub async fn admin_oidc_auth(
    State(state): State<AppState>,
    Json(f): Json<OidcAuthRequest>,
) -> Response {
    match begin_value(&state, &f, entity::login_log::CLIENT_WEB_ADMIN).await {
        Ok(v) => resp::no_store(resp::success(v)),
        Err(message) => resp::error(message),
    }
}

pub async fn admin_oidc_auth_query(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    ClientIp(ip): ClientIp,
    audit: AuditContext,
    Query(q): Query<OidcAuthQuery>,
) -> Response {
    match auth_query_pre(&state, &ip, &q.code, OidcClient::Console, &audit).await {
        Ok(OidcLogin::Session(u, ut, method)) => {
            super::admin::console_session_response(&audit, method, &u, ut)
        }
        Ok(OidcLogin::Challenge(u, challenge)) => {
            super::admin::console_challenge_response(&u, challenge, None)
        }
        Err(error) => error.response(&state, &lang),
    }
}

#[derive(Deserialize, Default)]
pub struct CodeForm {
    #[serde(default)]
    pub code: String,
}

/// Attach the current admin user to a webauth login request (≈ `Confirm`).
pub async fn admin_confirm(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<CodeForm>,
) -> Response {
    if !state.config.app.web_sso
        || !state
            .oauth_cache
            .confirm_webauth(&f.code, user.user.id, user.user_token.id)
    {
        return resp::fail(101, state.tr(&lang, "OauthExpired"));
    }
    resp::success(serde_json::Value::Null)
}

#[derive(Deserialize, Default)]
pub struct BindForm {
    #[serde(default)]
    pub op: String,
}

pub async fn admin_to_bind(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<BindForm>,
) -> Response {
    if let Err(error) =
        services::oauth::validate_binding_session(&state.db, user.user.id, user.user_token.id).await
    {
        tracing::debug!(%error, user_id = user.user.id, "OIDC binding requires a fully verified session");
        return resp::fail(101, state.tr(&lang, "OauthVerifySession"));
    }
    let provider = match services::oauth::login_provider(&state.db, &state.config, &f.op).await {
        Ok(Some(provider)) => provider,
        _ => return resp::fail(101, state.tr(&lang, "ConfigNotFound")),
    };
    match services::oauth::user_thirds_by_user_id(&state.db, user.user.id).await {
        Ok(bindings)
            if bindings
                .iter()
                .any(|binding| services::oauth::binding_matches(binding, &provider)) =>
        {
            return resp::fail(101, state.tr(&lang, "OauthHasBindOtherUser"));
        }
        Err(_) => return resp::fail(101, state.tr(&lang, "OperationFailed")),
        _ => {}
    }
    let auth = match services::oauth::begin_auth(&state.config, &state.db, &f.op).await {
        Ok(auth) => auth,
        Err(error) => return resp::fail(101, state.tr(&lang, &error)),
    };
    let poll_secret = crate::support::random::random_string(40);
    let code = format!("{}.{poll_secret}", auth.state);
    state.oauth_cache.set(
        &auth.state,
        OauthCacheItem {
            action: ACTION_BIND.into(),
            source_session_id: Some(user.user_token.id),
            op: f.op,
            user_id: user.user.id,
            poll_secret,
            verifier: auth.verifier,
            nonce: auth.nonce,
            provider_identity: auth.provider_identity,
            ..Default::default()
        },
        5 * 60,
    );
    resp::no_store(resp::success(json!({ "code": code, "url": auth.url })))
}

pub async fn admin_bind_confirm(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    audit: AuditContext,
    Json(f): Json<CodeForm>,
) -> Response {
    let Some(attempt) =
        state
            .oauth_cache
            .take_ready_binding(&f.code, user.user.id, user.user_token.id)
    else {
        return resp::fail(101, state.tr(&lang, "OauthExpired"));
    };
    let Some(identity) = attempt.identity else {
        return resp::fail(101, state.tr(&lang, attempt.failure.unwrap_or("BindFail")));
    };
    let result = services::oauth::bind_user_with_session(
        &state.db,
        &state.config,
        user.user.id,
        user.user_token.id,
        &attempt.provider_identity,
        &attempt.op,
        &identity,
    )
    .await;
    match result {
        Ok(()) => {
            audit.subject(user.user.id);
            audit.management("oauth.bind");
            resp::no_store(resp::success(serde_json::Value::Null))
        }
        Err(sea_orm::DbErr::Custom(message)) if message == "OauthHasBindOtherUser" => {
            resp::fail(101, state.tr(&lang, "OauthHasBindOtherUser"))
        }
        Err(error)
            if matches!(
                error.sql_err(),
                Some(sea_orm::SqlErr::UniqueConstraintViolation(_))
            ) =>
        {
            resp::fail(101, state.tr(&lang, "OauthHasBindOtherUser"))
        }
        Err(error) => {
            tracing::warn!(%error, user_id = user.user.id, "Could not bind OAuth identity");
            resp::fail(101, state.tr(&lang, "BindFail"))
        }
    }
}

pub async fn admin_info(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: BackendUser,
    Query(q): Query<OidcAuthQuery>,
) -> Response {
    if q.code.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match state.oauth_cache.get(&q.code).filter(|attempt| {
        attempt.action == ACTION_LOGIN && attempt.op == entity::oauth::TYPE_WEBAUTH
    }) {
        Some(v) => resp::success(v),
        None => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use sea_orm::DatabaseConnection;

    #[tokio::test]
    async fn review_oauth_message_quotes_untrusted_text_as_one_js_string() {
        let state =
            crate::http::api::deployment_tests::test_state(DatabaseConnection::Disconnected);
        let text = "';alert(1);//\ntranslation";
        let response = message(
            State(state),
            Query(MessageParams {
                title: text.into(),
                msg: text.into(),
                ..Default::default()
            }),
        )
        .await;
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        let quoted = serde_json::to_string(text).unwrap();
        assert_eq!(body, format!(";title={quoted};msg = {quoted};"));
    }

    #[tokio::test]
    async fn oauth_templates_preserve_error_text_inside_the_inline_script() {
        let state =
            crate::http::api::deployment_tests::test_state(DatabaseConnection::Disconnected);
        let message = "Database error for 'name'\n</script><script>alert(1)</script>";
        for name in ["oauth_fail.html", "oauth_success.html"] {
            let response = render_template(&state, name, message, "");
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let html = String::from_utf8(body.to_vec()).unwrap();
            let literal = html
                .lines()
                .find_map(|line| line.trim().strip_prefix("var msg = "))
                .unwrap();
            assert!(!literal.contains('<'), "{name} must not close the script");
            assert_eq!(serde_json::from_str::<String>(literal).unwrap(), message);
        }
    }

    #[tokio::test]
    async fn oauth_binding_is_only_redeemed_by_the_originating_console_session() {
        let state =
            crate::http::api::deployment_tests::test_state(DatabaseConnection::Disconnected);
        state.oauth_cache.set(
            "state",
            OauthCacheItem {
                action: ACTION_BIND.into(),
                user_id: 1,
                source_session_id: Some(2),
                poll_secret: "secret".into(),
                ..Default::default()
            },
            300,
        );
        assert!(state
            .oauth_cache
            .take_ready_binding("state.secret", 1, 2)
            .is_none());
        let mut attempt = state.oauth_cache.claim_callback("state").unwrap();
        assert!(state.oauth_cache.claim_callback("state").is_none());
        attempt.identity = Some(services::oauth::OauthUser {
            open_id: "subject".into(),
            ..Default::default()
        });
        assert!(state.oauth_cache.complete_callback("state", attempt));
        assert!(state
            .oauth_cache
            .take_ready_binding("state.secret", 9, 2)
            .is_none());
        assert!(state
            .oauth_cache
            .take_ready_binding("state.secret", 1, 9)
            .is_none());
        assert!(state
            .oauth_cache
            .take_ready_binding("state", 1, 2)
            .is_none());
        assert!(state
            .oauth_cache
            .take_ready_binding("state.invalid", 1, 2)
            .is_none());
        assert!(state
            .oauth_cache
            .take_ready_binding("state.secret", 1, 2)
            .is_some());
        assert!(state
            .oauth_cache
            .take_ready_binding("state.secret", 1, 2)
            .is_none());
    }
}

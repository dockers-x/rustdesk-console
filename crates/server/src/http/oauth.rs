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
    let (st, verifier, nonce, url) =
        services::oauth::begin_auth(&state.config, &state.db, &f.op).await?;
    let poll_secret = crate::support::random::random_string(40);
    let code = format!("{st}.{poll_secret}");
    state.oauth_cache.set(
        &st,
        OauthCacheItem {
            action: ACTION_LOGIN.into(),
            poll_secret,
            id: f.id.clone(),
            op: f.op.clone(),
            uuid: f.uuid.clone(),
            device_name: f.device_info.name.clone(),
            device_os: f.device_info.os.clone(),
            device_type: device_type.to_string(),
            verifier,
            nonce,
            ..Default::default()
        },
        5 * 60,
    );
    Ok(json!({ "code": code, "url": url }))
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
    if pending.user_id == 0 {
        audit.decision("federated", "pending", "awaiting_provider");
        return Err(PollError::Pending);
    }
    let Some(v) = state.oauth_cache.take_ready_login(code, console) else {
        audit.decision("federated", "rejected", "attempt_expired");
        return Err(PollError::Expired);
    };
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
    let session = services::user::login(
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
    audit.decision("federated", "rejected", "callback_denied");
    if q.state.is_empty() {
        return render_template(&state, "oauth_fail.html", "ParamIsEmpty", "state");
    }
    let Some(mut cache) = state.oauth_cache.get(&q.state) else {
        return render_template(&state, "oauth_fail.html", "OauthExpired", "");
    };
    let oauth_user = match services::oauth::callback(
        &state.config,
        &state.db,
        &q.code,
        &cache.verifier,
        &cache.nonce,
        &cache.op,
    )
    .await
    {
        Ok(u) => u,
        Err(e) => return render_template(&state, "oauth_fail.html", "OauthFailed", &e),
    };
    let openid = oauth_user.open_id.clone();

    if cache.action == ACTION_BIND {
        let Some(binding) = state.oauth_cache.take_binding(&q.state) else {
            return render_template(&state, "oauth_fail.html", "OauthExpired", "");
        };
        let Some(session_id) = binding.source_session_id else {
            return render_template(&state, "oauth_fail.html", "BindFail", "");
        };
        if services::oauth::bind_user_with_session(
            &state.db,
            binding.user_id,
            session_id,
            &oauth_user,
            &binding.op,
        )
        .await
        .is_err()
        {
            return render_template(&state, "oauth_fail.html", "BindFail", "");
        }
        audit.subject(cache.user_id);
        audit.decision("federated", "accepted", "provider_bound");
        return render_template(&state, "oauth_success.html", "BindSuccess", "");
    }

    if cache.action == ACTION_LOGIN {
        if cache.user_id != 0 {
            return render_template(&state, "oauth_fail.html", "OauthHasBeenSuccess", "");
        }
        // find existing bound user
        let mut user = None;
        if let Ok(Some(ut)) = services::oauth::user_third_info(&state.db, &cache.op, &openid).await
        {
            user = services::user::info_by_id(&state.db, ut.user_id)
                .await
                .ok()
                .flatten();
        }
        let user = match user {
            Some(u) => u,
            None => {
                let auto = services::oauth::info_by_op(&state.db, &cache.op)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|o| o.auto_register)
                    .unwrap_or(false);
                if !auto {
                    // store oauth user info and redirect to the bind page
                    cache.open_id = oauth_user.open_id.clone();
                    cache.username = oauth_user.username.clone();
                    cache.name = oauth_user.name.clone();
                    cache.email = oauth_user.email.clone();
                    if !state.oauth_cache.update_login_callback(&q.state, cache) {
                        return render_template(&state, "oauth_fail.html", "OauthExpired", "");
                    }
                    audit.decision("federated", "pending", "binding_required");
                    return Redirect::to(&format!("/_admin/#/oauth/bind/{}", q.state))
                        .into_response();
                }
                match services::oauth::register_or_login(
                    &state.db,
                    &oauth_user,
                    &cache.op,
                    services::login_security::registration_enabled(
                        &state.db,
                        state.config.app.register,
                    )
                    .await
                    .unwrap_or(false),
                )
                .await
                {
                    Ok(u) => u,
                    Err(e) => return render_template(&state, "oauth_fail.html", &e, ""),
                }
            }
        };
        cache.user_id = user.id;
        let console = cache.device_type == entity::login_log::CLIENT_WEB_ADMIN;
        if !state.oauth_cache.update_login_callback(&q.state, cache) {
            return render_template(&state, "oauth_fail.html", "OauthExpired", "");
        }
        audit.subject(user.id);
        audit.decision("federated", "pending", "provider_identity_verified");
        if console {
            return Redirect::to("/_admin/#/").into_response();
        }
        return render_template(&state, "oauth_success.html", "OauthSuccess", "");
    }

    render_template(&state, "oauth_fail.html", "ParamsError", "")
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
    if let Ok(Some(_)) =
        services::oauth::user_third_by_user_and_op(&state.db, user.user.id, &f.op).await
    {
        return resp::fail(101, state.tr(&lang, "OauthHasBindOtherUser"));
    }
    let (st, verifier, nonce, url) =
        match services::oauth::begin_auth(&state.config, &state.db, &f.op).await {
            Ok(v) => v,
            Err(e) => return resp::error(state.tr(&lang, &e)),
        };
    state.oauth_cache.set(
        &st,
        OauthCacheItem {
            action: ACTION_BIND.into(),
            source_session_id: Some(user.user_token.id),
            op: f.op.clone(),
            user_id: user.user.id,
            verifier,
            nonce,
            ..Default::default()
        },
        5 * 60,
    );
    resp::no_store(resp::success(json!({ "code": st, "url": url })))
}

pub async fn admin_bind_confirm(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<CodeForm>,
) -> Response {
    if f.code.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if state.oauth_cache.get(&f.code).is_none() {
        return resp::fail(101, state.tr(&lang, "OauthExpired"));
    }
    let Some(v) = state.oauth_cache.claim_login_binding(&f.code) else {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    };
    let identity = services::oauth::OauthUser {
        open_id: v.open_id,
        username: v.username,
        name: v.name,
        email: v.email,
        ..Default::default()
    };
    let linked = services::oauth::bind_user_with_session(
        &state.db,
        user.user.id,
        user.user_token.id,
        &identity,
        &v.op,
    )
    .await
    .is_ok();
    state
        .oauth_cache
        .finish_login_binding(&f.code, linked.then_some(user.user.id));
    if linked {
        resp::success(serde_json::Value::Null)
    } else {
        resp::fail(101, state.tr(&lang, "BindFail"))
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
    match state.oauth_cache.get(&q.code) {
        Some(v) => resp::success(v),
        None => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use sea_orm::{
        ActiveModelTrait, ConnectionTrait, Database, DatabaseConnection, DbBackend, Schema, Set,
    };
    use serde_json::Value;

    async fn response_body(response: Response) -> Value {
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
    }

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
    async fn review_oauth_rejects_wrong_actions_and_unfinished_binding() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for statement in [
            schema.create_table_from_entity(entity::user::Entity),
            schema.create_table_from_entity(entity::system_setting::Entity),
            schema.create_table_from_entity(entity::user_token::Entity),
            schema.create_table_from_entity(entity::trusted_browser::Entity),
            schema.create_table_from_entity(entity::user_third::Entity),
            schema.create_table_from_entity(entity::oauth::Entity),
        ] {
            db.execute(db.get_database_backend().build(&statement))
                .await
                .unwrap();
        }
        let current = entity::user::ActiveModel {
            id: Set(1),
            status: Set(entity::user::STATUS_ENABLE),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let token = entity::user_token::ActiveModel {
            user_id: Set(1),
            token: Set("token".into()),
            expired_at: Set(chrono::Utc::now().timestamp() + 3600),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        entity::oauth::ActiveModel {
            op: Set("oidc".into()),
            oauth_type: Set("oidc".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let state = crate::http::api::deployment_tests::test_state(db);
        state.oauth_cache.set(
            "bind",
            OauthCacheItem {
                action: ACTION_BIND.into(),
                user_id: 1,
                poll_secret: "test".into(),
                ..Default::default()
            },
            300,
        );
        let response = auth_query_pre(
            &state,
            "ip",
            "bind.test",
            OidcClient::Native,
            &AuditContext::default(),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(
            response_body(response.response(&state, "en")).await["error"],
            state.tr("en", "ParamsError")
        );
        for item in [
            OauthCacheItem {
                action: ACTION_LOGIN.into(),
                ..Default::default()
            },
            OauthCacheItem {
                action: ACTION_BIND.into(),
                open_id: "identity".into(),
                ..Default::default()
            },
            OauthCacheItem {
                action: ACTION_LOGIN.into(),
                open_id: "identity".into(),
                user_id: 2,
                ..Default::default()
            },
        ] {
            state.oauth_cache.set("invalid", item, 300);
            let response = admin_bind_confirm(
                State(state.clone()),
                AcceptLang("en".into()),
                BackendUser {
                    user: current.clone(),
                    token: String::new(),
                    user_token: token.clone(),
                },
                Json(CodeForm {
                    code: "invalid".into(),
                }),
            )
            .await;
            assert_eq!(
                response_body(response).await["message"],
                state.tr("en", "ParamsError")
            );
        }
        state.oauth_cache.set(
            "ready",
            OauthCacheItem {
                action: ACTION_LOGIN.into(),
                open_id: "identity".into(),
                op: "oidc".into(),
                ..Default::default()
            },
            300,
        );
        for expected_code in [0, 101] {
            let response = admin_bind_confirm(
                State(state.clone()),
                AcceptLang("en".into()),
                BackendUser {
                    user: current.clone(),
                    token: String::new(),
                    user_token: token.clone(),
                },
                Json(CodeForm {
                    code: "ready".into(),
                }),
            )
            .await;
            assert_eq!(response_body(response).await["code"], expected_code);
        }
    }
}

//! Admin panel API (`/api/admin/*`), ports of `http/controller/admin/*.go`.
//! Phase 1 implements auth, config and full CRUD for user/group/tag/peer/
//! device_group. Remaining admin areas return a clear "not implemented" code.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::{Extension, Json};
use sea_orm::Set;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use entity::{group, peer, user};

use crate::http::browser_trust;
use crate::http::middleware::{AcceptLang, AdminUser, AuditReader, BackendUser, ClientIp, DeviceAccess};
use crate::http::payloads::AdminLoginPayload;
use crate::http::response as resp;
use crate::http::security_audit::AuditContext;
use crate::services;
use crate::services::{audit, audit_devices};
use crate::services::console_roles::ConsoleRole;
use crate::services::login_security::{ChallengeSources, VerificationKind};
use crate::services::user::{LoginProof, AUTH_ENROLLMENT, AUTH_TOTP};
use crate::state::AppState;
use crate::support::admin_config::{AdminPanelConfig, logo_is_valid};
use crate::support::record_storage_config::RecordStorageConfigForm;
use crate::support::webclient_config::WebClientConfig;

pub fn list_json<T: serde::Serialize>(
    list: Vec<T>,
    page: i64,
    total: i64,
    page_size: i64,
) -> Response {
    resp::success(json!({
        "list": list,
        "page": page,
        "total": total,
        "page_size": page_size,
    }))
}

// ---------- login ----------

#[derive(Deserialize, Default)]
pub struct LoginForm {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub captcha: String,
    #[serde(default, rename = "captchaId")]
    pub captcha_id: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default, rename = "verificationCode")]
    pub verification_code: String,
    #[serde(default, rename = "tfaCode")]
    pub tfa_code: String,
    #[serde(default, rename = "trustBrowser")]
    pub trust_browser: bool,
    #[serde(default, rename = "useVerificationCode")]
    pub use_verification_code: bool,
    #[serde(default)]
    pub secret: String,
}

pub async fn login(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    ClientIp(ip): ClientIp,
    audit: AuditContext,
    headers: HeaderMap,
    Json(f): Json<LoginForm>,
) -> Response {
    if state.config.app.disable_pwd_login && f.secret.trim().is_empty() {
        audit.decision("password", "rejected", "password_login_disabled");
        return resp::fail(101, state.tr(&lang, "PwdLoginDisabled"));
    }
    let (banned, need_captcha) = state.limiter.check_security_status(&ip);
    if banned {
        audit.decision("login", "rejected", "source_banned");
        return resp::fail(101, state.tr(&lang, "LoginBanned"));
    }
    if !f.secret.trim().is_empty() {
        let verified = match services::login_security::verify_login_challenge(
            &state.db,
            &state.config,
            &f.secret,
            Some(&f.verification_code),
            Some(&f.tfa_code),
            if state.config.app.disable_pwd_login {
                ChallengeSources::FederatedOnly
            } else {
                ChallengeSources::All
            },
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                audit.decision("challenge", "rejected", "challenge_invalid");
                state.limiter.record_failed_attempt(&ip);
                return resp::fail(101, state.tr(&lang, &e));
            }
        };
        let user = verified.account;
        let device = verified.device;
        let method = challenge_method(verified.federated, verified.kind);
        audit.subject(user.id);
        if !user.is_enabled() {
            audit.decision(method, "rejected", "account_disabled");
            return resp::fail(101, state.tr(&lang, "UserDisabled"));
        }
        let trust_browser = f.trust_browser
            && !verified.federated
            && verified.kind == VerificationKind::Totp
            && device.client_type == entity::login_log::CLIENT_WEB_ADMIN;
        let proof = if verified.kind == VerificationKind::Email {
            match console_login_proof(&state, &user).await {
                Ok(proof) => proof,
                Err(message) => {
                    audit.decision(method, "error", "policy_unavailable");
                    return resp::fail(101, message);
                }
            }
        } else {
            LoginProof::Totp
        };
        if verified.federated && verified.oauth_binding_id.is_none() {
            return resp::fail(101, state.tr(&lang, "OauthExpired"));
        }
        let ut = match services::user::login_with_binding(
            &state.db,
            &state.jwt,
            &state.config,
            &user,
            services::user::LoginEvent {
                proof,
                client: entity::login_log::CLIENT_WEB_ADMIN.to_string(),
                device_id: device.id,
                uuid: device.uuid,
                ip: ip.clone(),
                login_type: if verified.federated {
                    entity::login_log::TYPE_OAUTH
                } else {
                    entity::login_log::TYPE_ACCOUNT
                }
                .to_owned(),
                platform: device.os,
            },
            verified.oauth_binding_id,
        )
        .await
        {
            Ok(ut) => ut,
            Err(_) => {
                audit.decision(method, "error", "session_issue_failed");
                return resp::fail(101, state.tr(&lang, "OperationFailed"));
            }
        };
        state.limiter.remove_attempts(&ip);
        let session_id = ut.id;
        let mut response = console_session_response(&audit, method, &user, ut);
        if trust_browser {
            match remember_browser(&state, &headers, &user, session_id).await {
                Ok(cookie) => {
                    response.headers_mut().append(header::SET_COOKIE, cookie);
                    audit.subject(user.id);
                    audit.management("login_security.trust_browser");
                }
                Err(error) => {
                    tracing::warn!(user_id = user.id, %error, "could not remember browser")
                }
            }
        }
        return response;
    }
    if !(2..=256).contains(&f.username.len()) || f.password.len() < 4 {
        audit.decision("password", "rejected", "invalid_input");
        state.limiter.record_failed_attempt(&ip);
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if need_captcha
        && (f.captcha_id.is_empty()
            || f.captcha.is_empty()
            || !state.limiter.verify_captcha(&f.captcha_id, &f.captcha))
    {
        audit.decision("password", "rejected", "captcha_required_or_invalid");
        return resp::fail(101, state.tr(&lang, "CaptchaError"));
    }
    if !state.limiter.reserve_password_attempt(&ip, &f.username) {
        audit.decision("password", "rejected", "rate_limited");
        return resp::fail(101, state.tr(&lang, "LoginBanned"));
    }

    let user = match services::user::info_by_username_password(
        &state.db,
        &state.config,
        &f.username,
        &f.password,
    )
    .await
    {
        Ok(Some(u)) => u,
        Ok(None) => {
            audit.decision("password", "rejected", "invalid_credentials");
            state.limiter.record_failed_attempt(&ip);
            let (_b, nc) = state.limiter.check_security_status(&ip);
            let code = if nc { 110 } else { 101 };
            return resp::fail(code, state.tr(&lang, "UsernameOrPasswordError"));
        }
        Err(_) => {
            audit.decision("password", "error", "credential_store_unavailable");
            return resp::fail(101, state.tr(&lang, "OperationFailed"));
        }
    };
    audit.subject(user.id);
    if !user.is_enabled() {
        audit.decision("password", "rejected", "account_disabled");
        let code = if need_captcha { 110 } else { 101 };
        return resp::fail(code, state.tr(&lang, "UserDisabled"));
    }
    let device = services::login_security::DeviceLoginInfo {
        id: String::new(),
        uuid: String::new(),
        name: "Admin console".to_string(),
        os: f.platform.clone(),
        client_type: entity::login_log::CLIENT_WEB_ADMIN.to_string(),
        ip: ip.clone(),
        auto_login: true,
    };
    let settings = match services::login_security::login_settings(&state.db).await {
        Ok(settings) => settings,
        Err(_) => {
            audit.decision("password", "error", "policy_unavailable");
            return resp::fail(101, state.tr(&lang, "OperationFailed"));
        }
    };
    let trust_allowed = services::browser_trust::allowed(&user, &settings);
    let mut trusted_id = None;
    if trust_allowed && !f.use_verification_code {
        if let Some(id) = browser_trust::browser_id(&headers, &state.config, &user) {
            if services::browser_trust::find(&state.db, &user, &id)
                .await
                .is_ok_and(|row| row.is_some())
            {
                trusted_id = Some(id);
            }
        }
    }
    let (proof, method) = if let Some(id) = trusted_id {
        (
            LoginProof::TrustedBrowser { id },
            "password_trusted_browser",
        )
    } else {
        if let Some(kind) = services::login_security::verification_for(&user, &settings) {
            let challenge = match services::login_security::create_login_challenge(
                &state.db, &user, &device, kind, &lang,
            )
            .await
            {
                Ok(challenge) => challenge,
                Err(e) => {
                    audit.decision("password", "rejected", "challenge_unavailable");
                    return resp::fail(101, e);
                }
            };
            audit.decision("password", "pending", "challenge_issued");
            return console_challenge_response(
                &user,
                challenge,
                trust_allowed.then_some(settings.browser_trust_days),
            );
        }
        match console_login_proof(&state, &user).await {
            Ok(proof) => (proof, "password"),
            Err(message) => {
                audit.decision("password", "error", "policy_unavailable");
                return resp::fail(101, message);
            }
        }
    };
    let ut = match services::user::login(
        &state.db,
        &state.jwt,
        &state.config,
        &user,
        services::user::LoginEvent {
            proof,
            client: entity::login_log::CLIENT_WEB_ADMIN.to_string(),
            device_id: String::new(),
            uuid: String::new(),
            ip: ip.clone(),
            login_type: entity::login_log::TYPE_ACCOUNT.to_string(),
            platform: f.platform,
        },
    )
    .await
    {
        Ok(ut) => ut,
        Err(_) => {
            audit.decision(method, "error", "session_issue_failed");
            return resp::fail(101, state.tr(&lang, "OperationFailed"));
        }
    };
    state.limiter.remove_attempts(&ip);
    console_session_response(&audit, method, &user, ut)
}

async fn remember_browser(
    state: &AppState,
    headers: &HeaderMap,
    account: &user::Model,
    session_id: i32,
) -> anyhow::Result<HeaderValue> {
    use services::browser_trust::{Enrollment, DAY_SECONDS};

    let settings = services::login_security::login_settings(&state.db).await?;
    let seconds = i64::from(settings.browser_trust_days) * DAY_SECONDS;
    let id = uuid::Uuid::new_v4();
    let cookie = browser_trust::issue(&state.config, account, id, seconds)?;
    let previous = browser_trust::browser_id(headers, &state.config, account);
    let (browser, platform) = browser_trust::browser_info(headers);
    services::browser_trust::enroll(
        &state.db,
        account,
        session_id,
        Enrollment {
            id: id.to_string(),
            previous: previous.as_deref(),
            browser,
            platform,
            seconds,
        },
    )
    .await?;
    Ok(cookie)
}

pub(super) fn challenge_method(federated: bool, kind: VerificationKind) -> &'static str {
    match (federated, kind) {
        (false, VerificationKind::Totp) => "password_totp",
        (false, VerificationKind::Email) => "password_email",
        (true, VerificationKind::Totp) => "federated_totp",
        (true, VerificationKind::Email) => "federated_email",
    }
}

pub(super) async fn console_login_proof(
    state: &AppState,
    user: &user::Model,
) -> Result<LoginProof, &'static str> {
    match services::user::console_requires_totp(&state.db, user).await {
        Ok(true) if !user.tfa_enabled => Ok(LoginProof::Enrollment),
        Ok(false) if user.is_admin() => Ok(LoginProof::ConsoleAdministrator),
        Ok(false) => Ok(LoginProof::SingleFactor),
        Ok(true) => Err("TOTP verification required"),
        Err(_) => Err("Login policy unavailable"),
    }
}

pub(super) fn console_challenge_response(
    user: &user::Model,
    challenge: services::login_security::LoginChallenge,
    trust_days: Option<u32>,
) -> Response {
    resp::no_store(resp::success(json!({
        "type": "email_check",
        "tfa_type": challenge.kind.response_tfa_type(),
        "secret": challenge.secret,
        "trust_browser": trust_days.is_some(),
        "trust_browser_days": trust_days,
        "user": { "username": user.username, "email": user.email },
    })))
}

pub(super) fn console_session_response(
    audit: &AuditContext,
    method: &'static str,
    user: &user::Model,
    session: entity::user_token::Model,
) -> Response {
    let enrollment_only = session.auth_level == AUTH_ENROLLMENT;
    audit.principal(user.id, session.id);
    audit.decision(
        method,
        "accepted",
        if enrollment_only {
            "enrollment_session_issued"
        } else {
            "session_issued"
        },
    );
    resp::no_store(resp::success(AdminLoginPayload::from_user(
        user,
        session.token,
        enrollment_only,
    )))
}

pub async fn captcha(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    AcceptLang(lang): AcceptLang,
) -> Response {
    let (banned, need_captcha) = state.limiter.check_security_status(&ip);
    if banned {
        return resp::fail(101, state.tr(&lang, "LoginBanned"));
    }
    if !need_captcha {
        return resp::fail(101, state.tr(&lang, "NoCaptchaRequired"));
    }
    match state.limiter.require_captcha() {
        Ok(c) => resp::success(json!({ "captcha": { "id": c.id, "b64": c.b64 } })),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "CaptchaError"), e)),
    }
}

pub async fn logout(State(state): State<AppState>, user: BackendUser) -> Response {
    match services::user::logout(&state.db, user.user.id, &user.token).await {
        Ok(()) => resp::success(Value::Null),
        Err(error) => resp::fail(101, error.to_string()),
    }
}

pub async fn login_options(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    AcceptLang(lang): AcceptLang,
) -> Response {
    let (banned, need_captcha) = state.limiter.check_security_status(&ip);
    if banned {
        return resp::fail(101, state.tr(&lang, "LoginBanned"));
    }
    let ops: Vec<String> = services::oauth::get_login_providers(&state.db, &state.config)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|provider| provider.name)
        .collect();
    resp::success(json!({
        "ops": ops,
        "register": services::login_security::registration_enabled(&state.db, state.config.app.register).await.unwrap_or(false),
        "need_captcha": need_captcha,
        "disable_pwd": state.config.app.disable_pwd_login,
        "auto_oidc": state.config.app.disable_pwd_login && ops.len() == 1,
    }))
}

#[derive(Deserialize, Default)]
pub struct PasswordResetRequestForm {
    #[serde(default)]
    pub account: String,
}

pub async fn password_reset_request(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    AcceptLang(lang): AcceptLang,
    Json(f): Json<PasswordResetRequestForm>,
) -> Response {
    if state.limiter.check_security_status(&ip).0 {
        return resp::fail(101, "LoginBanned");
    }
    if f.account.trim().is_empty() {
        return resp::fail(101, "ParamsError");
    }
    match services::login_security::request_password_reset(&state.db, &f.account, &ip, &lang).await
    {
        Ok(secret) => resp::success(json!({ "secret": secret })),
        Err(e) => resp::fail(101, e),
    }
}

#[derive(Deserialize, Default)]
pub struct PasswordResetConfirmForm {
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub password: String,
    #[serde(default, rename = "confirmPassword")]
    pub confirm_password: String,
}

pub async fn password_reset_confirm(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    ClientIp(ip): ClientIp,
    audit: AuditContext,
    Json(f): Json<PasswordResetConfirmForm>,
) -> Response {
    if state.limiter.check_security_status(&ip).0 {
        return resp::fail(101, state.tr(&lang, "LoginBanned"));
    }
    if f.secret.trim().is_empty()
        || f.code.trim().is_empty()
        || f.password.len() < 4
        || f.password != f.confirm_password
    {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::login_security::confirm_password_reset(
        &state.db,
        &f.secret,
        &f.code,
        &f.password,
    )
    .await
    {
        Ok(user_id) => {
            audit.subject(user_id);
            audit.decision("password_reset", "accepted", "password_reset_completed");
            resp::success(Value::Null)
        }
        Err(e) => {
            audit.decision("password_reset", "rejected", "reset_denied");
            state.limiter.record_failed_attempt(&ip);
            resp::fail(101, state.tr(&lang, &e))
        }
    }
}

pub async fn setup_status(State(state): State<AppState>) -> Response {
    let initialized = services::user::has_admin(&state.db).await.unwrap_or(true);
    resp::success(json!({
        "initialized": initialized,
        "can_initialize": !initialized,
        "title": state.config.admin.title,
    }))
}

#[derive(Deserialize, Default)]
pub struct SetupInitializeForm {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default, rename = "confirmPassword")]
    pub confirm_password: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub nickname: String,
}

pub async fn setup_initialize(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    ClientIp(ip): ClientIp,
    audit: AuditContext,
    Json(f): Json<SetupInitializeForm>,
) -> Response {
    if services::user::has_admin(&state.db).await.unwrap_or(true) {
        return resp::fail(101, state.tr(&lang, "SetupAlreadyInitialized"));
    }
    let username = f.username.trim();
    if username.len() < 2 || f.password.len() < 4 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if f.password != f.confirm_password {
        return resp::fail(101, state.tr(&lang, "PasswordMismatch"));
    }
    let model = user::Model {
        id: 0,
        username: username.to_string(),
        email: f.email.trim().to_string(),
        password: f.password,
        nickname: f.nickname.trim().to_string(),
        avatar: String::new(),
        group_id: 1,
        is_admin: Some(true),
        status: user::STATUS_ENABLE,
        must_change_password: false,
        tfa_secret: String::new(),
        tfa_enabled: false,
        tfa_enforced: false,
        email_verification_enabled: false,
        login_device_verification_enabled: false,
        remark: String::new(),
        created_at: None,
        updated_at: None,
    };
    let user = match services::user::create(&state.db, model).await {
        Ok(user) => user,
        Err(e) => return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    };
    let proof = match console_login_proof(&state, &user).await {
        Ok(proof) => proof,
        Err(message) => return resp::fail(101, message),
    };
    let ut = match services::user::login(
        &state.db,
        &state.jwt,
        &state.config,
        &user,
        services::user::LoginEvent {
            proof,
            client: entity::login_log::CLIENT_WEB_ADMIN.to_string(),
            device_id: String::new(),
            uuid: String::new(),
            ip,
            login_type: entity::login_log::TYPE_ACCOUNT.to_string(),
            platform: "webadmin".to_string(),
        },
    )
    .await
    {
        Ok(ut) => ut,
        Err(e) => return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    };
    console_session_response(&audit, "setup", &user, ut)
}

// ---------- config ----------

pub async fn config_admin(State(state): State<AppState>, user: Option<BackendUser>) -> Response {
    let timezone = std::env::var("TZ").unwrap_or_default();
    let audit_timezone = audit::audit_timezone().name();
    let Some(user) = user else {
        let cfg = state.admin_config.view(None).await;
        return resp::success(json!({
            "title": cfg.title,
            "timezone": timezone,
            "audit_timezone": audit_timezone,
            "logo_url": cfg.logo_url,
        }));
    };
    let cfg = state.admin_config.view(Some(&user.user.username)).await;
    resp::success(json!({
        "title": cfg.title,
        "hello": cfg.hello,
        "timezone": timezone,
        "audit_timezone": audit_timezone,
        "logo_url": cfg.logo_url,
    }))
}

pub async fn config_admin_manage(State(state): State<AppState>, user: AdminUser) -> Response {
    let timezone = std::env::var("TZ").unwrap_or_default();
    let cfg = state.admin_config.view(Some(&user.user.username)).await;
    resp::success(json!({
        "title": cfg.title,
        "hello": cfg.hello,
        "hello_raw": cfg.hello_raw,
        "hello_file": cfg.hello_file,
        "timezone": timezone,
        "logo": cfg.logo,
    }))
}

pub async fn config_admin_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<AdminPanelConfig>,
) -> Response {
    if f.title.chars().count() > 80
        || f.hello_file.chars().count() > 500
        || f.hello.chars().count() > 5000
    {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if f.logo.as_deref().is_some_and(|logo| !logo_is_valid(logo)) {
        return resp::fail(101, state.tr(&lang, "InvalidSiteLogo"));
    }

    match state.admin_config.update(f).await {
        Ok(_) => {
            let cfg = state.admin_config.view(None).await;
            resp::success(cfg)
        }
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

#[derive(Deserialize, Default)]
pub struct LoginSecurityConfigForm {
    #[serde(default)]
    pub login: services::login_security::LoginSecuritySettings,
}

pub async fn login_security_config(State(state): State<AppState>, _user: AdminUser) -> Response {
    match services::login_security::config_view(&state.db, state.config.app.register).await {
        Ok(view) => resp::success(view),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn login_security_config_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<LoginSecurityConfigForm>,
) -> Response {
    if let Err(e) = services::login_security::save_login_settings(&state.db, f.login).await {
        return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e));
    }
    match services::login_security::config_view(&state.db, state.config.app.register).await {
        Ok(view) => resp::success(view),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct SmtpEmailConfigForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub clear_password: bool,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub from_name: String,
    #[serde(default)]
    pub tls: String,
}

impl SmtpEmailConfigForm {
    fn into_update(self) -> services::login_security::SmtpEmailConfigUpdate {
        services::login_security::SmtpEmailConfigUpdate {
            name: self.name,
            host: self.host,
            port: self.port,
            username: self.username,
            password: self.password,
            clear_password: self.clear_password,
            from: self.from,
            from_name: self.from_name,
            tls: self.tls,
        }
    }
}

fn validate_smtp_form(lang: &str, state: &AppState, f: &SmtpEmailConfigForm) -> Option<Response> {
    let name = f.name.trim();
    let host = f.host.trim();
    let from = f.from.trim();
    let from_name = f.from_name.trim();
    if name.chars().count() > 80
        || host.is_empty()
        || host.chars().count() > 255
        || f.username.chars().count() > 255
        || f.password.chars().count() > 500
        || from.is_empty()
        || from.chars().count() > 255
        || !from.contains('@')
        || from_name.chars().count() > 120
        || from_name.chars().any(char::is_control)
    {
        return Some(resp::fail(101, state.tr(lang, "ParamsError")));
    }
    None
}

pub async fn smtp_email_configs(State(state): State<AppState>, _user: AdminUser) -> Response {
    match services::login_security::list_smtp_email_configs(&state.db).await {
        Ok(list) => resp::success(list),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn smtp_email_config_save(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<SmtpEmailConfigForm>,
) -> Response {
    if let Some(response) = validate_smtp_form(&lang, &state, &f) {
        return response;
    }
    let id = (f.id > 0).then_some(f.id);
    match services::login_security::save_smtp_email_config(&state.db, id, f.into_update()).await {
        Ok(saved) => resp::success(saved),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn smtp_email_config_enable(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    if f.id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::login_security::enable_smtp_email_config(&state.db, f.id).await {
        Ok(saved) => resp::success(saved),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn smtp_email_config_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    if f.id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::login_security::delete_smtp_email_config(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

#[derive(Deserialize, Default)]
pub struct TestEmailForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub to: String,
}

pub async fn smtp_email_config_test(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: AdminUser,
    Json(f): Json<TestEmailForm>,
) -> Response {
    send_test_email_response(state, lang, user, (f.id > 0).then_some(f.id), f.to).await
}

pub async fn login_security_test_email(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: AdminUser,
    Json(f): Json<TestEmailForm>,
) -> Response {
    send_test_email_response(state, lang, user, None, f.to).await
}

async fn send_test_email_response(
    state: AppState,
    lang: String,
    user: AdminUser,
    config_id: Option<i32>,
    to: String,
) -> Response {
    let to = if to.trim().is_empty() {
        user.user.email.trim()
    } else {
        to.trim()
    };
    if to.is_empty() || !to.contains('@') {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::login_security::send_test_email(&state.db, config_id, to, &lang).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e),
    }
}

pub async fn config_server(State(state): State<AppState>, _user: BackendUser) -> Response {
    resp::success(state.webclient_config.get().await)
}

pub async fn config_server_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<WebClientConfig>,
) -> Response {
    match state.webclient_config.update(f).await {
        Ok(cfg) => resp::success(cfg),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn config_deployment(State(state): State<AppState>, _user: BackendUser) -> Response {
    let cfg = state.webclient_config.get().await;
    resp::success(crate::support::deployment_config::build(&cfg))
}

pub async fn config_webclient_capabilities(
    State(state): State<AppState>,
    _user: BackendUser,
) -> Response {
    resp::success(webclient_capabilities(state.external_webclient.is_some()))
}

fn webclient_capabilities(has_v2: bool) -> Value {
    json!({
        "v1": true,
        "v2": has_v2,
        "preferred": if has_v2 { "v2" } else { "v1" },
    })
}

#[cfg(test)]
mod webclient_capability_tests {
    use super::*;

    #[test]
    fn prefers_v2_only_when_external_webclient_is_available() {
        let without_v2 = webclient_capabilities(false);
        assert_eq!(without_v2["v1"], true);
        assert_eq!(without_v2["v2"], false);
        assert_eq!(without_v2["preferred"], "v1");

        let with_v2 = webclient_capabilities(true);
        assert_eq!(with_v2["v1"], true);
        assert_eq!(with_v2["v2"], true);
        assert_eq!(with_v2["preferred"], "v2");
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeploymentPreviewRequest {
    #[serde(flatten)]
    pub config: WebClientConfig,
    #[serde(default)]
    pub permanent_password: String,
    #[serde(default)]
    pub approve_mode: String,
    #[serde(default)]
    pub verification_method: String,
}

pub async fn config_deployment_preview(
    _user: BackendUser,
    Json(f): Json<DeploymentPreviewRequest>,
) -> Response {
    resp::success(crate::support::deployment_config::build_with_options(
        &f.config,
        &crate::support::deployment_config::DeploymentOptions {
            permanent_password: f.permanent_password,
            approve_mode: f.approve_mode,
            verification_method: f.verification_method,
        },
    ))
}

pub async fn config_record_storage(State(state): State<AppState>, _user: BackendUser) -> Response {
    resp::success(state.record_storage_config.view().await)
}

pub async fn config_record_storage_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<RecordStorageConfigForm>,
) -> Response {
    match state.record_storage_config.update(f).await {
        Ok(cfg) => resp::success(cfg),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn config_app(State(state): State<AppState>, _user: BackendUser) -> Response {
    resp::success(json!({ "web_client": state.config.app.web_client }))
}

pub async fn overview(State(state): State<AppState>, _user: BackendUser) -> Response {
    match services::overview::load(&state.db, state.version.as_str(), state.start_time.as_str())
        .await
    {
        Ok(v) => resp::success(v),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn diagnostics_run(State(state): State<AppState>, _user: AdminUser) -> Response {
    let webclient = state.webclient_config.get().await;
    let report = services::diagnostics::run(
        &state.db,
        &state.config,
        &webclient,
        state.version.as_str(),
        state.start_time.as_str(),
    )
    .await;
    resp::success(report)
}

// ---------- shared query/forms ----------

#[derive(Deserialize, Default)]
pub struct PageQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub user_id: Option<i32>,
    #[serde(default)]
    pub collection_id: Option<i32>,
    #[serde(default)]
    pub client: Option<String>,
    #[serde(default, rename = "type")]
    pub r#type: Option<String>,
    pub group_id: Option<i32>,
}

#[derive(Deserialize, Default)]
pub struct IdForm {
    #[serde(default)]
    pub id: i32,
}

#[derive(Deserialize, Default)]
pub struct IdsForm {
    #[serde(default)]
    pub ids: Vec<i32>,
}

// ---------- message ----------

#[derive(Deserialize, Default)]
pub struct MessageQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct MessageForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub recipient_id: i32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub status: i32,
}

pub async fn message_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<MessageQuery>,
) -> Response {
    match services::message::admin_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.kind,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn message_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: AdminUser,
    Json(f): Json<MessageForm>,
) -> Response {
    match services::message::create(
        &state.db,
        services::message::CreateInput {
            sender: user.user,
            kind: f.kind,
            recipient_id: f.recipient_id,
            title: f.title,
            body: f.body,
            status: f.status,
            admin_mode: true,
        },
    )
    .await
    {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn message_status(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<MessageForm>,
) -> Response {
    let row = match services::message::info_by_id(&state.db, f.id).await {
        Ok(Some(row)) => row,
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    };
    match services::message::set_status(&state.db, row, f.status).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn message_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::message::delete(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

// ---------- user CRUD ----------

#[derive(Deserialize, Default)]
pub struct UserForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub avatar: String,
    #[serde(default)]
    pub group_id: i32,
    #[serde(default)]
    pub is_admin: Option<bool>,
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub remark: Option<String>,
    #[serde(default)]
    pub password: String,
}

impl UserForm {
    fn to_model(&self) -> user::Model {
        user::Model {
            id: self.id,
            username: self.username.clone(),
            email: self.email.clone(),
            password: self.password.clone(),
            nickname: self.nickname.clone(),
            avatar: self.avatar.clone(),
            group_id: self.group_id,
            is_admin: self.is_admin,
            status: self.status,
            must_change_password: false,
            tfa_secret: String::new(),
            tfa_enabled: false,
            tfa_enforced: false,
            email_verification_enabled: false,
            login_device_verification_enabled: false,
            remark: self.remark.clone().unwrap_or_default(),
            created_at: None,
            updated_at: None,
        }
    }
}

pub async fn user_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<PageQuery>,
) -> Response {
    match services::user::list_filtered(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.username,
        q.group_id,
    )
    .await
    {
        Ok(r) => {
            let login_settings = services::login_security::login_settings(&state.db)
                .await
                .unwrap_or_default();
            let mut list = Vec::with_capacity(r.list.len());
            for row in r.list {
                let trusted_device_count =
                    services::login_security::trusted_device_count_for_user(&state.db, row.id)
                        .await
                        .unwrap_or(0);
                let mut value = match serde_json::to_value(&row) {
                    Ok(Value::Object(map)) => map,
                    _ => serde_json::Map::new(),
                };
                value.insert(
                    "trusted_device_count".to_string(),
                    Value::from(trusted_device_count),
                );
                value.insert(
                    "system_require_totp".to_string(),
                    Value::from(
                        login_settings.require_totp
                            || (row.is_admin() && login_settings.require_admin_totp),
                    ),
                );
                value.insert(
                    "system_require_email_verification".to_string(),
                    Value::from(login_settings.require_email_verification),
                );
                value.insert(
                    "system_require_device_verification".to_string(),
                    Value::from(login_settings.require_device_verification),
                );
                value.insert(
                    "system_allow_trusted_login_devices".to_string(),
                    Value::from(login_settings.allow_trusted_login_devices),
                );
                value.insert(
                    "effective_tfa_required".to_string(),
                    Value::from(
                        row.tfa_enabled && (row.tfa_enforced || login_settings.require_totp),
                    ),
                );
                value.insert(
                    "effective_email_verification_enabled".to_string(),
                    Value::from(
                        login_settings.require_email_verification || row.email_verification_enabled,
                    ),
                );
                value.insert(
                    "effective_login_device_verification_enabled".to_string(),
                    Value::from(
                        login_settings.require_device_verification
                            || row.login_device_verification_enabled,
                    ),
                );
                list.push(Value::Object(value));
            }
            list_json(list, r.page, r.total, r.page_size)
        }
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn user_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::user::info_by_id(&state.db, id).await {
        Ok(Some(u)) => resp::success(u),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn user_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<UserForm>,
) -> Response {
    match services::user::create(&state.db, f.to_model()).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn user_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    audit: AuditContext,
    Json(f): Json<UserForm>,
) -> Response {
    audit.subject(f.id);
    if f.id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    let mut update = f.to_model();
    if f.remark.is_none() {
        let Ok(Some(current)) = services::user::info_by_id(&state.db, f.id).await else {
            return resp::fail(101, state.tr(&lang, "ItemNotFound"));
        };
        update.remark = current.remark;
    }
    match services::user::update(&state.db, &update).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn user_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    audit: AuditContext,
    Json(f): Json<IdForm>,
) -> Response {
    audit.subject(f.id);
    if f.id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::user::info_by_id(&state.db, f.id).await {
        Ok(Some(u)) => match services::user::delete(&state.db, &u).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, e),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

#[derive(Deserialize, Default)]
pub struct UserPasswordForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub password: String,
    #[serde(default, rename = "confirmPassword")]
    pub confirm_password: Option<String>,
}

pub async fn user_change_pwd(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: AdminUser,
    audit: AuditContext,
    Json(f): Json<UserPasswordForm>,
) -> Response {
    audit.subject(f.id);
    if f.password.len() < 4 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if f.confirm_password
        .as_ref()
        .is_some_and(|confirm| confirm != &f.password)
    {
        return resp::fail(101, state.tr(&lang, "PasswordMismatch"));
    }
    match services::user::info_by_id(&state.db, f.id).await {
        Ok(Some(target)) if target.is_admin() && user.auth_level != AUTH_TOTP => {
            resp::fail(resp::CODE_NO_ACCESS, state.tr(&lang, "NoAccess"))
        }
        Ok(Some(target)) => {
            match services::ldap::is_directory_user(&state.db, target.id).await {
                Ok(false) => {}
                Ok(true) => return resp::fail(101, state.tr(&lang, "DirectoryPasswordManaged")),
                Err(_) => return resp::fail(101, state.tr(&lang, "OperationFailed")),
            }
            match services::user::update_password(&state.db, &target, &f.password).await {
                Ok(()) => resp::success(Value::Null),
                Err(_) => resp::fail(101, state.tr(&lang, "OperationFailed")),
            }
        }
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn user_send_password_reset(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    ClientIp(ip): ClientIp,
    user: AdminUser,
    audit: AuditContext,
    Json(form): Json<IdForm>,
) -> Response {
    audit.subject(form.id);
    let target = match services::user::info_by_id(&state.db, form.id).await {
        Ok(Some(target)) => target,
        Ok(None) => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
        Err(_) => return resp::fail(101, state.tr(&lang, "OperationFailed")),
    };
    if target.is_admin() && user.auth_level != AUTH_TOTP {
        return resp::fail(resp::CODE_NO_ACCESS, state.tr(&lang, "NoAccess"));
    }
    match services::login_security::send_user_password_reset(
        &state.db,
        &target,
        &ip,
        &state.config.lang,
    )
    .await
    {
        Ok(()) => resp::success(Value::Null),
        Err(error) => resp::fail(101, state.tr(&lang, &error)),
    }
}

pub async fn user_current(user: BackendUser, role: Option<Extension<ConsoleRole>>) -> Response {
    let mut payload = AdminLoginPayload::from_user(
        &user.user,
        user.token,
        user.user_token.auth_level == AUTH_ENROLLMENT,
    );
    if !payload.must_enroll_totp {
        match role.map(|Extension(role)| role) {
            Some(ConsoleRole::AuditReader) => payload.route_names = vec!["AuditReader".to_owned()],
            Some(ConsoleRole::DeviceGroupOperator { .. }) => {
                payload.route_names = vec!["DeviceGroupOperator".to_owned()];
            }
            Some(ConsoleRole::ScopedAdministrator { .. }) => {
                payload.route_names = vec!["ScopedAdministrator".to_owned()];
            }
            _ => {}
        }
    }
    resp::no_store(resp::success(payload))
}

#[derive(Deserialize, Default)]
pub struct MyAvatarForm {
    #[serde(default)]
    pub avatar: String,
}

fn valid_user_avatar(avatar: &str) -> bool {
    let avatar = avatar.trim();
    avatar.is_empty()
        || avatar.starts_with("/upload/")
        || avatar.starts_with("http://")
        || avatar.starts_with("https://")
        || avatar.starts_with("data:image/")
}

pub async fn user_update_my_avatar(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<MyAvatarForm>,
) -> Response {
    let avatar = f.avatar.trim();
    if !valid_user_avatar(avatar) {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::user::update_avatar(&state.db, user.user.id, avatar).await {
        Ok(_) => resp::success(json!({ "avatar": avatar })),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn my_security(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
) -> Response {
    let trusted_devices =
        services::login_security::trusted_devices_for_user(&state.db, user.user.id)
            .await
            .unwrap_or_default();
    let settings = match services::login_security::login_settings(&state.db).await {
        Ok(settings) => settings,
        Err(e) => {
            return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e));
        }
    };
    resp::success(json!({
        "tfa_enabled": user.user.tfa_enabled,
        "tfa_enforced": user.user.tfa_enforced,
        "tfa_enforced_for_account": services::login_security::totp_enforced(&user.user, &settings),
        "email_verification_enabled": user.user.email_verification_enabled,
        "login_device_verification_enabled": user.user.login_device_verification_enabled,
        "system_require_totp": settings.require_totp,
        "system_require_email_verification": settings.require_email_verification,
        "system_require_device_verification": settings.require_device_verification,
        "system_allow_trusted_login_devices": settings.allow_trusted_login_devices,
        "effective_tfa_required": services::login_security::totp_required(&user.user, &settings),
        "effective_email_verification_enabled": settings.require_email_verification || user.user.email_verification_enabled,
        "effective_login_device_verification_enabled": settings.require_device_verification || user.user.login_device_verification_enabled,
        "trusted_device_count": trusted_devices.len(),
    }))
}

pub async fn my_tfa_setup(State(state): State<AppState>, user: BackendUser) -> Response {
    if user.user.tfa_enabled {
        return resp::fail(101, "TOTP is already configured");
    }
    let Ok(settings) = services::login_security::login_settings(&state.db).await else {
        return resp::fail(101, "Login policy unavailable");
    };
    let secret = services::login_security::generate_totp_secret();
    let uri = services::login_security::totp_uri(&user.user.username, &secret, &settings.totp_issuer);
    resp::no_store(resp::success(json!({ "secret": secret, "uri": uri })))
}

#[derive(Deserialize, Default)]
pub struct TfaCodeForm {
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub code: String,
}

pub async fn my_tfa_enable(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    audit: AuditContext,
    Json(f): Json<TfaCodeForm>,
) -> Response {
    audit.subject(user.user.id);
    if f.secret.trim().is_empty() || f.code.trim().is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::login_security::enable_user_totp(
        &state.db,
        &state.config,
        &user.user,
        f.secret.trim(),
        f.code.trim(),
    )
    .await
    {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, state.tr(&lang, &e)),
    }
}

pub async fn my_tfa_disable(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    audit: AuditContext,
    Json(f): Json<TfaCodeForm>,
) -> Response {
    audit.subject(user.user.id);
    match services::login_security::disable_user_totp(&state.db, &state.config, &user.user, Some(&f.code)).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, state.tr(&lang, &e)),
    }
}

pub async fn my_trusted_login_devices(
    State(state): State<AppState>,
    user: BackendUser,
) -> Response {
    match services::login_security::trusted_devices_for_user(&state.db, user.user.id).await {
        Ok(list) => resp::success(list),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct TrustedDeviceForm {
    #[serde(default)]
    pub id: i32,
}

pub async fn my_trusted_login_device_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<TrustedDeviceForm>,
) -> Response {
    if f.id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::login_security::delete_trusted_device(&state.db, Some(user.user.id), f.id).await
    {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct UserSecurityForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub tfa_enforced: bool,
    #[serde(default)]
    pub email_verification_enabled: bool,
    #[serde(default)]
    pub login_device_verification_enabled: bool,
}

pub async fn user_security_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    audit: AuditContext,
    Json(f): Json<UserSecurityForm>,
) -> Response {
    audit.subject(f.id);
    let target = match services::user::info_by_id(&state.db, f.id).await {
        Ok(Some(user)) => user,
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    };
    match services::login_security::update_user_login_security(
        &state.db,
        &target,
        f.tfa_enforced,
        f.email_verification_enabled,
        f.login_device_verification_enabled,
    )
    .await
    {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e),
    }
}

pub async fn user_tfa_reset(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: AdminUser,
    audit: AuditContext,
    Json(f): Json<IdForm>,
) -> Response {
    audit.subject(f.id);
    let target = match services::user::info_by_id(&state.db, f.id).await {
        Ok(Some(user)) => user,
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    };
    if target.is_admin() && user.auth_level != AUTH_TOTP {
        return resp::fail(resp::CODE_NO_ACCESS, state.tr(&lang, "NoAccess"));
    }
    match services::login_security::reset_user_totp(&state.db, &target).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e),
    }
}

#[derive(Deserialize, Default)]
pub struct TrustedDevicesQuery {
    #[serde(default)]
    pub user_id: i32,
}

pub async fn user_trusted_login_devices(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Query(q): Query<TrustedDevicesQuery>,
) -> Response {
    if q.user_id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::login_security::trusted_devices_for_user(&state.db, q.user_id).await {
        Ok(list) => resp::success(list),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn user_trusted_login_device_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<TrustedDeviceForm>,
) -> Response {
    if f.id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::login_security::delete_trusted_device(&state.db, None, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct ChangeCurPwdForm {
    #[serde(default)]
    pub old_password: String,
    #[serde(default)]
    pub new_password: String,
}

pub async fn user_change_cur_pwd(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<ChangeCurPwdForm>,
) -> Response {
    if f.new_password.len() < 4 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if !services::user::is_password_empty(&user.user) {
        let (ok, _) =
            crate::support::password::verify_password(&user.user.password, &f.old_password);
        if !ok {
            return resp::fail(101, state.tr(&lang, "OldPasswordError"));
        }
    }
    match services::user::update_password(&state.db, &user.user, &f.new_password).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn user_group_users(State(state): State<AppState>, _user: BackendUser) -> Response {
    let groups = services::group::list(&state.db, 1, 999)
        .await
        .map(|r| r.list)
        .unwrap_or_default();
    let users = services::user::list(&state.db, 1, 9999, None)
        .await
        .map(|r| r.list)
        .unwrap_or_default();
    resp::success(json!({ "groups": groups, "users": users }))
}

pub async fn user_my_oauth(State(_state): State<AppState>, _user: BackendUser) -> Response {
    // OAuth wired in a later phase; no providers configured.
    resp::success(Vec::<Value>::new())
}

// ---------- webhook / notification routing ----------

#[derive(Deserialize, Default)]
pub struct WebhookQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
}

pub async fn notification_template_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<WebhookQuery>,
) -> Response {
    match services::notification_template::list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
    )
    .await
    {
        Ok(page) => resp::success(page),
        Err(_) => resp::fail(101, "Failed to load notification templates"),
    }
}

pub async fn notification_template_save(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(input): Json<services::notification_template::Input>,
) -> Response {
    match services::notification_template::save(&state.db, input).await {
        Ok(template) => resp::success(template),
        Err(message) => resp::fail(101, message),
    }
}

pub async fn notification_template_delete(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(request): Json<IdForm>,
) -> Response {
    match services::notification_template::delete(&state.db, request.id).await {
        Ok(()) => resp::success(Value::Null),
        Err(message) => resp::fail(101, message),
    }
}

pub async fn webhook_subscription_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<WebhookQuery>,
) -> Response {
    match services::webhook::list_subscriptions(
        &state.db,
        q.page.unwrap_or(1),
        q.page_size.unwrap_or(20),
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn webhook_subscription_save(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<services::webhook::WebhookSubscriptionInput>,
) -> Response {
    match services::webhook::save_subscription(&state.db, f).await {
        Ok(saved) => resp::success(saved),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn webhook_subscription_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    if f.id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::webhook::delete_subscription(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

#[derive(Deserialize)]
pub struct WebhookTestForm {
    #[serde(default)]
    pub id: i32,
    pub config: Option<services::webhook::WebhookSubscriptionInput>,
}

pub async fn webhook_subscription_test(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<WebhookTestForm>,
) -> Response {
    let result = match f.config {
        Some(config) => services::webhook::test_configuration(&state.db, config).await,
        None if f.id > 0 => services::webhook::test_subscription(&state.db, f.id).await,
        None => return resp::fail(101, state.tr(&lang, "ParamsError")),
    };
    match result {
        Ok(()) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e),
    }
}

#[derive(Deserialize, Default)]
pub struct WebhookDeliveryQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub subscription_id: Option<i32>,
}

pub async fn webhook_delivery_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<WebhookDeliveryQuery>,
) -> Response {
    match services::webhook::list_deliveries(
        &state.db,
        q.page.unwrap_or(1),
        q.page_size.unwrap_or(20),
        q.subscription_id,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

// ---------- group CRUD ----------

#[derive(Deserialize, Default)]
pub struct GroupForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "type")]
    pub r#type: i32,
}

pub async fn group_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<PageQuery>,
) -> Response {
    match services::group::list(&state.db, q.page.unwrap_or(0), q.page_size.unwrap_or(0)).await {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn group_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::group::info_by_id(&state.db, id).await {
        Ok(Some(g)) => resp::success(g),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn group_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<GroupForm>,
) -> Response {
    let t = if f.r#type == 0 {
        group::TYPE_DEFAULT
    } else {
        f.r#type
    };
    match services::group::create(&state.db, &f.name, t).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn group_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<GroupForm>,
) -> Response {
    match services::group::update(&state.db, f.id, f.name, f.r#type).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn group_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::group::delete(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

// ---------- device group CRUD ----------

pub async fn device_group_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<PageQuery>,
) -> Response {
    match services::group::device_group_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn device_group_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::group::device_group_info_by_id(&state.db, id).await {
        Ok(Some(g)) => resp::success(g),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn device_group_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<GroupForm>,
) -> Response {
    match services::group::device_group_create(&state.db, &f.name).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn device_group_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<GroupForm>,
) -> Response {
    match services::group::device_group_update(&state.db, f.id, f.name).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn device_group_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::group::device_group_delete(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

// ---------- deployment token CRUD ----------

#[derive(Deserialize, Default)]
pub struct DeploymentTokenQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct DeploymentTokenForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub scopes: Value,
    #[serde(default)]
    pub default_user_id: i32,
    #[serde(default)]
    pub default_device_group_id: i32,
    #[serde(default)]
    pub default_strategy_id: i32,
    #[serde(default)]
    pub expires_at: i64,
    #[serde(default)]
    pub max_uses: i32,
}

pub async fn deployment_token_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<DeploymentTokenQuery>,
) -> Response {
    match services::deployment::list_tokens(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.name,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn deployment_token_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::deployment::info_by_id(&state.db, id).await {
        Ok(Some(row)) => resp::success(row),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn deployment_token_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: AdminUser,
    Json(f): Json<DeploymentTokenForm>,
) -> Response {
    let scopes = parse_scopes(f.scopes);
    let scopes = if scopes.is_empty() {
        services::deployment::default_scopes()
    } else {
        scopes
    };
    let input = services::deployment::CreateTokenInput {
        name: f.name,
        scopes,
        default_user_id: f.default_user_id,
        default_device_group_id: f.default_device_group_id,
        default_strategy_id: f.default_strategy_id,
        expires_at: f.expires_at,
        max_uses: f.max_uses,
        created_by: user.user.id,
    };
    match services::deployment::create_token(&state.db, input).await {
        Ok(created) => resp::success(json!({
            "token": created.token,
            "row": created.row,
        })),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn deployment_token_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::deployment::delete_token(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn deployment_token_revoke(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::deployment::revoke_token(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

// ---------- strategy CRUD ----------

#[derive(Deserialize, Default)]
pub struct StrategyQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct StrategyForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub config_options: Value,
    #[serde(default)]
    pub extra: Value,
}

#[derive(Deserialize, Default)]
pub struct StrategyAssignForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub strategy_id: i32,
    #[serde(default)]
    pub target_type: String,
    #[serde(default)]
    pub target_id: Value,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Deserialize, Default)]
pub struct StrategyAssignmentQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub target_type: Option<String>,
    pub target_id: Option<String>,
}

pub async fn strategy_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<StrategyQuery>,
) -> Response {
    match services::strategy::list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.name,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn strategy_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::strategy::info_by_id(&state.db, id).await {
        Ok(Some(row)) => resp::success(row),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn strategy_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<StrategyForm>,
) -> Response {
    match services::strategy::create(
        &state.db,
        &f.name,
        &f.note,
        f.status,
        parse_string_map(f.config_options),
        parse_string_map(f.extra),
    )
    .await
    {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn strategy_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<StrategyForm>,
) -> Response {
    let row = match services::strategy::info_by_id(&state.db, f.id).await {
        Ok(Some(row)) => row,
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    };
    match services::strategy::update(
        &state.db,
        row,
        &f.name,
        &f.note,
        f.status,
        parse_string_map(f.config_options),
        parse_string_map(f.extra),
    )
    .await
    {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn strategy_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::strategy::delete(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn strategy_assign(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<StrategyAssignForm>,
) -> Response {
    save_strategy_assignment(&state, &lang, None, f).await
}

async fn save_strategy_assignment(
    state: &AppState,
    lang: &str,
    previous: Option<i32>,
    f: StrategyAssignForm,
) -> Response {
    let target_id = value_as_string(f.target_id).unwrap_or_default();
    match services::strategy::replace_assignment(
        &state.db,
        previous,
        f.strategy_id,
        &f.target_type,
        &target_id,
        if f.priority == 0 {
            services::strategy::DEFAULT_PRIORITY
        } else {
            f.priority
        },
    )
    .await
    {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(lang, "OperationFailed"), e)),
    }
}

pub async fn strategy_assignment_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<StrategyAssignmentQuery>,
) -> Response {
    match services::strategy::list_assignments(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.target_type,
        q.target_id,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn strategy_assignment_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::strategy::assignment_by_id(&state.db, id).await {
        Ok(Some(row)) => resp::success(row),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn strategy_assignment_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: AdminUser,
    Json(f): Json<StrategyAssignForm>,
) -> Response {
    strategy_assign(State(state), AcceptLang(lang), user, Json(f)).await
}

pub async fn strategy_assignment_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<StrategyAssignForm>,
) -> Response {
    if f.id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    save_strategy_assignment(&state, &lang, Some(f.id), f).await
}

pub async fn strategy_assignment_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::strategy::delete_assignment(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

fn parse_string_map(value: Value) -> HashMap<String, String> {
    match value {
        Value::Object(obj) => obj
            .into_iter()
            .filter_map(|(k, v)| {
                value_as_string(v)
                    .map(|v| (k.trim().to_string(), v))
                    .filter(|(k, _)| !k.is_empty())
            })
            .collect(),
        Value::String(raw) => {
            let raw = raw.trim();
            if raw.is_empty() {
                return HashMap::new();
            }
            if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(raw) {
                return parse_string_map(Value::Object(obj));
            }
            raw.split(',')
                .filter_map(|pair| {
                    let (k, v) = pair.split_once('=')?;
                    let k = k.trim();
                    if k.is_empty() {
                        return None;
                    }
                    Some((k.to_string(), v.trim().to_string()))
                })
                .collect()
        }
        _ => HashMap::new(),
    }
}

fn parse_scopes(value: Value) -> Vec<String> {
    match value {
        Value::Array(values) => values
            .into_iter()
            .filter_map(value_as_string)
            .filter(|v| !v.is_empty())
            .collect(),
        Value::String(raw) => {
            let raw = raw.trim();
            if raw.is_empty() {
                return vec![];
            }
            if let Ok(Value::Array(values)) = serde_json::from_str::<Value>(raw) {
                return parse_scopes(Value::Array(values));
            }
            raw.split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        }
        _ => vec![],
    }
}

fn value_as_string(value: Value) -> Option<String> {
    match value {
        Value::String(v) => Some(v.trim().to_string()),
        Value::Number(v) => Some(v.to_string()),
        Value::Bool(v) => Some(if v { "Y" } else { "N" }.to_string()),
        _ => None,
    }
}

// ---------- tag CRUD ----------

#[derive(Deserialize, Default)]
pub struct TagForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub user_id: i32,
    #[serde(default)]
    pub color: i64,
    #[serde(default)]
    pub collection_id: i32,
}

pub async fn tag_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<PageQuery>,
) -> Response {
    match services::tag::list_filtered(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.user_id,
        q.collection_id,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn tag_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::tag::info_by_id(&state.db, id).await {
        Ok(Some(t)) => resp::success(t),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn tag_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<TagForm>,
) -> Response {
    match services::tag::create(&state.db, &f.name, f.color, f.user_id, f.collection_id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn tag_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<TagForm>,
) -> Response {
    let model = entity::tag::Model {
        id: f.id,
        name: f.name,
        user_id: f.user_id,
        color: f.color,
        collection_id: f.collection_id,
        created_at: None,
        updated_at: None,
    };
    match services::tag::update(&state.db, &model).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn tag_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::tag::delete(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

// ---------- peer CRUD ----------

#[derive(Deserialize, Default)]
pub struct PeerQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub time_ago: i64,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub exact_id: bool,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
}

pub async fn peer_list(
    State(state): State<AppState>,
    access: DeviceAccess,
    Query(q): Query<PeerQuery>,
) -> Response {
    let f = services::peer::PeerFilters {
        user_id: None,
        time_ago: q.time_ago,
        id: q.id,
        exact_id: q.exact_id,
        hostname: q.hostname,
        username: q.username,
        ip: q.ip,
        alias: q.alias,
    };
    let group_ids = match &access {
        DeviceAccess::Admin => None,
        DeviceAccess::Operator { group_ids, .. } => Some(group_ids.as_slice()),
    };
    match services::peer::list_filtered_in_groups(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        f,
        group_ids,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn peer_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    access: DeviceAccess,
    Path(id): Path<i32>,
) -> Response {
    let peer = match access {
        DeviceAccess::Admin => services::peer::info_by_row_id(&state.db, id).await,
        DeviceAccess::Operator { group_ids, .. } => {
            services::peer::detail_in_groups(&state.db, id, &group_ids).await
        }
    };
    match peer {
        Ok(Some(p)) => resp::success(p),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

#[derive(Deserialize, Default)]
pub struct PeerForm {
    #[serde(default)]
    pub row_id: i32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub group_id: Option<i32>,
    #[serde(default)]
    pub user_id: Option<i32>,
}

pub async fn peer_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    access: DeviceAccess,
    audit: AuditContext,
    Json(f): Json<PeerForm>,
) -> Response {
    audit.subject(f.row_id);
    if f.row_id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if let DeviceAccess::Operator { user_id, .. } = access {
        let denied = || resp::fail(resp::CODE_NO_ACCESS, state.tr(&lang, "NoAccess"));
        if f.id.is_some() || f.user_id.is_some() {
            return denied();
        }
        return match services::peer::update_in_groups(
            &state.db, user_id, f.row_id, f.alias, f.group_id,
        )
        .await
        {
            Ok(true) => resp::success(Value::Null),
            Ok(false) => denied(),
            Err(_) => resp::fail(101, state.tr(&lang, "OperationFailed")),
        };
    }
    let mut am = peer::ActiveModel {
        row_id: Set(f.row_id),
        ..Default::default()
    };
    // Only touch the columns the form actually carried: the device edit form
    // submits just `alias`/`group_id`/`user_id`, and blindly writing the
    // missing ones used to wipe `peers.id`, leaving the device permanently
    // offline (dockers-x/rustdesk-console#3). An empty `id` is never a valid
    // update either, so it is ignored as well.
    if let Some(id) = f.id.filter(|s| !s.is_empty()) {
        am.id = Set(id);
    }
    if let Some(alias) = f.alias {
        am.alias = Set(alias);
    }
    if let Some(group_id) = f.group_id {
        am.group_id = Set(group_id);
    }
    if let Some(user_id) = f.user_id {
        am.user_id = Set(user_id);
    }
    match services::peer::update(&state.db, am).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn peer_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<AbRowIdForm>,
) -> Response {
    match services::peer::info_by_row_id(&state.db, f.row_id).await {
        Ok(Some(p)) => match services::peer::delete(&state.db, &p).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

#[derive(Deserialize, Default)]
pub struct RowIdsForm {
    #[serde(default)]
    pub row_ids: Vec<i32>,
}

pub async fn peer_batch_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<RowIdsForm>,
) -> Response {
    match services::peer::batch_delete(&state.db, &f.row_ids).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn peer_request_sysinfo_refresh(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<AbRowIdForm>,
) -> Response {
    if f.row_id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    let peer = match services::peer::info_by_row_id(&state.db, f.row_id).await {
        Ok(Some(peer)) => peer,
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    };
    let am = peer::ActiveModel {
        row_id: Set(peer.row_id),
        force_sysinfo_refresh: Set(true),
        ..Default::default()
    };
    match services::peer::update(&state.db, am).await {
        Ok(_) => resp::success(json!({ "peer_id": peer.id })),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn peer_enable_trusted_devices(
    _user: AdminUser,
) -> Response {
    resp::fail(101, "Trusted devices must be configured on the client")
}

pub async fn peer_effective_strategy(
    State(state): State<AppState>,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    let Ok(Some(peer)) = services::peer::info_by_row_id(&state.db, id).await else {
        return resp::fail(101, "ItemNotFound");
    };
    match services::strategy::effective_for_peer(&state.db, &peer).await {
        Ok(Some(policy)) => resp::success(json!({
            "id": policy.id, "name": policy.name,
            "source": policy.source, "revision": policy.modified_at,
        })),
        Ok(None) => resp::success(Value::Null),
        Err(_) => resp::fail(101, "OperationFailed"),
    }
}

#[derive(Deserialize, Default)]
pub struct ActiveConnectionQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub peer_id: Option<String>,
    #[serde(default)]
    pub uuid: Option<String>,
}

pub async fn active_connection_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<ActiveConnectionQuery>,
) -> Response {
    match services::active_connection::list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.peer_id,
        q.uuid,
    )
    .await
    {
        Ok(r) => {
            let devices = audit_devices::lookup(
                &state.db,
                true,
                r.list.iter().map(|row| row.peer_id.as_str()),
            )
            .await;
            let list = r
                .list
                .into_iter()
                .map(|row| {
                    let device = devices.get(&row.peer_id).cloned();
                    audit_devices::DeviceFields::peers(row, device, None)
                })
                .collect();
            resp::no_store(list_json(list, r.page, r.total, r.page_size))
        }
        Err(e) => resp::fail(101, e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct PeerDisconnectForm {
    #[serde(default)]
    pub row_id: i32,
    #[serde(default)]
    pub peer_id: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub conn_ids: Vec<i64>,
}

pub async fn peer_disconnect(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<PeerDisconnectForm>,
) -> Response {
    let conn_ids: Vec<i64> = f.conn_ids.into_iter().filter(|id| *id > 0).collect();
    if conn_ids.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }

    let key = if f.row_id > 0 {
        match services::peer::info_by_row_id(&state.db, f.row_id).await {
            Ok(Some(p)) => disconnect_key(&p.uuid, &p.id),
            _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
        }
    } else {
        disconnect_key(&f.uuid, &f.peer_id)
    };
    if key.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }

    let pending = state.disconnect_store.add_pending(&key, &conn_ids);
    resp::success(json!({ "disconnect": pending }))
}

fn disconnect_key(uuid: &str, peer_id: &str) -> String {
    if !uuid.is_empty() {
        uuid.to_string()
    } else {
        peer_id.to_string()
    }
}

// ---------- login log (read) ----------

pub async fn login_log_list(
    State(state): State<AppState>,
    user: AuditReader,
    Query(q): Query<PageQuery>,
) -> Response {
    match services::login_log::list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        services::login_log::LoginLogFilters {
            user_id: None,
            client: q.client,
            login_type: q.r#type,
        },
    )
    .await
    {
        Ok(r) => {
            let devices = audit_devices::lookup(
                &state.db,
                user.can_lookup_devices(),
                r.list
                    .iter()
                    .filter(|row| entity::login_log::is_native_client(&row.client))
                    .map(|row| row.device_id.as_str()),
            )
            .await;
            let list = r
                .list
                .into_iter()
                .map(|row| {
                    let device = if entity::login_log::is_native_client(&row.client) {
                        devices.get(&row.device_id).cloned()
                    } else {
                        None
                    };
                    audit_devices::DeviceFields::login(row, device)
                })
                .collect();
            resp::no_store(list_json(list, r.page, r.total, r.page_size))
        }
        Err(e) => resp::fail(101, e.to_string()),
    }
}

// ---------- not-yet-implemented admin areas ----------

/// Placeholder for admin endpoints whose business logic is deferred to a later
/// phase. Returns a clear, logged "not implemented" code rather than a silent
/// empty success, so nothing reads as done when it isn't.
pub async fn not_implemented(uri: axum::http::Uri) -> Response {
    tracing::warn!("admin endpoint not implemented yet: {}", uri.path());
    resp::fail(501, format!("NotImplemented: {}", uri.path()))
}

// ===========================================================================
// oauth provider management
// ===========================================================================

#[derive(Deserialize, Default)]
pub struct OauthForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub oauth_type: String,
    #[serde(default)]
    pub issuer: String,
    #[serde(default)]
    pub scopes: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub auto_register: Option<bool>,
    #[serde(default)]
    pub pkce_enable: Option<bool>,
    #[serde(default)]
    pub pkce_method: String,
}

impl OauthForm {
    fn to_model(&self) -> entity::oauth::Model {
        entity::oauth::Model {
            id: self.id,
            op: self.op.clone(),
            oauth_type: self.oauth_type.clone(),
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            auto_register: self.auto_register,
            scopes: self.scopes.clone(),
            issuer: self.issuer.clone(),
            pkce_enable: self.pkce_enable,
            pkce_method: self.pkce_method.clone(),
            created_at: None,
            updated_at: None,
        }
    }
}

pub async fn oauth_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<PageQuery>,
) -> Response {
    match services::oauth::list(&state.db, q.page.unwrap_or(0), q.page_size.unwrap_or(0)).await {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn oauth_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::oauth::info_by_id(&state.db, id).await {
        Ok(Some(o)) => resp::success(o),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn oauth_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<OauthForm>,
) -> Response {
    let mut m = f.to_model();
    if let Err(e) = services::oauth::format_oauth_info(&mut m) {
        return resp::fail(101, format!("{}{}", state.tr(&lang, "ParamsError"), e));
    }
    if let Ok(Some(_)) = services::oauth::info_by_op(&state.db, &m.op).await {
        return resp::fail(101, state.tr(&lang, "ItemExists"));
    }
    match services::oauth::create(&state.db, m).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn oauth_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<OauthForm>,
) -> Response {
    if f.id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::oauth::update(&state.db, f.to_model()).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, state.tr(&lang, &e)),
    }
}

pub async fn oauth_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    if f.id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::oauth::info_by_id(&state.db, f.id).await {
        Ok(Some(_)) => match services::oauth::delete(&state.db, f.id).await {
            Ok(_) => resp::success(Value::Null),
            Err(sea_orm::DbErr::Custom(message)) => resp::fail(101, state.tr(&lang, &message)),
            Err(_) => resp::fail(101, state.tr(&lang, "OperationFailed")),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn oauth_test(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<OauthForm>,
) -> Response {
    let mut model = f.to_model();
    if let Err(e) = services::oauth::format_oauth_info(&mut model) {
        return resp::fail(101, format!("{}{}", state.tr(&lang, "ParamsError"), e));
    }
    match services::oauth::test_provider_config(&state.config, model).await {
        Ok(result) => resp::success(result),
        Err(e) => resp::fail(101, state.tr(&lang, &e)),
    }
}

/// Show configured providers and retain unavailable links so users can remove them.
pub async fn user_my_oauth_real(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
) -> Response {
    let (providers, bindings) = tokio::join!(
        services::oauth::get_login_provider_configs(&state.db, &state.config),
        services::oauth::user_thirds_by_user_id(&state.db, user.user.id),
    );
    let (Ok(providers), Ok(bindings)) = (providers, bindings) else {
        return resp::fail(101, state.tr(&lang, "OperationFailed"));
    };
    let status = |op: &str,
                  oauth_type: &str,
                  link: Option<&entity::user_third::Model>,
                  available| {
        json!({
            "op": op,
            "oauth_type": oauth_type,
            "status": i32::from(link.is_some()),
            "available": available,
            "name": link.map(|binding| binding.name.as_str()).unwrap_or_default(),
            "username": link.map(|binding| binding.username.as_str()).unwrap_or_default(),
            "email": link.map(|binding| binding.email.as_str()).unwrap_or_default(),
            "verified_email": link.is_some_and(|binding| binding.verified_email),
            "picture": link.map(|binding| binding.picture.as_str()).unwrap_or_default(),
            "created_at": link.and_then(|binding| binding.created_at).map(|dt| dt.to_string()).unwrap_or_default(),
        })
    };
    let mut result = Vec::with_capacity(providers.len() + bindings.len());
    let mut present = std::collections::HashSet::new();
    for provider in &providers {
        present.insert(provider.op.as_str());
        let active = bindings
            .iter()
            .find(|link| services::oauth::binding_matches(link, provider));
        let link = active.or_else(|| bindings.iter().find(|link| link.op == provider.op));
        result.push(status(
            &provider.op,
            &provider.oauth_type,
            link,
            link.is_none() || active.is_some(),
        ));
    }
    for binding in &bindings {
        if present.insert(&binding.op) {
            result.push(status(
                &binding.op,
                &binding.oauth_type,
                Some(binding),
                false,
            ));
        }
    }
    resp::no_store(resp::success(result))
}

#[derive(Deserialize, Default)]
pub struct UnbindForm {
    #[serde(default)]
    pub op: String,
}

pub async fn oauth_unbind(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    audit: AuditContext,
    Json(f): Json<UnbindForm>,
) -> Response {
    if services::oauth::validate_binding_session(&state.db, user.user.id, user.user_token.id)
        .await
        .is_err()
    {
        return resp::fail(101, state.tr(&lang, "OauthVerifySession"));
    }
    match services::oauth::unbind(
        &state.db,
        &state.config,
        user.user.id,
        user.user_token.id,
        &f.op,
    )
    .await
    {
        Ok(()) => {
            audit.subject(user.user.id);
            audit.management("oauth.unbind");
            resp::success(Value::Null)
        }
        Err(sea_orm::DbErr::RecordNotFound(message) | sea_orm::DbErr::Custom(message)) => {
            resp::fail(101, state.tr(&lang, &message))
        }
        Err(error) => {
            tracing::warn!(%error, user_id = user.user.id, "Could not unlink OAuth identity");
            resp::fail(101, state.tr(&lang, "OperationFailed"))
        }
    }
}

// ===========================================================================
// audit logs
// ===========================================================================

pub async fn audit_conn_list(
    State(state): State<AppState>,
    user: AuditReader,
    AcceptLang(lang): AcceptLang,
    Query(search): Query<audit::ConnSearch>,
) -> Response {
    let query = match search.query(audit::audit_timezone()) {
        Ok(query) => query,
        Err(key) => return resp::fail(101, state.tr(&lang, key)),
    };
    match audit::conn_list_with_details(&state.db, query, &search).await {
        Ok(r) => {
            let devices = audit_devices::lookup(
                &state.db,
                user.can_lookup_devices(),
                r.list
                    .iter()
                    .flat_map(|report| [report.row.peer_id.as_str(), report.row.from_peer.as_str()]),
            )
            .await;
            let list = r
                .list
                .into_iter()
                .map(|row| {
                    let peer = devices.get(&row.row.peer_id).cloned();
                    let from_peer = devices.get(&row.row.from_peer).cloned();
                    audit_devices::DeviceFields::peers(row, peer, from_peer)
                })
                .collect();
            resp::no_store(list_json(list, r.page, r.total, r.page_size))
        }
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn audit_conn_export(
    State(state): State<AppState>,
    _user: AuditReader,
    AcceptLang(lang): AcceptLang,
    Query(search): Query<audit::ConnSearch>,
) -> Response {
    let query = match search.query(audit::audit_timezone()) {
        Ok(query) => query,
        Err(key) => return resp::fail(101, state.tr(&lang, key)),
    };
    audit_export_response(
        &state,
        &lang,
        audit::conn_export(&state.db, query).await,
        "attachment; filename=connection-audit.csv",
    )
}

pub async fn audit_conn_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::audit::conn_info_by_id(&state.db, f.id).await {
        Ok(Some(_)) => match services::audit::delete_conn(&state.db, f.id).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, e.to_string()),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn audit_conn_batch_delete(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(f): Json<IdsForm>,
) -> Response {
    match services::audit::batch_delete_conn(&state.db, &f.ids).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn audit_file_list(
    State(state): State<AppState>,
    user: AuditReader,
    Query(search): Query<services::audit::FileSearch>,
) -> Response {
    match services::audit::file_list_with_details(&state.db, search).await {
        Ok(r) => {
            let devices = services::audit_devices::lookup(
                &state.db,
                user.can_lookup_devices(),
                r.list.iter().flat_map(services::audit::FileAuditRow::peer_ids),
            )
            .await;
            let rows = r
                .list
                .into_iter()
                .map(|row| {
                    let [peer, remote] = row.peer_ids();
                    let peer = devices.get(peer).cloned();
                    let remote = devices.get(remote).cloned();
                    services::audit_devices::DeviceFields::peers(row, peer, remote)
                })
                .collect();
            resp::no_store(list_json(rows, r.page, r.total, r.page_size))
        }
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn audit_file_export(
    State(state): State<AppState>,
    _user: AuditReader,
    AcceptLang(lang): AcceptLang,
    Query(search): Query<audit::FileSearch>,
) -> Response {
    audit_export_response(
        &state,
        &lang,
        audit::file_export(&state.db, &search).await,
        "attachment; filename=file-audit.csv",
    )
}

fn audit_export_response(
    state: &AppState,
    lang: &str,
    result: Result<String, audit::ExportError>,
    disposition: &'static str,
) -> Response {
    let response = match result {
        Ok(csv) => {
            let mut response = Response::new(Body::from(csv));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/csv; charset=utf-8"),
            );
            response.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static(disposition),
            );
            response
        }
        Err(audit::ExportError::Limit) => resp::fail(101, state.tr(lang, "AuditExportLimit")),
        Err(err) => {
            tracing::warn!(error = %err, "audit export failed");
            resp::fail(101, state.tr(lang, "AuditExportFailed"))
        }
    };
    resp::no_store(response)
}

pub async fn audit_file_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::audit::file_info_by_id(&state.db, f.id).await {
        Ok(Some(_)) => match services::audit::delete_file(&state.db, f.id).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, e.to_string()),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn audit_file_batch_delete(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(f): Json<IdsForm>,
) -> Response {
    match services::audit::batch_delete_file(&state.db, &f.ids).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

// ===========================================================================
// recording files
// ===========================================================================

#[derive(Deserialize, Default)]
pub struct RecordFileQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub peer_id: Option<String>,
}

pub async fn record_file_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<RecordFileQuery>,
) -> Response {
    match services::record_file::list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.filename,
        q.peer_id,
    )
    .await
    {
        Ok(r) => {
            let devices = audit_devices::lookup(
                &state.db,
                true,
                r.list.iter().map(|row| row.peer_id.as_str()),
            )
            .await;
            let list = r
                .list
                .into_iter()
                .map(|row| {
                    let device = devices.get(&row.peer_id).cloned();
                    audit_devices::DeviceFields::peers(row, device, None)
                })
                .collect();
            resp::no_store(list_json(list, r.page, r.total, r.page_size))
        }
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn record_file_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    let row = services::record_file::info_by_id(&state.db, f.id)
        .await
        .ok()
        .flatten();
    let Some(row) = row else {
        return resp::fail(101, state.tr(&lang, "ItemNotFound"));
    };
    let storage_cfg = match state.record_storage_config.for_recording(row.storage_profile_id.as_deref()).await {
        Ok(config) => config,
        Err(error) => return resp::fail(101, error),
    };
    if let Err(e) = services::record_storage::delete_object(
        &storage_cfg,
        &state.config.gin.resources_path,
        &row.storage_backend,
        &row.storage_key,
        &row.filename,
    )
    .await
    {
        return resp::fail(101, e);
    }
    if let Err(e) = services::record_storage::cleanup_staging(
        &storage_cfg,
        &state.config.gin.resources_path,
        &row.storage_backend,
        &row.filename,
    )
    .await
    {
        return resp::fail(101, e);
    }
    match services::record_file::delete(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn record_file_download(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    let row = services::record_file::info_by_id(&state.db, id)
        .await
        .ok()
        .flatten();
    let Some(row) = row else {
        return resp::fail(101, state.tr(&lang, "ItemNotFound"));
    };
    let storage_cfg = match state.record_storage_config.for_recording(row.storage_profile_id.as_deref()).await {
        Ok(config) => config,
        Err(error) => return resp::fail(101, error),
    };
    match services::record_storage::read_object(
        &storage_cfg,
        &state.config.gin.resources_path,
        &row.storage_backend,
        &row.storage_key,
        &row.filename,
    )
    .await
    {
        Ok(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = StatusCode::OK;
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            let disposition = format!("attachment; filename=\"{}\"", row.filename);
            if let Ok(value) = HeaderValue::from_str(&disposition) {
                response
                    .headers_mut()
                    .insert(header::CONTENT_DISPOSITION, value);
            }
            response
        }
        Err(e) => resp::fail(101, e),
    }
}

// ===========================================================================
// share records
// ===========================================================================

#[derive(Deserialize, Default)]
pub struct UserIdPageQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub user_id: Option<i32>,
    #[serde(default)]
    pub client: Option<String>,
    #[serde(default, rename = "type")]
    pub r#type: Option<String>,
}

pub async fn share_record_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<UserIdPageQuery>,
) -> Response {
    match services::share_record::list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.user_id,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn share_record_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::share_record::info_by_id(&state.db, f.id).await {
        Ok(Some(_)) => match services::share_record::delete(&state.db, f.id).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn share_record_batch_delete(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(f): Json<IdsForm>,
) -> Response {
    match services::share_record::batch_delete(&state.db, &f.ids).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

// ===========================================================================
// user tokens
// ===========================================================================

pub async fn user_token_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<UserIdPageQuery>,
) -> Response {
    match services::user::token_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.user_id,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn user_token_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    audit: AuditContext,
    Json(f): Json<IdForm>,
) -> Response {
    let t = services::user::token_info_by_id(&state.db, f.id)
        .await
        .ok()
        .flatten();
    let Some(t) = t else {
        return resp::fail(101, state.tr(&lang, "ItemNotFound"));
    };
    if !user.user.is_admin() && t.user_id != user.user.id {
        return resp::fail(101, state.tr(&lang, "NoAccess"));
    }
    audit.subject(t.user_id);
    match services::user::delete_token(&state.db, f.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn user_token_batch_delete(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(f): Json<IdsForm>,
) -> Response {
    match services::user::batch_delete_user_token(&state.db, &f.ids).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

// ===========================================================================
// address book (admin)
// ===========================================================================

#[derive(Deserialize, Default)]
pub struct AddressBookForm {
    #[serde(default)]
    pub row_id: i32,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub user_id: i32,
    #[serde(default)]
    pub user_ids: Vec<i32>,
    #[serde(default, rename = "forceAlwaysRelay")]
    pub force_always_relay: bool,
    #[serde(default, rename = "rdpPort")]
    pub rdp_port: String,
    #[serde(default, rename = "rdpUsername")]
    pub rdp_username: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default, rename = "loginName")]
    pub login_name: String,
    #[serde(default, rename = "sameServer")]
    pub same_server: bool,
    #[serde(default)]
    pub collection_id: i32,
}

impl AddressBookForm {
    pub fn to_model(&self, user_id: i32) -> entity::address_book::Model {
        entity::address_book::Model {
            row_id: self.row_id,
            id: self.id.clone(),
            username: self.username.clone(),
            password: self.password.clone(),
            hostname: self.hostname.clone(),
            alias: self.alias.clone(),
            note: self.note.clone(),
            platform: self.platform.clone(),
            tags: serde_json::to_value(&self.tags).unwrap_or(Value::Array(vec![])),
            hash: self.hash.clone(),
            user_id,
            force_always_relay: self.force_always_relay,
            rdp_port: self.rdp_port.clone(),
            rdp_username: self.rdp_username.clone(),
            online: self.online,
            login_name: self.login_name.clone(),
            same_server: self.same_server,
            collection_id: self.collection_id,
            created_at: None,
            updated_at: None,
        }
    }
}

pub async fn address_book_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<AddressBookQuery>,
) -> Response {
    let f = services::address_book::AbFilters {
        id: q.id,
        exact_id: q.exact_id,
        user_id: q.user_id,
        username: q.username,
        hostname: q.hostname,
        collection_id: q.collection_id,
    };
    match services::address_book::admin_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        f,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct AddressBookQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub exact_id: bool,
    #[serde(default)]
    pub user_id: Option<i32>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub collection_id: Option<i32>,
}

pub async fn address_book_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::address_book::info_by_row_id(&state.db, id).await {
        Ok(Some(ab)) => resp::success(ab),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn address_book_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<AddressBookForm>,
) -> Response {
    if f.user_id == 0 || f.id.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if f.collection_id > 0
        && !services::address_book::check_collection_owner(&state.db, f.user_id, f.collection_id)
            .await
            .unwrap_or(false)
    {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if let Ok(Some(_)) = services::address_book::info_by_user_id_and_id_and_cid(
        &state.db,
        f.user_id,
        &f.id,
        f.collection_id,
    )
    .await
    {
        return resp::fail(101, state.tr(&lang, "ItemExists"));
    }
    if let Err(e) =
        services::tag::ensure_names(&state.db, f.user_id, f.collection_id, &f.tags).await
    {
        return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e));
    }
    match services::address_book::create(&state.db, f.to_model(f.user_id)).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn address_book_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<AddressBookForm>,
) -> Response {
    if f.row_id == 0 {
        return resp::fail(101, state.tr(&lang, "ItemNotFound"));
    }
    if services::address_book::info_by_row_id(&state.db, f.row_id)
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return resp::fail(101, state.tr(&lang, "ItemNotFound"));
    }
    if let Err(e) =
        services::tag::ensure_names(&state.db, f.user_id, f.collection_id, &f.tags).await
    {
        return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e));
    }
    match services::address_book::update_all(&state.db, f.to_model(f.user_id)).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn address_book_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<AbRowIdForm>,
) -> Response {
    if f.row_id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::address_book::info_by_row_id(&state.db, f.row_id).await {
        Ok(Some(_)) => match services::address_book::delete(&state.db, f.row_id).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

#[derive(Deserialize, Default)]
pub struct AbRowIdForm {
    #[serde(default)]
    pub row_id: i32,
}

pub async fn address_book_batch_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(mut f): Json<AddressBookForm>,
) -> Response {
    if f.user_ids.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if f.user_ids.len() > 1 {
        f.tags = vec![];
        f.collection_id = 0;
    }
    for uid in f.user_ids.clone() {
        if uid == 0 {
            continue;
        }
        match services::address_book::info_by_user_id_and_id_and_cid(
            &state.db,
            uid,
            &f.id,
            f.collection_id,
        )
        .await
        {
            Ok(Some(_)) => continue,
            Ok(None) => {}
            Err(error) => {
                return resp::fail(
                    101,
                    format!("{}{}", state.tr(&lang, "OperationFailed"), error),
                )
            }
        }
        if let Err(e) = services::tag::ensure_names(&state.db, uid, f.collection_id, &f.tags).await
        {
            return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e));
        }
        if let Err(error) = services::address_book::create(&state.db, f.to_model(uid)).await {
            return resp::fail(
                101,
                format!("{}{}", state.tr(&lang, "OperationFailed"), error),
            );
        }
    }
    resp::success(Value::Null)
}

#[derive(Deserialize, Default)]
pub struct BatchCreateFromPeersForm {
    #[serde(default)]
    pub collection_id: i32,
    #[serde(default)]
    pub peer_ids: Vec<i32>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub user_id: i32,
}

pub async fn address_book_batch_create_from_peers(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<BatchCreateFromPeersForm>,
) -> Response {
    if f.user_id == 0 || f.peer_ids.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if f.collection_id != 0
        && services::address_book::collection_info_by_id(&state.db, f.collection_id)
            .await
            .ok()
            .flatten()
            .is_none()
    {
        return resp::fail(101, state.tr(&lang, "ItemNotFound"));
    }
    let peers = match services::peer::list_by_row_ids(&state.db, &f.peer_ids).await {
        Ok(peers) => peers,
        Err(error) => {
            return resp::fail(
                101,
                format!("{}{}", state.tr(&lang, "OperationFailed"), error),
            )
        }
    };
    if let Err(e) =
        services::tag::ensure_names(&state.db, f.user_id, f.collection_id, &f.tags).await
    {
        return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e));
    }
    let tags = serde_json::to_value(&f.tags).unwrap_or(Value::Array(vec![]));
    for p in peers {
        let mut ab = services::address_book::from_peer(&p);
        ab.tags = tags.clone();
        ab.collection_id = f.collection_id;
        ab.user_id = f.user_id;
        match services::address_book::info_by_user_id_and_id_and_cid(
            &state.db,
            f.user_id,
            &ab.id,
            f.collection_id,
        )
        .await
        {
            Ok(Some(_)) => continue,
            Ok(None) => {}
            Err(error) => {
                return resp::fail(
                    101,
                    format!("{}{}", state.tr(&lang, "OperationFailed"), error),
                )
            }
        }
        if let Err(error) = services::address_book::create(&state.db, ab).await {
            return resp::fail(
                101,
                format!("{}{}", state.tr(&lang, "OperationFailed"), error),
            );
        }
    }
    resp::success(Value::Null)
}

#[derive(Deserialize, Default)]
pub struct ShareByWebClientForm {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub password_type: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub expire: i64,
}

pub async fn address_book_share(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<ShareByWebClientForm>,
) -> Response {
    if f.id.is_empty() || f.password_type.is_empty() || f.password.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if services::address_book::info_by_user_id_and_id(&state.db, user.user.id, &f.id)
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return resp::fail(101, state.tr(&lang, "ItemNotFound"));
    }
    match services::share_record::share_by_web_client(
        &state.db,
        user.user.id,
        &f.id,
        &f.password_type,
        &f.password,
        f.expire,
    )
    .await
    {
        Ok(token) => resp::success(json!({ "share_token": token })),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

// ===========================================================================
// address book collections + rules (admin)
// ===========================================================================

#[derive(Deserialize, Default)]
pub struct CollectionForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub user_id: i32,
    #[serde(default)]
    pub name: String,
}

pub async fn collection_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<UserIdPageQuery>,
) -> Response {
    match services::address_book::collection_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.user_id,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn collection_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::address_book::collection_info_by_id(&state.db, id).await {
        Ok(Some(c)) => resp::success(c),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn collection_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<CollectionForm>,
) -> Response {
    if f.user_id == 0 || f.name.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::address_book::create_collection(&state.db, f.user_id, &f.name).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn collection_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<CollectionForm>,
) -> Response {
    if f.id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::address_book::update_collection(&state.db, f.id, f.user_id, &f.name).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn collection_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::address_book::collection_info_by_id(&state.db, f.id).await {
        Ok(Some(_)) => match services::address_book::delete_collection(&state.db, f.id).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

#[derive(Deserialize, Default)]
pub struct RuleForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub user_id: i32,
    #[serde(default)]
    pub collection_id: i32,
    #[serde(default)]
    pub rule: i32,
    #[serde(default, rename = "type")]
    pub r#type: i32,
    #[serde(default)]
    pub to_id: i32,
}

impl RuleForm {
    pub fn to_model(&self) -> entity::address_book_collection_rule::Model {
        entity::address_book_collection_rule::Model {
            id: self.id,
            user_id: self.user_id,
            collection_id: self.collection_id,
            rule: self.rule,
            r#type: self.r#type,
            to_id: self.to_id,
            created_at: None,
            updated_at: None,
        }
    }
}

#[derive(Deserialize, Default)]
pub struct RuleQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub user_id: Option<i32>,
    #[serde(default)]
    pub collection_id: Option<i32>,
}

pub async fn rule_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<RuleQuery>,
) -> Response {
    match services::address_book::rule_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.user_id,
        q.collection_id,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn rule_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::address_book::rule_info_by_id(&state.db, id).await {
        Ok(Some(r)) => resp::success(r),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

/// Shared validation for rule create/update (≈ `CheckForm`).
async fn check_rule(
    state: &AppState,
    t: &entity::address_book_collection_rule::Model,
) -> Result<(), String> {
    use entity::address_book_collection_rule as r;
    if t.user_id == 0 {
        return Err("ParamsError".into());
    }
    if t.collection_id > 0
        && !services::address_book::check_collection_owner(&state.db, t.user_id, t.collection_id)
            .await
            .unwrap_or(false)
    {
        return Err("ParamsError".into());
    }
    if t.r#type == r::RULE_TYPE_PERSONAL {
        if t.to_id == t.user_id {
            return Err("CannotShareToSelf".into());
        }
        if services::user::info_by_id(&state.db, t.to_id)
            .await
            .ok()
            .flatten()
            .is_none()
        {
            return Err("ItemNotFound".into());
        }
    } else if t.r#type == r::RULE_TYPE_GROUP {
        if services::group::info_by_id(&state.db, t.to_id)
            .await
            .ok()
            .flatten()
            .is_none()
        {
            return Err("ItemNotFound".into());
        }
    } else {
        return Err("ParamsError".into());
    }
    if let Ok(Some(ex)) = services::address_book::rule_info_by_type_to_cid(
        &state.db,
        t.r#type,
        t.to_id,
        t.collection_id,
    )
    .await
    {
        if t.id == 0 || t.id != ex.id {
            return Err("ItemExists".into());
        }
    }
    Ok(())
}

pub async fn rule_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<RuleForm>,
) -> Response {
    let m = f.to_model();
    if let Err(e) = check_rule(&state, &m).await {
        return resp::fail(101, state.tr(&lang, &e));
    }
    match services::address_book::create_rule(&state.db, &m).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn rule_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<RuleForm>,
) -> Response {
    if f.id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    let m = f.to_model();
    if let Err(e) = check_rule(&state, &m).await {
        return resp::fail(101, state.tr(&lang, &e));
    }
    match services::address_book::update_rule(&state.db, &m).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn rule_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::address_book::rule_info_by_id(&state.db, f.id).await {
        Ok(Some(_)) => match services::address_book::delete_rule(&state.db, f.id).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

// ===========================================================================
// rustdesk server commands
// ===========================================================================

pub async fn rustdesk_cmd_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<PageQuery>,
) -> Response {
    let r = services::server_cmd::list(&state.db, q.page.unwrap_or(1), 9999)
        .await
        .map(|r| r.list)
        .unwrap_or_default();
    let mut list = services::server_cmd::system_commands();
    list.extend(r);
    let total = list.len() as i64;
    list_json(list, 1, total, 9999)
}

#[derive(Deserialize, Default)]
pub struct ServerCmdForm {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub cmd: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub option: String,
    #[serde(default)]
    pub explain: String,
    #[serde(default)]
    pub target: String,
}

pub async fn rustdesk_cmd_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<ServerCmdForm>,
) -> Response {
    match services::server_cmd::create(
        &state.db, &f.cmd, &f.alias, &f.option, &f.explain, &f.target,
    )
    .await
    {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn rustdesk_cmd_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<ServerCmdForm>,
) -> Response {
    if f.id == 0 || f.cmd.is_empty() || f.target.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::server_cmd::update(
        &state.db, f.id, &f.cmd, &f.alias, &f.option, &f.explain, &f.target,
    )
    .await
    {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn rustdesk_cmd_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    if f.id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::server_cmd::info(&state.db, f.id).await {
        Ok(Some(_)) => match services::server_cmd::delete(&state.db, f.id).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, e.to_string()),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

#[derive(Deserialize, Default)]
pub struct SendCmdForm {
    #[serde(default)]
    pub cmd: String,
    #[serde(default)]
    pub option: String,
    #[serde(default)]
    pub target: String,
}

pub async fn rustdesk_send_cmd(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<SendCmdForm>,
) -> Response {
    use entity::server_cmd::{TARGET_ID_SERVER, TARGET_RELAY_SERVER};
    if f.cmd.is_empty() || f.target.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    let port: i32 = if f.target == TARGET_ID_SERVER {
        state.config.admin.id_server_port - 1
    } else if f.target == TARGET_RELAY_SERVER {
        state.config.admin.relay_server_port
    } else {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    };
    match services::server_cmd::send_cmd(port as u16, &f.cmd, &f.option).await {
        Ok(r) => resp::success(r),
        Err(e) => resp::fail(101, e),
    }
}

pub async fn rustdesk_status(State(state): State<AppState>, _user: AdminUser) -> Response {
    resp::success(services::server_cmd::structured_status(&state.config).await)
}

pub async fn rustdesk_relay_pool(State(state): State<AppState>, _user: AdminUser) -> Response {
    match services::server_cmd::load_relay_pool(&state.db).await {
        Ok(pool) => resp::success(pool),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct RelayServersForm {
    #[serde(default)]
    pub value: String,
}

pub async fn rustdesk_check_relay_pool(
    State(_state): State<AppState>,
    _user: AdminUser,
    Json(f): Json<RelayServersForm>,
) -> Response {
    match services::server_cmd::check_relay_pool(&f.value).await {
        Ok(checks) => resp::success(checks),
        Err(e) => resp::fail(101, e),
    }
}

pub async fn rustdesk_update_relay_servers(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(f): Json<RelayServersForm>,
) -> Response {
    match services::server_cmd::save_and_sync_relay_pool(&state.db, &state.config, &f.value).await {
        Ok((pool, response)) => resp::success(json!({ "pool": pool, "response": response })),
        Err(e) => resp::fail(101, e),
    }
}

#[derive(Deserialize, Default)]
pub struct ToggleForm {
    #[serde(default)]
    pub enabled: bool,
}

pub async fn rustdesk_update_always_use_relay(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(f): Json<ToggleForm>,
) -> Response {
    let value = if f.enabled { "Y" } else { "N" };
    match services::server_cmd::send_target_cmd(
        &state.config,
        entity::server_cmd::TARGET_ID_SERVER,
        "always-use-relay",
        value,
    )
    .await
    {
        Ok(r) => resp::success(r),
        Err(e) => resp::fail(101, e),
    }
}

pub async fn rustdesk_ip_blocker(State(state): State<AppState>, _user: AdminUser) -> Response {
    match services::server_cmd::send_target_cmd(
        &state.config,
        entity::server_cmd::TARGET_ID_SERVER,
        "ip-blocker",
        "",
    )
    .await
    {
        Ok(r) => resp::success(r),
        Err(e) => resp::fail(101, e),
    }
}

pub async fn rustdesk_geo_overview(State(state): State<AppState>, _user: AdminUser) -> Response {
    match services::geo_relay::overview(&state.db, &state.config).await {
        Ok(overview) => resp::success(overview),
        Err(error) => resp::fail(101, error),
    }
}

pub async fn rustdesk_geo_save_settings(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: AdminUser,
    Json(input): Json<services::geo_relay::GeoSettings>,
) -> Response {
    match services::geo_relay::save_and_apply(&state.db, &state.config, input).await {
        Ok(result) => {
            tracing::info!(
                admin_id = user.user.id,
                revision = result.settings.revision,
                applied = result.is_applied,
                "admin updated Geo routing settings"
            );
            resp::success(result)
        }
        Err(error) => resp::fail(101, localize_geo_error(&state.i18n, &lang, &error)),
    }
}

fn localize_geo_error(i18n: &crate::i18n::I18n, lang: &str, error: &str) -> String {
    let Some(rest) = error.strip_prefix("rule '") else {
        return error.to_owned();
    };
    let Some((rule, rest)) = rest.rsplit_once("' references relay '") else {
        return error.to_owned();
    };
    let Some(relay) = rest.strip_suffix("' outside the relay pool") else {
        return error.to_owned();
    };
    i18n.translate_params(lang, "GeoRelayOutsidePool", &[rule, relay])
}

#[cfg(test)]
mod geo_error_tests {
    use super::localize_geo_error;

    #[test]
    fn localizes_relay_pool_validation_error() {
        let i18n = crate::i18n::I18n::load("en");
        let error =
            "rule 'QA rule' references relay 'relay.example.com:21117' outside the relay pool";

        assert_eq!(
            localize_geo_error(&i18n, "zh-CN,zh;q=0.9,en;q=0.8", error),
            "规则“QA rule”引用的 Relay “relay.example.com:21117”不在服务端 Relay 池中。"
        );
        assert_eq!(
            localize_geo_error(&i18n, "en", error),
            "Rule “QA rule” references relay “relay.example.com:21117”, which is not in the server relay pool."
        );
    }

    #[test]
    fn preserves_unknown_geo_error() {
        let i18n = crate::i18n::I18n::load("en");
        let error = "enabled Geo routing requires a relay pool";

        assert_eq!(localize_geo_error(&i18n, "zh-CN", error), error);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MmdbDownloadForm {
    pub source_url: String,
}

pub async fn rustdesk_geo_download_database(
    State(state): State<AppState>,
    user: AdminUser,
    Path(kind): Path<String>,
    Json(input): Json<MmdbDownloadForm>,
) -> Response {
    let Some(kind) = services::geo_relay::MmdbKind::parse(&kind) else {
        return resp::fail(101, "database kind must be country, city, or asn");
    };
    match services::geo_relay::download_database(
        &state.db,
        &state.config,
        kind,
        &input.source_url,
    )
    .await
    {
        Ok((database, is_reloaded, reload_message, is_source_saved, source_message)) => {
            tracing::info!(
                admin_id = user.user.id,
                database_kind = ?kind,
                reloaded = is_reloaded,
                "admin downloaded a Geo database"
            );
            resp::success(json!({
                "database": database,
                "isReloaded": is_reloaded,
                "reloadMessage": reload_message,
                "isSourceSaved": is_source_saved,
                "sourceMessage": source_message,
            }))
        }
        Err(error) => resp::fail(101, error),
    }
}

pub async fn rustdesk_geo_restore_database(
    State(state): State<AppState>,
    user: AdminUser,
    Path(kind): Path<String>,
) -> Response {
    let Some(kind) = services::geo_relay::MmdbKind::parse(&kind) else {
        return resp::fail(101, "database kind must be country, city, or asn");
    };
    match services::geo_relay::restore_database(&state.config, kind).await {
        Ok((database, is_reloaded, reload_message)) => {
            tracing::info!(
                admin_id = user.user.id,
                database_kind = ?kind,
                reloaded = is_reloaded,
                "admin restored a Geo database backup"
            );
            resp::success(json!({
                "database": database,
                "isReloaded": is_reloaded,
                "reloadMessage": reload_message,
            }))
        }
        Err(error) => resp::fail(101, error),
    }
}

pub async fn rustdesk_geo_save_update_policy(
    State(state): State<AppState>,
    user: AdminUser,
    Json(input): Json<services::geo_relay::MmdbUpdatePolicy>,
) -> Response {
    match services::geo_relay::save_update_policy(&state.db, input).await {
        Ok(policy) => {
            tracing::info!(
                admin_id = user.user.id,
                enabled = policy.enabled,
                interval_hours = policy.interval_hours,
                "admin updated the automatic MMDB policy"
            );
            resp::success(policy)
        }
        Err(error) => resp::fail(101, error),
    }
}

pub async fn rustdesk_geo_reload(State(state): State<AppState>, user: AdminUser) -> Response {
    match services::geo_relay::apply_persisted(&state.db, &state.config).await {
        Ok(message) => {
            tracing::info!(admin_id = user.user.id, "admin reloaded Geo routing settings");
            resp::success(json!({ "isApplied": true, "message": message }))
        }
        Err(error) => resp::success(json!({ "isApplied": false, "message": error })),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoTestForm {
    pub client_a: String,
    pub client_b: Option<String>,
}

impl GeoTestForm {
    fn addresses(&self) -> Result<(std::net::IpAddr, Option<std::net::IpAddr>), &'static str> {
        let a = self
            .client_a
            .trim()
            .parse()
            .map_err(|_| "clientA must be a valid IP address")?;
        let b = self
            .client_b
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::parse)
            .transpose()
            .map_err(|_| "clientB must be a valid IP address")?;
        Ok((a, b))
    }
}

pub async fn rustdesk_geo_test(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(input): Json<GeoTestForm>,
) -> Response {
    let (client_a, client_b) = match input.addresses() {
        Ok(addresses) => addresses,
        Err(error) => return resp::fail(101, error),
    };
    match services::geo_relay::test_rule(&state.config, client_a, client_b).await {
        Ok(raw) => resp::success(json!({ "raw": raw })),
        Err(error) => resp::fail(101, error),
    }
}

pub async fn rustdesk_geo_preview(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(input): Json<GeoTestForm>,
) -> Response {
    let (client_a, client_b) = match input.addresses() {
        Ok(addresses) => addresses,
        Err(error) => return resp::fail(101, error),
    };
    match services::geo_relay::preview_rule(&state.config, client_a, client_b).await {
        Ok(preview) => resp::no_store(resp::success(json!({ "preview": preview }))),
        Err(error) => resp::fail(101, error),
    }
}

// ===========================================================================
// login log deletes
// ===========================================================================

pub async fn login_log_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::login_log::info_by_id(&state.db, f.id).await {
        Ok(Some(_)) => match services::login_log::delete(&state.db, f.id).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, e.to_string()),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn login_log_batch_delete(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(f): Json<IdsForm>,
) -> Response {
    match services::login_log::batch_delete(&state.db, &f.ids).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

// ===========================================================================
// peer simpleData + create
// ===========================================================================

pub async fn peer_simple_data(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: BackendUser,
    Json(f): Json<SimpleDataForm>,
) -> Response {
    if f.ids.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::peer::simple_data(&state.db, &f.ids).await {
        Ok(list) => {
            let total = list.len() as i64;
            resp::success(json!({ "list": list, "total": total }))
        }
        Err(e) => resp::fail(101, e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct SimpleDataForm {
    #[serde(default)]
    pub ids: Vec<String>,
}

pub async fn peer_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<PeerCreateForm>,
) -> Response {
    let am = peer::ActiveModel {
        id: Set(f.id),
        cpu: Set(f.cpu),
        hostname: Set(f.hostname),
        memory: Set(f.memory),
        os: Set(f.os),
        username: Set(f.username),
        uuid: Set(f.uuid),
        version: Set(f.version),
        group_id: Set(f.group_id),
        alias: Set(f.alias),
        ..Default::default()
    };
    match services::peer::create(&state.db, am).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

#[derive(Deserialize, Default)]
pub struct PeerCreateForm {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub cpu: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub memory: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub group_id: i32,
    #[serde(default)]
    pub alias: String,
}

// ===========================================================================
// user register
// ===========================================================================

#[derive(Deserialize, Default)]
pub struct RegisterForm {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub confirm_password: String,
}

pub async fn user_register(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    ClientIp(ip): ClientIp,
    audit: AuditContext,
    Json(f): Json<RegisterForm>,
) -> Response {
    if !services::login_security::registration_enabled(&state.db, state.config.app.register)
        .await
        .unwrap_or(false)
    {
        return resp::fail(101, state.tr(&lang, "RegisterClosed"));
    }
    if f.username.len() < 2 || f.password.len() < 4 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    let mut status = state.config.app.register_status;
    if status != user::STATUS_DISABLED && status != user::STATUS_ENABLE {
        status = user::STATUS_ENABLE;
    }
    let u = services::user::register(&state.db, &f.username, &f.email, &f.password, status).await;
    let Some(u) = u.filter(|u| u.id != 0) else {
        return resp::fail(101, state.tr(&lang, "OperationFailed"));
    };
    if status == user::STATUS_DISABLED {
        return resp::fail(101, state.tr(&lang, "RegisterSuccessWaitAdminConfirm"));
    }
    let proof = match console_login_proof(&state, &u).await {
        Ok(proof) => proof,
        Err(message) => return resp::fail(101, message),
    };
    let ut = match services::user::login(
        &state.db,
        &state.jwt,
        &state.config,
        &u,
        services::user::LoginEvent {
            proof,
            client: entity::login_log::CLIENT_WEB_ADMIN.to_string(),
            device_id: String::new(),
            uuid: String::new(),
            ip,
            login_type: entity::login_log::TYPE_ACCOUNT.to_string(),
            platform: String::new(),
        },
    )
    .await
    {
        Ok(ut) => ut,
        Err(e) => return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    };
    console_session_response(&audit, "registration", &u, ut)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, ConnectionTrait, Database, DbBackend, Schema, Set, Statement};

    async fn failed_write_state() -> (AppState, entity::user::Model, entity::user_token::Model) {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for statement in [
            schema.create_table_from_entity(entity::user::Entity),
            schema.create_table_from_entity(entity::user_token::Entity),
            schema.create_table_from_entity(entity::trusted_browser::Entity),
            schema.create_table_from_entity(entity::peer::Entity),
            schema.create_table_from_entity(entity::address_book::Entity),
        ] {
            db.execute(db.get_database_backend().build(&statement))
                .await
                .unwrap();
        }
        let user = entity::user::ActiveModel {
            username: Set("write-test".to_string()),
            is_admin: Set(Some(true)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let token = entity::user_token::ActiveModel {
            user_id: Set(user.id),
            token: Set("test-token".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        entity::peer::ActiveModel {
            row_id: Set(1),
            user_id: Set(user.id),
            id: Set("123456".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        for sql in [
            "CREATE TRIGGER reject_ab_insert BEFORE INSERT ON address_books BEGIN SELECT RAISE(FAIL, 'test write failure'); END",
            "CREATE TRIGGER reject_token_delete BEFORE DELETE ON user_tokens BEGIN SELECT RAISE(FAIL, 'test write failure'); END",
        ] {
            db.execute(Statement::from_string(DbBackend::Sqlite, sql)).await.unwrap();
        }
        (
            crate::http::api::deployment_tests::test_state(db),
            user,
            token,
        )
    }

    async fn response_code(response: Response) -> i64 {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice::<Value>(&bytes).unwrap()["code"]
            .as_i64()
            .unwrap()
    }

    #[tokio::test]
    async fn logout_reports_token_revocation_failure() {
        let (state, user, token) = failed_write_state().await;
        let response = logout(
            State(state),
            BackendUser {
                user,
                token: token.token.clone(),
                user_token: token,
            },
        )
        .await;
        assert_eq!(response_code(response).await, 101);
    }

    #[tokio::test]
    async fn address_book_batch_handlers_report_failed_inserts() {
        let (state, user, token) = failed_write_state().await;
        let admin = || AdminUser {
            auth_level: AUTH_TOTP,
            user: user.clone(),
            token: token.token.clone(),
        };
        let response = address_book_batch_create(
            State(state.clone()),
            AcceptLang(String::new()),
            admin(),
            Json(serde_json::from_value(json!({"id":"123456", "user_ids":[user.id]})).unwrap()),
        )
        .await;
        assert_eq!(response_code(response).await, 101);
        let response = address_book_batch_create_from_peers(
            State(state.clone()),
            AcceptLang(String::new()),
            admin(),
            Json(BatchCreateFromPeersForm {
                user_id: user.id,
                peer_ids: vec![1],
                ..Default::default()
            }),
        )
        .await;
        assert_eq!(response_code(response).await, 101);
        let response = crate::http::my::address_book_batch_create_from_peers(
            State(state),
            AcceptLang(String::new()),
            BackendUser {
                user,
                token: token.token.clone(),
                user_token: token,
            },
            Json(crate::http::my::MyBatchFromPeersForm {
                peer_ids: vec![1],
                ..Default::default()
            }),
        )
        .await;
        assert_eq!(response_code(response).await, 101);
    }

    #[test]
    fn peer_form_keeps_omitted_fields_unset() {
        // What the device edit form in the web UI actually submits.
        let f: PeerForm = serde_json::from_value(serde_json::json!({
            "row_id": 1,
            "alias": "pc-work",
            "group_id": 0,
            "user_id": 0
        }))
        .unwrap();
        assert_eq!(f.row_id, 1);
        assert_eq!(f.id, None, "omitted id must not become an empty string");
        assert_eq!(f.alias.as_deref(), Some("pc-work"));
        assert_eq!(f.group_id, Some(0));
        assert_eq!(f.user_id, Some(0));
    }

    #[test]
    fn peer_form_accepts_explicit_id() {
        let f: PeerForm =
            serde_json::from_value(serde_json::json!({"row_id": 2, "id": "1160644425"})).unwrap();
        assert_eq!(f.id.as_deref(), Some("1160644425"));
        assert_eq!(f.alias, None);
    }
}

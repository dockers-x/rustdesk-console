//! Admin panel API (`/api/admin/*`), ports of `http/controller/admin/*.go`.
//! Phase 1 implements auth, config and full CRUD for user/group/tag/peer/
//! device_group. Remaining admin areas return a clear "not implemented" code.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::Json;
use sea_orm::Set;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use entity::{group, peer, user};

use crate::http::middleware::{AcceptLang, AdminUser, BackendUser, ClientIp};
use crate::http::payloads::AdminLoginPayload;
use crate::http::response as resp;
use crate::services;
use crate::state::AppState;
use crate::support::admin_config::AdminPanelConfig;
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
    #[serde(default)]
    pub secret: String,
}

pub async fn login(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    ClientIp(ip): ClientIp,
    Json(f): Json<LoginForm>,
) -> Response {
    if state.config.app.disable_pwd_login {
        return resp::fail(101, state.tr(&lang, "PwdLoginDisabled"));
    }
    if !f.secret.trim().is_empty() {
        let (user, device) = match services::login_security::verify_login_challenge(
            &state.db,
            &f.secret,
            Some(&f.verification_code),
            Some(&f.tfa_code),
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                state.limiter.record_failed_attempt(&ip);
                return resp::fail(101, e);
            }
        };
        if !user.is_enabled() {
            return resp::fail(101, state.tr(&lang, "UserDisabled"));
        }
        let ut = match services::user::login(
            &state.db,
            &state.jwt,
            &state.config,
            &user,
            services::user::LoginEvent {
                client: entity::login_log::CLIENT_WEB_ADMIN.to_string(),
                device_id: device.id,
                uuid: device.uuid,
                ip: ip.clone(),
                login_type: entity::login_log::TYPE_ACCOUNT.to_string(),
                platform: device.os,
            },
        )
        .await
        {
            Ok(ut) => ut,
            Err(e) => {
                return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e))
            }
        };
        state.limiter.remove_attempts(&ip);
        return resp::success(AdminLoginPayload::from_user(&user, ut.token));
    }
    let (_banned, need_captcha) = state.limiter.check_security_status(&ip);

    if f.username.len() < 2 || f.password.len() < 4 {
        state.limiter.record_failed_attempt(&ip);
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if need_captcha
        && (f.captcha_id.is_empty()
            || f.captcha.is_empty()
            || !state.limiter.verify_captcha(&f.captcha_id, &f.captcha))
    {
        return resp::fail(101, state.tr(&lang, "CaptchaError"));
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
        _ => {
            state.limiter.record_failed_attempt(&ip);
            let (_b, nc) = state.limiter.check_security_status(&ip);
            let code = if nc { 110 } else { 101 };
            return resp::fail(code, state.tr(&lang, "UsernameOrPasswordError"));
        }
    };
    if !user.is_enabled() {
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
    match services::login_security::required_verification(&state.db, &user, &device).await {
        Ok(Some(kind)) => {
            let challenge = match services::login_security::create_login_challenge(
                &state.db, &user, &device, kind,
            )
            .await
            {
                Ok(challenge) => challenge,
                Err(e) => return resp::fail(101, e),
            };
            return resp::success(json!({
                "type": "email_check",
                "tfa_type": challenge.kind.response_tfa_type(),
                "secret": challenge.secret,
                "user": {
                    "username": user.username,
                    "email": user.email,
                }
            }));
        }
        Ok(None) => {}
        Err(e) => return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
    let ut = match services::user::login(
        &state.db,
        &state.jwt,
        &state.config,
        &user,
        services::user::LoginEvent {
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
        Err(e) => return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    };
    state.limiter.remove_attempts(&ip);
    resp::success(AdminLoginPayload::from_user(&user, ut.token))
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
    let _ = services::user::logout(&state.db, user.user.id, &user.token).await;
    resp::success(Value::Null)
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
    let ops: Vec<String> = services::oauth::get_login_providers(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|provider| provider.name)
        .collect();
    resp::success(json!({
        "ops": ops,
        "register": state.config.app.register,
        "need_captcha": need_captcha,
        "disable_pwd": state.config.app.disable_pwd_login,
        "auto_oidc": state.config.app.disable_pwd_login && ops.len() == 1,
    }))
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
    let ut = match services::user::login(
        &state.db,
        &state.jwt,
        &state.config,
        &user,
        services::user::LoginEvent {
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
    resp::success(AdminLoginPayload::from_user(&user, ut.token))
}

// ---------- config ----------

pub async fn config_admin(State(state): State<AppState>, user: Option<BackendUser>) -> Response {
    let timezone = std::env::var("TZ").unwrap_or_default();
    let Some(user) = user else {
        let cfg = state.admin_config.view(None).await;
        return resp::success(json!({
            "title": cfg.title,
            "timezone": timezone,
        }));
    };
    let cfg = state.admin_config.view(Some(&user.user.username)).await;
    resp::success(json!({
        "title": cfg.title,
        "hello": cfg.hello,
        "timezone": timezone,
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
    match services::login_security::config_view(&state.db).await {
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
    match services::login_security::config_view(&state.db).await {
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
            tls: self.tls,
        }
    }
}

fn validate_smtp_form(lang: &str, state: &AppState, f: &SmtpEmailConfigForm) -> Option<Response> {
    let name = f.name.trim();
    let host = f.host.trim();
    let from = f.from.trim();
    if name.chars().count() > 80
        || host.is_empty()
        || host.chars().count() > 255
        || f.username.chars().count() > 255
        || f.password.chars().count() > 500
        || from.is_empty()
        || from.chars().count() > 255
        || !from.contains('@')
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
    match services::login_security::send_test_email(
        &state.db,
        config_id,
        to,
        "RustDesk Console test email",
        "This is a RustDesk Console SMTP configuration test email.",
    )
    .await
    {
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
    pub remark: String,
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
            remark: self.remark.clone(),
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
    match services::user::list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.username,
    )
    .await
    {
        Ok(r) => {
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
    Json(f): Json<UserForm>,
) -> Response {
    if f.id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::user::update(&state.db, &f.to_model()).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn user_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
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
}

pub async fn user_change_pwd(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<UserPasswordForm>,
) -> Response {
    if f.password.len() < 4 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::user::info_by_id(&state.db, f.id).await {
        Ok(Some(u)) => match services::user::update_password(&state.db, &u, &f.password).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn user_current(State(_state): State<AppState>, user: BackendUser) -> Response {
    resp::success(AdminLoginPayload::from_user(&user.user, user.token))
}

pub async fn my_security(State(state): State<AppState>, user: BackendUser) -> Response {
    let trusted_devices =
        services::login_security::trusted_devices_for_user(&state.db, user.user.id)
            .await
            .unwrap_or_default();
    resp::success(json!({
        "tfa_enabled": user.user.tfa_enabled,
        "tfa_enforced": user.user.tfa_enforced,
        "email_verification_enabled": user.user.email_verification_enabled,
        "login_device_verification_enabled": user.user.login_device_verification_enabled,
        "trusted_device_count": trusted_devices.len(),
    }))
}

pub async fn my_tfa_setup(State(_state): State<AppState>, user: BackendUser) -> Response {
    let secret = services::login_security::generate_totp_secret();
    resp::success(json!({
        "secret": secret,
        "uri": services::login_security::totp_uri(&user.user.username, &secret),
    }))
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
    Json(f): Json<TfaCodeForm>,
) -> Response {
    if f.secret.trim().is_empty() || f.code.trim().is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::login_security::enable_user_totp(
        &state.db,
        &user.user,
        f.secret.trim(),
        f.code.trim(),
    )
    .await
    {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e),
    }
}

pub async fn my_tfa_disable(
    State(state): State<AppState>,
    user: BackendUser,
    Json(f): Json<TfaCodeForm>,
) -> Response {
    match services::login_security::disable_user_totp(&state.db, &user.user, Some(&f.code)).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e),
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
    Json(f): Json<UserSecurityForm>,
) -> Response {
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
    _user: AdminUser,
    Json(f): Json<IdForm>,
) -> Response {
    let target = match services::user::info_by_id(&state.db, f.id).await {
        Ok(Some(user)) => user,
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    };
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
    let target_id = value_as_string(f.target_id).unwrap_or_default();
    match services::strategy::assign(
        &state.db,
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
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
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
    user: AdminUser,
    Json(f): Json<StrategyAssignForm>,
) -> Response {
    if f.id > 0 {
        if let Err(e) = services::strategy::delete_assignment(&state.db, f.id).await {
            return resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e));
        }
    }
    strategy_assign(State(state), AcceptLang(lang), user, Json(f)).await
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
    match services::tag::list(&state.db, q.page.unwrap_or(0), q.page_size.unwrap_or(0)).await {
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
    _user: AdminUser,
    Query(q): Query<PeerQuery>,
) -> Response {
    let f = services::peer::PeerFilters {
        user_id: None,
        time_ago: q.time_ago,
        id: q.id,
        hostname: q.hostname,
        username: q.username,
        ip: q.ip,
        alias: q.alias,
    };
    match services::peer::list_filtered(&state.db, q.page.unwrap_or(0), q.page_size.unwrap_or(0), f)
        .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn peer_detail(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match services::peer::info_by_row_id(&state.db, id).await {
        Ok(Some(p)) => resp::success(p),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

#[derive(Deserialize, Default)]
pub struct PeerForm {
    #[serde(default)]
    pub row_id: i32,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub group_id: i32,
    #[serde(default)]
    pub user_id: i32,
}

pub async fn peer_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<PeerForm>,
) -> Response {
    if f.row_id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    let am = peer::ActiveModel {
        row_id: Set(f.row_id),
        id: Set(f.id),
        alias: Set(f.alias),
        group_id: Set(f.group_id),
        user_id: Set(f.user_id),
        ..Default::default()
    };
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
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
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
    _user: AdminUser,
    Query(q): Query<PageQuery>,
) -> Response {
    match services::login_log::list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        None,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
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
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
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
            Err(e) => resp::fail(101, e.to_string()),
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

/// Replaces the earlier stub: real list of providers + bind status.
pub async fn user_my_oauth_real(State(state): State<AppState>, user: BackendUser) -> Response {
    let providers = services::oauth::get_login_provider_configs(&state.db)
        .await
        .unwrap_or_default();
    let thirds = services::oauth::user_thirds_by_user_id(&state.db, user.user.id)
        .await
        .unwrap_or_default();
    let bound: std::collections::HashMap<String, entity::user_third::Model> =
        thirds.into_iter().map(|t| (t.op.clone(), t)).collect();
    let res: Vec<Value> = providers
        .into_iter()
        .map(|o| {
            let op = o.op;
            let binding = bound.get(&op);
            json!({
                "op": op,
                "oauth_type": o.oauth_type,
                "status": if binding.is_some() { 1 } else { 0 },
                "name": binding.map(|t| t.name.as_str()).unwrap_or(""),
                "username": binding.map(|t| t.username.as_str()).unwrap_or(""),
                "email": binding.map(|t| t.email.as_str()).unwrap_or(""),
                "verified_email": binding.map(|t| t.verified_email).unwrap_or(false),
                "picture": binding.map(|t| t.picture.as_str()).unwrap_or(""),
                "created_at": binding
                    .and_then(|t| t.created_at.map(|dt| dt.to_string()))
                    .unwrap_or_default(),
            })
        })
        .collect();
    resp::success(res)
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
    Json(f): Json<UnbindForm>,
) -> Response {
    match services::oauth::user_third_by_user_and_op(&state.db, user.user.id, &f.op).await {
        Ok(Some(_)) => match services::oauth::unbind(&state.db, user.user.id, &f.op).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

// ===========================================================================
// audit logs
// ===========================================================================

#[derive(Deserialize, Default)]
pub struct AuditQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub peer_id: Option<String>,
    #[serde(default)]
    pub from_peer: Option<String>,
}

pub async fn audit_conn_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<AuditQuery>,
) -> Response {
    match services::audit::conn_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.peer_id,
        q.from_peer,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
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
    _user: AdminUser,
    Query(q): Query<AuditQuery>,
) -> Response {
    match services::audit::file_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.peer_id,
        q.from_peer,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
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
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
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
    let storage_cfg = state.record_storage_config.get().await;
    if let Err(e) = services::record_storage::delete_object(
        &storage_cfg,
        &state.config.gin.resources_path,
        &row.storage_backend,
        &row.storage_key,
        &row.filename,
    )
    .await
    {
        tracing::warn!("failed to remove record object: {e}");
    }
    if let Err(e) = services::record_storage::cleanup_staging(
        &storage_cfg,
        &state.config.gin.resources_path,
        &row.storage_backend,
        &row.filename,
    )
    .await
    {
        tracing::warn!("failed to remove record staging file: {e}");
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
    let storage_cfg = state.record_storage_config.get().await;
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
        if let Ok(Some(_)) = services::address_book::info_by_user_id_and_id_and_cid(
            &state.db,
            uid,
            &f.id,
            f.collection_id,
        )
        .await
        {
            continue;
        }
        let _ = services::address_book::create(&state.db, f.to_model(uid)).await;
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
    let peers = services::peer::list_by_row_ids(&state.db, &f.peer_ids)
        .await
        .unwrap_or_default();
    let tags = serde_json::to_value(&f.tags).unwrap_or(Value::Array(vec![]));
    for p in peers {
        let mut ab = services::address_book::from_peer(&p);
        ab.tags = tags.clone();
        ab.collection_id = f.collection_id;
        ab.user_id = f.user_id;
        if let Ok(Some(_)) = services::address_book::info_by_user_id_and_id_and_cid(
            &state.db,
            f.user_id,
            &ab.id,
            f.collection_id,
        )
        .await
        {
            continue;
        }
        let _ = services::address_book::create(&state.db, ab).await;
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

#[derive(Deserialize, Default)]
pub struct RelayServersForm {
    #[serde(default)]
    pub value: String,
}

pub async fn rustdesk_update_relay_servers(
    State(state): State<AppState>,
    _user: AdminUser,
    Json(f): Json<RelayServersForm>,
) -> Response {
    match services::server_cmd::send_target_cmd(
        &state.config,
        entity::server_cmd::TARGET_ID_SERVER,
        "relay-servers",
        &f.value,
    )
    .await
    {
        Ok(r) => resp::success(r),
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
    Json(f): Json<RegisterForm>,
) -> Response {
    if !state.config.app.register {
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
    let ut = match services::user::login(
        &state.db,
        &state.jwt,
        &state.config,
        &u,
        services::user::LoginEvent {
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
    resp::success(AdminLoginPayload::from_user(&u, ut.token))
}

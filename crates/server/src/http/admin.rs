//! Admin panel API (`/api/admin/*`), ports of `http/controller/admin/*.go`.
//! Phase 1 implements auth, config and full CRUD for user/group/tag/peer/
//! device_group. Remaining admin areas return a clear "not implemented" code.

use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Json;
use sea_orm::Set;
use serde::Deserialize;
use serde_json::{json, Value};

use entity::{group, peer, user};

use crate::http::middleware::{AcceptLang, AdminUser, BackendUser, ClientIp};
use crate::http::payloads::AdminLoginPayload;
use crate::http::response as resp;
use crate::services;
use crate::state::AppState;

fn list_json<T: serde::Serialize>(list: Vec<T>, page: i64, total: i64, page_size: i64) -> Response {
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

    let user = match services::user::info_by_username_password(&state.db, &f.username, &f.password)
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

pub async fn captcha(State(state): State<AppState>, ClientIp(ip): ClientIp, AcceptLang(lang): AcceptLang) -> Response {
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

pub async fn login_options(State(state): State<AppState>, ClientIp(ip): ClientIp, AcceptLang(lang): AcceptLang) -> Response {
    let (banned, need_captcha) = state.limiter.check_security_status(&ip);
    if banned {
        return resp::fail(101, state.tr(&lang, "LoginBanned"));
    }
    let ops: Vec<String> = vec![]; // OAuth providers wired in a later phase
    resp::success(json!({
        "ops": ops,
        "register": state.config.app.register,
        "need_captcha": need_captcha,
        "disable_pwd": state.config.app.disable_pwd_login,
        "auto_oidc": false,
    }))
}

// ---------- config ----------

pub async fn config_admin(State(state): State<AppState>, user: Option<BackendUser>) -> Response {
    let Some(user) = user else {
        return resp::success(json!({ "title": state.config.admin.title }));
    };
    let mut hello = state.config.admin.hello.clone();
    if hello.is_empty() && !state.config.admin.hello_file.is_empty() {
        if let Ok(b) = std::fs::read_to_string(&state.config.admin.hello_file) {
            if !b.is_empty() {
                hello = b;
            }
        }
    }
    hello = hello.replace("{{username}}", &user.user.username);
    resp::success(json!({ "title": state.config.admin.title, "hello": hello }))
}

pub async fn config_server(State(state): State<AppState>, _user: BackendUser) -> Response {
    resp::success(json!({
        "id_server": state.config.rustdesk.id_server,
        "key": state.config.rustdesk.key,
        "relay_server": state.config.rustdesk.relay_server,
        "api_server": state.config.rustdesk.api_server,
    }))
}

pub async fn config_app(State(state): State<AppState>, _user: BackendUser) -> Response {
    resp::success(json!({ "web_client": state.config.app.web_client }))
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
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
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
        let (ok, _) = crate::support::password::verify_password(&user.user.password, &f.old_password);
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
    let t = if f.r#type == 0 { group::TYPE_DEFAULT } else { f.r#type };
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
    match services::group::device_group_list(&state.db, q.page.unwrap_or(0), q.page_size.unwrap_or(0))
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

pub async fn peer_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<PageQuery>,
) -> Response {
    match services::peer::list(&state.db, q.page.unwrap_or(0), q.page_size.unwrap_or(0), q.id).await
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
    let am = peer::ActiveModel {
        row_id: Set(f.row_id),
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
    Json(f): Json<IdForm>,
) -> Response {
    match services::peer::info_by_row_id(&state.db, f.id).await {
        Ok(Some(p)) => match services::peer::delete(&state.db, &p).await {
            Ok(_) => resp::success(Value::Null),
            Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
        },
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn peer_batch_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: AdminUser,
    Json(f): Json<IdsForm>,
) -> Response {
    match services::peer::batch_delete(&state.db, &f.ids).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

// ---------- login log (read) ----------

pub async fn login_log_list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(q): Query<PageQuery>,
) -> Response {
    match services::login_log::list(&state.db, q.page.unwrap_or(0), q.page_size.unwrap_or(0), None)
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

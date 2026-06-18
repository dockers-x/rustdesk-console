//! Stable RustDesk Pro-compatible facade for official `res/*.py` scripts.
//! These routes are adapters over the console-owned Fleet/Policy/Evidence
//! models; they do not write hbbs registration state.

use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{NaiveDateTime, TimeZone, Utc};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use ::entity::{device_group, group, peer, strategy, strategy_assignment, user};

use crate::http::middleware::RustClientUser;
use crate::http::response as resp;
use crate::services;
use crate::state::AppState;

#[derive(Deserialize, Default)]
pub struct ProPageQuery {
    #[serde(default)]
    #[serde(alias = "current")]
    pub page: Option<u64>,
    #[serde(default, rename = "pageSize")]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub device_name: Option<String>,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub group_name: Option<String>,
    #[serde(default)]
    pub device_group_name: Option<String>,
    #[serde(default)]
    pub device_username: Option<String>,
    #[serde(default)]
    pub remote: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub operator: Option<String>,
    #[serde(default)]
    pub conn_type: Option<i32>,
}

#[derive(Debug, Serialize)]
struct ProDevice {
    guid: String,
    id: String,
    device_name: String,
    user_name: String,
    group_name: String,
    device_group_name: String,
    device_username: String,
    last_online: String,
    status: i32,
}

#[derive(Debug, Serialize)]
struct ProGroup {
    guid: String,
    name: String,
    note: String,
    allowed_incomings: Value,
    allowed_outgoings: Value,
}

#[derive(Debug, Serialize)]
struct ProUser {
    guid: String,
    name: String,
    email: String,
    note: String,
    group_name: String,
    status: i32,
}

pub async fn devices(
    State(state): State<AppState>,
    user: RustClientUser,
    Query(q): Query<ProPageQuery>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(30).max(1);
    let peers = peer::Entity::find()
        .order_by_desc(peer::Column::RowId)
        .all(&state.db)
        .await
        .unwrap_or_default();
    let users = user::Entity::find()
        .all(&state.db)
        .await
        .unwrap_or_default();
    let groups = group::Entity::find()
        .all(&state.db)
        .await
        .unwrap_or_default();
    let dgroups = device_group::Entity::find()
        .all(&state.db)
        .await
        .unwrap_or_default();
    let user_names: std::collections::HashMap<i32, String> =
        users.iter().map(|u| (u.id, u.username.clone())).collect();
    let user_group_ids: std::collections::HashMap<i32, i32> =
        users.into_iter().map(|u| (u.id, u.group_id)).collect();
    let group_names: std::collections::HashMap<i32, String> =
        groups.into_iter().map(|g| (g.id, g.name)).collect();
    let dgroup_names: std::collections::HashMap<i32, String> =
        dgroups.into_iter().map(|g| (g.id, g.name)).collect();

    let filtered: Vec<ProDevice> = peers
        .into_iter()
        .map(|p| {
            let user_name = user_names.get(&p.user_id).cloned().unwrap_or_default();
            let group_name = user_group_ids
                .get(&p.user_id)
                .and_then(|group_id| group_names.get(group_id))
                .cloned()
                .unwrap_or_default();
            let device_group_name = dgroup_names.get(&p.group_id).cloned().unwrap_or_default();
            ProDevice {
                guid: peer_guid(&p),
                id: p.id,
                device_name: p.hostname,
                user_name,
                group_name,
                device_group_name,
                device_username: p.username,
                last_online: iso_from_unix(p.last_online_time),
                status: p.status,
            }
        })
        .filter(|d| matches_like(&d.id, q.id.as_deref()))
        .filter(|d| matches_like(&d.device_name, q.device_name.as_deref()))
        .filter(|d| matches_like(&d.user_name, q.user_name.as_deref()))
        .filter(|d| matches_like(&d.group_name, q.group_name.as_deref()))
        .filter(|d| matches_like(&d.device_group_name, q.device_group_name.as_deref()))
        .filter(|d| matches_like(&d.device_username, q.device_username.as_deref()))
        .collect();
    let total = filtered.len() as i64;
    let start = ((page - 1) * page_size) as usize;
    let rows: Vec<ProDevice> = filtered
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect();
    resp::data_response(total, rows)
}

pub async fn device_disable(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
) -> Response {
    set_device_status(state, user, guid, user::STATUS_DISABLED).await
}

pub async fn device_enable(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
) -> Response {
    set_device_status(state, user, guid, user::STATUS_ENABLE).await
}

pub async fn device_delete(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let Some(p) = find_peer_by_guidish(&state.db, &guid).await else {
        return resp::error("device not found");
    };
    match services::peer::delete(&state.db, &p).await {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct DeviceAssignForm {
    #[serde(default, rename = "type")]
    pub assign_type: String,
    #[serde(default)]
    pub value: String,
}

pub async fn device_assign(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(f): Json<DeviceAssignForm>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let Some(p) = find_peer_by_guidish(&state.db, &guid).await else {
        return resp::error("device not found");
    };
    match assign_device_value(&state, p, &f.assign_type, &f.value).await {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => resp::error(e),
    }
}

pub async fn device_groups(
    State(state): State<AppState>,
    user: RustClientUser,
    Query(q): Query<ProPageQuery>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).max(1);
    let mut rows: Vec<ProGroup> = device_group::Entity::find()
        .order_by_desc(device_group::Column::Id)
        .all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|g| ProGroup {
            guid: g.id.to_string(),
            name: g.name,
            note: String::new(),
            allowed_incomings: Value::Array(vec![]),
            allowed_outgoings: Value::Array(vec![]),
        })
        .filter(|g| matches_like(&g.name, q.name.as_deref()))
        .collect();
    let total = rows.len() as i64;
    let start = ((page - 1) * page_size) as usize;
    rows = rows
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect();
    resp::data_response(total, rows)
}

#[derive(Deserialize, Default)]
pub struct GroupForm {
    #[serde(default)]
    pub name: String,
}

pub async fn device_group_create(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<GroupForm>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    match services::group::device_group_create(&state.db, &f.name).await {
        Ok(row) => Json(json!({ "guid": row.id.to_string(), "name": row.name })).into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

pub async fn device_group_update(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(f): Json<GroupForm>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let Some(row) = find_device_group_by_guidish(&state.db, &guid).await else {
        return resp::error("device group not found");
    };
    match services::group::device_group_update(&state.db, row.id, f.name).await {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

pub async fn device_group_delete(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let Some(row) = find_device_group_by_guidish(&state.db, &guid).await else {
        return resp::error("device group not found");
    };
    match services::group::device_group_delete(&state.db, row.id).await {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

pub async fn device_group_add_devices(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(ids): Json<Vec<String>>,
) -> Response {
    set_devices_group(state, user, guid, ids, true).await
}

pub async fn device_group_remove_devices(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(ids): Json<Vec<String>>,
) -> Response {
    set_devices_group(state, user, guid, ids, false).await
}

pub async fn strategies(State(state): State<AppState>, user: RustClientUser) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let rows = strategy::Entity::find()
        .order_by_desc(strategy::Column::Id)
        .all(&state.db)
        .await
        .unwrap_or_default();
    Json(rows).into_response()
}

pub async fn strategy_detail(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    match services::strategy::info_by_guid(&state.db, &guid).await {
        Ok(Some(row)) => Json(row).into_response(),
        _ => resp::error("strategy not found"),
    }
}

pub async fn strategy_status(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(enabled): Json<bool>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let row = match services::strategy::info_by_guid(&state.db, &guid).await {
        Ok(Some(row)) => row,
        _ => return resp::error("strategy not found"),
    };
    match services::strategy::set_status(&state.db, row, enabled).await {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

#[derive(Deserialize, Default)]
pub struct StrategyAssignPayload {
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub peers: Vec<String>,
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
}

pub async fn strategy_assign(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<StrategyAssignPayload>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let strategy_id = match f.strategy.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(guid) => match services::strategy::info_by_guid(&state.db, guid).await {
            Ok(Some(row)) => row.id,
            _ => return resp::error("strategy not found"),
        },
        None => 0,
    };
    for peer in f.peers {
        let Some(p) = find_peer_by_guidish(&state.db, &peer).await else {
            return resp::error(format!("device not found: {peer}"));
        };
        if let Err(e) = services::strategy::assign(
            &state.db,
            strategy_id,
            strategy_assignment::TARGET_PEER,
            &p.id,
            services::strategy::DEFAULT_PRIORITY,
        )
        .await
        {
            return resp::error(e.to_string());
        }
    }
    for user_guid in f.users {
        let Some(u) = find_user_by_guidish(&state.db, &user_guid).await else {
            return resp::error(format!("user not found: {user_guid}"));
        };
        if let Err(e) = services::strategy::assign(
            &state.db,
            strategy_id,
            strategy_assignment::TARGET_USER,
            &u.id.to_string(),
            services::strategy::DEFAULT_PRIORITY,
        )
        .await
        {
            return resp::error(e.to_string());
        }
    }
    for group_guid in f.groups {
        let Some(g) = find_device_group_by_guidish(&state.db, &group_guid).await else {
            return resp::error(format!("device group not found: {group_guid}"));
        };
        if let Err(e) = services::strategy::assign(
            &state.db,
            strategy_id,
            strategy_assignment::TARGET_DEVICE_GROUP,
            &g.id.to_string(),
            services::strategy::DEFAULT_PRIORITY,
        )
        .await
        {
            return resp::error(e.to_string());
        }
    }
    Json(json!({ "success": true })).into_response()
}

pub async fn user_groups(
    State(state): State<AppState>,
    user: RustClientUser,
    Query(q): Query<ProPageQuery>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).max(1);
    let mut rows: Vec<ProGroup> = group::Entity::find()
        .order_by_desc(group::Column::Id)
        .all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|g| ProGroup {
            guid: g.id.to_string(),
            name: g.name,
            note: String::new(),
            allowed_incomings: Value::Array(vec![]),
            allowed_outgoings: Value::Array(vec![]),
        })
        .filter(|g| matches_like(&g.name, q.name.as_deref()))
        .collect();
    let total = rows.len() as i64;
    let start = ((page - 1) * page_size) as usize;
    rows = rows
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect();
    resp::data_response(total, rows)
}

pub async fn user_group_create(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<GroupForm>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    match services::group::create(&state.db, &f.name, group::TYPE_DEFAULT).await {
        Ok(row) => Json(json!({ "guid": row.id.to_string(), "name": row.name })).into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

pub async fn user_group_update(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(f): Json<GroupForm>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let Some(row) = find_user_group_by_guidish(&state.db, &guid).await else {
        return resp::error("user group not found");
    };
    match services::group::update(&state.db, row.id, f.name, row.r#type).await {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

pub async fn user_group_delete(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let Some(row) = find_user_group_by_guidish(&state.db, &guid).await else {
        return resp::error("user group not found");
    };
    match services::group::delete(&state.db, row.id).await {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

pub async fn user_group_add_users(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(user_guids): Json<Vec<String>>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let Some(group) = find_user_group_by_guidish(&state.db, &guid).await else {
        return resp::error("user group not found");
    };
    for user_guid in user_guids {
        let Some(u) = find_user_by_guidish(&state.db, &user_guid).await else {
            return resp::error(format!("user not found: {user_guid}"));
        };
        let mut am: user::ActiveModel = u.into();
        am.group_id = Set(group.id);
        am.updated_at = Set(services::now());
        if let Err(e) = am.update(&state.db).await {
            return resp::error(e.to_string());
        }
    }
    Json(json!({ "success": true })).into_response()
}

#[derive(Deserialize, Default)]
pub struct UserCreateForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub group_name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub note: String,
}

pub async fn user_create(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<UserCreateForm>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    if !valid_user_create_form(&f) {
        return resp::error("ParamsError");
    }
    let group_id = if f.group_name.trim().is_empty() {
        1
    } else {
        match group::Entity::find()
            .filter(group::Column::Name.eq(f.group_name.trim()))
            .one(&state.db)
            .await
        {
            Ok(Some(row)) => row.id,
            _ => return resp::error("user group not found"),
        }
    };
    let model = user::Model {
        id: 0,
        username: f.name.trim().to_string(),
        email: f.email.trim().to_string(),
        password: f.password,
        nickname: String::new(),
        avatar: String::new(),
        group_id,
        is_admin: Some(false),
        status: user::STATUS_ENABLE,
        must_change_password: false,
        tfa_secret: String::new(),
        tfa_enabled: false,
        tfa_enforced: false,
        email_verification_enabled: false,
        login_device_verification_enabled: false,
        remark: f.note,
        created_at: None,
        updated_at: None,
    };
    match services::user::create(&state.db, model).await {
        Ok(row) => {
            Json(json!({ "guid": row.id.to_string(), "name": row.username })).into_response()
        }
        Err(e) => resp::error(e),
    }
}

fn valid_user_create_form(f: &UserCreateForm) -> bool {
    !f.name.trim().is_empty() && !f.password.trim().is_empty()
}

#[derive(Deserialize, Default)]
pub struct UserInviteForm {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub group_name: String,
    #[serde(default)]
    pub note: String,
}

pub async fn user_invite(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<UserInviteForm>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let group_id = if f.group_name.trim().is_empty() {
        1
    } else {
        match group::Entity::find()
            .filter(group::Column::Name.eq(f.group_name.trim()))
            .one(&state.db)
            .await
        {
            Ok(Some(row)) => row.id,
            _ => return resp::error("user group not found"),
        }
    };
    if let Ok(Some(existing)) = services::user::info_by_username(&state.db, f.name.trim()).await {
        return Json(json!({ "guid": existing.id.to_string(), "name": existing.username }))
            .into_response();
    }
    let model = user::Model {
        id: 0,
        username: f.name,
        email: f.email,
        password: format!("invite-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)),
        nickname: String::new(),
        avatar: String::new(),
        group_id,
        is_admin: Some(false),
        status: user::STATUS_ENABLE,
        must_change_password: true,
        tfa_secret: String::new(),
        tfa_enabled: false,
        tfa_enforced: false,
        email_verification_enabled: false,
        login_device_verification_enabled: false,
        remark: f.note,
        created_at: None,
        updated_at: None,
    };
    match services::user::create(&state.db, model).await {
        Ok(row) => {
            Json(json!({ "guid": row.id.to_string(), "name": row.username })).into_response()
        }
        Err(e) => resp::error(e),
    }
}

#[derive(Deserialize, Default)]
pub struct UserGuidListForm {
    #[serde(default)]
    pub user_guids: Vec<String>,
    #[serde(default)]
    pub enforce: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default, rename = "type")]
    pub verification_type: String,
}

pub async fn user_tfa_enforce(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<UserGuidListForm>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    for guid in f.user_guids {
        let Some(row) = find_user_by_guidish(&state.db, &guid).await else {
            return resp::error(format!("user not found: {guid}"));
        };
        if let Err(e) = services::login_security::update_user_login_security(
            &state.db,
            &row,
            f.enforce,
            row.email_verification_enabled,
            row.login_device_verification_enabled,
        )
        .await
        {
            return resp::error(e);
        }
    }
    Json(json!({ "success": true })).into_response()
}

pub async fn user_disable_login_verification(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<UserGuidListForm>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    for guid in f.user_guids {
        let Some(row) = find_user_by_guidish(&state.db, &guid).await else {
            return resp::error(format!("user not found: {guid}"));
        };
        if let Err(e) = services::login_security::update_user_login_security(
            &state.db,
            &row,
            false,
            false,
            false,
        )
        .await
        {
            return resp::error(e);
        }
    }
    Json(json!({ "success": true })).into_response()
}

pub async fn user_force_logout(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<UserGuidListForm>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let mut ids = vec![];
    for guid in f.user_guids {
        let Some(row) = find_user_by_guidish(&state.db, &guid).await else {
            return resp::error(format!("user not found: {guid}"));
        };
        ids.push(row.id);
    }
    match services::user::delete_tokens_by_user_ids(&state.db, &ids).await {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

pub async fn user_disable(
    State(state): State<AppState>,
    admin: RustClientUser,
    Path(guid): Path<String>,
) -> Response {
    set_user_status(state, admin, guid, user::STATUS_DISABLED).await
}

pub async fn user_enable(
    State(state): State<AppState>,
    admin: RustClientUser,
    Path(guid): Path<String>,
) -> Response {
    set_user_status(state, admin, guid, user::STATUS_ENABLE).await
}

pub async fn user_delete(
    State(state): State<AppState>,
    admin: RustClientUser,
    Path(guid): Path<String>,
) -> Response {
    if !admin.user.is_admin() {
        return resp::error("Permission denied");
    }
    let Some(row) = find_user_by_guidish(&state.db, &guid).await else {
        return resp::error("user not found");
    };
    match services::user::delete(&state.db, &row).await {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => resp::error(e),
    }
}

pub async fn audits_conn(
    State(state): State<AppState>,
    user: RustClientUser,
    Query(q): Query<ProPageQuery>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let remote = q.remote.or(q.id);
    match services::audit::conn_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        remote,
        None,
    )
    .await
    {
        Ok(r) => resp::data_response(r.total, r.list),
        Err(e) => resp::error(e.to_string()),
    }
}

pub async fn audits_file(
    State(state): State<AppState>,
    user: RustClientUser,
    Query(q): Query<ProPageQuery>,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let remote = q.remote.or(q.id);
    match services::audit::file_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        remote,
        None,
    )
    .await
    {
        Ok(r) => resp::data_response(r.total, r.list),
        Err(e) => resp::error(e.to_string()),
    }
}

pub async fn audits_empty(user: RustClientUser) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    resp::data_response(0, Vec::<Value>::new())
}

async fn set_device_status(
    state: AppState,
    user: RustClientUser,
    guid: String,
    status: i32,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let Some(p) = find_peer_by_guidish(&state.db, &guid).await else {
        return resp::error("device not found");
    };
    let mut am: peer::ActiveModel = p.into();
    am.status = Set(status);
    am.updated_at = Set(services::now());
    match am.update(&state.db).await {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

async fn set_user_status(
    state: AppState,
    admin: RustClientUser,
    guid: String,
    status: i32,
) -> Response {
    if !admin.user.is_admin() {
        return resp::error("Permission denied");
    }
    let Some(mut row) = find_user_by_guidish(&state.db, &guid).await else {
        return resp::error("user not found");
    };
    row.status = status;
    match services::user::update(&state.db, &row).await {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => resp::error(e),
    }
}

async fn set_devices_group(
    state: AppState,
    user: RustClientUser,
    guid: String,
    ids: Vec<String>,
    assign: bool,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let Some(group) = find_device_group_by_guidish(&state.db, &guid).await else {
        return resp::error("device group not found");
    };
    for id in ids {
        let Some(p) = services::peer::find_by_id(&state.db, &id)
            .await
            .ok()
            .flatten()
        else {
            return resp::error(format!("device not found: {id}"));
        };
        let mut am: peer::ActiveModel = p.into();
        am.group_id = Set(if assign { group.id } else { 0 });
        am.updated_at = Set(services::now());
        if let Err(e) = am.update(&state.db).await {
            return resp::error(e.to_string());
        }
    }
    Json(json!({ "success": true })).into_response()
}

async fn assign_device_value(
    state: &AppState,
    p: peer::Model,
    assign_type: &str,
    value: &str,
) -> Result<(), String> {
    match assign_type {
        "user_name" => {
            let user = services::user::info_by_username(&state.db, value)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("user not found: {value}"))?;
            update_peer(p, &state.db, |am| am.user_id = Set(user.id)).await
        }
        "device_group_name" => {
            let group = services::group::device_group_info_by_name(&state.db, value)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("device group not found: {value}"))?;
            update_peer(p, &state.db, |am| am.group_id = Set(group.id)).await
        }
        "strategy_name" => {
            services::strategy::assign_peer_by_strategy_name(&state.db, &p.id, value)
                .await
                .map_err(|e| e.to_string())
        }
        "note" => update_peer(p, &state.db, |am| am.alias = Set(value.to_string())).await,
        "device_username" => {
            update_peer(p, &state.db, |am| am.username = Set(value.to_string())).await
        }
        "device_name" => update_peer(p, &state.db, |am| am.hostname = Set(value.to_string())).await,
        "ab" => assign_address_book_value(state, &p, value).await,
        _ => Err("invalid assignment type".to_string()),
    }
}

async fn update_peer<F>(p: peer::Model, db: &DatabaseConnection, change: F) -> Result<(), String>
where
    F: FnOnce(&mut peer::ActiveModel),
{
    let mut am: peer::ActiveModel = p.into();
    change(&mut am);
    am.updated_at = Set(services::now());
    am.update(db).await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn assign_address_book_value(
    state: &AppState,
    p: &peer::Model,
    value: &str,
) -> Result<(), String> {
    let parts: Vec<&str> = value.split(',').collect();
    let collection_name = parts.first().map(|s| s.trim()).unwrap_or("");
    if collection_name.is_empty() {
        return Err("address book name is required".to_string());
    }
    let user_id = if p.user_id > 0 { p.user_id } else { 1 };
    let collection = match services::address_book::collection_info_by_user_and_name(
        &state.db,
        user_id,
        collection_name,
    )
    .await
    .map_err(|e| e.to_string())?
    {
        Some(collection) => collection,
        None => services::address_book::create_collection(&state.db, user_id, collection_name)
            .await
            .map_err(|e| e.to_string())?,
    };
    let mut ab = services::address_book::from_peer(p);
    ab.user_id = user_id;
    ab.collection_id = collection.id;
    if let Some(alias) = parts.get(2).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        ab.alias = alias.to_string();
    }
    if let Some(password) = parts.get(3).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        ab.password = password.to_string();
    }
    if let Some(note) = parts.get(4).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if ab.alias.is_empty() {
            ab.alias = note.to_string();
        }
    }
    if let Some(existing) = services::address_book::info_by_user_id_and_id_and_cid(
        &state.db,
        user_id,
        &ab.id,
        collection.id,
    )
    .await
    .map_err(|e| e.to_string())?
    {
        ab.row_id = existing.row_id;
        services::address_book::update_all(&state.db, ab)
            .await
            .map_err(|e| e.to_string())
    } else {
        services::address_book::create(&state.db, ab)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

async fn find_peer_by_guidish(db: &DatabaseConnection, guid: &str) -> Option<peer::Model> {
    if let Ok(row_id) = guid.parse::<i32>() {
        if let Ok(Some(row)) = services::peer::info_by_row_id(db, row_id).await {
            return Some(row);
        }
    }
    if let Ok(Some(row)) = peer::Entity::find()
        .filter(peer::Column::Guid.eq(guid))
        .one(db)
        .await
    {
        return Some(row);
    }
    services::peer::find_by_id(db, guid).await.ok().flatten()
}

async fn find_user_by_guidish(db: &DatabaseConnection, guid: &str) -> Option<user::Model> {
    let id = guid.parse::<i32>().ok()?;
    services::user::info_by_id(db, id).await.ok().flatten()
}

async fn find_device_group_by_guidish(
    db: &DatabaseConnection,
    guid: &str,
) -> Option<device_group::Model> {
    let id = guid.parse::<i32>().ok()?;
    services::group::device_group_info_by_id(db, id)
        .await
        .ok()
        .flatten()
}

async fn find_user_group_by_guidish(db: &DatabaseConnection, guid: &str) -> Option<group::Model> {
    let id = guid.parse::<i32>().ok()?;
    services::group::info_by_id(db, id).await.ok().flatten()
}

fn peer_guid(p: &peer::Model) -> String {
    if p.guid.trim().is_empty() {
        p.row_id.to_string()
    } else {
        p.guid.clone()
    }
}

fn matches_like(value: &str, pattern: Option<&str>) -> bool {
    let Some(pattern) = pattern.map(str::trim).filter(|v| !v.is_empty()) else {
        return true;
    };
    if pattern == "-" {
        return value.is_empty();
    }
    let needle = pattern.trim_matches('%');
    if pattern.contains('%') {
        value.contains(needle)
    } else {
        value == needle || value.contains(needle)
    }
}

fn iso_from_unix(ts: i64) -> String {
    match Utc.timestamp_opt(ts.max(0), 0).single() {
        Some(dt) => dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        None => "1970-01-01T00:00:00.000Z".to_string(),
    }
}

#[allow(dead_code)]
fn naive_to_unix(ts: Option<NaiveDateTime>) -> i64 {
    ts.map(|ts| ts.and_utc().timestamp()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pro_page_query_accepts_script_pagination_aliases() {
        let q: ProPageQuery = serde_json::from_value(json!({
            "current": 3,
            "pageSize": 7,
            "device_name": "workstation"
        }))
        .unwrap();

        assert_eq!(q.page, Some(3));
        assert_eq!(q.page_size, Some(7));
        assert_eq!(q.device_name.as_deref(), Some("workstation"));
    }

    #[test]
    fn pro_page_query_keeps_native_page_field() {
        let q: ProPageQuery = serde_json::from_value(json!({
            "page": 2,
            "pageSize": 25,
            "group_name": "ops"
        }))
        .unwrap();

        assert_eq!(q.page, Some(2));
        assert_eq!(q.page_size, Some(25));
        assert_eq!(q.group_name.as_deref(), Some("ops"));
    }

    #[test]
    fn pro_user_create_accepts_official_script_payload_without_email() {
        let f: UserCreateForm = serde_json::from_value(json!({
            "name": "script-user",
            "password": "secret",
            "group_name": "ops",
            "note": "created by users.py"
        }))
        .unwrap();

        assert!(valid_user_create_form(&f));
        assert!(f.email.is_empty());
    }

    #[test]
    fn pro_user_create_rejects_missing_password() {
        let f: UserCreateForm = serde_json::from_value(json!({
            "name": "script-user",
            "group_name": "ops"
        }))
        .unwrap();

        assert!(!valid_user_create_form(&f));
    }
}

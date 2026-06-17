//! RustDesk PC-client API (`/api/*`), ports of `http/controller/api/*.go`.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Duration, Utc};
use sea_orm::Set;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

use entity::{address_book, audit_file, group, login_log, peer};

use crate::http::middleware::{AcceptLang, ClientIp, RustClientUser};
use crate::http::payloads::*;
use crate::http::response as resp;
use crate::services;
use crate::state::AppState;

// ---------- index ----------

pub async fn index() -> Response {
    resp::success("Hello Gwen")
}

pub async fn version(State(state): State<AppState>) -> Response {
    resp::success(state.version.as_str())
}

#[derive(Deserialize, Default)]
pub struct HeartbeatForm {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub ver: i64,
    #[serde(default)]
    pub conns: Option<Vec<i64>>,
    #[serde(default)]
    pub modified_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct HeartbeatStrategy {
    #[serde(default)]
    config_options: HashMap<String, String>,
    #[serde(default)]
    extra: HashMap<String, String>,
    translate_mode: bool,
}

pub async fn heartbeat(State(state): State<AppState>, body: String) -> Response {
    let info: HeartbeatForm = match serde_json::from_str(&body) {
        Ok(info) => info,
        Err(e) => return Json(json!({ "error": e.to_string() })).into_response(),
    };
    let rustdesk_id = resolve_heartbeat_rustdesk_id(&info.id, &info.uuid);
    if rustdesk_id.is_empty() {
        return Json(json!({ "error": "missing id" })).into_response();
    }
    if let Ok(Some(p)) = services::peer::find_by_id(&state.db, &rustdesk_id).await {
        if Utc::now().timestamp() - p.last_online_time >= 30 {
            let am = peer::ActiveModel {
                row_id: Set(p.row_id),
                last_online_time: Set(Utc::now().timestamp()),
                ..Default::default()
            };
            let _ = services::peer::update(&state.db, am).await;
        }
    }
    let disconnect_key = heartbeat_disconnect_key(&info.uuid, &rustdesk_id);
    if let Some(conns) = &info.conns {
        if let Err(e) =
            services::active_connection::sync_for_device(&state.db, &rustdesk_id, &info.uuid, conns)
                .await
        {
            tracing::warn!("failed to sync active connections: {e}");
        }
        state
            .disconnect_store
            .remove_disconnected(&disconnect_key, conns);
    }

    let mut res = json!({
        "modified_at": info.modified_at,
        "strategy": resolve_capability(&normalize_reported_version(&info.version, info.ver)),
    });
    let disconnect = state.disconnect_store.pending(&disconnect_key);
    if !disconnect.is_empty() {
        res["disconnect"] = json!(disconnect);
    }
    Json(res).into_response()
}

fn resolve_heartbeat_rustdesk_id(id: &str, uuid: &str) -> String {
    if !id.is_empty() {
        id.to_string()
    } else {
        uuid.to_string()
    }
}

fn heartbeat_disconnect_key(uuid: &str, rustdesk_id: &str) -> String {
    if !uuid.is_empty() {
        uuid.to_string()
    } else {
        rustdesk_id.to_string()
    }
}

fn normalize_reported_version(version: &str, ver: i64) -> String {
    if !version.is_empty() {
        version.to_string()
    } else if ver > 0 {
        ver.to_string()
    } else {
        String::new()
    }
}

fn resolve_capability(version: &str) -> HeartbeatStrategy {
    let translate_mode = version_gte(version, "1.4.6");
    let mut extra = HashMap::new();
    extra.insert("translate_mode".to_string(), translate_mode.to_string());
    HeartbeatStrategy {
        config_options: HashMap::new(),
        extra,
        translate_mode,
    }
}

fn version_gte(version: &str, target: &str) -> bool {
    let version = parse_version(version);
    let target = parse_version(target);
    for i in 0..3 {
        if version[i] > target[i] {
            return true;
        }
        if version[i] < target[i] {
            return false;
        }
    }
    true
}

fn parse_version(version: &str) -> [i64; 3] {
    let version = version.trim().trim_start_matches('v');
    let mut parsed = [0; 3];
    for (i, part) in version.split('.').take(3).enumerate() {
        let Ok(n) = part.parse::<i64>() else {
            return [0; 3];
        };
        parsed[i] = n;
    }
    parsed
}

#[cfg(test)]
mod heartbeat_tests {
    use super::*;

    #[test]
    fn heartbeat_id_prefers_id_and_falls_back_to_uuid() {
        assert_eq!(
            resolve_heartbeat_rustdesk_id("182921366", "u-1"),
            "182921366"
        );
        assert_eq!(resolve_heartbeat_rustdesk_id("", "u-1"), "u-1");
    }

    #[test]
    fn heartbeat_version_prefers_string_and_falls_back_to_numeric_ver() {
        assert_eq!(normalize_reported_version("1.4.6", 1002070), "1.4.6");
        assert_eq!(normalize_reported_version("", 10604), "10604");
        assert_eq!(normalize_reported_version("", 0), "");
    }

    #[test]
    fn heartbeat_disconnect_key_prefers_uuid_and_falls_back_to_rustdesk_id() {
        assert_eq!(heartbeat_disconnect_key("uuid-1", "182921366"), "uuid-1");
        assert_eq!(heartbeat_disconnect_key("", "182921366"), "182921366");
    }

    #[test]
    fn heartbeat_capability_enables_translate_mode_from_1_4_6() {
        assert!(!resolve_capability("1.4.5").translate_mode);
        assert!(resolve_capability("1.4.6").translate_mode);
        assert!(resolve_capability("v1.4.7").translate_mode);
    }
}

// ---------- login ----------

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
pub struct LoginForm {
    #[serde(default, rename = "autoLogin")]
    pub auto_login: bool,
    #[serde(default, rename = "deviceInfo")]
    pub device_info: DeviceInfo,
    #[serde(default)]
    pub id: String,
    #[serde(default, rename = "type")]
    pub r#type: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

pub async fn login(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    ClientIp(ip): ClientIp,
    headers: axum::http::HeaderMap,
    Json(mut form): Json<LoginForm>,
) -> Response {
    if state.config.app.disable_pwd_login {
        return resp::error(state.tr(&lang, "PwdLoginDisabled"));
    }
    if form.username.len() < 2 || form.password.len() < 4 {
        state.limiter.record_failed_attempt(&ip);
        return resp::error(state.tr(&lang, "ParamsError"));
    }
    let user = match services::user::info_by_username_password(
        &state.db,
        &state.config,
        &form.username,
        &form.password,
    )
    .await
    {
        Ok(Some(u)) => u,
        _ => {
            state.limiter.record_failed_attempt(&ip);
            return resp::error(state.tr(&lang, "UsernameOrPasswordError"));
        }
    };
    if !user.is_enabled() {
        return resp::error(state.tr(&lang, "UserDisabled"));
    }
    // referer => web client login
    if headers.get("referer").is_some() {
        form.device_info.r#type = login_log::CLIENT_WEB.to_string();
    }
    let ut = match services::user::login(
        &state.db,
        &state.jwt,
        &state.config,
        &user,
        services::user::LoginEvent {
            client: form.device_info.r#type.clone(),
            device_id: form.id.clone(),
            uuid: form.uuid.clone(),
            ip,
            login_type: login_log::TYPE_ACCOUNT.to_string(),
            platform: form.device_info.os.clone(),
        },
    )
    .await
    {
        Ok(ut) => ut,
        Err(e) => return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    };
    Json(LoginRes {
        r#type: "access_token".into(),
        access_token: ut.token,
        user: UserPayload::from_user(&user),
        secret: String::new(),
        tfa_type: String::new(),
    })
    .into_response()
}

pub async fn login_options(State(state): State<AppState>) -> Response {
    let mut ops: Vec<String> = services::oauth::get_oauth_providers(&state.db)
        .await
        .unwrap_or_default();
    if state.config.app.web_sso {
        ops.push("webauth".to_string());
    }
    let oidc_items: Vec<Map<String, Value>> = ops
        .iter()
        .map(|v| {
            let mut m = Map::new();
            m.insert("name".into(), Value::String(v.clone()));
            m
        })
        .collect();
    let common = serde_json::to_string(&oidc_items).unwrap_or_else(|_| "[]".into());
    let mut res = vec![format!("common-oidc/{common}")];
    for v in &ops {
        res.push(format!("oidc/{v}"));
    }
    Json(res).into_response()
}

pub async fn logout(State(state): State<AppState>, user: RustClientUser) -> Response {
    let _ = services::user::logout(&state.db, user.user.id, &user.token).await;
    Json(Value::Null).into_response()
}

// ---------- user ----------

pub async fn user_info(user: RustClientUser) -> Response {
    Json(UserPayload::from_user(&user.user)).into_response()
}

// ---------- sysinfo ----------

#[derive(Deserialize, Default)]
pub struct PeerForm {
    #[serde(default)]
    pub cpu: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub id: String,
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
}

pub async fn sysinfo(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    Json(f): Json<PeerForm>,
) -> Response {
    let existing = match services::peer::find_by_id(&state.db, &f.id).await {
        Ok(p) => p,
        Err(e) => return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    };
    match existing {
        None => {
            let user_id = services::user::find_latest_user_id_by_uuid(&state.db, &f.uuid, &f.id)
                .await
                .unwrap_or(0);
            let am = peer::ActiveModel {
                id: Set(f.id.clone()),
                cpu: Set(f.cpu),
                hostname: Set(f.hostname),
                memory: Set(f.memory),
                os: Set(f.os),
                username: Set(f.username),
                uuid: Set(f.uuid),
                version: Set(f.version),
                user_id: Set(user_id),
                ..Default::default()
            };
            if let Err(e) = services::peer::create(&state.db, am).await {
                return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
            }
        }
        Some(p) => {
            let user_id = if p.user_id == 0 {
                services::user::find_latest_user_id_by_uuid(&state.db, &f.uuid, &f.id)
                    .await
                    .unwrap_or(0)
            } else {
                p.user_id
            };
            let am = peer::ActiveModel {
                row_id: Set(p.row_id),
                id: Set(f.id),
                cpu: Set(f.cpu),
                hostname: Set(f.hostname),
                memory: Set(f.memory),
                os: Set(f.os),
                username: Set(f.username),
                uuid: Set(f.uuid),
                version: Set(f.version),
                user_id: Set(user_id),
                ..Default::default()
            };
            if let Err(e) = services::peer::update(&state.db, am).await {
                return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
            }
        }
    }
    "SYSINFO_UPDATED".into_response()
}

pub async fn sysinfo_ver(State(state): State<AppState>) -> Response {
    format!("{}\n{}", state.version, state.start_time).into_response()
}

// ---------- group ----------

#[derive(Deserialize)]
pub struct PageQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default, rename = "pageSize")]
    pub page_size: Option<u64>,
}

pub async fn group_users(
    State(state): State<AppState>,
    user: RustClientUser,
    Query(q): Query<PageQuery>,
) -> Response {
    let u = &user.user;
    let gr = services::group::info_by_id(&state.db, u.group_id)
        .await
        .ok()
        .flatten();
    let is_share = gr.map(|g| g.r#type == group::TYPE_SHARE).unwrap_or(false);

    let (users, total) = if !u.is_admin() && !is_share {
        (vec![u.clone()], 1i64)
    } else {
        let r = services::user::list_by_group_id(
            &state.db,
            u.group_id,
            q.page.unwrap_or(1),
            q.page_size.unwrap_or(10),
        )
        .await
        .unwrap_or(services::user::UserListResult {
            list: vec![],
            page: 1,
            page_size: 10,
            total: 0,
        });
        (r.list, r.total)
    };
    let data: Vec<UserPayload> = users.iter().map(UserPayload::from_user).collect();
    resp::data_response(total, data)
}

pub async fn group_peers(
    State(state): State<AppState>,
    user: RustClientUser,
    Query(q): Query<PageQuery>,
) -> Response {
    let u = &user.user;
    let gr = services::group::info_by_id(&state.db, u.group_id)
        .await
        .ok()
        .flatten();
    let is_share = gr.map(|g| g.r#type == group::TYPE_SHARE).unwrap_or(false);

    let users = if !u.is_admin() && !is_share {
        vec![u.clone()]
    } else {
        services::user::list_id_and_name_by_group_id(&state.db, u.group_id)
            .await
            .unwrap_or_default()
    };
    let names: std::collections::HashMap<i32, String> =
        users.iter().map(|x| (x.id, x.username.clone())).collect();
    let user_ids: Vec<i32> = users.iter().map(|x| x.id).collect();

    let dgroups = services::group::device_group_list_all(&state.db)
        .await
        .unwrap_or_default();
    let dgroup_names: std::collections::HashMap<i32, String> =
        dgroups.iter().map(|g| (g.id, g.name.clone())).collect();

    let pl = services::peer::list_by_user_ids(
        &state.db,
        &user_ids,
        q.page.unwrap_or(1),
        q.page_size.unwrap_or(10),
    )
    .await
    .unwrap_or(services::peer::PeerListResult {
        list: vec![],
        page: 1,
        page_size: 10,
        total: 0,
    });
    let data: Vec<GroupPeerPayload> = pl
        .list
        .iter()
        .map(|p| {
            let uname = names.get(&p.user_id).cloned().unwrap_or_default();
            let dgname = dgroup_names.get(&p.group_id).cloned().unwrap_or_default();
            GroupPeerPayload::from_peer(p, &uname, &dgname)
        })
        .collect();
    resp::data_response(pl.total, data)
}

pub async fn device_group_accessible(
    State(state): State<AppState>,
    user: RustClientUser,
) -> Response {
    if !user.user.is_admin() {
        return resp::error("Permission denied");
    }
    let dgroups = services::group::device_group_list_all(&state.db)
        .await
        .unwrap_or_default();
    resp::data_response(0, dgroups)
}

// ---------- device deployment / CLI assignment ----------

#[derive(Deserialize, Default)]
pub struct DeviceDeployForm {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub pk: String,
}

pub async fn device_deploy(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<DeviceDeployForm>,
) -> Response {
    let _ = f.pk.trim();
    if f.id.trim().is_empty() || f.uuid.trim().is_empty() {
        return Json(json!({ "result": "INVALID_INPUT" })).into_response();
    }
    match upsert_deployed_peer(&state, &user.user, &f.id, &f.uuid, None).await {
        Ok(_) => Json(json!({ "result": "OK" })).into_response(),
        Err(DeviceDeployError::IdTaken) => Json(json!({ "result": "ID_TAKEN" })).into_response(),
        Err(DeviceDeployError::InvalidInput) => {
            Json(json!({ "result": "INVALID_INPUT" })).into_response()
        }
        Err(DeviceDeployError::Other(e)) => Json(json!({ "error": e })).into_response(),
    }
}

#[derive(Debug)]
enum DeviceDeployError {
    InvalidInput,
    IdTaken,
    Other(String),
}

#[derive(Deserialize, Default)]
pub struct DeviceCliForm {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub strategy_name: Option<String>,
    #[serde(default)]
    pub address_book_name: Option<String>,
    #[serde(default)]
    pub address_book_tag: Option<String>,
    #[serde(default)]
    pub address_book_alias: Option<String>,
    #[serde(default)]
    pub address_book_password: Option<String>,
    #[serde(default)]
    pub address_book_note: Option<String>,
    #[serde(default)]
    pub device_group_name: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub device_username: Option<String>,
    #[serde(default)]
    pub device_name: Option<String>,
}

pub async fn device_cli(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<DeviceCliForm>,
) -> Response {
    match apply_device_cli_assignment(&state, &user.user, f).await {
        Ok(message) => message.into_response(),
        Err(message) => message.into_response(),
    }
}

async fn apply_device_cli_assignment(
    state: &AppState,
    token_user: &entity::user::Model,
    f: DeviceCliForm,
) -> Result<String, String> {
    if f.id.trim().is_empty() || f.uuid.trim().is_empty() {
        return Err("INVALID_INPUT".to_string());
    }
    let target_user = match option_text(&f.user_name) {
        Some(name) => services::user::info_by_username(&state.db, &name)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("User not found: {name}"))?,
        None => token_user.clone(),
    };
    let device_group_id = match option_text(&f.device_group_name) {
        Some(name) => Some(
            services::group::device_group_info_by_name(&state.db, &name)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Device group not found: {name}"))?
                .id,
        ),
        None => None,
    };
    let mut peer = upsert_deployed_peer(state, &target_user, &f.id, &f.uuid, device_group_id)
        .await
        .map_err(|e| match e {
            DeviceDeployError::InvalidInput => "INVALID_INPUT".to_string(),
            DeviceDeployError::IdTaken => "ID_TAKEN".to_string(),
            DeviceDeployError::Other(e) => e,
        })?;

    let mut changed_peer = false;
    if let Some(name) = option_text(&f.device_username) {
        peer.username = name;
        changed_peer = true;
    }
    if let Some(name) = option_text(&f.device_name) {
        peer.hostname = name;
        changed_peer = true;
    }
    if let Some(note) = option_text(&f.note) {
        peer.alias = note;
        changed_peer = true;
    }
    if changed_peer {
        let am = peer::ActiveModel {
            row_id: Set(peer.row_id),
            id: Set(peer.id.clone()),
            uuid: Set(peer.uuid.clone()),
            username: Set(peer.username.clone()),
            hostname: Set(peer.hostname.clone()),
            alias: Set(peer.alias.clone()),
            user_id: Set(peer.user_id),
            group_id: Set(peer.group_id),
            ..Default::default()
        };
        services::peer::update(&state.db, am)
            .await
            .map_err(|e| e.to_string())?;
    }

    if let Some(address_book_name) = option_text(&f.address_book_name) {
        assign_peer_to_address_book(state, &target_user, &peer, &address_book_name, &f).await?;
    }

    let _ = option_text(&f.strategy_name);
    Ok(String::new())
}

async fn upsert_deployed_peer(
    state: &AppState,
    user: &entity::user::Model,
    id: &str,
    uuid: &str,
    device_group_id: Option<i32>,
) -> Result<peer::Model, DeviceDeployError> {
    let id = id.trim();
    let uuid = uuid.trim();
    if id.is_empty() || uuid.is_empty() {
        return Err(DeviceDeployError::InvalidInput);
    }
    let group_id = device_group_id.unwrap_or(0);
    let existing = services::peer::find_by_id(&state.db, id)
        .await
        .map_err(|e| DeviceDeployError::Other(e.to_string()))?;
    match existing {
        Some(mut p) => {
            if !p.uuid.is_empty() && p.uuid != uuid {
                return Err(DeviceDeployError::IdTaken);
            }
            p.uuid = uuid.to_string();
            p.user_id = user.id;
            if device_group_id.is_some() {
                p.group_id = group_id;
            }
            let am = peer::ActiveModel {
                row_id: Set(p.row_id),
                id: Set(p.id.clone()),
                uuid: Set(p.uuid.clone()),
                user_id: Set(p.user_id),
                group_id: Set(p.group_id),
                ..Default::default()
            };
            services::peer::update(&state.db, am)
                .await
                .map_err(|e| DeviceDeployError::Other(e.to_string()))?;
            Ok(p)
        }
        None => {
            let p = peer::ActiveModel {
                id: Set(id.to_string()),
                uuid: Set(uuid.to_string()),
                user_id: Set(user.id),
                group_id: Set(group_id),
                ..Default::default()
            };
            services::peer::create(&state.db, p)
                .await
                .map_err(|e| DeviceDeployError::Other(e.to_string()))
        }
    }
}

async fn assign_peer_to_address_book(
    state: &AppState,
    user: &entity::user::Model,
    peer: &peer::Model,
    collection_name: &str,
    f: &DeviceCliForm,
) -> Result<(), String> {
    let collection = match services::address_book::collection_info_by_user_and_name(
        &state.db,
        user.id,
        collection_name,
    )
    .await
    .map_err(|e| e.to_string())?
    {
        Some(collection) => collection,
        None => services::address_book::create_collection(&state.db, user.id, collection_name)
            .await
            .map_err(|e| e.to_string())?,
    };

    let tags = option_text(&f.address_book_tag)
        .map(|tag| json!([tag]))
        .unwrap_or_else(|| Value::Array(vec![]));
    let mut ab = services::address_book::from_peer(peer);
    ab.user_id = user.id;
    ab.collection_id = collection.id;
    ab.tags = tags;
    if let Some(alias) = option_text(&f.address_book_alias) {
        ab.alias = alias;
    }
    if let Some(password) = option_text(&f.address_book_password) {
        ab.password = password;
    }
    if ab.alias.is_empty() {
        if let Some(note) = option_text(&f.address_book_note) {
            ab.alias = note;
        }
    }

    if let Some(existing) = services::address_book::info_by_user_id_and_id_and_cid(
        &state.db,
        user.id,
        &ab.id,
        collection.id,
    )
    .await
    .map_err(|e| e.to_string())?
    {
        ab.row_id = existing.row_id;
        services::address_book::update_all(&state.db, ab)
            .await
            .map_err(|e| e.to_string())?;
    } else {
        services::address_book::create(&state.db, ab)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn option_text(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

// ---------- web client ----------

pub async fn server_config(State(state): State<AppState>, user: RustClientUser) -> Response {
    let cfg = state.webclient_config.get().await;
    let abs = services::address_book::list_by_user_and_collection(&state.db, user.user.id, 0)
        .await
        .unwrap_or_default();
    let tm = (Utc::now() - Duration::hours(24))
        .timestamp_nanos_opt()
        .unwrap_or(0);
    let mut peers = Map::new();
    for ab in &abs {
        let pp = WebClientPeerPayload::from_address_book(ab, tm);
        peers.insert(
            ab.id.clone(),
            serde_json::to_value(pp).unwrap_or(Value::Null),
        );
    }
    resp::success(json!({
        "id_server": cfg.id_server,
        "key": cfg.key,
        "relay_server": cfg.relay_server,
        "api_server": cfg.api_server,
        "peers": peers,
    }))
}

pub async fn server_config_v2(State(state): State<AppState>, _user: RustClientUser) -> Response {
    let cfg = state.webclient_config.get().await;
    resp::success(json!({
        "id_server": cfg.id_server,
        "key": cfg.key,
        "relay_server": cfg.relay_server,
        "api_server": cfg.api_server,
    }))
}

pub async fn shared_peer(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let cfg = state.webclient_config.get().await;
    let token = body
        .get("share_token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if token.is_empty() {
        return resp::fail(101, "share_token is required");
    }
    let sr = match services::address_book::shared_peer(&state.db, token).await {
        Ok(Some(sr)) => sr,
        _ => return resp::fail(101, "share not found"),
    };
    if sr.expire != 0 {
        if let Some(created) = sr.created_at {
            let expires = created + Duration::seconds(sr.expire);
            if expires < Utc::now().naive_utc() {
                return resp::fail(101, "share expired");
            }
        }
    }
    let ab =
        match services::address_book::info_by_user_id_and_id(&state.db, sr.user_id, &sr.peer_id)
            .await
        {
            Ok(Some(ab)) => ab,
            _ => return resp::fail(101, "peer not found"),
        };
    let tm = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let mut pp = WebClientPeerPayload::from_share_record(&sr, tm);
    pp.info.username = ab.username.clone();
    pp.info.hostname = ab.hostname.clone();
    resp::success(json!({
        "id_server": cfg.id_server,
        "key": cfg.key,
        "relay_server": cfg.relay_server,
        "api_server": cfg.api_server,
        "peer": pp,
    }))
}

// ---------- audit ----------

pub async fn audit_conn(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if let Err(e) = services::audit::record_conn_event(&state.db, &body).await {
        tracing::warn!("failed to record audit conn event: {e}");
    }
    Json(Value::Null).into_response()
}

#[derive(Deserialize, Default)]
pub struct AuditConnActiveQuery {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub conn_type: Option<i32>,
}

pub async fn audit_conn_active(
    State(state): State<AppState>,
    _user: RustClientUser,
    Query(q): Query<AuditConnActiveQuery>,
) -> Response {
    if q.id.is_empty() || q.session_id.is_empty() {
        return Json(String::new()).into_response();
    }
    match services::audit::active_conn_guid(&state.db, &q.id, &q.session_id, q.conn_type).await {
        Ok(Some(guid)) => Json(guid).into_response(),
        Ok(None) => Json(String::new()).into_response(),
        Err(e) => {
            tracing::warn!("failed to query active audit conn: {e}");
            Json(String::new()).into_response()
        }
    }
}

pub async fn audit_file(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let g = |k: &str| {
        body.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let gi = |k: &str| body.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
    let gb = |k: &str| body.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let am = audit_file::ActiveModel {
        from_peer: Set(g("from_peer")),
        info: Set(g("info")),
        is_file: Set(gb("is_file")),
        path: Set(g("path")),
        peer_id: Set(g("peer_id")),
        r#type: Set(gi("type") as i32),
        uuid: Set(g("uuid")),
        ip: Set(g("ip")),
        num: Set(gi("num") as i32),
        from_name: Set(g("from_name")),
        created_at: Set(services::now()),
        updated_at: Set(services::now()),
        ..Default::default()
    };
    use sea_orm::ActiveModelTrait;
    let _ = am.insert(&state.db).await;
    Json(Value::Null).into_response()
}

pub async fn audit_alarm(Json(body): Json<Value>) -> Response {
    tracing::warn!("received rustdesk alarm audit event: {}", body);
    Json(Value::Null).into_response()
}

#[derive(Deserialize, Default)]
pub struct AuditNoteForm {
    #[serde(default)]
    pub guid: String,
    #[serde(default)]
    pub note: String,
}

pub async fn audit_update(
    State(state): State<AppState>,
    _user: RustClientUser,
    Json(form): Json<AuditNoteForm>,
) -> Response {
    if form.guid.is_empty() {
        return resp::error("missing guid");
    }
    if let Err(e) =
        services::audit::update_conn_note_by_guid(&state.db, &form.guid, &form.note).await
    {
        tracing::warn!("failed to update audit note: {e}");
        return resp::error("failed to update audit note");
    }
    Json(Value::Null).into_response()
}

// ---------- recording upload ----------

#[derive(Deserialize, Default)]
pub struct RecordQuery {
    #[serde(default, rename = "type")]
    pub record_type: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub offset: Option<u64>,
    #[serde(default)]
    pub length: Option<usize>,
}

pub async fn record(
    State(state): State<AppState>,
    Query(q): Query<RecordQuery>,
    body: Bytes,
) -> Response {
    let filename = match services::record_file::sanitize_filename(&q.file) {
        Ok(filename) => filename,
        Err(e) => return resp::error(e),
    };
    let result = match q.record_type.as_str() {
        "new" => record_new(&state, &filename).await,
        "part" => record_write(&state, &filename, q.offset, q.length, body, false).await,
        "tail" => record_write(&state, &filename, q.offset, q.length, body, true).await,
        "remove" => record_remove(&state, &filename).await,
        "" => Err("missing record type".to_string()),
        _ => Err("invalid record type".to_string()),
    };

    match result {
        Ok(()) => Json(json!({})).into_response(),
        Err(e) => resp::error(e),
    }
}

async fn record_new(state: &AppState, filename: &str) -> Result<(), String> {
    let cfg = state.record_storage_config.get().await;
    let location =
        services::record_storage::start_upload(&cfg, &state.config.gin.resources_path, filename)
            .await?;
    if let Err(e) =
        services::record_file::start_upload(&state.db, filename, &location.backend, &location.key)
            .await
    {
        tracing::warn!("failed to create record file row: {e}");
    }
    Ok(())
}

async fn record_write(
    state: &AppState,
    filename: &str,
    offset: Option<u64>,
    length: Option<usize>,
    body: Bytes,
    complete: bool,
) -> Result<(), String> {
    let offset = offset.ok_or_else(|| "missing offset".to_string())?;
    if let Some(length) = length {
        if length != body.len() {
            return Err("record length mismatch".to_string());
        }
    }
    let cfg = state.record_storage_config.get().await;
    let location = match services::record_file::info_by_filename(&state.db, filename)
        .await
        .map_err(|_| "failed to query record file".to_string())?
    {
        Some(row) => services::record_storage::RecordStorageLocation {
            backend: services::record_storage::normalize_backend(&row.storage_backend).to_string(),
            key: row.storage_key,
        },
        None => services::record_storage::location_for(
            &cfg,
            &state.config.gin.resources_path,
            filename,
        )?,
    };
    let write = services::record_storage::write_chunk(
        &cfg,
        &state.config.gin.resources_path,
        &location,
        filename,
        offset,
        body,
        complete,
    )
    .await?;
    let result = if complete {
        services::record_file::complete_upload(&state.db, filename, write.size).await
    } else {
        services::record_file::mark_uploading(&state.db, filename, write.size).await
    };
    if let Err(e) = result {
        tracing::warn!("failed to update record file row: {e}");
    }
    if let Err(e) =
        services::record_file::bind_storage(&state.db, filename, &location.backend, &location.key)
            .await
    {
        tracing::warn!("failed to bind record file storage: {e}");
    }
    Ok(())
}

async fn record_remove(state: &AppState, filename: &str) -> Result<(), String> {
    let cfg = state.record_storage_config.get().await;
    let row = services::record_file::info_by_filename(&state.db, filename)
        .await
        .map_err(|_| "failed to query record file".to_string())?;
    let location = match row {
        Some(row) => services::record_storage::RecordStorageLocation {
            backend: services::record_storage::normalize_backend(&row.storage_backend).to_string(),
            key: row.storage_key,
        },
        None => services::record_storage::location_for(
            &cfg,
            &state.config.gin.resources_path,
            filename,
        )?,
    };
    if let Err(e) = services::record_storage::delete_object(
        &cfg,
        &state.config.gin.resources_path,
        &location.backend,
        &location.key,
        filename,
    )
    .await
    {
        tracing::warn!("failed to remove record object: {e}");
    }
    if let Err(e) = services::record_storage::cleanup_staging(
        &cfg,
        &state.config.gin.resources_path,
        &location.backend,
        filename,
    )
    .await
    {
        tracing::warn!("failed to remove record staging file: {e}");
    }
    if let Err(e) = services::record_file::delete_by_filename(&state.db, filename).await {
        tracing::warn!("failed to delete record file row: {e}");
    }
    Ok(())
}

#[cfg(test)]
mod record_tests {
    use super::*;

    #[test]
    fn record_filename_accepts_client_generated_names() {
        assert_eq!(
            services::record_file::sanitize_filename(
                "incoming_123456__20260617123345001_display0_vp9.webm"
            )
            .unwrap(),
            "incoming_123456__20260617123345001_display0_vp9.webm"
        );
    }

    #[test]
    fn record_filename_rejects_path_traversal() {
        assert!(services::record_file::sanitize_filename("../x.webm").is_err());
        assert!(services::record_file::sanitize_filename("dir/x.webm").is_err());
        assert!(services::record_file::sanitize_filename(r"dir\x.webm").is_err());
        assert!(services::record_file::sanitize_filename(".").is_err());
    }
}

// ---------- address book (legacy /ab) ----------

pub async fn ab_get(State(state): State<AppState>, user: RustClientUser) -> Response {
    let abs = services::address_book::list_by_user_and_collection(&state.db, user.user.id, 0)
        .await
        .unwrap_or_default();
    let tags = services::tag::list_by_user_and_collection(&state.db, user.user.id, 0)
        .await
        .unwrap_or_default();
    let mut tag_names = vec![];
    let mut tag_colors = Map::new();
    for t in &tags {
        tag_names.push(t.name.clone());
        tag_colors.insert(t.name.clone(), json!(t.color));
    }
    let tgc = serde_json::to_string(&tag_colors).unwrap_or_else(|_| "{}".into());
    let inner = json!({
        "peers": abs,
        "tags": tag_names,
        "tag_colors": tgc,
    });
    let data = serde_json::to_string(&inner).unwrap_or_else(|_| "{}".into());
    Json(json!({ "data": data })).into_response()
}

#[derive(Deserialize)]
pub struct AbForm {
    pub data: String,
}

#[derive(Deserialize)]
pub struct AbFormData {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub peers: Vec<address_book::Model>,
    #[serde(default)]
    pub tag_colors: String,
}

pub async fn ab_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: RustClientUser,
    Json(form): Json<AbForm>,
) -> Response {
    let abd: AbFormData = match serde_json::from_str(&form.data) {
        Ok(d) => d,
        Err(e) => return resp::error(format!("{}{}", state.tr(&lang, "ParamsError"), e)),
    };
    let tc: std::collections::HashMap<String, i64> =
        serde_json::from_str(&abd.tag_colors).unwrap_or_default();
    if let Err(e) =
        services::address_book::update_address_book(&state.db, abd.peers, user.user.id).await
    {
        return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
    }
    let _ = services::tag::update_tags(&state.db, user.user.id, tc).await;
    Json(Value::Null).into_response()
}

// ---------- address book (personal) ----------

fn parse_guid(guid: &str) -> (i32, i32, i32) {
    let parts: Vec<&str> = guid.split('-').collect();
    if parts.len() < 2 {
        return (0, 0, 0);
    }
    let cid = if parts.len() == 3 {
        parts[2].parse().unwrap_or(0)
    } else {
        0
    };
    let gid = parts[0].parse().unwrap_or(0);
    let uid = parts[1].parse().unwrap_or(0);
    if gid == 0 || uid == 0 {
        return (0, 0, 0);
    }
    (gid, uid, cid)
}

fn compose_guid(gid: i32, uid: i32, cid: i32) -> String {
    format!("{gid}-{uid}-{cid}")
}

/// Validate a guid against the current user (≈ `CheckGuid`); returns (uid, cid).
async fn check_guid(
    state: &AppState,
    cur: &entity::user::Model,
    guid: &str,
) -> Result<(i32, i32), Response> {
    let (gid, uid, cid) = parse_guid(guid);
    if gid == 0 || uid == 0 {
        return Err(resp::error("ParamsError"));
    }
    let u = if cur.id == uid {
        cur.clone()
    } else {
        match services::user::info_by_id(&state.db, uid).await {
            Ok(Some(u)) => u,
            _ => return Err(resp::error("ParamsError")),
        }
    };
    if u.group_id != gid {
        return Err(resp::error("ParamsError"));
    }
    if cid == 0 && cur.id != uid {
        return Err(resp::error("ParamsError"));
    }
    if cid > 0 {
        match services::address_book::collection_info_by_id(&state.db, cid).await {
            Ok(Some(c)) if c.user_id == uid => {}
            _ => return Err(resp::error("ParamsError")),
        }
    }
    Ok((uid, cid))
}

pub async fn ab_personal(State(state): State<AppState>, user: RustClientUser) -> Response {
    if state.config.rustdesk.personal == 1 {
        let guid = compose_guid(user.user.group_id, user.user.id, 0);
        Json(json!({ "guid": guid, "name": user.user.username, "rule": 3 })).into_response()
    } else {
        Json(Value::Null).into_response()
    }
}

pub async fn ab_settings() -> Response {
    Json(json!({ "max_peer_one_ab": 0 })).into_response()
}

pub async fn ab_shared_profiles(State(state): State<AppState>, user: RustClientUser) -> Response {
    let mut res: Vec<SharedProfilesPayload> = vec![];
    let u = &user.user;
    let my = services::address_book::collection_list_by_user_id(&state.db, u.id)
        .await
        .unwrap_or_default();
    for ab in &my {
        res.push(SharedProfilesPayload {
            guid: compose_guid(u.group_id, u.id, ab.id),
            name: ab.name.clone(),
            owner: u.username.clone(),
            note: String::new(),
            rule: entity::address_book_collection_rule::RULE_FULL_CONTROL,
        });
    }
    // shared-with-me collections
    let rules = services::address_book::collection_read_rules(&state.db, u.id, u.group_id)
        .await
        .unwrap_or_default();
    let mut max_by_cid: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
    for r in &rules {
        let e = max_by_cid.entry(r.collection_id).or_insert(0);
        if r.rule > *e {
            *e = r.rule;
        }
    }
    let cids: Vec<i32> = max_by_cid.keys().copied().collect();
    let collections = services::address_book::collection_list_by_ids(&state.db, &cids)
        .await
        .unwrap_or_default();
    let owner_ids: Vec<i32> = collections.iter().map(|c| c.user_id).collect();
    let owners = services::user::list_by_ids(&state.db, &owner_ids)
        .await
        .unwrap_or_default();
    let owner_map: std::collections::HashMap<i32, entity::user::Model> =
        owners.into_iter().map(|u| (u.id, u)).collect();
    for c in &collections {
        if let Some(owner) = owner_map.get(&c.user_id) {
            res.push(SharedProfilesPayload {
                guid: compose_guid(owner.group_id, owner.id, c.id),
                name: c.name.clone(),
                owner: owner.username.clone(),
                note: String::new(),
                rule: *max_by_cid.get(&c.id).unwrap_or(&0),
            });
        }
    }
    Json(json!({ "total": 0, "data": res })).into_response()
}

#[derive(Deserialize)]
pub struct AbQuery {
    #[serde(default)]
    pub ab: String,
}

pub async fn ab_peers(
    State(state): State<AppState>,
    user: RustClientUser,
    Query(q): Query<AbQuery>,
) -> Response {
    let (uid, cid) = match check_guid(&state, &user.user, &q.ab).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match services::address_book::can_read(&state.db, user.user.id, user.user.group_id, uid, cid)
        .await
    {
        Ok(true) => {}
        _ => return resp::error("NoAccess"),
    }
    let abs = services::address_book::list_by_user_and_collection(&state.db, uid, cid)
        .await
        .unwrap_or_default();
    Json(json!({
        "total": abs.len(),
        "data": abs,
        "licensed_devices": 99999,
    }))
    .into_response()
}

pub async fn ab_tags(
    State(state): State<AppState>,
    user: RustClientUser,
    Path(guid): Path<String>,
) -> Response {
    let (uid, cid) = match check_guid(&state, &user.user, &guid).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match services::address_book::can_read(&state.db, user.user.id, user.user.group_id, uid, cid)
        .await
    {
        Ok(true) => {}
        _ => return resp::error("NoAccess"),
    }
    let tags = services::tag::list_by_user_and_collection(&state.db, uid, cid)
        .await
        .unwrap_or_default();
    Json(tags).into_response()
}

pub async fn ab_peer_add(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let (uid, cid) = match check_guid(&state, &user.user, &guid).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match services::address_book::can_write(&state.db, user.user.id, user.user.group_id, uid, cid)
        .await
    {
        Ok(true) => {}
        _ => return resp::error("NoAccess"),
    }
    let g = |k: &str| {
        body.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let mut platform = g("platform");
    let mut username = g("username");
    let mut hostname = g("hostname");
    let id = g("id");
    if platform.is_empty() || username.is_empty() || hostname.is_empty() {
        if let Ok(Some(peer)) = services::peer::find_by_id(&state.db, &id).await {
            platform = services::address_book::platform_from_os(&peer.os);
            username = peer.username;
            hostname = peer.hostname;
        }
    }
    let force_relay = match body.get("forceAlwaysRelay") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "true",
        _ => false,
    };
    let tags = body.get("tags").cloned().unwrap_or(Value::Array(vec![]));
    let am = address_book::ActiveModel {
        id: Set(id),
        username: Set(username),
        password: Set(g("password")),
        hostname: Set(hostname),
        alias: Set(g("alias")),
        platform: Set(platform),
        tags: Set(tags),
        hash: Set(g("hash")),
        user_id: Set(uid),
        force_always_relay: Set(force_relay),
        rdp_port: Set(g("rdpPort")),
        rdp_username: Set(g("rdpUsername")),
        login_name: Set(g("loginName")),
        collection_id: Set(cid),
        ..Default::default()
    };
    if let Err(e) = services::address_book::add(&state.db, am).await {
        return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
    }
    "".into_response()
}

pub async fn ab_peer_del(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(ids): Json<Vec<String>>,
) -> Response {
    let (uid, cid) = match check_guid(&state, &user.user, &guid).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match services::address_book::can_full_control(
        &state.db,
        user.user.id,
        user.user.group_id,
        uid,
        cid,
    )
    .await
    {
        Ok(true) => {}
        _ => return resp::error("NoAccess"),
    }
    for id in ids {
        match services::address_book::info_by_user_id_and_id_and_cid(&state.db, uid, &id, cid).await
        {
            Ok(Some(ab)) => {
                if let Err(e) = services::address_book::delete(&state.db, ab.row_id).await {
                    return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
                }
            }
            _ => return resp::error(state.tr(&lang, "ItemNotFound")),
        }
    }
    "".into_response()
}

pub async fn ab_peer_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let (uid, cid) = match check_guid(&state, &user.user, &guid).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match services::address_book::can_write(&state.db, user.user.id, user.user.group_id, uid, cid)
        .await
    {
        Ok(true) => {}
        _ => return resp::error("NoAccess"),
    }
    let Some(id) = body.get("id").and_then(|v| v.as_str()) else {
        return resp::error(state.tr(&lang, "ParamsError"));
    };
    let ab = match services::address_book::info_by_user_id_and_id_and_cid(&state.db, uid, id, cid)
        .await
    {
        Ok(Some(ab)) => ab,
        _ => return resp::error(state.tr(&lang, "ItemNotFound")),
    };
    if let Err(e) = services::address_book::update_fields(&state.db, &ab, &body).await {
        return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
    }
    "".into_response()
}

// ---------- personal tags ----------

#[derive(Deserialize)]
pub struct TagAddForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: i64,
}

pub async fn ab_tag_add(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(t): Json<TagAddForm>,
) -> Response {
    let (uid, cid) = match check_guid(&state, &user.user, &guid).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match services::address_book::can_write(&state.db, user.user.id, user.user.group_id, uid, cid)
        .await
    {
        Ok(true) => {}
        _ => return resp::error("NoAccess"),
    }
    if let Ok(Some(_)) =
        services::tag::info_by_user_name_collection(&state.db, uid, &t.name, cid).await
    {
        return resp::error(state.tr(&lang, "ItemExists"));
    }
    if let Err(e) = services::tag::create(&state.db, &t.name, t.color, uid, cid).await {
        return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
    }
    "".into_response()
}

#[derive(Deserialize)]
pub struct TagRenameForm {
    pub old: String,
    pub new: String,
}

pub async fn ab_tag_rename(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(t): Json<TagRenameForm>,
) -> Response {
    let (uid, cid) = match check_guid(&state, &user.user, &guid).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match services::address_book::can_write(&state.db, user.user.id, user.user.group_id, uid, cid)
        .await
    {
        Ok(true) => {}
        _ => return resp::error("NoAccess"),
    }
    let mut tag =
        match services::tag::info_by_user_name_collection(&state.db, uid, &t.old, cid).await {
            Ok(Some(tag)) => tag,
            _ => return resp::error(state.tr(&lang, "ItemNotFound")),
        };
    if let Ok(Some(_)) =
        services::tag::info_by_user_name_collection(&state.db, uid, &t.new, cid).await
    {
        return resp::error(state.tr(&lang, "ItemExists"));
    }
    tag.name = t.new;
    if let Err(e) = services::tag::update(&state.db, &tag).await {
        return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
    }
    "".into_response()
}

#[derive(Deserialize)]
pub struct TagColorForm {
    pub name: String,
    #[serde(default)]
    pub color: i64,
}

pub async fn ab_tag_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(t): Json<TagColorForm>,
) -> Response {
    let (uid, cid) = match check_guid(&state, &user.user, &guid).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match services::address_book::can_write(&state.db, user.user.id, user.user.group_id, uid, cid)
        .await
    {
        Ok(true) => {}
        _ => return resp::error("NoAccess"),
    }
    let mut tag =
        match services::tag::info_by_user_name_collection(&state.db, uid, &t.name, cid).await {
            Ok(Some(tag)) => tag,
            _ => return resp::error(state.tr(&lang, "ItemNotFound")),
        };
    tag.color = t.color;
    if let Err(e) = services::tag::update(&state.db, &tag).await {
        return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
    }
    "".into_response()
}

pub async fn ab_tag_del(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: RustClientUser,
    Path(guid): Path<String>,
    Json(names): Json<Vec<String>>,
) -> Response {
    let (uid, cid) = match check_guid(&state, &user.user, &guid).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match services::address_book::can_full_control(
        &state.db,
        user.user.id,
        user.user.group_id,
        uid,
        cid,
    )
    .await
    {
        Ok(true) => {}
        _ => return resp::error("NoAccess"),
    }
    for name in names {
        match services::tag::info_by_user_name_collection(&state.db, uid, &name, cid).await {
            Ok(Some(tag)) => {
                if let Err(e) = services::tag::delete(&state.db, tag.id).await {
                    return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
                }
            }
            _ => return resp::error(state.tr(&lang, "ItemNotFound")),
        }
    }
    "".into_response()
}

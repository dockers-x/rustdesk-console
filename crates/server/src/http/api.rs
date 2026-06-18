//! RustDesk PC-client API (`/api/*`), ports of `http/controller/api/*.go`.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Duration, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

use entity::{
    address_book, address_book_collection, address_book_collection_rule, audit_file,
    deployment_event, deployment_token, group, login_log, peer,
};

use crate::http::middleware::{AcceptLang, ClientIp, DeploymentAuth, RustClientUser};
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
    let peer = match services::peer::find_by_id(&state.db, &rustdesk_id).await {
        Ok(peer) => peer,
        Err(e) => {
            tracing::warn!("failed to query heartbeat peer {rustdesk_id}: {e}");
            None
        }
    };
    if let Some(p) = &peer {
        if Utc::now().timestamp() - p.last_online_time >= 30 {
            let am = peer::ActiveModel {
                row_id: Set(p.row_id),
                last_online_time: Set(Utc::now().timestamp()),
                ..Default::default()
            };
            let _ = services::peer::update(&state.db, am).await;
        }
        services::webhook::spawn_device_seen(state.db.clone(), p.clone());
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

    let mut modified_at = info.modified_at;
    let mut strategy = resolve_capability(&normalize_reported_version(&info.version, info.ver));
    if let Some(p) = &peer {
        match services::strategy::effective_for_peer(&state.db, p).await {
            Ok(Some(effective)) => {
                modified_at = effective.modified_at;
                strategy.extra.extend(effective.extra);
                if effective.modified_at != info.modified_at {
                    strategy.config_options = effective.config_options;
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("failed to resolve strategy for {}: {e}", p.id),
        }
    }

    let mut res = json!({
        "modified_at": modified_at,
        "strategy": strategy,
    });
    if let Some(p) = &peer {
        if p.force_sysinfo_refresh {
            res["sysinfo"] = json!(true);
            let am = peer::ActiveModel {
                row_id: Set(p.row_id),
                force_sysinfo_refresh: Set(false),
                ..Default::default()
            };
            if let Err(e) = services::peer::update(&state.db, am).await {
                tracing::warn!("failed to clear sysinfo refresh flag for {}: {e}", p.id);
            }
        }
    }
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
    headers: axum::http::HeaderMap,
    Json(mut form): Json<LoginForm>,
) -> Response {
    if state.config.app.disable_pwd_login {
        return resp::error(state.tr(&lang, "PwdLoginDisabled"));
    }
    if !form.secret.trim().is_empty() {
        let (user, device) = match services::login_security::verify_login_challenge(
            &state.db,
            &form.secret,
            Some(&form.verification_code),
            Some(&form.tfa_code),
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                state.limiter.record_failed_attempt(&ip);
                return resp::error(e);
            }
        };
        if !user.is_enabled() {
            return resp::error(state.tr(&lang, "UserDisabled"));
        }
        let ut = match services::user::login(
            &state.db,
            &state.jwt,
            &state.config,
            &user,
            services::user::LoginEvent {
                client: device.client_type,
                device_id: device.id,
                uuid: device.uuid,
                ip,
                login_type: login_log::TYPE_ACCOUNT.to_string(),
                platform: device.os,
            },
        )
        .await
        {
            Ok(ut) => ut,
            Err(e) => return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
        };
        return Json(LoginRes {
            r#type: "access_token".into(),
            access_token: ut.token,
            user: UserPayload::from_user(&user),
            secret: String::new(),
            tfa_type: String::new(),
        })
        .into_response();
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
    let device = client_login_device_info(&form, &ip);
    match services::login_security::required_verification(&state.db, &user, &device).await {
        Ok(Some(kind)) => {
            let challenge = match services::login_security::create_login_challenge(
                &state.db, &user, &device, kind,
            )
            .await
            {
                Ok(challenge) => challenge,
                Err(e) => return resp::error(e),
            };
            return Json(LoginRes {
                r#type: "email_check".into(),
                access_token: String::new(),
                user: UserPayload::from_user(&user),
                secret: challenge.secret,
                tfa_type: challenge.kind.response_tfa_type().to_string(),
            })
            .into_response();
        }
        Ok(None) => {}
        Err(e) => return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
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

fn client_login_device_info(
    form: &LoginForm,
    ip: &str,
) -> services::login_security::DeviceLoginInfo {
    services::login_security::DeviceLoginInfo {
        id: form.id.clone(),
        uuid: form.uuid.clone(),
        name: form.device_info.name.clone(),
        os: form.device_info.os.clone(),
        client_type: form.device_info.r#type.clone(),
        ip: ip.to_string(),
        auto_login: form.auto_login,
    }
}

pub async fn login_options(State(state): State<AppState>) -> Response {
    let mut ops = services::oauth::get_login_providers(&state.db)
        .await
        .unwrap_or_default();
    if state.config.app.web_sso {
        ops.push(services::oauth::LoginProvider {
            name: "webauth".to_string(),
            oauth_type: entity::oauth::TYPE_WEBAUTH.to_string(),
        });
    }
    let oidc_items: Vec<Map<String, Value>> = ops
        .iter()
        .map(|v| {
            let mut m = Map::new();
            m.insert("name".into(), Value::String(v.name.clone()));
            m
        })
        .collect();
    let common = serde_json::to_string(&oidc_items).unwrap_or_else(|_| "[]".into());
    let mut res = vec![format!("common-oidc/{common}")];
    for v in &ops {
        res.push(format!("oidc/{}", v.name));
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

#[derive(Deserialize, Default, Clone)]
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
    #[serde(default, rename = "preset-user-name")]
    pub preset_user_name: Option<String>,
    #[serde(default, rename = "preset-strategy-name")]
    pub preset_strategy_name: Option<String>,
    #[serde(default, rename = "preset-device-group-name")]
    pub preset_device_group_name: Option<String>,
    #[serde(default, rename = "preset-address-book-name")]
    pub preset_address_book_name: Option<String>,
    #[serde(default, rename = "preset-address-book-tag")]
    pub preset_address_book_tag: Option<String>,
    #[serde(default, rename = "preset-address-book-alias")]
    pub preset_address_book_alias: Option<String>,
    #[serde(default, rename = "preset-address-book-password")]
    pub preset_address_book_password: Option<String>,
    #[serde(default, rename = "preset-address-book-note")]
    pub preset_address_book_note: Option<String>,
    #[serde(default, rename = "preset-device-username")]
    pub preset_device_username: Option<String>,
    #[serde(default, rename = "preset-device-name")]
    pub preset_device_name: Option<String>,
    #[serde(default, rename = "preset-note")]
    pub preset_note: Option<String>,
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
    let preset_user_id = resolve_sysinfo_preset_user_id(&state, &f).await;
    let preset_group_id = resolve_sysinfo_preset_device_group_id(&state, &f).await;
    let saved_peer = match existing {
        None => {
            let user_id = match preset_user_id {
                Some(user_id) => user_id,
                None => services::user::find_latest_user_id_by_uuid(&state.db, &f.uuid, &f.id)
                    .await
                    .unwrap_or(0),
            };
            let am = peer::ActiveModel {
                id: Set(f.id.clone()),
                cpu: Set(f.cpu.clone()),
                hostname: Set(sysinfo_hostname(&f)),
                memory: Set(f.memory.clone()),
                os: Set(f.os.clone()),
                username: Set(sysinfo_username(&f)),
                uuid: Set(f.uuid.clone()),
                version: Set(f.version.clone()),
                user_id: Set(user_id),
                group_id: Set(preset_group_id.unwrap_or(0)),
                alias: Set(option_text(&f.preset_note).unwrap_or_default()),
                ..Default::default()
            };
            match services::peer::create(&state.db, am).await {
                Ok(peer) => peer,
                Err(e) => {
                    return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e))
                }
            }
        }
        Some(p) => {
            let user_id = if let Some(user_id) = preset_user_id {
                user_id
            } else if p.user_id == 0 {
                services::user::find_latest_user_id_by_uuid(&state.db, &f.uuid, &f.id)
                    .await
                    .unwrap_or(0)
            } else {
                p.user_id
            };
            let mut am = peer::ActiveModel {
                row_id: Set(p.row_id),
                id: Set(f.id.clone()),
                cpu: Set(f.cpu.clone()),
                hostname: Set(sysinfo_hostname(&f)),
                memory: Set(f.memory.clone()),
                os: Set(f.os.clone()),
                username: Set(sysinfo_username(&f)),
                uuid: Set(f.uuid.clone()),
                version: Set(f.version.clone()),
                user_id: Set(user_id),
                ..Default::default()
            };
            if let Some(group_id) = preset_group_id {
                am.group_id = Set(group_id);
            }
            if let Some(note) = option_text(&f.preset_note) {
                am.alias = Set(note);
            }
            if let Err(e) = services::peer::update(&state.db, am).await {
                return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
            }
            match services::peer::find_by_id(&state.db, &f.id).await {
                Ok(Some(peer)) => peer,
                Ok(None) => return resp::error(state.tr(&lang, "ItemNotFound")),
                Err(e) => {
                    return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e))
                }
            }
        }
    };
    apply_sysinfo_preset_assignment(&state, &f, &saved_peer).await;
    "SYSINFO_UPDATED".into_response()
}

fn sysinfo_hostname(f: &PeerForm) -> String {
    option_text(&f.preset_device_name).unwrap_or_else(|| f.hostname.clone())
}

fn sysinfo_username(f: &PeerForm) -> String {
    option_text(&f.preset_device_username).unwrap_or_else(|| f.username.clone())
}

async fn resolve_sysinfo_preset_user_id(state: &AppState, f: &PeerForm) -> Option<i32> {
    let name = option_text(&f.preset_user_name)?;
    match services::user::info_by_username(&state.db, &name).await {
        Ok(Some(user)) => Some(user.id),
        Ok(None) => {
            tracing::warn!("sysinfo preset user not found: {name}");
            None
        }
        Err(e) => {
            tracing::warn!("failed to resolve sysinfo preset user {name}: {e}");
            None
        }
    }
}

async fn resolve_sysinfo_preset_device_group_id(state: &AppState, f: &PeerForm) -> Option<i32> {
    let name = option_text(&f.preset_device_group_name)?;
    match services::group::device_group_info_by_name(&state.db, &name).await {
        Ok(Some(group)) => Some(group.id),
        Ok(None) => {
            tracing::warn!("sysinfo preset device group not found: {name}");
            None
        }
        Err(e) => {
            tracing::warn!("failed to resolve sysinfo preset device group {name}: {e}");
            None
        }
    }
}

async fn apply_sysinfo_preset_assignment(state: &AppState, f: &PeerForm, peer: &peer::Model) {
    let target_user = resolve_sysinfo_target_user(state, f, peer).await;

    if let Some(address_book_name) = option_text(&f.preset_address_book_name) {
        match &target_user {
            Some(user) => {
                let cli = DeviceCliForm {
                    id: f.id.clone(),
                    uuid: f.uuid.clone(),
                    address_book_name: Some(address_book_name.clone()),
                    address_book_tag: f.preset_address_book_tag.clone(),
                    address_book_alias: f.preset_address_book_alias.clone(),
                    address_book_password: f.preset_address_book_password.clone(),
                    address_book_note: f.preset_address_book_note.clone(),
                    ..Default::default()
                };
                if let Err(e) =
                    assign_peer_to_address_book(state, user, peer, &address_book_name, &cli).await
                {
                    tracing::warn!(
                        "failed to apply sysinfo preset address book {} for {}: {}",
                        address_book_name,
                        peer.id,
                        e
                    );
                }
            }
            None => tracing::warn!(
                "skip sysinfo preset address book {} for {}: target user not resolved",
                address_book_name,
                peer.id
            ),
        }
    }

    if let Some(strategy_name) = option_text(&f.preset_strategy_name) {
        if let Err(e) =
            services::strategy::assign_peer_by_strategy_name(&state.db, &peer.id, &strategy_name)
                .await
        {
            tracing::warn!(
                "failed to apply sysinfo preset strategy {} for {}: {}",
                strategy_name,
                peer.id,
                e
            );
        }
    }
}

async fn resolve_sysinfo_target_user(
    state: &AppState,
    f: &PeerForm,
    peer: &peer::Model,
) -> Option<entity::user::Model> {
    if let Some(name) = option_text(&f.preset_user_name) {
        match services::user::info_by_username(&state.db, &name).await {
            Ok(Some(user)) => return Some(user),
            Ok(None) => tracing::warn!("sysinfo preset user not found: {name}"),
            Err(e) => tracing::warn!("failed to resolve sysinfo preset user {name}: {e}"),
        }
    }
    if peer.user_id <= 0 {
        return None;
    }
    match services::user::info_by_id(&state.db, peer.user_id).await {
        Ok(user) => user,
        Err(e) => {
            tracing::warn!("failed to resolve sysinfo peer user {}: {e}", peer.user_id);
            None
        }
    }
}

pub async fn sysinfo_ver(State(state): State<AppState>) -> Response {
    format!("{}\n{}", state.version, state.start_time).into_response()
}

// ---------- group ----------

#[derive(Deserialize)]
pub struct PageQuery {
    #[serde(default)]
    #[serde(alias = "current")]
    pub page: Option<u64>,
    #[serde(default, rename = "pageSize")]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub group_name: Option<String>,
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
    } else if u.is_admin() {
        let r = services::user::list(
            &state.db,
            q.page.unwrap_or(1),
            q.page_size.unwrap_or(10),
            q.name.map(strip_like),
        )
        .await
        .unwrap_or(services::user::UserListResult {
            list: vec![],
            page: 1,
            page_size: 10,
            total: 0,
        });
        (r.list, r.total)
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
    let group_names: std::collections::HashMap<i32, String> =
        services::group::list(&state.db, 1, 9999)
            .await
            .map(|r| r.list.into_iter().map(|g| (g.id, g.name)).collect())
            .unwrap_or_default();
    let mut data: Vec<UserPayload> = users
        .iter()
        .map(|u| {
            let mut payload = UserPayload::from_user(u);
            payload.group_name = group_names.get(&u.group_id).cloned().unwrap_or_default();
            payload
        })
        .collect();
    if let Some(group_name) = q
        .group_name
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        data.retain(|u| matches_ab_filter(&u.group_name, Some(group_name)));
    }
    let total = if q
        .group_name
        .as_deref()
        .is_some_and(|v| !v.trim().is_empty())
    {
        data.len() as i64
    } else {
        total
    };
    resp::data_response(total, data)
}

fn strip_like(value: String) -> String {
    value.trim().trim_matches('%').to_string()
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
    ClientIp(ip): ClientIp,
    auth: DeploymentAuth,
    Json(f): Json<DeviceDeployForm>,
) -> Response {
    if f.id.trim().is_empty() || f.uuid.trim().is_empty() {
        let _ = record_deployment_event(
            &state,
            &auth,
            deployment_event::ACTION_DEPLOY,
            &f.id,
            &f.uuid,
            "INVALID_INPUT",
            "missing id or uuid",
            &ip,
        )
        .await;
        return Json(json!({ "result": "INVALID_INPUT" })).into_response();
    }
    if !auth.has_scope(deployment_token::SCOPE_DEPLOY) {
        let _ = record_deployment_event(
            &state,
            &auth,
            deployment_event::ACTION_DEPLOY,
            &f.id,
            &f.uuid,
            "UNAUTHORIZED",
            "deployment token lacks deploy scope",
            &ip,
        )
        .await;
        return Json(json!({ "error": "Unauthorized" })).into_response();
    }
    match upsert_deployed_peer(&state, &auth, None, &f.id, &f.uuid, Some(&f.pk), None).await {
        Ok(peer) => {
            if let Some(strategy_id) = auth
                .deployment_token
                .as_ref()
                .map(|token| token.default_strategy_id)
                .filter(|id| *id > 0)
            {
                let _ = services::strategy::assign(
                    &state.db,
                    strategy_id,
                    entity::strategy_assignment::TARGET_PEER,
                    &peer.id,
                    services::strategy::DEFAULT_PRIORITY,
                )
                .await;
            }
            if auth.is_deployment_token() {
                let _ = services::deployment::increment_used(&state.db, auth.deployment_token_id())
                    .await;
            }
            let _ = record_deployment_event(
                &state,
                &auth,
                deployment_event::ACTION_DEPLOY,
                &f.id,
                &f.uuid,
                "OK",
                "",
                &ip,
            )
            .await;
            Json(json!({ "result": "OK" })).into_response()
        }
        Err(DeviceDeployError::IdTaken) => {
            let _ = record_deployment_event(
                &state,
                &auth,
                deployment_event::ACTION_DEPLOY,
                &f.id,
                &f.uuid,
                "ID_TAKEN",
                "device id belongs to another uuid",
                &ip,
            )
            .await;
            Json(json!({ "result": "ID_TAKEN" })).into_response()
        }
        Err(DeviceDeployError::InvalidInput) => {
            let _ = record_deployment_event(
                &state,
                &auth,
                deployment_event::ACTION_DEPLOY,
                &f.id,
                &f.uuid,
                "INVALID_INPUT",
                "invalid device deployment input",
                &ip,
            )
            .await;
            Json(json!({ "result": "INVALID_INPUT" })).into_response()
        }
        Err(DeviceDeployError::Other(e)) => {
            let _ = record_deployment_event(
                &state,
                &auth,
                deployment_event::ACTION_DEPLOY,
                &f.id,
                &f.uuid,
                "ERROR",
                &e,
                &ip,
            )
            .await;
            Json(json!({ "error": e })).into_response()
        }
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
    ClientIp(ip): ClientIp,
    auth: DeploymentAuth,
    Json(f): Json<DeviceCliForm>,
) -> Response {
    if !auth.has_scope(deployment_token::SCOPE_ASSIGN) {
        let _ = record_deployment_event(
            &state,
            &auth,
            deployment_event::ACTION_ASSIGN,
            &f.id,
            &f.uuid,
            "UNAUTHORIZED",
            "deployment token lacks assign scope",
            &ip,
        )
        .await;
        return "UNAUTHORIZED".into_response();
    }
    match apply_device_cli_assignment(&state, &auth, f).await {
        Ok(message) => {
            if auth.is_deployment_token() {
                let _ = services::deployment::increment_used(&state.db, auth.deployment_token_id())
                    .await;
            }
            let _ = record_deployment_event(
                &state,
                &auth,
                deployment_event::ACTION_ASSIGN,
                "",
                "",
                "OK",
                "",
                &ip,
            )
            .await;
            message.into_response()
        }
        Err(message) => {
            let _ = record_deployment_event(
                &state,
                &auth,
                deployment_event::ACTION_ASSIGN,
                "",
                "",
                "ERROR",
                &message,
                &ip,
            )
            .await;
            message.into_response()
        }
    }
}

async fn apply_device_cli_assignment(
    state: &AppState,
    auth: &DeploymentAuth,
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
        None => resolve_deployment_default_user(state, auth).await?,
    };
    let device_group_id = match option_text(&f.device_group_name) {
        Some(name) => Some(
            services::group::device_group_info_by_name(&state.db, &name)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Device group not found: {name}"))?
                .id,
        ),
        None => auth
            .deployment_token
            .as_ref()
            .map(|token| token.default_device_group_id)
            .filter(|id| *id > 0),
    };
    let mut peer = upsert_deployed_peer(
        state,
        auth,
        Some(&target_user),
        &f.id,
        &f.uuid,
        None,
        device_group_id,
    )
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
        if !auth.has_scope(deployment_token::SCOPE_ADDRESS_BOOK_ASSIGN) {
            return Err("UNAUTHORIZED".to_string());
        }
        assign_peer_to_address_book(state, &target_user, &peer, &address_book_name, &f).await?;
    }

    if let Some(strategy_name) = option_text(&f.strategy_name) {
        if !auth.has_scope(deployment_token::SCOPE_STRATEGY_ASSIGN) {
            return Err("UNAUTHORIZED".to_string());
        }
        services::strategy::assign_peer_by_strategy_name(&state.db, &peer.id, &strategy_name)
            .await
            .map_err(|e| e.to_string())?;
    } else if let Some(strategy_id) = auth
        .deployment_token
        .as_ref()
        .map(|token| token.default_strategy_id)
        .filter(|id| *id > 0)
    {
        services::strategy::assign(
            &state.db,
            strategy_id,
            entity::strategy_assignment::TARGET_PEER,
            &peer.id,
            services::strategy::DEFAULT_PRIORITY,
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(String::new())
}

async fn upsert_deployed_peer(
    state: &AppState,
    auth: &DeploymentAuth,
    user_override: Option<&entity::user::Model>,
    id: &str,
    uuid: &str,
    pk: Option<&str>,
    device_group_id: Option<i32>,
) -> Result<peer::Model, DeviceDeployError> {
    let id = id.trim();
    let uuid = uuid.trim();
    if id.is_empty() || uuid.is_empty() {
        return Err(DeviceDeployError::InvalidInput);
    }
    let pk = pk.map(str::trim).filter(|value| !value.is_empty());
    let user = match user_override {
        Some(user) => user.clone(),
        None => resolve_deployment_default_user(state, auth)
            .await
            .map_err(DeviceDeployError::Other)?,
    };
    let group_id = device_group_id
        .or_else(|| {
            auth.deployment_token
                .as_ref()
                .map(|token| token.default_device_group_id)
                .filter(|id| *id > 0)
        })
        .unwrap_or(0);
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
            if let Some(pk) = pk {
                p.pk = pk.to_string();
            }
            if p.guid.is_empty() {
                p.guid = uuid::Uuid::new_v4().to_string();
            }
            if device_group_id.is_some() {
                p.group_id = group_id;
            }
            let am = peer::ActiveModel {
                row_id: Set(p.row_id),
                id: Set(p.id.clone()),
                uuid: Set(p.uuid.clone()),
                pk: Set(p.pk.clone()),
                guid: Set(p.guid.clone()),
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
                pk: Set(pk.unwrap_or_default().to_string()),
                guid: Set(uuid::Uuid::new_v4().to_string()),
                user_id: Set(user.id),
                group_id: Set(group_id),
                status: Set(entity::user::STATUS_ENABLE),
                ..Default::default()
            };
            services::peer::create(&state.db, p)
                .await
                .map_err(|e| DeviceDeployError::Other(e.to_string()))
        }
    }
}

async fn resolve_deployment_default_user(
    state: &AppState,
    auth: &DeploymentAuth,
) -> Result<entity::user::Model, String> {
    if let Some(user) = &auth.user {
        return Ok(user.clone());
    }
    let Some(token) = &auth.deployment_token else {
        return Err("Unauthorized".to_string());
    };
    let user_id = token.default_user_id;
    if user_id <= 0 {
        return Err("deployment token has no default user".to_string());
    }
    services::user::info_by_id(&state.db, user_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "deployment token default user not found".to_string())
}

async fn record_deployment_event(
    state: &AppState,
    auth: &DeploymentAuth,
    action: &str,
    peer_id: &str,
    uuid: &str,
    result: &str,
    message: &str,
    ip: &str,
) -> Result<(), sea_orm::DbErr> {
    services::deployment::record_event(
        &state.db,
        auth.deployment_token_id(),
        peer_id,
        uuid,
        action,
        result,
        message,
        ip,
    )
    .await
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

fn script_page(page: Option<u64>, page_size: Option<u64>) -> (u64, u64) {
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size.unwrap_or(30).max(1);
    (page, page_size)
}

fn matches_ab_filter(value: &str, pattern: Option<&str>) -> bool {
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

async fn full_control_collection(
    state: &AppState,
    cur: &entity::user::Model,
    guid: &str,
) -> Result<(entity::user::Model, address_book_collection::Model), Response> {
    let (uid, cid) = check_guid(state, cur, guid).await?;
    if cid == 0 {
        return Err(resp::error("ParamsError"));
    }
    let collection = match services::address_book::collection_info_by_id(&state.db, cid).await {
        Ok(Some(row)) => row,
        _ => return Err(resp::error("ItemNotFound")),
    };
    if !cur.is_admin() {
        match services::address_book::can_full_control(&state.db, cur.id, cur.group_id, uid, cid)
            .await
        {
            Ok(true) => {}
            _ => return Err(resp::error("NoAccess")),
        }
    }
    let owner = match services::user::info_by_id(&state.db, uid).await {
        Ok(Some(row)) => row,
        _ => return Err(resp::error("ItemNotFound")),
    };
    Ok((owner, collection))
}

async fn can_control_collection_id(
    state: &AppState,
    cur: &entity::user::Model,
    collection_id: i32,
) -> bool {
    let collection =
        match services::address_book::collection_info_by_id(&state.db, collection_id).await {
            Ok(Some(row)) => row,
            _ => return false,
        };
    cur.is_admin()
        || services::address_book::can_full_control(
            &state.db,
            cur.id,
            cur.group_id,
            collection.user_id,
            collection.id,
        )
        .await
        .unwrap_or(false)
}

async fn resolve_rule_target(state: &AppState, f: &AbRuleForm) -> Result<(i32, i32), Response> {
    if !matches!(
        f.rule,
        address_book_collection_rule::RULE_READ
            | address_book_collection_rule::RULE_READ_WRITE
            | address_book_collection_rule::RULE_FULL_CONTROL
    ) {
        return Err(resp::error("ParamsError"));
    }
    let user_name = f.user.trim();
    let group_name = f.group.trim();
    if !user_name.is_empty() && !group_name.is_empty() {
        return Err(resp::error("ParamsError"));
    }
    if !user_name.is_empty() {
        let user = match services::user::info_by_username(&state.db, user_name).await {
            Ok(Some(row)) => row,
            _ => return Err(resp::error("user not found")),
        };
        return Ok((address_book_collection_rule::RULE_TYPE_PERSONAL, user.id));
    }
    if !group_name.is_empty() {
        let group = match group::Entity::find()
            .filter(group::Column::Name.eq(group_name))
            .one(&state.db)
            .await
        {
            Ok(Some(row)) => row,
            _ => return Err(resp::error("group not found")),
        };
        return Ok((address_book_collection_rule::RULE_TYPE_GROUP, group.id));
    }
    Ok((address_book_collection_rule::RULE_TYPE_GROUP, 0))
}

fn parse_i32(value: &str) -> Option<i32> {
    value.trim().parse::<i32>().ok()
}

pub async fn ab_personal(State(state): State<AppState>, user: RustClientUser) -> Response {
    if state.config.rustdesk.personal != 1 {
        return StatusCode::NOT_FOUND.into_response();
    }

    let cid = services::address_book::client_personal_collection_id(&state.db, user.user.id)
        .await
        .unwrap_or(0);
    let guid = compose_guid(user.user.group_id, user.user.id, cid);
    Json(json!({ "guid": guid, "name": user.user.username, "rule": 3 })).into_response()
}

pub async fn ab_settings() -> Response {
    Json(json!({ "max_peer_one_ab": 0 })).into_response()
}

#[derive(Deserialize, Default)]
pub struct AbProfilesQuery {
    #[serde(default)]
    #[serde(alias = "current")]
    pub page: Option<u64>,
    #[serde(default, rename = "pageSize")]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
}

pub async fn ab_shared_profiles(
    State(state): State<AppState>,
    user: RustClientUser,
    Query(q): Query<AbProfilesQuery>,
) -> Response {
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
        if c.user_id == u.id {
            continue;
        }
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
    let mut res: Vec<SharedProfilesPayload> = res
        .into_iter()
        .filter(|profile| matches_ab_filter(&profile.name, q.name.as_deref()))
        .collect();
    let total = res.len() as i64;
    let (page, page_size) = script_page(q.page, q.page_size);
    let start = ((page - 1) * page_size) as usize;
    res = res
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect();
    Json(json!({ "total": total, "data": res })).into_response()
}

#[derive(Deserialize, Default)]
pub struct SharedAbForm {
    #[serde(default)]
    pub guid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub info: Value,
}

pub async fn ab_shared_add(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<SharedAbForm>,
) -> Response {
    let name = f.name.trim();
    if name.is_empty() {
        return resp::error("ParamsError");
    }
    let _ = (&f.note, &f.info);
    match services::address_book::create_collection(&state.db, user.user.id, name).await {
        Ok(row) => Json(json!({
            "guid": compose_guid(user.user.group_id, user.user.id, row.id),
            "name": row.name,
        }))
        .into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

pub async fn ab_shared_update_profile(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<SharedAbForm>,
) -> Response {
    let (owner, collection) = match full_control_collection(&state, &user.user, &f.guid).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let new_name = if f.name.trim().is_empty() {
        collection.name.clone()
    } else {
        f.name.trim().to_string()
    };
    let new_owner = if f.owner.trim().is_empty() || f.owner.trim() == owner.username {
        owner.clone()
    } else {
        if !user.user.is_admin() {
            return resp::error("NoAccess");
        }
        match services::user::info_by_username(&state.db, f.owner.trim()).await {
            Ok(Some(row)) => row,
            _ => return resp::error("owner not found"),
        }
    };
    if let Err(e) =
        services::address_book::update_collection(&state.db, collection.id, new_owner.id, &new_name)
            .await
    {
        return resp::error(e.to_string());
    }
    if let Err(e) = services::address_book::transfer_collection_owner(
        &state.db,
        collection.id,
        owner.id,
        new_owner.id,
    )
    .await
    {
        return resp::error(e.to_string());
    }
    let _ = (&f.note, &f.info);
    Json(json!({
        "guid": compose_guid(new_owner.group_id, new_owner.id, collection.id),
        "name": new_name,
    }))
    .into_response()
}

pub async fn ab_shared_delete(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(guids): Json<Vec<String>>,
) -> Response {
    for guid in guids {
        let (_, collection) = match full_control_collection(&state, &user.user, &guid).await {
            Ok(v) => v,
            Err(r) => return r,
        };
        if let Err(e) = services::address_book::delete_collection(&state.db, collection.id).await {
            return resp::error(e.to_string());
        }
    }
    "".into_response()
}

#[derive(Deserialize)]
pub struct AbQuery {
    #[serde(default)]
    pub ab: String,
}

#[derive(Deserialize, Default)]
pub struct AbPeersQuery {
    #[serde(default)]
    pub ab: String,
    #[serde(default)]
    #[serde(alias = "current")]
    pub page: Option<u64>,
    #[serde(default, rename = "pageSize")]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
}

pub async fn ab_peers(
    State(state): State<AppState>,
    user: RustClientUser,
    Query(q): Query<AbPeersQuery>,
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
    let mut abs = services::address_book::list_by_user_and_collection(&state.db, uid, cid)
        .await
        .unwrap_or_default();
    abs = dedup_address_book_peers(abs);
    abs.retain(|ab| {
        matches_ab_filter(&ab.id, q.id.as_deref())
            && matches_ab_filter(&ab.alias, q.alias.as_deref())
    });
    let total = abs.len() as i64;
    let (page, page_size) = script_page(q.page, q.page_size);
    let start = ((page - 1) * page_size) as usize;
    let abs: Vec<address_book::Model> = abs
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect();
    Json(json!({
        "total": total,
        "data": abs,
        "licensed_devices": 99999,
    }))
    .into_response()
}

fn address_book_peer_is_newer(
    candidate: &address_book::Model,
    current: &address_book::Model,
) -> bool {
    match (candidate.updated_at.as_ref(), current.updated_at.as_ref()) {
        (Some(a), Some(b)) => a > b || (a == b && candidate.row_id > current.row_id),
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => candidate.row_id > current.row_id,
    }
}

fn prefer_peer_text(current: &mut String, candidate: String, candidate_is_newer: bool) {
    if !candidate.trim().is_empty() && (current.trim().is_empty() || candidate_is_newer) {
        *current = candidate;
    }
}

fn address_book_tags_non_empty(tags: &Value) -> bool {
    matches!(tags, Value::Array(items) if !items.is_empty())
}

fn merge_duplicate_address_book_peer(
    current: &mut address_book::Model,
    candidate: address_book::Model,
) {
    let candidate_is_newer = address_book_peer_is_newer(&candidate, current);
    let address_book::Model {
        row_id,
        id: _,
        username,
        password,
        hostname,
        alias,
        platform,
        tags,
        hash,
        user_id,
        force_always_relay,
        rdp_port,
        rdp_username,
        online,
        login_name,
        same_server,
        collection_id,
        created_at,
        updated_at,
    } = candidate;

    if candidate_is_newer {
        current.row_id = row_id;
        current.user_id = user_id;
        current.force_always_relay = force_always_relay;
        current.online = online;
        current.same_server = same_server;
        current.collection_id = collection_id;
        current.created_at = created_at;
        current.updated_at = updated_at;
    }

    prefer_peer_text(&mut current.username, username, candidate_is_newer);
    prefer_peer_text(&mut current.password, password, candidate_is_newer);
    prefer_peer_text(&mut current.hostname, hostname, candidate_is_newer);
    prefer_peer_text(&mut current.alias, alias, candidate_is_newer);
    prefer_peer_text(&mut current.platform, platform, candidate_is_newer);
    prefer_peer_text(&mut current.hash, hash, candidate_is_newer);
    prefer_peer_text(&mut current.rdp_port, rdp_port, candidate_is_newer);
    prefer_peer_text(&mut current.rdp_username, rdp_username, candidate_is_newer);
    prefer_peer_text(&mut current.login_name, login_name, candidate_is_newer);
    if address_book_tags_non_empty(&tags)
        && (!address_book_tags_non_empty(&current.tags) || candidate_is_newer)
    {
        current.tags = tags;
    }
}

fn dedup_address_book_peers(abs: Vec<address_book::Model>) -> Vec<address_book::Model> {
    let mut positions: HashMap<String, usize> = HashMap::new();
    let mut peers = Vec::new();
    for ab in abs {
        if let Some(pos) = positions.get(&ab.id).copied() {
            merge_duplicate_address_book_peer(&mut peers[pos], ab);
        } else {
            positions.insert(ab.id.clone(), peers.len());
            peers.push(ab);
        }
    }
    peers
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
    let peer = address_book::Model {
        row_id: 0,
        id,
        username,
        password: g("password"),
        hostname,
        alias: g("alias"),
        platform,
        tags,
        hash: g("hash"),
        user_id: uid,
        force_always_relay: force_relay,
        rdp_port: g("rdpPort"),
        rdp_username: g("rdpUsername"),
        online: false,
        login_name: g("loginName"),
        same_server: false,
        collection_id: cid,
        created_at: None,
        updated_at: None,
    };
    if let Err(e) = services::address_book::add_or_update(&state.db, peer).await {
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
        match services::address_book::delete_by_identity(&state.db, uid, &id, cid).await {
            Ok(n) if n > 0 => {}
            Ok(_) => return resp::error(state.tr(&lang, "ItemNotFound")),
            Err(e) => return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
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
    match services::address_book::update_fields_by_identity(&state.db, uid, id, cid, &body).await {
        Ok(true) => {}
        Ok(false) => return resp::error(state.tr(&lang, "ItemNotFound")),
        Err(e) => return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
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

#[derive(Deserialize, Default)]
pub struct AbRulesQuery {
    #[serde(default)]
    pub ab: String,
    #[serde(default)]
    #[serde(alias = "current")]
    pub page: Option<u64>,
    #[serde(default, rename = "pageSize")]
    pub page_size: Option<u64>,
}

#[derive(Serialize)]
struct AbRulePayload {
    guid: String,
    #[serde(rename = "type")]
    rule_type: String,
    user: String,
    group: String,
    rule: i32,
}

pub async fn ab_rules(
    State(state): State<AppState>,
    user: RustClientUser,
    Query(q): Query<AbRulesQuery>,
) -> Response {
    let (_, collection) = match full_control_collection(&state, &user.user, &q.ab).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let (page, page_size) = script_page(q.page, q.page_size);
    let rules = match services::address_book::rules_by_collection(
        &state.db,
        collection.id,
        page,
        page_size,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return resp::error(e.to_string()),
    };
    let user_ids: Vec<i32> = rules
        .list
        .iter()
        .filter(|r| r.r#type == address_book_collection_rule::RULE_TYPE_PERSONAL)
        .map(|r| r.to_id)
        .collect();
    let users = services::user::list_by_ids(&state.db, &user_ids)
        .await
        .unwrap_or_default();
    let user_names: std::collections::HashMap<i32, String> =
        users.into_iter().map(|u| (u.id, u.username)).collect();
    let groups = services::group::list(&state.db, 1, 9999)
        .await
        .map(|r| r.list)
        .unwrap_or_default();
    let group_names: std::collections::HashMap<i32, String> =
        groups.into_iter().map(|g| (g.id, g.name)).collect();
    let data: Vec<AbRulePayload> = rules
        .list
        .into_iter()
        .map(|r| {
            let (rule_type, user, group) =
                if r.r#type == address_book_collection_rule::RULE_TYPE_PERSONAL {
                    (
                        "user".to_string(),
                        user_names.get(&r.to_id).cloned().unwrap_or_default(),
                        String::new(),
                    )
                } else if r.to_id == 0 {
                    (
                        "everyone".to_string(),
                        String::new(),
                        "everyone".to_string(),
                    )
                } else {
                    (
                        "group".to_string(),
                        String::new(),
                        group_names.get(&r.to_id).cloned().unwrap_or_default(),
                    )
                };
            AbRulePayload {
                guid: r.id.to_string(),
                rule_type,
                user,
                group,
                rule: r.rule,
            }
        })
        .collect();
    resp::data_response(rules.total, data)
}

#[derive(Deserialize, Default)]
pub struct AbRuleForm {
    #[serde(default)]
    pub guid: String,
    #[serde(default)]
    pub rule: i32,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub group: String,
}

pub async fn ab_rule_add(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<AbRuleForm>,
) -> Response {
    let (owner, collection) = match full_control_collection(&state, &user.user, &f.guid).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let (rule_type, to_id) = match resolve_rule_target(&state, &f).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let mut row = match services::address_book::rule_info_by_type_to_cid(
        &state.db,
        rule_type,
        to_id,
        collection.id,
    )
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => address_book_collection_rule::Model {
            id: 0,
            user_id: owner.id,
            collection_id: collection.id,
            rule: f.rule,
            r#type: rule_type,
            to_id,
            created_at: None,
            updated_at: None,
        },
        Err(e) => return resp::error(e.to_string()),
    };
    row.rule = f.rule;
    row.user_id = owner.id;
    let saved = if row.id > 0 {
        match services::address_book::update_rule(&state.db, &row).await {
            Ok(_) => row,
            Err(e) => return resp::error(e.to_string()),
        }
    } else {
        match services::address_book::create_rule(&state.db, &row).await {
            Ok(row) => row,
            Err(e) => return resp::error(e.to_string()),
        }
    };
    Json(json!({ "guid": saved.id.to_string(), "rule": saved.rule })).into_response()
}

pub async fn ab_rule_update(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(f): Json<AbRuleForm>,
) -> Response {
    let Some(rule_id) = parse_i32(&f.guid) else {
        return resp::error("ParamsError");
    };
    let mut rule = match services::address_book::rule_info_by_id(&state.db, rule_id).await {
        Ok(Some(row)) => row,
        _ => return resp::error("ItemNotFound"),
    };
    if !can_control_collection_id(&state, &user.user, rule.collection_id).await {
        return resp::error("NoAccess");
    }
    rule.rule = f.rule;
    match services::address_book::update_rule(&state.db, &rule).await {
        Ok(_) => "".into_response(),
        Err(e) => resp::error(e.to_string()),
    }
}

pub async fn ab_rules_delete(
    State(state): State<AppState>,
    user: RustClientUser,
    Json(guids): Json<Vec<String>>,
) -> Response {
    for guid in guids {
        let Some(rule_id) = parse_i32(&guid) else {
            return resp::error("ParamsError");
        };
        let rule = match services::address_book::rule_info_by_id(&state.db, rule_id).await {
            Ok(Some(row)) => row,
            _ => return resp::error("ItemNotFound"),
        };
        if !can_control_collection_id(&state, &user.user, rule.collection_id).await {
            return resp::error("NoAccess");
        }
        if let Err(e) = services::address_book::delete_rule(&state.db, rule.id).await {
            return resp::error(e.to_string());
        }
    }
    "".into_response()
}

#[cfg(test)]
mod pagination_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn client_page_query_accepts_current_and_page_size_aliases() {
        let q: PageQuery = serde_json::from_value(json!({
            "current": 4,
            "pageSize": 12,
            "name": "alice",
            "group_name": "support"
        }))
        .unwrap();

        assert_eq!(q.page, Some(4));
        assert_eq!(q.page_size, Some(12));
        assert_eq!(q.name.as_deref(), Some("alice"));
        assert_eq!(q.group_name.as_deref(), Some("support"));
    }

    #[test]
    fn shared_address_book_queries_accept_script_pagination_aliases() {
        let profiles: AbProfilesQuery = serde_json::from_value(json!({
            "current": 2,
            "pageSize": 20,
            "name": "team"
        }))
        .unwrap();
        let peers: AbPeersQuery = serde_json::from_value(json!({
            "ab": "1-2-3",
            "current": 3,
            "pageSize": 15,
            "alias": "laptop"
        }))
        .unwrap();
        let rules: AbRulesQuery = serde_json::from_value(json!({
            "ab": "1-2-3",
            "current": 5,
            "pageSize": 8
        }))
        .unwrap();

        assert_eq!(profiles.page, Some(2));
        assert_eq!(profiles.page_size, Some(20));
        assert_eq!(profiles.name.as_deref(), Some("team"));
        assert_eq!(peers.ab, "1-2-3");
        assert_eq!(peers.page, Some(3));
        assert_eq!(peers.page_size, Some(15));
        assert_eq!(peers.alias.as_deref(), Some("laptop"));
        assert_eq!(rules.ab, "1-2-3");
        assert_eq!(rules.page, Some(5));
        assert_eq!(rules.page_size, Some(8));
    }

    #[test]
    fn script_page_defaults_and_clamps_to_positive_values() {
        assert_eq!(script_page(None, None), (1, 30));
        assert_eq!(script_page(Some(0), Some(0)), (1, 1));
    }

    #[test]
    fn dedup_address_book_peers_preserves_non_empty_alias() {
        let ts = |day| {
            chrono::NaiveDate::from_ymd_opt(2026, 6, day)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
        };
        let peer = |row_id, alias: &str, updated_at| address_book::Model {
            row_id,
            id: "1380385931".to_string(),
            username: "czyt".to_string(),
            password: String::new(),
            hostname: "hpdev".to_string(),
            alias: alias.to_string(),
            platform: "Windows".to_string(),
            tags: serde_json::json!([]),
            hash: String::new(),
            user_id: 1,
            force_always_relay: false,
            rdp_port: String::new(),
            rdp_username: String::new(),
            online: false,
            login_name: String::new(),
            same_server: false,
            collection_id: 1,
            created_at: None,
            updated_at: Some(updated_at),
        };

        let peers =
            dedup_address_book_peers(vec![peer(1, "公司开发台式机", ts(17)), peer(3, "", ts(18))]);

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].alias, "公司开发台式机");
        assert_eq!(peers[0].row_id, 3);
    }
}

#[cfg(test)]
mod deployment_tests {
    use super::*;
    use crate::config::{Config, RecordStorage};
    use crate::i18n::I18n;
    use crate::support::admin_config::{AdminConfigStore, AdminPanelConfig};
    use crate::support::disconnect_store::DisconnectStore;
    use crate::support::jwt::Jwt;
    use crate::support::login_limiter::{LoginLimiter, SecurityPolicy};
    use crate::support::oauth_cache::OauthCache;
    use crate::support::record_storage_config::RecordStorageConfigStore;
    use crate::support::webclient_config::{WebClientConfig, WebClientConfigStore};
    use sea_orm::{
        ActiveModelTrait, ConnectionTrait, Database, DatabaseConnection, DbBackend, Schema,
    };
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration as StdDuration;

    async fn setup_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let backend = db.get_database_backend();
        let schema = Schema::new(DbBackend::Sqlite);
        db.execute(backend.build(&schema.create_table_from_entity(entity::user::Entity)))
            .await
            .unwrap();
        db.execute(backend.build(&schema.create_table_from_entity(peer::Entity)))
            .await
            .unwrap();
        entity::user::ActiveModel {
            id: Set(7),
            username: Set("deployer".to_string()),
            status: Set(entity::user::STATUS_ENABLE),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        db
    }

    fn test_state(db: DatabaseConnection) -> AppState {
        let config = Config::default();
        AppState {
            db,
            config: Arc::new(config.clone()),
            jwt: Arc::new(Jwt::new("test", StdDuration::from_secs(3600))),
            limiter: Arc::new(LoginLimiter::new(SecurityPolicy {
                captcha_threshold: -1,
                ban_threshold: 0,
                attempts_window: StdDuration::from_secs(60),
                ban_duration: StdDuration::from_secs(60),
            })),
            i18n: Arc::new(I18n::load("en")),
            oauth_cache: Arc::new(OauthCache::new()),
            disconnect_store: Arc::new(DisconnectStore::new()),
            external_webclient: None,
            admin_config: Arc::new(AdminConfigStore::new(
                PathBuf::from("/tmp/rustdesk-console-test-config.yaml"),
                AdminPanelConfig::from(&config.admin),
            )),
            webclient_config: Arc::new(WebClientConfigStore::new(
                PathBuf::from("/tmp/rustdesk-console-test-config.yaml"),
                WebClientConfig::default(),
            )),
            record_storage_config: Arc::new(RecordStorageConfigStore::new(
                PathBuf::from("/tmp/rustdesk-console-test-config.yaml"),
                RecordStorage::default(),
            )),
            start_time: Arc::new(String::new()),
            version: Arc::new("test".to_string()),
        }
    }

    fn deployment_auth() -> DeploymentAuth {
        DeploymentAuth {
            user: None,
            token: "rdt_test".to_string(),
            deployment_token: Some(deployment_token::Model {
                id: 42,
                token_hash: "hash".to_string(),
                name: "test-token".to_string(),
                scopes: serde_json::to_string(&services::deployment::default_scopes()).unwrap(),
                default_user_id: 7,
                default_device_group_id: 5,
                default_strategy_id: 0,
                expires_at: 0,
                max_uses: 0,
                used_count: 0,
                revoked_at: 0,
                created_by: 1,
                created_at: None,
                updated_at: None,
            }),
        }
    }

    #[tokio::test]
    async fn deployment_upsert_persists_uuid_pk_and_rejects_conflicting_uuid() {
        let db = setup_db().await;
        let state = test_state(db.clone());
        let auth = deployment_auth();

        let peer = upsert_deployed_peer(
            &state,
            &auth,
            None,
            "desk-1",
            "uuid-1",
            Some("public-key-1"),
            None,
        )
        .await
        .unwrap();

        assert_eq!(peer.id, "desk-1");
        assert_eq!(peer.uuid, "uuid-1");
        assert_eq!(peer.pk, "public-key-1");
        assert_eq!(peer.user_id, 7);
        assert_eq!(peer.group_id, 5);

        let stored = services::peer::find_by_id(&db, "desk-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.uuid, "uuid-1");
        assert_eq!(stored.pk, "public-key-1");
        assert_eq!(stored.user_id, 7);
        assert_eq!(stored.group_id, 5);

        let conflict = upsert_deployed_peer(
            &state,
            &auth,
            None,
            "desk-1",
            "uuid-2",
            Some("public-key-2"),
            None,
        )
        .await;

        assert!(matches!(conflict, Err(DeviceDeployError::IdTaken)));
    }
}

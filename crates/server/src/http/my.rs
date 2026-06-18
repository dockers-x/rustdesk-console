//! `my/*` API (`/api/admin/my/*`), ports of `http/controller/admin/my/*.go`.
//! Same shapes as the admin CRUD but scoped to the current user, with ownership
//! checks. Available to any logged-in backend user (not just admins).

use axum::extract::{Query, State};
use axum::response::Response;
use axum::Json;
use sea_orm::{
    ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::http::admin::{
    list_json, AbRowIdForm, AddressBookForm, AddressBookQuery, CollectionForm, IdForm, IdsForm,
    PeerQuery, RuleForm, RuleQuery, TagForm, UserIdPageQuery,
};
use crate::http::middleware::{AcceptLang, BackendUser};
use crate::http::response as resp;
use crate::services;
use crate::state::AppState;
use ::entity::{peer, user as user_entity};

// ---------- my/share_record ----------

pub async fn share_record_list(
    State(state): State<AppState>,
    user: BackendUser,
    Query(q): Query<UserIdPageQuery>,
) -> Response {
    match services::share_record::list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        Some(user.user.id),
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
    user: BackendUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::share_record::info_by_id(&state.db, f.id).await {
        Ok(Some(sr)) if sr.user_id == user.user.id => {
            match services::share_record::delete(&state.db, f.id).await {
                Ok(_) => resp::success(Value::Null),
                Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
            }
        }
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn share_record_batch_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<IdsForm>,
) -> Response {
    if f.ids.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    let owned = services::share_record::count_owned(&state.db, user.user.id, &f.ids)
        .await
        .unwrap_or(0);
    if owned != f.ids.len() as i64 {
        return resp::fail(101, state.tr(&lang, "ItemNotFound"));
    }
    match services::share_record::batch_delete(&state.db, &f.ids).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

// ---------- my/address_book ----------

pub async fn address_book_list(
    State(state): State<AppState>,
    user: BackendUser,
    Query(q): Query<AddressBookQuery>,
) -> Response {
    let f = services::address_book::AbFilters {
        id: q.id,
        user_id: Some(user.user.id),
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

pub async fn address_book_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<AddressBookForm>,
) -> Response {
    let uid = user.user.id;
    if f.collection_id > 0
        && !services::address_book::check_collection_owner(&state.db, uid, f.collection_id)
            .await
            .unwrap_or(false)
    {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if let Ok(Some(_)) = services::address_book::info_by_user_id_and_id_and_cid(
        &state.db,
        uid,
        &f.id,
        f.collection_id,
    )
    .await
    {
        return resp::fail(101, state.tr(&lang, "ItemExists"));
    }
    match services::address_book::create(&state.db, f.to_model(uid)).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn address_book_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<AddressBookForm>,
) -> Response {
    let uid = user.user.id;
    if f.row_id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::address_book::info_by_row_id(&state.db, f.row_id).await {
        Ok(Some(ex)) if ex.user_id == uid => {}
        Ok(Some(_)) => return resp::fail(101, state.tr(&lang, "NoAccess")),
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
    if f.collection_id > 0
        && !services::address_book::check_collection_owner(&state.db, uid, f.collection_id)
            .await
            .unwrap_or(false)
    {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::address_book::update_all(&state.db, f.to_model(uid)).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

#[derive(Deserialize, Default)]
pub struct MyAbRowIdForm {
    #[serde(default)]
    pub row_id: i32,
}

pub async fn address_book_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<MyAbRowIdForm>,
) -> Response {
    match services::address_book::info_by_row_id(&state.db, f.row_id).await {
        Ok(Some(ex)) if ex.user_id == user.user.id => {
            match services::address_book::delete(&state.db, f.row_id).await {
                Ok(_) => resp::success(Value::Null),
                Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
            }
        }
        Ok(Some(_)) => resp::fail(101, state.tr(&lang, "NoAccess")),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

#[derive(Deserialize, Default)]
pub struct MyBatchFromPeersForm {
    #[serde(default)]
    pub collection_id: i32,
    #[serde(default)]
    pub peer_ids: Vec<i32>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub async fn address_book_batch_create_from_peers(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<MyBatchFromPeersForm>,
) -> Response {
    let uid = user.user.id;
    if f.peer_ids.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    if f.collection_id != 0 {
        match services::address_book::collection_info_by_id(&state.db, f.collection_id).await {
            Ok(Some(c)) if c.user_id == uid => {}
            Ok(Some(_)) => return resp::fail(101, state.tr(&lang, "NoAccess")),
            _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
        }
    }
    let peers: Vec<_> = services::peer::list_by_row_ids(&state.db, &f.peer_ids)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.user_id == uid)
        .collect();
    let tags = serde_json::to_value(&f.tags).unwrap_or(Value::Array(vec![]));
    for p in peers {
        let mut ab = services::address_book::from_peer(&p);
        ab.tags = tags.clone();
        ab.collection_id = f.collection_id;
        ab.user_id = uid;
        if let Ok(Some(_)) = services::address_book::info_by_user_id_and_id_and_cid(
            &state.db,
            uid,
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

pub async fn peer_request_sysinfo_refresh(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<AbRowIdForm>,
) -> Response {
    if f.row_id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    let peer = match services::peer::info_by_row_id(&state.db, f.row_id).await {
        Ok(Some(peer)) if peer.user_id == user.user.id => peer,
        Ok(Some(_)) => return resp::fail(101, state.tr(&lang, "NoAccess")),
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
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<AbRowIdForm>,
) -> Response {
    if f.row_id <= 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    let peer = match services::peer::info_by_row_id(&state.db, f.row_id).await {
        Ok(Some(peer)) if peer.user_id == user.user.id => peer,
        Ok(Some(_)) => return resp::fail(101, state.tr(&lang, "NoAccess")),
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    };
    match services::strategy::ensure_trusted_devices_enabled_for_peer(&state.db, &peer).await {
        Ok(result) => resp::success(json!({
            "peer_id": peer.id,
            "strategy_id": result.strategy_id,
            "strategy_name": result.strategy_name,
            "already_enabled": result.already_enabled,
        })),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

#[derive(Deserialize, Default)]
pub struct MyBatchUpdateTagsForm {
    #[serde(default)]
    pub row_ids: Vec<i32>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub async fn address_book_batch_update_tags(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<MyBatchUpdateTagsForm>,
) -> Response {
    let tags = serde_json::to_value(&f.tags).unwrap_or(Value::Array(vec![]));
    match services::address_book::batch_update_tags(&state.db, user.user.id, &f.row_ids, &tags)
        .await
    {
        Ok(0) => resp::fail(101, state.tr(&lang, "ItemNotFound")),
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

// ---------- my/tag ----------

#[derive(Deserialize, Default)]
pub struct MyTagQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub collection_id: Option<i32>,
}

pub async fn tag_list(
    State(state): State<AppState>,
    user: BackendUser,
    Query(q): Query<MyTagQuery>,
) -> Response {
    match services::tag::list_filtered(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        Some(user.user.id),
        q.collection_id,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn tag_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<TagForm>,
) -> Response {
    match services::tag::create(&state.db, &f.name, f.color, user.user.id, f.collection_id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn tag_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<TagForm>,
) -> Response {
    if f.id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::tag::info_by_id(&state.db, f.id).await {
        Ok(Some(ex)) if ex.user_id == user.user.id => {}
        Ok(Some(_)) => return resp::fail(101, state.tr(&lang, "NoAccess")),
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
    let model = entity::tag::Model {
        id: f.id,
        name: f.name,
        user_id: user.user.id,
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
    user: BackendUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::tag::info_by_id(&state.db, f.id).await {
        Ok(Some(ex)) if ex.user_id == user.user.id => {
            match services::tag::delete(&state.db, f.id).await {
                Ok(_) => resp::success(Value::Null),
                Err(e) => resp::fail(101, e.to_string()),
            }
        }
        Ok(Some(_)) => resp::fail(101, state.tr(&lang, "NoAccess")),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

// ---------- my/address_book_collection ----------

pub async fn collection_list(
    State(state): State<AppState>,
    user: BackendUser,
    Query(q): Query<UserIdPageQuery>,
) -> Response {
    match services::address_book::collection_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        Some(user.user.id),
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn collection_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<CollectionForm>,
) -> Response {
    if f.name.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::address_book::create_collection(&state.db, user.user.id, &f.name).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn collection_update(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<CollectionForm>,
) -> Response {
    if f.id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::address_book::collection_info_by_id(&state.db, f.id).await {
        Ok(Some(ex)) if ex.user_id == user.user.id => {}
        Ok(Some(_)) => return resp::fail(101, state.tr(&lang, "NoAccess")),
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
    match services::address_book::update_collection(&state.db, f.id, user.user.id, &f.name).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn collection_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::address_book::collection_info_by_id(&state.db, f.id).await {
        Ok(Some(ex)) if ex.user_id == user.user.id => {
            match services::address_book::delete_collection(&state.db, f.id).await {
                Ok(_) => resp::success(Value::Null),
                Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
            }
        }
        Ok(Some(_)) => resp::fail(101, state.tr(&lang, "NoAccess")),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

// ---------- my/address_book_collection_rule ----------

pub async fn rule_list(
    State(state): State<AppState>,
    user: BackendUser,
    Query(q): Query<RuleQuery>,
) -> Response {
    match services::address_book::rule_list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        Some(user.user.id),
        q.collection_id,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

async fn check_my_rule(
    state: &AppState,
    user_id: i32,
    t: &entity::address_book_collection_rule::Model,
) -> Result<(), String> {
    use entity::address_book_collection_rule as r;
    if t.user_id != user_id {
        return Err("NoAccess".into());
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
    user: BackendUser,
    Json(f): Json<RuleForm>,
) -> Response {
    let mut m = f.to_model();
    m.user_id = user.user.id;
    if let Err(e) = check_my_rule(&state, user.user.id, &m).await {
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
    user: BackendUser,
    Json(f): Json<RuleForm>,
) -> Response {
    if f.id == 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::address_book::rule_info_by_id(&state.db, f.id).await {
        Ok(Some(ex)) if ex.user_id == user.user.id => {}
        Ok(Some(_)) => return resp::fail(101, state.tr(&lang, "NoAccess")),
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
    let mut m = f.to_model();
    m.user_id = user.user.id;
    if let Err(e) = check_my_rule(&state, user.user.id, &m).await {
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
    user: BackendUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::address_book::rule_info_by_id(&state.db, f.id).await {
        Ok(Some(ex)) if ex.user_id == user.user.id => {
            match services::address_book::delete_rule(&state.db, f.id).await {
                Ok(_) => resp::success(Value::Null),
                Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
            }
        }
        Ok(Some(_)) => resp::fail(101, state.tr(&lang, "NoAccess")),
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

// ---------- my/peer ----------

pub async fn peer_list(
    State(state): State<AppState>,
    user: BackendUser,
    Query(q): Query<PeerQuery>,
) -> Response {
    let f = services::peer::PeerFilters {
        user_id: Some(user.user.id),
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

// ---------- my/login_log ----------

pub async fn login_log_list(
    State(state): State<AppState>,
    user: BackendUser,
    Query(q): Query<UserIdPageQuery>,
) -> Response {
    match services::login_log::list(
        &state.db,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        Some(user.user.id),
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn login_log_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<IdForm>,
) -> Response {
    match services::login_log::info_by_id(&state.db, f.id).await {
        Ok(Some(l)) if l.user_id == user.user.id && l.is_deleted == 0 => {
            match services::login_log::soft_delete(&state.db, f.id).await {
                Ok(_) => resp::success(Value::Null),
                Err(e) => resp::fail(101, e.to_string()),
            }
        }
        _ => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

pub async fn login_log_batch_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<IdsForm>,
) -> Response {
    if f.ids.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::login_log::batch_soft_delete(&state.db, user.user.id, &f.ids).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

// ---------- my/message ----------

#[derive(Deserialize, Default)]
pub struct MyMessageQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub folder: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct MyMessageForm {
    #[serde(default)]
    pub recipient_id: i32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
}

#[derive(Deserialize, Default)]
pub struct MyMessageUserQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(default)]
    pub q: Option<String>,
}

#[derive(Serialize)]
pub struct MessageUserPayload {
    pub id: i32,
    pub username: String,
    pub nickname: String,
    pub email: String,
}

pub async fn message_list(
    State(state): State<AppState>,
    user: BackendUser,
    Query(q): Query<MyMessageQuery>,
) -> Response {
    match services::message::user_list(
        &state.db,
        user.user.id,
        q.page.unwrap_or(0),
        q.page_size.unwrap_or(0),
        q.folder,
    )
    .await
    {
        Ok(r) => list_json(r.list, r.page, r.total, r.page_size),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn message_latest(State(state): State<AppState>, user: BackendUser) -> Response {
    let latest = match services::message::latest_for_user(&state.db, user.user.id, 3).await {
        Ok(list) => list,
        Err(e) => return resp::fail(101, e.to_string()),
    };
    let unread = services::message::unread_count(&state.db, user.user.id)
        .await
        .unwrap_or(0);
    resp::success(json!({ "list": latest, "unread": unread }))
}

pub async fn message_unread_count(State(state): State<AppState>, user: BackendUser) -> Response {
    match services::message::unread_count(&state.db, user.user.id).await {
        Ok(unread) => resp::success(json!({ "unread": unread })),
        Err(e) => resp::fail(101, e.to_string()),
    }
}

pub async fn message_create(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<MyMessageForm>,
) -> Response {
    match services::message::create(
        &state.db,
        services::message::CreateInput {
            sender: user.user,
            kind: ::entity::message::KIND_PRIVATE.to_string(),
            recipient_id: f.recipient_id,
            title: f.title,
            body: f.body,
            status: ::entity::message::STATUS_ENABLE,
            admin_mode: false,
        },
    )
    .await
    {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn message_read(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<IdForm>,
) -> Response {
    let row = match services::message::info_by_id(&state.db, f.id).await {
        Ok(Some(row)) if services::message::is_visible_to_user(&row, user.user.id) => row,
        Ok(Some(_)) => return resp::fail(101, state.tr(&lang, "NoAccess")),
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    };
    match services::message::mark_read(&state.db, row.id, user.user.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn message_delete(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<IdForm>,
) -> Response {
    let row = match services::message::info_by_id(&state.db, f.id).await {
        Ok(Some(row)) if services::message::is_visible_to_user(&row, user.user.id) => row,
        Ok(Some(_)) => return resp::fail(101, state.tr(&lang, "NoAccess")),
        _ => return resp::fail(101, state.tr(&lang, "ItemNotFound")),
    };
    match services::message::mark_deleted(&state.db, row.id, user.user.id).await {
        Ok(_) => resp::success(Value::Null),
        Err(e) => resp::fail(101, format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
    }
}

pub async fn message_users(
    State(state): State<AppState>,
    user: BackendUser,
    Query(q): Query<MyMessageUserQuery>,
) -> Response {
    let (page, page_size) = services::paginate(q.page.unwrap_or(0), q.page_size.unwrap_or(0));
    let mut query = user_entity::Entity::find()
        .filter(user_entity::Column::Status.eq(user_entity::STATUS_ENABLE))
        .filter(user_entity::Column::Id.ne(user.user.id));
    if let Some(search) = q.q.filter(|value| !value.trim().is_empty()) {
        let search = search.trim().to_string();
        query = query.filter(
            sea_orm::Condition::any()
                .add(user_entity::Column::Username.contains(&search))
                .add(user_entity::Column::Nickname.contains(&search))
                .add(user_entity::Column::Email.contains(&search)),
        );
    }
    let total = query.clone().count(&state.db).await.unwrap_or(0) as i64;
    let rows = query
        .order_by_asc(user_entity::Column::Username)
        .offset((page - 1) * page_size)
        .limit(page_size.min(50))
        .all(&state.db)
        .await
        .unwrap_or_default();
    let list: Vec<MessageUserPayload> = rows
        .into_iter()
        .map(|row| MessageUserPayload {
            id: row.id,
            username: row.username,
            nickname: row.nickname,
            email: row.email,
        })
        .collect();
    list_json(list, page as i64, total, page_size as i64)
}

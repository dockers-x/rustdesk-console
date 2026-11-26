//! Full-administrator CRUD and the read-only paired Server snapshot.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    middleware::{AcceptLang, AdminUser},
    response as resp, routing_policy,
    security_audit::AuditContext,
};
use crate::{
    services::{blocklist, device_admission},
    state::AppState,
};

#[derive(Deserialize)]
pub(super) struct SnapshotQuery {
    agent: blocklist::Agent,
    applied_revision: Option<String>,
}

pub(super) async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SnapshotQuery>,
) -> Response {
    if !routing_policy::authenticated(&state, &headers) {
        return resp::unauthorized();
    }
    let applied = query
        .applied_revision
        .filter(|revision| revision.len() == 64 && revision.bytes().all(|b| b.is_ascii_hexdigit()));
    let Ok(rules) = blocklist::rules(&state.db).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Ok(snapshot) = blocklist::Snapshot::new(&rules) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    state.blocklist_pairing.received(query.agent, applied).await;
    resp::no_store(Json(snapshot).into_response())
}

pub(super) async fn list(
    State(state): State<AppState>,
    _user: AdminUser,
    AcceptLang(lang): AcceptLang,
) -> Response {
    let Ok(rules) = blocklist::rules(&state.db).await else {
        return resp::fail(101, state.tr(&lang, "OperationFailed"));
    };
    let Ok(snapshot) = blocklist::Snapshot::new(&rules) else {
        return resp::fail(101, state.tr(&lang, "OperationFailed"));
    };
    let [hbbs, hbbr] = state.blocklist_pairing.status(&snapshot.revision).await;
    resp::no_store(resp::success(json!({
        "rules": rules,
        "paired_token_configured": device_admission::token_is_configured(&state.config.security.hbbs_admission_token),
        "hbbs": hbbs,
        "hbbr": hbbr,
    })))
}

#[derive(Deserialize)]
pub(super) struct DeviceQuery {
    #[serde(default)]
    q: String,
}

pub(super) async fn devices(
    State(state): State<AppState>,
    _user: AdminUser,
    AcceptLang(lang): AcceptLang,
    Query(query): Query<DeviceQuery>,
) -> Response {
    if query.q.len() > 128 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match blocklist::devices(&state.db, query.q.trim()).await {
        Ok(devices) => resp::no_store(resp::success(devices)),
        Err(_) => resp::fail(101, state.tr(&lang, "OperationFailed")),
    }
}

fn operation_error(state: &AppState, lang: &str, error: blocklist::Error) -> Response {
    use blocklist::Error;
    let key = match error {
        Error::Invalid => "BlocklistInvalid",
        Error::Duplicate => "BlocklistDuplicate",
        Error::Full => "BlocklistFull",
        Error::NotFound => "ItemNotFound",
        Error::Database(error) => {
            tracing::warn!(%error, "Blocklist mutation failed");
            "OperationFailed"
        }
    };
    resp::fail(101, state.tr(lang, key))
}

pub(super) async fn create(
    State(state): State<AppState>,
    user: AdminUser,
    AcceptLang(lang): AcceptLang,
    audit: AuditContext,
    Json(form): Json<blocklist::Form>,
) -> Response {
    match blocklist::save(&state.db, user.user.id, None, form).await {
        Ok(rule) => {
            audit.subject(rule.id);
            resp::no_store(resp::success(rule))
        }
        Err(error) => operation_error(&state, &lang, error),
    }
}

pub(super) async fn update(
    State(state): State<AppState>,
    user: AdminUser,
    AcceptLang(lang): AcceptLang,
    audit: AuditContext,
    Path(id): Path<i32>,
    Json(form): Json<blocklist::Form>,
) -> Response {
    audit.subject(id);
    match blocklist::save(&state.db, user.user.id, Some(id), form).await {
        Ok(rule) => resp::no_store(resp::success(rule)),
        Err(error) => operation_error(&state, &lang, error),
    }
}

pub(super) async fn remove(
    State(state): State<AppState>,
    user: AdminUser,
    AcceptLang(lang): AcceptLang,
    audit: AuditContext,
    Path(id): Path<i32>,
) -> Response {
    audit.subject(id);
    match blocklist::remove(&state.db, user.user.id, id).await {
        Ok(()) => resp::success(Value::Null),
        Err(error) => operation_error(&state, &lang, error),
    }
}

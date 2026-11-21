//! Dedicated routes for scoped administrators. Global management handlers retain their gates.

use axum::{
    extract::{Path, Query, State},
    response::Response,
    Json,
};
use serde::Serialize;

use crate::{
    services::delegated_admin::{
        self, Actor, CollectionProfile, EntryProfile, Error, PageQuery, UserStatus,
    },
    state::AppState,
};

use super::{
    middleware::{AcceptLang, AdminUser},
    response as resp,
    security_audit::AuditContext,
};

pub(crate) async fn collection_options(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _actor: AdminUser,
    Query(query): Query<PageQuery>,
) -> Response {
    reply(
        &state,
        &lang,
        delegated_admin::collection_options(&state.db, query).await,
    )
}

pub(crate) async fn scopes(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    actor: Actor,
) -> Response {
    reply(
        &state,
        &lang,
        delegated_admin::scope_view(&state.db, actor).await,
    )
}

pub(crate) async fn group_users(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    actor: Actor,
    Path(group_id): Path<i32>,
    Query(query): Query<PageQuery>,
) -> Response {
    reply(
        &state,
        &lang,
        delegated_admin::group_users(&state.db, actor, group_id, query).await,
    )
}

pub(crate) async fn set_user_status(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    actor: Actor,
    audit: AuditContext,
    Path(group_id): Path<i32>,
    Json(input): Json<UserStatus>,
) -> Response {
    audit.subject(group_id);
    reply(
        &state,
        &lang,
        delegated_admin::set_user_status(&state.db, actor, group_id, input).await,
    )
}

pub(crate) async fn collection_entries(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    actor: Actor,
    Path(id): Path<i32>,
    Query(query): Query<PageQuery>,
) -> Response {
    reply(
        &state,
        &lang,
        delegated_admin::collection_entries(&state.db, actor, id, query).await,
    )
}

pub(crate) async fn update_collection(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    actor: Actor,
    audit: AuditContext,
    Path(id): Path<i32>,
    Json(input): Json<CollectionProfile>,
) -> Response {
    audit.subject(id);
    reply(
        &state,
        &lang,
        delegated_admin::update_collection(&state.db, actor, id, input).await,
    )
}

pub(crate) async fn update_entry(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    actor: Actor,
    audit: AuditContext,
    Path((collection_id, row_id)): Path<(i32, i32)>,
    Json(input): Json<EntryProfile>,
) -> Response {
    audit.subject(row_id);
    reply(
        &state,
        &lang,
        delegated_admin::update_entry(&state.db, actor, collection_id, row_id, input).await,
    )
}

fn reply<T: Serialize>(state: &AppState, lang: &str, result: Result<T, Error>) -> Response {
    match result {
        Ok(value) => resp::no_store(resp::success(value)),
        Err(Error::Denied) => resp::fail(resp::CODE_NO_ACCESS, state.tr(lang, "NoAccess")),
        Err(Error::Invalid) => resp::fail(101, state.tr(lang, "ParamsError")),
        Err(Error::Database) => resp::fail(101, state.tr(lang, "OperationFailed")),
    }
}

#[cfg(test)]
#[path = "delegated_admin_tests.rs"]
mod tests;

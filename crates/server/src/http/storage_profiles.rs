use axum::{
    extract::{Path, State},
    response::Response,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    state::AppState,
    support::storage_profiles::{BackupSelection, ProfileForm, Profiles, ProfilesView},
};

use super::{middleware::AdminUser, response as resp};

fn response(result: anyhow::Result<ProfilesView>) -> Response {
    match result {
        Ok(view) => resp::no_store(resp::success(view)),
        Err(error) => resp::fail(101, error.to_string()),
    }
}

pub(crate) async fn list(State(state): State<AppState>, _admin: AdminUser) -> Response {
    response(
        Profiles::load(state.record_storage_config.profiles_path()).map(|profiles| profiles.view()),
    )
}

pub(crate) async fn create(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(form): Json<ProfileForm>,
) -> Response {
    response(Profiles::edit(
        state.record_storage_config.profiles_path(),
        |profiles| profiles.upsert(None, form),
    ))
}

pub(crate) async fn update(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(form): Json<ProfileForm>,
) -> Response {
    response(Profiles::edit(
        state.record_storage_config.profiles_path(),
        |profiles| profiles.upsert(Some(id), form),
    ))
}

pub(crate) async fn archive(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Response {
    response(Profiles::edit(
        state.record_storage_config.profiles_path(),
        |profiles| profiles.archive(id),
    ))
}

#[derive(Deserialize)]
pub(crate) struct RecordingSelection {
    profile_id: Option<Uuid>,
}

pub(crate) async fn recording(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(form): Json<RecordingSelection>,
) -> Response {
    response(Profiles::edit(
        state.record_storage_config.profiles_path(),
        |profiles| profiles.select_recording(form.profile_id),
    ))
}

#[derive(Deserialize)]
pub(crate) struct BackupForm {
    selection: Option<BackupSelection>,
}

pub(crate) async fn backup(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(form): Json<BackupForm>,
) -> Response {
    response(Profiles::edit(
        state.record_storage_config.profiles_path(),
        |profiles| profiles.select_backup(form.selection),
    ))
}

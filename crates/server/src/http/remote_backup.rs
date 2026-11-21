use axum::{extract::State, response::Response};

use crate::{services::remote_backup, state::AppState};

use super::{middleware::AdminUser, response as resp};

pub(crate) async fn status(State(state): State<AppState>, _admin: AdminUser) -> Response {
    let result = remote_backup::effective_settings(
        &state.config,
        state.record_storage_config.profiles_path(),
    )
    .and_then(|settings| remote_backup::view(&state.config, &settings));
    match result {
        Ok(view) => resp::no_store(resp::success(view)),
        Err(error) => {
            tracing::warn!(%error, "Could not read remote backup status");
            resp::fail(101, "Could not read backup status")
        }
    }
}

pub(crate) async fn start(State(state): State<AppState>, _admin: AdminUser) -> Response {
    let result = remote_backup::effective_settings(
        &state.config,
        state.record_storage_config.profiles_path(),
    )
    .and_then(|settings| remote_backup::start(&state.config, settings, false));
    match result {
        Ok(started) => resp::no_store(resp::success(serde_json::json!({ "started": started }))),
        Err(error) => {
            tracing::warn!(%error, "Could not start remote backup");
            resp::fail(101, "Could not start backup")
        }
    }
}

pub mod self_signed_client;

use axum::{extract::State, response::Response, routing::get, Router};

use crate::{
    http::{middleware::BackendUser, response},
    state::AppState,
};

pub use self_signed_client::FeatureStore;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/features", get(metadata))
        .merge(self_signed_client::routes())
}

async fn metadata(State(state): State<AppState>, _user: BackendUser) -> Response {
    match state.features.settings().await {
        Ok(settings) => response::success(serde_json::json!({
            "self_signed_client": { "enabled": settings.enabled },
        })),
        Err(_) => self_signed_client::storage_error(),
    }
}

//! Application error type. Converts to the Go `{ "error": "..." }` envelope at
//! HTTP 400 by default. Most handlers build success/fail envelopes explicitly
//! via the `http::response` helpers; this type is for the `?` error path.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl AppError {
    pub fn msg(s: impl Into<String>) -> Self {
        AppError::Message(s.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = json!({ "error": self.to_string() });
        (StatusCode::BAD_REQUEST, Json(body)).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;

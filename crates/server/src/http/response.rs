//! Response envelopes, mirroring `http/response/response.go`.
//!
//! - `success(data)` -> `{ "code": 0, "message": "success", "data": ... }` (HTTP 200)
//! - `fail(code, msg)` -> `{ "code": code, "message": msg, "data": null }` (HTTP 200)
//! - `error(msg)` -> `{ "error": msg }` (HTTP 400)

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::{json, Value};

/// `{ code, message, data }` at HTTP 200 with `code: 0`.
pub fn success<T: Serialize>(data: T) -> Response {
    let data = serde_json::to_value(data).unwrap_or(Value::Null);
    Json(json!({ "code": 0, "message": "success", "data": data })).into_response()
}

/// `{ code, message, data: null }` at HTTP 200 with a non-zero code.
pub fn fail(code: i32, message: impl Into<String>) -> Response {
    Json(json!({ "code": code, "message": message.into(), "data": Value::Null })).into_response()
}

/// `{ error }` at HTTP 400.
pub fn error(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message.into() })),
    )
        .into_response()
}

/// `401 { "error": "Unauthorized" }` (used by the RustDesk client auth path).
pub fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "Unauthorized" })),
    )
        .into_response()
}

/// `{ total, data }` (mirrors `response.DataResponse`).
pub fn data_response<T: Serialize>(total: i64, data: T) -> Response {
    let data = serde_json::to_value(data).unwrap_or(Value::Null);
    Json(json!({ "total": total, "data": data })).into_response()
}

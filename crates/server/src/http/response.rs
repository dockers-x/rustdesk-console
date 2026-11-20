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

pub const CODE_NO_ACCESS: i32 = 401;
pub const CODE_NEED_LOGIN: i32 = 403;

/// `{ code, message, data }` at HTTP 200 with `code: 0`.
pub fn success<T: Serialize>(data: T) -> Response {
    let data = serde_json::to_value(data).unwrap_or(Value::Null);
    Json(json!({ "code": 0, "message": "success", "data": data })).into_response()
}

/// `{ code, message, data: null }` at HTTP 200 with a non-zero code.
pub fn fail(code: i32, message: impl Into<String>) -> Response {
    (
        axum::Extension(super::security_audit::BusinessCode(code)),
        Json(json!({ "code": code, "message": message.into(), "data": Value::Null })),
    )
        .into_response()
}

/// Prevent credentials and challenge material from being cached.
pub fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
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

#[cfg(test)]
mod tests {
    use super::{CODE_NEED_LOGIN, CODE_NO_ACCESS};

    #[test]
    fn authorization_codes_match_the_web_client_contract() {
        assert_eq!(CODE_NO_ACCESS, 401);
        assert_eq!(CODE_NEED_LOGIN, 403);
    }
}

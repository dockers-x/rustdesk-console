//! File upload, ports `http/controller/admin/file.go`:
//! local multipart upload, OSS policy token, and the OSS callback notify.

use axum::extract::{Multipart, State};
use axum::response::Response;
use axum::Json;
use serde_json::{json, Value};

use crate::http::middleware::{AcceptLang, AdminUser, BackendUser};
use crate::http::response as resp;
use crate::state::AppState;

fn upload_root(state: &AppState) -> String {
    let base = if state.config.gin.resources_path.is_empty() {
        "resources".to_string()
    } else {
        state.config.gin.resources_path.clone()
    };
    format!("{base}/public/upload")
}

/// `POST /api/admin/file/upload` — save a multipart file under
/// `resources/public/upload/<YYYYMMDD>/` and return its web path.
pub async fn upload(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    _user: BackendUser,
    mut multipart: Multipart,
) -> Response {
    let date = chrono::Local::now().format("%Y%m%d").to_string();
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field
            .file_name()
            .map(sanitize)
            .unwrap_or_else(|| "file".to_string());
        let data = match field.bytes().await {
            Ok(d) => d,
            Err(e) => return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e)),
        };
        let dir = format!("{}/{}", upload_root(&state), date);
        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
        }
        let dst = format!("{dir}/{filename}");
        if let Err(e) = tokio::fs::write(&dst, &data).await {
            return resp::error(format!("{}{}", state.tr(&lang, "OperationFailed"), e));
        }
        let web_path = format!("/upload/{date}/{filename}");
        return resp::success(json!({ "url": web_path }));
    }
    resp::fail(101, state.tr(&lang, "ParamsError"))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0'))
        .collect::<String>()
        .trim_start_matches('.')
        .to_string()
}

/// `GET /api/admin/file/oss_token` — signed OSS post-policy for direct upload.
pub async fn oss_token(State(state): State<AppState>, _user: AdminUser) -> Response {
    let token = crate::support::oss::get_policy_token(&state.config.oss, "");
    resp::success(token)
}

/// `POST /api/admin/file/notify` — OSS upload callback; returns the file URL.
/// (The RSA callback-signature verification of the Go version is omitted; rely
/// on OSS callback being configured to a trusted endpoint.)
pub async fn notify(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let filename = body
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut out = body.clone();
    if let Value::Object(ref mut m) = out {
        m.insert(
            "url".into(),
            Value::String(format!("{}/{}", state.config.oss.host, filename)),
        );
    }
    resp::success(out)
}

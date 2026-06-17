//! Serving of embedded static assets (single-binary frontend) and the few
//! dynamic web routes from `http/controller/web/index.go`.

use std::path::{Component, Path as FsPath, PathBuf};

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::assets::{AdminAssets, Resources};
use crate::state::AppState;
use crate::support::external_webclient::ExternalWebClient;

/// `GET /` -> redirect to the admin SPA (matches the Go web index).
pub async fn index() -> Response {
    Redirect::temporary("/_admin/").into_response()
}

/// `GET /webclient-config/index.js` -> bootstraps the web client's defaults.
pub async fn config_js(State(state): State<AppState>) -> Response {
    let cfg = state.webclient_config.get().await;
    let rd = &state.config.rustdesk;
    let body = format!(
        "(() => {{\n\
  const config = {{\n\
    'api-server': {api},\n\
    'custom-rendezvous-server': {id_server},\n\
    'relay-server': {relay_server},\n\
    'ws-host': {ws},\n\
    'ws-id-host': {ws_id},\n\
    'ws-relay-host': {ws_relay},\n\
    'key': {key},\n\
  }};\n\
  const localOverrideKey = 'rustdesk-console.webclient.local-override';\n\
  const hasLocalOverride = localStorage.getItem(localOverrideKey) === '1';\n\
  const setDefault = (name, value) => {{\n\
    if (!hasLocalOverride || localStorage.getItem(name) === null) {{\n\
      localStorage.setItem(name, value);\n\
    }}\n\
  }};\n\
  const ws2Prefix = 'wc-';\n\
  for (const [name, value] of Object.entries(config)) {{\n\
    setDefault(name, value);\n\
    setDefault(ws2Prefix + name, value);\n\
  }}\n\
  window.webclient_magic_queryonline = {magic};\n\
  window.ws_host = localStorage.getItem('ws-host') || '';\n\
  window.ws_id_host = localStorage.getItem('ws-id-host') || '';\n\
  window.ws_relay_host = localStorage.getItem('ws-relay-host') || '';\n\
}})();\n",
        api = js_string(&cfg.api_server),
        id_server = js_string(&cfg.id_server),
        relay_server = js_string(&cfg.relay_server),
        key = js_string(&cfg.key),
        magic = rd.webclient_magic_queryonline,
        ws = js_string(&cfg.ws_host),
        ws_id = js_string(&cfg.ws_id_host),
        ws_relay = js_string(&cfg.ws_relay_host),
    );
    ([(header::CONTENT_TYPE, "application/javascript")], body).into_response()
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn respond(bytes: Vec<u8>, path_for_mime: &str) -> Response {
    let mime = mime_guess::from_path(path_for_mime).first_or_octet_stream();
    ([(header::CONTENT_TYPE, mime.as_ref().to_string())], bytes).into_response()
}

fn respond_html(body: String) -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

/// Serve `web/<path>` from the embedded Flutter client, with SPA fallback to
/// `web/index.html`.
async fn web_asset(state: &AppState, path: &str) -> Response {
    if let Some(external) = &state.external_webclient {
        if let Some(response) = external_web_asset(state, external, path).await {
            return response;
        }
    }
    embedded_web_asset(path)
}

fn embedded_web_asset(path: &str) -> Response {
    let Some(path) = safe_web_asset_path(path) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let path_for_mime = path.to_string_lossy().to_string();
    let full = format!("web/{path_for_mime}");
    if let Some(bytes) = Resources::read(&full) {
        return respond(bytes, &path_for_mime);
    }
    if let Some(bytes) = Resources::read("web/index.html") {
        return respond(bytes, "index.html");
    }
    (StatusCode::NOT_FOUND, "not found").into_response()
}

async fn external_web_asset(
    state: &AppState,
    external: &ExternalWebClient,
    path: &str,
) -> Option<Response> {
    let Some(relative_path) = safe_web_asset_path(path) else {
        return Some((StatusCode::NOT_FOUND, "not found").into_response());
    };

    let request_path = relative_path.to_string_lossy().to_string();
    let full_path = external.root().join(&relative_path);
    if external_file_exists(&full_path).await {
        if request_path == "index.html" {
            return render_external_index(state, external).await;
        }
        match tokio::fs::read(&full_path).await {
            Ok(bytes) => return Some(respond(bytes, &request_path)),
            Err(err) => {
                tracing::warn!(
                    path = %full_path.display(),
                    error = %err,
                    "failed to read external WebClient asset; falling back to embedded asset"
                );
                return None;
            }
        }
    }

    render_external_index(state, external).await
}

async fn render_external_index(state: &AppState, external: &ExternalWebClient) -> Option<Response> {
    let index_path = external.root().join("index.html");
    let bytes = match tokio::fs::read(&index_path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            tracing::warn!(
                path = %index_path.display(),
                error = %err,
                "failed to read external WebClient index; falling back to embedded WebClient"
            );
            return None;
        }
    };

    let Ok(html) = String::from_utf8(bytes) else {
        tracing::warn!(
            path = %index_path.display(),
            "external WebClient index is not UTF-8; falling back to embedded WebClient"
        );
        return None;
    };

    let custom_config = external_custom_config(state).await;
    let html = html
        .replace(
            r#"<base href="/static/web/" />"#,
            r#"<base href="/webclient/" />"#,
        )
        .replace(
            r#"<base href="/static/web/">"#,
            r#"<base href="/webclient/">"#,
        )
        .replace("{{CUSTOM_CONFIG}}", &custom_config);
    Some(respond_html(html))
}

async fn external_file_exists(path: &FsPath) -> bool {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata.is_file() && !metadata.file_type().is_symlink(),
        Err(_) => false,
    }
}

async fn external_custom_config(state: &AppState) -> String {
    let cfg = state.webclient_config.get().await;
    let ini = format!(
        "[default-settings]\n\
custom-rendezvous-server={}\n\
relay-server={}\n\
api-server={}\n\
key={}\n",
        ini_value(&cfg.id_server),
        ini_value(&cfg.relay_server),
        ini_value(&cfg.api_server),
        ini_value(&cfg.key),
    );
    STANDARD.encode(ini)
}

fn ini_value(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| match ch {
            '\r' | '\n' => ' ',
            _ => ch,
        })
        .collect()
}

fn safe_web_asset_path(path: &str) -> Option<PathBuf> {
    let path = if path.is_empty() { "index.html" } else { path };
    if path.contains('\0') || path.contains('\\') || looks_like_windows_drive_path(path) {
        return None;
    }

    let mut clean = PathBuf::new();
    for component in FsPath::new(path).components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if clean.as_os_str().is_empty() {
        Some(PathBuf::from("index.html"))
    } else {
        Some(clean)
    }
}

fn looks_like_windows_drive_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

pub async fn webclient_index(State(state): State<AppState>) -> Response {
    web_asset(&state, "").await
}

pub async fn webclient_path(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    web_asset(&state, &path).await
}

/// Serve `<path>` from the embedded admin SPA, with SPA fallback to `index.html`.
async fn admin_asset(path: &str) -> Response {
    let path = if path.is_empty() { "index.html" } else { path };
    if let Some(f) = AdminAssets::get(path) {
        return respond(f.data.into_owned(), path);
    }
    if let Some(f) = AdminAssets::get("index.html") {
        return respond(f.data.into_owned(), "index.html");
    }
    (
        StatusCode::NOT_FOUND,
        "Admin UI not built into this binary. Build it from \
         https://github.com/lejianwen/rustdesk-api-web into resources/admin and rebuild.",
    )
        .into_response()
}

pub async fn admin_index() -> Response {
    admin_asset("").await
}

pub async fn admin_path(Path(path): Path<String>) -> Response {
    admin_asset(&path).await
}

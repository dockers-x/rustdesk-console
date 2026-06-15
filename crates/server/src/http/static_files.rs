//! Serving of embedded static assets (single-binary frontend) and the few
//! dynamic web routes from `http/controller/web/index.go`.

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};

use crate::assets::{AdminAssets, Resources};
use crate::state::AppState;

/// `GET /` -> redirect to the admin SPA (matches the Go web index).
pub async fn index() -> Response {
    Redirect::temporary("/_admin/").into_response()
}

/// `GET /webclient-config/index.js` -> bootstraps the web client's api-server.
pub async fn config_js(State(state): State<AppState>) -> Response {
    let rd = &state.config.rustdesk;
    let body = format!(
        "localStorage.setItem('api-server', '{api}');\n\
const ws2_prefix = 'wc-';\n\
localStorage.setItem(ws2_prefix+'api-server', '{api}');\n\
\n\
window.webclient_magic_queryonline = {magic};\n\
window.ws_host = '{ws}';\n",
        api = rd.api_server,
        magic = rd.webclient_magic_queryonline,
        ws = rd.ws_host,
    );
    ([(header::CONTENT_TYPE, "application/javascript")], body).into_response()
}

fn respond(bytes: Vec<u8>, path_for_mime: &str) -> Response {
    let mime = mime_guess::from_path(path_for_mime).first_or_octet_stream();
    (
        [(header::CONTENT_TYPE, mime.as_ref().to_string())],
        bytes,
    )
        .into_response()
}

/// Serve `web/<path>` from the embedded Flutter client, with SPA fallback to
/// `web/index.html`.
async fn web_asset(path: &str) -> Response {
    let path = if path.is_empty() { "index.html" } else { path };
    let full = format!("web/{path}");
    if let Some(bytes) = Resources::read(&full) {
        return respond(bytes, path);
    }
    if let Some(bytes) = Resources::read("web/index.html") {
        return respond(bytes, "index.html");
    }
    (StatusCode::NOT_FOUND, "not found").into_response()
}

pub async fn webclient_index() -> Response {
    web_asset("").await
}

pub async fn webclient_path(Path(path): Path<String>) -> Response {
    web_asset(&path).await
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

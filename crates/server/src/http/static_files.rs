//! Serving of embedded static assets (single-binary frontend) and the few
//! dynamic web routes from `http/controller/web/index.go`.

use std::path::{Component, Path as FsPath, PathBuf};

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rustdesk_uri::WebClientVersion;
use serde_json::json;

use crate::assets::{AdminAssets, Resources};
use crate::state::AppState;
use crate::support::external_webclient::ExternalWebClient;
use crate::support::webclient_config::WebClientConfig;

/// `GET /` -> redirect to the admin SPA (matches the Go web index).
pub async fn index() -> Response {
    Redirect::temporary("/_admin/").into_response()
}

/// `GET /webclient-config/index.js` -> bootstraps the web client's defaults.
pub async fn config_js(State(state): State<AppState>) -> Response {
    let cfg = state.webclient_config.get().await;
    let body = webclient_bootstrap_js(
        &cfg,
        state.config.rustdesk.webclient_magic_queryonline,
        false,
    );
    ([(header::CONTENT_TYPE, "application/javascript")], body).into_response()
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
fn embedded_web_asset(path: &str, version: WebClientVersion) -> Response {
    let Some(path) = safe_web_asset_path(path) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let path_for_mime = path.to_string_lossy().to_string();
    let full = format!("web/{path_for_mime}");
    if let Some(bytes) = Resources::read(&full) {
        if path_for_mime == "index.html" {
            return match String::from_utf8(bytes) {
                Ok(html) => respond_html(rewrite_webclient_index_html(html, version)),
                Err(err) => respond(err.into_bytes(), &path_for_mime),
            };
        }
        return respond(bytes, &path_for_mime);
    }
    if let Some(bytes) = Resources::read("web/index.html") {
        return match String::from_utf8(bytes) {
            Ok(html) => respond_html(rewrite_webclient_index_html(html, version)),
            Err(err) => respond(err.into_bytes(), "index.html"),
        };
    }
    (StatusCode::NOT_FOUND, "not found").into_response()
}

async fn external_web_asset(
    state: &AppState,
    external: &ExternalWebClient,
    path: &str,
    version: WebClientVersion,
) -> Option<Response> {
    let Some(relative_path) = safe_web_asset_path(path) else {
        return Some((StatusCode::NOT_FOUND, "not found").into_response());
    };

    let request_path = relative_path.to_string_lossy().to_string();
    let full_path = external.root().join(&relative_path);
    if external_file_exists(&full_path).await {
        if request_path == "index.html" {
            return render_external_index(state, external, version).await;
        }
        match tokio::fs::read(&full_path).await {
            Ok(bytes) => {
                let bytes = patch_external_webclient_asset(&request_path, bytes);
                return Some(respond(bytes, &request_path));
            }
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

    render_external_index(state, external, version).await
}

async fn render_external_index(
    state: &AppState,
    external: &ExternalWebClient,
    version: WebClientVersion,
) -> Option<Response> {
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

    let cfg = state.webclient_config.get().await;
    let custom_config = external_custom_config(&cfg);
    let bootstrap =
        external_webclient_bootstrap_tag(&cfg, state.config.rustdesk.webclient_magic_queryonline);
    let html =
        rewrite_webclient_index_html(html, version).replace("{{CUSTOM_CONFIG}}", &custom_config);
    Some(respond_html(inject_external_bootstrap(html, &bootstrap)))
}

fn rewrite_webclient_index_html(html: String, version: WebClientVersion) -> String {
    let base_href = format!(r#"<base href="{}">"#, version.path());
    let base_href_self_closing = format!(r#"<base href="{}" />"#, version.path());

    html.replace(r#"<base href="/static/web/" />"#, &base_href_self_closing)
        .replace(r#"<base href="/static/web/">"#, &base_href)
        .replace(r#"<base href="/webclient/" />"#, &base_href_self_closing)
        .replace(r#"<base href="/webclient/">"#, &base_href)
        .replace(r#"<base href="/webclient2/" />"#, &base_href_self_closing)
        .replace(r#"<base href="/webclient2/">"#, &base_href)
}

fn patch_external_webclient_asset(path: &str, bytes: Vec<u8>) -> Vec<u8> {
    if path != "js/dist/index.js" {
        return bytes;
    }
    match String::from_utf8(bytes) {
        Ok(text) => patch_external_webclient_js(&text).into_bytes(),
        Err(err) => err.into_bytes(),
    }
}

fn patch_external_webclient_js(text: &str) -> String {
    const V2_WS_ROUTE_FUNCTION: &str =
        r#"function Ce(u=!1){const e=k.getItem("custom-rendezvous-server");return ce(e||s1,u)}"#;
    const V2_WS_ROUTE_PATCH: &str = r#"function Ce(u=!1){const r=u?k.getItem("ws-relay-host"):k.getItem("ws-id-host"),h=k.getItem("ws-host");if(r)return __rdConsoleWsRoute(r,u,!1);if(h)return __rdConsoleWsRoute(h,u,!0);const e=k.getItem("custom-rendezvous-server");return ce(e||s1,u)}function __rdConsoleWsRoute(r,u=!1,h=!1){const p=u?"/ws/relay":"/ws/id",v=String(r||"").trim().replace(/\/+$/g,"");if(!v)return"";if(v.startsWith("http://")){const b="ws://"+v.slice(7);return h&&!/\/ws\/(id|relay)$/i.test(b)?b+p:b}if(v.startsWith("https://")){const b="wss://"+v.slice(8);return h&&!/\/ws\/(id|relay)$/i.test(b)?b+p:b}if(v.startsWith("ws://")||v.startsWith("wss://"))return h&&!/\/ws\/(id|relay)$/i.test(v)?v+p:v;return h?ce(v,u):v}"#;
    text.replace(V2_WS_ROUTE_FUNCTION, V2_WS_ROUTE_PATCH)
}

async fn external_file_exists(path: &FsPath) -> bool {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata.is_file() && !metadata.file_type().is_symlink(),
        Err(_) => false,
    }
}

fn external_custom_config(cfg: &WebClientConfig) -> String {
    let ini = format!(
        "[default-settings]\n\
custom-rendezvous-server={}\n\
relay-server={}\n\
api-server={}\n\
ws-host={}\n\
ws-id-host={}\n\
ws-relay-host={}\n\
key={}\n",
        ini_value(&cfg.id_server),
        ini_value(&cfg.relay_server),
        ini_value(&cfg.api_server),
        ini_value(&cfg.ws_host),
        ini_value(&cfg.ws_id_host),
        ini_value(&cfg.ws_relay_host),
        ini_value(&cfg.key),
    );
    STANDARD.encode(ini)
}

fn external_webclient_bootstrap_tag(cfg: &WebClientConfig, magic: i32) -> String {
    format!(
        "<script>\n{}\n</script>\n",
        webclient_bootstrap_js(cfg, magic, true)
    )
}

fn inject_external_bootstrap(mut html: String, bootstrap: &str) -> String {
    if let Some(idx) = html.find(r#"<script type="module""#) {
        html.insert_str(idx, bootstrap);
        return html;
    }
    if let Some(idx) = html.find("</head>") {
        html.insert_str(idx, bootstrap);
        return html;
    }
    if let Some(idx) = html.find("</body>") {
        html.insert_str(idx, bootstrap);
        return html;
    }
    html.push_str(bootstrap);
    html
}

fn webclient_bootstrap_js(cfg: &WebClientConfig, magic: i32, patch_external_v2_ws: bool) -> String {
    let deployment = crate::support::deployment_config::build(cfg);
    let config_json = safe_script_json(json!({
        "api-server": cfg.api_server.as_str(),
        "custom-rendezvous-server": cfg.id_server.as_str(),
        "relay-server": cfg.relay_server.as_str(),
        "ws-host": cfg.ws_host.as_str(),
        "ws-id-host": cfg.ws_id_host.as_str(),
        "ws-relay-host": cfg.ws_relay_host.as_str(),
        "key": cfg.key.as_str(),
    }));
    let routes_json = safe_script_json(json!({
        "id": deployment.webclient_ws_routes.id,
        "relay": deployment.webclient_ws_routes.relay,
    }));
    let ws_patch = if patch_external_v2_ws {
        "\n\
  const wsRoutes = __WS_ROUTES__;\n\
  const trimTrailingSlash = (value) => String(value || '').trim().replace(/\\/+$/g, '');\n\
  const toWsBase = (value) => {\n\
    const raw = trimTrailingSlash(value);\n\
    if (!raw) return '';\n\
    if (raw.startsWith('http://')) return 'ws://' + raw.slice(7);\n\
    if (raw.startsWith('https://')) return 'wss://' + raw.slice(8);\n\
    return raw;\n\
  };\n\
  const withWsPath = (base, path) => {\n\
    const next = toWsBase(base);\n\
    if (!next) return '';\n\
    return /\\/ws\\/(id|relay)$/i.test(next) ? next : next + path;\n\
  };\n\
  const wsIdRoute = toWsBase(getStored('ws-id-host')) || withWsPath(getStored('ws-host'), '/ws/id') || wsRoutes.id || '';\n\
  const wsRelayRoute = toWsBase(getStored('ws-relay-host')) || withWsPath(getStored('ws-host'), '/ws/relay') || wsRoutes.relay || '';\n\
  const scheme = () => window.location.protocol === 'https:' ? 'wss' : 'ws';\n\
  const isIpv4 = (value) => /^(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)(:\\d+)?$/.test(value);\n\
  const isDomain = (value) => /^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\\.)+[a-z][a-z-]{0,61}[a-z]$/i.test(value);\n\
  const isDomainWithPort = (value) => /^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\\.)+[a-z][a-z-]{0,61}[a-z]:\\d{1,5}$/i.test(value);\n\
  const deriveRustDeskWsRoute = (host, relay) => {\n\
    const value = String(host || '').trim();\n\
    if (!value) return '';\n\
    if (value.startsWith('ws://') || value.startsWith('wss://')) return value;\n\
    const path = relay ? '/ws/relay' : '/ws/id';\n\
    const port = relay ? 21119 : 21118;\n\
    if (isIpv4(value)) {\n\
      const idx = value.indexOf(':');\n\
      if (idx >= 0) {\n\
        const base = value.slice(0, idx);\n\
        const parsed = parseInt(value.slice(idx + 1), 10);\n\
        return `${scheme()}://${base}:${Number.isNaN(parsed) ? port : parsed + 2}`;\n\
      }\n\
      return `${scheme()}://${value}:${port}`;\n\
    }\n\
    if (value.includes(':')) {\n\
      const [base] = value.split(':');\n\
      if (isDomainWithPort(value)) return `${scheme()}://${base}${path}`;\n\
    } else if (isDomain(value)) {\n\
      return `${scheme()}://${value}${path}`;\n\
    }\n\
    return value;\n\
  };\n\
  const idServer = getStored('custom-rendezvous-server');\n\
  const defaultIdRoute = deriveRustDeskWsRoute(idServer, false);\n\
  const defaultRelayRoute = deriveRustDeskWsRoute(idServer, true);\n\
  const rewriteWsUrl = (url) => {\n\
    const value = String(url || '');\n\
    if (wsIdRoute && (value === defaultIdRoute || /\\/ws\\/id(?:[?#].*)?$/i.test(value))) return wsIdRoute;\n\
    if (wsRelayRoute && (value === defaultRelayRoute || /\\/ws\\/relay(?:[?#].*)?$/i.test(value))) return wsRelayRoute;\n\
    return url;\n\
  };\n\
  if ((wsIdRoute || wsRelayRoute) && typeof window.WebSocket === 'function' && typeof Proxy === 'function') {\n\
    const NativeWebSocket = window.WebSocket;\n\
    window.WebSocket = new Proxy(NativeWebSocket, {\n\
      construct(target, args) {\n\
        if (args.length > 0) args[0] = rewriteWsUrl(args[0]);\n\
        return Reflect.construct(target, args);\n\
      }\n\
    });\n\
  }\n"
            .replace("__WS_ROUTES__", &routes_json)
    } else {
        String::new()
    };
    let oauth_bridge = webclient_oauth_bridge_js();

    format!(
        "(() => {{\n\
  const config = {config_json};\n\
  const localOverrideKey = 'rustdesk-console.webclient.local-override';\n\
  const hasLocalOverride = localStorage.getItem(localOverrideKey) === '1';\n\
  const wcPrefix = 'wc-';\n\
  const setDefault = (name, value) => {{\n\
    const next = value == null ? '' : String(value);\n\
    if (!hasLocalOverride || localStorage.getItem(name) === null) {{\n\
      localStorage.setItem(name, next);\n\
    }}\n\
  }};\n\
  const getStored = (name) => localStorage.getItem(name) || localStorage.getItem(wcPrefix + name) || '';\n\
  for (const [name, value] of Object.entries(config)) {{\n\
    setDefault(name, value);\n\
    setDefault(wcPrefix + name, value);\n\
  }}\n\
  window.webclient_magic_queryonline = {magic};\n\
  window.ws_host = getStored('ws-host');\n\
  window.ws_id_host = getStored('ws-id-host');\n\
  window.ws_relay_host = getStored('ws-relay-host');\
{oauth_bridge}\
{ws_patch}\
}})();\n"
    )
}

fn webclient_oauth_bridge_js() -> &'static str {
    r#"
  const oauthBridge = (() => {
    const REQUESTING = 'Requesting account auth';
    const WAITING = 'Waiting account auth';
    const LOGIN = 'Login account auth';
    const blankResult = () => ({
      state_msg: '',
      failed_msg: '',
      url: null,
      url_launched: false,
      auth_body: null,
    });
    let result = blankResult();
    let generation = 0;
    let timer = 0;
    let popup = null;
    const clearTimer = () => {
      if (timer) window.clearTimeout(timer);
      timer = 0;
    };
    const apiBase = () => {
      const base = getStored('api-server') || window.location.origin;
      return String(base || '').replace(/\/+$/g, '');
    };
    const apiUrl = (path) => `${apiBase()}${path}`;
    const parseJsonResponse = async (response) => {
      const body = await response.json().catch(() => ({}));
      if (!response.ok) {
        throw new Error((body && body.error) || `HTTP ${response.status}`);
      }
      if (body && body.error) {
        throw new Error(body.error);
      }
      return body;
    };
    const deviceInfo = () => ({
      name: window.navigator.userAgent || '',
      os: window.navigator.platform || '',
      type: 'browser',
    });
    const openAuthPopup = () => {
      try {
        popup = window.open('about:blank', '_blank');
        if (popup) {
          popup.opener = null;
          popup.document.title = 'RustDesk OAuth';
          popup.document.body.textContent = 'Redirecting to OAuth provider...';
          result.url_launched = true;
        }
      } catch (_) {
        popup = null;
      }
    };
    const navigatePopup = (url) => {
      if (!url) return;
      if (popup && !popup.closed) {
        try {
          popup.location.href = url;
          return;
        } catch (_) {
          popup = null;
        }
      }
      result.url_launched = false;
    };
    const storeAuthBody = (authBody, remember) => {
      if (!remember || !authBody || authBody.type !== 'access_token' || !authBody.access_token) return;
      localStorage.setItem('access_token', authBody.access_token);
      if (authBody.user) {
        localStorage.setItem('user_info', JSON.stringify(authBody.user));
      }
    };
    const poll = (token, code, id, uuid, remember, deadline) => {
      if (token !== generation) return;
      if (Date.now() > deadline) {
        result = { ...result, state_msg: WAITING, failed_msg: 'timeout' };
        return;
      }
      const url = new URL(apiUrl('/api/oidc/auth-query'), window.location.href);
      url.searchParams.set('code', code);
      url.searchParams.set('id', id);
      url.searchParams.set('uuid', uuid);
      fetch(url.toString(), { method: 'GET' })
        .then(parseJsonResponse)
        .then((body) => {
          if (token !== generation) return;
          if (body && body.type === 'access_token') {
            storeAuthBody(body, remember);
            result = { ...result, state_msg: LOGIN, failed_msg: '', auth_body: body };
            return;
          }
          timer = window.setTimeout(() => poll(token, code, id, uuid, remember, deadline), 1000);
        })
        .catch((err) => {
          if (token !== generation) return;
          const message = String((err && err.message) || err || '');
          if (message.includes('No authed oidc is found')) {
            timer = window.setTimeout(() => poll(token, code, id, uuid, remember, deadline), 1000);
          } else {
            result = { ...result, state_msg: WAITING, failed_msg: message || 'OauthFailed' };
          }
        });
    };
    const start = (raw) => {
      generation += 1;
      clearTimer();
      result = { ...blankResult(), state_msg: REQUESTING };
      let parsed = {};
      try {
        parsed = raw ? JSON.parse(raw) : {};
      } catch (_) {
        parsed = {};
      }
      const op = String(parsed.op || '').trim();
      const remember = parsed.remember === true || parsed.remember === 'true';
      if (!op) {
        result = { ...result, failed_msg: 'ParamsError' };
        return;
      }
      const id = localStorage.getItem('server-id') || localStorage.getItem('server_id') || 'webclient';
      const uuid = localStorage.getItem('uuid') || '';
      const token = generation;
      openAuthPopup();
      fetch(apiUrl('/api/oidc/auth'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ op, id, uuid, deviceInfo: deviceInfo() }),
      })
        .then(parseJsonResponse)
        .then((body) => {
          if (token !== generation) return;
          if (!body || !body.code || !body.url) throw new Error('Invalid auth response');
          result = { ...result, state_msg: WAITING, failed_msg: '', url: body.url };
          navigatePopup(body.url);
          poll(token, body.code, id, uuid, remember, Date.now() + 180000);
        })
        .catch((err) => {
          if (token !== generation) return;
          result = {
            ...result,
            state_msg: REQUESTING,
            failed_msg: String((err && err.message) || err || 'OauthFailed'),
          };
        });
    };
    const cancel = () => {
      generation += 1;
      clearTimer();
      result = blankResult();
    };
    const wrapSetByName = (original) => {
      if (typeof original !== 'function' || original.__rdConsoleOauthBridgeWrapped) return original;
      const wrapped = function(name, value) {
        if (name === 'account_auth') return start(value);
        if (name === 'account_auth_cancel') return cancel();
        return original.apply(this, arguments);
      };
      wrapped.__rdConsoleOauthBridgeWrapped = true;
      return wrapped;
    };
    const wrapGetByName = (original) => {
      if (typeof original !== 'function' || original.__rdConsoleOauthBridgeWrapped) return original;
      const wrapped = function(name) {
        if (name === 'account_auth_result') return JSON.stringify(result);
        return original.apply(this, arguments);
      };
      wrapped.__rdConsoleOauthBridgeWrapped = true;
      return wrapped;
    };
    const trap = (name, wrap) => {
      let current = typeof window[name] === 'function' ? wrap(window[name]) : window[name];
      try {
        Object.defineProperty(window, name, {
          configurable: true,
          enumerable: true,
          get() {
            return current;
          },
          set(next) {
            current = typeof next === 'function' ? wrap(next) : next;
          },
        });
      } catch (_) {
        if (typeof current === 'function') window[name] = current;
      }
    };
    trap('setByName', wrapSetByName);
    trap('getByName', wrapGetByName);
    return { start, cancel, result: () => result };
  })();
"#
}

fn safe_script_json(value: serde_json::Value) -> String {
    serde_json::to_string(&value)
        .unwrap_or_else(|_| "{}".to_string())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
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

pub async fn webclient_v1_index(State(state): State<AppState>) -> Response {
    let _ = state;
    embedded_web_asset("", WebClientVersion::V1)
}

pub async fn webclient_v1_path(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Response {
    let _ = state;
    embedded_web_asset(&path, WebClientVersion::V1)
}

pub async fn webclient_v2_index(State(state): State<AppState>) -> Response {
    let Some(external) = &state.external_webclient else {
        return webclient_v2_unavailable();
    };
    external_web_asset(&state, external, "", WebClientVersion::V2)
        .await
        .unwrap_or_else(webclient_v2_unavailable)
}

pub async fn webclient_v2_path(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Response {
    let Some(external) = &state.external_webclient else {
        return webclient_v2_unavailable();
    };
    external_web_asset(&state, external, &path, WebClientVersion::V2)
        .await
        .unwrap_or_else(webclient_v2_unavailable)
}

fn webclient_v2_unavailable() -> Response {
    (
        StatusCode::NOT_FOUND,
        "WebClient v2 is unavailable because data/web.zip was not loaded",
    )
        .into_response()
}

/// Serve `<path>` from the embedded admin SPA, with SPA fallback to `index.html`.
async fn admin_asset(path: &str) -> Response {
    let path = if path.is_empty() { "index.html" } else { path };
    if let Some(f) = AdminAssets::get(path) {
        return respond(f.data.into_owned(), path);
    }
    if path.starts_with("assets/") || FsPath::new(path).extension().is_some() {
        return (StatusCode::NOT_FOUND, "Admin asset not found").into_response();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn missing_admin_assets_return_not_found_instead_of_spa_html() {
        let response = admin_asset("assets/missing-chunk.js").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = admin_asset("missing.css").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    fn sample_webclient_config() -> WebClientConfig {
        WebClientConfig {
            id_server: "rd.czyt.tech".to_string(),
            relay_server: "rd.czyt.tech:21117".to_string(),
            api_server: "https://rd-console.czyt.tech".to_string(),
            ws_host: "https://rd-ws.czyt.tech".to_string(),
            ws_id_host: String::new(),
            ws_relay_host: String::new(),
            key: "ERRhI72zmZHHN5tXiCnS".to_string(),
        }
    }

    fn full_webclient_config() -> WebClientConfig {
        WebClientConfig {
            id_server: "id.example.com:21116".to_string(),
            relay_server: "relay.example.com:21117".to_string(),
            api_server: "https://api.example.com".to_string(),
            ws_host: "https://ws.example.com".to_string(),
            ws_id_host: "wss://id-ws.example.com:21118".to_string(),
            ws_relay_host: "wss://relay-ws.example.com:21119".to_string(),
            key: "server-public-key".to_string(),
        }
    }

    fn ini_map(decoded: &str) -> HashMap<&str, &str> {
        decoded
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect()
    }

    #[test]
    fn external_custom_config_includes_webclient_defaults() {
        let encoded = external_custom_config(&full_webclient_config());
        let decoded = String::from_utf8(STANDARD.decode(encoded).unwrap()).unwrap();
        let fields = ini_map(&decoded);

        assert!(decoded.contains("[default-settings]"));
        assert_eq!(
            fields.get("custom-rendezvous-server"),
            Some(&"id.example.com:21116")
        );
        assert_eq!(fields.get("relay-server"), Some(&"relay.example.com:21117"));
        assert_eq!(fields.get("api-server"), Some(&"https://api.example.com"));
        assert_eq!(fields.get("ws-host"), Some(&"https://ws.example.com"));
        assert_eq!(
            fields.get("ws-id-host"),
            Some(&"wss://id-ws.example.com:21118")
        );
        assert_eq!(
            fields.get("ws-relay-host"),
            Some(&"wss://relay-ws.example.com:21119")
        );
        assert_eq!(fields.get("key"), Some(&"server-public-key"));
    }

    #[test]
    fn external_bootstrap_writes_v2_active_config_and_ws_routes() {
        let js = webclient_bootstrap_js(&sample_webclient_config(), 1, true);

        assert!(js.contains("\"custom-rendezvous-server\":\"rd.czyt.tech\""));
        assert!(js.contains("\"api-server\":\"https://rd-console.czyt.tech\""));
        assert!(js.contains("\"ws-host\":\"https://rd-ws.czyt.tech\""));
        assert!(js.contains("\"id\":\"wss://rd-ws.czyt.tech/ws/id\""));
        assert!(js.contains("setDefault(wcPrefix + name, value)"));
        assert!(js.contains("account_auth"));
        assert!(js.contains("/api/oidc/auth-query"));
        assert!(js.contains("Proxy(NativeWebSocket"));
        assert!(js.contains("rewriteWsUrl(args[0])"));
    }

    #[test]
    fn bootstrap_writes_every_input_server_setting() {
        let js = webclient_bootstrap_js(&full_webclient_config(), 1, true);

        assert!(js.contains("\"custom-rendezvous-server\":\"id.example.com:21116\""));
        assert!(js.contains("\"relay-server\":\"relay.example.com:21117\""));
        assert!(js.contains("\"api-server\":\"https://api.example.com\""));
        assert!(js.contains("\"ws-host\":\"https://ws.example.com\""));
        assert!(js.contains("\"ws-id-host\":\"wss://id-ws.example.com:21118\""));
        assert!(js.contains("\"ws-relay-host\":\"wss://relay-ws.example.com:21119\""));
        assert!(js.contains("\"key\":\"server-public-key\""));
        assert!(js.contains("\"id\":\"wss://id-ws.example.com:21118\""));
        assert!(js.contains("\"relay\":\"wss://relay-ws.example.com:21119\""));
    }

    #[test]
    fn rewrites_index_base_for_webclient_version() {
        let html = r#"<html><head><base href="/webclient/"></head></html>"#.to_string();

        let v1 = rewrite_webclient_index_html(html.clone(), WebClientVersion::V1);
        assert!(v1.contains(r#"<base href="/webclient/">"#));

        let v2 = rewrite_webclient_index_html(html, WebClientVersion::V2);
        assert!(v2.contains(r#"<base href="/webclient2/">"#));
        assert!(!v2.contains(r#"<base href="/webclient/">"#));
    }

    #[test]
    fn webclient_v2_is_not_available_without_external_zip() {
        let response = webclient_v2_unavailable();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn embedded_bootstrap_does_not_patch_websocket_constructor() {
        let js = webclient_bootstrap_js(&sample_webclient_config(), 1, false);

        assert!(js.contains("setDefault(wcPrefix + name, value)"));
        assert!(!js.contains("Proxy(NativeWebSocket"));
    }

    #[test]
    fn external_bootstrap_escapes_script_breakout_values() {
        let mut cfg = sample_webclient_config();
        cfg.key = "</script><script>alert(1)</script>".to_string();
        let tag = external_webclient_bootstrap_tag(&cfg, 1);

        assert!(!tag.contains("</script><script>alert(1)"));
        assert!(tag.contains("\\u003c/script\\u003e"));
    }

    #[test]
    fn injects_external_bootstrap_before_module_script() {
        let html = r#"<html><head><script id="custom-config"></script><script type="module" src="js/dist/index.js"></script></head></html>"#;
        let out = inject_external_bootstrap(html.to_string(), "<script>boot</script>");

        assert!(out.find("boot").unwrap() < out.find(r#"<script type="module""#).unwrap());
    }

    #[test]
    fn patches_external_v2_ws_route_function() {
        let js = r#"const a=1;function Ce(u=!1){const e=k.getItem("custom-rendezvous-server");return ce(e||s1,u)}const b=2;"#;
        let patched = patch_external_webclient_js(js);

        assert!(patched.contains("k.getItem(\"ws-host\")"));
        assert!(patched.contains("__rdConsoleWsRoute(h,u,!0)"));
        assert!(!patched.contains("return ce(e||s1,u)}const b=2"));
    }

    #[test]
    fn leaves_unmatched_external_js_unchanged() {
        let js = "const untouched = true;";

        assert_eq!(patch_external_webclient_js(js), js);
    }
}

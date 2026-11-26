//! Admin-managed release links and the capability-protected download page.

use axum::{
    extract::{rejection::PathRejection, DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, put},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use entity::client_download_portal as portal;
use rand::{rngs::OsRng, RngCore};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{features::self_signed_client::OperationError, state::AppState};

use super::{middleware::AdminUser, response as resp, security_audit::AuditContext};

const PUBLIC_PATH: &str = "/client-downloads/";
const TOKEN_BYTES: usize = 32;
const MAX_PACKAGES: usize = 16;
const MAX_URL_BYTES: usize = 2048;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/client-downloads",
            get(list)
                .post(publish)
                .layer(DefaultBodyLimit::max(48 * 1024)),
        )
        .route(
            "/api/admin/client-downloads/settings",
            put(set_enabled).layer(DefaultBodyLimit::max(1024)),
        )
        .route("/api/admin/client-downloads/:id", delete(revoke))
        .route("/client-downloads/:token", get(public_page))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsUpdate {
    enabled: bool,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Platform {
    Windows,
    Macos,
    Linux,
}

impl Platform {
    fn label(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Macos => "macOS",
            Self::Linux => "Linux",
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Architecture {
    X86_64,
    X86,
    Aarch64,
    Armv7,
    Riscv64,
    Universal,
}

impl Architecture {
    fn label(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::X86 => "x86",
            Self::Aarch64 => "ARM64",
            Self::Armv7 => "ARMv7",
            Self::Riscv64 => "RISC-V 64",
            Self::Universal => "Universal",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Package {
    platform: Platform,
    architecture: Architecture,
    url: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Release {
    version: String,
    packages: Vec<Package>,
}

impl Release {
    fn validate(mut self) -> Result<Self, &'static str> {
        self.version = self.version.trim().to_owned();
        if self.version.len() > 64
            || !self
                .version
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            || !self.version.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'_')
            })
        {
            return Err("ClientDownloadsInvalidVersion");
        }
        if !(1..=MAX_PACKAGES).contains(&self.packages.len()) {
            return Err("ClientDownloadsInvalidPackages");
        }
        for package in &mut self.packages {
            let sha256 = package.sha256.trim();
            if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("ClientDownloadsInvalidChecksum");
            }
            package.sha256 = sha256.to_ascii_lowercase();
            let url = package_url(package.url.trim()).ok_or("ClientDownloadsInvalidUrl")?;
            package.url = url.into();
        }
        Ok(self)
    }
}

fn package_url(value: &str) -> Option<Url> {
    if value.len() > MAX_URL_BYTES
        || !value
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || ch == '\\')
    {
        return None;
    }
    let url = Url::parse(value).ok()?;
    (url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.as_str().len() <= MAX_URL_BYTES)
        .then_some(url)
}

#[derive(Serialize)]
struct PortalView {
    id: i32,
    path: String,
    #[serde(flatten)]
    release: Release,
}

impl TryFrom<portal::Model> for PortalView {
    type Error = serde_json::Error;

    fn try_from(row: portal::Model) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            path: format!("{PUBLIC_PATH}{}", row.token),
            release: Release {
                version: row.version,
                packages: serde_json::from_str(&row.packages)?,
            },
        })
    }
}

async fn set_enabled(
    State(state): State<AppState>,
    _admin: AdminUser,
    headers: HeaderMap,
    Json(settings): Json<SettingsUpdate>,
) -> Response {
    let lang = language(&headers);
    match state
        .features
        .set_download_portal_enabled(settings.enabled)
        .await
    {
        Ok(()) => resp::success(serde_json::json!({ "enabled": settings.enabled })),
        Err(error) => feature_failure(&state, lang, error),
    }
}

async fn list(State(state): State<AppState>, _admin: AdminUser, headers: HeaderMap) -> Response {
    let lang = language(&headers);
    if let Err(error) = require_enabled(&state).await {
        return feature_failure(&state, lang, error);
    }
    let Ok(rows) = portal::Entity::find()
        .order_by_desc(portal::Column::Id)
        .all(&state.db)
        .await
    else {
        return admin_failure(&state, lang);
    };
    match rows
        .into_iter()
        .map(PortalView::try_from)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(portals) => resp::no_store(resp::success(portals)),
        Err(_) => admin_failure(&state, lang),
    }
}

async fn publish(
    State(state): State<AppState>,
    _admin: AdminUser,
    audit: AuditContext,
    headers: HeaderMap,
    Json(release): Json<Release>,
) -> Response {
    let lang = language(&headers);
    if let Err(error) = require_enabled(&state).await {
        return feature_failure(&state, lang, error);
    }
    let release = match release.validate() {
        Ok(release) => release,
        Err(message) => return resp::fail(101, state.tr(lang, message)),
    };
    let Ok(packages) = serde_json::to_string(&release.packages) else {
        return admin_failure(&state, lang);
    };
    let mut secret = [0u8; TOKEN_BYTES];
    if OsRng.try_fill_bytes(&mut secret).is_err() {
        return admin_failure(&state, lang);
    }
    let token = URL_SAFE_NO_PAD.encode(secret);
    match (portal::ActiveModel {
        version: Set(release.version.clone()),
        packages: Set(packages),
        token: Set(token.clone()),
        ..Default::default()
    })
    .insert(&state.db)
    .await
    {
        Ok(portal) => {
            audit.subject(portal.id);
            resp::no_store(resp::success(PortalView {
                id: portal.id,
                path: format!("{PUBLIC_PATH}{token}"),
                release,
            }))
        }
        Err(_) => admin_failure(&state, lang),
    }
}

async fn revoke(
    State(state): State<AppState>,
    _admin: AdminUser,
    audit: AuditContext,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Response {
    let lang = language(&headers);
    if let Err(error) = require_enabled(&state).await {
        return feature_failure(&state, lang, error);
    }
    audit.subject(id);
    match portal::Entity::delete_by_id(id).exec(&state.db).await {
        Ok(_) => resp::success(()),
        Err(_) => admin_failure(&state, lang),
    }
}

async fn require_enabled(state: &AppState) -> Result<(), OperationError> {
    if state.features.download_portal_enabled().await? {
        Ok(())
    } else {
        Err(OperationError::Disabled)
    }
}

fn feature_failure(state: &AppState, lang: &str, error: OperationError) -> Response {
    match error {
        OperationError::Disabled => resp::fail(101, state.tr(lang, "ClientDownloadsDisabled")),
        _ => admin_failure(state, lang),
    }
}

fn admin_failure(state: &AppState, lang: &str) -> Response {
    tracing::warn!("Client download portal operation failed");
    resp::fail(101, state.tr(lang, "ClientDownloadsFailure"))
}

async fn public_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> Response {
    let lang = language(&headers);
    let missing = || {
        public_response((
            StatusCode::NOT_FOUND,
            Html(document(
                &state,
                lang,
                &escape_html(&state.tr(lang, "ClientDownloadsNotFound")),
            )),
        ))
    };
    let Ok(Path(token)) = path else {
        return missing();
    };
    if !valid_token(&token) || !matches!(state.features.download_portal_enabled().await, Ok(true)) {
        return missing();
    }
    let Ok(Some(row)) = portal::Entity::find()
        .filter(portal::Column::Token.eq(token.as_str()))
        .one(&state.db)
        .await
    else {
        return missing();
    };
    // Database collation can be case-insensitive, but the capability is case-sensitive.
    if row.token != token {
        return missing();
    }
    let Ok(view) = PortalView::try_from(row) else {
        return missing();
    };
    let Ok(release) = view.release.validate() else {
        return missing();
    };
    public_response(Html(render_release(&state, lang, release)))
}

fn valid_token(token: &str) -> bool {
    token.len() == 43
        && URL_SAFE_NO_PAD
            .decode(token)
            .is_ok_and(|bytes| bytes.len() == TOKEN_BYTES)
}

fn language(headers: &HeaderMap) -> &str {
    headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

fn public_response(response: impl IntoResponse) -> Response {
    resp::no_store((
        [
            (header::CONTENT_SECURITY_POLICY, "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"),
            (header::REFERRER_POLICY, "no-referrer"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::HeaderName::from_static("x-robots-tag"), "noindex, nofollow"),
        ],
        response,
    ).into_response())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_release(state: &AppState, lang: &str, release: Release) -> String {
    let download = escape_html(&state.tr(lang, "ClientDownloadsDownload"));
    let universal = escape_html(&state.tr(lang, "ClientDownloadsUniversal"));
    let package_rows = release.packages.into_iter().map(|package| {
        let platform = package.platform.label();
        let arch: &str = match package.architecture {
            Architecture::Universal => &universal,
            architecture => architecture.label(),
        };
        let url = escape_html(&package.url);
        let checksum = escape_html(&package.sha256);
        format!(r#"
<li>
  <div class="package">
    <div><h2>{platform} <span>{arch}</span></h2><p class="source">{url}</p></div>
    <a href="{url}" rel="noreferrer nofollow" aria-label="{download} · {platform} · {arch} · {url}">{download}</a>
  </div>
  <dl><dt>SHA-256</dt><dd><code>{checksum}</code></dd></dl>
</li>"#)
    }).collect::<String>();
    let version = escape_html(&release.version);
    let release_label = escape_html(&state.tr(lang, "ClientDownloadsRelease"));
    let hint = escape_html(&state.tr(lang, "ClientDownloadsPublicHint"));
    document(
        state,
        lang,
        &format!(
            r#"
<p class="release">{release_label} <strong>{version}</strong></p>
<p class="hint">{hint}</p>
<ul>{package_rows}</ul>"#
        ),
    )
}

fn document(state: &AppState, lang: &str, content: &str) -> String {
    let title = escape_html(&state.tr(lang, "ClientDownloadsTitle"));
    let html_lang = escape_html(&state.tr(lang, "ClientDownloadsLanguage"));
    let style = include_str!("client_download.css");
    format!(
        r#"<!doctype html>
<html lang="{html_lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>{style}</style>
</head>
<body><main><h1>{title}</h1>{content}</main></body>
</html>"#
    )
}

#[cfg(test)]
#[path = "client_download_tests.rs"]
mod tests;

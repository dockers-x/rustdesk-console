//! Embedded static assets: the Flutter web client, the admin SPA (when built in
//! at compile time), i18n bundles, templates and the version file. This is what
//! turns the server into a single self-contained binary.

use rust_embed::RustEmbed;

/// Flutter web client + i18n + templates + version, copied from the Go repo.
/// The admin SPA is embedded separately via [`AdminAssets`], so it is excluded
/// here to avoid embedding it twice.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../resources"]
#[exclude = "admin/*"]
pub struct Resources;

/// Admin SPA, built from `lejianwen/rustdesk-api-web` during the Docker/CI build
/// into `resources/admin`. The folder may be absent in a bare checkout, so the
/// build falls back to an empty directory.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../resources/admin"]
pub struct AdminAssets;

impl Resources {
    pub fn read(path: &str) -> Option<Vec<u8>> {
        Self::get(path).map(|f| f.data.into_owned())
    }

    pub fn read_string(path: &str) -> Option<String> {
        Self::read(path).and_then(|b| String::from_utf8(b).ok())
    }
}

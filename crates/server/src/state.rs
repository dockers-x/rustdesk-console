//! Shared application state passed to every handler (≈ the Go `global` package
//! plus `service.AllService`).

use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::config::Config;
use crate::i18n::I18n;
use crate::support::disconnect_store::DisconnectStore;
use crate::support::jwt::Jwt;
use crate::support::login_limiter::LoginLimiter;
use crate::support::oauth_cache::OauthCache;
use crate::support::record_storage_config::RecordStorageConfigStore;
use crate::support::webclient_config::WebClientConfigStore;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Arc<Config>,
    pub jwt: Arc<Jwt>,
    pub limiter: Arc<LoginLimiter>,
    pub i18n: Arc<I18n>,
    pub oauth_cache: Arc<OauthCache>,
    pub disconnect_store: Arc<DisconnectStore>,
    pub webclient_config: Arc<WebClientConfigStore>,
    pub record_storage_config: Arc<RecordStorageConfigStore>,
    pub start_time: Arc<String>,
    pub version: Arc<String>,
}

impl AppState {
    /// Translate a message id for the given `Accept-Language`.
    pub fn tr(&self, lang: &str, id: &str) -> String {
        self.i18n.translate(lang, id)
    }
}

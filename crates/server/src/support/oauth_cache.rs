//! In-memory OAuth state cache with TTL, mirroring the Go `OauthCache` sync.Map
//! + `time.AfterFunc` expiry. Keyed by the `state` value generated at auth start.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DEFAULT_TTL_SECS: u64 = 5 * 60;
const MAX_ENTRIES: usize = 1024;

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct OauthCacheItem {
    pub user_id: i32,
    #[serde(skip)]
    pub source_session_id: Option<i32>,
    #[serde(skip)]
    pub poll_secret: String,
    #[serde(skip)]
    pub(crate) callback_claimed: bool,
    #[serde(skip)]
    pub(crate) identity: Option<crate::services::oauth::OauthUser>,
    #[serde(skip)]
    pub(crate) provider_identity: String,
    #[serde(skip)]
    pub(crate) binding_id: Option<i32>,
    #[serde(skip)]
    pub(crate) failure: Option<&'static str>,
    pub id: String,
    pub op: String,
    pub action: String,
    pub uuid: String,
    pub device_name: String,
    pub device_os: String,
    pub device_type: String,
    #[serde(skip)]
    pub verifier: String,
    #[serde(skip)]
    pub nonce: String,
}

pub const ACTION_LOGIN: &str = "login";
pub const ACTION_BIND: &str = "bind";

struct Entry {
    item: OauthCacheItem,
    created_at: Instant,
    expires_at: Option<Instant>,
}

#[derive(Default)]
pub struct OauthCache {
    inner: Mutex<HashMap<String, Entry>>,
}

impl OauthCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, key: &str, item: OauthCacheItem, expire_secs: u64) {
        let expire_secs = if expire_secs == 0 {
            DEFAULT_TTL_SECS
        } else {
            expire_secs
        };
        let expires_at = if expire_secs > 0 {
            Some(Instant::now() + Duration::from_secs(expire_secs))
        } else {
            None
        };
        let mut guard = self.inner.lock().unwrap();
        prune_locked(&mut guard);
        if guard.len() >= MAX_ENTRIES {
            remove_oldest_locked(&mut guard);
        }
        guard.insert(
            key.to_string(),
            Entry {
                item,
                created_at: Instant::now(),
                expires_at,
            },
        );
    }

    pub fn get(&self, key: &str) -> Option<OauthCacheItem> {
        let mut g = self.inner.lock().unwrap();
        if let Some(e) = g.get(key) {
            if let Some(exp) = e.expires_at {
                if Instant::now() > exp {
                    g.remove(key);
                    return None;
                }
            }
            return Some(e.item.clone());
        }
        None
    }

    pub fn get_poll(&self, code: &str) -> Option<OauthCacheItem> {
        let (key, secret) = code.split_once('.')?;
        self.get(key)
            .filter(|item| !item.poll_secret.is_empty() && item.poll_secret == secret)
    }

    pub fn claim_callback(&self, key: &str) -> Option<OauthCacheItem> {
        let mut entries = self.inner.lock().expect("OAuth cache mutex poisoned");
        prune_locked(&mut entries);
        let entry = entries.get_mut(key)?;
        if entry.item.callback_claimed || entry.item.op == entity::oauth::TYPE_WEBAUTH {
            return None;
        }
        entry.item.callback_claimed = true;
        Some(entry.item.clone())
    }

    pub fn complete_callback(&self, key: &str, item: OauthCacheItem) -> bool {
        let mut entries = self.inner.lock().expect("OAuth cache mutex poisoned");
        prune_locked(&mut entries);
        let Some(entry) = entries.get_mut(key) else {
            return false;
        };
        if !entry.item.callback_claimed
            || entry.item.identity.is_some()
            || entry.item.binding_id.is_some()
            || entry.item.failure.is_some()
            || entry.item.op == entity::oauth::TYPE_WEBAUTH
        {
            return false;
        }
        entry.item = item;
        true
    }

    pub fn take_ready_binding(
        &self,
        code: &str,
        user_id: i32,
        session_id: i32,
    ) -> Option<OauthCacheItem> {
        let (key, secret) = code.split_once('.')?;
        let mut entries = self.inner.lock().expect("OAuth cache mutex poisoned");
        prune_locked(&mut entries);
        let entry = entries.get(key)?;
        if entry.item.action != ACTION_BIND
            || entry.item.poll_secret != secret
            || entry.item.user_id != user_id
            || entry.item.source_session_id != Some(session_id)
            || (entry.item.identity.is_none() && entry.item.failure.is_none())
        {
            return None;
        }
        entries.remove(key).map(|entry| entry.item)
    }

    pub fn take_ready_login(&self, code: &str, console: bool) -> Option<OauthCacheItem> {
        let (key, secret) = code.split_once('.')?;
        let mut entries = self.inner.lock().expect("OAuth cache mutex poisoned");
        prune_locked(&mut entries);
        let entry = entries.get(key)?;
        if entry.item.poll_secret.is_empty()
            || entry.item.poll_secret != secret
            || (entry.item.user_id == 0 && entry.item.failure.is_none())
            || entry.item.action != ACTION_LOGIN
            || (entry.item.device_type == entity::login_log::CLIENT_WEB_ADMIN) != console
        {
            return None;
        }
        entries.remove(key).map(|entry| entry.item)
    }

    pub fn confirm_webauth(&self, key: &str, user_id: i32, session_id: i32) -> bool {
        let mut entries = self.inner.lock().expect("OAuth cache mutex poisoned");
        prune_locked(&mut entries);
        let Some(entry) = entries.get_mut(key) else {
            return false;
        };
        if entry.item.action != ACTION_LOGIN
            || entry.item.op != entity::oauth::TYPE_WEBAUTH
            || entry.item.user_id != 0
        {
            return false;
        }
        entry.item.user_id = user_id;
        entry.item.source_session_id = Some(session_id);
        true
    }
}

fn prune_locked(entries: &mut HashMap<String, Entry>) {
    let now = Instant::now();
    entries.retain(|_, entry| entry.expires_at.map(|exp| now <= exp).unwrap_or(true));
}

fn remove_oldest_locked(entries: &mut HashMap<String, Entry>) {
    if let Some(key) = entries
        .iter()
        .min_by_key(|(_, entry)| entry.created_at)
        .map(|(key, _)| key.clone())
    {
        entries.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> OauthCacheItem {
        OauthCacheItem {
            action: ACTION_LOGIN.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn provider_state_is_not_a_poll_credential_and_ready_login_is_single_use() {
        let cache = OauthCache::new();
        cache.set(
            "provider-state",
            OauthCacheItem {
                user_id: 1,
                poll_secret: "client-secret".into(),
                device_type: entity::login_log::CLIENT_WEB_ADMIN.into(),
                ..item()
            },
            0,
        );
        assert!(cache.get_poll("provider-state").is_none());
        assert!(cache
            .take_ready_login("provider-state.bad-secret", true)
            .is_none());
        assert!(cache
            .take_ready_login("provider-state.client-secret", false)
            .is_none());
        assert!(cache
            .take_ready_login("provider-state.client-secret", true)
            .is_some());
        assert!(cache
            .take_ready_login("provider-state.client-secret", true)
            .is_none());
        assert!(!cache.complete_callback("provider-state", item()));
    }

    #[test]
    fn zero_ttl_uses_default_expiry() {
        let cache = OauthCache::new();
        cache.set("a", item(), 0);

        assert!(cache.get("a").is_some());
        let guard = cache.inner.lock().unwrap();
        assert!(guard.get("a").and_then(|entry| entry.expires_at).is_some());
    }

    #[test]
    fn expired_entries_are_removed_on_get() {
        let cache = OauthCache::new();
        cache.set("a", item(), 1);
        {
            let mut guard = cache.inner.lock().unwrap();
            guard.get_mut("a").unwrap().expires_at = Some(Instant::now() - Duration::from_secs(1));
        }

        assert!(cache.get("a").is_none());
        assert!(!cache.inner.lock().unwrap().contains_key("a"));
    }
}

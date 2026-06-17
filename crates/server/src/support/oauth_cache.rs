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
    pub id: String,
    pub op: String,
    pub action: String,
    pub uuid: String,
    pub device_name: String,
    pub device_os: String,
    pub device_type: String,
    pub open_id: String,
    pub username: String,
    pub name: String,
    pub email: String,
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

    pub fn delete(&self, key: &str) {
        self.inner.lock().unwrap().remove(key);
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

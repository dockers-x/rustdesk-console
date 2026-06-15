//! In-memory OAuth state cache with TTL, mirroring the Go `OauthCache` sync.Map
//! + `time.AfterFunc` expiry. Keyed by the `state` value generated at auth start.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

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
        let expires_at = if expire_secs > 0 {
            Some(Instant::now() + Duration::from_secs(expire_secs))
        } else {
            None
        };
        self.inner
            .lock()
            .unwrap()
            .insert(key.to_string(), Entry { item, expires_at });
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

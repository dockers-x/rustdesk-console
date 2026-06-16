use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct DisconnectStore {
    pending: Mutex<HashMap<String, BTreeSet<i64>>>,
}

impl DisconnectStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_pending(&self, device_key: &str, conn_ids: &[i64]) -> Vec<i64> {
        if device_key.is_empty() {
            return vec![];
        }
        let conn_ids: Vec<i64> = conn_ids.iter().copied().filter(|id| *id > 0).collect();
        if conn_ids.is_empty() {
            return vec![];
        }

        let mut pending = self.pending.lock().unwrap();
        let entry = pending.entry(device_key.to_string()).or_default();
        for conn_id in conn_ids {
            entry.insert(conn_id);
        }
        entry.iter().copied().collect()
    }

    pub fn pending(&self, device_key: &str) -> Vec<i64> {
        if device_key.is_empty() {
            return vec![];
        }
        self.pending
            .lock()
            .unwrap()
            .get(device_key)
            .map(|items| items.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn remove_disconnected(&self, device_key: &str, current_conn_ids: &[i64]) -> Vec<i64> {
        if device_key.is_empty() {
            return vec![];
        }
        let current: BTreeSet<i64> = current_conn_ids
            .iter()
            .copied()
            .filter(|id| *id > 0)
            .collect();
        let mut pending = self.pending.lock().unwrap();
        let Some(items) = pending.get_mut(device_key) else {
            return vec![];
        };
        items.retain(|conn_id| current.contains(conn_id));
        let remaining: Vec<i64> = items.iter().copied().collect();
        if items.is_empty() {
            pending.remove(device_key);
        }
        remaining
    }
}

#[cfg(test)]
mod tests {
    use super::DisconnectStore;

    #[test]
    fn pending_disconnects_remain_until_heartbeat_stops_reporting_them() {
        let store = DisconnectStore::new();

        assert_eq!(
            store.add_pending("uuid-1", &[762, 762, 763]),
            vec![762, 763]
        );
        assert_eq!(store.pending("uuid-1"), vec![762, 763]);

        assert_eq!(store.remove_disconnected("uuid-1", &[762, 888]), vec![762]);
        assert_eq!(store.pending("uuid-1"), vec![762]);

        assert!(store.remove_disconnected("uuid-1", &[888]).is_empty());
        assert!(store.pending("uuid-1").is_empty());
    }

    #[test]
    fn pending_disconnects_ignore_empty_key_and_invalid_conn_ids() {
        let store = DisconnectStore::new();

        assert!(store.add_pending("", &[1]).is_empty());
        assert!(store.add_pending("uuid-1", &[0, -1]).is_empty());
        assert!(store.pending("uuid-1").is_empty());
    }
}

pub mod admin_config;
pub mod deployment_config;
pub mod disconnect_store;
pub mod external_webclient;
pub mod jwt;
pub mod login_limiter;
pub mod oauth_cache;
pub mod oss;
pub mod password;
pub mod random;
pub mod record_storage_config;
pub mod webclient_config;

static CONFIG_PERSISTENCE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::{
        admin_config::{AdminConfigStore, AdminPanelConfig},
        record_storage_config::{RecordStorageConfigForm, RecordStorageConfigStore},
        webclient_config::{WebClientConfig, WebClientConfigStore},
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn concurrent_config_updates_preserve_all_sections() {
        let path = std::env::temp_dir().join(format!("console-config-{}.yaml", uuid::Uuid::new_v4()));
        std::fs::write(&path, format!("# {}\n", "padding".repeat(20_000))).unwrap();
        let admin = AdminConfigStore::new(path.clone(), AdminPanelConfig::default());
        let web = WebClientConfigStore::new(path.clone(), WebClientConfig::default());
        let storage = RecordStorageConfigStore::new(path.clone(), Default::default());
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
        let admin_barrier = barrier.clone();
        let web_barrier = barrier.clone();
        let admin_task = tokio::spawn(async move {
            admin_barrier.wait().await;
            admin
                .update(AdminPanelConfig {
                    title: "Saved title".into(),
                    ..Default::default()
                })
                .await
                .unwrap();
        });
        let web_task = tokio::spawn(async move {
            web_barrier.wait().await;
            web.update(WebClientConfig {
                id_server: "saved.example.com".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        });
        let storage_task = tokio::spawn(async move {
            barrier.wait().await;
            storage
                .update(RecordStorageConfigForm {
                    temp_dir: "saved-recordings".into(),
                    ..Default::default()
                })
                .await
                .unwrap();
        });
        admin_task.await.unwrap();
        web_task.await.unwrap();
        storage_task.await.unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(raw.contains("Saved title"));
        assert!(raw.contains("saved.example.com"));
        assert!(raw.contains("saved-recordings"));
    }
}

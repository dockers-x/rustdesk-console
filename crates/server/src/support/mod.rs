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

fn persist_config_file(path: &std::path::Path, updated: &str) -> anyhow::Result<()> {
    use anyhow::Context as _;
    use std::{fs::OpenOptions, io::Write as _};

    config::Config::builder()
        .add_source(config::File::from_str(updated, config::FileFormat::Yaml))
        .build()
        .with_context(|| format!("validate updated config {}", path.display()))?;
    let permissions = std::fs::metadata(path)
        .with_context(|| format!("read config permissions {}", path.display()))?
        .permissions();
    let temporary_path = path.with_extension(format!("yaml.{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary_path)
        .with_context(|| format!("create temp config {}", temporary_path.display()))?;
    let result = (|| -> anyhow::Result<()> {
        file.write_all(updated.as_bytes())
            .with_context(|| format!("write temp config {}", temporary_path.display()))?;
        file.set_permissions(permissions)
            .with_context(|| format!("set temp config permissions {}", temporary_path.display()))?;
        drop(file);
        std::fs::rename(&temporary_path, path)
            .with_context(|| format!("replace config {}", path.display()))?;
        Ok(())
    })();
    if let Err(error) = result {
        std::fs::remove_file(&temporary_path).with_context(|| {
            format!("{error:#}; remove temp config {}", temporary_path.display())
        })?;
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        admin_config::{AdminConfigStore, AdminPanelConfig},
        record_storage_config::{RecordStorageConfigForm, RecordStorageConfigStore},
        webclient_config::{WebClientConfig, WebClientConfigStore},
    };

    #[tokio::test]
    async fn invalid_patched_yaml_preserves_config_and_settings() {
        let path =
            std::env::temp_dir().join(format!("console-config-{}.yaml", uuid::Uuid::new_v4()));
        let original = "admin:\n  title: Old\n  hello: |\n    <h1>Original welcome</h1>\n  hello-file: ''\njwt:\n  key: test-secret\n";
        std::fs::write(&path, original).unwrap();
        let admin = AdminConfigStore::new(
            path.clone(),
            AdminPanelConfig {
                title: "Old".into(),
                hello: "<h1>Original welcome</h1>".into(),
                ..Default::default()
            },
        );
        let result = admin
            .update(AdminPanelConfig {
                title: "New".into(),
                hello: "New welcome".into(),
                ..Default::default()
            })
            .await;
        let saved = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(result.is_err());
        assert_eq!(saved, original);
        let view = admin.view(None).await;
        assert_eq!(view.title, "Old");
        assert_eq!(view.hello_raw, "<h1>Original welcome</h1>");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn config_update_does_not_follow_fixed_temporary_symlink() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let directory =
            std::env::temp_dir().join(format!("console-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("config.yaml");
        let victim = directory.join("unrelated.txt");
        std::fs::write(&path, "jwt:\n  key: test-secret\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&victim, "must stay unchanged").unwrap();
        symlink(&victim, path.with_extension("yaml.tmp")).unwrap();
        let admin = AdminConfigStore::new(path.clone(), AdminPanelConfig::default());
        let result = admin
            .update(AdminPanelConfig {
                title: "Saved title".into(),
                ..Default::default()
            })
            .await;
        let victim_contents = std::fs::read_to_string(&victim).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        std::fs::remove_dir_all(directory).unwrap();
        result.unwrap();
        assert_eq!(victim_contents, "must stay unchanged");
        assert!(metadata.is_file());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert!(saved.contains("Saved title"));
    }

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

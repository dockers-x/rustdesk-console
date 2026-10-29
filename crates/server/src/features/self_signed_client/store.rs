use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{SignRequest, SignedArtifact, MAX_CONTENT_SIZE};

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredSettings {
    enabled: bool,
    public_key: String,
    private_key: String,
    #[serde(default)]
    profile: Option<SignRequest>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsUpdate {
    pub enabled: bool,
    pub public_key: Option<String>,
    pub private_key: Option<String>,
}

#[derive(Serialize)]
pub struct PublicSettings {
    pub enabled: bool,
    pub public_key: String,
    pub key_configured: bool,
    pub fingerprint: String,
}

#[derive(Debug)]
pub enum OperationError {
    Disabled,
    Invalid(&'static str),
    Storage,
}

#[derive(Clone)]
pub struct FeatureStore {
    path: PathBuf,
    access: Arc<Mutex<()>>,
}

impl FeatureStore {
    pub fn new(config_path: &Path) -> Self {
        let parent = config_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        Self {
            path: parent.join("features/self-signed-client.json"),
            access: Arc::new(Mutex::new(())),
        }
    }

    async fn run<ResultType: Send + 'static>(
        &self,
        operation: impl FnOnce(&Path) -> Result<ResultType, OperationError> + Send + 'static,
    ) -> Result<ResultType, OperationError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = store.access.lock().map_err(|_| OperationError::Storage)?;
            operation(&store.path)
        })
        .await
        .map_err(|_| OperationError::Storage)?
    }

    pub async fn settings(&self) -> Result<PublicSettings, OperationError> {
        self.run(|path| public_settings(&load(path)?)).await
    }

    pub async fn profile(&self) -> Result<SignRequest, OperationError> {
        self.run(|path| {
            let settings = load(path)?;
            require_enabled(&settings)?;
            Ok(settings.profile.unwrap_or_else(|| SignRequest {
                app_name: "RustDesk".to_string(),
                ..Default::default()
            }))
        })
        .await
    }

    pub async fn save_profile(&self, profile: SignRequest) -> Result<SignRequest, OperationError> {
        self.run(move |path| {
            let mut settings = load(path)?;
            require_enabled(&settings)?;
            profile.clone().payload().map_err(OperationError::Invalid)?;
            settings.profile = Some(profile.clone());
            persist(path, &settings)?;
            Ok(profile)
        })
        .await
    }

    pub async fn update(&self, update: SettingsUpdate) -> Result<PublicSettings, OperationError> {
        self.run(move |path| {
            if !update.enabled && (update.public_key.is_some() || update.private_key.is_some()) {
                return Err(OperationError::Disabled);
            }
            let mut settings = load(path)?;
            settings.enabled = update.enabled;
            if let Some(secret) = update.private_key {
                settings.private_key = secret.trim().to_string();
                if !settings.private_key.is_empty() {
                    let pair = keypair(&settings.private_key)?;
                    settings.public_key = STANDARD.encode(pair.public_key().as_ref());
                    let seed = STANDARD
                        .decode(&settings.private_key)
                        .map_err(|_| OperationError::Invalid("invalid private key encoding"))?;
                    let mut canonical = seed[..32].to_vec();
                    canonical.extend_from_slice(pair.public_key().as_ref());
                    settings.private_key = STANDARD.encode(canonical);
                }
            }
            if let Some(public_key) = update.public_key {
                settings.public_key = public_key.trim().to_string();
            }
            let redacted = public_settings(&settings)?;
            persist(path, &settings)?;
            Ok(redacted)
        })
        .await
    }

    pub async fn generate(&self) -> Result<PublicSettings, OperationError> {
        self.run(|path| {
            let mut settings = load(path)?;
            require_enabled(&settings)?;
            let mut seed = [0u8; 32];
            rand::rngs::OsRng
                .try_fill_bytes(&mut seed)
                .map_err(|_| OperationError::Storage)?;
            let pair =
                Ed25519KeyPair::from_seed_unchecked(&seed).map_err(|_| OperationError::Storage)?;
            let mut secret = seed.to_vec();
            secret.extend_from_slice(pair.public_key().as_ref());
            settings.private_key = STANDARD.encode(secret);
            settings.public_key = STANDARD.encode(pair.public_key().as_ref());
            let redacted = public_settings(&settings)?;
            persist(path, &settings)?;
            Ok(redacted)
        })
        .await
    }

    pub async fn sign(&self, payload: Vec<u8>) -> Result<SignedArtifact, OperationError> {
        self.run(move |path| {
            let settings = load(path)?;
            require_enabled(&settings)?;
            let redacted = public_settings(&settings)?;
            if !redacted.key_configured {
                return Err(OperationError::Invalid("configure a private signing key first"));
            }
            if payload.len().saturating_add(64).saturating_add(2) / 3 * 4 > MAX_CONTENT_SIZE {
                return Err(OperationError::Invalid("signed client configuration exceeds 1 MiB"));
            }
            let signature = keypair(&settings.private_key)?.sign(&payload);
            let mut signed = signature.as_ref().to_vec();
            signed.extend_from_slice(&payload);
            let artifact = SignedArtifact {
                filename: "custom.txt",
                content: STANDARD.encode(signed),
                public_key: redacted.public_key,
                fingerprint: redacted.fingerprint,
                source_hint: "Replace the KEY constant in src/common.rs::read_custom_client with this public key in your own client source and build it yourself. Stock official clients do not trust your signing key. Keep the private feature file on the server; never ship it with clients.",
            };
            let response_size = serde_json::to_vec(&artifact)
                .map_err(|_| OperationError::Storage)?
                .len();
            if response_size.saturating_add(128) > MAX_CONTENT_SIZE {
                return Err(OperationError::Invalid("signed client configuration exceeds 1 MiB"));
            }
            Ok(artifact)
        }).await
    }
}

fn require_enabled(settings: &StoredSettings) -> Result<(), OperationError> {
    if settings.enabled {
        Ok(())
    } else {
        Err(OperationError::Disabled)
    }
}

fn keypair(encoded: &str) -> Result<Ed25519KeyPair, OperationError> {
    let secret = STANDARD
        .decode(encoded)
        .map_err(|_| OperationError::Invalid("invalid private key encoding"))?;
    match secret.len() {
        32 => Ed25519KeyPair::from_seed_unchecked(&secret),
        64 => Ed25519KeyPair::from_seed_and_public_key(&secret[..32], &secret[32..]),
        _ => {
            return Err(OperationError::Invalid(
                "private key must be a base64 seed32 or libsodium secret64",
            ))
        }
    }
    .map_err(|_| OperationError::Invalid("invalid Ed25519 private key pair"))
}

fn public_settings(settings: &StoredSettings) -> Result<PublicSettings, OperationError> {
    let public = STANDARD
        .decode(&settings.public_key)
        .map_err(|_| OperationError::Invalid("invalid public key encoding"))?;
    if !public.is_empty() && public.len() != 32 {
        return Err(OperationError::Invalid(
            "public key must be base64 Ed25519 public32",
        ));
    }
    if !settings.private_key.is_empty()
        && keypair(&settings.private_key)?.public_key().as_ref() != public
    {
        return Err(OperationError::Invalid(
            "public and private keys do not match",
        ));
    }
    let fingerprint = if public.is_empty() {
        String::new()
    } else {
        STANDARD.encode(Sha256::digest(&public))
    };
    Ok(PublicSettings {
        enabled: settings.enabled,
        public_key: settings.public_key.clone(),
        key_configured: !settings.private_key.is_empty(),
        fingerprint,
    })
}

fn load(path: &Path) -> Result<StoredSettings, OperationError> {
    if let Some(directory) = path.parent() {
        match fs::symlink_metadata(directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(OperationError::Storage);
            }
            Ok(metadata) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o777 != 0o700 {
                        return Err(OperationError::Storage);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(OperationError::Storage),
        }
    }
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StoredSettings::default())
        }
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(OperationError::Storage)
        }
        Err(_) => return Err(OperationError::Storage),
        Ok(metadata) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o777 != 0o600 {
                    return Err(OperationError::Storage);
                }
            }
        }
    }
    let mut contents = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take((MAX_CONTENT_SIZE + 1) as u64)
                .read_to_end(&mut contents)
        })
        .map_err(|_| OperationError::Storage)?;
    if contents.len() > MAX_CONTENT_SIZE {
        return Err(OperationError::Storage);
    }
    serde_json::from_slice(&contents).map_err(|_| OperationError::Storage)
}

fn persist(path: &Path, settings: &StoredSettings) -> Result<(), OperationError> {
    let directory = path.parent().ok_or(OperationError::Storage)?;
    if !directory.exists() {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(directory)
            .map_err(|_| OperationError::Storage)?;
    }
    let metadata = fs::symlink_metadata(directory).map_err(|_| OperationError::Storage)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(OperationError::Storage);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| OperationError::Storage)?;
    }
    let temporary = directory.join(format!(".self-signed-client-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| OperationError::Storage)?;
        let bytes = serde_json::to_vec(settings).map_err(|_| OperationError::Storage)?;
        if bytes.len() > MAX_CONTENT_SIZE {
            return Err(OperationError::Invalid(
                "saved feature configuration exceeds 1 MiB",
            ));
        }
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| OperationError::Storage)?;
        fs::rename(&temporary, path).map_err(|_| OperationError::Storage)?;
        Ok(())
    })();
    if result.is_err() {
        if let Err(error) = fs::remove_file(&temporary) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("could not remove temporary self-signed feature file");
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{UnparsedPublicKey, ED25519};

    fn store() -> (PathBuf, FeatureStore) {
        let root = std::env::temp_dir().join(format!("console-signing-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let store = FeatureStore::new(&root.join("config.yaml"));
        (root, store)
    }

    #[tokio::test]
    async fn disabled_operations_fail_closed_without_requiring_a_key_to_enable() {
        let (root, store) = store();
        assert!(!store.settings().await.unwrap().enabled);
        assert!(matches!(
            store.profile().await,
            Err(OperationError::Disabled)
        ));
        assert!(matches!(
            store.save_profile(SignRequest::default()).await,
            Err(OperationError::Disabled)
        ));
        for error in [
            store.generate().await.err(),
            store.sign(b"{}".to_vec()).await.err(),
        ] {
            assert!(matches!(error, Some(OperationError::Disabled)));
        }
        let settings = store
            .update(SettingsUpdate {
                enabled: true,
                public_key: None,
                private_key: None,
            })
            .await
            .unwrap();
        assert!(settings.enabled);
        assert!(!settings.key_configured);
        assert!(matches!(
            store.sign(b"{}".to_vec()).await,
            Err(OperationError::Invalid(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn generated_key_signs_client_payload_and_never_appears_in_responses() {
        let (root, store) = store();
        store
            .update(SettingsUpdate {
                enabled: true,
                public_key: None,
                private_key: None,
            })
            .await
            .unwrap();
        let settings = store.generate().await.unwrap();
        let request = super::super::SignRequest {
            app_name: "OwnDesk".to_string(),
            default_settings: std::collections::BTreeMap::from([(
                "enable-webrtc".to_string(),
                "Y".to_string(),
            )]),
            ..Default::default()
        };
        let payload = request.payload().unwrap();
        let artifact = store.sign(payload.clone()).await.unwrap();
        let signed = STANDARD.decode(&artifact.content).unwrap();
        let public = STANDARD.decode(&settings.public_key).unwrap();
        UnparsedPublicKey::new(&ED25519, public)
            .verify(&signed[64..], &signed[..64])
            .unwrap();
        assert_eq!(&signed[64..], payload);
        let profile = SignRequest {
            app_name: "SavedDesk".to_string(),
            ..Default::default()
        };
        store.save_profile(profile).await.unwrap();
        assert_eq!(
            FeatureStore::new(&root.join("config.yaml"))
                .profile()
                .await
                .unwrap()
                .app_name,
            "SavedDesk"
        );
        let stored = load(&store.path).unwrap();
        assert_eq!(STANDARD.decode(&stored.private_key).unwrap().len(), 64);
        for response in [
            serde_json::to_string(&settings).unwrap(),
            serde_json::to_string(&artifact).unwrap(),
        ] {
            assert!(!response.contains(&stored.private_key));
            assert!(!response.contains("private_key"));
        }
        let reloaded = FeatureStore::new(&root.join("config.yaml"))
            .settings()
            .await
            .unwrap();
        assert_eq!(reloaded.public_key, settings.public_key);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&store.path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(store.path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        assert!(matches!(
            store.sign(vec![b'x'; MAX_CONTENT_SIZE]).await,
            Err(OperationError::Invalid(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn seed_import_validates_public_and_secret_halves_without_partial_updates() {
        let (root, store) = store();
        let seed = STANDARD.encode([7u8; 32]);
        let settings = store
            .update(SettingsUpdate {
                enabled: true,
                public_key: None,
                private_key: Some(seed),
            })
            .await
            .unwrap();
        assert!(settings.key_configured);
        let before = fs::read(&store.path).unwrap();
        for (public_key, private_key) in [
            (Some(STANDARD.encode([1u8; 32])), None),
            (None, Some(STANDARD.encode([9u8; 64]))),
            (None, Some("not-base64".to_string())),
        ] {
            assert!(store
                .update(SettingsUpdate {
                    enabled: true,
                    public_key,
                    private_key
                })
                .await
                .is_err());
            assert_eq!(fs::read(&store.path).unwrap(), before);
        }
        let cleared = store
            .update(SettingsUpdate {
                enabled: true,
                public_key: Some(String::new()),
                private_key: Some(String::new()),
            })
            .await
            .unwrap();
        assert!(!cleared.key_configured);
        assert!(cleared.public_key.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn existing_insecure_files_and_symlink_directories_are_rejected() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let (root, store) = store();
        store
            .update(SettingsUpdate {
                enabled: false,
                public_key: None,
                private_key: None,
            })
            .await
            .unwrap();
        fs::set_permissions(&store.path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            store.settings().await,
            Err(OperationError::Storage)
        ));
        fs::set_permissions(&store.path, fs::Permissions::from_mode(0o600)).unwrap();
        let directory = store.path.parent().unwrap();
        fs::set_permissions(directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            store.settings().await,
            Err(OperationError::Storage)
        ));
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
        fs::rename(directory, root.join("real-features")).unwrap();
        symlink(root.join("real-features"), directory).unwrap();
        assert!(matches!(
            store.settings().await,
            Err(OperationError::Storage)
        ));
        fs::remove_dir_all(root).unwrap();
    }
}

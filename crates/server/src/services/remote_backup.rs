//! Consistent Console snapshots with independent remote delivery.

use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{ensure, Context};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    bootstrap,
    config::{Config, RecordStorage, DB_TYPE_SQLITE, RECORD_STORAGE_S3, RECORD_STORAGE_WEBDAV},
};

const LOCK: &str = "data/.remote-backup.lock";
const STATUS: &str = "data/remote-backup-status.json";
const WORK: &str = "data/.remote-backup-work";
const DEADLINE: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Settings {
    pub(crate) interval_hours: u32,
    pub(crate) targets: Vec<Target>,
    pub(crate) age_recipient: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Target {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) allow_http: bool,
    pub(crate) storage: RecordStorage,
}

impl std::fmt::Debug for Target {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.debug_struct("BackupTarget")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Settings {
    pub(crate) fn recipient(&self) -> anyhow::Result<age::x25519::Recipient> {
        ensure!(
            !self.age_recipient.is_empty(),
            "Configure an age public recipient before running remote backups"
        );
        self.age_recipient
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid age public recipient"))
    }

    pub(crate) fn validate(&mut self) -> anyhow::Result<()> {
        ensure!(
            self.interval_hours <= 720,
            "Backup interval must be between 0 and 720 hours"
        );
        ensure!(
            self.targets.len() <= 16,
            "Configure at most 16 backup targets"
        );
        self.age_recipient = self.age_recipient.trim().to_string();
        if !self.age_recipient.is_empty() {
            self.recipient()?;
        }
        let mut names = HashSet::new();
        for target in &mut self.targets {
            target.name = target.name.trim().to_string();
            ensure!(
                !target.name.is_empty()
                    && target.name.chars().count() <= 60
                    && !target.name.chars().any(char::is_control),
                "Invalid backup target name"
            );
            ensure!(
                names.insert(target.name.clone()),
                "Duplicate backup target name"
            );
            ensure!(
                matches!(
                    target.storage.r#type.trim(),
                    RECORD_STORAGE_S3 | RECORD_STORAGE_WEBDAV
                ),
                "Unsupported backup target protocol for {}",
                target.name
            );
            target.storage = std::mem::take(&mut target.storage).normalize();
        }
        Ok(())
    }
}

impl Target {
    pub(crate) fn validate_endpoint(&self) -> anyhow::Result<()> {
        let endpoint = if self.storage.r#type == RECORD_STORAGE_S3 {
            &self.storage.s3.endpoint
        } else {
            &self.storage.webdav.url
        };
        let url = url::Url::parse(endpoint)
            .map_err(|_| anyhow::anyhow!("Invalid backup endpoint for {}", self.name))?;
        ensure!(
            (url.scheme() == "https" || (self.allow_http && url.scheme() == "http"))
                && url.has_host()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "Invalid backup endpoint for {}",
            self.name
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Pending,
    Uploaded,
    Failed,
    Interrupted,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Delivery {
    pub(crate) name: String,
    kind: String,
    outcome: Outcome,
    detail: Option<String>,
}

impl Delivery {
    pub(crate) fn uploaded(&self) -> bool {
        matches!(self.outcome, Outcome::Uploaded)
    }
}

#[derive(Serialize, Deserialize)]
struct Run {
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    archive: String,
    snapshot_failed: bool,
    scheduled: bool,
    targets: Vec<Delivery>,
}

#[derive(Serialize)]
pub(crate) struct View {
    sqlite: bool,
    interval_hours: u32,
    age_recipient: String,
    running: bool,
    targets: Vec<Delivery>,
    last_run: Option<Run>,
}

fn lock() -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(LOCK)
}

fn read_run() -> anyhow::Result<Option<Run>> {
    match fs::read(STATUS) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn save_run(run: &Run) -> anyhow::Result<()> {
    let temp = Path::new(WORK).join("status.json");
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&temp)?;
    serde_json::to_writer(&file, run)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, STATUS)?;
    #[cfg(unix)]
    File::open("data")?.sync_all()?;
    Ok(())
}

fn configured_targets(settings: &Settings) -> Vec<Delivery> {
    settings
        .targets
        .iter()
        .map(|target| Delivery {
            name: target.name.clone(),
            kind: target.storage.r#type.clone(),
            outcome: Outcome::Pending,
            detail: None,
        })
        .collect()
}

pub(crate) fn view(config: &Config, settings: &Settings) -> anyhow::Result<View> {
    let sqlite = bootstrap::database_type(config) == DB_TYPE_SQLITE;
    let (running, mut last_run) = if sqlite {
        let lock = lock()?;
        match fs2::FileExt::try_lock_shared(&lock) {
            Ok(()) => (false, read_run()?),
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
            {
                (true, read_run()?)
            }
            Err(error) => return Err(error.into()),
        }
    } else {
        (false, None)
    };
    if let Some(run) = last_run
        .as_mut()
        .filter(|run| !running && run.finished_at.is_none())
    {
        for delivery in &mut run.targets {
            if matches!(delivery.outcome, Outcome::Pending) {
                delivery.outcome = Outcome::Interrupted;
            }
        }
    }
    Ok(View {
        sqlite,
        interval_hours: settings.interval_hours,
        age_recipient: settings.age_recipient.clone(),
        running,
        targets: configured_targets(settings),
        last_run,
    })
}

pub(crate) fn effective_settings(config: &Config, path: &Path) -> anyhow::Result<Settings> {
    crate::support::storage_profiles::Profiles::load(path)?.backup_settings(&config.backup)
}

pub(crate) fn spawn_scheduler(config: Arc<Config>, config_path: PathBuf) {
    if bootstrap::database_type(&config) != DB_TYPE_SQLITE {
        return;
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let result = effective_settings(&config, &config_path)
                .and_then(|settings| start(&config, settings, true));
            if let Err(error) = result {
                tracing::warn!(%error, "Could not start a scheduled Console backup");
            }
        }
    });
}

pub(crate) fn start(config: &Config, settings: Settings, scheduled: bool) -> anyhow::Result<bool> {
    ensure!(
        bootstrap::database_type(config) == DB_TYPE_SQLITE,
        "Remote backups require SQLite"
    );
    if scheduled
        && (settings.interval_hours == 0
            || settings.targets.is_empty()
            || settings.age_recipient.is_empty())
    {
        return Ok(false);
    }
    ensure!(
        !settings.targets.is_empty(),
        "No backup targets are configured"
    );
    let recipient = settings.recipient()?;
    let lock = lock()?;
    if let Err(error) = fs2::FileExt::try_lock_exclusive(&lock) {
        if error.kind() == io::ErrorKind::WouldBlock
            || error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
        {
            return Ok(false);
        }
        return Err(error.into());
    }
    let now = Utc::now();
    if scheduled
        && read_run()?.is_some_and(|run| {
            now < run.started_at + chrono::Duration::hours(i64::from(settings.interval_hours))
        })
    {
        return Ok(false);
    }

    match fs::remove_dir_all(WORK) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    let mut directory = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(WORK)?;
    let mut run = Run {
        started_at: now,
        finished_at: None,
        archive: format!(
            "console-{}-{}.zip.age",
            now.format("%Y%m%dT%H%M%SZ"),
            uuid::Uuid::new_v4()
        ),
        snapshot_failed: false,
        scheduled,
        targets: configured_targets(&settings),
    };
    save_run(&run)?;
    tokio::spawn(async move {
        if let Err(error) = deliver(&settings, recipient, &mut run).await {
            tracing::warn!(%error, "Could not complete Console backup");
        }
        if let Err(error) = tokio::fs::remove_dir_all(WORK).await {
            tracing::warn!(%error, "Could not clean Console backup staging");
        }
        drop(lock);
    });
    Ok(true)
}

async fn deliver(
    settings: &Settings,
    recipient: age::x25519::Recipient,
    run: &mut Run,
) -> anyhow::Result<()> {
    use futures_util::StreamExt as _;

    let path = Path::new(WORK).join(&run.archive);
    let name = run.archive.clone();
    let snapshot = async {
        let directory = Path::new(WORK).join("snapshot");
        bootstrap::backup_sqlite(&directory).await?;
        let path = path.clone();
        tokio::task::spawn_blocking(move || {
            archive(&directory, &path, &recipient)?;
            checksum(&path)
        })
        .await?
    }
    .await;
    match snapshot {
        Ok((bytes, hash)) => {
            let client = client()?;
            let mut uploads = upload_all(settings, &client, &name, &path, &hash, bytes);
            while let Some((index, result)) = uploads.next().await {
                let delivery = &mut run.targets[index];
                delivery.outcome = if result.is_ok() {
                    Outcome::Uploaded
                } else {
                    Outcome::Failed
                };
                delivery.detail = result.err();
                if let Err(error) = save_run(run) {
                    tracing::warn!(%error, "Could not save remote backup progress");
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "Could not create Console backup archive");
            run.snapshot_failed = true;
            for delivery in &mut run.targets {
                delivery.outcome = Outcome::Failed;
            }
        }
    }
    run.finished_at = Some(Utc::now());
    save_run(run).context("could not save Console backup results")
}

pub(crate) async fn publish_file(
    settings: &Settings,
    path: &Path,
) -> anyhow::Result<Vec<Delivery>> {
    use futures_util::StreamExt as _;

    ensure!(
        !settings.targets.is_empty(),
        "No backup targets are configured"
    );
    ensure!(
        path.metadata()?.is_file(),
        "Backup archive must be a regular file"
    );
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Invalid backup filename")?;
    ensure!(
        name.len() <= 255
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
        "Backup filename must use ASCII letters, digits, periods, underscores or hyphens"
    );
    let recipient = settings.recipient()?;
    ensure!(
        name.len() <= 251 && !name.ends_with(".age"),
        "Provide the plaintext archive to encrypt"
    );
    let name = format!("{name}.age");
    let temporary =
        tempfile::tempdir().context("Could not create a private backup staging directory")?;
    let output = temporary.path().join(&name);
    let source = path.to_path_buf();
    let destination = output.clone();
    let (bytes, hash) = tokio::task::spawn_blocking(move || {
        encrypt_archive(&recipient, &source, &destination)?;
        checksum(&destination)
    })
    .await??;
    let client = client()?;
    let mut deliveries = configured_targets(settings);
    let mut uploads = upload_all(settings, &client, &name, &output, &hash, bytes);
    while let Some((index, result)) = uploads.next().await {
        deliveries[index].outcome = if result.is_ok() {
            Outcome::Uploaded
        } else {
            Outcome::Failed
        };
        deliveries[index].detail = result.err();
    }
    Ok(deliveries)
}

fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .timeout(DEADLINE)
        .build()?)
}

fn upload_all<'a>(
    settings: &'a Settings,
    client: &'a reqwest::Client,
    name: &'a str,
    path: &'a Path,
    hash: &'a str,
    bytes: u64,
) -> impl futures_util::Stream<Item = (usize, Result<(), String>)> + Unpin + 'a {
    settings
        .targets
        .iter()
        .enumerate()
        .map(move |(index, target)| async move {
            let result = match target.validate_endpoint() {
                Ok(()) => tokio::time::timeout(
                    DEADLINE,
                    super::record_storage::upload_backup(
                        client,
                        &target.storage,
                        name,
                        path,
                        hash,
                        bytes,
                    ),
                )
                .await
                .unwrap_or_else(|_| Err("Upload timed out".to_string())),
                Err(error) => Err(error.to_string()),
            };
            (index, result)
        })
        .collect::<futures_util::stream::FuturesUnordered<_>>()
}

fn archive(
    directory: &Path,
    path: &Path,
    recipient: &age::x25519::Recipient,
) -> anyhow::Result<()> {
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))?;
    let ciphertext = encryptor.wrap_output(File::create(path)?)?;
    let mut writer = ZipWriter::new_stream(ciphertext);
    for name in ["manifest.json", "rustdeskapi.db"] {
        writer.start_file(
            name,
            SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .unix_permissions(0o600)
                .large_file(true),
        )?;
        io::copy(&mut File::open(directory.join(name))?, &mut writer)?;
    }
    writer.finish()?.into_inner().finish()?.sync_all()?;
    fs::remove_dir_all(directory)?;
    Ok(())
}

fn encrypt_archive(
    recipient: &age::x25519::Recipient,
    source: &Path,
    path: &Path,
) -> anyhow::Result<()> {
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))?;
    let mut input = File::open(source)?;
    let mut output = encryptor.wrap_output(File::create(path)?)?;
    io::copy(&mut input, &mut output)?;
    output.finish()?.sync_all()?;
    Ok(())
}

fn checksum(path: &Path) -> anyhow::Result<(u64, String)> {
    use sha2::{Digest as _, Sha256};

    let mut hash = Sha256::new();
    let bytes = io::copy(&mut File::open(path)?, &mut hash)?;
    Ok((bytes, format!("{:x}", hash.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_archive_requires_public_key_and_can_be_decrypted_with_matching_identity() {
        let mut settings = Settings::default();
        assert!(settings.recipient().is_err());
        let identity = age::x25519::Identity::generate();
        settings.age_recipient = identity.to_public().to_string();
        let recipient = settings.recipient().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("snapshot.zip");
        let output = directory.path().join("snapshot.zip.age");
        let plaintext = b"private database archive fixture".repeat(5_000);
        fs::write(&input, &plaintext).unwrap();

        encrypt_archive(&recipient, &input, &output).unwrap();

        let ciphertext = fs::read(&output).unwrap();
        assert!(ciphertext.starts_with(b"age-encryption.org/v1\n"));
        assert!(!ciphertext
            .windows(32)
            .any(|bytes| bytes == b"private database archive fixture"));
        let wrong = age::x25519::Identity::generate();
        let decryptor = age::Decryptor::new(File::open(&output).unwrap()).unwrap();
        assert!(decryptor
            .decrypt(std::iter::once(&wrong as &dyn age::Identity))
            .is_err());
        let decryptor = age::Decryptor::new(File::open(&output).unwrap()).unwrap();
        let mut stream = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .unwrap();
        let mut restored = Vec::new();
        io::copy(&mut stream, &mut restored).unwrap();
        assert_eq!(restored, plaintext);
    }
}

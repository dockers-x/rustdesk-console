//! Consistent Console snapshots with independent remote delivery.

use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io,
    path::Path,
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
    interval_hours: u32,
    targets: Vec<Target>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Target {
    name: String,
    #[serde(default)]
    allow_http: bool,
    storage: RecordStorage,
}

impl std::fmt::Debug for Target {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.debug_struct("BackupTarget")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Settings {
    pub(crate) fn validate(&mut self) -> anyhow::Result<()> {
        ensure!(
            self.interval_hours <= 720,
            "Backup interval must be between 0 and 720 hours"
        );
        ensure!(
            self.targets.len() <= 16,
            "Configure at most 16 backup targets"
        );
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
    fn validate_endpoint(&self) -> anyhow::Result<()> {
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

fn configured_targets(config: &Config) -> Vec<Delivery> {
    config
        .backup
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

pub(crate) fn view(config: &Config) -> anyhow::Result<View> {
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
        interval_hours: config.backup.interval_hours,
        running,
        targets: configured_targets(config),
        last_run,
    })
}

pub(crate) fn spawn_scheduler(config: Arc<Config>) {
    if bootstrap::database_type(&config) != DB_TYPE_SQLITE
        || config.backup.targets.is_empty()
        || config.backup.interval_hours == 0
    {
        return;
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if let Err(error) = start(config.clone(), true) {
                tracing::warn!(%error, "Could not start a scheduled Console backup");
            }
        }
    });
}

pub(crate) fn start(config: Arc<Config>, scheduled: bool) -> anyhow::Result<bool> {
    ensure!(
        bootstrap::database_type(&config) == DB_TYPE_SQLITE,
        "Remote backups require SQLite"
    );
    ensure!(
        !config.backup.targets.is_empty(),
        "No backup targets are configured"
    );
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
            now < run.started_at + chrono::Duration::hours(i64::from(config.backup.interval_hours))
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
            "console-{}-{}.zip",
            now.format("%Y%m%dT%H%M%SZ"),
            uuid::Uuid::new_v4()
        ),
        snapshot_failed: false,
        scheduled,
        targets: configured_targets(&config),
    };
    save_run(&run)?;
    tokio::spawn(async move {
        if let Err(error) = deliver(&config.backup, &mut run).await {
            tracing::warn!(%error, "Could not complete Console backup");
        }
        if let Err(error) = tokio::fs::remove_dir_all(WORK).await {
            tracing::warn!(%error, "Could not clean Console backup staging");
        }
        drop(lock);
    });
    Ok(true)
}

async fn deliver(settings: &Settings, run: &mut Run) -> anyhow::Result<()> {
    use futures_util::StreamExt as _;

    let path = Path::new(WORK).join(&run.archive);
    let name = run.archive.clone();
    let snapshot = async {
        let directory = Path::new(WORK).join("snapshot");
        bootstrap::backup_sqlite(&directory).await?;
        let path = path.clone();
        tokio::task::spawn_blocking(move || archive(&directory, &path)).await?
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

pub(crate) async fn publish_file(config: &Config, path: &Path) -> anyhow::Result<Vec<Delivery>> {
    use futures_util::StreamExt as _;

    ensure!(
        !config.backup.targets.is_empty(),
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
    let source = path.to_path_buf();
    let (bytes, hash) = tokio::task::spawn_blocking(move || checksum(&source)).await??;
    let client = client()?;
    let mut deliveries = configured_targets(config);
    let mut uploads = upload_all(&config.backup, &client, name, path, &hash, bytes);
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

fn archive(directory: &Path, path: &Path) -> anyhow::Result<(u64, String)> {
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    let mut writer = ZipWriter::new(File::create(path)?);
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
    writer.finish()?.sync_all()?;
    fs::remove_dir_all(directory)?;
    checksum(path)
}

fn checksum(path: &Path) -> anyhow::Result<(u64, String)> {
    use sha2::{Digest as _, Sha256};

    let mut hash = Sha256::new();
    let bytes = io::copy(&mut File::open(path)?, &mut hash)?;
    Ok((bytes, format!("{:x}", hash.finalize())))
}

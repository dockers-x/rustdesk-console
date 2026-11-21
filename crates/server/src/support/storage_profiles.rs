use std::{
    fs::{self, File, OpenOptions},
    io,
    path::Path,
};

use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    config::{RecordStorage, RECORD_STORAGE_LOCAL, RECORD_STORAGE_S3, RECORD_STORAGE_WEBDAV},
    services::remote_backup::{Settings, Target},
};

use super::record_storage_config::{
    merge_form, validate_active, view_from_config, RecordStorageConfigForm, RecordStorageConfigView,
};

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Profiles {
    entries: Vec<Profile>,
    recording: Option<Uuid>,
    backup: Option<BackupSelection>,
}

#[derive(Deserialize, Serialize)]
struct Profile {
    id: Uuid,
    name: String,
    config: RecordStorage,
    allow_http: bool,
    #[serde(default)]
    archived: bool,
}

#[derive(Deserialize)]
pub(crate) struct ProfileForm {
    name: String,
    config: RecordStorageConfigForm,
    #[serde(default)]
    allow_http: bool,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct BackupSelection {
    interval_hours: u32,
    profile_ids: Vec<Uuid>,
    #[serde(default)]
    age_recipient: String,
}

#[derive(Serialize)]
pub(crate) struct ProfilesView {
    entries: Vec<ProfileView>,
    recording: Option<Uuid>,
    backup: Option<BackupSelection>,
}

#[derive(Serialize)]
struct ProfileView {
    id: Uuid,
    name: String,
    config: RecordStorageConfigView,
    allow_http: bool,
    archived: bool,
}

impl Profiles {
    pub(crate) fn load(path: &Path) -> anyhow::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).context("Invalid storage configuration file")
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).context("Could not read storage configuration file"),
        }
    }

    pub(crate) fn edit(
        path: &Path,
        update: impl FnOnce(&mut Self) -> anyhow::Result<()>,
    ) -> anyhow::Result<ProfilesView> {
        let directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(directory)?;
        let lock = private_file()
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))?;
        fs2::FileExt::lock_exclusive(&lock)?;
        let mut profiles = Self::load(path)?;
        update(&mut profiles)?;
        let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
        let result = (|| -> anyhow::Result<()> {
            let file = private_file().create_new(true).open(&temporary)?;
            serde_json::to_writer_pretty(&file, &profiles)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)?;
            #[cfg(unix)]
            File::open(directory)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        Ok(profiles.view())
    }

    pub(crate) fn view(&self) -> ProfilesView {
        ProfilesView {
            entries: self
                .entries
                .iter()
                .map(|profile| ProfileView {
                    id: profile.id,
                    name: profile.name.clone(),
                    config: view_from_config(&profile.config),
                    allow_http: profile.allow_http,
                    archived: profile.archived,
                })
                .collect(),
            recording: self.recording,
            backup: self.backup.clone(),
        }
    }

    pub(crate) fn upsert(&mut self, id: Option<Uuid>, form: ProfileForm) -> anyhow::Result<()> {
        let name = form.name.trim().to_string();
        ensure!(
            !name.is_empty() && name.chars().count() <= 60 && !name.chars().any(char::is_control),
            "Invalid storage name"
        );
        ensure!(
            !self
                .entries
                .iter()
                .any(|profile| Some(profile.id) != id && !profile.archived && profile.name == name),
            "Storage name already exists"
        );
        ensure!(
            matches!(
                form.config.storage_type.as_str(),
                RECORD_STORAGE_LOCAL | RECORD_STORAGE_S3 | RECORD_STORAGE_WEBDAV
            ),
            "Invalid storage type"
        );
        let old = id
            .map(|id| {
                self.entries
                    .iter()
                    .find(|profile| profile.id == id)
                    .context("Storage profile not found")
            })
            .transpose()?;
        ensure!(
            old.is_some() || self.entries.len() < 256,
            "Storage configuration limit reached"
        );
        let baseline = old
            .map(|profile| profile.config.clone())
            .unwrap_or_default();
        let config = merge_form(baseline, form.config).normalize();
        validate_active(&config)?;
        if config.normalized_type() != RECORD_STORAGE_LOCAL {
            Target {
                name: name.clone(),
                storage: config.clone(),
                allow_http: form.allow_http,
            }
            .validate_endpoint()?;
        }
        if let Some(old) = old {
            ensure!(
                same_location(&old.config, &config),
                "Create a new profile to change the storage location"
            );
        }
        if let Some(profile) = self
            .entries
            .iter_mut()
            .find(|profile| Some(profile.id) == id)
        {
            profile.name = name;
            profile.config = config;
            profile.allow_http = form.allow_http;
        } else {
            self.entries.push(Profile {
                id: Uuid::new_v4(),
                name,
                config,
                allow_http: form.allow_http,
                archived: false,
            });
        }
        Ok(())
    }

    pub(crate) fn archive(&mut self, id: Uuid) -> anyhow::Result<()> {
        ensure!(
            self.recording != Some(id)
                && !self
                    .backup
                    .as_ref()
                    .is_some_and(|backup| backup.profile_ids.contains(&id)),
            "This storage profile is selected"
        );
        let profile = self
            .entries
            .iter_mut()
            .find(|profile| profile.id == id)
            .context("Storage profile not found")?;
        profile.archived = true;
        Ok(())
    }

    pub(crate) fn select_recording(&mut self, id: Option<Uuid>) -> anyhow::Result<()> {
        if let Some(id) = id {
            self.active(id)?;
        }
        self.recording = id;
        Ok(())
    }

    pub(crate) fn select_backup(
        &mut self,
        selection: Option<BackupSelection>,
    ) -> anyhow::Result<()> {
        if let Some(selection) = selection.as_ref() {
            ensure!(
                selection.interval_hours == 0 || !selection.profile_ids.is_empty(),
                "Select a backup target before scheduling backups"
            );
            let settings = self.resolve_backup(selection)?;
            if !settings.targets.is_empty() {
                settings.recipient()?;
            }
        }
        self.backup = selection;
        Ok(())
    }

    pub(crate) fn recording_id(&self) -> Option<String> {
        self.recording.map(|id| id.to_string())
    }

    pub(crate) fn recording_config(&self, id: &str) -> anyhow::Result<RecordStorage> {
        let id = Uuid::parse_str(id).context("Invalid recording storage profile")?;
        self.entries
            .iter()
            .find(|profile| profile.id == id)
            .map(|profile| profile.config.clone())
            .context("Recording storage profile is missing")
    }

    pub(crate) fn backup_settings(&self, fallback: &Settings) -> anyhow::Result<Settings> {
        match &self.backup {
            Some(selection) => self.resolve_backup(selection),
            None => Ok(fallback.clone()),
        }
    }

    fn resolve_backup(&self, selection: &BackupSelection) -> anyhow::Result<Settings> {
        ensure!(
            selection.profile_ids.len() <= 16,
            "Configure at most 16 backup targets"
        );
        let mut targets = Vec::with_capacity(selection.profile_ids.len());
        for id in &selection.profile_ids {
            let profile = self.active(*id)?;
            targets.push(Target {
                name: profile.name.clone(),
                storage: profile.config.clone(),
                allow_http: profile.allow_http,
            });
        }
        let mut settings = Settings {
            interval_hours: selection.interval_hours,
            targets,
            age_recipient: selection.age_recipient.clone(),
        };
        settings.validate()?;
        Ok(settings)
    }

    fn active(&self, id: Uuid) -> anyhow::Result<&Profile> {
        self.entries
            .iter()
            .find(|profile| profile.id == id && !profile.archived)
            .context("Storage profile not found")
    }
}

fn private_file() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
}

fn same_location(old: &RecordStorage, new: &RecordStorage) -> bool {
    if old.r#type != new.r#type || old.temp_dir != new.temp_dir {
        return false;
    }
    match old.normalized_type() {
        RECORD_STORAGE_S3 => {
            old.s3.endpoint == new.s3.endpoint
                && old.s3.region == new.s3.region
                && old.s3.bucket == new.s3.bucket
                && old.s3.prefix == new.s3.prefix
                && old.s3.force_path_style == new.s3.force_path_style
        }
        RECORD_STORAGE_WEBDAV => {
            old.webdav.url == new.webdav.url
                && old.webdav.prefix == new.webdav.prefix
                && old.webdav.username == new.webdav.username
        }
        _ => old.local_dir == new.local_dir,
    }
}

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::{
    RecordStorage, RecordStorageS3, RecordStorageWebDav, RECORD_STORAGE_LOCAL, RECORD_STORAGE_S3,
    RECORD_STORAGE_WEBDAV,
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RecordStorageConfigForm {
    #[serde(default, rename = "type")]
    pub storage_type: String,
    #[serde(default)]
    pub local_dir: String,
    #[serde(default)]
    pub temp_dir: String,
    #[serde(default)]
    pub s3: RecordStorageS3Form,
    #[serde(default)]
    pub webdav: RecordStorageWebDavForm,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RecordStorageS3Form {
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default)]
    pub secret_access_key: String,
    #[serde(default)]
    pub force_path_style: bool,
    #[serde(default)]
    pub clear_access_key_id: bool,
    #[serde(default)]
    pub clear_secret_access_key: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RecordStorageWebDavForm {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub clear_password: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RecordStorageConfigView {
    #[serde(rename = "type")]
    pub storage_type: String,
    pub local_dir: String,
    pub temp_dir: String,
    pub s3: RecordStorageS3View,
    pub webdav: RecordStorageWebDavView,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RecordStorageS3View {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub access_key_id_configured: bool,
    pub secret_access_key_configured: bool,
    pub force_path_style: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RecordStorageWebDavView {
    pub url: String,
    pub username: String,
    pub password: String,
    pub password_configured: bool,
    pub prefix: String,
}

#[derive(Debug)]
pub struct RecordStorageConfigStore {
    config_path: PathBuf,
    current: RwLock<RecordStorage>,
}

impl RecordStorageConfigStore {
    pub fn new(config_path: PathBuf, current: RecordStorage) -> Self {
        Self {
            config_path,
            current: RwLock::new(current.normalize()),
        }
    }

    pub async fn get(&self) -> RecordStorage {
        self.current.read().await.clone()
    }

    pub async fn view(&self) -> RecordStorageConfigView {
        view_from_config(&self.get().await)
    }

    pub async fn update(
        &self,
        form: RecordStorageConfigForm,
    ) -> anyhow::Result<RecordStorageConfigView> {
        let mut current = self.current.write().await;
        let next = merge_form(current.clone(), form).normalize();
        validate_active(&next)?;
        persist_to_config_file(&self.config_path, &next)?;
        *current = next.clone();
        Ok(view_from_config(&next))
    }
}

fn view_from_config(cfg: &RecordStorage) -> RecordStorageConfigView {
    RecordStorageConfigView {
        storage_type: cfg.normalized_type().to_string(),
        local_dir: cfg.local_dir.clone(),
        temp_dir: cfg.temp_dir.clone(),
        s3: RecordStorageS3View {
            endpoint: cfg.s3.endpoint.clone(),
            region: cfg.s3.region.clone(),
            bucket: cfg.s3.bucket.clone(),
            prefix: cfg.s3.prefix.clone(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            access_key_id_configured: !cfg.s3.access_key_id.is_empty(),
            secret_access_key_configured: !cfg.s3.secret_access_key.is_empty(),
            force_path_style: cfg.s3.force_path_style,
        },
        webdav: RecordStorageWebDavView {
            url: cfg.webdav.url.clone(),
            username: cfg.webdav.username.clone(),
            password: String::new(),
            password_configured: !cfg.webdav.password.is_empty(),
            prefix: cfg.webdav.prefix.clone(),
        },
    }
}

fn merge_form(mut current: RecordStorage, form: RecordStorageConfigForm) -> RecordStorage {
    current.r#type = form.storage_type.trim().to_string();
    current.local_dir = form.local_dir.trim().to_string();
    current.temp_dir = form.temp_dir.trim().to_string();
    current.s3.endpoint = form.s3.endpoint.trim().to_string();
    current.s3.region = form.s3.region.trim().to_string();
    current.s3.bucket = form.s3.bucket.trim().to_string();
    current.s3.prefix = form.s3.prefix.trim().to_string();
    current.s3.force_path_style = form.s3.force_path_style;
    if form.s3.clear_access_key_id {
        current.s3.access_key_id.clear();
    } else if !form.s3.access_key_id.trim().is_empty() {
        current.s3.access_key_id = form.s3.access_key_id.trim().to_string();
    }
    if form.s3.clear_secret_access_key {
        current.s3.secret_access_key.clear();
    } else if !form.s3.secret_access_key.trim().is_empty() {
        current.s3.secret_access_key = form.s3.secret_access_key.trim().to_string();
    }
    current.webdav.url = form.webdav.url.trim().to_string();
    current.webdav.username = form.webdav.username.trim().to_string();
    current.webdav.prefix = form.webdav.prefix.trim().to_string();
    if form.webdav.clear_password {
        current.webdav.password.clear();
    } else if !form.webdav.password.trim().is_empty() {
        current.webdav.password = form.webdav.password.trim().to_string();
    }
    current
}

fn validate_active(cfg: &RecordStorage) -> anyhow::Result<()> {
    match cfg.normalized_type() {
        RECORD_STORAGE_S3 => {
            if cfg.s3.endpoint.is_empty()
                || cfg.s3.bucket.is_empty()
                || cfg.s3.access_key_id.is_empty()
                || cfg.s3.secret_access_key.is_empty()
            {
                anyhow::bail!("S3 record storage requires endpoint, bucket, access key id and secret access key");
            }
        }
        RECORD_STORAGE_WEBDAV => {
            if cfg.webdav.url.is_empty() {
                anyhow::bail!("WebDAV record storage requires url");
            }
        }
        RECORD_STORAGE_LOCAL => {}
        _ => {}
    }
    Ok(())
}

fn persist_to_config_file(path: &Path, cfg: &RecordStorage) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("read config {}: {}", path.display(), e))?;
    let updated = patch_record_storage_section(&raw, cfg);
    let permissions = std::fs::metadata(path).ok().map(|m| m.permissions());
    let tmp_path = path.with_extension("yaml.tmp");
    std::fs::write(&tmp_path, updated)
        .map_err(|e| anyhow::anyhow!("write temp config {}: {}", tmp_path.display(), e))?;
    if let Some(permissions) = permissions {
        std::fs::set_permissions(&tmp_path, permissions).map_err(|e| {
            anyhow::anyhow!("set temp config permissions {}: {}", tmp_path.display(), e)
        })?;
    }
    std::fs::rename(&tmp_path, path)
        .map_err(|e| anyhow::anyhow!("replace config {}: {}", path.display(), e))?;
    Ok(())
}

fn patch_record_storage_section(raw: &str, cfg: &RecordStorage) -> String {
    let mut lines: Vec<String> = raw.lines().map(ToOwned::to_owned).collect();
    let had_trailing_newline = raw.ends_with('\n');
    let block = record_storage_block(cfg);
    let section_start = lines
        .iter()
        .position(|line| is_top_level_key(line, "record-storage"));

    let Some(start) = section_start else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
            lines.push(String::new());
        }
        lines.extend(block);
        return join_lines(lines, had_trailing_newline);
    };

    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(i, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                None
            } else if !starts_with_whitespace(line) {
                Some(i)
            } else {
                None
            }
        })
        .unwrap_or(lines.len());

    lines.splice(start..end, block);
    join_lines(lines, had_trailing_newline)
}

fn record_storage_block(cfg: &RecordStorage) -> Vec<String> {
    vec![
        "record-storage:".to_string(),
        format!("  type: {}", yaml_double_quoted(cfg.normalized_type())),
        format!("  local-dir: {}", yaml_double_quoted(&cfg.local_dir)),
        format!("  temp-dir: {}", yaml_double_quoted(&cfg.temp_dir)),
        "  s3:".to_string(),
        format!("    endpoint: {}", yaml_double_quoted(&cfg.s3.endpoint)),
        format!("    region: {}", yaml_double_quoted(&cfg.s3.region)),
        format!("    bucket: {}", yaml_double_quoted(&cfg.s3.bucket)),
        format!("    prefix: {}", yaml_double_quoted(&cfg.s3.prefix)),
        format!(
            "    access-key-id: {}",
            yaml_double_quoted(&cfg.s3.access_key_id)
        ),
        format!(
            "    secret-access-key: {}",
            yaml_double_quoted(&cfg.s3.secret_access_key)
        ),
        format!("    force-path-style: {}", cfg.s3.force_path_style),
        "  webdav:".to_string(),
        format!("    url: {}", yaml_double_quoted(&cfg.webdav.url)),
        format!("    username: {}", yaml_double_quoted(&cfg.webdav.username)),
        format!("    password: {}", yaml_double_quoted(&cfg.webdav.password)),
        format!("    prefix: {}", yaml_double_quoted(&cfg.webdav.prefix)),
    ]
}

fn join_lines(lines: Vec<String>, trailing_newline: bool) -> String {
    let mut out = lines.join("\n");
    if trailing_newline {
        out.push('\n');
    }
    out
}

fn is_top_level_key(line: &str, key: &str) -> bool {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix(&format!("{key}:")) else {
        return false;
    };
    !starts_with_whitespace(line) && (rest.trim().is_empty() || rest.trim_start().starts_with('#'))
}

fn starts_with_whitespace(line: &str) -> bool {
    line.chars().next().is_some_and(|ch| ch.is_whitespace())
}

fn yaml_double_quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => {
                let _ = write!(out, "{ch}");
            }
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_s3() -> RecordStorage {
        RecordStorage {
            r#type: RECORD_STORAGE_S3.to_string(),
            local_dir: String::new(),
            temp_dir: String::new(),
            s3: RecordStorageS3 {
                endpoint: "https://s3.example.com".to_string(),
                region: "us-east-1".to_string(),
                bucket: "bucket".to_string(),
                prefix: "record/".to_string(),
                access_key_id: "ak".to_string(),
                secret_access_key: "sk".to_string(),
                force_path_style: true,
            },
            webdav: RecordStorageWebDav::default(),
        }
    }

    #[test]
    fn view_hides_secrets_but_reports_configured_state() {
        let view = view_from_config(&configured_s3());
        assert_eq!(view.s3.access_key_id, "");
        assert_eq!(view.s3.secret_access_key, "");
        assert!(view.s3.access_key_id_configured);
        assert!(view.s3.secret_access_key_configured);
    }

    #[test]
    fn merge_preserves_secrets_when_secret_fields_are_empty() {
        let current = configured_s3();
        let form = RecordStorageConfigForm {
            storage_type: RECORD_STORAGE_S3.to_string(),
            local_dir: String::new(),
            temp_dir: String::new(),
            s3: RecordStorageS3Form {
                endpoint: "https://new.example.com".to_string(),
                region: "us-west-2".to_string(),
                bucket: "new-bucket".to_string(),
                prefix: "new".to_string(),
                access_key_id: String::new(),
                secret_access_key: String::new(),
                force_path_style: false,
                clear_access_key_id: false,
                clear_secret_access_key: false,
            },
            webdav: RecordStorageWebDavForm::default(),
        };
        let merged = merge_form(current, form).normalize();
        assert_eq!(merged.s3.endpoint, "https://new.example.com");
        assert_eq!(merged.s3.prefix, "new/");
        assert_eq!(merged.s3.access_key_id, "ak");
        assert_eq!(merged.s3.secret_access_key, "sk");
    }

    #[test]
    fn patch_replaces_only_record_storage_block() {
        let raw = "app:\n  web-client: 1\nrecord-storage:\n  type: \"local\"\nlogger:\n  level: \"info\"\n";
        let out = patch_record_storage_section(raw, &configured_s3());
        assert!(out.contains("app:\n  web-client: 1\nrecord-storage:\n"));
        assert!(out.contains("  type: \"s3\""));
        assert!(out.contains("    access-key-id: \"ak\""));
        assert!(out.contains("logger:\n  level: \"info\""));
    }
}

use std::fmt::Write as _;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::config::Admin;

const MAX_LOGO_BYTES: usize = 256 * 1024;
const MAX_LOGO_DIMENSION: u32 = 512;
pub const FAVICON_PATH: &str = "/api/admin/config/favicon";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AdminPanelConfig {
    pub title: String,
    pub hello: String,
    pub hello_file: String,
    #[serde(default)]
    pub logo: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminPanelConfigView {
    pub title: String,
    pub hello: String,
    pub hello_raw: String,
    pub hello_file: String,
    pub logo: String,
    pub logo_url: Option<String>,
}

impl From<&Admin> for AdminPanelConfig {
    fn from(value: &Admin) -> Self {
        Self {
            title: value.title.clone(),
            hello: value.hello.clone(),
            hello_file: value.hello_file.clone(),
            logo: (!value.logo.is_empty() && logo_is_valid(&value.logo))
                .then(|| value.logo.clone()),
        }
    }
}

impl AdminPanelConfig {
    fn normalized(self) -> Self {
        Self {
            title: self.title.trim().to_string(),
            hello: self.hello.trim().to_string(),
            hello_file: self.hello_file.trim().to_string(),
            logo: self.logo,
        }
    }
}

pub fn logo_is_valid(logo: &str) -> bool {
    if logo.is_empty() {
        return true;
    }
    if logo.len() > MAX_LOGO_BYTES.div_ceil(3) * 4 {
        return false;
    }
    let Ok(bytes) = STANDARD.decode(logo) else {
        return false;
    };
    if bytes.len() > MAX_LOGO_BYTES {
        return false;
    }
    let decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: 4 * 1024 * 1024,
        },
    );
    let Ok(mut reader) = decoder.read_info() else {
        return false;
    };
    let info = reader.info();
    if info.width > MAX_LOGO_DIMENSION
        || info.height > MAX_LOGO_DIMENSION
        || info.animation_control.is_some()
    {
        return false;
    }
    let mut output = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut output).is_ok() && reader.finish().is_ok()
}

#[derive(Debug)]
pub struct AdminConfigStore {
    config_path: PathBuf,
    current: RwLock<AdminPanelConfig>,
}

impl AdminConfigStore {
    pub fn new(config_path: PathBuf, current: AdminPanelConfig) -> Self {
        Self {
            config_path,
            current: RwLock::new(current),
        }
    }

    pub async fn view(&self, username: Option<&str>) -> AdminPanelConfigView {
        let current = self.current.read().await.clone();
        let mut hello = current.hello.clone();
        if hello.is_empty() && !current.hello_file.is_empty() {
            if let Ok(file) = std::fs::read_to_string(&current.hello_file) {
                hello = file;
            }
        }
        if let Some(username) = username {
            hello = hello.replace("{{username}}", username);
        }
        let logo = current.logo.unwrap_or_default();
        let logo_url =
            (!logo.is_empty()).then(|| format!("{FAVICON_PATH}?v={:x}", Sha256::digest(&logo)));
        AdminPanelConfigView {
            title: current.title,
            hello,
            hello_raw: current.hello,
            hello_file: current.hello_file,
            logo,
            logo_url,
        }
    }

    pub async fn logo_bytes(&self) -> Option<Vec<u8>> {
        let current = self.current.read().await;
        let encoded = current.logo.as_deref().filter(|logo| !logo.is_empty())?;
        STANDARD.decode(encoded).ok()
    }

    pub async fn update(&self, next: AdminPanelConfig) -> anyhow::Result<AdminPanelConfig> {
        let mut next = next.normalized();
        if next.title.is_empty() {
            next.title = "RustDesk Console".to_string();
        }
        let mut current = self.current.write().await;
        if next.logo.is_none() {
            next.logo.clone_from(&current.logo);
        }
        persist_to_config_file(&self.config_path, &next)?;
        *current = next.clone();
        Ok(next)
    }
}

fn persist_to_config_file(path: &Path, cfg: &AdminPanelConfig) -> anyhow::Result<()> {
    let _guard = super::CONFIG_PERSISTENCE_LOCK
        .lock()
        .map_err(|error| anyhow::anyhow!("config persistence lock poisoned: {error}"))?;
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("read config {}: {}", path.display(), e))?;
    let updated = patch_admin_section(&raw, cfg);
    super::persist_config_file(path, &updated)
}

fn patch_admin_section(raw: &str, cfg: &AdminPanelConfig) -> String {
    let mut lines: Vec<String> = raw.lines().map(ToOwned::to_owned).collect();
    let had_trailing_newline = raw.ends_with('\n');
    let section_start = lines
        .iter()
        .position(|line| is_top_level_key(line, "admin"));
    let values = [
        ("title", cfg.title.as_str()),
        ("hello-file", cfg.hello_file.as_str()),
        ("hello", cfg.hello.as_str()),
        ("logo", cfg.logo.as_deref().unwrap_or_default()),
    ];

    let Some(start) = section_start else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
            lines.push(String::new());
        }
        lines.push("admin:".to_string());
        for (key, value) in values {
            lines.push(format!("  {key}: {}", yaml_double_quoted(value)));
        }
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

    let mut seen = std::collections::HashSet::new();
    for line in &mut lines[start + 1..end] {
        for (key, value) in values {
            if is_nested_key(line, key) {
                *line = replace_value_preserving_comment(line, key, value);
                seen.insert(key);
                break;
            }
        }
    }

    let mut insert_at = end;
    for (key, value) in values {
        if seen.contains(key) {
            continue;
        }
        lines.insert(insert_at, format!("  {key}: {}", yaml_double_quoted(value)));
        insert_at += 1;
    }

    join_lines(lines, had_trailing_newline)
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

fn is_nested_key(line: &str, key: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(&format!("{key}:"))
}

fn starts_with_whitespace(line: &str) -> bool {
    line.chars().next().is_some_and(|ch| ch.is_whitespace())
}

fn replace_value_preserving_comment(line: &str, key: &str, value: &str) -> String {
    let indent = line
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect::<String>();
    let comment = split_comment(line).map_or("", |idx| &line[idx..]);
    let mut next = format!("{indent}{key}: {}", yaml_double_quoted(value));
    if !comment.is_empty() {
        if !comment.starts_with(' ') {
            next.push(' ');
        }
        next.push_str(comment.trim_start());
    }
    next
}

fn split_comment(line: &str) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    let mut prev = '\0';
    for (idx, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single && prev != '\\' => in_double = !in_double,
            '#' if !in_single && !in_double => return Some(idx),
            _ => {}
        }
        prev = ch;
    }
    None
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

    fn png_logo(width: u32) -> String {
        let mut bytes = Vec::new();
        let mut png = png::Encoder::new(&mut bytes, width, 1);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()
            .unwrap()
            .write_image_data(&vec![0; width as usize * 4])
            .unwrap();
        STANDARD.encode(bytes)
    }

    #[test]
    fn accepts_only_bounded_decodable_png_logos() {
        assert!(logo_is_valid(&png_logo(512)));
        assert!(!logo_is_valid(&png_logo(513)));
        assert!(!logo_is_valid(&STANDARD.encode("<svg></svg>")));
        assert!(!logo_is_valid(
            &STANDARD.encode(vec![0; MAX_LOGO_BYTES + 1])
        ));
        let mut truncated = STANDARD.decode(png_logo(1)).unwrap();
        truncated.truncate(20);
        assert!(!logo_is_valid(&STANDARD.encode(truncated)));
    }

    #[tokio::test]
    async fn legacy_update_preserves_logo_and_empty_logo_clears_it() {
        let path = std::env::temp_dir().join(format!("console-logo-{}.yaml", uuid::Uuid::new_v4()));
        std::fs::write(&path, "admin:\n  title: Original\n").unwrap();
        let store = AdminConfigStore::new(path.clone(), AdminPanelConfig::default());
        let logo = png_logo(1);
        store
            .update(AdminPanelConfig {
                logo: Some(logo.clone()),
                ..Default::default()
            })
            .await
            .unwrap();
        store
            .update(AdminPanelConfig {
                title: "Renamed".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(store.view(None).await.logo, logo);
        assert_eq!(
            store.logo_bytes().await,
            Some(STANDARD.decode(&logo).unwrap())
        );
        assert!(std::fs::read_to_string(&path).unwrap().contains(&logo));

        store
            .update(AdminPanelConfig {
                logo: Some(String::new()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(store.logo_bytes().await.is_none());
        assert!(store.view(None).await.logo_url.is_none());
        assert!(!std::fs::read_to_string(&path).unwrap().contains(&logo));
        std::fs::remove_file(path).unwrap();
    }
}

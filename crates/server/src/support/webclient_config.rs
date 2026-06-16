use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::Rustdesk;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WebClientConfig {
    #[serde(default)]
    pub id_server: String,
    #[serde(default)]
    pub relay_server: String,
    #[serde(default)]
    pub api_server: String,
    #[serde(default)]
    pub key: String,
}

impl From<&Rustdesk> for WebClientConfig {
    fn from(value: &Rustdesk) -> Self {
        Self {
            id_server: value.id_server.clone(),
            relay_server: value.relay_server.clone(),
            api_server: value.api_server.clone(),
            key: value.key.clone(),
        }
    }
}

impl WebClientConfig {
    fn trimmed(self) -> Self {
        Self {
            id_server: self.id_server.trim().to_string(),
            relay_server: self.relay_server.trim().to_string(),
            api_server: self.api_server.trim().to_string(),
            key: self.key.trim().to_string(),
        }
    }
}

#[derive(Debug)]
pub struct WebClientConfigStore {
    config_path: PathBuf,
    current: RwLock<WebClientConfig>,
}

impl WebClientConfigStore {
    pub fn new(config_path: PathBuf, current: WebClientConfig) -> Self {
        Self {
            config_path,
            current: RwLock::new(current),
        }
    }

    pub async fn get(&self) -> WebClientConfig {
        self.current.read().await.clone()
    }

    pub async fn update(&self, next: WebClientConfig) -> anyhow::Result<WebClientConfig> {
        let next = next.trimmed();
        let mut current = self.current.write().await;
        persist_to_config_file(&self.config_path, &next)?;
        *current = next.clone();
        Ok(next)
    }
}

fn persist_to_config_file(path: &Path, cfg: &WebClientConfig) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("read config {}: {}", path.display(), e))?;
    let updated = patch_rustdesk_section(&raw, cfg);
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

fn patch_rustdesk_section(raw: &str, cfg: &WebClientConfig) -> String {
    let mut lines: Vec<String> = raw.lines().map(ToOwned::to_owned).collect();
    let had_trailing_newline = raw.ends_with('\n');
    let section_start = lines
        .iter()
        .position(|line| is_top_level_key(line, "rustdesk"));

    let values = [
        ("id-server", cfg.id_server.as_str()),
        ("relay-server", cfg.relay_server.as_str()),
        ("api-server", cfg.api_server.as_str()),
        ("key", cfg.key.as_str()),
    ];

    let Some(start) = section_start else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
            lines.push(String::new());
        }
        lines.push("rustdesk:".to_string());
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

    let mut seen = HashSet::new();
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

    #[test]
    fn patches_existing_rustdesk_keys_and_preserves_comments() {
        let raw = r#"app:
  web-client: 1
rustdesk:
  id-server: "old-id"
  relay-server: "old-relay" # keep
  api-server: "http://old"
  key: ""
  key-file: "/data/id_ed25519.pub"
logger:
  level: "info"
"#;
        let out = patch_rustdesk_section(
            raw,
            &WebClientConfig {
                id_server: "id.example:21116".to_string(),
                relay_server: "relay.example:21117".to_string(),
                api_server: "https://api.example".to_string(),
                key: "abc\"def".to_string(),
            },
        );
        assert!(out.contains("  id-server: \"id.example:21116\""));
        assert!(out.contains("  relay-server: \"relay.example:21117\" # keep"));
        assert!(out.contains("  api-server: \"https://api.example\""));
        assert!(out.contains("  key: \"abc\\\"def\""));
        assert!(out.contains("logger:\n  level: \"info\""));
    }

    #[test]
    fn appends_missing_rustdesk_keys() {
        let raw = "rustdesk:\n  key-file: \"pub\"\n";
        let out = patch_rustdesk_section(
            raw,
            &WebClientConfig {
                id_server: "id".to_string(),
                relay_server: "relay".to_string(),
                api_server: "api".to_string(),
                key: "key".to_string(),
            },
        );
        assert!(out.contains("  key-file: \"pub\"\n  id-server: \"id\""));
        assert!(out.ends_with('\n'));
    }
}

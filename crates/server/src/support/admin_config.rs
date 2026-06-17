use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::Admin;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AdminPanelConfig {
    pub title: String,
    pub hello: String,
    pub hello_file: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminPanelConfigView {
    pub title: String,
    pub hello: String,
    pub hello_raw: String,
    pub hello_file: String,
}

impl From<&Admin> for AdminPanelConfig {
    fn from(value: &Admin) -> Self {
        Self {
            title: value.title.clone(),
            hello: value.hello.clone(),
            hello_file: value.hello_file.clone(),
        }
    }
}

impl AdminPanelConfig {
    fn normalized(self) -> Self {
        Self {
            title: self.title.trim().to_string(),
            hello: self.hello.trim().to_string(),
            hello_file: self.hello_file.trim().to_string(),
        }
    }
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
        AdminPanelConfigView {
            title: current.title,
            hello,
            hello_raw: current.hello,
            hello_file: current.hello_file,
        }
    }

    pub async fn update(&self, next: AdminPanelConfig) -> anyhow::Result<AdminPanelConfig> {
        let mut next = next.normalized();
        if next.title.is_empty() {
            next.title = "RustDesk Console".to_string();
        }
        let mut current = self.current.write().await;
        persist_to_config_file(&self.config_path, &next)?;
        *current = next.clone();
        Ok(next)
    }
}

fn persist_to_config_file(path: &Path, cfg: &AdminPanelConfig) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("read config {}: {}", path.display(), e))?;
    let updated = patch_admin_section(&raw, cfg);
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

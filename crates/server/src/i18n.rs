//! Minimal i18n, mirroring `global/i18n.go` semantics: load every `*.toml`
//! bundle from the embedded `i18n/` directory and translate a message id by
//! language with English fallback. Template params `{{.P0}}`, `{{.Name}}` etc.
//! are substituted.

use std::collections::HashMap;

use crate::assets::Resources;

#[derive(Debug, Default)]
pub struct I18n {
    /// language -> (message id -> message string)
    langs: HashMap<String, HashMap<String, String>>,
    default_lang: String,
}

impl I18n {
    pub fn load(default_lang: &str) -> Self {
        let mut langs: HashMap<String, HashMap<String, String>> = HashMap::new();
        for file in Resources::iter() {
            let name = file.as_ref();
            let Some(rest) = name.strip_prefix("i18n/") else {
                continue;
            };
            let Some(stem) = rest.strip_suffix(".toml") else {
                continue;
            };
            if stem.contains('/') {
                continue;
            }
            // "zh_CN" -> "zh-CN", "en" -> "en"
            let lang = stem.replace('_', "-");
            if let Some(content) = Resources::read_string(name) {
                let messages = parse_bundle(&content);
                langs.insert(lang, messages);
            }
        }
        Self {
            langs,
            default_lang: if default_lang.is_empty() {
                "en".to_string()
            } else {
                default_lang.to_string()
            },
        }
    }

    /// Translate `id` for the requested language (falls back to base language,
    /// then English, then the id itself).
    pub fn translate(&self, lang: &str, id: &str) -> String {
        let lang = if lang.is_empty() {
            self.default_lang.clone()
        } else {
            lang.to_string()
        };
        if let Some(msg) = self.lookup(&lang, id) {
            return msg;
        }
        id.to_string()
    }

    /// Translate with positional params (`{{.P0}}`, `{{.P1}}`, ...).
    pub fn translate_params(&self, lang: &str, id: &str, params: &[&str]) -> String {
        let mut msg = self.translate(lang, id);
        for (i, p) in params.iter().enumerate() {
            msg = msg.replace(&format!("{{{{.P{i}}}}}"), p);
        }
        msg
    }

    fn lookup(&self, lang: &str, id: &str) -> Option<String> {
        // exact language
        if let Some(m) = self.langs.get(lang).and_then(|m| m.get(id)) {
            return Some(m.clone());
        }
        // base language (e.g. "zh-CN" -> "zh")
        if let Some(base) = lang.split('-').next() {
            if base != lang {
                if let Some(m) = self.langs.get(base).and_then(|m| m.get(id)) {
                    return Some(m.clone());
                }
            }
        }
        // English fallback
        self.langs.get("en").and_then(|m| m.get(id)).cloned()
    }
}

/// Parse a go-i18n TOML bundle: each `[Id]` table has `one`/`other`/`description`.
/// We use `other` (the default form), falling back to `one`.
fn parse_bundle(content: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(value) = content.parse::<toml::Value>() else {
        return out;
    };
    if let Some(table) = value.as_table() {
        for (id, entry) in table {
            if let Some(entry_table) = entry.as_table() {
                let msg = entry_table
                    .get("other")
                    .and_then(|v| v.as_str())
                    .or_else(|| entry_table.get("one").and_then(|v| v.as_str()));
                if let Some(msg) = msg {
                    out.insert(id.clone(), msg.to_string());
                }
            } else if let Some(s) = entry.as_str() {
                out.insert(id.clone(), s.to_string());
            }
        }
    }
    out
}

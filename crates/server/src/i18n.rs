//! Minimal i18n, mirroring `global/i18n.go` semantics: load every `*.toml`
//! bundle from the embedded `i18n/` directory and translate a message id by
//! language with English fallback. Template params `{{.P0}}`, `{{.Name}}` etc.
//! are substituted.

use std::cmp::Ordering;
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
        let preferences = language_preferences(lang);
        let preferences = if preferences.is_empty() && lang.trim().is_empty() {
            language_preferences(&self.default_lang)
        } else {
            preferences
        };
        for lang in preferences {
            if let Some(msg) = self.lookup(lang, id) {
                return msg;
            }
        }
        if let Some(msg) = self.lang("en").and_then(|messages| messages.get(id)) {
            return msg.clone();
        }
        id.to_string()
    }

    /// Translate with positional params (`{{.P0}}`, `{{.P1}}`, ...).
    pub fn translate_params(&self, lang: &str, id: &str, params: &[&str]) -> String {
        let msg = self.translate(lang, id);
        let mut output = String::with_capacity(msg.len());
        let mut remaining = msg.as_str();
        while let Some(start) = remaining.find("{{.P") {
            output.push_str(&remaining[..start]);
            let placeholder = &remaining[start + 4..];
            let Some(end) = placeholder.find("}}") else {
                output.push_str(&remaining[start..]);
                return output;
            };
            let index = &placeholder[..end];
            if !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()) {
                if let Some(param) = index
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| params.get(index))
                {
                    output.push_str(param);
                    remaining = &placeholder[end + 2..];
                    continue;
                }
            }
            output.push_str("{{.P");
            remaining = placeholder;
        }
        output.push_str(remaining);
        output
    }

    fn lookup(&self, lang: &str, id: &str) -> Option<String> {
        // exact language
        if let Some(m) = self.lang(lang).and_then(|m| m.get(id)) {
            return Some(m.clone());
        }
        // base language (e.g. "zh-CN" -> "zh")
        if let Some(base) = lang.split('-').next() {
            if base != lang {
                if let Some(m) = self.lang(base).and_then(|m| m.get(id)) {
                    return Some(m.clone());
                }
            }
        }
        if !lang.contains('-') {
            if let Some(m) = self
                .langs
                .iter()
                .find(|(name, _)| {
                    name.split('-')
                        .next()
                        .is_some_and(|base| base.eq_ignore_ascii_case(lang))
                })
                .and_then(|(_, messages)| messages.get(id))
            {
                return Some(m.clone());
            }
        }
        None
    }

    fn lang(&self, lang: &str) -> Option<&HashMap<String, String>> {
        self.langs
            .iter()
            .find_map(|(name, messages)| name.eq_ignore_ascii_case(lang).then_some(messages))
    }
}

pub(crate) fn language_preferences(header: &str) -> Vec<&str> {
    let mut preferences: Vec<(&str, f32)> = header
        .split(',')
        .filter_map(|entry| {
            let mut parts = entry.split(';');
            let range = parts.next().unwrap_or_default().trim();
            if range.is_empty() || range == "*" {
                return None;
            }
            let mut quality = 1.0;
            for parameter in parts {
                let Some((name, value)) = parameter.trim().split_once('=') else {
                    continue;
                };
                if name.trim().eq_ignore_ascii_case("q") {
                    quality = value
                        .trim()
                        .parse::<f32>()
                        .ok()
                        .filter(|quality| (0.0..=1.0).contains(quality))
                        .unwrap_or(0.0);
                }
            }
            (quality > 0.0).then_some((range, quality))
        })
        .collect();
    preferences.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(Ordering::Equal));
    preferences.into_iter().map(|(range, _)| range).collect()
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

#[cfg(test)]
mod tests {
    use super::I18n;

    #[test]
    fn accepts_browser_language_header() {
        let i18n = I18n::load("en");

        assert_eq!(
            i18n.translate("zh-CN,zh;q=0.9,en;q=0.8", "OperationSuccess"),
            "操作成功。"
        );
    }

    #[test]
    fn matches_language_case_insensitively() {
        let i18n = I18n::load("en");

        assert_eq!(i18n.translate("zh-cn", "OperationSuccess"), "操作成功。");
    }

    #[test]
    fn honors_language_quality_and_exclusions() {
        let i18n = I18n::load("en");

        assert_eq!(
            i18n.translate("en;q=0.1, zh-CN;q=1", "OperationSuccess"),
            "操作成功。"
        );
        assert_eq!(
            i18n.translate("zh-CN;q=0, en;q=1", "OperationSuccess"),
            "the operation success."
        );
    }

    #[test]
    fn matches_base_language_to_regional_bundle() {
        let i18n = I18n::load("en");

        assert_eq!(i18n.translate("zh", "OperationSuccess"), "操作成功。");
    }

    #[test]
    fn does_not_replace_placeholders_inside_params() {
        let i18n = I18n::load("en");

        assert_eq!(
            i18n.translate_params(
                "en",
                "GeoRelayOutsidePool",
                &["{{.P1}}", "relay.example.com:21117"],
            ),
            "Rule “{{.P1}}” references relay “relay.example.com:21117”, which is not in the server relay pool."
        );
    }
}

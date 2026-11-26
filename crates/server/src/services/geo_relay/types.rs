use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const SETTINGS_VERSION: u8 = 1;
pub const MAX_RULES: usize = 256;
pub const MAX_RELAYS_PER_RULE: usize = 32;
pub const MAX_EXPRESSION_BYTES: usize = 512;
pub const MIN_UPDATE_INTERVAL_HOURS: u16 = 6;
pub const MAX_UPDATE_INTERVAL_HOURS: u16 = 24 * 30;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct GeoSettings {
    pub version: u8,
    pub revision: u64,
    pub enabled: bool,
    pub rules: Vec<GeoRule>,
}

impl Default for GeoSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            revision: 0,
            enabled: false,
            rules: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GeoRule {
    pub name: String,
    #[serde(default = "default_true")]
    pub symmetric: bool,
    #[serde(rename = "match")]
    pub expressions: EndpointExpressions,
    pub relays: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct EndpointExpressions {
    pub client_a: String,
    pub client_b: String,
}

impl Default for EndpointExpressions {
    fn default() -> Self {
        Self {
            client_a: "*".to_owned(),
            client_b: "*".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MmdbKind {
    Country,
    City,
    Asn,
}

impl MmdbKind {
    pub const ALL: [Self; 3] = [Self::Country, Self::City, Self::Asn];

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "country" => Some(Self::Country),
            "city" => Some(Self::City),
            "asn" => Some(Self::Asn),
            _ => None,
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Self::Country => "GeoLite2-Country.mmdb",
            Self::City => "GeoLite2-City.mmdb",
            Self::Asn => "GeoLite2-ASN.mmdb",
        }
    }

    pub fn expected_database_marker(self) -> &'static str {
        match self {
            Self::Country => "Country",
            Self::City => "City",
            Self::Asn => "ASN",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct MmdbSources {
    pub country: Option<String>,
    pub city: Option<String>,
    pub asn: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MmdbSourceMode {
    #[default]
    Custom,
    Maxmind,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct MmdbSourceUpdate {
    pub mode: MmdbSourceMode,
    pub custom: MmdbSources,
    pub maxmind_account_id: Option<String>,
    pub maxmind_license_key: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MmdbSourceView {
    pub mode: MmdbSourceMode,
    pub maxmind_account_id: String,
    pub maxmind_key_configured: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MmdbSourceResult {
    pub source: MmdbSourceView,
    pub custom: MmdbSources,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct MmdbUpdatePolicy {
    pub enabled: bool,
    pub interval_hours: u16,
}

impl Default for MmdbUpdatePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: 24 * 7,
        }
    }
}

impl MmdbUpdatePolicy {
    pub fn validate(&self) -> Result<(), String> {
        if !(MIN_UPDATE_INTERVAL_HOURS..=MAX_UPDATE_INTERVAL_HOURS).contains(&self.interval_hours) {
            return Err(format!(
                "MMDB update interval must be between {MIN_UPDATE_INTERVAL_HOURS} and {MAX_UPDATE_INTERVAL_HOURS} hours"
            ));
        }
        Ok(())
    }
}

impl MmdbSources {
    pub fn get(&self, kind: MmdbKind) -> Option<&str> {
        match kind {
            MmdbKind::Country => self.country.as_deref(),
            MmdbKind::City => self.city.as_deref(),
            MmdbKind::Asn => self.asn.as_deref(),
        }
    }

    pub fn set(&mut self, kind: MmdbKind, value: Option<String>) {
        match kind {
            MmdbKind::Country => self.country = value,
            MmdbKind::City => self.city = value,
            MmdbKind::Asn => self.asn = value,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MmdbStatus {
    pub kind: MmdbKind,
    pub path: Option<String>,
    pub is_present: bool,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<String>,
    pub database_type: Option<String>,
    pub build_epoch: Option<u64>,
    pub has_backup: bool,
    pub source_url: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub is_available: bool,
    pub applied_revision: Option<u64>,
    pub is_geo_enabled: Option<bool>,
    pub rule_count: Option<usize>,
    pub country_available: Option<bool>,
    pub city_available: Option<bool>,
    pub asn_available: Option<bool>,
    pub warnings: Vec<String>,
    pub raw: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoOverview {
    pub settings: GeoSettings,
    pub databases: Vec<MmdbStatus>,
    pub update_policy: MmdbUpdatePolicy,
    pub source: MmdbSourceView,
    pub runtime: RuntimeStatus,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsResult {
    pub settings: GeoSettings,
    pub is_persisted: bool,
    pub is_applied: bool,
    pub apply_message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot<'a> {
    pub version: u8,
    pub revision: u64,
    pub relay_servers: &'a [String],
    pub geo: RuntimeGeo<'a>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeGeo<'a> {
    pub enabled: bool,
    pub rules: &'a [GeoRule],
}

pub fn validate_settings(settings: &mut GeoSettings, relay_pool: &[String]) -> Result<(), String> {
    if settings.version != SETTINGS_VERSION {
        return Err(format!(
            "unsupported settings version {}; expected {SETTINGS_VERSION}",
            settings.version
        ));
    }
    if settings.rules.len() > MAX_RULES {
        return Err(format!("rule count must not exceed {MAX_RULES}"));
    }
    let relay_pool: HashSet<String> = relay_pool
        .iter()
        .map(|relay| relay.to_ascii_lowercase())
        .collect();
    let mut names = HashSet::new();
    for (index, rule) in settings.rules.iter_mut().enumerate() {
        rule.name = rule.name.trim().to_owned();
        if rule.name.is_empty() || rule.name.len() > 120 {
            return Err(format!("rules[{index}].name must contain 1 to 120 bytes"));
        }
        if !names.insert(rule.name.to_ascii_lowercase()) {
            return Err(format!("duplicate rule name: {}", rule.name));
        }
        rule.expressions.client_a = normalize_expression(&rule.expressions.client_a);
        rule.expressions.client_b = normalize_expression(&rule.expressions.client_b);
        if rule.expressions.client_a.len() > MAX_EXPRESSION_BYTES
            || rule.expressions.client_b.len() > MAX_EXPRESSION_BYTES
        {
            return Err(format!(
                "rule '{}' expressions must not exceed {MAX_EXPRESSION_BYTES} bytes",
                rule.name
            ));
        }
        ExpressionParser::parse(&rule.expressions.client_a)
            .map_err(|error| format!("rule '{}' clientA: {error}", rule.name))?;
        ExpressionParser::parse(&rule.expressions.client_b)
            .map_err(|error| format!("rule '{}' clientB: {error}", rule.name))?;
        if rule.relays.is_empty() || rule.relays.len() > MAX_RELAYS_PER_RULE {
            return Err(format!(
                "rule '{}' must contain 1 to {MAX_RELAYS_PER_RULE} relays",
                rule.name
            ));
        }
        let mut seen = HashSet::new();
        for relay in &mut rule.relays {
            let normalized = crate::services::server_cmd::normalize_relay_pool(relay)?;
            if normalized.len() != 1 {
                return Err(format!(
                    "rule '{}' must contain one relay address per entry",
                    rule.name
                ));
            }
            let Some(normalized) = normalized.into_iter().next() else {
                return Err(format!("rule '{}' contains an empty relay", rule.name));
            };
            if !relay_pool.contains(&normalized.to_ascii_lowercase()) {
                return Err(format!(
                    "rule '{}' references relay '{}' outside the relay pool",
                    rule.name, normalized
                ));
            }
            if !seen.insert(normalized.to_ascii_lowercase()) {
                return Err(format!(
                    "rule '{}' contains duplicate relay '{normalized}'",
                    rule.name
                ));
            }
            *relay = normalized;
        }
    }
    if settings.enabled && settings.rules.is_empty() {
        return Err("enabled Geo routing requires at least one rule".to_owned());
    }
    if settings.enabled && relay_pool.is_empty() {
        return Err("enabled Geo routing requires a relay pool".to_owned());
    }
    Ok(())
}

fn normalize_expression(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "*".to_owned()
    } else {
        value.to_owned()
    }
}

fn default_true() -> bool {
    true
}

struct ExpressionParser<'a> {
    source: &'a str,
    position: usize,
}

impl<'a> ExpressionParser<'a> {
    fn parse(source: &'a str) -> Result<(), String> {
        let mut parser = Self {
            source,
            position: 0,
        };
        parser.parse_or()?;
        parser.skip_whitespace();
        if parser.position != source.len() {
            return Err(format!("unexpected token at byte {}", parser.position));
        }
        Ok(())
    }

    fn parse_or(&mut self) -> Result<(), String> {
        self.parse_and()?;
        loop {
            self.skip_whitespace();
            if !self.consume('/') {
                return Ok(());
            }
            self.parse_and()?;
        }
    }

    fn parse_and(&mut self) -> Result<(), String> {
        self.parse_primary()?;
        loop {
            self.skip_whitespace();
            if !self.consume('+') {
                return Ok(());
            }
            self.parse_primary()?;
        }
    }

    fn parse_primary(&mut self) -> Result<(), String> {
        self.skip_whitespace();
        if self.consume('(') {
            self.parse_or()?;
            self.skip_whitespace();
            if !self.consume(')') {
                return Err(format!("missing ')' at byte {}", self.position));
            }
            return Ok(());
        }
        let predicate = self.read_predicate()?;
        validate_predicate(predicate)
    }

    fn read_predicate(&mut self) -> Result<&'a str, String> {
        self.skip_whitespace();
        let start = self.position;
        let mut quote = None;
        let mut escaped = false;
        while let Some(ch) = self.peek() {
            if let Some(active_quote) = quote {
                self.advance(ch);
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == active_quote {
                    quote = None;
                }
                continue;
            }
            if ch == '\'' || ch == '"' {
                quote = Some(ch);
                self.advance(ch);
                continue;
            }
            if matches!(ch, '+' | '/' | '(' | ')') {
                break;
            }
            self.advance(ch);
        }
        if quote.is_some() {
            return Err(format!("unterminated quoted value at byte {start}"));
        }
        let value = self.source[start..self.position].trim();
        if value.is_empty() {
            Err(format!("missing expression term at byte {start}"))
        } else {
            Ok(value)
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if !ch.is_whitespace() {
                break;
            }
            self.advance(ch);
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.advance(expected);
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.position..].chars().next()
    }

    fn advance(&mut self, ch: char) {
        self.position += ch.len_utf8();
    }
}

fn validate_predicate(raw: &str) -> Result<(), String> {
    let raw = raw.trim();
    if raw == "*" || is_country_code(raw) {
        return Ok(());
    }
    let (field, raw_value) = raw
        .split_once(':')
        .ok_or_else(|| format!("'{raw}' must be '*' or field:value"))?;
    let field = field.trim().to_ascii_lowercase();
    let value = decode_value(raw_value)?;
    if value.is_empty() {
        return Err(format!("field '{field}' has an empty value"));
    }
    match field.as_str() {
        "continent" | "subdivision" | "region" | "city" | "isp" | "asn_org" => Ok(()),
        "country" if is_country_code(&value) => Ok(()),
        "country" => Err(format!("country '{value}' must be a two-letter code")),
        "geoname" | "city_id" => validate_nonzero_number(&field, &value),
        "asn" => {
            let value = value
                .strip_prefix("AS")
                .or_else(|| value.strip_prefix("as"))
                .unwrap_or(&value);
            validate_nonzero_number(&field, value)
        }
        _ => Err(format!("unsupported expression field '{field}'")),
    }
}

fn decode_value(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let Some(first) = raw.chars().next() else {
        return Ok(String::new());
    };
    if first != '\'' && first != '"' {
        return Ok(raw.to_owned());
    }
    if raw.len() < first.len_utf8() * 2 || !raw.ends_with(first) {
        return Err("quoted value must end with the same quote".to_owned());
    }
    let body_start = first.len_utf8();
    let body_end = raw.len() - first.len_utf8();
    let mut value = String::new();
    let mut escaped = false;
    for ch in raw[body_start..body_end].chars() {
        if escaped {
            value.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            value.push(ch);
        }
    }
    if escaped {
        return Err("quoted value ends with an incomplete escape".to_owned());
    }
    Ok(value.trim().to_owned())
}

fn validate_nonzero_number(field: &str, value: &str) -> Result<(), String> {
    match value.parse::<u32>() {
        Ok(value) if value > 0 => Ok(()),
        _ => Err(format!("{field} '{value}' must be a positive integer")),
    }
}

fn is_country_code(value: &str) -> bool {
    value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_expression_grammar() {
        for expression in [
            "*",
            "CN/JP",
            "city:上海+isp:Telecom",
            "(country:CN/asn:AS4134)+continent:AS",
            r#"city:"A/B"+isp:'X+Y'"#,
        ] {
            assert!(ExpressionParser::parse(expression).is_ok(), "{expression}");
        }
        for expression in ["CN/", "(CN", "city:", "unknown:value", "USA"] {
            assert!(ExpressionParser::parse(expression).is_err(), "{expression}");
        }
    }

    #[test]
    fn rejects_multiple_relay_addresses_in_one_entry() {
        let mut settings = GeoSettings {
            enabled: true,
            rules: vec![GeoRule {
                name: "bad relay entry".to_owned(),
                symmetric: true,
                expressions: EndpointExpressions::default(),
                relays: vec!["relay-a.example.com,relay-b.example.com".to_owned()],
            }],
            ..GeoSettings::default()
        };
        let relay_pool = [
            "relay-a.example.com:21117".to_owned(),
            "relay-b.example.com:21117".to_owned(),
        ];
        assert!(validate_settings(&mut settings, &relay_pool).is_err());
    }

    #[test]
    fn validates_mmdb_update_interval_bounds() {
        assert!(MmdbUpdatePolicy::default().validate().is_ok());
        assert!(MmdbUpdatePolicy {
            enabled: true,
            interval_hours: MIN_UPDATE_INTERVAL_HOURS - 1,
        }
        .validate()
        .is_err());
        assert!(MmdbUpdatePolicy {
            enabled: true,
            interval_hours: MAX_UPDATE_INTERVAL_HOURS + 1,
        }
        .validate()
        .is_err());
    }
}

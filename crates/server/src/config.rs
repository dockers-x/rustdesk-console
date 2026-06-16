//! Configuration, ported from the Go `config/*.go` structs and `conf/config.yaml`.
//!
//! Loading mirrors viper's behaviour: the YAML file is read first, then every
//! known key can be overridden by an environment variable named
//! `RUSTDESK_API_<PATH>` where `<PATH>` is the upper-cased key path joined by
//! `_` (and `-` replaced by `_`). For example `app.web-client` is overridden by
//! `RUSTDESK_API_APP_WEB_CLIENT`.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DB_TYPE_SQLITE: &str = "sqlite";
pub const DB_TYPE_MYSQL: &str = "mysql";
pub const DB_TYPE_POSTGRESQL: &str = "postgresql";
pub const CACHE_TYPE_FILE: &str = "file";
pub const CACHE_TYPE_REDIS: &str = "redis";

pub const DEFAULT_ID_SERVER_PORT: i32 = 21116;
pub const DEFAULT_RELAY_SERVER_PORT: i32 = 21117;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Config {
    pub lang: String,
    pub app: App,
    pub admin: Admin,
    pub gorm: Gorm,
    pub mysql: Mysql,
    pub postgresql: Postgresql,
    pub gin: Gin,
    pub logger: Logger,
    pub redis: Redis,
    pub cache: Cache,
    pub oss: Oss,
    pub jwt: Jwt,
    pub rustdesk: Rustdesk,
    pub proxy: Proxy,
    pub ldap: Ldap,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct App {
    #[serde(rename = "web-client")]
    pub web_client: i32,
    pub register: bool,
    #[serde(rename = "register-status")]
    pub register_status: i32,
    #[serde(rename = "show-swagger")]
    pub show_swagger: i32,
    #[serde(rename = "token-expire")]
    pub token_expire: String,
    #[serde(rename = "web-sso")]
    pub web_sso: bool,
    #[serde(rename = "disable-pwd-login")]
    pub disable_pwd_login: bool,
    #[serde(rename = "captcha-threshold")]
    pub captcha_threshold: i32,
    #[serde(rename = "ban-threshold")]
    pub ban_threshold: i32,
}

impl App {
    /// Token lifetime; defaults to 7 days when unset, matching the Go server.
    pub fn token_expire_duration(&self) -> Duration {
        parse_go_duration(&self.token_expire).unwrap_or_else(|| Duration::from_secs(604800))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Admin {
    pub title: String,
    pub hello: String,
    #[serde(rename = "hello-file")]
    pub hello_file: String,
    #[serde(rename = "id-server-port")]
    pub id_server_port: i32,
    #[serde(rename = "relay-server-port")]
    pub relay_server_port: i32,
}

impl Admin {
    pub fn init(&mut self) {
        if self.id_server_port == 0 {
            self.id_server_port = DEFAULT_ID_SERVER_PORT;
        }
        if self.relay_server_port == 0 {
            self.relay_server_port = DEFAULT_RELAY_SERVER_PORT;
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Gorm {
    pub r#type: String,
    #[serde(rename = "max-idle-conns")]
    pub max_idle_conns: u32,
    #[serde(rename = "max-open-conns")]
    pub max_open_conns: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Mysql {
    pub addr: String,
    pub username: String,
    pub password: String,
    pub dbname: String,
    pub tls: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Postgresql {
    pub host: String,
    pub port: String,
    pub user: String,
    pub password: String,
    pub dbname: String,
    pub sslmode: String,
    #[serde(rename = "time-zone")]
    pub time_zone: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Gin {
    #[serde(rename = "api-addr")]
    pub api_addr: String,
    #[serde(rename = "admin-addr")]
    pub admin_addr: String,
    pub mode: String,
    #[serde(rename = "resources-path")]
    pub resources_path: String,
    #[serde(rename = "trust-proxy")]
    pub trust_proxy: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Logger {
    pub path: String,
    pub level: String,
    #[serde(rename = "report-caller")]
    pub report_caller: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Redis {
    pub addr: String,
    pub password: String,
    pub db: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Cache {
    pub r#type: String,
    #[serde(rename = "redis-addr")]
    pub redis_addr: String,
    #[serde(rename = "redis-pwd")]
    pub redis_pwd: String,
    #[serde(rename = "redis-db")]
    pub redis_db: i64,
    #[serde(rename = "file-dir")]
    pub file_dir: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Oss {
    #[serde(rename = "access-key-id")]
    pub access_key_id: String,
    #[serde(rename = "access-key-secret")]
    pub access_key_secret: String,
    pub host: String,
    #[serde(rename = "callback-url")]
    pub callback_url: String,
    #[serde(rename = "expire-time")]
    pub expire_time: i64,
    #[serde(rename = "max-byte")]
    pub max_byte: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Jwt {
    pub key: String,
    #[serde(rename = "expire-duration")]
    pub expire_duration: String,
}

impl Jwt {
    pub fn expire_duration(&self) -> Duration {
        parse_go_duration(&self.expire_duration).unwrap_or_else(|| Duration::from_secs(604800))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Rustdesk {
    #[serde(rename = "id-server")]
    pub id_server: String,
    #[serde(rename = "relay-server")]
    pub relay_server: String,
    #[serde(rename = "api-server")]
    pub api_server: String,
    pub key: String,
    #[serde(rename = "key-file")]
    pub key_file: String,
    pub personal: i32,
    #[serde(rename = "webclient-magic-queryonline")]
    pub webclient_magic_queryonline: i32,
    #[serde(rename = "ws-host")]
    pub ws_host: String,
}

impl Rustdesk {
    /// Load the server key from `key-file` when `key` is empty (mirrors LoadKeyFile).
    pub fn load_key_file(&mut self) {
        if !self.key.is_empty() {
            return;
        }
        if !self.key_file.is_empty() {
            if let Ok(b) = std::fs::read_to_string(&self.key_file) {
                self.key = b;
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Proxy {
    pub enable: bool,
    pub host: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Ldap {
    pub enable: bool,
    pub url: String,
    #[serde(rename = "tls-ca-file")]
    pub tls_ca_file: String,
    #[serde(rename = "tls-verify")]
    pub tls_verify: bool,
    #[serde(rename = "base-dn")]
    pub base_dn: String,
    #[serde(rename = "bind-dn")]
    pub bind_dn: String,
    #[serde(rename = "bind-password")]
    pub bind_password: String,
    pub user: LdapUser,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct LdapUser {
    #[serde(rename = "base-dn")]
    pub base_dn: String,
    #[serde(rename = "enable-attr")]
    pub enable_attr: String,
    #[serde(rename = "enable-attr-value")]
    pub enable_attr_value: String,
    pub filter: String,
    pub username: String,
    pub email: String,
    #[serde(rename = "first-name")]
    pub first_name: String,
    #[serde(rename = "last-name")]
    pub last_name: String,
    pub sync: bool,
    #[serde(rename = "admin-group")]
    pub admin_group: String,
    #[serde(rename = "allow-group")]
    pub allow_group: String,
}

/// Load configuration from `path`, apply environment overrides, then run the
/// same post-load fix-ups as the Go server (`LoadKeyFile`, `Admin.Init`).
///
/// Env overrides are applied against the *full* config schema (defaults merged
/// with the file), so every field can be set by `RUSTDESK_API_*` even when it is
/// absent from the YAML — matching viper's `AutomaticEnv` on the Go side.
pub fn init(path: &str) -> anyhow::Result<Config> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Fatal error config file {}: {}", path, e))?;
    let file_val = serde_yaml_to_json(&raw)?;
    // Start from the full default schema so env vars for keys missing in the
    // file still resolve, then overlay the file, then env vars.
    let mut value = serde_json::to_value(Config::default())?;
    merge(&mut value, file_val);
    apply_env_overrides(&mut value, "RUSTDESK_API");
    let mut cfg: Config = serde_json::from_value(value)?;
    cfg.rustdesk.load_key_file();
    cfg.admin.init();
    Ok(cfg)
}

/// Deep-merge `overlay` into `base` (objects merge recursively; other values
/// replace). Used to layer the YAML file on top of the default schema.
fn merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                merge(b.entry(k).or_insert(Value::Null), v);
            }
        }
        (b, o) => *b = o,
    }
}

/// Parse YAML into a `serde_json::Value` (the `config` crate's yaml support
/// returns its own value type, so we round-trip through serde_yaml-equivalent
/// parsing using `serde_json` after a yaml deserialize).
fn serde_yaml_to_json(raw: &str) -> anyhow::Result<Value> {
    // `config` crate parses yaml; reuse it to avoid pulling serde_yaml.
    let builder = config::Config::builder()
        .add_source(config::File::from_str(raw, config::FileFormat::Yaml))
        .build()?;
    let v: Value = builder.try_deserialize()?;
    Ok(v)
}

/// Walk every scalar leaf and override it from `PREFIX_<PATH>` when present.
fn apply_env_overrides(value: &mut Value, prefix: &str) {
    fn walk(value: &mut Value, path: &str) {
        match value {
            Value::Object(map) => {
                for (k, v) in map.iter_mut() {
                    let child = if path.is_empty() {
                        k.clone()
                    } else {
                        format!("{path}_{k}")
                    };
                    walk(v, &child);
                }
            }
            _ => {
                let env_name = path.replace('-', "_").to_uppercase();
                if let Ok(raw) = std::env::var(&env_name) {
                    *value = coerce_scalar(&raw);
                }
            }
        }
    }
    let root = prefix.to_string();
    if let Value::Object(map) = value {
        for (k, v) in map.iter_mut() {
            walk(v, &format!("{root}_{k}"));
        }
    }
}

/// Coerce an env-var string into a bool/number/string, like viper.
fn coerce_scalar(raw: &str) -> Value {
    if let Ok(b) = raw.parse::<bool>() {
        return Value::Bool(b);
    }
    if let Ok(i) = raw.parse::<i64>() {
        return Value::Number(i.into());
    }
    Value::String(raw.to_string())
}

/// Parse a Go `time.Duration` string such as `168h`, `30m`, `604800s`.
/// A bare number is treated as nanoseconds by Go; we accept seconds for the
/// numeric default path and otherwise require a unit suffix.
pub fn parse_go_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(secs) = s.parse::<u64>() {
        // Go treats a unit-less duration as nanoseconds; the configs only ever
        // use suffixed values, so a bare integer here is our own seconds form.
        return Some(Duration::from_secs(secs));
    }
    let mut total = Duration::ZERO;
    let mut num = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            num.push(ch);
        } else {
            let val: f64 = num.parse().ok()?;
            num.clear();
            let unit = match ch {
                'h' => 3600.0,
                'm' => 60.0,
                's' => 1.0,
                _ => return None,
            };
            total += Duration::from_secs_f64(val * unit);
        }
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_go_durations() {
        assert_eq!(parse_go_duration("168h").unwrap().as_secs(), 168 * 3600);
        assert_eq!(parse_go_duration("30m").unwrap().as_secs(), 1800);
        assert_eq!(parse_go_duration("604800").unwrap().as_secs(), 604800);
        assert!(parse_go_duration("").is_none());
    }

    #[test]
    fn env_overrides_hyphenated_keys() {
        // app.web-client -> RUSTDESK_API_APP_WEB_CLIENT
        let mut v: Value = serde_json::json!({
            "app": { "web-client": 1, "register": false },
            "rustdesk": { "personal": 0 }
        });
        std::env::set_var("RUSTDESK_API_APP_WEB_CLIENT", "0");
        std::env::set_var("RUSTDESK_API_APP_REGISTER", "true");
        std::env::set_var("RUSTDESK_API_RUSTDESK_PERSONAL", "1");
        apply_env_overrides(&mut v, "RUSTDESK_API");
        std::env::remove_var("RUSTDESK_API_APP_WEB_CLIENT");
        std::env::remove_var("RUSTDESK_API_APP_REGISTER");
        std::env::remove_var("RUSTDESK_API_RUSTDESK_PERSONAL");
        assert_eq!(v["app"]["web-client"], serde_json::json!(0));
        assert_eq!(v["app"]["register"], serde_json::json!(true));
        assert_eq!(v["rustdesk"]["personal"], serde_json::json!(1));
    }

    #[test]
    fn env_overrides_apply_to_schema_keys_absent_from_file() {
        // cache/redis/oss are not in conf/config.yaml, but viper (and now we)
        // still bind them from the env via the full default schema.
        std::env::set_var("RUSTDESK_API_CACHE_TYPE", "redis");
        std::env::set_var("RUSTDESK_API_REDIS_DB", "3");
        std::env::set_var("RUSTDESK_API_OSS_HOST", "https://oss.example.com");
        let mut value = serde_json::to_value(Config::default()).unwrap();
        apply_env_overrides(&mut value, "RUSTDESK_API");
        let cfg: Config = serde_json::from_value(value).unwrap();
        std::env::remove_var("RUSTDESK_API_CACHE_TYPE");
        std::env::remove_var("RUSTDESK_API_REDIS_DB");
        std::env::remove_var("RUSTDESK_API_OSS_HOST");
        assert_eq!(cfg.cache.r#type, "redis");
        assert_eq!(cfg.redis.db, 3);
        assert_eq!(cfg.oss.host, "https://oss.example.com");
    }
}

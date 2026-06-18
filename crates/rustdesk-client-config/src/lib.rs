use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerConfig {
    pub id_server: String,
    pub relay_server: String,
    pub api_server: String,
    pub key: String,
}

impl ServerConfig {
    pub fn new(id_server: &str, relay_server: &str, api_server: &str, key: &str) -> Self {
        Self {
            id_server: id_server.trim().to_string(),
            relay_server: relay_server.trim().to_string(),
            api_server: api_server.trim().to_string(),
            key: key.trim().to_string(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.id_server.is_empty()
            && self.relay_server.is_empty()
            && self.api_server.is_empty()
            && self.key.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesktopCommandSet {
    pub linux: Vec<String>,
    pub macos: Vec<String>,
    pub windows: Vec<String>,
}

impl DesktopCommandSet {
    pub fn empty() -> Self {
        Self {
            linux: Vec::new(),
            macos: Vec::new(),
            windows: Vec::new(),
        }
    }

    pub fn extend(&mut self, other: DesktopCommandSet) {
        self.linux.extend(other.linux);
        self.macos.extend(other.macos);
        self.windows.extend(other.windows);
    }
}

#[derive(Serialize)]
struct RustDeskServerConfig<'a> {
    host: &'a str,
    relay: &'a str,
    api: &'a str,
    key: &'a str,
}

pub fn encode_server_config(config: &ServerConfig) -> String {
    let json = serde_json::to_vec(&RustDeskServerConfig {
        host: &config.id_server,
        relay: &config.relay_server,
        api: &config.api_server,
        key: &config.key,
    })
    .unwrap_or_default();
    URL_SAFE_NO_PAD
        .encode(json)
        .chars()
        .rev()
        .collect::<String>()
}

pub fn mobile_config_text(encoded_config: &str) -> String {
    format!("config={}", encoded_config.trim())
}

pub fn filename_hint(config: &ServerConfig) -> String {
    let mut parts = vec![format!("host={}", config.id_server)];
    if !config.api_server.is_empty() {
        parts.push(format!("api={}", config.api_server));
    }
    if !config.key.is_empty() {
        parts.push(format!("key={}", config.key));
    }
    if !config.relay_server.is_empty() {
        parts.push(format!("relay={}", config.relay_server));
    }
    format!("rustdesk-{}.exe", parts.join(","))
}

pub fn config_commands(encoded_config: &str) -> DesktopCommandSet {
    let encoded = encoded_config.trim();
    DesktopCommandSet {
        linux: vec![format!("sudo rustdesk --config {}", sh_arg(encoded))],
        macos: vec![format!(
            "sudo /Applications/RustDesk.app/Contents/MacOS/RustDesk --config {}",
            sh_arg(encoded)
        )],
        windows: vec![format!("rustdesk.exe --config {}", cmd_arg(encoded))],
    }
}

pub fn option_commands(config: &ServerConfig) -> DesktopCommandSet {
    DesktopCommandSet {
        linux: option_commands_for_bin("sudo rustdesk", config, sh_arg),
        macos: option_commands_for_bin(
            "sudo /Applications/RustDesk.app/Contents/MacOS/RustDesk",
            config,
            sh_arg,
        ),
        windows: option_commands_for_bin("rustdesk.exe", config, cmd_arg),
    }
}

pub fn password_commands(password: &str) -> DesktopCommandSet {
    let password = password.trim();
    if password.is_empty() {
        return DesktopCommandSet::empty();
    }
    DesktopCommandSet {
        linux: vec![format!("sudo rustdesk --password {}", sh_arg(password))],
        macos: vec![format!(
            "sudo /Applications/RustDesk.app/Contents/MacOS/RustDesk --password {}",
            sh_arg(password)
        )],
        windows: vec![format!("rustdesk.exe --password {}", cmd_arg(password))],
    }
}

pub fn setting_option_commands(name: &str, value: &str) -> DesktopCommandSet {
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() || value.is_empty() {
        return DesktopCommandSet::empty();
    }
    DesktopCommandSet {
        linux: vec![format!("sudo rustdesk --option {name} {}", sh_arg(value))],
        macos: vec![format!(
            "sudo /Applications/RustDesk.app/Contents/MacOS/RustDesk --option {name} {}",
            sh_arg(value)
        )],
        windows: vec![format!("rustdesk.exe --option {name} {}", cmd_arg(value))],
    }
}

fn option_commands_for_bin(
    bin: &str,
    config: &ServerConfig,
    quote: fn(&str) -> String,
) -> Vec<String> {
    [
        ("custom-rendezvous-server", config.id_server.as_str()),
        ("relay-server", config.relay_server.as_str()),
        ("api-server", config.api_server.as_str()),
        ("key", config.key.as_str()),
    ]
    .into_iter()
    .filter(|(_, value)| !value.trim().is_empty())
    .map(|(name, value)| format!("{bin} --option {name} {}", quote(value)))
    .collect()
}

fn sh_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn cmd_arg(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_rustdesk_import_string_as_reversed_urlsafe_base64_json() {
        let encoded = encode_server_config(&ServerConfig::new(
            "id.example.com:21116",
            "relay.example.com:21117",
            "https://api.example.com",
            "pk",
        ));
        let normalized = encoded.chars().rev().collect::<String>();
        let decoded = URL_SAFE_NO_PAD.decode(normalized).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(json["host"], "id.example.com:21116");
        assert_eq!(json["relay"], "relay.example.com:21117");
        assert_eq!(json["api"], "https://api.example.com");
        assert_eq!(json["key"], "pk");
    }

    #[test]
    fn builds_plain_filename_hint_supported_by_rustdesk() {
        assert_eq!(
            filename_hint(&ServerConfig::new(
                "id.example.com:21116",
                "relay.example.com:21117",
                "https://api.example.com",
                "pk",
            )),
            "rustdesk-host=id.example.com:21116,api=https://api.example.com,key=pk,relay=relay.example.com:21117.exe"
        );
    }

    #[test]
    fn skips_empty_option_commands() {
        let commands = option_commands(&ServerConfig::new("id.example.com:21116", "", "", "pk"));
        assert_eq!(
            commands.linux,
            vec![
                "sudo rustdesk --option custom-rendezvous-server 'id.example.com:21116'",
                "sudo rustdesk --option key 'pk'",
            ]
        );
    }

    #[test]
    fn mobile_config_text_reuses_encoded_config() {
        assert_eq!(mobile_config_text("abc123"), "config=abc123");
    }

    #[test]
    fn config_commands_quote_encoded_config() {
        let commands = config_commands("abc'123");
        assert_eq!(commands.linux, vec!["sudo rustdesk --config 'abc'\\''123'"]);
        assert_eq!(commands.windows, vec!["rustdesk.exe --config \"abc'123\""]);
    }

    #[test]
    fn password_commands_are_quoted_and_not_generated_for_empty_password() {
        assert!(password_commands("").linux.is_empty());
        let commands = password_commands("abc\"123");
        assert_eq!(
            commands.windows,
            vec!["rustdesk.exe --password \"abc\\\"123\""]
        );
    }

    #[test]
    fn setting_option_commands_quote_values() {
        let commands = setting_option_commands("approve-mode", "password");
        assert_eq!(
            commands.macos,
            vec![
                "sudo /Applications/RustDesk.app/Contents/MacOS/RustDesk --option approve-mode 'password'"
            ]
        );
    }
}

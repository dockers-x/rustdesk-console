use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Serialize;

use crate::support::webclient_config::WebClientConfig;

#[derive(Debug, Clone, Serialize)]
pub struct DeploymentConfig {
    pub id_server: String,
    pub relay_server: String,
    pub api_server: String,
    pub ws_host: String,
    pub ws_id_host: String,
    pub ws_relay_host: String,
    pub key: String,
    pub encoded_config: String,
    pub filename_hint: String,
    pub config_command: DeploymentCommandSet,
    pub option_commands: DeploymentCommandSet,
    pub webclient_ws_routes: WebSocketRoutes,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeploymentCommandSet {
    pub linux: Vec<String>,
    pub macos: Vec<String>,
    pub windows: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebSocketRoutes {
    pub id: String,
    pub relay: String,
}

#[derive(Serialize)]
struct RustDeskServerConfig<'a> {
    host: &'a str,
    relay: &'a str,
    api: &'a str,
    key: &'a str,
}

pub fn build(cfg: &WebClientConfig) -> DeploymentConfig {
    let id_server = cfg.id_server.trim().to_string();
    let relay_server = cfg.relay_server.trim().to_string();
    let api_server = cfg.api_server.trim().to_string();
    let ws_host = cfg.ws_host.trim().to_string();
    let ws_id_host = cfg.ws_id_host.trim().to_string();
    let ws_relay_host = cfg.ws_relay_host.trim().to_string();
    let key = cfg.key.trim().to_string();
    let encoded_config = encode_server_config(&id_server, &relay_server, &api_server, &key);
    let filename_hint = filename_hint(&id_server, &relay_server, &api_server, &key);
    let config_command = DeploymentCommandSet {
        linux: vec![format!(
            "sudo rustdesk --config {}",
            sh_arg(&encoded_config)
        )],
        macos: vec![format!(
            "sudo /Applications/RustDesk.app/Contents/MacOS/RustDesk --config {}",
            sh_arg(&encoded_config)
        )],
        windows: vec![format!(
            "rustdesk.exe --config {}",
            cmd_arg(&encoded_config)
        )],
    };
    let option_commands = DeploymentCommandSet {
        linux: option_commands(
            "sudo rustdesk",
            &id_server,
            &relay_server,
            &api_server,
            &key,
            sh_arg,
        ),
        macos: option_commands(
            "sudo /Applications/RustDesk.app/Contents/MacOS/RustDesk",
            &id_server,
            &relay_server,
            &api_server,
            &key,
            sh_arg,
        ),
        windows: option_commands(
            "rustdesk.exe",
            &id_server,
            &relay_server,
            &api_server,
            &key,
            cmd_arg,
        ),
    };
    let webclient_ws_routes = ws_routes(&ws_host, &ws_id_host, &ws_relay_host);

    DeploymentConfig {
        id_server,
        relay_server,
        api_server,
        ws_host,
        ws_id_host,
        ws_relay_host,
        key,
        encoded_config,
        filename_hint,
        config_command,
        option_commands,
        webclient_ws_routes,
    }
}

fn encode_server_config(host: &str, relay: &str, api: &str, key: &str) -> String {
    let json = serde_json::to_vec(&RustDeskServerConfig {
        host,
        relay,
        api,
        key,
    })
    .unwrap_or_default();
    URL_SAFE_NO_PAD
        .encode(json)
        .chars()
        .rev()
        .collect::<String>()
}

fn filename_hint(host: &str, relay: &str, api: &str, key: &str) -> String {
    let mut parts = vec![format!("host={host}")];
    if !api.is_empty() {
        parts.push(format!("api={api}"));
    }
    if !key.is_empty() {
        parts.push(format!("key={key}"));
    }
    if !relay.is_empty() {
        parts.push(format!("relay={relay}"));
    }
    format!("rustdesk-{}.exe", parts.join(","))
}

fn option_commands(
    bin: &str,
    id_server: &str,
    relay_server: &str,
    api_server: &str,
    key: &str,
    quote: fn(&str) -> String,
) -> Vec<String> {
    [
        ("custom-rendezvous-server", id_server),
        ("relay-server", relay_server),
        ("api-server", api_server),
        ("key", key),
    ]
    .into_iter()
    .filter(|(_, value)| !value.trim().is_empty())
    .map(|(name, value)| format!("{bin} --option {name} {}", quote(value)))
    .collect()
}

fn ws_routes(ws_host: &str, ws_id_host: &str, ws_relay_host: &str) -> WebSocketRoutes {
    let explicit_id = normalize_ws_uri(ws_id_host);
    let explicit_relay = normalize_ws_uri(ws_relay_host);
    if !explicit_id.is_empty() || !explicit_relay.is_empty() {
        return WebSocketRoutes {
            id: explicit_id,
            relay: explicit_relay,
        };
    }
    let base = normalize_ws_base(ws_host);
    WebSocketRoutes {
        id: if base.is_empty() {
            String::new()
        } else {
            format!("{base}/ws/id")
        },
        relay: if base.is_empty() {
            String::new()
        } else {
            format!("{base}/ws/relay")
        },
    }
}

fn normalize_ws_uri(value: &str) -> String {
    normalize_ws_base(value)
}

fn normalize_ws_base(value: &str) -> String {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return String::new();
    }
    if let Some(rest) = value.strip_prefix("http://") {
        return format!("ws://{rest}");
    }
    if let Some(rest) = value.strip_prefix("https://") {
        return format!("wss://{rest}");
    }
    value.to_string()
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
        let encoded = encode_server_config(
            "id.example.com:21116",
            "relay.example.com:21117",
            "https://api.example.com",
            "pk",
        );
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
            filename_hint("id.example.com:21116", "relay.example.com:21117", "https://api.example.com", "pk"),
            "rustdesk-host=id.example.com:21116,api=https://api.example.com,key=pk,relay=relay.example.com:21117.exe"
        );
    }

    #[test]
    fn skips_empty_option_commands() {
        assert_eq!(
            option_commands("rustdesk", "id.example.com:21116", "", "", "pk", sh_arg),
            vec![
                "rustdesk --option custom-rendezvous-server 'id.example.com:21116'",
                "rustdesk --option key 'pk'",
            ]
        );
    }

    #[test]
    fn normalizes_http_ws_host_to_websocket_routes() {
        let routes = ws_routes("https://rd.example.com/", "", "");
        assert_eq!(routes.id, "wss://rd.example.com/ws/id");
        assert_eq!(routes.relay, "wss://rd.example.com/ws/relay");
    }

    #[test]
    fn explicit_websocket_routes_override_ws_host() {
        let routes = ws_routes(
            "https://rd.example.com/",
            "https://rd.example.com:21118/",
            "wss://rd.example.com:21119/",
        );
        assert_eq!(routes.id, "wss://rd.example.com:21118");
        assert_eq!(routes.relay, "wss://rd.example.com:21119");
    }

    #[test]
    fn shell_quotes_single_quotes() {
        assert_eq!(sh_arg("a'b"), "'a'\\''b'");
    }
}

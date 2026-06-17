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
    pub mobile_config_text: String,
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

#[derive(Debug, Clone, Default)]
pub struct DeploymentOptions {
    pub permanent_password: String,
    pub approve_mode: String,
    pub verification_method: String,
}

#[derive(Serialize)]
struct RustDeskServerConfig<'a> {
    host: &'a str,
    relay: &'a str,
    api: &'a str,
    key: &'a str,
}

pub fn build(cfg: &WebClientConfig) -> DeploymentConfig {
    build_with_options(cfg, &DeploymentOptions::default())
}

pub fn build_with_password(cfg: &WebClientConfig, permanent_password: &str) -> DeploymentConfig {
    build_with_options(
        cfg,
        &DeploymentOptions {
            permanent_password: permanent_password.to_string(),
            ..Default::default()
        },
    )
}

pub fn build_with_options(cfg: &WebClientConfig, options: &DeploymentOptions) -> DeploymentConfig {
    let id_server = cfg.id_server.trim().to_string();
    let relay_server = cfg.relay_server.trim().to_string();
    let api_server = cfg.api_server.trim().to_string();
    let ws_host = cfg.ws_host.trim().to_string();
    let ws_id_host = cfg.ws_id_host.trim().to_string();
    let ws_relay_host = cfg.ws_relay_host.trim().to_string();
    let key = cfg.key.trim().to_string();
    let encoded_config = encode_server_config(&id_server, &relay_server, &api_server, &key);
    let mobile_config_text = format!("config={encoded_config}");
    let filename_hint = filename_hint(&id_server, &relay_server, &api_server, &key);
    let mut config_command = DeploymentCommandSet {
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
    let mut option_commands = DeploymentCommandSet {
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
    append_deployment_options(&mut config_command, options);
    append_deployment_options(&mut option_commands, options);
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
        mobile_config_text,
        filename_hint,
        config_command,
        option_commands,
        webclient_ws_routes,
    }
}

fn append_deployment_options(commands: &mut DeploymentCommandSet, options: &DeploymentOptions) {
    let password = options.permanent_password.trim();
    if password.is_empty() {
    } else {
        commands
            .linux
            .push(format!("sudo rustdesk --password {}", sh_arg(password)));
        commands.macos.push(format!(
            "sudo /Applications/RustDesk.app/Contents/MacOS/RustDesk --password {}",
            sh_arg(password)
        ));
        commands
            .windows
            .push(format!("rustdesk.exe --password {}", cmd_arg(password)));
    }
    append_option_if_valid(
        commands,
        "approve-mode",
        normalize_approve_mode(&options.approve_mode),
    );
    append_option_if_valid(
        commands,
        "verification-method",
        normalize_verification_method(&options.verification_method),
    );
}

fn append_option_if_valid(commands: &mut DeploymentCommandSet, name: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    commands
        .linux
        .push(format!("sudo rustdesk --option {name} {}", sh_arg(value)));
    commands.macos.push(format!(
        "sudo /Applications/RustDesk.app/Contents/MacOS/RustDesk --option {name} {}",
        sh_arg(value)
    ));
    commands
        .windows
        .push(format!("rustdesk.exe --option {name} {}", cmd_arg(value)));
}

fn normalize_approve_mode(value: &str) -> &str {
    match value.trim() {
        "password" => "password",
        "click" => "click",
        "password-click" => "password-click",
        _ => "",
    }
}

fn normalize_verification_method(value: &str) -> &str {
    match value.trim() {
        "use-temporary-password" => "use-temporary-password",
        "use-permanent-password" => "use-permanent-password",
        "use-both-passwords" => "use-both-passwords",
        _ => "",
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

    #[test]
    fn build_without_password_keeps_existing_command_shape() {
        let cfg = WebClientConfig {
            id_server: "id.example.com:21116".to_string(),
            relay_server: "relay.example.com:21117".to_string(),
            api_server: "https://api.example.com".to_string(),
            key: "pk".to_string(),
            ..Default::default()
        };
        let without = build(&cfg);
        let with_empty = build_with_password(&cfg, "");
        assert_eq!(
            without.config_command.linux,
            with_empty.config_command.linux
        );
        assert_eq!(
            without.config_command.macos,
            with_empty.config_command.macos
        );
        assert_eq!(
            without.config_command.windows,
            with_empty.config_command.windows
        );
        assert_eq!(
            without.option_commands.linux,
            with_empty.option_commands.linux
        );
        assert_eq!(
            without.option_commands.macos,
            with_empty.option_commands.macos
        );
        assert_eq!(
            without.option_commands.windows,
            with_empty.option_commands.windows
        );
    }

    #[test]
    fn appends_shell_quoted_password_commands() {
        let cfg = WebClientConfig::default();
        let deployment = build_with_password(&cfg, "abc'123");
        assert!(deployment
            .config_command
            .linux
            .contains(&"sudo rustdesk --password 'abc'\\''123'".to_string()));
        assert!(deployment.config_command.macos.contains(
            &"sudo /Applications/RustDesk.app/Contents/MacOS/RustDesk --password 'abc'\\''123'"
                .to_string()
        ));
        assert!(deployment
            .option_commands
            .linux
            .contains(&"sudo rustdesk --password 'abc'\\''123'".to_string()));
    }

    #[test]
    fn appends_cmd_quoted_password_command() {
        let cfg = WebClientConfig::default();
        let deployment = build_with_password(&cfg, "abc\"123");
        assert!(deployment
            .config_command
            .windows
            .contains(&"rustdesk.exe --password \"abc\\\"123\"".to_string()));
        assert!(deployment
            .option_commands
            .windows
            .contains(&"rustdesk.exe --password \"abc\\\"123\"".to_string()));
    }

    #[test]
    fn password_is_not_encoded_or_added_to_filename_hint() {
        let cfg = WebClientConfig {
            id_server: "id.example.com:21116".to_string(),
            relay_server: "relay.example.com:21117".to_string(),
            api_server: "https://api.example.com".to_string(),
            key: "pk".to_string(),
            ..Default::default()
        };
        let deployment = build_with_password(&cfg, "do-not-store");
        assert!(!deployment.encoded_config.contains("do-not-store"));
        assert!(!deployment.mobile_config_text.contains("do-not-store"));
        assert!(!deployment.filename_hint.contains("do-not-store"));
        let normalized = deployment.encoded_config.chars().rev().collect::<String>();
        let decoded = URL_SAFE_NO_PAD.decode(normalized).unwrap();
        let json = String::from_utf8(decoded).unwrap();
        assert!(!json.contains("do-not-store"));
    }

    #[test]
    fn mobile_config_text_reuses_encoded_config() {
        let cfg = WebClientConfig {
            id_server: "id.example.com:21116".to_string(),
            relay_server: "relay.example.com:21117".to_string(),
            api_server: "https://api.example.com".to_string(),
            key: "pk".to_string(),
            ..Default::default()
        };
        let deployment = build(&cfg);
        assert_eq!(
            deployment.mobile_config_text,
            format!("config={}", deployment.encoded_config)
        );
    }

    #[test]
    fn appends_approve_mode_commands() {
        let deployment = build_with_options(
            &WebClientConfig::default(),
            &DeploymentOptions {
                approve_mode: "password".to_string(),
                ..Default::default()
            },
        );
        assert!(deployment
            .config_command
            .linux
            .contains(&"sudo rustdesk --option approve-mode 'password'".to_string()));
        assert!(deployment.config_command.macos.contains(
            &"sudo /Applications/RustDesk.app/Contents/MacOS/RustDesk --option approve-mode 'password'"
                .to_string()
        ));
        assert!(deployment
            .config_command
            .windows
            .contains(&"rustdesk.exe --option approve-mode \"password\"".to_string()));
    }

    #[test]
    fn appends_verification_method_commands() {
        let deployment = build_with_options(
            &WebClientConfig::default(),
            &DeploymentOptions {
                verification_method: "use-permanent-password".to_string(),
                ..Default::default()
            },
        );
        assert!(deployment.config_command.linux.contains(
            &"sudo rustdesk --option verification-method 'use-permanent-password'".to_string()
        ));
        assert!(deployment.config_command.windows.contains(
            &"rustdesk.exe --option verification-method \"use-permanent-password\"".to_string()
        ));
    }

    #[test]
    fn invalid_mode_values_do_not_generate_commands() {
        let deployment = build_with_options(
            &WebClientConfig::default(),
            &DeploymentOptions {
                approve_mode: "abc'123".to_string(),
                verification_method: "abc\"123".to_string(),
                ..Default::default()
            },
        );
        assert!(!deployment
            .config_command
            .linux
            .iter()
            .any(|cmd| cmd.contains("approve-mode") || cmd.contains("verification-method")));
        assert!(!deployment
            .config_command
            .windows
            .iter()
            .any(|cmd| cmd.contains("approve-mode") || cmd.contains("verification-method")));
    }

    #[test]
    fn modes_are_not_encoded_or_added_to_filename_hint() {
        let cfg = WebClientConfig {
            id_server: "id.example.com:21116".to_string(),
            ..Default::default()
        };
        let deployment = build_with_options(
            &cfg,
            &DeploymentOptions {
                approve_mode: "password".to_string(),
                verification_method: "use-permanent-password".to_string(),
                ..Default::default()
            },
        );
        assert!(!deployment.encoded_config.contains("approve-mode"));
        assert!(!deployment.encoded_config.contains("verification-method"));
        assert!(!deployment.mobile_config_text.contains("approve-mode"));
        assert!(!deployment
            .mobile_config_text
            .contains("verification-method"));
        assert!(!deployment.filename_hint.contains("approve-mode"));
        assert!(!deployment.filename_hint.contains("verification-method"));
        let normalized = deployment.encoded_config.chars().rev().collect::<String>();
        let decoded = URL_SAFE_NO_PAD.decode(normalized).unwrap();
        let json = String::from_utf8(decoded).unwrap();
        assert!(!json.contains("approve-mode"));
        assert!(!json.contains("verification-method"));
    }
}

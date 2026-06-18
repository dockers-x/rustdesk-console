use urlencoding::encode;

pub const SCHEME: &str = "rustdesk";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustDeskAction {
    MainWindow,
    Connect,
    FileTransfer,
    ViewCamera,
    PortForward,
    Rdp,
    Terminal,
    TerminalAdmin,
    Config,
    Password,
}

impl RustDeskAction {
    pub fn authority(self) -> Option<&'static str> {
        match self {
            Self::MainWindow => None,
            Self::Connect => Some("connect"),
            Self::FileTransfer => Some("file-transfer"),
            Self::ViewCamera => Some("view-camera"),
            Self::PortForward => Some("port-forward"),
            Self::Rdp => Some("rdp"),
            Self::Terminal => Some("terminal"),
            Self::TerminalAdmin => Some("terminal-admin"),
            Self::Config => Some("config"),
            Self::Password => Some("password"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustDeskPlatform {
    Windows,
    Macos,
    Linux,
    Android,
    Ios,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebClientVersion {
    V1,
    V2,
}

impl WebClientVersion {
    pub fn path(self) -> &'static str {
        match self {
            Self::V1 => "/webclient/",
            Self::V2 => "/webclient2/",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Supported,
    Unsupported,
    RequiresClientOption(&'static str),
    RequiresProtocolRegistration,
    DependsOnWebClient,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionParams {
    pub password: Option<String>,
    pub force_relay: bool,
    pub switch_uuid: Option<String>,
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebClientLaunch {
    pub origin: String,
    pub version: WebClientVersion,
}

pub fn platform_capability(platform: RustDeskPlatform, action: RustDeskAction) -> Capability {
    use RustDeskAction::*;
    use RustDeskPlatform::*;
    match (platform, action) {
        (Web, MainWindow | Connect | FileTransfer | ViewCamera) => Capability::DependsOnWebClient,
        (Web, _) => Capability::Unsupported,
        (Android | Ios, Config) => {
            Capability::RequiresClientOption("allow-deep-link-server-settings")
        }
        (Android | Ios, Password) => Capability::RequiresClientOption("allow-deep-link-password"),
        (Windows | Macos | Linux, Config | Password) => Capability::Unsupported,
        (Android | Ios, PortForward | Rdp) => Capability::Unsupported,
        (Windows | Macos | Linux, _) => Capability::RequiresProtocolRegistration,
        (Android | Ios, _) => Capability::Supported,
    }
}

pub fn main_window_uri() -> String {
    format!("{SCHEME}://")
}

pub fn connection_uri(action: RustDeskAction, peer_id: &str, params: &ConnectionParams) -> String {
    let peer_id = peer_id.trim();
    let authority = action.authority().unwrap_or("connect");
    let mut url = format!("{SCHEME}://{authority}/{}", encode(peer_id));
    let query = connection_query(params);
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query);
    }
    url
}

pub fn shorthand_connect_uri(peer_id: &str, params: &ConnectionParams) -> String {
    let peer_id = peer_id.trim();
    let mut url = format!("{SCHEME}://{}", encode(peer_id));
    let query = connection_query(params);
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query);
    }
    url
}

pub fn mobile_config_uri(encoded_config: &str) -> String {
    format!("{SCHEME}://config/{}", encode(encoded_config.trim()))
}

pub fn mobile_password_uri(password: &str) -> String {
    format!("{SCHEME}://password/{}", encode(password.trim()))
}

pub fn webclient_url(launch: &WebClientLaunch, peer_id: &str) -> String {
    let origin = launch.origin.trim_end_matches('/');
    match launch.version {
        WebClientVersion::V1 => {
            format!(
                "{origin}{}#/{}",
                launch.version.path(),
                encode(peer_id.trim())
            )
        }
        WebClientVersion::V2 => {
            format!(
                "{origin}{}#/?id={}",
                launch.version.path(),
                encode(peer_id.trim())
            )
        }
    }
}

pub fn webclient_share_url(launch: &WebClientLaunch, share_token: &str) -> String {
    let origin = launch.origin.trim_end_matches('/');
    format!(
        "{origin}{}#/?share_token={}",
        launch.version.path(),
        encode(share_token.trim())
    )
}

fn connection_query(params: &ConnectionParams) -> String {
    let mut pairs = Vec::new();
    if let Some(password) = params
        .password
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        pairs.push(format!("password={}", encode(password)));
    }
    if params.force_relay {
        pairs.push("relay=true".to_string());
    }
    if let Some(switch_uuid) = params
        .switch_uuid
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        pairs.push(format!("switch_uuid={}", encode(switch_uuid)));
    }
    if let Some(key) = params
        .key
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        pairs.push(format!("key={}", encode(key)));
    }
    pairs.join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_shorthand_and_explicit_connection_uris() {
        let params = ConnectionParams {
            password: Some("p@ss word".to_string()),
            force_relay: true,
            switch_uuid: Some("uuid-1".to_string()),
            key: Some("pk+/=".to_string()),
        };
        assert_eq!(
            shorthand_connect_uri("182 921", &params),
            "rustdesk://182%20921?password=p%40ss%20word&relay=true&switch_uuid=uuid-1&key=pk%2B%2F%3D"
        );
        assert_eq!(
            connection_uri(
                RustDeskAction::FileTransfer,
                "182921",
                &ConnectionParams::default()
            ),
            "rustdesk://file-transfer/182921"
        );
    }

    #[test]
    fn builds_mobile_config_and_password_uris() {
        assert_eq!(
            mobile_config_uri("abc/+="),
            "rustdesk://config/abc%2F%2B%3D"
        );
        assert_eq!(
            mobile_password_uri("p@ss word"),
            "rustdesk://password/p%40ss%20word"
        );
    }

    #[test]
    fn distinguishes_webclient_versions() {
        assert_eq!(
            webclient_url(
                &WebClientLaunch {
                    origin: "https://console.example.com/".to_string(),
                    version: WebClientVersion::V1,
                },
                "182921"
            ),
            "https://console.example.com/webclient/#/182921"
        );
        assert_eq!(
            webclient_url(
                &WebClientLaunch {
                    origin: "https://console.example.com".to_string(),
                    version: WebClientVersion::V2,
                },
                "182921"
            ),
            "https://console.example.com/webclient2/#/?id=182921"
        );
        assert_eq!(
            webclient_share_url(
                &WebClientLaunch {
                    origin: "https://console.example.com".to_string(),
                    version: WebClientVersion::V2,
                },
                "share token"
            ),
            "https://console.example.com/webclient2/#/?share_token=share%20token"
        );
    }

    #[test]
    fn exposes_platform_capabilities() {
        assert_eq!(
            platform_capability(RustDeskPlatform::Android, RustDeskAction::Config),
            Capability::RequiresClientOption("allow-deep-link-server-settings")
        );
        assert_eq!(
            platform_capability(RustDeskPlatform::Windows, RustDeskAction::Config),
            Capability::Unsupported
        );
        assert_eq!(
            platform_capability(RustDeskPlatform::Web, RustDeskAction::Connect),
            Capability::DependsOnWebClient
        );
    }
}

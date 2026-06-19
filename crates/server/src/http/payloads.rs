//! Response payloads, mirroring `http/response/api/*.go` and
//! `http/response/admin/user.go`.

use serde::Serialize;
use serde_json::{Map, Value};

use entity::{address_book, peer, share_record, user};

/// `apiResp.UserPayload`.
#[derive(Debug, Serialize)]
pub struct UserPayload {
    pub guid: String,
    pub name: String,
    pub group_name: String,
    pub display_name: String,
    pub avatar: String,
    pub email: String,
    pub note: String,
    pub verifier: Option<String>,
    pub is_admin: Option<bool>,
    pub status: i32,
    pub info: Map<String, Value>,
}

impl UserPayload {
    pub fn from_user(u: &user::Model) -> Self {
        Self::from_user_with_avatar_base(u, "")
    }

    pub fn from_user_with_avatar_base(u: &user::Model, avatar_base_url: &str) -> Self {
        let mut info = Map::new();
        info.insert(
            "email_verification".to_string(),
            Value::Bool(u.email_verification_enabled),
        );
        info.insert("login_device_whitelist".to_string(), Value::Array(vec![]));
        UserPayload {
            guid: u.id.to_string(),
            name: u.username.clone(),
            group_name: String::new(),
            display_name: u.nickname.clone(),
            avatar: public_avatar_url(&u.avatar, avatar_base_url),
            email: u.email.clone(),
            note: String::new(),
            verifier: None,
            is_admin: u.is_admin,
            status: u.status,
            info,
        }
    }
}

pub fn public_avatar_url(avatar: &str, base_url: &str) -> String {
    let avatar = avatar.trim();
    if avatar.starts_with("/upload/") {
        let base_url = base_url.trim().trim_end_matches('/');
        if !base_url.is_empty() {
            return format!("{base_url}{avatar}");
        }
    }
    avatar.to_string()
}

/// `apiResp.LoginRes`.
#[derive(Debug, Serialize)]
pub struct LoginRes {
    #[serde(rename = "type")]
    pub r#type: String,
    pub access_token: String,
    pub user: UserPayload,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub secret: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tfa_type: String,
}

/// `apiResp.PeerPayloadInfo`.
#[derive(Debug, Serialize)]
pub struct PeerPayloadInfo {
    pub device_name: String,
    pub os: String,
    pub username: String,
}

/// `apiResp.GroupPeerPayload`.
#[derive(Debug, Serialize)]
pub struct GroupPeerPayload {
    pub id: String,
    pub info: PeerPayloadInfo,
    pub status: i32,
    pub user: String,
    pub user_name: String,
    pub note: String,
    pub device_group_name: String,
}

impl GroupPeerPayload {
    pub fn from_peer(p: &peer::Model, username: &str, device_group_name: &str) -> Self {
        GroupPeerPayload {
            id: p.id.clone(),
            info: PeerPayloadInfo {
                device_name: p.hostname.clone(),
                os: p.os.clone(),
                username: p.username.clone(),
            },
            status: 0,
            user: String::new(),
            user_name: username.to_string(),
            note: String::new(),
            device_group_name: device_group_name.to_string(),
        }
    }
}

/// `apiResp.WebClientPeerInfoPayload`.
#[derive(Debug, Serialize)]
pub struct WebClientPeerInfoPayload {
    pub username: String,
    pub hostname: String,
    pub platform: String,
    pub hash: String,
    pub id: String,
}

/// `apiResp.WebClientPeerPayload`.
#[derive(Debug, Serialize)]
pub struct WebClientPeerPayload {
    #[serde(rename = "view-style")]
    pub view_style: String,
    pub tm: i64,
    pub info: WebClientPeerInfoPayload,
    pub tmppwd: String,
}

impl WebClientPeerPayload {
    pub fn from_address_book(a: &address_book::Model, tm: i64) -> Self {
        WebClientPeerPayload {
            view_style: "shrink".into(),
            tm,
            info: WebClientPeerInfoPayload {
                username: a.username.clone(),
                hostname: a.hostname.clone(),
                platform: a.platform.clone(),
                hash: a.hash.clone(),
                id: String::new(),
            },
            tmppwd: String::new(),
        }
    }

    pub fn from_share_record(sr: &share_record::Model, tm: i64) -> Self {
        WebClientPeerPayload {
            view_style: "shrink".into(),
            tm,
            info: WebClientPeerInfoPayload {
                username: String::new(),
                hostname: String::new(),
                platform: String::new(),
                hash: String::new(),
                id: sr.peer_id.clone(),
            },
            tmppwd: sr.password.clone(),
        }
    }
}

/// `apiResp.SharedProfilesPayload`.
#[derive(Debug, Serialize)]
pub struct SharedProfilesPayload {
    pub guid: String,
    pub name: String,
    pub owner: String,
    pub note: String,
    pub rule: i32,
}

/// `adResp.LoginPayload` (admin panel login response).
#[derive(Debug, Serialize)]
pub struct AdminLoginPayload {
    pub username: String,
    pub email: String,
    pub avatar: String,
    pub token: String,
    pub route_names: Vec<String>,
    pub nickname: String,
    pub must_change_password: bool,
    pub tfa_enabled: bool,
    pub tfa_enforced: bool,
    pub email_verification_enabled: bool,
    pub login_device_verification_enabled: bool,
}

impl AdminLoginPayload {
    pub fn from_user(u: &user::Model, token: String) -> Self {
        let route_names = if u.is_admin() {
            vec!["*".to_string()]
        } else {
            vec![
                "MyTagList",
                "MyAddressBookList",
                "MyInfo",
                "MyAddressBookCollection",
                "MyPeer",
                "MyShareRecordList",
                "MyLoginLog",
            ]
            .into_iter()
            .map(String::from)
            .collect()
        };
        AdminLoginPayload {
            username: u.username.clone(),
            email: u.email.clone(),
            avatar: u.avatar.clone(),
            token,
            route_names,
            nickname: u.nickname.clone(),
            must_change_password: u.must_change_password,
            tfa_enabled: u.tfa_enabled,
            tfa_enforced: u.tfa_enforced,
            email_verification_enabled: u.email_verification_enabled,
            login_device_verification_enabled: u.login_device_verification_enabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::public_avatar_url;

    #[test]
    fn public_avatar_url_makes_uploaded_avatar_absolute() {
        assert_eq!(
            public_avatar_url("/upload/20260619/a.png", "https://console.example.com/"),
            "https://console.example.com/upload/20260619/a.png"
        );
    }

    #[test]
    fn public_avatar_url_keeps_external_and_data_avatars() {
        assert_eq!(
            public_avatar_url(
                "https://cdn.example.com/a.png",
                "https://console.example.com"
            ),
            "https://cdn.example.com/a.png"
        );
        assert_eq!(
            public_avatar_url("data:image/png;base64,abc", "https://console.example.com"),
            "data:image/png;base64,abc"
        );
    }

    #[test]
    fn public_avatar_url_keeps_relative_avatar_without_base() {
        assert_eq!(
            public_avatar_url("/upload/20260619/a.png", ""),
            "/upload/20260619/a.png"
        );
    }
}

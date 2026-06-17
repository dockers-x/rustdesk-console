//! OAuth service — ports the management side of `service/oauth.go`:
//! provider CRUD, third-party bindings, and provider listing. The external
//! login/token-exchange flow lives in a later sub-phase.

use sea_orm::*;

use ::entity::{oauth, user, user_third};

use crate::services::{now, paginate};

pub async fn info_by_id(db: &DatabaseConnection, id: i32) -> Result<Option<oauth::Model>, DbErr> {
    oauth::Entity::find_by_id(id).one(db).await
}

pub async fn info_by_op(db: &DatabaseConnection, op: &str) -> Result<Option<oauth::Model>, DbErr> {
    oauth::Entity::find()
        .filter(oauth::Column::Op.eq(op))
        .one(db)
        .await
}

pub struct OauthListResult {
    pub list: Vec<oauth::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
) -> Result<OauthListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let q = oauth::Entity::find();
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(OauthListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn get_oauth_providers(db: &DatabaseConnection) -> Result<Vec<String>, DbErr> {
    Ok(oauth::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|o| o.op)
        .collect())
}

pub fn validate_oauth_type(t: &str) -> Result<(), String> {
    match t {
        oauth::TYPE_GITHUB
        | oauth::TYPE_GOOGLE
        | oauth::TYPE_OIDC
        | oauth::TYPE_WEBAUTH
        | oauth::TYPE_LINUXDO => Ok(()),
        _ => Err("invalid Oauth type".to_string()),
    }
}

/// Normalize provider fields (≈ `model.Oauth.FormatOauthInfo`). Mutates `m`.
pub fn format_oauth_info(m: &mut oauth::Model) -> Result<(), String> {
    let oauth_type = m.oauth_type.trim().to_string();
    validate_oauth_type(&oauth_type)?;
    match oauth_type.as_str() {
        oauth::TYPE_GITHUB => m.op = oauth::TYPE_GITHUB.into(),
        oauth::TYPE_GOOGLE => m.op = oauth::TYPE_GOOGLE.into(),
        oauth::TYPE_LINUXDO => m.op = oauth::TYPE_LINUXDO.into(),
        _ => {}
    }
    if m.op.trim().is_empty() && oauth_type == oauth::TYPE_OIDC {
        m.op = oauth::TYPE_OIDC.into();
    }
    if oauth_type == oauth::TYPE_GOOGLE && m.issuer.trim().is_empty() {
        m.issuer = oauth::ISSUER_GOOGLE.into();
    }
    if m.pkce_enable.is_none() {
        m.pkce_enable = Some(false);
    }
    if m.pkce_method.is_empty() {
        m.pkce_method = oauth::PKCE_METHOD_S256.into();
    }
    Ok(())
}

pub async fn create(db: &DatabaseConnection, mut m: oauth::Model) -> Result<oauth::Model, String> {
    format_oauth_info(&mut m)?;
    let am = oauth::ActiveModel {
        op: Set(m.op),
        oauth_type: Set(m.oauth_type),
        client_id: Set(m.client_id),
        client_secret: Set(m.client_secret),
        auto_register: Set(m.auto_register),
        scopes: Set(m.scopes),
        issuer: Set(m.issuer),
        pkce_enable: Set(m.pkce_enable),
        pkce_method: Set(m.pkce_method),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await.map_err(|e| e.to_string())
}

pub async fn update(db: &DatabaseConnection, mut m: oauth::Model) -> Result<(), String> {
    format_oauth_info(&mut m)?;
    let am = oauth::ActiveModel {
        id: Set(m.id),
        op: Set(m.op),
        oauth_type: Set(m.oauth_type),
        client_id: Set(m.client_id),
        client_secret: Set(m.client_secret),
        auto_register: Set(m.auto_register),
        scopes: Set(m.scopes),
        issuer: Set(m.issuer),
        pkce_enable: Set(m.pkce_enable),
        pkce_method: Set(m.pkce_method),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.update(db).await.map(|_| ()).map_err(|e| e.to_string())
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    oauth::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

// ---- third-party bindings ----

pub async fn user_third_info(
    db: &DatabaseConnection,
    op: &str,
    open_id: &str,
) -> Result<Option<user_third::Model>, DbErr> {
    user_third::Entity::find()
        .filter(user_third::Column::OpenId.eq(open_id))
        .filter(user_third::Column::Op.eq(op))
        .one(db)
        .await
}

pub async fn user_third_by_user_and_op(
    db: &DatabaseConnection,
    user_id: i32,
    op: &str,
) -> Result<Option<user_third::Model>, DbErr> {
    user_third::Entity::find()
        .filter(user_third::Column::UserId.eq(user_id))
        .filter(user_third::Column::Op.eq(op))
        .one(db)
        .await
}

pub async fn user_thirds_by_user_id(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<user_third::Model>, DbErr> {
    user_third::Entity::find()
        .filter(user_third::Column::UserId.eq(user_id))
        .all(db)
        .await
}

pub async fn unbind(db: &DatabaseConnection, user_id: i32, op: &str) -> Result<(), DbErr> {
    user_third::Entity::delete_many()
        .filter(user_third::Column::UserId.eq(user_id))
        .filter(user_third::Column::Op.eq(op))
        .exec(db)
        .await?;
    Ok(())
}

// ===========================================================================
// External login flow (OIDC / GitHub / Google / LinuxDO / webauth).
// Manual OAuth2 over rustls (reqwest). Requires real providers to verify.
// ===========================================================================

use base64::Engine;
use sha2::{Digest, Sha256};

use crate::config::Config;

/// The normalized third-party user (≈ `model.OauthUser`).
#[derive(Debug, Default, Clone)]
pub struct OauthUser {
    pub open_id: String,
    pub name: String,
    pub username: String,
    pub email: String,
    pub verified_email: bool,
    pub picture: String,
}

struct Endpoints {
    auth_url: String,
    token_url: String,
    userinfo_url: String,
    scopes: Vec<String>,
}

fn http_client(cfg: &Config) -> reqwest::Client {
    let mut b = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("rustdesk-console");
    if cfg.proxy.enable && !cfg.proxy.host.is_empty() {
        if let Ok(p) = reqwest::Proxy::all(&cfg.proxy.host) {
            b = b.proxy(p);
        }
    }
    b.build().unwrap_or_default()
}

fn redirect_url(cfg: &Config) -> String {
    format!("{}/api/oidc/callback", cfg.rustdesk.api_server)
}

fn split_scopes(scopes: &str) -> Vec<String> {
    let s = scopes.trim();
    let s = if s.is_empty() {
        oauth::OIDC_DEFAULT_SCOPES
    } else {
        s
    };
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

async fn endpoints(client: &reqwest::Client, info: &oauth::Model) -> Result<Endpoints, String> {
    match info.oauth_type.as_str() {
        oauth::TYPE_GITHUB => Ok(Endpoints {
            auth_url: "https://github.com/login/oauth/authorize".into(),
            token_url: "https://github.com/login/oauth/access_token".into(),
            userinfo_url: "https://api.github.com/user".into(),
            scopes: vec!["read:user".into(), "user:email".into()],
        }),
        oauth::TYPE_LINUXDO => Ok(Endpoints {
            auth_url: "https://connect.linux.do/oauth2/authorize".into(),
            token_url: "https://connect.linux.do/oauth2/token".into(),
            userinfo_url: "https://connect.linux.do/api/user".into(),
            scopes: vec!["profile".into()],
        }),
        // google + generic oidc: discover from the issuer's well-known document
        _ => {
            let issuer = info.issuer.trim_end_matches('/');
            let url = format!("{issuer}/.well-known/openid-configuration");
            let doc: serde_json::Value = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("OIDC discovery failed: {e}"))?
                .json()
                .await
                .map_err(|e| format!("OIDC discovery parse failed: {e}"))?;
            let s = |k: &str| {
                doc.get(k)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            Ok(Endpoints {
                auth_url: s("authorization_endpoint"),
                token_url: s("token_endpoint"),
                userinfo_url: s("userinfo_endpoint"),
                scopes: split_scopes(&info.scopes),
            })
        }
    }
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Build the authorize URL and per-flow secrets (≈ `BeginAuth`).
/// Returns `(state, verifier, nonce, url)`.
pub async fn begin_auth(
    cfg: &Config,
    db: &DatabaseConnection,
    op: &str,
) -> Result<(String, String, String, String), String> {
    let state = format!(
        "{}{}",
        crate::support::random::random_string(10),
        chrono::Utc::now().timestamp()
    );
    if op == oauth::TYPE_WEBAUTH {
        let url = format!("{}/_admin/#/oauth/{}", cfg.rustdesk.api_server, state);
        return Ok((state, String::new(), String::new(), url));
    }
    let info = info_by_op(db, op)
        .await
        .map_err(|e| e.to_string())?
        .filter(|o| !o.client_id.is_empty() && !o.client_secret.is_empty())
        .ok_or("ConfigNotFound")?;
    let client = http_client(cfg);
    let ep = endpoints(&client, &info).await?;
    let nonce = crate::support::random::random_string(10);
    let mut verifier = String::new();
    let mut params = vec![
        ("client_id", info.client_id.clone()),
        ("response_type", "code".to_string()),
        ("redirect_uri", redirect_url(cfg)),
        ("scope", ep.scopes.join(" ")),
        ("state", state.clone()),
        ("nonce", nonce.clone()),
    ];
    if info.pkce_enable.unwrap_or(false) {
        verifier = crate::support::random::random_string(64);
        if info.pkce_method == oauth::PKCE_METHOD_S256 {
            params.push(("code_challenge_method", "S256".into()));
            params.push(("code_challenge", pkce_challenge(&verifier)));
        } else {
            params.push(("code_challenge_method", "plain".into()));
            params.push(("code_challenge", verifier.clone()));
        }
    }
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect();
    let url = format!("{}?{}", ep.auth_url, query.join("&"));
    Ok((state, verifier, nonce, url))
}

/// Exchange `code` for an access token and fetch the user info (≈ `Callback`).
pub async fn callback(
    cfg: &Config,
    db: &DatabaseConnection,
    code: &str,
    verifier: &str,
    op: &str,
) -> Result<OauthUser, String> {
    let info = info_by_op(db, op)
        .await
        .map_err(|e| e.to_string())?
        .filter(|o| !o.client_id.is_empty())
        .ok_or("ConfigNotFound")?;
    let client = http_client(cfg);
    let ep = endpoints(&client, &info).await?;

    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_url(cfg)),
        ("client_id", info.client_id.clone()),
        ("client_secret", info.client_secret.clone()),
    ];
    if !verifier.is_empty() {
        form.push(("code_verifier", verifier.to_string()));
    }
    let token_resp: serde_json::Value = client
        .post(&ep.token_url)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("GetOauthTokenError: {e}"))?
        .json()
        .await
        .map_err(|e| format!("GetOauthTokenError: {e}"))?;
    let access_token = token_resp
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("GetOauthTokenError")?
        .to_string();

    let info_resp: serde_json::Value = client
        .get(&ep.userinfo_url)
        .bearer_auth(&access_token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("GetOauthUserInfoError: {e}"))?
        .json()
        .await
        .map_err(|e| format!("DecodeOauthUserInfoError: {e}"))?;

    let s = |k: &str| {
        info_resp
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let oauth_user = match info.oauth_type.as_str() {
        oauth::TYPE_GITHUB => {
            let id = info_resp.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            let mut email = s("email");
            // GitHub may not return the email inline; fetch the primary verified one.
            if email.is_empty() {
                if let Ok(emails) = client
                    .get("https://api.github.com/user/emails")
                    .bearer_auth(&access_token)
                    .header("Accept", "application/json")
                    .send()
                    .await
                {
                    if let Ok(list) = emails.json::<serde_json::Value>().await {
                        if let Some(arr) = list.as_array() {
                            for e in arr {
                                if e.get("primary").and_then(|v| v.as_bool()).unwrap_or(false)
                                    && e.get("verified").and_then(|v| v.as_bool()).unwrap_or(false)
                                {
                                    email = e
                                        .get("email")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            OauthUser {
                open_id: id.to_string(),
                name: s("name"),
                username: s("login").to_lowercase(),
                email,
                verified_email: true,
                picture: s("avatar_url"),
            }
        }
        oauth::TYPE_LINUXDO => {
            let id = info_resp.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            OauthUser {
                open_id: id.to_string(),
                name: s("name"),
                username: s("username").to_lowercase(),
                email: s("email"),
                verified_email: true,
                picture: s("avatar_url"),
            }
        }
        _ => {
            // oidc / google
            let preferred = s("preferred_username");
            let email = s("email");
            let username = if !preferred.is_empty() {
                preferred
            } else {
                email.to_lowercase()
            };
            OauthUser {
                open_id: s("sub"),
                name: s("name"),
                username,
                email,
                verified_email: info_resp
                    .get("email_verified")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                picture: s("picture"),
            }
        }
    };
    Ok(oauth_user)
}

/// Find or create the local user for an OAuth identity (≈ `RegisterByOauth`).
pub async fn register_or_login(
    db: &DatabaseConnection,
    ou: &OauthUser,
    op: &str,
) -> Result<user::Model, String> {
    let oauth_type = info_by_op(db, op)
        .await
        .map_err(|e| e.to_string())?
        .map(|o| o.oauth_type)
        .unwrap_or_else(|| op.to_string());

    // already bound?
    if let Ok(Some(ut)) = user_third_info(db, op, &ou.open_id).await {
        if let Ok(Some(u)) = crate::services::user::info_by_id(db, ut.user_id).await {
            return Ok(u);
        }
    }
    // bind to an existing user by email
    let email = ou.email.to_lowercase();
    if !email.is_empty() {
        if let Ok(Some(u)) = user::Entity::find()
            .filter(user::Column::Email.eq(&email))
            .one(db)
            .await
        {
            bind_user(db, u.id, ou, &oauth_type, op)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(u);
        }
    }
    // create a fresh user + binding
    let username = generate_unique_username(db, &ou.username).await;
    let am = user::ActiveModel {
        username: Set(username),
        email: Set(email),
        nickname: Set(ou.name.clone()),
        avatar: Set(ou.picture.clone()),
        group_id: Set(1),
        is_admin: Set(Some(false)),
        status: Set(user::STATUS_ENABLE),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    let created = am
        .insert(db)
        .await
        .map_err(|e| format!("OauthRegisterFailed: {e}"))?;
    bind_user(db, created.id, ou, &oauth_type, op)
        .await
        .map_err(|e| e.to_string())?;
    Ok(created)
}

async fn generate_unique_username(db: &DatabaseConnection, base: &str) -> String {
    let mut name = base.replace(' ', "").to_lowercase();
    if name.is_empty() {
        name = format!("user{}", chrono::Utc::now().timestamp());
    }
    let mut suffix = 0;
    loop {
        let candidate = if suffix == 0 {
            name.clone()
        } else {
            format!("{name}{suffix}")
        };
        let exists = user::Entity::find()
            .filter(user::Column::Username.eq(&candidate))
            .one(db)
            .await
            .ok()
            .flatten()
            .is_some();
        if !exists {
            return candidate;
        }
        suffix += 1;
    }
}

pub async fn bind_user(
    db: &DatabaseConnection,
    user_id: i32,
    ou: &OauthUser,
    oauth_type: &str,
    op: &str,
) -> Result<(), DbErr> {
    let am = user_third::ActiveModel {
        user_id: Set(user_id),
        open_id: Set(ou.open_id.clone()),
        name: Set(ou.name.clone()),
        username: Set(ou.username.clone()),
        email: Set(ou.email.to_lowercase()),
        verified_email: Set(ou.verified_email),
        picture: Set(ou.picture.clone()),
        oauth_type: Set(oauth_type.to_string()),
        third_type: Set(oauth_type.to_string()),
        op: Set(op.to_string()),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await?;
    Ok(())
}

//! OAuth service — ports the management side of `service/oauth.go`:
//! provider CRUD, third-party bindings, and provider listing. The external
//! login/token-exchange flow lives in a later sub-phase.

use sea_orm::*;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize)]
pub struct LoginProvider {
    pub name: String,
}

pub async fn get_login_providers(db: &DatabaseConnection) -> Result<Vec<LoginProvider>, DbErr> {
    Ok(oauth::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .filter(is_login_provider_configured)
        .map(|o| LoginProvider { name: o.op })
        .collect())
}

pub fn is_login_provider_configured(info: &oauth::Model) -> bool {
    if info.op.trim().is_empty() {
        return false;
    }
    match info.oauth_type.as_str() {
        oauth::TYPE_WEBAUTH => true,
        oauth::TYPE_GITHUB | oauth::TYPE_LINUXDO | oauth::TYPE_GOOGLE => {
            !info.client_id.trim().is_empty() && !info.client_secret.trim().is_empty()
        }
        oauth::TYPE_OIDC => {
            !info.client_id.trim().is_empty()
                && !info.client_secret.trim().is_empty()
                && !info.issuer.trim().is_empty()
        }
        _ => false,
    }
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
    issuer: String,
    jwks_uri: String,
    scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderTestResult {
    pub op: String,
    pub oauth_type: String,
    pub ready: bool,
    pub redirect_uri: String,
    pub auth_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    pub jwks_uri: String,
    pub scopes: Vec<String>,
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
            issuer: String::new(),
            jwks_uri: String::new(),
            scopes: vec!["read:user".into(), "user:email".into()],
        }),
        oauth::TYPE_LINUXDO => Ok(Endpoints {
            auth_url: "https://connect.linux.do/oauth2/authorize".into(),
            token_url: "https://connect.linux.do/oauth2/token".into(),
            userinfo_url: "https://connect.linux.do/api/user".into(),
            issuer: String::new(),
            jwks_uri: String::new(),
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
                .map_err(|_| "OidcDiscoveryFailed".to_string())?
                .json()
                .await
                .map_err(|_| "OidcDiscoveryParseFailed".to_string())?;
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
                issuer: s("issuer"),
                jwks_uri: s("jwks_uri"),
                scopes: split_scopes(&info.scopes),
            })
        }
    }
}

pub async fn test_provider_config(
    cfg: &Config,
    mut info: oauth::Model,
) -> Result<ProviderTestResult, String> {
    format_oauth_info(&mut info)?;
    if !is_login_provider_configured(&info) {
        return Err("ProviderNotReady".to_string());
    }
    if info.oauth_type == oauth::TYPE_WEBAUTH {
        return Ok(ProviderTestResult {
            op: info.op,
            oauth_type: info.oauth_type,
            ready: true,
            redirect_uri: redirect_url(cfg),
            auth_url: format!("{}/_admin/#/oauth/<state>", cfg.rustdesk.api_server),
            token_url: String::new(),
            userinfo_url: String::new(),
            jwks_uri: String::new(),
            scopes: Vec::new(),
        });
    }

    let client = http_client(cfg);
    let ep = endpoints(&client, &info).await?;
    validate_provider_endpoints(&info, &ep)?;
    Ok(ProviderTestResult {
        op: info.op,
        oauth_type: info.oauth_type,
        ready: true,
        redirect_uri: redirect_url(cfg),
        auth_url: ep.auth_url,
        token_url: ep.token_url,
        userinfo_url: ep.userinfo_url,
        jwks_uri: ep.jwks_uri,
        scopes: ep.scopes,
    })
}

fn validate_provider_endpoints(info: &oauth::Model, ep: &Endpoints) -> Result<(), String> {
    if ep.auth_url.trim().is_empty()
        || ep.token_url.trim().is_empty()
        || ep.userinfo_url.trim().is_empty()
    {
        return Err("OAuthProviderEndpointMissing".to_string());
    }
    if matches!(
        info.oauth_type.as_str(),
        oauth::TYPE_OIDC | oauth::TYPE_GOOGLE
    ) && ep.jwks_uri.trim().is_empty()
    {
        return Err("OAuthProviderJwksMissing".to_string());
    }
    Ok(())
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
        .filter(is_login_provider_configured)
        .ok_or("ConfigNotFound")?;
    let client = http_client(cfg);
    let ep = endpoints(&client, &info).await?;
    validate_provider_endpoints(&info, &ep)?;
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
#[derive(Debug, Deserialize)]
struct Jwks {
    #[serde(default)]
    keys: Vec<Jwk>,
}

#[derive(Debug, Clone, Deserialize)]
struct Jwk {
    #[serde(default)]
    kty: String,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    alg: Option<String>,
    #[serde(default)]
    n: Option<String>,
    #[serde(default)]
    e: Option<String>,
    #[serde(default)]
    x: Option<String>,
    #[serde(default)]
    y: Option<String>,
}

async fn validate_oidc_id_token(
    client: &reqwest::Client,
    ep: &Endpoints,
    info: &oauth::Model,
    id_token: &str,
    nonce: &str,
) -> Result<String, String> {
    let header =
        jsonwebtoken::decode_header(id_token).map_err(|_| "OidcIdTokenInvalid".to_string())?;
    let key = oidc_decoding_key(&header, &fetch_jwks(client, &ep.jwks_uri).await?)?;

    let mut validation = jsonwebtoken::Validation::new(header.alg);
    validation.set_audience(&[info.client_id.as_str()]);
    let issuer = if ep.issuer.trim().is_empty() {
        info.issuer.trim_end_matches('/').to_string()
    } else {
        ep.issuer.trim_end_matches('/').to_string()
    };
    validation.set_issuer(&[issuer.as_str()]);
    validation.required_spec_claims.insert("aud".to_string());
    validation.required_spec_claims.insert("iss".to_string());
    validation.required_spec_claims.insert("sub".to_string());

    let decoded = jsonwebtoken::decode::<serde_json::Value>(id_token, &key, &validation)
        .map_err(|_| "OidcIdTokenInvalid".to_string())?;
    if !nonce.is_empty() {
        let claim_nonce = decoded
            .claims
            .get("nonce")
            .and_then(|v| v.as_str())
            .ok_or("OidcNonceMissing")?;
        if claim_nonce != nonce {
            return Err("OidcNonceMismatch".to_string());
        }
    }
    decoded
        .claims
        .get("sub")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .ok_or("OidcSubjectMissing".to_string())
}

async fn fetch_jwks(client: &reqwest::Client, jwks_uri: &str) -> Result<Jwks, String> {
    if jwks_uri.trim().is_empty() {
        return Err("OAuthProviderJwksMissing".to_string());
    }
    client
        .get(jwks_uri)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|_| "OidcJwksFetchFailed".to_string())?
        .json()
        .await
        .map_err(|_| "OidcJwksParseFailed".to_string())
}

fn oidc_decoding_key(
    header: &jsonwebtoken::Header,
    jwks: &Jwks,
) -> Result<jsonwebtoken::DecodingKey, String> {
    if matches!(
        header.alg,
        jsonwebtoken::Algorithm::HS256
            | jsonwebtoken::Algorithm::HS384
            | jsonwebtoken::Algorithm::HS512
    ) {
        return Err("OidcUnsupportedAlgorithm".to_string());
    }
    let jwk = select_jwk(header, jwks).ok_or("OidcSigningKeyNotFound")?;
    match jwk.kty.as_str() {
        "RSA" => {
            let n = jwk.n.as_deref().ok_or("OidcSigningKeyInvalid")?;
            let e = jwk.e.as_deref().ok_or("OidcSigningKeyInvalid")?;
            jsonwebtoken::DecodingKey::from_rsa_components(n, e)
                .map_err(|_| "OidcSigningKeyInvalid".to_string())
        }
        "EC" => {
            let x = jwk.x.as_deref().ok_or("OidcSigningKeyInvalid")?;
            let y = jwk.y.as_deref().ok_or("OidcSigningKeyInvalid")?;
            jsonwebtoken::DecodingKey::from_ec_components(x, y)
                .map_err(|_| "OidcSigningKeyInvalid".to_string())
        }
        _ => Err("OidcUnsupportedKeyType".to_string()),
    }
}

fn select_jwk<'a>(header: &jsonwebtoken::Header, jwks: &'a Jwks) -> Option<&'a Jwk> {
    if let Some(kid) = header.kid.as_deref() {
        return jwks.keys.iter().find(|jwk| jwk.kid.as_deref() == Some(kid));
    }
    jwks.keys.iter().find(|jwk| {
        jwk.alg
            .as_deref()
            .map(|alg| alg_matches_header(alg, header.alg))
            .unwrap_or(true)
    })
}

fn alg_matches_header(alg: &str, header_alg: jsonwebtoken::Algorithm) -> bool {
    alg.parse::<jsonwebtoken::Algorithm>()
        .map(|alg| alg == header_alg)
        .unwrap_or(false)
}

pub async fn callback(
    cfg: &Config,
    db: &DatabaseConnection,
    code: &str,
    verifier: &str,
    nonce: &str,
    op: &str,
) -> Result<OauthUser, String> {
    let info = info_by_op(db, op)
        .await
        .map_err(|e| e.to_string())?
        .filter(is_login_provider_configured)
        .ok_or("ConfigNotFound")?;
    let client = http_client(cfg);
    let ep = endpoints(&client, &info).await?;
    validate_provider_endpoints(&info, &ep)?;

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

    let id_token_subject = if matches!(
        info.oauth_type.as_str(),
        oauth::TYPE_OIDC | oauth::TYPE_GOOGLE
    ) {
        let id_token = token_resp
            .get("id_token")
            .and_then(|v| v.as_str())
            .ok_or("GetOauthTokenError")?;
        Some(validate_oidc_id_token(&client, &ep, &info, id_token, nonce).await?)
    } else {
        None
    };

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
            let mut verified_email = false;
            // GitHub may not return the email inline; fetch the primary verified one.
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
                                verified_email = !email.is_empty();
                                break;
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
                verified_email,
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
            let sub = s("sub");
            if let Some(expected_sub) = id_token_subject.as_ref() {
                if !sub.is_empty() && sub != *expected_sub {
                    return Err("OidcSubjectMismatch".to_string());
                }
            }
            let username = if !preferred.is_empty() {
                preferred
            } else {
                email.to_lowercase()
            };
            OauthUser {
                open_id: if sub.is_empty() {
                    id_token_subject.clone().unwrap_or_default()
                } else {
                    sub
                },
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
    if !email.is_empty() && ou.verified_email {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Json, Router};
    use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};
    use rsa::traits::PublicKeyParts;
    use rsa::{RsaPrivateKey, RsaPublicKey};
    use serde_json::json;
    use tokio::net::TcpListener;

    const TEST_KID: &str = "test-key";

    fn oauth_model(oauth_type: &str) -> oauth::Model {
        oauth::Model {
            id: 1,
            op: oauth_type.to_string(),
            oauth_type: oauth_type.to_string(),
            client_id: "client-id".to_string(),
            client_secret: "client-secret".to_string(),
            auto_register: Some(false),
            scopes: String::new(),
            issuer: "https://issuer.example.com".to_string(),
            pkce_enable: Some(false),
            pkce_method: oauth::PKCE_METHOD_S256.to_string(),
            created_at: None,
            updated_at: None,
        }
    }

    fn jwt_header() -> jsonwebtoken::Header {
        jsonwebtoken::Header {
            alg: jsonwebtoken::Algorithm::RS256,
            kid: Some(TEST_KID.to_string()),
            ..Default::default()
        }
    }

    fn base64url(bytes: &[u8]) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    fn rsa_test_key() -> RsaPrivateKey {
        RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap()
    }

    fn signed_id_token(
        private_key: &RsaPrivateKey,
        issuer: &str,
        audience: &str,
        nonce: &str,
    ) -> String {
        let pem = private_key.to_pkcs1_pem(LineEnding::LF).unwrap();
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap();
        let claims = json!({
            "iss": issuer,
            "aud": audience,
            "sub": "oauth-subject-1",
            "nonce": nonce,
            "exp": chrono::Utc::now().timestamp() + 300,
            "iat": chrono::Utc::now().timestamp(),
        });
        jsonwebtoken::encode(&jwt_header(), &claims, &key).unwrap()
    }

    async fn jwks_server(private_key: &RsaPrivateKey) -> String {
        let public_key = RsaPublicKey::from(private_key);
        let jwks = json!({
            "keys": [{
                "kty": "RSA",
                "kid": TEST_KID,
                "alg": "RS256",
                "use": "sig",
                "n": base64url(&public_key.n().to_bytes_be()),
                "e": base64url(&public_key.e().to_bytes_be()),
            }]
        });
        let app = Router::new().route("/jwks", get(move || async move { Json(jwks) }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}/jwks")
    }

    async fn validate_test_token(
        private_key: &RsaPrivateKey,
        issuer: &str,
        audience: &str,
        expected_nonce: &str,
        token_nonce: &str,
    ) -> Result<String, String> {
        let jwks_uri = jwks_server(private_key).await;
        let ep = Endpoints {
            auth_url: "https://issuer.example.com/auth".to_string(),
            token_url: "https://issuer.example.com/token".to_string(),
            userinfo_url: "https://issuer.example.com/userinfo".to_string(),
            issuer: issuer.to_string(),
            jwks_uri,
            scopes: vec!["openid".to_string()],
        };
        let mut info = oauth_model(oauth::TYPE_OIDC);
        info.client_id = audience.to_string();
        let token = signed_id_token(private_key, issuer, audience, token_nonce);
        validate_oidc_id_token(
            &http_client(&Config::default()),
            &ep,
            &info,
            &token,
            expected_nonce,
        )
        .await
    }

    #[test]
    fn login_provider_configuration_filters_incomplete_oidc() {
        let mut oidc = oauth_model(oauth::TYPE_OIDC);
        assert!(is_login_provider_configured(&oidc));

        oidc.issuer = String::new();
        assert!(!is_login_provider_configured(&oidc));

        let mut github = oauth_model(oauth::TYPE_GITHUB);
        github.issuer = String::new();
        assert!(is_login_provider_configured(&github));

        github.client_secret = String::new();
        assert!(!is_login_provider_configured(&github));
    }

    #[test]
    fn select_jwk_prefers_header_kid() {
        let jwks = Jwks {
            keys: vec![
                Jwk {
                    kty: "RSA".to_string(),
                    kid: Some("old".to_string()),
                    alg: Some("RS256".to_string()),
                    n: Some("n-old".to_string()),
                    e: Some("e-old".to_string()),
                    x: None,
                    y: None,
                },
                Jwk {
                    kty: "RSA".to_string(),
                    kid: Some("current".to_string()),
                    alg: Some("RS256".to_string()),
                    n: Some("n-current".to_string()),
                    e: Some("e-current".to_string()),
                    x: None,
                    y: None,
                },
            ],
        };
        let header = jsonwebtoken::Header {
            kid: Some("current".to_string()),
            alg: jsonwebtoken::Algorithm::RS256,
            ..Default::default()
        };

        let selected = select_jwk(&header, &jwks).unwrap();

        assert_eq!(selected.n.as_deref(), Some("n-current"));
    }

    #[test]
    fn oidc_decoding_key_rejects_hmac_algorithms() {
        let jwks = Jwks { keys: Vec::new() };
        let header = jsonwebtoken::Header {
            alg: jsonwebtoken::Algorithm::HS256,
            ..Default::default()
        };

        let err = match oidc_decoding_key(&header, &jwks) {
            Ok(_) => panic!("HS256 should be rejected for OIDC ID token validation"),
            Err(err) => err,
        };

        assert_eq!(err, "OidcUnsupportedAlgorithm");
    }

    #[tokio::test]
    async fn validates_rs256_oidc_id_token_signature_and_claims() {
        let private_key = rsa_test_key();

        let sub = validate_test_token(
            &private_key,
            "https://issuer.example.com",
            "client-id",
            "nonce-1",
            "nonce-1",
        )
        .await
        .unwrap();

        assert_eq!(sub, "oauth-subject-1");
    }

    #[tokio::test]
    async fn rejects_oidc_id_token_nonce_mismatch() {
        let private_key = rsa_test_key();

        let err = validate_test_token(
            &private_key,
            "https://issuer.example.com",
            "client-id",
            "expected-nonce",
            "actual-nonce",
        )
        .await
        .unwrap_err();

        assert_eq!(err, "OidcNonceMismatch");
    }

    #[tokio::test]
    async fn rejects_oidc_id_token_audience_mismatch() {
        let private_key = rsa_test_key();
        let jwks_uri = jwks_server(&private_key).await;
        let ep = Endpoints {
            auth_url: "https://issuer.example.com/auth".to_string(),
            token_url: "https://issuer.example.com/token".to_string(),
            userinfo_url: "https://issuer.example.com/userinfo".to_string(),
            issuer: "https://issuer.example.com".to_string(),
            jwks_uri,
            scopes: vec!["openid".to_string()],
        };
        let info = oauth_model(oauth::TYPE_OIDC);
        let token = signed_id_token(
            &private_key,
            "https://issuer.example.com",
            "other-client",
            "nonce-1",
        );

        let err = validate_oidc_id_token(
            &http_client(&Config::default()),
            &ep,
            &info,
            &token,
            "nonce-1",
        )
        .await
        .unwrap_err();

        assert_eq!(err, "OidcIdTokenInvalid");
    }
}

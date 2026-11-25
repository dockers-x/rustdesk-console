//! OAuth provider configuration, verified identities, and account bindings.

use sea_orm::*;
use serde::{Deserialize, Serialize};

use ::entity::{oauth, user, user_third};

use crate::services::{now, paginate};

pub async fn info_by_id(db: &DatabaseConnection, id: i32) -> Result<Option<oauth::Model>, DbErr> {
    oauth::Entity::find_by_id(id).one(db).await
}

pub async fn info_by_op(
    db: &impl ConnectionTrait,
    op: &str,
) -> Result<Option<oauth::Model>, DbErr> {
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

fn deployment_provider(cfg: &Config) -> Option<oauth::Model> {
    let oidc = &cfg.oidc;
    let name = oidc.name.trim();
    let op = if name.is_empty() { "OIDC" } else { name };
    if op.eq_ignore_ascii_case(oauth::TYPE_WEBAUTH)
        || op.len() > 64
        || !op
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return None;
    }
    let info = oauth::Model {
        id: 0,
        op: op.into(),
        oauth_type: oauth::TYPE_OIDC.into(),
        client_id: oidc.client_id.trim().into(),
        client_secret: oidc.client_secret.clone(),
        issuer: oidc.issuer.trim().into(),
        auto_register: Some(false),
        scopes: oidc.scopes.clone(),
        pkce_enable: Some(true),
        pkce_method: oauth::PKCE_METHOD_S256.into(),
        created_at: None,
        updated_at: None,
    };
    is_login_provider_configured(&info).then_some(info)
}

pub async fn login_provider(
    db: &impl ConnectionTrait,
    cfg: &Config,
    op: &str,
) -> Result<Option<oauth::Model>, DbErr> {
    let registered = info_by_op(db, op).await?;
    let info = registered.or_else(|| deployment_provider(cfg).filter(|info| info.op == op));
    Ok(info.filter(|info| info.op == op && is_external_provider(info)))
}

fn is_external_provider(info: &oauth::Model) -> bool {
    !info.op.eq_ignore_ascii_case(oauth::TYPE_WEBAUTH)
        && info.oauth_type != oauth::TYPE_WEBAUTH
        && is_login_provider_configured(info)
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginProvider {
    pub name: String,
    pub oauth_type: String,
}

pub async fn get_login_provider_configs(
    db: &impl ConnectionTrait,
    cfg: &Config,
) -> Result<Vec<oauth::Model>, DbErr> {
    let mut providers = oauth::Entity::find().all(db).await?;
    if let Some(info) = deployment_provider(cfg).filter(|info| {
        !providers
            .iter()
            .any(|row| row.op.eq_ignore_ascii_case(&info.op))
    }) {
        providers.push(info);
    }
    providers.retain(is_external_provider);
    Ok(providers)
}

pub async fn get_login_providers(
    db: &DatabaseConnection,
    cfg: &Config,
) -> Result<Vec<LoginProvider>, DbErr> {
    Ok(get_login_provider_configs(db, cfg)
        .await?
        .into_iter()
        .map(|o| LoginProvider {
            name: o.op,
            oauth_type: o.oauth_type,
        })
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
    m.op = m.op.trim().to_owned();
    if m.op.is_empty() && oauth_type == oauth::TYPE_OIDC {
        m.op = oauth::TYPE_OIDC.into();
    }
    if m.op.eq_ignore_ascii_case(oauth::TYPE_WEBAUTH) && oauth_type != oauth::TYPE_WEBAUTH {
        return Err("OauthProviderNameReserved".into());
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
    let tx = db.begin().await.map_err(|error| error.to_string())?;
    let current = lock_stored_provider(&tx, m.id)
        .await
        .map_err(|error| error.to_string())?;
    if current.op != m.op
        && info_by_op(&tx, &m.op)
            .await
            .map_err(|error| error.to_string())?
            .is_some()
    {
        return Err("ItemExists".into());
    }
    if provider_identity(&current) != provider_identity(&m)
        && has_stored_bindings(&tx, &current.op)
            .await
            .map_err(|error| error.to_string())?
    {
        return Err("OauthProviderHasBindings".into());
    }
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
    am.update(&tx).await.map_err(|error| error.to_string())?;
    tx.commit().await.map_err(|error| error.to_string())
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    let tx = db.begin().await?;
    let provider = lock_stored_provider(&tx, id).await?;
    if has_stored_bindings(&tx, &provider.op).await? {
        return Err(DbErr::Custom("OauthProviderHasBindings".into()));
    }
    oauth::Entity::delete_by_id(id).exec(&tx).await?;
    tx.commit().await
}

async fn has_stored_bindings(db: &impl ConnectionTrait, op: &str) -> Result<bool, DbErr> {
    Ok(user_third::Entity::find()
        .filter(user_third::Column::Op.eq(op))
        .one(db)
        .await?
        .is_some())
}

async fn lock_stored_provider(db: &DatabaseTransaction, id: i32) -> Result<oauth::Model, DbErr> {
    oauth::Entity::update_many()
        .col_expr(
            oauth::Column::Id,
            sea_orm::sea_query::Expr::col(oauth::Column::Id).into(),
        )
        .filter(oauth::Column::Id.eq(id))
        .exec(db)
        .await?;
    let query = oauth::Entity::find_by_id(id);
    let query = if db.get_database_backend() == DatabaseBackend::Sqlite {
        query
    } else {
        query.lock_exclusive()
    };
    query
        .one(db)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("OAuth provider not found".into()))
}

pub fn provider_identity(info: &oauth::Model) -> String {
    let identity = (
        &info.id,
        &info.op,
        &info.oauth_type,
        &info.issuer,
        &info.client_id,
    );
    let encoded = serde_json::to_vec(&identity).expect("OAuth provider identity is serializable");
    format!("{:x}", Sha256::digest(encoded))
}

fn binding_key(info: &oauth::Model, sub: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{}:{sub}", provider_identity(info)))
    )
}

// ---- third-party bindings ----

pub fn binding_matches(binding: &user_third::Model, provider: &oauth::Model) -> bool {
    if binding.op != provider.op {
        return false;
    }
    match &binding.oidc_key {
        Some(key) => key == &binding_key(provider, &binding.open_id),
        None => provider.id != 0,
    }
}

async fn linked_identity(
    db: &impl ConnectionTrait,
    provider: &oauth::Model,
    subject: &str,
) -> Result<Option<user_third::Model>, DbErr> {
    let mut predicate =
        Condition::any().add(user_third::Column::OidcKey.eq(binding_key(provider, subject)));
    if provider.id != 0 {
        predicate = predicate.add(
            Condition::all()
                .add(user_third::Column::OidcKey.is_null())
                .add(user_third::Column::Op.eq(&provider.op))
                .add(user_third::Column::OpenId.eq(subject)),
        );
    }
    let candidates = user_third::Entity::find().filter(predicate);
    let mut matches = candidates
        .all(db)
        .await?
        .into_iter()
        .filter(|binding| binding.open_id == subject && binding_matches(binding, provider));
    let found = matches.next();
    if matches.next().is_some() {
        return Err(DbErr::Custom("Ambiguous OAuth identity".into()));
    }
    Ok(found)
}

pub async fn user_thirds_by_user_id(
    db: &impl ConnectionTrait,
    user_id: i32,
) -> Result<Vec<user_third::Model>, DbErr> {
    user_third::Entity::find()
        .filter(user_third::Column::UserId.eq(user_id))
        .order_by_asc(user_third::Column::Id)
        .all(db)
        .await
}

pub async fn binding_active(
    db: &impl ConnectionTrait,
    cfg: &Config,
    user_id: i32,
    binding_id: i32,
) -> Result<bool, DbErr> {
    let Some(binding) = user_third::Entity::find_by_id(binding_id)
        .one(db)
        .await?
        .filter(|binding| binding.user_id == user_id)
    else {
        return Ok(false);
    };
    Ok(login_provider(db, cfg, &binding.op)
        .await?
        .is_some_and(|provider| binding_matches(&binding, &provider)))
}

pub async fn unbind(
    db: &DatabaseConnection,
    cfg: &Config,
    user_id: i32,
    session_id: i32,
    op: &str,
) -> Result<(), DbErr> {
    let tx = db.begin().await?;
    let account = super::user::lock_account(&tx, user_id)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("user not found".into()))?;
    super::user::lock_account_management_session(&tx, &account, session_id).await?;
    let bindings = user_thirds_by_user_id(&tx, user_id).await?;
    let ids: Vec<_> = bindings
        .iter()
        .filter(|binding| binding.op == op)
        .map(|binding| binding.id)
        .collect();
    if ids.is_empty() {
        return Err(DbErr::RecordNotFound("ItemNotFound".into()));
    }
    if cfg.app.disable_pwd_login || account.password.is_empty() {
        let providers = get_login_provider_configs(&tx, cfg).await?;
        let has_alternative = bindings.iter().any(|binding| {
            binding.op != op
                && providers
                    .iter()
                    .any(|provider| binding_matches(binding, provider))
        });
        // A directory-backed account may also sign in with its directory credentials.
        let directory_login = !cfg.app.disable_pwd_login
            && crate::services::ldap_settings::read(&tx, &cfg.ldap)
                .await
                .is_ok_and(|settings| settings.enable)
            && ::entity::ldap_identity::Entity::find_by_id(user_id)
                .one(&tx)
                .await?
                .is_some();
        if !has_alternative && !directory_login {
            return Err(DbErr::Custom("OauthLastLoginMethod".into()));
        }
    }
    user_third::Entity::delete_many()
        .filter(user_third::Column::Id.is_in(ids))
        .exec(&tx)
        .await?;
    tx.commit().await
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
    format!(
        "{}/api/oidc/callback",
        cfg.rustdesk.api_server.trim_end_matches('/')
    )
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

fn validate_discovered_issuer(configured: &str, discovered: &str) -> Result<(), String> {
    if discovered.is_empty() {
        return Err("OidcIssuerMissing".to_string());
    }
    if discovered != configured {
        return Err("OidcIssuerMismatch".to_string());
    }
    Ok(())
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
            let issuer = s("issuer");
            validate_discovered_issuer(&info.issuer, &issuer)?;
            Ok(Endpoints {
                auth_url: s("authorization_endpoint"),
                token_url: s("token_endpoint"),
                userinfo_url: s("userinfo_endpoint"),
                issuer,
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
pub struct Authorization {
    pub state: String,
    pub verifier: String,
    pub nonce: String,
    pub url: String,
    pub provider_identity: String,
}

fn callback_url(cfg: &Config, provider: &oauth::Model) -> String {
    if provider.id == 0 && !cfg.oidc.redirect_url.trim().is_empty() {
        cfg.oidc.redirect_url.trim().to_owned()
    } else {
        redirect_url(cfg)
    }
}

pub async fn begin_auth(
    cfg: &Config,
    db: &DatabaseConnection,
    op: &str,
) -> Result<Authorization, String> {
    let state = crate::support::random::random_string(40);
    if op == oauth::TYPE_WEBAUTH {
        if !cfg.app.web_sso {
            return Err("Web SSO is disabled".into());
        }
        let url = format!(
            "{}/_admin/#/oauth/{}",
            cfg.rustdesk.api_server.trim_end_matches('/'),
            state
        );
        return Ok(Authorization {
            state,
            url,
            verifier: String::new(),
            nonce: String::new(),
            provider_identity: String::new(),
        });
    }
    let info = login_provider(db, cfg, op)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("ConfigNotFound")?;
    let client = http_client(cfg);
    let ep = endpoints(&client, &info).await?;
    validate_provider_endpoints(&info, &ep)?;
    let nonce = crate::support::random::random_string(40);
    let mut verifier = String::new();
    let mut params = vec![
        ("client_id", info.client_id.clone()),
        ("response_type", "code".to_string()),
        ("redirect_uri", callback_url(cfg, &info)),
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
    let mut url = url::Url::parse(&ep.auth_url).map_err(|_| "OAuthProviderEndpointMissing")?;
    url.query_pairs_mut().extend_pairs(params);
    Ok(Authorization {
        state,
        verifier,
        nonce,
        url: url.into(),
        provider_identity: provider_identity(&info),
    })
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
    validation.set_issuer(&[ep.issuer.as_str()]);
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

fn provider_numeric_id(profile: &serde_json::Value) -> Result<i64, String> {
    profile
        .get("id")
        .and_then(serde_json::Value::as_i64)
        .filter(|id| *id > 0)
        .ok_or_else(|| "GetOauthUserInfoError: missing or invalid user id".to_string())
}

pub async fn callback(
    cfg: &Config,
    info: &oauth::Model,
    code: &str,
    verifier: &str,
    nonce: &str,
) -> Result<OauthUser, String> {
    let client = http_client(cfg);
    let ep = endpoints(&client, info).await?;
    validate_provider_endpoints(info, &ep)?;

    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", callback_url(cfg, info)),
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
        .error_for_status()
        .map_err(|e| format!("GetOauthTokenError: {e}"))?
        .json()
        .await
        .map_err(|e| format!("GetOauthTokenError: {e}"))?;
    let access_token = token_resp
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|token| !token.trim().is_empty())
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
        Some(validate_oidc_id_token(&client, &ep, info, id_token, nonce).await?)
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
        .error_for_status()
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
            let id = provider_numeric_id(&info_resp)?;
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
            let id = provider_numeric_id(&info_resp)?;
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

/// Return the linked account, or provision a regular account when explicitly enabled.
pub async fn resolve_identity(
    db: &DatabaseConnection,
    provider: &oauth::Model,
    identity: &OauthUser,
    allow_registration: bool,
) -> Result<Option<user_third::Model>, DbErr> {
    validate_subject(identity)?;
    if let Some(binding) = linked_identity(db, provider, &identity.open_id).await? {
        return Ok(Some(binding));
    }
    if provider.id == 0
        || !provider.auto_register.unwrap_or_default()
        || !allow_registration
        || !super::user::has_admin(db).await?
    {
        return Ok(None);
    }
    let tx = db.begin().await?;
    let current = lock_stored_provider(&tx, provider.id).await?;
    if provider_identity(&current) != provider_identity(provider)
        || !current.auto_register.unwrap_or_default()
    {
        return Ok(None);
    }
    if let Some(binding) = linked_identity(&tx, &current, &identity.open_id).await? {
        return Ok(Some(binding));
    }
    let email = if identity.verified_email {
        identity.email.trim().to_lowercase()
    } else {
        String::new()
    };
    if !email.is_empty()
        && user::Entity::find()
            .filter(user::Column::Email.eq(&email))
            .one(&tx)
            .await?
            .is_some()
    {
        return Ok(None);
    }
    let username = generate_unique_username(&tx, &identity.username).await?;
    let account = user::ActiveModel {
        username: Set(username),
        email: Set(email),
        nickname: Set(identity.name.clone()),
        avatar: Set(identity.picture.clone()),
        group_id: Set(1),
        is_admin: Set(Some(false)),
        status: Set(user::STATUS_ENABLE),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    let created = super::login_security::insert_user_with_enrollment_policy(&tx, account).await?;
    let binding = bind_user(&tx, created.id, identity, &current).await?;
    tx.commit().await?;
    Ok(Some(binding))
}

async fn generate_unique_username(db: &impl ConnectionTrait, base: &str) -> Result<String, DbErr> {
    let name: String = base
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | '@')
        })
        .map(|character| character.to_ascii_lowercase())
        .take(64)
        .collect();
    let name = if name.len() >= 2 {
        name
    } else {
        "oauth-user".to_owned()
    };
    if user::Entity::find()
        .filter(user::Column::Username.eq(&name))
        .one(db)
        .await?
        .is_none()
    {
        return Ok(name);
    }
    Ok(format!(
        "{name}-{}",
        crate::support::random::random_string(16).to_ascii_lowercase()
    ))
}

fn validate_subject(identity: &OauthUser) -> Result<(), DbErr> {
    if identity.open_id.trim().is_empty() || identity.open_id.len() > 255 {
        return Err(DbErr::Custom("Invalid OAuth subject".into()));
    }
    Ok(())
}

pub async fn validate_binding_session(
    db: &DatabaseConnection,
    user_id: i32,
    session_id: i32,
) -> Result<(), DbErr> {
    let tx = db.begin().await?;
    let account = super::user::lock_account(&tx, user_id)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("user not found".into()))?;
    super::user::lock_account_management_session(&tx, &account, session_id).await?;
    tx.commit().await
}

/// Add a provider credential only while the session approving it is live.
pub async fn bind_user_with_session(
    db: &DatabaseConnection,
    cfg: &Config,
    user_id: i32,
    session_id: i32,
    expected: &str,
    op: &str,
    identity: &OauthUser,
) -> Result<(), DbErr> {
    validate_subject(identity)?;
    let tx = db.begin().await?;
    let account = super::user::lock_account(&tx, user_id)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("user not found".into()))?;
    super::user::lock_account_management_session(&tx, &account, session_id).await?;
    let mut provider = login_provider(&tx, cfg, op)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("OAuth provider not found".into()))?;
    if provider.id != 0 {
        provider = lock_stored_provider(&tx, provider.id).await?;
    }
    if provider_identity(&provider) != expected || !is_external_provider(&provider) {
        return Err(DbErr::Custom("OAuth provider changed".into()));
    }
    if user_thirds_by_user_id(&tx, user_id)
        .await?
        .iter()
        .any(|binding| binding.op == provider.op)
        || linked_identity(&tx, &provider, &identity.open_id)
            .await?
            .is_some()
    {
        return Err(DbErr::Custom("OauthHasBindOtherUser".into()));
    }
    bind_user(&tx, user_id, identity, &provider).await?;
    tx.commit().await
}

async fn bind_user(
    db: &impl ConnectionTrait,
    user_id: i32,
    identity: &OauthUser,
    provider: &oauth::Model,
) -> Result<user_third::Model, DbErr> {
    user_third::ActiveModel {
        user_id: Set(user_id),
        open_id: Set(identity.open_id.clone()),
        name: Set(identity.name.clone()),
        username: Set(identity.username.clone()),
        email: Set(identity.email.trim().to_lowercase()),
        verified_email: Set(identity.verified_email),
        picture: Set(identity.picture.clone()),
        oauth_type: Set(provider.oauth_type.clone()),
        third_type: Set(provider.oauth_type.clone()),
        op: Set(provider.op.clone()),
        oidc_key: Set(Some(binding_key(provider, &identity.open_id))),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    }
    .insert(db)
    .await
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

    #[test]
    fn provider_numeric_id_rejects_error_profiles_and_invalid_ids() {
        for profile in [
            json!({"message": "Bad credentials"}),
            json!({"id": 0}),
            json!({"id": -1}),
            json!({"id": "123"}),
        ] {
            assert!(provider_numeric_id(&profile).is_err());
        }
        assert_eq!(provider_numeric_id(&json!({"id": 123})).unwrap(), 123);
    }

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

    async fn accounts_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let cfg = Config::default();
        crate::bootstrap::migrate_and_seed(&db, &cfg, &crate::i18n::I18n::load("en"), "en")
            .await
            .unwrap();
        db
    }

    async fn account(
        db: &DatabaseConnection,
        username: &str,
        password: &str,
        admin: bool,
    ) -> user::Model {
        user::ActiveModel {
            username: Set(username.into()),
            email: Set(format!("{username}@example.test")),
            password: Set(password.into()),
            is_admin: Set(Some(admin)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn session(db: &DatabaseConnection, user_id: i32) -> i32 {
        ::entity::user_token::ActiveModel {
            user_id: Set(user_id),
            expired_at: Set(chrono::Utc::now().timestamp() + 600),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn deployment_binding_is_explicit_unique_and_revocable() {
        let db = accounts_db().await;
        let cfg = Config {
            oidc: crate::config::Oidc {
                name: "TestProvider".into(),
                issuer: "https://issuer.example.test".into(),
                client_id: "client".into(),
                client_secret: "test-secret".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let provider = login_provider(&db, &cfg, "TestProvider")
            .await
            .unwrap()
            .unwrap();
        let alice = account(&db, "alice", "test-hash", false).await;
        let bob = account(&db, "bob", "", false).await;
        let alice_session = session(&db, alice.id).await;
        let bob_session = session(&db, bob.id).await;
        let identity = OauthUser {
            open_id: "subject".into(),
            email: alice.email.clone(),
            verified_email: true,
            ..Default::default()
        };
        assert!(resolve_identity(&db, &provider, &identity, true)
            .await
            .unwrap()
            .is_none());

        let expected = provider_identity(&provider);
        bind_user_with_session(
            &db,
            &cfg,
            alice.id,
            alice_session,
            &expected,
            &provider.op,
            &identity,
        )
        .await
        .unwrap();
        assert!(bind_user_with_session(
            &db,
            &cfg,
            bob.id,
            bob_session,
            &expected,
            &provider.op,
            &identity
        )
        .await
        .is_err());
        let link = resolve_identity(&db, &provider, &identity, false)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(link.user_id, alice.id);
        assert!(binding_active(&db, &cfg, alice.id, link.id).await.unwrap());

        let mut changed = cfg.clone();
        changed.oidc.issuer = "https://other-issuer.example.test".into();
        assert!(!binding_active(&db, &changed, alice.id, link.id)
            .await
            .unwrap());
        unbind(&db, &cfg, alice.id, alice_session, &provider.op)
            .await
            .unwrap();
        assert!(!binding_active(&db, &cfg, alice.id, link.id).await.unwrap());
        assert!(resolve_identity(&db, &provider, &identity, true)
            .await
            .unwrap()
            .is_none());

        bind_user_with_session(
            &db,
            &cfg,
            bob.id,
            bob_session,
            &expected,
            &provider.op,
            &identity,
        )
        .await
        .unwrap();
        let error = unbind(&db, &cfg, bob.id, bob_session, &provider.op)
            .await
            .unwrap_err();
        assert!(matches!(error, DbErr::Custom(message) if message == "OauthLastLoginMethod"));
    }

    #[tokio::test]
    async fn oidc_provisioning_requires_an_initialized_site_and_creates_only_members() {
        let db = accounts_db().await;
        let cfg = Config::default();
        let mut record = oauth_model(oauth::TYPE_OIDC);
        record.auto_register = Some(true);
        let provider = create(&db, record).await.unwrap();
        let mut identity = OauthUser {
            open_id: "issuer-subject".into(),
            username: "New Member".into(),
            email: "admin@example.test".into(),
            verified_email: true,
            ..Default::default()
        };
        assert!(resolve_identity(&db, &provider, &identity, true)
            .await
            .unwrap()
            .is_none());

        account(&db, "admin", "test-hash", true).await;
        assert!(resolve_identity(&db, &provider, &identity, true)
            .await
            .unwrap()
            .is_none());
        identity.email = "member@example.test".into();
        assert!(resolve_identity(&db, &provider, &identity, false)
            .await
            .unwrap()
            .is_none());

        let linked = resolve_identity(&db, &provider, &identity, true)
            .await
            .unwrap()
            .unwrap();
        let same = resolve_identity(&db, &provider, &identity, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(linked.id, same.id);
        let member = user::Entity::find_by_id(linked.user_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(!member.is_admin());
        assert_eq!(member.group_id, 1);
        assert_eq!(user::Entity::find().count(&db).await.unwrap(), 2);
        assert!(binding_active(&db, &cfg, member.id, linked.id)
            .await
            .unwrap());
        let mut changed = provider.clone();
        changed.issuer = "https://other-issuer.example.test".into();
        assert_eq!(
            update(&db, changed).await.unwrap_err(),
            "OauthProviderHasBindings"
        );
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
        validate_test_token_with_token_issuer(
            private_key,
            issuer,
            issuer,
            audience,
            expected_nonce,
            token_nonce,
        )
        .await
    }

    async fn validate_test_token_with_token_issuer(
        private_key: &RsaPrivateKey,
        issuer: &str,
        token_issuer: &str,
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
        info.issuer = issuer.to_string();
        let token = signed_id_token(private_key, token_issuer, audience, token_nonce);
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
    async fn validates_oidc_issuer_with_trailing_slash() {
        let private_key = rsa_test_key();

        let sub = validate_test_token(
            &private_key,
            "https://issuer.example.com/",
            "client-id",
            "nonce-1",
            "nonce-1",
        )
        .await
        .unwrap();

        assert_eq!(sub, "oauth-subject-1");
    }

    #[test]
    fn rejects_missing_or_mismatched_discovered_issuer() {
        assert_eq!(
            validate_discovered_issuer("https://issuer.example.com/", "").unwrap_err(),
            "OidcIssuerMissing"
        );
        assert_eq!(
            validate_discovered_issuer("https://issuer.example.com/", "https://issuer.example.com")
                .unwrap_err(),
            "OidcIssuerMismatch"
        );
    }

    #[tokio::test]
    async fn rejects_oidc_issuer_trailing_slash_mismatch() {
        let private_key = rsa_test_key();

        let err = validate_test_token_with_token_issuer(
            &private_key,
            "https://issuer.example.com/",
            "https://issuer.example.com",
            "client-id",
            "nonce-1",
            "nonce-1",
        )
        .await
        .unwrap_err();

        assert_eq!(err, "OidcIdTokenInvalid");
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

//! LDAP password verification and explicit directory identity mapping.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use ldap3::{Ldap, LdapConnAsync, LdapConnSettings, Scope, SearchEntry, SearchOptions};
use sea_orm::*;
use sha2::{Digest, Sha256};
use url::Url;

use ::entity::{ldap_identity, user};

use crate::config::{Ldap as Settings, LdapIdentityAttribute, LdapScope};
use crate::services::now;
use crate::services::user::format_username;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const LDAP_TIMEOUT: Duration = Duration::from_secs(5);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_CA_SIZE: usize = 128 * 1024;

struct DirectoryUser {
    dn: String,
    subject: String,
    username: String,
    email: String,
    nickname: String,
}

fn attribute<'a>(value: &'a str, default: &'a str) -> &'a str {
    if value.is_empty() { default } else { value }
}

fn base_dn(cfg: &Settings) -> &str {
    attribute(&cfg.user.base_dn, &cfg.base_dn)
}

fn endpoint(cfg: &Settings) -> Result<Url, &'static str> {
    let url = Url::parse(&cfg.url).map_err(|_| "LdapInvalidUrl")?;
    if !matches!(url.scheme(), "ldap" | "ldaps")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("LdapInvalidUrl");
    }
    Ok(url)
}

pub fn has_safe_url(cfg: &Settings) -> bool {
    endpoint(cfg).is_ok()
}

pub fn validate(cfg: &Settings, require_bind: bool) -> Result<(), &'static str> {
    if !cfg.url.is_empty() || require_bind {
        let url = endpoint(cfg)?;
        if cfg.start_tls && url.scheme() != "ldap" {
            return Err("LdapInvalidTlsMode");
        }
        if (!cfg.tls_ca_pem.is_empty() || !cfg.tls_ca_file.is_empty())
            && (!(cfg.start_tls || url.scheme() == "ldaps") || !cfg.tls_verify)
        {
            return Err("LdapInvalidTlsMode");
        }
    }
    if cfg.tls_ca_pem.len() > MAX_CA_SIZE {
        return Err("LdapInvalidCa");
    }
    if !cfg.tls_ca_pem.is_empty() {
        ca_roots(cfg.tls_ca_pem.as_bytes())?;
    }
    if require_bind {
        if base_dn(cfg).trim().is_empty() {
            return Err("LdapMissingBaseDn");
        }
        if cfg.bind_dn.trim().is_empty() != cfg.bind_password.is_empty() {
            return Err("LdapInvalidBindCredentials");
        }
    }
    if [
        &cfg.user.username,
        &cfg.user.email,
        &cfg.user.first_name,
        &cfg.user.last_name,
        &cfg.user.enable_attr,
    ]
    .into_iter()
    .any(|attr| {
        !attr
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b';'))
    }) {
        return Err("LdapInvalidAttribute");
    }
    if !cfg.user.filter.is_empty()
        && !(cfg.user.filter.starts_with('(') && cfg.user.filter.ends_with(')'))
    {
        return Err("LdapInvalidFilter");
    }
    Ok(())
}

fn ca_roots(pem: &[u8]) -> Result<rustls::RootCertStore, &'static str> {
    let certs = rustls_pemfile::certs(&mut &*pem).map_err(|_| "LdapInvalidCa")?;
    if certs.is_empty() {
        return Err("LdapInvalidCa");
    }
    let mut roots = rustls::RootCertStore::empty();
    for cert in certs {
        roots
            .add(&rustls::Certificate(cert))
            .map_err(|_| "LdapInvalidCa")?;
    }
    Ok(roots)
}

fn custom_tls(pem: &str, file: &str) -> Result<rustls::ClientConfig, &'static str> {
    let pem = if pem.is_empty() {
        let file = std::fs::File::open(file).map_err(|_| "LdapInvalidCa")?;
        if !file.metadata().is_ok_and(|meta| meta.is_file()) {
            return Err("LdapInvalidCa");
        }
        let mut bytes = Vec::new();
        file.take((MAX_CA_SIZE + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "LdapInvalidCa")?;
        if bytes.len() > MAX_CA_SIZE {
            return Err("LdapInvalidCa");
        }
        bytes
    } else {
        pem.as_bytes().to_vec()
    };
    let mut roots = ca_roots(&pem)?;
    for cert in rustls_native_certs::load_native_certs().unwrap_or_default() {
        let _ = roots.add(&rustls::Certificate(cert.0));
    }
    Ok(rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

async fn connect_bind(cfg: &Settings, dn: &str, password: &str) -> Result<Ldap, &'static str> {
    let mut settings = LdapConnSettings::new()
        .set_conn_timeout(CONNECT_TIMEOUT)
        .set_starttls(cfg.start_tls)
        .set_no_tls_verify(!cfg.tls_verify);
    if !cfg.tls_ca_pem.is_empty() || !cfg.tls_ca_file.is_empty() {
        let pem = cfg.tls_ca_pem.clone();
        let file = cfg.tls_ca_file.clone();
        let tls = tokio::time::timeout(
            CONNECT_TIMEOUT,
            tokio::task::spawn_blocking(move || custom_tls(&pem, &file)),
        )
        .await
        .map_err(|_| "LdapConnectTimedOut")?
        .map_err(|_| "LdapInvalidCa")??;
        settings = settings.set_config(Arc::new(tls));
    }
    let (conn, mut ldap) = LdapConnAsync::from_url_with_settings(settings, &endpoint(cfg)?)
        .await
        .map_err(|_| "LdapConnectFailed")?;
    // The driver can contain directory-supplied error text. Keep it out of logs.
    tokio::spawn(async move {
        let _ = conn.drive().await;
    });
    ldap.with_timeout(LDAP_TIMEOUT)
        .simple_bind(dn, password)
        .await
        .map_err(|_| "LdapBindFailed")?
        .success()
        .map_err(|_| "LdapBindFailed")?;
    Ok(ldap)
}

async fn unbind(mut ldap: Ldap) {
    let _ = tokio::time::timeout(LDAP_TIMEOUT, ldap.unbind()).await;
}

pub async fn test_bind(cfg: &Settings) -> Result<(), &'static str> {
    tokio::time::timeout(LOGIN_TIMEOUT, async {
        let ldap = connect_bind(cfg, &cfg.bind_dn, &cfg.bind_password).await?;
        unbind(ldap).await;
        Ok(())
    })
    .await
    .map_err(|_| "LdapConnectTimedOut")?
}

fn attr_values<'a, T>(attrs: &'a std::collections::HashMap<String, Vec<T>>, name: &str) -> &'a [T] {
    attrs
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, values)| values.as_slice())
        .unwrap_or_default()
}

async fn search_user(
    cfg: &Settings,
    ldap: &mut Ldap,
    username: &str,
) -> Result<DirectoryUser, &'static str> {
    let user = &cfg.user;
    let username_attr = attribute(&user.username, "uid");
    let email_attr = attribute(&user.email, "mail");
    let first_attr = attribute(&user.first_name, "givenName");
    let last_attr = attribute(&user.last_name, "sn");
    let identity_attr = match user.identity_attribute {
        LdapIdentityAttribute::EntryUuid => "entryUUID",
        LdapIdentityAttribute::ObjectGuid => "objectGUID",
    };
    let filter = format!(
        "(&{}({}={}))",
        attribute(&user.filter, "(cn=*)"),
        username_attr,
        ldap3::ldap_escape(username)
    );
    let scope = match user.scope {
        LdapScope::Base => Scope::Base,
        LdapScope::One => Scope::OneLevel,
        LdapScope::Subtree => Scope::Subtree,
    };
    let mut attrs = vec![
        username_attr,
        email_attr,
        first_attr,
        last_attr,
        identity_attr,
    ];
    if !user.enable_attr.is_empty() {
        attrs.push(&user.enable_attr);
    }
    let (entries, _) = ldap
        .with_timeout(LDAP_TIMEOUT)
        .with_search_options(SearchOptions::new().sizelimit(2))
        .search(base_dn(cfg), scope, &filter, attrs)
        .await
        .map_err(|_| "LdapSearchFailed")?
        .success()
        .map_err(|_| "LdapSearchFailed")?;
    if entries.len() != 1 {
        return Err("UserNotFound");
    }
    let entry = SearchEntry::construct(entries.into_iter().next().ok_or("UserNotFound")?);
    let get = |name: &str| {
        attr_values(&entry.attrs, name)
            .first()
            .map(String::as_str)
            .unwrap_or_default()
    };
    let enabled = get(&user.enable_attr);
    if !user.enable_attr.is_empty() && !user.enable_attr_value.is_empty() {
        let active = if user.enable_attr.eq_ignore_ascii_case("userAccountControl") {
            enabled.parse::<i64>().is_ok_and(|value| value & 2 == 0)
        } else {
            enabled == user.enable_attr_value
        };
        if !active {
            return Err("UserDisabledAtLdap");
        }
    }
    let identity = match (
        attr_values(&entry.attrs, identity_attr),
        attr_values(&entry.bin_attrs, identity_attr),
    ) {
        ([id], []) => id.as_bytes(),
        ([], [id]) => id.as_slice(),
        _ => return Err("LdapMissingIdentity"),
    };
    let identity = match user.identity_attribute {
        LdapIdentityAttribute::EntryUuid => {
            let text = std::str::from_utf8(identity).map_err(|_| "LdapInvalidIdentity")?;
            *uuid::Uuid::parse_str(text)
                .map_err(|_| "LdapInvalidIdentity")?
                .as_bytes()
        }
        LdapIdentityAttribute::ObjectGuid => {
            identity.try_into().map_err(|_| "LdapInvalidIdentity")?
        }
    };
    if entry.dn.is_empty() || format_username(get(username_attr)) != format_username(username) {
        return Err("LdapInvalidIdentity");
    }
    let url = endpoint(cfg)?;
    let authority = (
        url.scheme(),
        url.host_str(),
        url.port()
            .unwrap_or(if url.scheme() == "ldaps" { 636 } else { 389 }),
        cfg.start_tls,
        identity_attr.to_ascii_lowercase(),
        identity,
    );
    let subject = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&authority).map_err(|_| "LdapInvalidIdentity")?)
    );
    Ok(DirectoryUser {
        dn: entry.dn.clone(),
        subject,
        username: format_username(get(username_attr)),
        email: get(email_attr).to_owned(),
        nickname: format!("{} {}", get(first_attr), get(last_attr))
            .trim()
            .to_owned(),
    })
}

async fn in_group(ldap: &mut Ldap, user: &DirectoryUser, group: &str) -> bool {
    if group.is_empty() {
        return false;
    }
    let membership = format!("(memberOf={})", ldap3::ldap_escape(group));
    if ldap.with_timeout(LDAP_TIMEOUT)
        .search(&user.dn, Scope::Base, &membership, vec!["1.1"])
        .await
        .and_then(|result| result.success())
        .is_ok_and(|(entries, _)| !entries.is_empty())
    {
        return true;
    }
    let filter = format!("(member={})", ldap3::ldap_escape(&user.dn));
    ldap.with_timeout(LDAP_TIMEOUT)
        .search(group, Scope::Base, &filter, vec!["1.1"])
        .await
        .and_then(|result| result.success())
        .is_ok_and(|(entries, _)| !entries.is_empty())
}

async fn verify_directory(
    cfg: &Settings,
    username: &str,
    password: &str,
) -> Result<DirectoryUser, &'static str> {
    if !cfg.enable || password.is_empty() {
        return Err("LdapAuthenticationUnavailable");
    }
    validate(cfg, true)?;
    let mut ldap = connect_bind(cfg, &cfg.bind_dn, &cfg.bind_password).await?;
    let user = search_user(cfg, &mut ldap, username).await?;
    if !cfg.user.allow_group.is_empty()
        && !in_group(&mut ldap, &user, &cfg.user.allow_group).await
        && !in_group(&mut ldap, &user, &cfg.user.admin_group).await
    {
        return Err("LdapGroupRequired");
    }
    unbind(ldap).await;
    let verify = connect_bind(cfg, &user.dn, password).await?;
    unbind(verify).await;
    Ok(user)
}

pub async fn is_directory_user(db: &DatabaseConnection, user_id: i32) -> Result<bool, DbErr> {
    Ok(ldap_identity::Entity::find_by_id(user_id)
        .one(db)
        .await?
        .is_some())
}

/// Resolve a verified LDAP identity. Existing unbound accounts are local-only,
/// including legacy directory profiles whose origin cannot be established.
pub async fn authenticate(
    cfg: &Settings,
    db: &DatabaseConnection,
    username: &str,
    password: &str,
) -> Result<Option<user::Model>, DbErr> {
    let Ok(Ok(directory)) =
        tokio::time::timeout(LOGIN_TIMEOUT, verify_directory(cfg, username, password)).await
    else {
        return Ok(None);
    };
    map_to_local_user(cfg, db, &directory).await
}

async fn map_to_local_user(
    cfg: &Settings,
    db: &DatabaseConnection,
    directory: &DirectoryUser,
) -> Result<Option<user::Model>, DbErr> {
    let existing = ldap_identity::Entity::find()
        .filter(ldap_identity::Column::Subject.eq(&directory.subject))
        .one(db)
        .await?;
    if let Some(identity) = existing {
        let Some(account) = user::Entity::find_by_id(identity.user_id).one(db).await? else {
            return Ok(None);
        };
        if !account.is_enabled() || account.username != directory.username {
            return Ok(None);
        }
        if !cfg.user.sync {
            return Ok(Some(account));
        }
        let mut update: user::ActiveModel = account.into();
        update.email = Set(directory.email.clone());
        update.nickname = Set(directory.nickname.clone());
        update.updated_at = Set(now());
        return update.update(db).await.map(Some);
    }
    let tx = db.begin().await?;
    if user::Entity::find()
        .filter(user::Column::Username.eq(&directory.username))
        .one(&tx)
        .await?
        .is_some()
    {
        return Ok(None);
    }
    let new_account = user::ActiveModel {
        username: Set(directory.username.clone()),
        email: Set(directory.email.clone()),
        nickname: Set(directory.nickname.clone()),
        is_admin: Set(Some(false)),
        group_id: Set(1),
        status: Set(user::STATUS_ENABLE),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    let account = super::login_security::insert_user_with_enrollment_policy(&tx, new_account).await?;
    ldap_identity::ActiveModel {
        user_id: Set(account.id),
        subject: Set(directory.subject.clone()),
    }
    .insert(&tx)
    .await?;
    tx.commit().await?;
    Ok(Some(account))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::services::{ldap_settings::tests::test_db, user::info_by_username_password};

    fn directory_user(username: &str, subject: &str) -> DirectoryUser {
        DirectoryUser {
            dn: format!("uid={username},dc=example"),
            username: username.to_owned(),
            subject: subject.to_owned(),
            email: format!("{username}@directory.test"),
            nickname: "Directory Display Name".to_owned(),
        }
    }

    #[tokio::test]
    async fn ldap_identity_mapping_keeps_local_and_directory_accounts_separate() {
        let db = test_db().await;
        let mut config = Config::default();
        config.ldap.enable = true;
        config.ldap.url = "ldap://127.0.0.1:1".to_owned();
        config.ldap.base_dn = "dc=example".to_owned();
        config.ldap.user.admin_group = "cn=directory-admins,dc=example".to_owned();
        config.ldap.user.sync = true;
        let local = user::ActiveModel {
            username: Set("local-admin".to_owned()),
            password: Set(crate::support::password::md5_hex(
                "local-passwordrustdesk-api",
            )),
            is_admin: Set(Some(true)),
            status: Set(user::STATUS_ENABLE),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let namesake = directory_user("local-admin", "other-identity");
        assert!(
            map_to_local_user(&config.ldap, &db, &namesake)
                .await
                .unwrap()
                .is_none()
        );
        let verified = info_by_username_password(&db, &config, "LOCAL-ADMIN", "local-password")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(verified.id, local.id);
        assert!(verified.is_admin());
        assert!(!is_directory_user(&db, local.id).await.unwrap());

        let directory = directory_user("directory-user", "directory-identity");
        let mapped = map_to_local_user(&config.ldap, &db, &directory)
            .await
            .unwrap()
            .unwrap();
        assert!(!mapped.is_admin());
        assert!(is_directory_user(&db, mapped.id).await.unwrap());
        assert!(
            map_to_local_user(
                &config.ldap,
                &db,
                &directory_user("directory-user", "new-identity")
            )
            .await
            .unwrap()
            .is_none()
        );
        let mut active: user::ActiveModel = mapped.into();
        active.password = Set(crate::support::password::md5_hex(
            "stale-passwordrustdesk-api",
        ));
        let mapped = active.update(&db).await.unwrap();
        assert!(
            info_by_username_password(&db, &config, "directory-user", "stale-password")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            map_to_local_user(&config.ldap, &db, &directory)
                .await
                .unwrap()
                .unwrap()
                .id,
            mapped.id
        );
    }

    #[tokio::test]
    #[ignore = "requires the loopback OpenLDAP server from tests/fixtures/ldap.sh"]
    async fn ldap_openldap_loopback() {
        let host = std::env::var("LDAP_TEST_PLAIN").unwrap();
        let secure = std::env::var("LDAP_TEST_SECURE").unwrap();
        let ca_file = std::env::var("LDAP_TEST_CA").unwrap();
        let db = test_db().await;
        let mut settings = Settings {
            url: host.clone(),
            bind_dn: "cn=fixture,dc=example,dc=test".to_owned(),
            bind_password: "fixture-service-password".to_owned(),
            base_dn: "ou=users,dc=example,dc=test".to_owned(),
            ..Default::default()
        };

        let before = user::Entity::find().count(&db).await.unwrap();
        let request = serde_json::from_value(serde_json::json!({})).unwrap();
        crate::services::ldap_settings::test(&db, &settings, request)
            .await
            .unwrap();
        assert_eq!(user::Entity::find().count(&db).await.unwrap(), before);
        assert!(
            ::entity::ldap_setting::Entity::find()
                .count(&db)
                .await
                .unwrap()
                == 0
        );

        settings.enable = true;
        settings.start_tls = true;
        settings.tls_verify = true;
        assert!(test_bind(&settings).await.is_err());
        settings.tls_ca_pem = std::fs::read_to_string(&ca_file).unwrap();
        test_bind(&settings).await.unwrap();
        settings.user.scope = LdapScope::One;
        let account = authenticate(&settings, &db, "directory-user", "fixture-user-password")
            .await
            .unwrap()
            .unwrap();
        assert!(!account.is_admin());
        assert!(is_directory_user(&db, account.id).await.unwrap());
        assert!(
            authenticate(&settings, &db, "directory-user", "incorrect")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            authenticate(&settings, &db, "directory-user", "")
                .await
                .unwrap()
                .is_none()
        );

        settings.url = secure;
        settings.start_tls = false;
        settings.tls_ca_pem.clear();
        settings.tls_ca_file = ca_file;
        settings.user.scope = LdapScope::Base;
        settings.user.base_dn = "uid=second-user,ou=users,dc=example,dc=test".to_owned();
        test_bind(&settings).await.unwrap();
        assert!(
            authenticate(&settings, &db, "second-user", "fixture-user-password")
                .await
                .unwrap()
                .is_some()
        );
    }
}

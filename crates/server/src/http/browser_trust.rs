use axum::http::{header, HeaderMap, HeaderValue};
use chrono::Utc;
use entity::user;
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::services::login_security::LoginSecuritySettings;
use crate::support::totp_vault::TotpVault;

const COOKIE: &str = "__Secure-rustdesk-console-trust";
const TRUST_SECONDS: i64 = 30 * 24 * 60 * 60;

pub(super) fn allowed(account: &user::Model, settings: &LoginSecuritySettings) -> bool {
    account.tfa_enabled
        && !account.must_change_password
        && settings.allow_trusted_login_devices
        && !settings.require_email_verification
        && !settings.require_device_verification
        && !account.email_verification_enabled
        && !account.login_device_verification_enabled
}

fn binding(account: &user::Model) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"rustdesk-console-browser-trust-v1");
    hash.update(account.id.to_be_bytes());
    hash.update([u8::from(account.is_admin())]);
    for value in [
        &account.username,
        &account.password,
        &account.tfa_secret,
        &account
            .created_at
            .map(|time| time.to_string())
            .unwrap_or_default(),
    ] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    hash.finalize().into()
}

pub(super) fn issue(config: &Config, account: &user::Model) -> anyhow::Result<HeaderValue> {
    let expires = Utc::now().timestamp() + TRUST_SECONDS;
    let sealed =
        TotpVault::load(config, false)?.seal_bound(&binding(account), &expires.to_string())?;
    Ok(HeaderValue::from_str(&format!(
        "{COOKIE}={sealed}; Max-Age={TRUST_SECONDS}; Path=/api/admin/login; Secure; HttpOnly; SameSite=Strict"
    ))?)
}

pub(super) fn expires_at(
    headers: &HeaderMap,
    config: &Config,
    account: &user::Model,
) -> Option<i64> {
    let mut values = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter_map(|(name, value)| (name == COOKIE).then_some(value));
    let value = values.next()?;
    if value.len() > 128 || values.next().is_some() {
        return None;
    }
    let vault = TotpVault::load(config, false).ok()?;
    let expiry = vault
        .open_bound(&binding(account), value)
        .ok()?
        .parse()
        .ok()?;
    (expiry > Utc::now().timestamp()).then_some(expiry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cookie_is_bound_to_the_account_and_expires() {
        use sea_orm::{ActiveModelTrait, ConnectionTrait, Database, Set};

        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.security.totp_key_file = temp.path().join("totp.key").display().to_string();
        let vault = TotpVault::load(&config, true).unwrap();
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let backend = db.get_database_backend();
        db.execute(
            backend.build(&sea_orm::Schema::new(backend).create_table_from_entity(user::Entity)),
        )
        .await
        .unwrap();
        let mut account = user::ActiveModel {
            username: Set("fixture".into()),
            password: Set("old-hash".into()),
            tfa_secret: Set("enrolled-secret".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let account_id = account.id;
        let as_headers = |cookie: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
            headers
        };
        let expired = vault
            .seal_bound(
                &binding(&account),
                &(Utc::now().timestamp() - 1).to_string(),
            )
            .unwrap();
        assert_eq!(
            expires_at(
                &as_headers(&format!("{COOKIE}={expired}")),
                &config,
                &account
            ),
            None
        );

        let cookie = issue(&config, &account).unwrap();
        let cookie = cookie.to_str().unwrap().split(';').next().unwrap();
        let headers = as_headers(cookie);
        assert!(expires_at(&headers, &config, &account).is_some());
        account.id += 1;
        assert_eq!(expires_at(&headers, &config, &account), None);
        account.id = account_id;
        account.password = "new-hash".into();
        assert_eq!(expires_at(&headers, &config, &account), None);
        account.password = "old-hash".into();
        account.tfa_secret = "re-enrolled".into();
        assert_eq!(expires_at(&headers, &config, &account), None);
    }
}

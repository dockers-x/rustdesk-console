use chrono::Utc;
use entity::{trusted_browser, user, user_token};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, Set, TransactionTrait,
};
use sha2::{Digest, Sha256};

use super::{
    login_security::LoginSecuritySettings,
    user::{lock_account, AUTH_TOTP},
};

pub(crate) const DAY_SECONDS: i64 = 24 * 60 * 60;

pub(crate) fn allowed(account: &user::Model, settings: &LoginSecuritySettings) -> bool {
    account.tfa_enabled
        && !account.must_change_password
        && settings.allow_trusted_login_devices
        && !settings.require_email_verification
        && !settings.require_device_verification
        && !account.email_verification_enabled
        && !account.login_device_verification_enabled
}

pub(crate) fn binding(account: &user::Model) -> String {
    let mut hash = Sha256::new();
    hash.update(b"rustdesk-console-browser-trust-v2");
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
    format!("{:x}", hash.finalize())
}

fn active(account: &user::Model) -> sea_orm::Select<trusted_browser::Entity> {
    trusted_browser::Entity::find()
        .filter(trusted_browser::Column::UserId.eq(account.id))
        .filter(trusted_browser::Column::AccountBinding.eq(binding(account)))
        .filter(trusted_browser::Column::ExpiresAt.gt(Utc::now().timestamp()))
}

pub(crate) async fn list(
    db: &DatabaseConnection,
    account: &user::Model,
) -> Result<Vec<trusted_browser::Model>, DbErr> {
    active(account)
        .order_by_desc(trusted_browser::Column::LastUsedAt)
        .order_by_asc(trusted_browser::Column::Id)
        .all(db)
        .await
}

pub(crate) async fn find(
    db: &impl ConnectionTrait,
    account: &user::Model,
    id: &str,
) -> Result<Option<trusted_browser::Model>, DbErr> {
    active(account)
        .filter(trusted_browser::Column::Id.eq(id))
        .one(db)
        .await
}

pub(crate) async fn mark_used(db: &impl ConnectionTrait, id: &str, now: i64) -> Result<(), DbErr> {
    trusted_browser::Entity::update_many()
        .col_expr(
            trusted_browser::Column::LastUsedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .filter(trusted_browser::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(())
}

pub(crate) struct Enrollment<'a> {
    pub id: String,
    pub previous: Option<&'a str>,
    pub browser: &'a str,
    pub platform: &'a str,
    pub seconds: i64,
}

pub(crate) async fn enroll(
    db: &DatabaseConnection,
    account: &user::Model,
    session_id: i32,
    grant: Enrollment<'_>,
) -> Result<(), DbErr> {
    let tx = db.begin().await?;
    let account_binding = binding(account);
    let current = lock_account(&tx, account.id)
        .await?
        .filter(|current| current.is_enabled() && binding(current) == account_binding)
        .ok_or_else(|| DbErr::Custom("Account changed during browser enrollment".into()))?;
    let settings = super::login_security::login_settings(&tx).await?;
    if !allowed(&current, &settings)
        || i64::from(settings.browser_trust_days) * DAY_SECONDS != grant.seconds
    {
        return Err(DbErr::Custom(
            "Browser trust policy changed during enrollment".into(),
        ));
    }
    let now = Utc::now().timestamp();
    let linked = user_token::Entity::update_many()
        .col_expr(
            user_token::Column::TrustedBrowserId,
            sea_orm::sea_query::Expr::value(grant.id.clone()),
        )
        .filter(user_token::Column::Id.eq(session_id))
        .filter(user_token::Column::UserId.eq(account.id))
        .filter(user_token::Column::AuthLevel.eq(AUTH_TOTP))
        .filter(user_token::Column::ExpiredAt.gt(now))
        .exec(&tx)
        .await?;
    if linked.rows_affected != 1 {
        return Err(DbErr::Custom("Verified session is no longer active".into()));
    }
    trusted_browser::Entity::delete_many()
        .filter(trusted_browser::Column::UserId.eq(account.id))
        .filter(trusted_browser::Column::ExpiresAt.lte(now))
        .exec(&tx)
        .await?;
    if let Some(previous) = grant.previous {
        if find(&tx, &current, previous).await?.is_some() {
            user_token::Entity::update_many()
                .col_expr(
                    user_token::Column::TrustedBrowserId,
                    sea_orm::sea_query::Expr::value(grant.id.clone()),
                )
                .filter(user_token::Column::UserId.eq(account.id))
                .filter(user_token::Column::TrustedBrowserId.eq(previous))
                .exec(&tx)
                .await?;
            trusted_browser::Entity::delete_by_id(previous)
                .exec(&tx)
                .await?;
        }
    }
    trusted_browser::ActiveModel {
        id: Set(grant.id),
        user_id: Set(account.id),
        account_binding: Set(account_binding),
        browser: Set(grant.browser.to_owned()),
        platform: Set(grant.platform.to_owned()),
        created_at: Set(now),
        last_used_at: Set(now),
        expires_at: Set(now + grant.seconds),
    }
    .insert(&tx)
    .await?;
    tx.commit().await
}

pub(crate) async fn revoke(
    db: &DatabaseConnection,
    user_id: i32,
    id: Option<&str>,
    end_sessions: bool,
) -> Result<(), DbErr> {
    let tx = db.begin().await?;
    lock_account(&tx, user_id)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("User".into()))?;
    let mut grants =
        trusted_browser::Entity::delete_many().filter(trusted_browser::Column::UserId.eq(user_id));
    if let Some(id) = id {
        grants = grants.filter(trusted_browser::Column::Id.eq(id));
    }
    grants.exec(&tx).await?;
    if end_sessions {
        let mut sessions =
            user_token::Entity::delete_many().filter(user_token::Column::UserId.eq(user_id));
        sessions = match id {
            Some(id) => sessions.filter(user_token::Column::TrustedBrowserId.eq(id)),
            None => sessions.filter(user_token::Column::TrustedBrowserId.is_not_null()),
        };
        sessions.exec(&tx).await?;
    }
    tx.commit().await
}

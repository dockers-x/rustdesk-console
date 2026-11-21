//! User service — ports `service/user.go` (the subset needed for Phase 1).

use chrono::Utc;
use sea_orm::*;

use ::entity::{login_log, login_verification, user, user_token};

use crate::config::Config;
use crate::services::{login_security, now, paginate};
use crate::support::jwt::Jwt;
use crate::support::password::{encrypt_password, verify_password};

pub async fn info_by_id(db: &DatabaseConnection, id: i32) -> Result<Option<user::Model>, DbErr> {
    user::Entity::find_by_id(id).one(db).await
}

pub async fn info_by_username(
    db: &DatabaseConnection,
    username: &str,
) -> Result<Option<user::Model>, DbErr> {
    user::Entity::find()
        .filter(user::Column::Username.eq(username))
        .one(db)
        .await
}

/// Select the password source before authentication. A directory account has
/// no local password fallback, and an existing local account never uses LDAP.
pub async fn info_by_username_password(
    db: &DatabaseConnection,
    cfg: &Config,
    username: &str,
    password: &str,
) -> Result<Option<user::Model>, DbErr> {
    let local = info_by_username(db, &format_username(username)).await?;
    let use_directory = match &local {
        Some(account) => crate::services::ldap::is_directory_user(db, account.id).await?,
        None => true,
    };
    if use_directory {
        let Ok(settings) = crate::services::ldap_settings::read(db, &cfg.ldap).await else {
            return Ok(None);
        };
        return crate::services::ldap::authenticate(&settings, db, username, password).await;
    }
    let Some(u) = local else {
        return Ok(None);
    };
    let hash = u.password.clone();
    let password = password.to_string();
    let (ok, new_hash) = tokio::task::spawn_blocking(move || verify_password(&hash, &password))
        .await
        .map_err(|e| DbErr::Custom(e.to_string()))?;
    if !ok {
        return Ok(None);
    }
    if let Some(nh) = new_hash {
        let upgraded = user::Entity::update_many()
            .col_expr(
                user::Column::Password,
                sea_orm::sea_query::Expr::value(nh.clone()),
            )
            .filter(user::Column::Id.eq(u.id))
            .filter(user::Column::Password.eq(&u.password))
            .exec(db)
            .await?;
        if upgraded.rows_affected != 1 {
            return Ok(None);
        }
        let mut u = u;
        u.password = nh;
        return Ok(Some(u));
    }
    Ok(Some(u))
}

/// Look up a user by access token (token must exist and be unexpired).
pub async fn info_by_access_token(
    db: &DatabaseConnection,
    token: &str,
) -> Result<Option<(user::Model, user_token::Model)>, DbErr> {
    let Some((account, session)) = session_by_token(db, token).await? else {
        return Ok(None);
    };
    if session.auth_level != AUTH_TOTP
        && (session.auth_level != 0 || requires_totp(db, &account).await?)
    {
        return Ok(None);
    }
    Ok(Some((account, session)))
}

pub async fn requires_totp(db: &impl ConnectionTrait, user: &user::Model) -> Result<bool, DbErr> {
    if user.is_admin() {
        return Ok(true);
    }
    console_requires_totp(db, user).await
}

pub async fn console_requires_totp(
    db: &impl ConnectionTrait,
    user: &user::Model,
) -> Result<bool, DbErr> {
    if user.tfa_enabled || user.tfa_enforced {
        return Ok(true);
    }
    Ok(login_security::login_settings(db).await?.require_totp)
}

pub const AUTH_TOTP: i32 = 1;
pub const AUTH_ENROLLMENT: i32 = 2;
pub const AUTH_CONSOLE_ADMIN: i32 = 3;
const ADMIN_SESSION_SECONDS: i64 = 12 * 60 * 60;
const ENROLLMENT_SECONDS: i64 = 10 * 60;

/// Looks up a live session before applying the permissions of the requested route.
pub async fn session_by_token(
    db: &DatabaseConnection,
    token: &str,
) -> Result<Option<(user::Model, user_token::Model)>, DbErr> {
    let Some(ut) = user_token::Entity::find()
        .filter(user_token::Column::Token.eq(token))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    if ut.expired_at <= Utc::now().timestamp() {
        return Ok(None);
    }
    let Some(u) = info_by_id(db, ut.user_id).await? else {
        return Ok(None);
    };
    if !u.is_enabled()
        || (ut.auth_level == AUTH_CONSOLE_ADMIN && (!u.is_admin() || u.tfa_enabled))
        || (ut.auth_level == AUTH_TOTP && !u.tfa_enabled)
        || (ut.auth_level == AUTH_ENROLLMENT && u.tfa_enabled)
        || (u.is_admin()
            && ut.created_at.is_none_or(|created| {
                Utc::now().timestamp() >= created.and_utc().timestamp() + ADMIN_SESSION_SECONDS
            }))
    {
        return Ok(None);
    }
    Ok(Some((u, ut)))
}

pub fn token_expire_timestamp(config: &Config) -> i64 {
    let secs = config.app.token_expire_duration().as_secs() as i64;
    Utc::now().timestamp() + secs
}

/// Generate an independently random session token, including when JWT is disabled.
pub fn generate_token(jwt: &Jwt, u: &user::Model) -> String {
    if jwt.has_key() {
        jwt.generate_token(u.id as u32)
    } else {
        uuid::Uuid::new_v4().to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginProof {
    SingleFactor,
    ConsoleAdministrator,
    Totp,
    Enrollment,
    WebSso { session_id: i32 },
}

impl From<login_security::VerificationKind> for LoginProof {
    fn from(kind: login_security::VerificationKind) -> Self {
        match kind {
            login_security::VerificationKind::Totp => Self::Totp,
            login_security::VerificationKind::Email => Self::SingleFactor,
        }
    }
}

/// Parameters for a login event (subset of `model.LoginLog`).
pub struct LoginEvent {
    pub proof: LoginProof,
    pub client: String,
    pub device_id: String,
    pub uuid: String,
    pub ip: String,
    pub login_type: String,
    pub platform: String,
}

/// Serialize authentication against writes to this account.
pub(super) async fn lock_account(
    transaction: &DatabaseTransaction,
    user_id: i32,
) -> Result<Option<user::Model>, DbErr> {
    user::Entity::update_many()
        .col_expr(
            user::Column::Id,
            sea_orm::sea_query::Expr::col(user::Column::Id).into(),
        )
        .filter(user::Column::Id.eq(user_id))
        .exec(transaction)
        .await?;
    user::Entity::find_by_id(user_id).one(transaction).await
}

/// Requires the account lock, then locks and revalidates the authorizing session.
pub(super) async fn lock_full_session(
    transaction: &DatabaseTransaction,
    account: &user::Model,
    session_id: i32,
) -> Result<(i32, i64), DbErr> {
    // Serialize authorization with concurrent logout and session revocation.
    user_token::Entity::update_many()
        .col_expr(
            user_token::Column::Id,
            sea_orm::sea_query::Expr::col(user_token::Column::Id).into(),
        )
        .filter(user_token::Column::Id.eq(session_id))
        .filter(user_token::Column::UserId.eq(account.id))
        .exec(transaction)
        .await?;
    let source = user_token::Entity::find_by_id(session_id)
        .one(transaction)
        .await?
        .filter(|session| {
            session.user_id == account.id
                && matches!(session.auth_level, 0 | AUTH_TOTP)
                && session.expired_at > Utc::now().timestamp()
        })
        .ok_or_else(|| DbErr::Custom("authorizing session expired".into()))?;
    if !account.is_enabled()
        || account.must_change_password
        || (requires_totp(transaction, account).await? && source.auth_level != AUTH_TOTP)
    {
        return Err(DbErr::Custom(
            "authorizing session is no longer valid".into(),
        ));
    }
    let mut expires = source.expired_at;
    if account.is_admin() {
        let created = source
            .created_at
            .ok_or_else(|| DbErr::Custom("authorizing session has no timestamp".into()))?;
        expires = expires.min(created.and_utc().timestamp() + ADMIN_SESSION_SECONDS);
    }
    if expires <= Utc::now().timestamp() {
        return Err(DbErr::Custom("authorizing session expired".into()));
    }
    Ok((source.auth_level, expires))
}

/// Create a token and login log. Client-reported identifiers never change ownership.
pub async fn login(
    db: &DatabaseConnection,
    jwt: &Jwt,
    config: &Config,
    u: &user::Model,
    event: LoginEvent,
) -> Result<user_token::Model, DbErr> {
    let transaction = db.begin().await?;
    let current = lock_account(&transaction, u.id)
        .await?
        .filter(|current| {
            current.is_enabled()
                && current.password == u.password
                && current.tfa_enabled == u.tfa_enabled
                && current.tfa_secret == u.tfa_secret
                && current.updated_at == u.updated_at
        })
        .ok_or_else(|| DbErr::Custom("account changed during authentication".into()))?;
    let (auth_level, source_expiry) = match event.proof {
        LoginProof::Totp if current.tfa_enabled => (AUTH_TOTP, None),
        LoginProof::Enrollment if !current.tfa_enabled => (AUTH_ENROLLMENT, None),
        LoginProof::SingleFactor if !requires_totp(&transaction, &current).await? => (0, None),
        LoginProof::ConsoleAdministrator
            if current.is_admin()
                && event.client == login_log::CLIENT_WEB_ADMIN
                && !console_requires_totp(&transaction, &current).await? =>
        {
            (AUTH_CONSOLE_ADMIN, None)
        }
        LoginProof::WebSso { session_id } => {
            let (auth_level, expires) =
                lock_full_session(&transaction, &current, session_id).await?;
            (auth_level, Some(expires))
        }
        _ => return Err(DbErr::Custom("TOTP is required".into())),
    };
    let now_ts = Utc::now().timestamp();
    let mut expires = token_expire_timestamp(config);
    if auth_level == AUTH_ENROLLMENT {
        expires = expires.min(now_ts + ENROLLMENT_SECONDS);
    } else if current.is_admin() {
        expires = expires.min(now_ts + ADMIN_SESSION_SECONDS);
    }
    if let Some(source_expiry) = source_expiry {
        expires = expires.min(source_expiry);
    }
    let token = if auth_level == AUTH_CONSOLE_ADMIN {
        // Only the Console accepts this token. Do not emit a native JWT.
        uuid::Uuid::new_v4().to_string()
    } else {
        generate_token(jwt, &current)
    };
    let ut = user_token::ActiveModel {
        user_id: Set(u.id),
        token: Set(token),
        device_uuid: Set(event.uuid.clone()),
        device_id: Set(event.device_id.clone()),
        expired_at: Set(expires),
        auth_level: Set(auth_level),
        renewable: Set(source_expiry.is_none()),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    let ut = ut.insert(&transaction).await?;

    let llog = login_log::ActiveModel {
        user_id: Set(u.id),
        client: Set(event.client),
        device_id: Set(event.device_id),
        uuid: Set(event.uuid),
        ip: Set(event.ip),
        r#type: Set(event.login_type),
        platform: Set(event.platform),
        user_token_id: Set(ut.id),
        is_deleted: Set(0),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    llog.insert(&transaction).await?;
    transaction.commit().await?;

    Ok(ut)
}

/// Delete this session without changing durable device ownership.
pub async fn logout(db: &DatabaseConnection, user_id: i32, token: &str) -> Result<(), DbErr> {
    user_token::Entity::delete_many()
        .filter(user_token::Column::UserId.eq(user_id))
        .filter(user_token::Column::Token.eq(token))
        .exec(db)
        .await?;
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub enum SessionLifetime {
    Admin,
    User,
}

/// Refresh the token when it has less than ~1/3 of its lifetime remaining.
pub async fn auto_refresh_access_token(
    db: &DatabaseConnection,
    config: &Config,
    ut: &user_token::Model,
    lifetime: SessionLifetime,
) -> Result<(), DbErr> {
    if ut.auth_level == AUTH_ENROLLMENT || !ut.renewable {
        return Ok(());
    }
    let third = (config.app.token_expire_duration().as_secs() / 3) as i64;
    if ut.expired_at - Utc::now().timestamp() < third {
        let mut expires = token_expire_timestamp(config);
        if let SessionLifetime::Admin = lifetime {
            let Some(created) = ut.created_at else {
                return Ok(());
            };
            expires = expires.min(created.and_utc().timestamp() + ADMIN_SESSION_SECONDS);
        }
        if expires > ut.expired_at {
            let mut am: user_token::ActiveModel = ut.clone().into();
            am.expired_at = Set(expires);
            am.update(db).await?;
        }
    }
    Ok(())
}

// --- admin CRUD ---

pub struct UserListResult {
    pub list: Vec<user::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    username_like: Option<String>,
) -> Result<UserListResult, DbErr> {
    list_filtered(db, page, page_size, username_like, None).await
}

pub async fn list_filtered(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    username_like: Option<String>,
    group_id: Option<i32>,
) -> Result<UserListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = user::Entity::find();
    if let Some(name) = username_like.filter(|s| !s.is_empty()) {
        q = q.filter(user::Column::Username.contains(&name));
    }
    if let Some(group_id) = group_id.filter(|id| *id > 0) {
        q = q.filter(user::Column::GroupId.eq(group_id));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(UserListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn list_by_group_id(
    db: &DatabaseConnection,
    group_id: i32,
    page: u64,
    page_size: u64,
) -> Result<UserListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let q = user::Entity::find().filter(user::Column::GroupId.eq(group_id));
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(UserListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn list_id_and_name_by_group_id(
    db: &DatabaseConnection,
    group_id: i32,
) -> Result<Vec<user::Model>, DbErr> {
    user::Entity::find()
        .filter(user::Column::GroupId.eq(group_id))
        .all(db)
        .await
}

pub async fn list_by_ids(db: &DatabaseConnection, ids: &[i32]) -> Result<Vec<user::Model>, DbErr> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    user::Entity::find()
        .filter(user::Column::Id.is_in(ids.to_vec()))
        .all(db)
        .await
}

pub(crate) fn format_username(username: &str) -> String {
    username.replace(' ', "").to_lowercase()
}

pub async fn is_username_exists(db: &DatabaseConnection, username: &str) -> Result<bool, DbErr> {
    Ok(info_by_username(db, username).await?.is_some())
}

pub async fn admin_user_count(db: &DatabaseConnection) -> Result<u64, DbErr> {
    user::Entity::find()
        .filter(user::Column::IsAdmin.eq(true))
        .count(db)
        .await
}

pub async fn has_admin(db: &DatabaseConnection) -> Result<bool, DbErr> {
    Ok(admin_user_count(db).await? > 0)
}

/// Create a user (formats + dedups username, bcrypt-hashes the password).
pub async fn create(
    db: &DatabaseConnection,
    mut model: user::Model,
) -> Result<user::Model, String> {
    if is_username_exists(db, &model.username)
        .await
        .map_err(|e| e.to_string())?
    {
        return Err("UsernameExists".to_string());
    }
    model.username = format_username(&model.username);
    let password = std::mem::take(&mut model.password);
    model.password = tokio::task::spawn_blocking(move || encrypt_password(&password))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let am = user::ActiveModel {
        username: Set(model.username),
        email: Set(model.email),
        password: Set(model.password),
        nickname: Set(model.nickname),
        avatar: Set(model.avatar),
        group_id: Set(if model.group_id == 0 {
            1
        } else {
            model.group_id
        }),
        is_admin: Set(model.is_admin),
        status: Set(if model.status == 0 {
            user::STATUS_ENABLE
        } else {
            model.status
        }),
        must_change_password: Set(model.must_change_password),
        tfa_secret: Set(model.tfa_secret),
        tfa_enabled: Set(model.tfa_enabled),
        tfa_enforced: Set(model.tfa_enforced),
        email_verification_enabled: Set(model.email_verification_enabled),
        login_device_verification_enabled: Set(model.login_device_verification_enabled),
        remark: Set(model.remark),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    login_security::insert_user_with_enrollment_policy(db, am)
        .await
        .map_err(|e| e.to_string())
}

pub async fn update(db: &DatabaseConnection, model: &user::Model) -> Result<(), String> {
    let transaction = super::console_roles::authority_transaction(db)
        .await
        .map_err(|e| e.to_string())?;
    if (!model.is_admin() || !model.is_enabled())
        && is_last_enabled_admin(&transaction, model.id).await?
    {
        return Err("The last admin user cannot be disabled or demoted".to_owned());
    }
    let am = user::ActiveModel {
        id: Set(model.id),
        username: Set(model.username.clone()),
        email: Set(model.email.clone()),
        nickname: Set(model.nickname.clone()),
        avatar: Set(model.avatar.clone()),
        group_id: Set(model.group_id),
        is_admin: Set(model.is_admin),
        status: Set(model.status),
        remark: Set(model.remark.clone()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.update(&transaction).await.map_err(|e| e.to_string())?;
    if model.is_admin() {
        super::console_roles::clear(&transaction, model.id)
            .await
            .map_err(|e| e.to_string())?;
    }
    transaction.commit().await.map_err(|e| e.to_string())
}

async fn is_last_enabled_admin(db: &DatabaseTransaction, id: i32) -> Result<bool, String> {
    let admins = user::Entity::find()
        .filter(user::Column::IsAdmin.eq(true))
        .filter(user::Column::Status.eq(user::STATUS_ENABLE));
    let target_is_enabled_admin = admins
        .clone()
        .filter(user::Column::Id.eq(id))
        .one(db)
        .await
        .map_err(|err| err.to_string())?
        .is_some();
    Ok(target_is_enabled_admin
        && admins
            .filter(user::Column::Id.ne(id))
            .one(db)
            .await
            .map_err(|err| err.to_string())?
            .is_none())
}

pub async fn update_avatar(db: &DatabaseConnection, id: i32, avatar: &str) -> Result<(), String> {
    let am = user::ActiveModel {
        id: Set(id),
        avatar: Set(avatar.trim().to_string()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.update(db).await.map(|_| ()).map_err(|e| e.to_string())
}

pub async fn delete(db: &DatabaseConnection, u: &user::Model) -> Result<(), String> {
    use super::address_book::{self, ShareRecipient};

    let transaction = super::console_roles::authority_transaction(db)
        .await
        .map_err(|e| e.to_string())?;
    if is_last_enabled_admin(&transaction, u.id).await? {
        return Err("The last admin user cannot be deleted".to_owned());
    }
    let affected = address_book::collections_affected_by_recipient_deletion(
        &transaction,
        ShareRecipient::User(u.id),
    )
    .await
    .map_err(|e| e.to_string())?;
    user::Entity::delete_by_id(u.id)
        .exec(&transaction)
        .await
        .map_err(|e| e.to_string())?;
    for collection_id in affected {
        address_book::clear_unshared_delegation(&transaction, collection_id)
            .await
            .map_err(|e| e.to_string())?;
    }
    super::console_roles::clear(&transaction, u.id)
        .await
        .map_err(|e| e.to_string())?;
    transaction.commit().await.map_err(|e| e.to_string())?;
    // Unlink related peers (best-effort).
    let _ = crate::services::peer::erase_user_id(db, u.id).await;
    Ok(())
}

pub async fn update_password(
    db: &DatabaseConnection,
    u: &user::Model,
    password: &str,
) -> Result<(), String> {
    let password = password.to_string();
    let hash = tokio::task::spawn_blocking(move || encrypt_password(&password))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let transaction = db.begin().await.map_err(|e| e.to_string())?;
    let updated = user::Entity::update_many()
        .col_expr(
            user::Column::Password,
            sea_orm::sea_query::Expr::value(hash),
        )
        .col_expr(
            user::Column::MustChangePassword,
            sea_orm::sea_query::Expr::value(false),
        )
        .col_expr(
            user::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now()),
        )
        .filter(user::Column::Id.eq(u.id))
        .filter(user::Column::Password.eq(&u.password))
        .filter(user::Column::Email.eq(&u.email))
        .filter(user::Column::Status.eq(u.status))
        .filter(match u.is_admin {
            Some(is_admin) => user::Column::IsAdmin.eq(is_admin),
            None => user::Column::IsAdmin.is_null(),
        })
        .filter(match u.updated_at {
            Some(timestamp) => user::Column::UpdatedAt.eq(timestamp),
            None => user::Column::UpdatedAt.is_null(),
        })
        .exec(&transaction)
        .await
        .map_err(|e| e.to_string())?;
    if updated.rows_affected != 1 {
        return Err("account credentials have changed".into());
    }
    revoke_credentials(&transaction, u.id)
        .await
        .map_err(|e| e.to_string())?;
    transaction.commit().await.map_err(|e| e.to_string())
}

/// Invalidate sessions and outstanding challenges in the credential update transaction.
pub async fn revoke_credentials(db: &DatabaseTransaction, user_id: i32) -> Result<(), DbErr> {
    user_token::Entity::delete_many()
        .filter(user_token::Column::UserId.eq(user_id))
        .exec(db)
        .await?;
    login_verification::Entity::delete_many()
        .filter(login_verification::Column::UserId.eq(user_id))
        .exec(db)
        .await?;
    Ok(())
}

pub fn is_password_empty(u: &user::Model) -> bool {
    u.password.is_empty()
}

pub async fn flush_token_by_uuid(db: &DatabaseConnection, uuid: &str) -> Result<(), DbErr> {
    user_token::Entity::delete_many()
        .filter(user_token::Column::DeviceUuid.eq(uuid))
        .exec(db)
        .await?;
    Ok(())
}

pub async fn delete_tokens_by_user_ids(db: &DatabaseConnection, ids: &[i32]) -> Result<(), DbErr> {
    if ids.is_empty() {
        return Ok(());
    }
    user_token::Entity::delete_many()
        .filter(user_token::Column::UserId.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

// --- user tokens (admin) ---

pub struct UserTokenListResult {
    pub list: Vec<user_token::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn token_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    user_id: Option<i32>,
) -> Result<UserTokenListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = user_token::Entity::find();
    if let Some(uid) = user_id.filter(|v| *v > 0) {
        q = q.filter(user_token::Column::UserId.eq(uid));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(user_token::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(UserTokenListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn token_info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<user_token::Model>, DbErr> {
    user_token::Entity::find_by_id(id).one(db).await
}

pub async fn delete_token(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    user_token::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn batch_delete_user_token(db: &DatabaseConnection, ids: &[i32]) -> Result<(), DbErr> {
    user_token::Entity::delete_many()
        .filter(user_token::Column::Id.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

/// Register a user (≈ `UserService.Register`); returns None on failure.
pub async fn register(
    db: &DatabaseConnection,
    username: &str,
    email: &str,
    password: &str,
    status: i32,
) -> Option<user::Model> {
    let model = user::Model {
        id: 0,
        username: username.to_string(),
        email: email.to_string(),
        password: password.to_string(),
        nickname: String::new(),
        avatar: String::new(),
        group_id: 1,
        is_admin: Some(false),
        status,
        must_change_password: false,
        tfa_secret: String::new(),
        tfa_enabled: false,
        tfa_enforced: false,
        email_verification_enabled: false,
        login_device_verification_enabled: false,
        remark: String::new(),
        created_at: None,
        updated_at: None,
    };
    create(db, model).await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn login_log_references_the_created_token() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for statement in [
            schema.create_table_from_entity(user::Entity),
            schema.create_table_from_entity(user_token::Entity),
            schema.create_table_from_entity(login_log::Entity),
            schema.create_table_from_entity(::entity::system_setting::Entity),
        ] {
            db.execute(db.get_database_backend().build(&statement))
                .await
                .unwrap();
        }
        let account = user::ActiveModel {
            id: Set(42),
            username: Set("login-test".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let event = LoginEvent {
            proof: LoginProof::SingleFactor,
            client: login_log::CLIENT_APP.to_string(),
            device_id: String::new(),
            uuid: String::new(),
            ip: "127.0.0.1".to_string(),
            login_type: login_log::TYPE_ACCOUNT.to_string(),
            platform: "Linux".to_string(),
        };
        let config = Config::default();
        let jwt = Jwt::new("", config.app.token_expire_duration());
        let token = login(&db, &jwt, &config, &account, event).await.unwrap();
        let log = login_log::Entity::find().one(&db).await.unwrap().unwrap();
        assert_ne!(account.id, token.id);
        assert_eq!(log.user_id, account.id);
        assert_eq!(log.user_token_id, token.id);

        db.execute_unprepared("DROP TABLE login_logs")
            .await
            .unwrap();
        let rejected = login(
            &db,
            &jwt,
            &config,
            &account,
            LoginEvent {
                proof: LoginProof::SingleFactor,
                client: login_log::CLIENT_APP.to_owned(),
                device_id: String::new(),
                uuid: String::new(),
                ip: "127.0.0.1".to_owned(),
                login_type: login_log::TYPE_ACCOUNT.to_owned(),
                platform: "Linux".to_owned(),
            },
        )
        .await;
        assert!(rejected.is_err());
        assert_eq!(user_token::Entity::find().count(&db).await.unwrap(), 1);
    }
}

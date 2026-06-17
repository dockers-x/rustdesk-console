//! User service — ports `service/user.go` (the subset needed for Phase 1).

use chrono::Utc;
use sea_orm::*;

use ::entity::{login_log, user, user_token};

use crate::config::Config;
use crate::services::{now, paginate};
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

/// Verify username/password. If LDAP is enabled it is tried first, falling back
/// to the local database (with legacy MD5 upgrade). Returns the user on success.
pub async fn info_by_username_password(
    db: &DatabaseConnection,
    cfg: &Config,
    username: &str,
    password: &str,
) -> Result<Option<user::Model>, DbErr> {
    if cfg.ldap.enable {
        match crate::services::ldap::authenticate(cfg, db, username, password).await {
            Ok(u) => return Ok(Some(u)),
            Err(e) => {
                tracing::warn!("LDAP authentication failed: {e}; falling back to local database");
            }
        }
    }
    let Some(u) = info_by_username(db, username).await? else {
        return Ok(None);
    };
    let (ok, new_hash) = verify_password(&u.password, password);
    if !ok {
        return Ok(None);
    }
    if let Some(nh) = new_hash {
        let mut am: user::ActiveModel = u.clone().into();
        am.password = Set(nh.clone());
        am.update(db).await?;
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
    let Some(ut) = user_token::Entity::find()
        .filter(user_token::Column::Token.eq(token))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    if ut.expired_at < Utc::now().timestamp() {
        return Ok(None);
    }
    let Some(u) = info_by_id(db, ut.user_id).await? else {
        return Ok(None);
    };
    Ok(Some((u, ut)))
}

pub fn token_expire_timestamp(config: &Config) -> i64 {
    let secs = config.app.token_expire_duration().as_secs() as i64;
    Utc::now().timestamp() + secs
}

/// Generate a token: JWT when a key is configured, else md5(username+now).
pub fn generate_token(jwt: &Jwt, u: &user::Model) -> String {
    if jwt.has_key() {
        jwt.generate_token(u.id as u32)
    } else {
        crate::support::password::md5_hex(&format!("{}{}", u.username, Utc::now()))
    }
}

/// Parameters for a login event (subset of `model.LoginLog`).
pub struct LoginEvent {
    pub client: String,
    pub device_id: String,
    pub uuid: String,
    pub ip: String,
    pub login_type: String,
    pub platform: String,
}

/// Create a token + login log and bind the device uuid (≈ `UserService.Login`).
pub async fn login(
    db: &DatabaseConnection,
    jwt: &Jwt,
    config: &Config,
    u: &user::Model,
    event: LoginEvent,
) -> Result<user_token::Model, DbErr> {
    let token = generate_token(jwt, u);
    let ut = user_token::ActiveModel {
        user_id: Set(u.id),
        token: Set(token),
        device_uuid: Set(event.uuid.clone()),
        device_id: Set(event.device_id.clone()),
        expired_at: Set(token_expire_timestamp(config)),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    let ut = ut.insert(db).await?;

    let llog = login_log::ActiveModel {
        user_id: Set(u.id),
        client: Set(event.client),
        device_id: Set(event.device_id),
        uuid: Set(event.uuid.clone()),
        ip: Set(event.ip),
        r#type: Set(event.login_type),
        platform: Set(event.platform),
        user_token_id: Set(u.id),
        is_deleted: Set(0),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    llog.insert(db).await?;

    if !event.uuid.is_empty() {
        crate::services::peer::uuid_bind_user_id(db, &event.uuid, u.id).await?;
    }
    Ok(ut)
}

/// Delete the token for (user, token) and unbind its uuid.
pub async fn logout(db: &DatabaseConnection, user_id: i32, token: &str) -> Result<(), DbErr> {
    let ut = user_token::Entity::find()
        .filter(user_token::Column::UserId.eq(user_id))
        .filter(user_token::Column::Token.eq(token))
        .one(db)
        .await?;
    let uuid = ut
        .as_ref()
        .map(|t| t.device_uuid.clone())
        .unwrap_or_default();
    user_token::Entity::delete_many()
        .filter(user_token::Column::UserId.eq(user_id))
        .filter(user_token::Column::Token.eq(token))
        .exec(db)
        .await?;
    if !uuid.is_empty() {
        crate::services::peer::uuid_unbind_user_id(db, &uuid, user_id).await?;
    }
    Ok(())
}

/// Refresh the token when it has less than ~1/3 of its lifetime remaining.
pub async fn auto_refresh_access_token(
    db: &DatabaseConnection,
    config: &Config,
    ut: &user_token::Model,
) -> Result<(), DbErr> {
    let third = (config.app.token_expire_duration().as_secs() / 3) as i64;
    if ut.expired_at - Utc::now().timestamp() < third {
        let mut am: user_token::ActiveModel = ut.clone().into();
        am.expired_at = Set(token_expire_timestamp(config));
        am.update(db).await?;
    }
    Ok(())
}

pub async fn find_latest_user_id_by_uuid(
    db: &DatabaseConnection,
    uuid: &str,
    device_id: &str,
) -> Result<i32, DbErr> {
    let llog = login_log::Entity::find()
        .filter(login_log::Column::Uuid.eq(uuid))
        .filter(login_log::Column::DeviceId.eq(device_id))
        .order_by_desc(login_log::Column::Id)
        .one(db)
        .await?;
    Ok(llog.map(|l| l.user_id).unwrap_or(0))
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
    let (page, page_size) = paginate(page, page_size);
    let mut q = user::Entity::find();
    if let Some(name) = username_like.filter(|s| !s.is_empty()) {
        q = q.filter(user::Column::Username.contains(&name));
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

fn format_username(username: &str) -> String {
    username.replace(' ', "").to_lowercase()
}

pub async fn is_username_exists(db: &DatabaseConnection, username: &str) -> Result<bool, DbErr> {
    Ok(info_by_username(db, username).await?.is_some())
}

async fn admin_user_count(db: &DatabaseConnection) -> Result<u64, DbErr> {
    user::Entity::find()
        .filter(user::Column::IsAdmin.eq(true))
        .count(db)
        .await
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
    model.password = encrypt_password(&model.password).map_err(|e| e.to_string())?;
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
        remark: Set(model.remark),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await.map_err(|e| e.to_string())
}

pub async fn update(db: &DatabaseConnection, model: &user::Model) -> Result<(), String> {
    let current = info_by_id(db, model.id).await.map_err(|e| e.to_string())?;
    let must_change_password = current
        .as_ref()
        .map(|u| u.must_change_password)
        .unwrap_or(model.must_change_password);
    if let Some(current) = &current {
        if current.is_admin() {
            let count = admin_user_count(db).await.map_err(|e| e.to_string())?;
            if count <= 1 && (!model.is_admin() || model.status == user::STATUS_DISABLED) {
                return Err("The last admin user cannot be disabled or demoted".to_string());
            }
        }
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
        must_change_password: Set(must_change_password),
        remark: Set(model.remark.clone()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.update(db).await.map(|_| ()).map_err(|e| e.to_string())
}

pub async fn delete(db: &DatabaseConnection, u: &user::Model) -> Result<(), String> {
    let count = admin_user_count(db).await.map_err(|e| e.to_string())?;
    if count <= 1 && u.is_admin() {
        return Err("The last admin user cannot be deleted".to_string());
    }
    user::Entity::delete_by_id(u.id)
        .exec(db)
        .await
        .map_err(|e| e.to_string())?;
    // Unlink related peers (best-effort).
    let _ = crate::services::peer::erase_user_id(db, u.id).await;
    Ok(())
}

pub async fn update_password(
    db: &DatabaseConnection,
    u: &user::Model,
    password: &str,
) -> Result<(), String> {
    let hash = encrypt_password(password).map_err(|e| e.to_string())?;
    let mut am: user::ActiveModel = u.clone().into();
    am.password = Set(hash);
    am.must_change_password = Set(false);
    am.updated_at = Set(now());
    am.update(db).await.map_err(|e| e.to_string())?;
    // Invalidate existing tokens.
    user_token::Entity::delete_many()
        .filter(user_token::Column::UserId.eq(u.id))
        .exec(db)
        .await
        .map_err(|e| e.to_string())?;
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
        remark: String::new(),
        created_at: None,
        updated_at: None,
    };
    create(db, model).await.ok()
}

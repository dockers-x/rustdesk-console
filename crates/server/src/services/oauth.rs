//! OAuth service — ports the management side of `service/oauth.go`:
//! provider CRUD, third-party bindings, and provider listing. The external
//! login/token-exchange flow lives in a later sub-phase.

use sea_orm::*;

use ::entity::{oauth, user_third};

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
        oauth::TYPE_GITHUB | oauth::TYPE_GOOGLE | oauth::TYPE_OIDC | oauth::TYPE_WEBAUTH
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

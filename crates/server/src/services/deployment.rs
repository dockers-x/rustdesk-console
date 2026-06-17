use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use sea_orm::sea_query::Expr;
use sea_orm::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use ::entity::{deployment_event, deployment_token};

use crate::services::{now, paginate};

#[derive(Debug, Clone)]
pub struct CreateTokenInput {
    pub name: String,
    pub scopes: Vec<String>,
    pub default_user_id: i32,
    pub default_device_group_id: i32,
    pub default_strategy_id: i32,
    pub expires_at: i64,
    pub max_uses: i32,
    pub created_by: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatedToken {
    pub token: String,
    pub row: deployment_token::Model,
}

pub struct DeploymentTokenListResult {
    pub list: Vec<deployment_token::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list_tokens(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    name: Option<String>,
) -> Result<DeploymentTokenListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = deployment_token::Entity::find();
    if let Some(v) = name.filter(|s| !s.trim().is_empty()) {
        q = q.filter(deployment_token::Column::Name.contains(v.trim()));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(deployment_token::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(DeploymentTokenListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<deployment_token::Model>, DbErr> {
    deployment_token::Entity::find_by_id(id).one(db).await
}

pub async fn create_token(
    db: &DatabaseConnection,
    input: CreateTokenInput,
) -> Result<CreatedToken, DbErr> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(DbErr::Custom(
            "deployment token name is required".to_string(),
        ));
    }
    let scopes = normalize_scopes(input.scopes);
    if scopes.is_empty() {
        return Err(DbErr::Custom(
            "deployment token requires at least one scope".to_string(),
        ));
    }
    let token = generate_token();
    let am = deployment_token::ActiveModel {
        token_hash: Set(hash_token(&token)),
        name: Set(name.to_string()),
        scopes: Set(encode_scopes(&scopes)?),
        default_user_id: Set(input.default_user_id.max(0)),
        default_device_group_id: Set(input.default_device_group_id.max(0)),
        default_strategy_id: Set(input.default_strategy_id.max(0)),
        expires_at: Set(input.expires_at.max(0)),
        max_uses: Set(input.max_uses.max(0)),
        used_count: Set(0),
        revoked_at: Set(0),
        created_by: Set(input.created_by.max(0)),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    let row = am.insert(db).await?;
    Ok(CreatedToken { token, row })
}

pub async fn delete_token(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    deployment_token::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn revoke_token(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    deployment_token::Entity::update_many()
        .col_expr(
            deployment_token::Column::RevokedAt,
            Expr::value(chrono::Utc::now().timestamp()),
        )
        .col_expr(
            deployment_token::Column::UpdatedAt,
            Expr::value(chrono::Utc::now().naive_utc()),
        )
        .filter(deployment_token::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(())
}

pub async fn verify_token(
    db: &DatabaseConnection,
    token: &str,
) -> Result<Option<deployment_token::Model>, DbErr> {
    let token = token.trim();
    if token.is_empty() {
        return Ok(None);
    }
    let Some(row) = deployment_token::Entity::find()
        .filter(deployment_token::Column::TokenHash.eq(hash_token(token)))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let now_ts = chrono::Utc::now().timestamp();
    if row.is_revoked() || row.is_expired(now_ts) || row.is_exhausted() {
        return Ok(None);
    }
    Ok(Some(row))
}

pub async fn increment_used(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    deployment_token::Entity::update_many()
        .col_expr(
            deployment_token::Column::UsedCount,
            Expr::col(deployment_token::Column::UsedCount).add(1),
        )
        .col_expr(
            deployment_token::Column::UpdatedAt,
            Expr::value(chrono::Utc::now().naive_utc()),
        )
        .filter(deployment_token::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(())
}

pub fn has_scope(row: &deployment_token::Model, scope: &str) -> bool {
    decode_scopes(&row.scopes)
        .map(|scopes| scopes.iter().any(|s| s == scope))
        .unwrap_or(false)
}

pub async fn record_event(
    db: &DatabaseConnection,
    token_id: i32,
    peer_id: &str,
    uuid: &str,
    action: &str,
    result: &str,
    message: &str,
    ip: &str,
) -> Result<(), DbErr> {
    let am = deployment_event::ActiveModel {
        token_id: Set(token_id),
        peer_id: Set(peer_id.to_string()),
        uuid: Set(uuid.to_string()),
        action: Set(action.to_string()),
        result: Set(result.to_string()),
        message: Set(message.to_string()),
        ip: Set(ip.to_string()),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await?;
    Ok(())
}

pub fn default_scopes() -> Vec<String> {
    [
        deployment_token::SCOPE_DEPLOY,
        deployment_token::SCOPE_ASSIGN,
        deployment_token::SCOPE_STRATEGY_ASSIGN,
        deployment_token::SCOPE_ADDRESS_BOOK_ASSIGN,
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect()
}

pub fn decode_scopes(raw: &str) -> Result<Vec<String>, DbErr> {
    if raw.trim().is_empty() {
        return Ok(vec![]);
    }
    serde_json::from_str(raw).map_err(|e| DbErr::Custom(e.to_string()))
}

fn encode_scopes(scopes: &[String]) -> Result<String, DbErr> {
    serde_json::to_string(scopes).map_err(|e| DbErr::Custom(e.to_string()))
}

fn normalize_scopes(scopes: Vec<String>) -> Vec<String> {
    let mut normalized = vec![];
    for scope in scopes {
        let scope = scope.trim();
        let scope = match scope {
            deployment_token::SCOPE_DEPLOY => deployment_token::SCOPE_DEPLOY,
            deployment_token::SCOPE_ASSIGN => deployment_token::SCOPE_ASSIGN,
            deployment_token::SCOPE_STRATEGY_ASSIGN => deployment_token::SCOPE_STRATEGY_ASSIGN,
            deployment_token::SCOPE_ADDRESS_BOOK_ASSIGN => {
                deployment_token::SCOPE_ADDRESS_BOOK_ASSIGN
            }
            _ => continue,
        };
        if !normalized.iter().any(|s| s == scope) {
            normalized.push(scope.to_string());
        }
    }
    normalized
}

fn generate_token() -> String {
    let suffix: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(40)
        .map(char::from)
        .collect();
    format!("rdt_{suffix}")
}

fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    to_hex(&digest)
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[derive(Debug, Deserialize)]
pub struct ScopesForm {
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hash_is_stable_hex_sha256() {
        assert_eq!(hash_token("abc").len(), 64);
        assert_eq!(hash_token("abc"), hash_token("abc"));
        assert_ne!(hash_token("abc"), hash_token("abcd"));
    }

    #[test]
    fn scopes_are_allowlisted() {
        let scopes = normalize_scopes(vec![
            "deploy".to_string(),
            "unknown".to_string(),
            "deploy".to_string(),
        ]);
        assert_eq!(scopes, vec!["deploy".to_string()]);
    }
}

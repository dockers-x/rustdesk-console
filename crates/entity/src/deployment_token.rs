use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub const SCOPE_DEPLOY: &str = "deploy";
pub const SCOPE_ASSIGN: &str = "assign";
pub const SCOPE_STRATEGY_ASSIGN: &str = "strategy_assign";
pub const SCOPE_ADDRESS_BOOK_ASSIGN: &str = "address_book_assign";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "deployment_tokens")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique_key, default_value = "")]
    #[serde(skip_serializing)]
    pub token_hash: String,
    #[sea_orm(default_value = "")]
    pub name: String,
    #[sea_orm(default_value = "[]")]
    pub scopes: String,
    #[sea_orm(default_value = 0)]
    pub default_user_id: i32,
    #[sea_orm(default_value = 0)]
    pub default_device_group_id: i32,
    #[sea_orm(default_value = 0)]
    pub default_strategy_id: i32,
    #[sea_orm(default_value = 0)]
    pub expires_at: i64,
    #[sea_orm(default_value = 0)]
    pub max_uses: i32,
    #[sea_orm(default_value = 0)]
    pub used_count: i32,
    #[sea_orm(default_value = 0)]
    pub revoked_at: i64,
    #[sea_orm(default_value = 0)]
    pub created_by: i32,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    pub fn is_revoked(&self) -> bool {
        self.revoked_at > 0
    }

    pub fn is_expired(&self, now: i64) -> bool {
        self.expires_at > 0 && self.expires_at <= now
    }

    pub fn is_exhausted(&self) -> bool {
        self.max_uses > 0 && self.used_count >= self.max_uses
    }
}

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Common status codes (mirrors `model.COMMON_STATUS_*`).
pub const STATUS_ENABLE: i32 = 1;
pub const STATUS_DISABLED: i32 = 2;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique_key, default_value = "")]
    pub username: String,
    #[sea_orm(default_value = "")]
    pub email: String,
    /// Never serialized (Go `json:"-"`).
    #[sea_orm(default_value = "")]
    #[serde(skip_serializing)]
    pub password: String,
    #[sea_orm(default_value = "")]
    pub nickname: String,
    #[sea_orm(default_value = "")]
    pub avatar: String,
    #[sea_orm(default_value = 0)]
    pub group_id: i32,
    /// Nullable in Go (`*bool`).
    pub is_admin: Option<bool>,
    #[sea_orm(default_value = 1)]
    pub status: i32,
    #[sea_orm(default_value = "")]
    pub remark: String,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    pub fn is_admin(&self) -> bool {
        self.is_admin.unwrap_or(false)
    }
    pub fn is_enabled(&self) -> bool {
        self.status == STATUS_ENABLE
    }
}

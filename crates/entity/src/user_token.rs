use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "user_tokens")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = 0)]
    pub user_id: i32,
    #[sea_orm(default_value = "")]
    pub device_uuid: String,
    #[sea_orm(default_value = "")]
    pub device_id: String,
    #[sea_orm(default_value = "")]
    #[serde(skip_serializing)]
    pub token: String,
    #[sea_orm(default_value = 0)]
    pub expired_at: i64,
    #[sea_orm(default_value = 0)]
    pub auth_level: i32,
    #[sea_orm(default_value = true)]
    pub renewable: bool,
    #[sea_orm(column_type = "String(StringLen::N(36))", nullable)]
    #[serde(skip_serializing, default)]
    pub trusted_browser_id: Option<String>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

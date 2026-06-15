use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

// Share rule types / levels (mirrors model constants).
pub const RULE_TYPE_PERSONAL: i32 = 1;
pub const RULE_TYPE_GROUP: i32 = 2;
pub const RULE_READ: i32 = 1;
pub const RULE_READ_WRITE: i32 = 2;
pub const RULE_FULL_CONTROL: i32 = 3;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "address_book_collection_rules")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = 0)]
    pub user_id: i32,
    #[sea_orm(default_value = 0)]
    pub collection_id: i32,
    /// 0: none 1: read 2: read-write 3: full control
    #[sea_orm(default_value = 0)]
    pub rule: i32,
    /// 1: personal 2: group
    #[sea_orm(column_name = "type", default_value = 1)]
    #[serde(rename = "type")]
    pub r#type: i32,
    #[sea_orm(default_value = 0)]
    pub to_id: i32,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

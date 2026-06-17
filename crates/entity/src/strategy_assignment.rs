use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub const TARGET_PEER: &str = "peer";
pub const TARGET_USER: &str = "user";
pub const TARGET_DEVICE_GROUP: &str = "device_group";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "strategy_assignments")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = 0)]
    pub strategy_id: i32,
    #[sea_orm(default_value = "")]
    pub target_type: String,
    #[sea_orm(default_value = "")]
    pub target_id: String,
    #[sea_orm(default_value = 100)]
    pub priority: i32,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub const STATUS_ENABLE: i32 = 1;
pub const STATUS_DISABLED: i32 = 2;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "strategies")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique_key, default_value = "")]
    pub guid: String,
    #[sea_orm(unique_key, default_value = "")]
    pub name: String,
    #[sea_orm(default_value = "")]
    pub note: String,
    #[sea_orm(default_value = 1)]
    pub status: i32,
    #[sea_orm(default_value = "{}")]
    pub config_options: String,
    #[sea_orm(default_value = "{}")]
    pub extra: String,
    #[sea_orm(default_value = 0)]
    pub modified_at: i64,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    pub fn is_enabled(&self) -> bool {
        self.status == STATUS_ENABLE
    }
}

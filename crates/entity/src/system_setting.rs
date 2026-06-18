use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "system_settings")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique_key, default_value = "")]
    pub key: String,
    #[sea_orm(default_value = "")]
    #[serde(skip_serializing)]
    pub value: String,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

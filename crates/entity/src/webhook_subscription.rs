use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "webhook_subscriptions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = "")]
    pub name: String,
    #[sea_orm(default_value = "")]
    pub url: String,
    #[sea_orm(default_value = "")]
    #[serde(skip_serializing)]
    pub secret: String,
    #[sea_orm(column_type = "Text", default_value = "")]
    pub event_types: String,
    #[sea_orm(column_type = "Text", default_value = "")]
    pub device_ids: String,
    #[sea_orm(column_type = "Text", default_value = "")]
    pub device_group_ids: String,
    #[sea_orm(column_type = "Text", default_value = "")]
    pub tags: String,
    #[sea_orm(default_value = true)]
    pub enabled: bool,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

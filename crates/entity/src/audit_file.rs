use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "audit_files")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = "")]
    pub from_peer: String,
    #[sea_orm(default_value = "")]
    pub info: String,
    #[sea_orm(default_value = false)]
    pub is_file: bool,
    #[sea_orm(default_value = "")]
    pub path: String,
    #[sea_orm(default_value = "")]
    pub peer_id: String,
    #[sea_orm(column_name = "type", default_value = 0)]
    #[serde(rename = "type")]
    pub r#type: i32,
    #[sea_orm(default_value = "")]
    pub uuid: String,
    #[sea_orm(default_value = "")]
    pub ip: String,
    #[sea_orm(default_value = 0)]
    pub num: i32,
    #[sea_orm(default_value = "")]
    pub from_name: String,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

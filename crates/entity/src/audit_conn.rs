use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "audit_conns")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = "")]
    pub action: String,
    #[sea_orm(default_value = 0)]
    pub conn_id: i64,
    #[sea_orm(default_value = "")]
    pub peer_id: String,
    #[sea_orm(default_value = "")]
    pub from_peer: String,
    #[sea_orm(default_value = "")]
    pub from_name: String,
    #[sea_orm(default_value = "")]
    pub ip: String,
    #[sea_orm(default_value = "")]
    pub session_id: String,
    #[sea_orm(column_name = "type", default_value = 0)]
    #[serde(rename = "type")]
    pub r#type: i32,
    #[sea_orm(default_value = "")]
    pub uuid: String,
    #[sea_orm(default_value = "")]
    pub note: String,
    #[sea_orm(default_value = 0)]
    pub close_time: i64,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

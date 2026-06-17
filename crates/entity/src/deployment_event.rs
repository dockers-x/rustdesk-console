use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub const ACTION_DEPLOY: &str = "deploy";
pub const ACTION_ASSIGN: &str = "assign";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "deployment_events")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = 0)]
    pub token_id: i32,
    #[sea_orm(default_value = "")]
    pub peer_id: String,
    #[sea_orm(default_value = "")]
    pub uuid: String,
    #[sea_orm(default_value = "")]
    pub action: String,
    #[sea_orm(default_value = "")]
    pub result: String,
    #[sea_orm(default_value = "")]
    pub message: String,
    #[sea_orm(default_value = "")]
    pub ip: String,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

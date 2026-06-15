use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub const TARGET_ID_SERVER: &str = "21115";
pub const TARGET_RELAY_SERVER: &str = "21117";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "server_cmds")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = "")]
    pub cmd: String,
    #[sea_orm(default_value = "")]
    pub alias: String,
    #[sea_orm(default_value = "")]
    pub option: String,
    #[sea_orm(default_value = "")]
    pub explain: String,
    #[sea_orm(default_value = "")]
    pub target: String,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

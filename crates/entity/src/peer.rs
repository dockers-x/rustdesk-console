use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "peers")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub row_id: i32,
    #[sea_orm(default_value = "")]
    pub id: String,
    #[sea_orm(default_value = "")]
    pub cpu: String,
    #[sea_orm(default_value = "")]
    pub hostname: String,
    #[sea_orm(default_value = "")]
    pub memory: String,
    #[sea_orm(default_value = "")]
    pub os: String,
    #[sea_orm(default_value = "")]
    pub username: String,
    #[sea_orm(default_value = "")]
    pub uuid: String,
    #[sea_orm(default_value = "")]
    pub version: String,
    #[sea_orm(default_value = 0)]
    pub user_id: i32,
    #[sea_orm(default_value = 0)]
    pub last_online_time: i64,
    #[sea_orm(default_value = "")]
    pub last_online_ip: String,
    #[sea_orm(default_value = 0)]
    pub group_id: i32,
    #[sea_orm(default_value = "")]
    pub alias: String,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

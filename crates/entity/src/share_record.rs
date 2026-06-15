use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "share_records")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = 0)]
    pub user_id: i32,
    #[sea_orm(default_value = "")]
    pub peer_id: String,
    #[sea_orm(default_value = "")]
    pub share_token: String,
    #[sea_orm(default_value = "")]
    pub password_type: String,
    #[sea_orm(default_value = "")]
    pub password: String,
    #[sea_orm(default_value = 0)]
    pub expire: i64,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

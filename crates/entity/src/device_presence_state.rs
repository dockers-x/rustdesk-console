use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "device_presence_states")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = 0)]
    pub peer_row_id: i32,
    #[sea_orm(default_value = "")]
    pub peer_id: String,
    #[sea_orm(default_value = false)]
    pub online: bool,
    #[sea_orm(default_value = 0)]
    pub last_seen_at: i64,
    #[sea_orm(default_value = 0)]
    pub last_changed_at: i64,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

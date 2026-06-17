use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub const STATUS_UPLOADING: i32 = 0;
pub const STATUS_COMPLETE: i32 = 1;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "record_files")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = "")]
    pub filename: String,
    #[sea_orm(default_value = "")]
    pub peer_id: String,
    #[sea_orm(default_value = "")]
    pub direction: String,
    #[sea_orm(default_value = 0)]
    pub size: i64,
    #[sea_orm(default_value = 0)]
    pub status: i32,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

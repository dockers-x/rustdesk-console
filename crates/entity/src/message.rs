use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub const KIND_ANNOUNCEMENT: &str = "announcement";
pub const KIND_BROADCAST: &str = "broadcast";
pub const KIND_PRIVATE: &str = "private";

pub const STATUS_ENABLE: i32 = 1;
pub const STATUS_DISABLED: i32 = 2;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "messages")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = 0)]
    pub sender_id: i32,
    #[sea_orm(default_value = "")]
    pub sender_name: String,
    #[sea_orm(default_value = 0)]
    pub recipient_id: i32,
    #[sea_orm(default_value = "")]
    pub recipient_name: String,
    #[sea_orm(default_value = "")]
    pub kind: String,
    #[sea_orm(default_value = "")]
    pub title: String,
    #[sea_orm(column_type = "Text", default_value = "")]
    pub body: String,
    #[sea_orm(default_value = 1)]
    pub status: i32,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

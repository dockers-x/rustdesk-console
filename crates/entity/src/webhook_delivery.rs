use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_SUCCESS: &str = "success";
pub const STATUS_FAILED: &str = "failed";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "webhook_deliveries")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = 0)]
    pub subscription_id: i32,
    #[sea_orm(default_value = "")]
    pub event_id: String,
    #[sea_orm(default_value = "")]
    pub event_type: String,
    #[sea_orm(default_value = "pending")]
    pub status: String,
    #[sea_orm(default_value = 0)]
    pub status_code: i32,
    #[sea_orm(default_value = 1)]
    pub attempt: i32,
    #[sea_orm(column_type = "Text", default_value = "")]
    pub request_body: String,
    #[sea_orm(column_type = "Text", default_value = "")]
    pub response_body: String,
    #[sea_orm(column_type = "Text", default_value = "")]
    pub error: String,
    #[sea_orm(default_value = 0)]
    pub delivered_at: i64,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub const KIND_TOTP: &str = "totp";
pub const KIND_EMAIL: &str = "email";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "login_verifications")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = 0)]
    pub user_id: i32,
    #[sea_orm(default_value = "")]
    #[serde(skip_serializing)]
    pub secret: String,
    #[sea_orm(default_value = "")]
    pub kind: String,
    #[sea_orm(default_value = "")]
    #[serde(skip_serializing)]
    pub code_hash: String,
    #[sea_orm(default_value = "")]
    pub device_id: String,
    #[sea_orm(default_value = "")]
    pub device_uuid: String,
    #[sea_orm(default_value = "")]
    pub device_name: String,
    #[sea_orm(default_value = "")]
    pub device_os: String,
    #[sea_orm(default_value = "")]
    pub device_type: String,
    #[sea_orm(default_value = "")]
    pub ip: String,
    #[sea_orm(default_value = false)]
    pub auto_login: bool,
    #[sea_orm(default_value = 0)]
    pub expires_at: i64,
    #[sea_orm(default_value = 0)]
    pub verified_at: i64,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

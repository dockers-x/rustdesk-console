use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "smtp_email_configs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = "")]
    pub name: String,
    #[sea_orm(default_value = "")]
    pub host: String,
    #[sea_orm(default_value = 587)]
    pub port: i32,
    #[sea_orm(default_value = "")]
    pub username: String,
    #[sea_orm(default_value = "")]
    #[serde(skip_serializing)]
    pub password: String,
    #[sea_orm(default_value = "")]
    pub from_address: String,
    #[sea_orm(default_value = "starttls")]
    pub tls: String,
    #[sea_orm(default_value = false)]
    pub enabled: bool,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

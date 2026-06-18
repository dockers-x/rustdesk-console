use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "address_books")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub row_id: i32,
    #[sea_orm(default_value = "")]
    #[serde(default)]
    pub id: String,
    #[sea_orm(default_value = "")]
    #[serde(default)]
    pub username: String,
    #[sea_orm(default_value = "")]
    #[serde(default)]
    pub password: String,
    #[sea_orm(default_value = "")]
    #[serde(default)]
    pub hostname: String,
    #[sea_orm(default_value = "")]
    #[serde(default)]
    pub alias: String,
    #[sea_orm(default_value = "")]
    #[serde(default)]
    pub note: String,
    #[sea_orm(default_value = "")]
    #[serde(default)]
    pub platform: String,
    /// JSON array of tag names (Go `custom_types.AutoJson`). Always set on insert.
    #[sea_orm(column_type = "Json")]
    #[serde(default = "default_tags")]
    pub tags: Json,
    #[sea_orm(default_value = "")]
    #[serde(default)]
    pub hash: String,
    #[sea_orm(default_value = 0)]
    #[serde(default)]
    pub user_id: i32,
    #[sea_orm(default_value = false)]
    #[serde(rename = "forceAlwaysRelay", default)]
    pub force_always_relay: bool,
    #[sea_orm(default_value = "")]
    #[serde(rename = "rdpPort", default)]
    pub rdp_port: String,
    #[sea_orm(default_value = "")]
    #[serde(rename = "rdpUsername", default)]
    pub rdp_username: String,
    #[sea_orm(default_value = false)]
    #[serde(default)]
    pub online: bool,
    #[sea_orm(default_value = "")]
    #[serde(rename = "loginName", default)]
    pub login_name: String,
    #[sea_orm(default_value = false)]
    #[serde(rename = "sameServer", default)]
    pub same_server: bool,
    #[sea_orm(default_value = 0)]
    #[serde(default)]
    pub collection_id: i32,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

fn default_tags() -> Json {
    Json::Array(vec![])
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

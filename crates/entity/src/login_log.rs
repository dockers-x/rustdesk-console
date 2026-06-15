use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

// Login log client/type constants.
pub const CLIENT_WEB_ADMIN: &str = "webadmin";
pub const CLIENT_WEB: &str = "webclient";
pub const CLIENT_APP: &str = "app";
pub const TYPE_ACCOUNT: &str = "account";
pub const TYPE_OAUTH: &str = "oauth";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "login_logs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = 0)]
    pub user_id: i32,
    #[sea_orm(default_value = "")]
    pub client: String,
    #[sea_orm(default_value = "")]
    pub device_id: String,
    #[sea_orm(default_value = "")]
    pub uuid: String,
    #[sea_orm(default_value = "")]
    pub ip: String,
    #[sea_orm(column_name = "type", default_value = "")]
    #[serde(rename = "type")]
    pub r#type: String,
    #[sea_orm(default_value = "")]
    pub platform: String,
    #[sea_orm(default_value = 0)]
    pub user_token_id: i32,
    #[sea_orm(default_value = 0)]
    pub is_deleted: i32,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

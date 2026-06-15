use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Mirrors Go `UserThird`, which embeds `OauthUser` (open_id, name, username,
/// email, verified_email, picture) as columns.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "user_thirds")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = 0)]
    pub user_id: i32,
    #[sea_orm(default_value = "")]
    pub open_id: String,
    #[sea_orm(default_value = "")]
    pub name: String,
    #[sea_orm(default_value = "")]
    pub username: String,
    #[sea_orm(default_value = "")]
    pub email: String,
    #[sea_orm(default_value = false)]
    #[serde(skip_serializing_if = "is_false", default)]
    pub verified_email: bool,
    #[sea_orm(default_value = "")]
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub picture: String,
    #[sea_orm(default_value = "")]
    pub union_id: String,
    /// deprecated
    #[sea_orm(default_value = "")]
    pub third_type: String,
    #[sea_orm(default_value = "")]
    pub oauth_type: String,
    #[sea_orm(default_value = "")]
    pub op: String,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

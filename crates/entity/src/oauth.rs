use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

// Oauth provider type constants (lowercase).
pub const TYPE_GITHUB: &str = "github";
pub const TYPE_GOOGLE: &str = "google";
pub const TYPE_OIDC: &str = "oidc";
pub const TYPE_WEBAUTH: &str = "webauth";
pub const TYPE_LINUXDO: &str = "linuxdo";
pub const ISSUER_GOOGLE: &str = "https://accounts.google.com";
pub const OIDC_DEFAULT_SCOPES: &str = "openid,profile,email";
pub const PKCE_METHOD_S256: &str = "S256";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "oauths")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(default_value = "")]
    pub op: String,
    #[sea_orm(default_value = "")]
    pub oauth_type: String,
    #[sea_orm(default_value = "")]
    pub client_id: String,
    #[sea_orm(default_value = "")]
    pub client_secret: String,
    pub auto_register: Option<bool>,
    #[sea_orm(default_value = "")]
    pub scopes: String,
    #[sea_orm(default_value = "")]
    pub issuer: String,
    pub pkce_enable: Option<bool>,
    #[sea_orm(default_value = "")]
    pub pkce_method: String,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

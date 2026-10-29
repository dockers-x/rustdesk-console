use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "audit_alarms")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub row_id: i32,
    #[sea_orm(default_value = "")]
    pub id: String,
    #[sea_orm(default_value = "")]
    pub uuid: String,
    #[sea_orm(default_value = 0)]
    pub typ: i32,
    #[sea_orm(column_type = "Text", default_value = "")]
    pub info: String,
    pub conn_id: Option<i64>,
    pub nonce: Option<String>,
    pub conn_audit_ref: Option<String>,
    #[serde(serialize_with = "crate::datetime::serialize_opt", skip_deserializing)]
    pub created_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

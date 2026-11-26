//! Rules for the paired RustDesk servers, independent of the device inventory.

use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "blocklist_rules")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub kind: String,
    pub value: String,
    pub note: String,
    #[sea_orm(column_type = "String(StringLen::N(64))", unique)]
    #[serde(skip)]
    pub rule_key: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

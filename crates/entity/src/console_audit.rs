use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "console_audits")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub action: String,
    pub result: String,
    pub actor_id: i32,
    pub subject_id: Option<i32>,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

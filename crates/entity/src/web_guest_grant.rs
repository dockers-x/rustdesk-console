use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "web_guest_grants")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique_key)]
    #[serde(skip)]
    pub token_hash: String,
    #[serde(skip)]
    pub peer_row_id: i32,
    pub peer_id: String,
    #[serde(skip)]
    pub peer_uuid: String,
    #[serde(skip)]
    pub peer_key: String,
    pub expires_at: i64,
    pub max_uses: i32,
    #[sea_orm(default_value = 0)]
    pub used_count: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

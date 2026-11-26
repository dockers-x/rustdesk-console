//! Server-observed changes in device presence.

use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "device_presence_events")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[serde(skip)]
    pub peer_row_id: i32,
    pub online: bool,
    pub observed_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

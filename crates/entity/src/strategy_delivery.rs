use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "strategy_delivery")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub peer_row_id: i32,
    #[sea_orm(column_type = "Json")]
    pub managed_keys: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

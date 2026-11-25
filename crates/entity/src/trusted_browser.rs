use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "trusted_browsers")]
pub struct Model {
    #[sea_orm(
        primary_key,
        auto_increment = false,
        column_type = "String(StringLen::N(36))"
    )]
    pub id: String,
    pub user_id: i32,
    #[sea_orm(column_type = "String(StringLen::N(64))")]
    pub account_binding: String,
    #[sea_orm(column_type = "String(StringLen::N(64))")]
    pub browser: String,
    #[sea_orm(column_type = "String(StringLen::N(64))")]
    pub platform: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub expires_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

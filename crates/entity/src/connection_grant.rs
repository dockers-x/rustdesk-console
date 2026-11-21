use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "connection_grants")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_group_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub device_group_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::group::Entity",
        from = "Column::UserGroupId",
        to = "super::group::Column::Id",
        on_delete = "Cascade"
    )]
    UserGroup,
    #[sea_orm(
        belongs_to = "super::device_group::Entity",
        from = "Column::DeviceGroupId",
        to = "super::device_group::Column::Id",
        on_delete = "Cascade"
    )]
    DeviceGroup,
}

impl ActiveModelBehavior for ActiveModel {}

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "notification_template_bindings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub subscription_id: i32,
    pub template_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::webhook_subscription::Entity",
        from = "Column::SubscriptionId",
        to = "super::webhook_subscription::Column::Id",
        on_delete = "Cascade"
    )]
    Subscription,
    #[sea_orm(
        belongs_to = "super::notification_template::Entity",
        from = "Column::TemplateId",
        to = "super::notification_template::Column::Id",
        on_delete = "Restrict"
    )]
    Template,
}

impl Related<super::notification_template::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Template.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

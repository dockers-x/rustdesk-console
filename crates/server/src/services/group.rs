//! Group + DeviceGroup service — ports `service/group.go`.

use sea_orm::*;

use ::entity::{device_group, group};

use crate::services::{now, paginate};

// --- Group ---

pub async fn info_by_id(db: &DatabaseConnection, id: i32) -> Result<Option<group::Model>, DbErr> {
    group::Entity::find_by_id(id).one(db).await
}

pub struct GroupListResult {
    pub list: Vec<group::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
) -> Result<GroupListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let q = group::Entity::find();
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(GroupListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn create(
    db: &impl ConnectionTrait,
    name: &str,
    group_type: i32,
) -> Result<group::Model, DbErr> {
    let am = group::ActiveModel {
        name: Set(name.to_string()),
        r#type: Set(group_type),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await
}

pub async fn update(db: &DatabaseConnection, id: i32, name: String, t: i32) -> Result<(), DbErr> {
    let am = group::ActiveModel {
        id: Set(id),
        name: Set(name),
        r#type: Set(t),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    use super::address_book::{self, ShareRecipient};

    let transaction = super::console_roles::authority_transaction(db).await?;
    let affected = address_book::collections_affected_by_recipient_deletion(
        &transaction,
        ShareRecipient::Group(id),
    )
    .await?;
    group::Entity::delete_by_id(id).exec(&transaction).await?;
    for collection_id in affected {
        address_book::clear_unshared_delegation(&transaction, collection_id).await?;
    }
    super::console_roles::remove_scope(
        &transaction,
        super::console_roles::RemovedScope::UserGroup(id),
    )
    .await?;
    transaction.commit().await
}

// --- DeviceGroup ---

pub async fn device_group_list_all(
    db: &DatabaseConnection,
) -> Result<Vec<device_group::Model>, DbErr> {
    device_group::Entity::find().all(db).await
}

pub async fn device_group_info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<device_group::Model>, DbErr> {
    device_group::Entity::find_by_id(id).one(db).await
}

pub async fn device_group_info_by_name(
    db: &DatabaseConnection,
    name: &str,
) -> Result<Option<device_group::Model>, DbErr> {
    device_group::Entity::find()
        .filter(device_group::Column::Name.eq(name))
        .one(db)
        .await
}

pub struct DeviceGroupListResult {
    pub list: Vec<device_group::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn device_group_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
) -> Result<DeviceGroupListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let q = device_group::Entity::find();
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(DeviceGroupListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn device_group_create(
    db: &DatabaseConnection,
    name: &str,
) -> Result<device_group::Model, DbErr> {
    let am = device_group::ActiveModel {
        name: Set(name.to_string()),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await
}

pub async fn device_group_update(
    db: &DatabaseConnection,
    id: i32,
    name: String,
) -> Result<(), DbErr> {
    let am = device_group::ActiveModel {
        id: Set(id),
        name: Set(name),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

pub async fn device_group_delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    let transaction = super::console_roles::authority_transaction(db).await?;
    device_group::Entity::delete_by_id(id).exec(&transaction).await?;
    super::console_roles::remove_scope(
        &transaction,
        super::console_roles::RemovedScope::DeviceGroup(id),
    )
    .await?;
    transaction.commit().await
}

//! Console role assignments and the transaction lock for account authority changes.

use ::entity::{device_group, system_setting, user};
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbErr,
    EntityTrait, PaginatorTrait, QueryFilter, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use super::now;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConsoleRole {
    #[default]
    Member,
    AuditReader,
    DeviceGroupOperator {
        group_ids: Vec<i32>,
    },
}

fn key(user_id: i32) -> String {
    format!("console_role:{user_id}")
}

pub(super) async fn setting<T: serde::de::DeserializeOwned>(
    db: &impl ConnectionTrait,
    key: &str,
) -> Result<Option<T>, DbErr> {
    system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(key))
        .one(db)
        .await?
        .map(|row| serde_json::from_str(&row.value).map_err(|err| DbErr::Custom(err.to_string())))
        .transpose()
}

pub(super) async fn write_setting(
    db: &impl ConnectionTrait,
    key: String,
    value: String,
) -> Result<(), DbErr> {
    let existing = system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(&key))
        .one(db)
        .await?;
    if let Some(row) = existing {
        let mut setting: system_setting::ActiveModel = row.into();
        setting.value = Set(value);
        setting.updated_at = Set(now());
        setting.update(db).await?;
    } else {
        system_setting::ActiveModel {
            key: Set(key),
            value: Set(value),
            created_at: Set(now()),
            updated_at: Set(now()),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }
    Ok(())
}

/// Acquire the same durable write lock before reading or changing account authority.
pub(crate) async fn authority_transaction(
    db: &DatabaseConnection,
) -> Result<DatabaseTransaction, DbErr> {
    let transaction = db.begin().await?;
    // Use a reserved negative primary key. Existing installations may lack
    // the unique index on the setting key, but always have the primary key.
    system_setting::Entity::insert(system_setting::ActiveModel {
        id: Set(i32::MIN),
        key: Set("console_authority_lock".to_owned()),
        created_at: Set(now()),
        value: Set(String::new()),
        updated_at: Set(now()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(system_setting::Column::Id)
            .update_column(system_setting::Column::UpdatedAt)
            .to_owned(),
    )
    .exec_without_returning(&transaction)
    .await?;
    Ok(transaction)
}

pub(crate) async fn for_user(
    db: &impl ConnectionTrait,
    user_id: i32,
) -> Result<ConsoleRole, DbErr> {
    Ok(setting(db, &key(user_id)).await?.unwrap_or_default())
}

pub(super) async fn clear(db: &impl ConnectionTrait, user_id: i32) -> Result<(), DbErr> {
    system_setting::Entity::delete_many()
        .filter(system_setting::Column::Key.eq(key(user_id)))
        .exec(db)
        .await?;
    Ok(())
}

/// Remove a deleted group from every assignment while the authority lock is held.
pub(super) async fn remove_group(db: &DatabaseTransaction, group_id: i32) -> Result<(), DbErr> {
    let assignments = system_setting::Entity::find()
        .filter(system_setting::Column::Key.starts_with("console_role:"))
        .all(db)
        .await?;
    for row in assignments {
        let role =
            serde_json::from_str(&row.value).map_err(|err| DbErr::Custom(err.to_string()))?;
        if let ConsoleRole::DeviceGroupOperator { mut group_ids } = role {
            if group_ids.contains(&group_id) {
                group_ids.retain(|id| *id != group_id);
                let value = serde_json::to_string(&ConsoleRole::DeviceGroupOperator { group_ids })
                    .map_err(|err| DbErr::Custom(err.to_string()))?;
                let mut setting: system_setting::ActiveModel = row.into();
                setting.value = Set(value);
                setting.updated_at = Set(now());
                setting.update(db).await?;
            }
        }
    }
    Ok(())
}

pub(crate) async fn assign(
    db: &DatabaseConnection,
    user_id: i32,
    mut role: ConsoleRole,
) -> Result<(), DbErr> {
    let transaction = authority_transaction(db).await?;
    let account = user::Entity::find_by_id(user_id)
        .one(&transaction)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("Account not found".to_owned()))?;
    if account.is_admin() && role != ConsoleRole::Member {
        return Err(DbErr::Custom(
            "Restricted roles require a non-admin account".to_owned(),
        ));
    }
    if let ConsoleRole::DeviceGroupOperator { group_ids } = &mut role {
        group_ids.sort_unstable();
        group_ids.dedup();
        if group_ids.is_empty()
            || group_ids.len() > 50
            || group_ids.iter().any(|id| *id <= 0)
            || device_group::Entity::find()
                .filter(device_group::Column::Id.is_in(group_ids.iter().copied()))
                .count(&transaction)
                .await?
                != group_ids.len() as u64
        {
            return Err(DbErr::Custom(
                "Select between 1 and 50 existing device groups".to_owned(),
            ));
        }
    }
    if role == ConsoleRole::Member {
        clear(&transaction, user_id).await?;
    } else {
        let value = serde_json::to_string(&role).map_err(|err| DbErr::Custom(err.to_string()))?;
        write_setting(&transaction, key(user_id), value).await?;
    }
    transaction.commit().await
}

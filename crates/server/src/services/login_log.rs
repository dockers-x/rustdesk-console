//! Login log service — ports the read side of `service/loginLog.go`.

use sea_orm::*;

use ::entity::login_log;

use crate::services::paginate;

pub struct LoginLogListResult {
    pub list: Vec<login_log::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    user_id: Option<i32>,
) -> Result<LoginLogListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = login_log::Entity::find().filter(login_log::Column::IsDeleted.eq(0));
    if let Some(uid) = user_id {
        q = q.filter(login_log::Column::UserId.eq(uid));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(login_log::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(LoginLogListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<login_log::Model>, DbErr> {
    login_log::Entity::find_by_id(id).one(db).await
}

/// Hard delete (admin).
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    login_log::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn batch_delete(db: &DatabaseConnection, ids: &[i32]) -> Result<(), DbErr> {
    login_log::Entity::delete_many()
        .filter(login_log::Column::Id.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

/// Soft delete (set is_deleted) — used by the `my/*` endpoints.
pub async fn soft_delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    let am = login_log::ActiveModel {
        id: Set(id),
        is_deleted: Set(1),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

pub async fn batch_soft_delete(
    db: &DatabaseConnection,
    user_id: i32,
    ids: &[i32],
) -> Result<(), DbErr> {
    login_log::Entity::update_many()
        .col_expr(
            login_log::Column::IsDeleted,
            sea_orm::sea_query::Expr::value(1),
        )
        .filter(login_log::Column::UserId.eq(user_id))
        .filter(login_log::Column::Id.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

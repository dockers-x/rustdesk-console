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

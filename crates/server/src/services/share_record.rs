//! Share-record service — ports `service/shareRecord.go` + ShareByWebClient.

use sea_orm::*;

use ::entity::share_record;

use crate::services::{now, paginate};

pub struct ShareRecordListResult {
    pub list: Vec<share_record::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    user_id: Option<i32>,
) -> Result<ShareRecordListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = share_record::Entity::find();
    if let Some(uid) = user_id.filter(|v| *v > 0) {
        q = q.filter(share_record::Column::UserId.eq(uid));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(share_record::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(ShareRecordListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<share_record::Model>, DbErr> {
    share_record::Entity::find_by_id(id).one(db).await
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    share_record::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn batch_delete(db: &DatabaseConnection, ids: &[i32]) -> Result<(), DbErr> {
    share_record::Entity::delete_many()
        .filter(share_record::Column::Id.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

/// Count how many of `ids` belong to `user_id` (for ownership checks).
pub async fn count_owned(
    db: &DatabaseConnection,
    user_id: i32,
    ids: &[i32],
) -> Result<i64, DbErr> {
    Ok(share_record::Entity::find()
        .filter(share_record::Column::UserId.eq(user_id))
        .filter(share_record::Column::Id.is_in(ids.to_vec()))
        .count(db)
        .await? as i64)
}

/// Create a share record with a generated share token (≈ ShareByWebClient).
pub async fn share_by_web_client(
    db: &DatabaseConnection,
    user_id: i32,
    peer_id: &str,
    password_type: &str,
    password: &str,
    expire: i64,
) -> Result<String, DbErr> {
    let token = uuid::Uuid::new_v4().to_string();
    let am = share_record::ActiveModel {
        user_id: Set(user_id),
        peer_id: Set(peer_id.to_string()),
        share_token: Set(token.clone()),
        password_type: Set(password_type.to_string()),
        password: Set(password.to_string()),
        expire: Set(expire),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await?;
    Ok(token)
}

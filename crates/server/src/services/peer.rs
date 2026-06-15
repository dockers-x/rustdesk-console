//! Peer service — ports `service/peer.go`.

use sea_orm::*;

use ::entity::peer;

use crate::services::{now, paginate};

pub async fn find_by_id(db: &DatabaseConnection, id: &str) -> Result<Option<peer::Model>, DbErr> {
    peer::Entity::find()
        .filter(peer::Column::Id.eq(id))
        .one(db)
        .await
}

pub async fn find_by_uuid(
    db: &DatabaseConnection,
    uuid: &str,
) -> Result<Option<peer::Model>, DbErr> {
    peer::Entity::find()
        .filter(peer::Column::Uuid.eq(uuid))
        .one(db)
        .await
}

pub async fn find_by_user_id_and_uuid(
    db: &DatabaseConnection,
    uuid: &str,
    user_id: i32,
) -> Result<Option<peer::Model>, DbErr> {
    peer::Entity::find()
        .filter(peer::Column::Uuid.eq(uuid))
        .filter(peer::Column::UserId.eq(user_id))
        .one(db)
        .await
}

pub async fn info_by_row_id(
    db: &DatabaseConnection,
    row_id: i32,
) -> Result<Option<peer::Model>, DbErr> {
    peer::Entity::find_by_id(row_id).one(db).await
}

/// Bind a uuid to a user id (updates an existing peer; does not create one).
pub async fn uuid_bind_user_id(
    db: &DatabaseConnection,
    uuid: &str,
    user_id: i32,
) -> Result<(), DbErr> {
    if let Some(p) = find_by_uuid(db, uuid).await? {
        let mut am: peer::ActiveModel = p.into();
        am.user_id = Set(user_id);
        am.updated_at = Set(now());
        am.update(db).await?;
    }
    Ok(())
}

pub async fn uuid_unbind_user_id(
    db: &DatabaseConnection,
    uuid: &str,
    user_id: i32,
) -> Result<(), DbErr> {
    if let Some(p) = find_by_user_id_and_uuid(db, uuid, user_id).await? {
        let mut am: peer::ActiveModel = p.into();
        am.user_id = Set(0);
        am.update(db).await?;
    }
    Ok(())
}

pub async fn erase_user_id(db: &DatabaseConnection, user_id: i32) -> Result<(), DbErr> {
    let peers = peer::Entity::find()
        .filter(peer::Column::UserId.eq(user_id))
        .all(db)
        .await?;
    for p in peers {
        let mut am: peer::ActiveModel = p.into();
        am.user_id = Set(0);
        am.update(db).await?;
    }
    Ok(())
}

pub struct PeerListResult {
    pub list: Vec<peer::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list_by_user_ids(
    db: &DatabaseConnection,
    user_ids: &[i32],
    page: u64,
    page_size: u64,
) -> Result<PeerListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let q = peer::Entity::find().filter(peer::Column::UserId.is_in(user_ids.to_vec()));
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(PeerListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    id_like: Option<String>,
) -> Result<PeerListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = peer::Entity::find();
    if let Some(id) = id_like.filter(|s| !s.is_empty()) {
        q = q.filter(peer::Column::Id.contains(&id));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(PeerListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn create(db: &DatabaseConnection, am: peer::ActiveModel) -> Result<peer::Model, DbErr> {
    let mut am = am;
    am.created_at = Set(now());
    am.updated_at = Set(now());
    am.insert(db).await
}

pub async fn update(db: &DatabaseConnection, am: peer::ActiveModel) -> Result<(), DbErr> {
    let mut am = am;
    am.updated_at = Set(now());
    am.update(db).await?;
    Ok(())
}

pub async fn delete(db: &DatabaseConnection, p: &peer::Model) -> Result<(), DbErr> {
    let uuid = p.uuid.clone();
    peer::Entity::delete_by_id(p.row_id).exec(db).await?;
    if !uuid.is_empty() {
        crate::services::user::flush_token_by_uuid(db, &uuid).await?;
    }
    Ok(())
}

pub async fn batch_delete(db: &DatabaseConnection, ids: &[i32]) -> Result<(), DbErr> {
    let uuids: Vec<String> = peer::Entity::find()
        .filter(peer::Column::RowId.is_in(ids.to_vec()))
        .all(db)
        .await?
        .into_iter()
        .map(|p| p.uuid)
        .filter(|u| !u.is_empty())
        .collect();
    peer::Entity::delete_many()
        .filter(peer::Column::RowId.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    for uuid in uuids {
        crate::services::user::flush_token_by_uuid(db, &uuid).await?;
    }
    Ok(())
}

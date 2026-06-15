//! Audit service — ports `service/audit.go` (conn + file logs).

use sea_orm::*;

use ::entity::{audit_conn, audit_file};

use crate::services::paginate;

// ---- AuditConn ----

pub struct AuditConnListResult {
    pub list: Vec<audit_conn::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn conn_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    peer_id: Option<String>,
    from_peer: Option<String>,
) -> Result<AuditConnListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = audit_conn::Entity::find();
    if let Some(p) = peer_id.filter(|s| !s.is_empty()) {
        q = q.filter(audit_conn::Column::PeerId.contains(&p));
    }
    if let Some(p) = from_peer.filter(|s| !s.is_empty()) {
        q = q.filter(audit_conn::Column::FromPeer.contains(&p));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(audit_conn::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(AuditConnListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn conn_info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<audit_conn::Model>, DbErr> {
    audit_conn::Entity::find_by_id(id).one(db).await
}

pub async fn delete_conn(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    audit_conn::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn batch_delete_conn(db: &DatabaseConnection, ids: &[i32]) -> Result<(), DbErr> {
    audit_conn::Entity::delete_many()
        .filter(audit_conn::Column::Id.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

// ---- AuditFile ----

pub struct AuditFileListResult {
    pub list: Vec<audit_file::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn file_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    peer_id: Option<String>,
    from_peer: Option<String>,
) -> Result<AuditFileListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = audit_file::Entity::find();
    if let Some(p) = peer_id.filter(|s| !s.is_empty()) {
        q = q.filter(audit_file::Column::PeerId.contains(&p));
    }
    if let Some(p) = from_peer.filter(|s| !s.is_empty()) {
        q = q.filter(audit_file::Column::FromPeer.contains(&p));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(audit_file::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(AuditFileListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn file_info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<audit_file::Model>, DbErr> {
    audit_file::Entity::find_by_id(id).one(db).await
}

pub async fn delete_file(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    audit_file::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn batch_delete_file(db: &DatabaseConnection, ids: &[i32]) -> Result<(), DbErr> {
    audit_file::Entity::delete_many()
        .filter(audit_file::Column::Id.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

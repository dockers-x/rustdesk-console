use std::path::{Path, PathBuf};

use sea_orm::*;

use ::entity::record_file;

use crate::services::{now, paginate};

pub const MAX_RECORD_FILENAME_LEN: usize = 255;

pub struct RecordFileListResult {
    pub list: Vec<record_file::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    filename: Option<String>,
    peer_id: Option<String>,
) -> Result<RecordFileListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = record_file::Entity::find();
    if let Some(v) = filename.filter(|s| !s.is_empty()) {
        q = q.filter(record_file::Column::Filename.contains(&v));
    }
    if let Some(v) = peer_id.filter(|s| !s.is_empty()) {
        q = q.filter(record_file::Column::PeerId.contains(&v));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(record_file::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(RecordFileListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<record_file::Model>, DbErr> {
    record_file::Entity::find_by_id(id).one(db).await
}

pub async fn start_upload(db: &DatabaseConnection, filename: &str) -> Result<(), DbErr> {
    let peer_id = parse_peer_id(filename);
    let direction = parse_direction(filename);
    if let Some(existing) = info_by_filename(db, filename).await? {
        let mut am: record_file::ActiveModel = existing.into();
        am.peer_id = Set(peer_id);
        am.direction = Set(direction);
        am.size = Set(0);
        am.status = Set(record_file::STATUS_UPLOADING);
        am.updated_at = Set(now());
        am.update(db).await?;
        return Ok(());
    }
    let am = record_file::ActiveModel {
        filename: Set(filename.to_string()),
        peer_id: Set(peer_id),
        direction: Set(direction),
        size: Set(0),
        status: Set(record_file::STATUS_UPLOADING),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await?;
    Ok(())
}

pub async fn mark_uploading(
    db: &DatabaseConnection,
    filename: &str,
    size: i64,
) -> Result<(), DbErr> {
    upsert_size_status(db, filename, size, record_file::STATUS_UPLOADING).await
}

pub async fn complete_upload(
    db: &DatabaseConnection,
    filename: &str,
    size: i64,
) -> Result<(), DbErr> {
    upsert_size_status(db, filename, size, record_file::STATUS_COMPLETE).await
}

async fn upsert_size_status(
    db: &DatabaseConnection,
    filename: &str,
    size: i64,
    status: i32,
) -> Result<(), DbErr> {
    if let Some(existing) = info_by_filename(db, filename).await? {
        let mut am: record_file::ActiveModel = existing.into();
        am.size = Set(size);
        am.status = Set(status);
        am.updated_at = Set(now());
        am.update(db).await?;
        return Ok(());
    }
    let am = record_file::ActiveModel {
        filename: Set(filename.to_string()),
        peer_id: Set(parse_peer_id(filename)),
        direction: Set(parse_direction(filename)),
        size: Set(size),
        status: Set(status),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await?;
    Ok(())
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    record_file::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn delete_by_filename(db: &DatabaseConnection, filename: &str) -> Result<(), DbErr> {
    record_file::Entity::delete_many()
        .filter(record_file::Column::Filename.eq(filename))
        .exec(db)
        .await?;
    Ok(())
}

pub fn record_root(resources_path: &str) -> PathBuf {
    let base = if resources_path.is_empty() {
        "resources"
    } else {
        resources_path
    };
    PathBuf::from(base).join("record")
}

pub fn record_path(resources_path: &str, filename: &str) -> Result<PathBuf, String> {
    Ok(record_root(resources_path).join(sanitize_filename(filename)?))
}

pub fn sanitize_filename(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("missing file".to_string());
    }
    if name.len() > MAX_RECORD_FILENAME_LEN {
        return Err("file name too long".to_string());
    }
    if name.chars().any(|c| {
        c.is_control()
            || matches!(
                c,
                '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            )
    }) {
        return Err("invalid file name".to_string());
    }
    let name = name.trim_start_matches('.');
    if name.is_empty() || name == "." || name == ".." {
        return Err("invalid file name".to_string());
    }
    Ok(name.to_string())
}

fn parse_direction(filename: &str) -> String {
    filename
        .split_once('_')
        .map(|(direction, _)| direction.to_string())
        .unwrap_or_default()
}

fn parse_peer_id(filename: &str) -> String {
    let mut parts = filename.split('_');
    let _direction = parts.next();
    parts.next().unwrap_or_default().to_string()
}

async fn info_by_filename(
    db: &DatabaseConnection,
    filename: &str,
) -> Result<Option<record_file::Model>, DbErr> {
    record_file::Entity::find()
        .filter(record_file::Column::Filename.eq(filename))
        .one(db)
        .await
}

pub async fn file_size(path: &Path) -> i64 {
    tokio::fs::metadata(path)
        .await
        .map(|m| m.len() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_accepts_rustdesk_record_names() {
        assert_eq!(
            sanitize_filename("incoming_123456__20260617123345001_display0_vp9.webm").unwrap(),
            "incoming_123456__20260617123345001_display0_vp9.webm"
        );
    }

    #[test]
    fn filename_rejects_path_traversal() {
        assert!(sanitize_filename("../x.webm").is_err());
        assert!(sanitize_filename("dir/x.webm").is_err());
        assert!(sanitize_filename(r"dir\x.webm").is_err());
        assert!(sanitize_filename(".").is_err());
    }

    #[test]
    fn parses_direction_and_peer_id_from_rustdesk_record_name() {
        let name = "incoming_123456__20260617123345001_display0_vp9.webm";
        assert_eq!(parse_direction(name), "incoming");
        assert_eq!(parse_peer_id(name), "123456");
    }
}

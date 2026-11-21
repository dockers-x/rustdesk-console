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

pub async fn save_upload(
    db: &DatabaseConnection,
    filename: &str,
    location: &super::record_storage::RecordStorageLocation,
    profile_id: Option<String>,
    size: i64,
    status: i32,
) -> Result<(), DbErr> {
    let existing = info_by_filename(db, filename).await?;
    let mut row: record_file::ActiveModel = match existing {
        Some(existing) => existing.into(),
        None => record_file::ActiveModel {
            filename: Set(filename.to_string()),
            peer_id: Set(parse_peer_id(filename)),
            direction: Set(parse_direction(filename)),
            created_at: Set(now()),
            ..Default::default()
        },
    };
    row.storage_backend = Set(location.backend.clone());
    row.storage_key = Set(location.key.clone());
    row.storage_profile_id = Set(profile_id);
    row.size = Set(size);
    row.status = Set(status);
    row.updated_at = Set(now());
    row.save(db).await?;
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

pub fn record_root_for_config(resources_path: &str, local_dir: &str) -> PathBuf {
    if local_dir.trim().is_empty() {
        record_root(resources_path)
    } else {
        PathBuf::from(local_dir.trim())
    }
}

pub fn record_temp_root(resources_path: &str, temp_dir: &str) -> PathBuf {
    if temp_dir.trim().is_empty() {
        record_root(resources_path).join("tmp")
    } else {
        PathBuf::from(temp_dir.trim())
    }
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

pub async fn info_by_filename(
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

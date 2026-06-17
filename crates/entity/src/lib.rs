//! SeaORM entities ported from the Go `model/*.go` definitions.
//!
//! Field names are snake_case (matching GORM's default column naming) and JSON
//! keys are reproduced exactly via `#[serde(rename = ...)]` where they differ,
//! so the wire format is identical to the original server.

pub mod datetime;

pub mod active_connection;
pub mod address_book;
pub mod address_book_collection;
pub mod address_book_collection_rule;
pub mod audit_conn;
pub mod audit_file;
pub mod deployment_event;
pub mod deployment_token;
pub mod device_group;
pub mod group;
pub mod login_log;
pub mod message;
pub mod message_read;
pub mod oauth;
pub mod peer;
pub mod record_file;
pub mod server_cmd;
pub mod share_record;
pub mod strategy;
pub mod strategy_assignment;
pub mod tag;
pub mod user;
pub mod user_third;
pub mod user_token;
pub mod version;

use serde::Serialize;

/// Mirrors `model.Pagination` (embedded in every `*List` response).
#[derive(Debug, Clone, Serialize)]
pub struct Pagination {
    pub page: i64,
    pub total: i64,
    pub page_size: i64,
}

/// Generic list envelope: `{ "list": [...], "page": .., "total": .., "page_size": .. }`.
#[derive(Debug, Clone, Serialize)]
pub struct ListResult<T: Serialize> {
    pub list: Vec<T>,
    pub page: i64,
    pub total: i64,
    pub page_size: i64,
}

impl<T: Serialize> ListResult<T> {
    pub fn new(list: Vec<T>, page: i64, page_size: i64, total: i64) -> Self {
        Self {
            list,
            page,
            total,
            page_size,
        }
    }
}

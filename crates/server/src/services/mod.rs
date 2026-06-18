//! Service layer — ports of the Go `service/*.go` files. Each module holds the
//! data-access + business logic for one domain, operating on a SeaORM
//! `DatabaseConnection`.

pub mod active_connection;
pub mod address_book;
pub mod audit;
pub mod deployment;
pub mod diagnostics;
pub mod group;
pub mod ldap;
pub mod login_log;
pub mod login_security;
pub mod message;
pub mod oauth;
pub mod overview;
pub mod peer;
pub mod record_file;
pub mod record_storage;
pub mod server_cmd;
pub mod share_record;
pub mod strategy;
pub mod tag;
pub mod user;
pub mod webhook;

use chrono::Utc;
use sea_orm::prelude::DateTime;

/// Current time as the entities' optional timestamp (≈ GORM auto timestamps).
pub fn now() -> Option<DateTime> {
    Some(Utc::now().naive_utc())
}

/// Clamp page/page_size the way `service.Paginate` does (default 1 / 10).
pub fn paginate(page: u64, page_size: u64) -> (u64, u64) {
    let page = if page == 0 { 1 } else { page };
    let page_size = if page_size == 0 { 10 } else { page_size };
    (page, page_size)
}

//! Service layer — ports of the Go `service/*.go` files. Each module holds the
//! data-access + business logic for one domain, operating on a SeaORM
//! `DatabaseConnection`.

pub mod active_connection;
pub mod address_book;
pub mod audit;
pub mod audit_alarm;
pub mod audit_receipt;
pub mod deployment;
pub mod diagnostics;
pub mod group;
pub mod geo_relay;
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
    let max = i64::MAX as u64;
    let page_size = page_size.min(max);
    let page = page.min(max).min(max / page_size + 1);
    (page, page_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_preserves_defaults_and_bounds_database_offsets() {
        assert_eq!(paginate(0, 0), (1, 10));
        assert_eq!(paginate(3, 20), (3, 20));
        for (page, page_size) in [(u64::MAX, u64::MAX), (u64::MAX, 1), (u64::MAX, 10)] {
            let (page, page_size) = paginate(page, page_size);
            let offset = (page - 1).checked_mul(page_size).unwrap();
            assert!(offset > 0 && offset <= i64::MAX as u64);
            assert!(page <= i64::MAX as u64 && page_size <= i64::MAX as u64);
        }
    }
}

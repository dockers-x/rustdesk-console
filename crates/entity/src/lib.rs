//! SeaORM entities ported from the Go `model/*.go` definitions.
//!
//! Field names are snake_case (matching GORM's default column naming) and JSON
//! keys are reproduced exactly via `#[serde(rename = ...)]` where they differ,
//! so the wire format is identical to the original server.

pub mod datetime;
pub mod developer_key;

pub mod active_connection;
pub mod address_book;
pub mod address_book_collection;
pub mod address_book_collection_rule;
pub mod address_book_trash;
pub mod audit_conn;
pub mod audit_file;
pub mod audit_alarm;
pub mod audit_receipt;
pub mod client_download_portal;
pub mod blocklist_rule;
pub mod console_audit;
pub mod connection_grant;
pub mod deployment_event;
pub mod deployment_token;
pub mod control_role;
pub mod device_admission;
pub mod device_group;
pub mod routing_settings;
pub mod device_presence_event;
pub mod device_presence_state;
pub mod enrollment_invitation;
pub mod geo_mmdb_download;
pub mod group;
pub mod ldap_identity;
pub mod ldap_setting;
pub mod login_log;
pub mod login_verification;
pub mod message;
pub mod message_read;
pub mod notification_template;
pub mod notification_template_binding;
pub mod oauth;
pub mod peer;
pub mod record_file;
pub mod server_cmd;
pub mod share_record;
pub mod smtp_email_config;
pub mod strategy;
pub mod strategy_assignment;
pub mod strategy_delivery;
pub mod system_setting;
pub mod tag;
pub mod trusted_browser;
pub mod trusted_login_device;
pub mod user;
pub mod user_third;
pub mod user_token;
pub mod version;
pub mod web_guest_grant;
pub mod web_guest_ticket;
pub mod webhook_delivery;
pub mod webhook_subscription;

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

pub mod totp_counter;

//! Custom timestamp (de)serialization matching the Go `custom_types.AutoTime`,
//! which renders timestamps as `"2006-01-02 15:04:05"` and a zero time as `null`.

use chrono::NaiveDateTime;
use serde::Serializer;

pub const FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Serialize `Option<NaiveDateTime>` as `"YYYY-MM-DD HH:MM:SS"` or `null`.
pub fn serialize_opt<S>(value: &Option<NaiveDateTime>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(dt) => serializer.serialize_str(&dt.format(FORMAT).to_string()),
        None => serializer.serialize_none(),
    }
}

/// A newtype kept for places that want the formatted string directly.
pub fn format(dt: &NaiveDateTime) -> String {
    dt.format(FORMAT).to_string()
}

/// Helper used by entities: serialize the optional timestamp field.
pub mod opt_auto_time {
    use super::*;

    pub fn serialize<S>(value: &Option<NaiveDateTime>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_opt(value, serializer)
    }
}

/// Marker so `Serialize` derive can be used on entities while still routing
/// timestamps through the Go-compatible format.
pub trait FormattedTimestamps {}

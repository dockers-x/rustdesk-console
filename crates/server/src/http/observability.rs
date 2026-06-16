//! Machine-readable health and Prometheus metrics endpoints.

use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde::Serialize;

use entity::{active_connection, audit_conn, audit_file, login_log, peer, user};

use crate::state::AppState;

const ONLINE_WINDOW_SECONDS: i64 = 90;

#[derive(Debug, Clone, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: String,
    start_time: String,
    uptime_seconds: i64,
    database: ComponentHealth,
}

#[derive(Debug, Clone, Serialize)]
struct ComponentHealth {
    status: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MetricsSnapshot {
    version: String,
    uptime_seconds: i64,
    db_up: bool,
    users_total: u64,
    peers_total: u64,
    peers_online: u64,
    active_connections_total: u64,
    audit_conns_total: u64,
    audit_files_total: u64,
    login_logs_total: u64,
}

pub async fn health(State(state): State<AppState>) -> Response {
    let db_up = state.db.ping().await.is_ok();
    let status = if db_up { "ok" } else { "degraded" };
    let status_code = if db_up {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(HealthResponse {
            status,
            version: state.version.as_str().to_owned(),
            start_time: state.start_time.as_str().to_owned(),
            uptime_seconds: uptime_seconds(state.start_time.as_str()),
            database: ComponentHealth {
                status: if db_up { "ok" } else { "error" },
            },
        }),
    )
        .into_response()
}

pub async fn metrics(State(state): State<AppState>) -> Response {
    let snapshot = collect_metrics(&state).await.unwrap_or_else(|err| {
        tracing::warn!("failed to collect metrics: {err}");
        MetricsSnapshot {
            version: state.version.as_str().to_owned(),
            uptime_seconds: uptime_seconds(state.start_time.as_str()),
            db_up: false,
            ..Default::default()
        }
    });

    let mut response = format_metrics(&snapshot).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    response
}

async fn collect_metrics(state: &AppState) -> Result<MetricsSnapshot, sea_orm::DbErr> {
    state.db.ping().await?;

    let now = Utc::now().timestamp();
    let online_since = now - ONLINE_WINDOW_SECONDS;

    Ok(MetricsSnapshot {
        version: state.version.as_str().to_owned(),
        uptime_seconds: uptime_seconds(state.start_time.as_str()),
        db_up: true,
        users_total: user::Entity::find().count(&state.db).await?,
        peers_total: peer::Entity::find().count(&state.db).await?,
        peers_online: peer::Entity::find()
            .filter(peer::Column::LastOnlineTime.gte(online_since))
            .count(&state.db)
            .await?,
        active_connections_total: active_connection::Entity::find().count(&state.db).await?,
        audit_conns_total: audit_conn::Entity::find().count(&state.db).await?,
        audit_files_total: audit_file::Entity::find().count(&state.db).await?,
        login_logs_total: login_log::Entity::find().count(&state.db).await?,
    })
}

fn uptime_seconds(start_time: &str) -> i64 {
    let Ok(started_at) = chrono::DateTime::parse_from_rfc3339(start_time) else {
        return 0;
    };
    (Utc::now() - started_at.with_timezone(&Utc))
        .num_seconds()
        .max(0)
}

fn format_metrics(snapshot: &MetricsSnapshot) -> String {
    let version = escape_label_value(&snapshot.version);
    let db_up = u8::from(snapshot.db_up);

    format!(
        concat!(
            "# HELP rustdesk_console_info RustDesk Console build information.\n",
            "# TYPE rustdesk_console_info gauge\n",
            "rustdesk_console_info{{version=\"{version}\"}} 1\n",
            "# HELP rustdesk_console_uptime_seconds Seconds since the API server started.\n",
            "# TYPE rustdesk_console_uptime_seconds gauge\n",
            "rustdesk_console_uptime_seconds {uptime_seconds}\n",
            "# HELP rustdesk_console_db_up Database connectivity status, 1 for up and 0 for down.\n",
            "# TYPE rustdesk_console_db_up gauge\n",
            "rustdesk_console_db_up {db_up}\n",
            "# HELP rustdesk_console_users_total Total users.\n",
            "# TYPE rustdesk_console_users_total gauge\n",
            "rustdesk_console_users_total {users_total}\n",
            "# HELP rustdesk_console_peers_total Total registered peers.\n",
            "# TYPE rustdesk_console_peers_total gauge\n",
            "rustdesk_console_peers_total {peers_total}\n",
            "# HELP rustdesk_console_peers_online Peers seen in the last {online_window_seconds} seconds.\n",
            "# TYPE rustdesk_console_peers_online gauge\n",
            "rustdesk_console_peers_online {peers_online}\n",
            "# HELP rustdesk_console_active_connections_total Active connection rows reported by heartbeats.\n",
            "# TYPE rustdesk_console_active_connections_total gauge\n",
            "rustdesk_console_active_connections_total {active_connections_total}\n",
            "# HELP rustdesk_console_audit_conns_total Total connection audit rows.\n",
            "# TYPE rustdesk_console_audit_conns_total gauge\n",
            "rustdesk_console_audit_conns_total {audit_conns_total}\n",
            "# HELP rustdesk_console_audit_files_total Total file audit rows.\n",
            "# TYPE rustdesk_console_audit_files_total gauge\n",
            "rustdesk_console_audit_files_total {audit_files_total}\n",
            "# HELP rustdesk_console_login_logs_total Total login log rows.\n",
            "# TYPE rustdesk_console_login_logs_total gauge\n",
            "rustdesk_console_login_logs_total {login_logs_total}\n",
        ),
        version = version,
        uptime_seconds = snapshot.uptime_seconds,
        db_up = db_up,
        users_total = snapshot.users_total,
        peers_total = snapshot.peers_total,
        online_window_seconds = ONLINE_WINDOW_SECONDS,
        peers_online = snapshot.peers_online,
        active_connections_total = snapshot.active_connections_total,
        audit_conns_total = snapshot.audit_conns_total,
        audit_files_total = snapshot.audit_files_total,
        login_logs_total = snapshot.login_logs_total,
    )
}

fn escape_label_value(value: &str) -> String {
    value
        .replace('\\', r"\\")
        .replace('\n', r"\n")
        .replace('"', r#"\""#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_format_includes_prometheus_gauges() {
        let snapshot = MetricsSnapshot {
            version: "0.2.3".to_string(),
            uptime_seconds: 42,
            db_up: true,
            users_total: 2,
            peers_total: 3,
            peers_online: 1,
            active_connections_total: 4,
            audit_conns_total: 5,
            audit_files_total: 6,
            login_logs_total: 7,
        };

        let body = format_metrics(&snapshot);

        assert!(body.contains("rustdesk_console_info{version=\"0.2.3\"} 1"));
        assert!(body.contains("rustdesk_console_db_up 1"));
        assert!(body.contains("rustdesk_console_peers_online 1"));
        assert!(body.contains("rustdesk_console_login_logs_total 7"));
    }

    #[test]
    fn metric_label_values_are_escaped() {
        assert_eq!(escape_label_value("a\"b\\c\nd"), r#"a\"b\\c\nd"#);
    }

    #[test]
    fn invalid_start_time_has_zero_uptime() {
        assert_eq!(uptime_seconds("not-a-date"), 0);
    }
}

//! Runtime diagnostics for operators. Checks are read-only and safe to run from
//! the admin UI.

use std::time::Instant;

use sea_orm::DatabaseConnection;
use serde::Serialize;
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

use crate::config::Config;
use crate::support::webclient_config::WebClientConfig;

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsReport {
    pub generated_at: String,
    pub summary: DiagnosticsSummary,
    pub checks: Vec<DiagnosticCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsSummary {
    pub ok: usize,
    pub warning: usize,
    pub error: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DiagnosticCheck {
    pub key: String,
    pub label: String,
    pub status: DiagnosticStatus,
    pub message: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStatus {
    Ok,
    Warning,
    Error,
}

pub async fn run(
    db: &DatabaseConnection,
    config: &Config,
    webclient: &WebClientConfig,
    version: &str,
    start_time: &str,
) -> DiagnosticsReport {
    let mut checks = Vec::new();
    checks.push(check_database(db).await);
    checks.push(check_api_runtime(version, start_time));
    checks.push(check_webclient_config(webclient));
    checks.push(
        check_tcp_port(
            "id_command_port",
            "ID command port",
            normalized_id_command_port(config),
        )
        .await,
    );
    checks.push(
        check_tcp_port(
            "relay_command_port",
            "Relay command port",
            normalize_port(config.admin.relay_server_port),
        )
        .await,
    );

    let summary = summarize(&checks);
    DiagnosticsReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        summary,
        checks,
    }
}

async fn check_database(db: &DatabaseConnection) -> DiagnosticCheck {
    let started = Instant::now();
    let result = db.ping().await;
    DiagnosticCheck {
        key: "database".to_string(),
        label: "Database".to_string(),
        status: if result.is_ok() {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Error
        },
        message: result
            .map(|_| "Database connection is healthy".to_string())
            .unwrap_or_else(|e| format!("Database ping failed: {e}")),
        elapsed_ms: started.elapsed().as_millis(),
    }
}

fn check_api_runtime(version: &str, start_time: &str) -> DiagnosticCheck {
    let started = Instant::now();
    let missing = [("version", version), ("start_time", start_time)]
        .into_iter()
        .filter_map(|(name, value)| value.trim().is_empty().then_some(name))
        .collect::<Vec<_>>();
    DiagnosticCheck {
        key: "api_runtime".to_string(),
        label: "API runtime".to_string(),
        status: if missing.is_empty() {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Warning
        },
        message: if missing.is_empty() {
            format!("API runtime is available, version {version}")
        } else {
            format!("Missing runtime fields: {}", missing.join(", "))
        },
        elapsed_ms: started.elapsed().as_millis(),
    }
}

fn check_webclient_config(cfg: &WebClientConfig) -> DiagnosticCheck {
    let started = Instant::now();
    let missing = [
        ("id-server", cfg.id_server.as_str()),
        ("relay-server", cfg.relay_server.as_str()),
        ("api-server", cfg.api_server.as_str()),
        ("public-key", cfg.key.as_str()),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.trim().is_empty().then_some(name))
    .collect::<Vec<_>>();

    DiagnosticCheck {
        key: "webclient_config".to_string(),
        label: "WebClient config".to_string(),
        status: if missing.is_empty() {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Warning
        },
        message: if missing.is_empty() {
            "WebClient defaults are complete".to_string()
        } else {
            format!("Missing values: {}", missing.join(", "))
        },
        elapsed_ms: started.elapsed().as_millis(),
    }
}

pub async fn check_tcp_port(key: &str, label: &str, port: Option<u16>) -> DiagnosticCheck {
    let started = Instant::now();
    let Some(port) = port else {
        return DiagnosticCheck {
            key: key.to_string(),
            label: label.to_string(),
            status: DiagnosticStatus::Warning,
            message: "Port is not configured".to_string(),
            elapsed_ms: started.elapsed().as_millis(),
        };
    };

    let result = timeout(
        Duration::from_millis(800),
        TcpStream::connect(("127.0.0.1", port)),
    )
    .await;
    let (status, message) = match result {
        Ok(Ok(_)) => (
            DiagnosticStatus::Ok,
            format!("127.0.0.1:{port} accepts TCP connections"),
        ),
        Ok(Err(e)) => (
            DiagnosticStatus::Error,
            format!("127.0.0.1:{port} is not reachable: {e}"),
        ),
        Err(_) => (
            DiagnosticStatus::Error,
            format!("127.0.0.1:{port} timed out"),
        ),
    };

    DiagnosticCheck {
        key: key.to_string(),
        label: label.to_string(),
        status,
        message,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

pub fn normalized_id_command_port(config: &Config) -> Option<u16> {
    normalize_port(config.admin.id_server_port - 1)
}

pub fn normalize_port(port: i32) -> Option<u16> {
    if (1..=u16::MAX as i32).contains(&port) {
        Some(port as u16)
    } else {
        None
    }
}

pub fn summarize(checks: &[DiagnosticCheck]) -> DiagnosticsSummary {
    let mut summary = DiagnosticsSummary {
        ok: 0,
        warning: 0,
        error: 0,
    };
    for check in checks {
        match check.status {
            DiagnosticStatus::Ok => summary.ok += 1,
            DiagnosticStatus::Warning => summary.warning += 1,
            DiagnosticStatus::Error => summary.error += 1,
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_port_rejects_invalid_values() {
        assert_eq!(normalize_port(0), None);
        assert_eq!(normalize_port(-1), None);
        assert_eq!(normalize_port(65536), None);
        assert_eq!(normalize_port(21117), Some(21117));
    }

    #[test]
    fn summarize_counts_statuses() {
        let checks = vec![
            DiagnosticCheck {
                key: "a".into(),
                label: "A".into(),
                status: DiagnosticStatus::Ok,
                message: String::new(),
                elapsed_ms: 0,
            },
            DiagnosticCheck {
                key: "b".into(),
                label: "B".into(),
                status: DiagnosticStatus::Warning,
                message: String::new(),
                elapsed_ms: 0,
            },
            DiagnosticCheck {
                key: "c".into(),
                label: "C".into(),
                status: DiagnosticStatus::Error,
                message: String::new(),
                elapsed_ms: 0,
            },
        ];

        let summary = summarize(&checks);
        assert_eq!(summary.ok, 1);
        assert_eq!(summary.warning, 1);
        assert_eq!(summary.error, 1);
    }
}

//! Logging setup, mirroring the Go `logger` config (path + level). When
//! `logger.path` is set, logs are written to that file; otherwise to stdout.

use std::path::Path;

use tracing_subscriber::EnvFilter;

use crate::config::Config;

fn level_filter(level: &str) -> String {
    let lvl = match level.to_lowercase().as_str() {
        "trace" => "trace",
        "debug" => "debug",
        "warn" | "warning" => "warn",
        "error" | "fatal" => "error",
        _ => "info",
    };
    format!("{lvl},sqlx=warn")
}

/// Initialize tracing. Respects `RUST_LOG` if set, else uses `logger.level`.
pub fn init(config: &Config) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level_filter(&config.logger.level)));

    let path = config.logger.path.trim();
    if path.is_empty() {
        tracing_subscriber::fmt().with_env_filter(filter).init();
        return;
    }

    let p = Path::new(path);
    let dir = p.parent().filter(|d| !d.as_os_str().is_empty());
    if let Some(dir) = dir {
        let _ = std::fs::create_dir_all(dir);
    }
    let file_name = p
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "log.txt".to_string());
    let dir = dir.map(|d| d.to_path_buf()).unwrap_or_else(|| ".".into());

    let appender = tracing_appender::rolling::never(dir, file_name);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(appender)
        .with_ansi(false)
        .init();
}

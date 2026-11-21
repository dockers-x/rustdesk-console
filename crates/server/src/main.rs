//! RustDesk API server entry point + CLI, ports `cmd/apimain.go`.
//!
//! Some constants, helpers and struct fields are wired ahead of the later
//! phases that consume them (OAuth/LDAP, full admin CRUD), so dead-code is
//! allowed crate-wide for this phased deliverable.
#![allow(dead_code)]

mod assets;
mod bootstrap;
mod config;
mod error;
mod features;
mod http;
mod i18n;
mod logging;
mod operator_cli;
mod services;
mod state;
mod support;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = operator_cli::Cli::parse();
    if let Some(command) = cli.command {
        return command.run(&cli.config).await;
    }

    let cfg = match config::init(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    logging::init(&cfg);
    if let Err(e) = serve(cfg, PathBuf::from(cli.config)).await {
        tracing::error!("{e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

async fn serve(cfg: config::Config, config_path: PathBuf) -> anyhow::Result<()> {
    let addr_str = if cfg.gin.api_addr.is_empty() {
        "0.0.0.0:21114".to_string()
    } else {
        cfg.gin.api_addr.clone()
    };
    tracing::info!("API SERVER START");
    let state = bootstrap::build_state(cfg, config_path).await?;
    services::server_cmd::spawn_saved_relay_pool_sync(state.db.clone(), state.config.clone());
    services::geo_relay::cleanup_stale_handoffs(&state.config);
    services::geo_relay::spawn_startup_sync(state.db.clone(), state.config.clone());
    services::geo_relay::spawn_mmdb_update_worker(state.db.clone(), state.config.clone());
    services::webhook::spawn_presence_worker(state.db.clone());
    let addr: SocketAddr = addr_str.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    services::console_audit::spawn_retention_worker(state.db.clone());
    let app = http::router::build(state);
    tracing::info!("listening on http://{addr}");
    tracing::info!(
        target: "security_audit",
        event = "console.started",
        version = env!("CARGO_PKG_VERSION"),
        revision = option_env!("RUSTDESK_CONSOLE_BUILD_REVISION").unwrap_or("unknown"),
        listen = %addr,
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

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
mod i18n;
mod http;
mod services;
mod state;
mod support;

use std::net::SocketAddr;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rustdesk-api-server", about = "RUSTDESK API SERVER (Rust)")]
struct Cli {
    /// Path to the config file.
    #[arg(short, long, default_value = "./conf/config.yaml")]
    config: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Reset the admin (user id 1) password.
    ResetAdminPwd { pwd: String },
    /// Reset a user's password by id.
    ResetPwd { user_id: i32, pwd: String },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let cfg = match config::init(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };

    let result = match cli.command {
        Some(Command::ResetAdminPwd { pwd }) => reset_password(&cfg, 1, &pwd).await,
        Some(Command::ResetPwd { user_id, pwd }) => {
            if user_id <= 0 {
                tracing::warn!("userId must be greater than 0!");
                return;
            }
            reset_password(&cfg, user_id, &pwd).await
        }
        None => serve(cfg).await,
    };

    if let Err(e) = result {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}

async fn serve(cfg: config::Config) -> anyhow::Result<()> {
    let addr_str = if cfg.gin.api_addr.is_empty() {
        "0.0.0.0:21114".to_string()
    } else {
        cfg.gin.api_addr.clone()
    };
    tracing::info!("API SERVER START");
    let state = bootstrap::build_state(cfg).await?;
    let app = http::router::build(state);

    let addr: SocketAddr = addr_str.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Reset a password without running the full migrate/seed/serve path.
async fn reset_password(cfg: &config::Config, user_id: i32, pwd: &str) -> anyhow::Result<()> {
    let db = bootstrap::connect(cfg).await?;
    match services::user::info_by_id(&db, user_id).await? {
        Some(u) if u.id != 0 => {
            services::user::update_password(&db, &u, pwd)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            tracing::info!("reset password success!");
        }
        _ => tracing::warn!("user not found!"),
    }
    Ok(())
}

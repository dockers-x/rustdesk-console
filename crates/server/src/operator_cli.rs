//! Local version, configuration, database and account recovery commands.

use std::io::{self, IsTerminal, Read, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::{json, Value};

use crate::bootstrap::{self, SchemaError, SchemaState, DATABASE_VERSION};
use crate::config::{self, Config};
use crate::{logging, services, support};

#[derive(Parser)]
#[command(name = "rustdesk-console", version, about = "RustDesk Console")]
pub(crate) struct Cli {
    /// Path to the config file.
    #[arg(short, long, global = true, default_value = "./conf/config.yaml")]
    pub config: String,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Show build and database schema information without reading the config.
    Version(Output),
    /// Inspect the local configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Inspect, migrate or back up the database.
    Database {
        #[command(subcommand)]
        command: DatabaseCommand,
    },
    /// Reset the admin (user id 1) password.
    #[command(name = "reset-admin-pwd")]
    AdminPassword(PasswordInput),
    /// Reset a user's password by id.
    #[command(name = "reset-pwd")]
    Password {
        user_id: i32,
        #[command(flatten)]
        password: PasswordInput,
    },
    /// Reset a lost TOTP factor and revoke the user's sessions and challenges.
    #[command(name = "reset-totp")]
    Totp { user_id: i32 },
}

#[derive(Subcommand)]
pub(crate) enum ConfigCommand {
    /// Validate the configuration and file references without connecting.
    Validate(Output),
}

#[derive(Subcommand)]
pub(crate) enum DatabaseCommand {
    /// Read the installed schema version without changing the database.
    Status(Output),
    /// Apply the database schema without starting Console or seeding accounts.
    Migrate(Output),
    /// Write a consistent SQLite snapshot and manifest to a new directory.
    Backup {
        /// New directory for the backup files. Its parent must exist.
        #[arg(long, value_name = "DIRECTORY")]
        output: PathBuf,
        #[command(flatten)]
        format: Output,
    },
    /// Upload an existing backup archive to every configured remote target.
    UploadBackup {
        #[arg(value_name = "ARCHIVE")]
        archive: PathBuf,
        #[command(flatten)]
        format: Output,
    },
}

#[derive(Args)]
pub(crate) struct Output {
    /// Print a JSON object to stdout.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
#[group(id = "password_source", required = true, multiple = false)]
pub(crate) struct PasswordInput {
    /// Legacy positional password. Prefer --password-stdin or --password-file.
    pwd: Option<String>,
    /// Read one password from piped stdin.
    #[arg(long)]
    password_stdin: bool,
    /// Read one password from a regular owner-only file (Unix).
    #[arg(long, value_name = "PATH")]
    password_file: Option<PathBuf>,
}

impl PasswordInput {
    fn read(self) -> anyhow::Result<String> {
        if let Some(pwd) = self.pwd {
            return Ok(pwd);
        }
        if let Some(path) = self.password_file {
            return read_password_file(&path);
        }

        let stdin = io::stdin();
        anyhow::ensure!(
            !stdin.is_terminal(),
            "--password-stdin requires a pipe or input redirection"
        );
        read_password(stdin.lock())
    }
}

const MIN_PASSWORD_BYTES: usize = 4;
const MAX_PASSWORD_BYTES: usize = 72;

fn read_password(input: impl Read) -> anyhow::Result<String> {
    let mut bytes = Vec::new();
    input
        .take((MAX_PASSWORD_BYTES + 3) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("could not read password input"))?;
    if bytes.ends_with(b"\r\n") {
        bytes.truncate(bytes.len() - 2);
    } else if bytes.ends_with(b"\n") {
        bytes.pop();
    }
    anyhow::ensure!(
        (MIN_PASSWORD_BYTES..=MAX_PASSWORD_BYTES).contains(&bytes.len())
            && !bytes.iter().any(|byte| matches!(byte, b'\r' | b'\n' | b'\0')),
        "password input must contain one line of {MIN_PASSWORD_BYTES} to {MAX_PASSWORD_BYTES} UTF-8 bytes"
    );
    String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("password input is not valid UTF-8"))
}

#[cfg(unix)]
fn read_password_file(path: &std::path::Path) -> anyhow::Result<String> {
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let before = fs::symlink_metadata(path)
        .map_err(|_| anyhow::anyhow!("could not inspect password file"))?;
    anyhow::ensure!(
        before.is_file() && before.mode() & 0o077 == 0,
        "password file must be a regular file with no group or other permissions"
    );
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| anyhow::anyhow!("could not open password file safely"))?;
    let after = file
        .metadata()
        .map_err(|_| anyhow::anyhow!("could not inspect open password file"))?;
    anyhow::ensure!(
        after.is_file()
            && after.mode() & 0o077 == 0
            && (after.dev(), after.ino()) == (before.dev(), before.ino()),
        "password file changed while opening"
    );
    read_password(file)
}

#[cfg(not(unix))]
fn read_password_file(_path: &std::path::Path) -> anyhow::Result<String> {
    anyhow::bail!("--password-file is supported on Unix; use --password-stdin")
}

impl Command {
    pub(crate) async fn run(self, config_path: &str) -> ExitCode {
        let result = match self {
            Self::Version(output) => version(output),
            Self::Config {
                command: ConfigCommand::Validate(output),
            } => validate(config_path, output),
            Self::Database {
                command: DatabaseCommand::Status(output),
            } => status(config_path, output).await,
            Self::Database {
                command: DatabaseCommand::Migrate(output),
            } => migrate(config_path, output).await,
            Self::Database {
                command: DatabaseCommand::Backup { output, format },
            } => backup(config_path, &output, format).await,
            Self::Database {
                command: DatabaseCommand::UploadBackup { archive, format },
            } => upload_backup(config_path, &archive, format).await,
            Self::AdminPassword(password) => return password_reset(config_path, 1, password).await,
            Self::Password { user_id, password } => {
                if user_id <= 0 {
                    eprintln!("userId must be greater than 0!");
                    return ExitCode::SUCCESS;
                }
                return password_reset(config_path, user_id, password).await;
            }
            Self::Totp { user_id } => {
                return run_recovery(config_path, user_id, Recovery::Totp).await;
            }
        };
        match result {
            Ok(code) => code.into(),
            Err(error) => {
                eprintln!("could not write command output: {error}");
                ExitCode::FAILURE
            }
        }
    }
}

const EXIT_CONFIG: u8 = 3;
const EXIT_DATABASE: u8 = 4;
const EXIT_SCHEMA: u8 = 5;

fn emit(output: Output, mut data: Value, message: &str, code: u8) -> io::Result<u8> {
    if output.json {
        data["schema_version"] = 1.into();
        let mut stdout = io::stdout().lock();
        serde_json::to_writer(&mut stdout, &data)?;
        writeln!(stdout)?;
    } else if code == 0 {
        writeln!(io::stdout().lock(), "{message}")?;
    } else {
        writeln!(io::stderr().lock(), "{message}")?;
    }
    Ok(code)
}

fn version(output: Output) -> io::Result<u8> {
    let version = env!("CARGO_PKG_VERSION");
    let revision = option_env!("RUSTDESK_CONSOLE_BUILD_REVISION").unwrap_or("unknown");
    emit(
        output,
        json!({
            "component": "rustdesk-console",
            "version": version,
            "revision": revision,
            "database_schema": DATABASE_VERSION,
        }),
        &format!("rustdesk-console {version} ({revision}), database schema {DATABASE_VERSION}"),
        0,
    )
}

#[derive(Serialize)]
struct ConfigIssue {
    field: &'static str,
    message: &'static str,
}

fn validate(config_path: &str, output: Output) -> io::Result<u8> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut database_type = None;
    match config::init(config_path) {
        Ok(config) => {
            match bootstrap::validate_database_options(&config) {
                Ok(kind) => database_type = Some(kind),
                Err((field, message)) => errors.push(ConfigIssue { field, message }),
            }
            validate_config(&config, &mut errors, &mut warnings);
        }
        Err(_) => errors.push(ConfigIssue {
            field: "configuration",
            message: "could not read or parse the configuration",
        }),
    }
    let mut message = if errors.is_empty() {
        "configuration is valid".to_owned()
    } else {
        "configuration is invalid".to_owned()
    };
    for issue in &errors {
        message.push_str(&format!("\n{}: {}", issue.field, issue.message));
    }
    for issue in &warnings {
        message.push_str(&format!("\nwarning {}: {}", issue.field, issue.message));
    }
    emit(
        output,
        json!({
            "valid": errors.is_empty(),
            "database_type": database_type,
            "errors": errors,
            "warnings": warnings,
        }),
        &message,
        if errors.is_empty() { 0 } else { EXIT_CONFIG },
    )
}

fn validate_config(
    config: &Config,
    errors: &mut Vec<ConfigIssue>,
    warnings: &mut Vec<ConfigIssue>,
) {
    let mut error = |field, message| errors.push(ConfigIssue { field, message });
    if !config.gin.api_addr.is_empty() && config.gin.api_addr.parse::<SocketAddr>().is_err() {
        error(
            "gin.api-addr",
            "API address must be a socket address including a port",
        );
    }
    if support::jwt::Jwt::try_new(&config.jwt.key, config.jwt.expire_duration()).is_err() {
        error(
            "jwt.expire-duration",
            "JWT expiration is outside the supported range",
        );
    }
    if support::record_storage_config::validate_active(&config.record_storage).is_err() {
        error(
            "record-storage",
            "active recording storage backend is missing required settings",
        );
    }
    if config.ldap.enable && services::ldap::validate_with_references(&config.ldap).is_err() {
        error(
            "ldap",
            "enabled LDAP configuration or its certificate authority is invalid",
        );
    }
    let mut warning = |field, message| warnings.push(ConfigIssue { field, message });
    if config.rustdesk.key.is_empty() && !config.rustdesk.key_file.is_empty() {
        warning(
            "rustdesk.key-file",
            "optional public key file is empty or unreadable",
        );
    }
    if config.admin.hello.is_empty()
        && !config.admin.hello_file.is_empty()
        && std::fs::read_to_string(&config.admin.hello_file).is_err()
    {
        warning("admin.hello-file", "optional welcome file is unreadable");
    }
}

fn config_failed(output: Output) -> io::Result<u8> {
    emit(
        output,
        json!({"error": {"code": "CONFIG_INVALID", "message": "configuration is invalid"}}),
        "configuration is invalid",
        EXIT_CONFIG,
    )
}

async fn status(config_path: &str, output: Output) -> io::Result<u8> {
    let Ok(config) = config::init(config_path) else {
        return config_failed(output);
    };
    let Ok(database_type) = bootstrap::validate_database_options(&config) else {
        return config_failed(output);
    };
    let status = bootstrap::database_status(&config).await;
    let (code, error) = match status.state {
        SchemaState::NewerThanBinary => (
            EXIT_SCHEMA,
            Some((
                "DATABASE_SCHEMA_NEWER_THAN_BINARY",
                "database schema is newer than this binary",
            )),
        ),
        SchemaState::Invalid if status.connection_ok => (
            EXIT_SCHEMA,
            Some((
                "DATABASE_SCHEMA_INVALID",
                "database schema metadata is invalid",
            )),
        ),
        SchemaState::Invalid => (
            EXIT_DATABASE,
            Some(("DATABASE_CONNECTION_FAILED", "database connection failed")),
        ),
        _ => (0, None),
    };
    let mut data = json!({
        "database_type": database_type,
        "connection_ok": status.connection_ok,
        "installed_schema": status.installed_schema,
        "target_schema": DATABASE_VERSION,
        "state": status.state,
        "migration_required": matches!(status.state, SchemaState::Empty | SchemaState::UpgradeRequired),
        "safe_to_start": matches!(status.state, SchemaState::Current),
    });
    if let Some((reason, message)) = error {
        data["error"] = json!({"code": reason, "message": message});
    }
    let installed = status
        .installed_schema
        .map_or_else(|| "none".to_owned(), |version| version.to_string());
    emit(
        output,
        data,
        &format!(
            "{database_type} database schema {} (installed {installed}, target {DATABASE_VERSION})",
            status.state
        ),
        code,
    )
}

async fn upload_backup(config_path: &str, archive: &std::path::Path, output: Output) -> io::Result<u8> {
    let Ok(config) = config::init(config_path) else {
        return config_failed(output);
    };
    match services::remote_backup::publish_file(&config, archive).await {
        Ok(deliveries) => {
            let text = deliveries.iter().map(|delivery| {
                let outcome = if delivery.uploaded() { "uploaded" } else { "failed" };
                format!("{}: {outcome}", delivery.name)
            }).collect::<Vec<_>>().join("\n");
            let code = if deliveries.iter().all(|delivery| delivery.uploaded()) { 0 } else { EXIT_DATABASE };
            emit(output, json!({"targets": deliveries}), &text, code)
        }
        Err(error) => emit(
            output,
            json!({"error": {"code": "BACKUP_UPLOAD_FAILED", "message": error.to_string()}}),
            &format!("Could not upload archive: {error}"),
            EXIT_DATABASE,
        ),
    }
}

async fn backup(
    config_path: &str,
    destination: &std::path::Path,
    output: Output,
) -> io::Result<u8> {
    let Ok(config) = config::init(config_path) else {
        return config_failed(output);
    };
    let Ok(database_type) = bootstrap::validate_database_options(&config) else {
        return config_failed(output);
    };
    let external_tool = match database_type {
        config::DB_TYPE_MYSQL => Some("mysqldump (or mariadb-dump)"),
        config::DB_TYPE_POSTGRESQL => Some("pg_dump"),
        _ => None,
    };
    if let Some(tool) = external_tool {
        let message = format!("use {tool} to back up the {database_type} database");
        return emit(
            output,
            json!({
                "database_type": database_type,
                "error": {"code": "DATABASE_BACKUP_UNSUPPORTED", "message": message},
            }),
            &message,
            EXIT_DATABASE,
        );
    }

    match bootstrap::backup_sqlite(destination).await {
        Ok(manifest) => emit(
            output,
            json!(manifest),
            &format!("SQLite backup written to {}", destination.display()),
            0,
        ),
        Err(error) => {
            let message = format!("database backup failed: {error}");
            emit(
                output,
                json!({
                    "database_type": database_type,
                    "error": {"code": "DATABASE_BACKUP_FAILED", "message": message},
                }),
                &message,
                EXIT_DATABASE,
            )
        }
    }
}

async fn migrate(config_path: &str, output: Output) -> io::Result<u8> {
    let Ok(config) = config::init(config_path) else {
        return config_failed(output);
    };
    let Ok(database_type) = bootstrap::validate_database_options(&config) else {
        return config_failed(output);
    };
    match bootstrap::migrate_database(&config).await {
        Ok(outcome) => emit(
            output,
            json!({
                "database_type": database_type,
                "from_schema": outcome.from_schema,
                "installed_schema": outcome.installed_schema,
                "target_schema": DATABASE_VERSION,
                "state": SchemaState::Current,
                "migrated": outcome.from_schema != Some(outcome.installed_schema),
            }),
            &format!("database schema is current ({DATABASE_VERSION})"),
            0,
        ),
        Err(error) => {
            let (state, installed, code, reason, message) =
                match error.downcast_ref::<SchemaError>() {
                    Some(SchemaError::Future(version)) => (
                        SchemaState::NewerThanBinary,
                        Some(*version),
                        EXIT_SCHEMA,
                        "DATABASE_SCHEMA_NEWER_THAN_BINARY",
                        error.to_string(),
                    ),
                    Some(SchemaError::Invalid) => (
                        SchemaState::Invalid,
                        None,
                        EXIT_SCHEMA,
                        "DATABASE_SCHEMA_INVALID",
                        error.to_string(),
                    ),
                    None => (
                        SchemaState::Invalid,
                        None,
                        EXIT_DATABASE,
                        "DATABASE_MIGRATION_FAILED",
                        "database migration failed".to_owned(),
                    ),
                };
            emit(
                output,
                json!({
                    "database_type": database_type,
                    "from_schema": installed,
                    "installed_schema": installed,
                    "target_schema": DATABASE_VERSION,
                    "state": state,
                    "migrated": false,
                    "error": {"code": reason, "message": message},
                }),
                &message,
                code,
            )
        }
    }
}

enum Recovery {
    Password(String),
    Totp,
}

async fn password_reset(config_path: &str, user_id: i32, password: PasswordInput) -> ExitCode {
    match password.read() {
        Ok(pwd) => run_recovery(config_path, user_id, Recovery::Password(pwd)).await,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

async fn run_recovery(config_path: &str, user_id: i32, recovery: Recovery) -> ExitCode {
    let cfg = match config::init(config_path) {
        Ok(cfg) => cfg,
        Err(_) => {
            eprintln!("configuration is invalid");
            return ExitCode::FAILURE;
        }
    };
    logging::init(&cfg);
    if let Err(error) = recover_account(&cfg, user_id, recovery).await {
        if let Some(error) = error.downcast_ref::<SchemaError>() {
            tracing::error!("{error}");
        } else {
            tracing::error!(subject_id = user_id, "account recovery failed");
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

async fn recover_account(cfg: &Config, user_id: i32, recovery: Recovery) -> anyhow::Result<()> {
    let db = bootstrap::connect(cfg).await?;
    bootstrap::require_compatible_schema(&db).await?;
    let account = services::user::info_by_id(&db, user_id).await?;
    let event = match recovery {
        Recovery::Password(pwd) => {
            let Some(account) = account.filter(|account| account.id != 0) else {
                tracing::warn!("user not found!");
                return Ok(());
            };
            services::user::update_password(&db, &account, &pwd)
                .await
                .map_err(anyhow::Error::msg)?;
            "console.password_reset"
        }
        Recovery::Totp => {
            let Some(account) = account else {
                anyhow::bail!("user {user_id} not found");
            };
            services::login_security::reset_user_totp(&db, &account)
                .await
                .map_err(anyhow::Error::msg)?;
            "console.totp_reset"
        }
    };
    tracing::info!(
        target: "security_audit",
        event,
        actor = "local_cli",
        subject_id = user_id,
    );
    Ok(())
}

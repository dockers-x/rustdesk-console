//! Bootstrap: build the database connection, run schema sync + seed, and
//! assemble the shared [`AppState`]. Mirrors the Go
//! `InitGlobal` / `DatabaseAutoUpdate` / `Migrate` flow.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::{fmt, future::Future};

use anyhow::Context;
use sea_orm::sea_query::{ColumnDef, Expr, Table};
use sea_orm::{
    AccessMode, ActiveModelTrait, ColumnTrait, ConnectOptions, ConnectionTrait, Database,
    DatabaseBackend, DatabaseConnection, EntityTrait, IsolationLevel, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Schema, Set, Statement, TransactionTrait,
};

use entity::{
    active_connection, address_book, address_book_collection, address_book_collection_rule,
    audit_alarm, audit_conn, audit_file, audit_receipt, deployment_event, deployment_token,
    device_group, device_presence_state, group, login_log, login_verification, message,
    message_read, oauth, peer, record_file, server_cmd, share_record, smtp_email_config, strategy,
    strategy_assignment, system_setting, tag, trusted_login_device, user, user_third, user_token,
    version, webhook_delivery, webhook_subscription,
};

use crate::config::{self, Config};
use crate::i18n::I18n;
use crate::services;
use crate::state::AppState;
use crate::support::admin_config::{AdminConfigStore, AdminPanelConfig};
use crate::support::jwt::Jwt;
use crate::support::login_limiter::{LoginLimiter, SecurityPolicy};
use crate::support::record_storage_config::RecordStorageConfigStore;
use crate::support::webclient_config::{WebClientConfig, WebClientConfigStore};
use crate::support::{external_webclient::ExternalWebClient, password};

/// The schema version the binary expects (mirrors Go `DatabaseVersion`).
pub const DATABASE_VERSION: i32 = 290;

type MigrationFuture<'a> = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;
// SeaORM entity methods require a sized connection type, so instantiate the
// same migration list for a pool (MySQL) or a transaction (SQLite/PostgreSQL).
type MigrationFn<C> = for<'a> fn(&'a C, &'a Config) -> MigrationFuture<'a>;

struct SchemaMigration<C> {
    version: i32,
    name: &'static str,
    apply: MigrationFn<C>,
}

impl<C> fmt::Display for SchemaMigration<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.version, self.name)
    }
}

fn schema_migrations<C: ConnectionTrait>() -> [SchemaMigration<C>; 17] {
    [
        SchemaMigration {
            version: 271,
            name: "legacy column reconciliation",
            apply: migration_271,
        },
        SchemaMigration {
            version: 272,
            name: "address book peer notes",
            apply: migration_272,
        },
        SchemaMigration {
            version: 273,
            name: "smtp sender display names",
            apply: migration_273,
        },
        SchemaMigration {
            version: 274,
            name: "developer device group notes",
            apply: migration_274,
        },
        SchemaMigration {
            version: 275,
            name: "session authentication level",
            apply: migration_275,
        },
        SchemaMigration {
            version: 276,
            name: "notification destinations and delivery schedule",
            apply: migration_276,
        },
        SchemaMigration {
            version: 277,
            name: "shared address book notes",
            apply: migration_277,
        },
        SchemaMigration {
            version: 278,
            name: "file audit report metadata",
            apply: migration_278,
        },
        SchemaMigration {
            version: 279,
            name: "long system setting values",
            apply: migration_279,
        },
        SchemaMigration {
            version: 280,
            name: "directed group connection grants",
            apply: migration_280,
        },
        SchemaMigration {
            version: 281,
            name: "recording storage profile",
            apply: migration_281,
        },
        SchemaMigration {
            version: 282,
            name: "TOTP encryption and replay protection",
            apply: migration_282,
        },
        SchemaMigration {
            version: 283,
            name: "device lookup and active connection indexes",
            apply: migration_283,
        },
        SchemaMigration {
            version: 284,
            name: "revocable Console browser trust",
            apply: migration_284,
        },
        SchemaMigration {
            version: 285,
            name: "deployment OIDC identity and verification bindings",
            apply: migration_285,
        },
        SchemaMigration {
            version: 286,
            name: "GeoIP download sources",
            apply: migration_286,
        },
        SchemaMigration {
            version: 290,
            name: "one-time enrollment invitations",
            apply: migration_290,
        },
    ]
}

const SQLITE_DATABASE_PATH: &str = "./data/rustdeskapi.db";
const FIRST_RUN_SEED_PENDING: &str = "bootstrap.seed_pending";

fn database_url(config: &Config) -> String {
    match config.db.r#type.as_str() {
        config::DB_TYPE_MYSQL => format!(
            "mysql://{}:{}@{}/{}",
            urlencoding::encode(&config.mysql.username),
            urlencoding::encode(&config.mysql.password),
            config.mysql.addr,
            config.mysql.dbname
        ),
        config::DB_TYPE_POSTGRESQL => format!(
            "postgres://{}:{}@{}:{}/{}?sslmode={}",
            urlencoding::encode(&config.postgresql.user),
            urlencoding::encode(&config.postgresql.password),
            config.postgresql.host,
            config.postgresql.port,
            config.postgresql.dbname,
            if config.postgresql.sslmode.is_empty() {
                "disable"
            } else {
                &config.postgresql.sslmode
            },
        ),
        _ => format!("sqlite://{SQLITE_DATABASE_PATH}?mode=rwc"),
    }
}

pub(crate) fn validate_database_options(
    config: &Config,
) -> Result<&'static str, (&'static str, &'static str)> {
    use sea_orm::sqlx::{mysql::MySqlConnectOptions, postgres::PgConnectOptions};

    match config.db.r#type.as_str() {
        "" | config::DB_TYPE_SQLITE => Ok(config::DB_TYPE_SQLITE),
        config::DB_TYPE_MYSQL => {
            let valid = !config.mysql.addr.is_empty()
                && database_url(config)
                    .parse::<MySqlConnectOptions>()
                    .is_ok_and(|options| {
                        options.get_database().is_some_and(|name| !name.is_empty())
                    });
            valid
                .then_some(config::DB_TYPE_MYSQL)
                .ok_or(("mysql", "configure a valid MySQL address and database name"))
        }
        config::DB_TYPE_POSTGRESQL => {
            let valid = database_url(config).parse::<PgConnectOptions>().is_ok()
                && (config.postgresql.port.is_empty()
                    || config
                        .postgresql
                        .port
                        .parse::<u16>()
                        .is_ok_and(|port| port > 0));
            valid.then_some(config::DB_TYPE_POSTGRESQL).ok_or((
                "postgresql",
                "configure a valid PostgreSQL endpoint and SSL mode",
            ))
        }
        _ => Err((
            "db.type",
            "database type must be sqlite, mysql, or postgresql",
        )),
    }
}

pub(crate) fn database_type(config: &Config) -> &'static str {
    match config.db.r#type.as_str() {
        config::DB_TYPE_MYSQL => config::DB_TYPE_MYSQL,
        config::DB_TYPE_POSTGRESQL => config::DB_TYPE_POSTGRESQL,
        _ => config::DB_TYPE_SQLITE,
    }
}

/// Connect to the configured database.
pub async fn connect(config: &Config) -> anyhow::Result<DatabaseConnection> {
    if database_type(config) == config::DB_TYPE_SQLITE {
        std::fs::create_dir_all("./data")?;
    }
    let mut opt = ConnectOptions::new(database_url(config));
    if config.db.max_open_conns > 0 {
        opt.max_connections(config.db.max_open_conns);
    }
    if config.db.max_idle_conns > 0 {
        opt.min_connections(config.db.max_idle_conns);
    }
    opt.connect_timeout(Duration::from_secs(10));
    Ok(Database::connect(opt).await?)
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SchemaState {
    Empty,
    Current,
    UpgradeRequired,
    NewerThanBinary,
    Invalid,
}

impl fmt::Display for SchemaState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "empty",
            Self::Current => "current",
            Self::UpgradeRequired => "upgrade_required",
            Self::NewerThanBinary => "newer_than_binary",
            Self::Invalid => "invalid",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SchemaError {
    #[error("database schema {0} is newer than the supported schema {DATABASE_VERSION}")]
    Future(i32),
    #[error("database schema metadata is invalid")]
    Invalid,
}

pub(crate) struct DatabaseStatus {
    pub connection_ok: bool,
    pub installed_schema: Option<i32>,
    pub state: SchemaState,
}

pub(crate) struct MigrationOutcome {
    pub from_schema: Option<i32>,
    pub installed_schema: i32,
}

#[derive(serde::Serialize)]
pub(crate) struct BackupManifest {
    pub schema_version: u8,
    pub component: &'static str,
    pub version: &'static str,
    pub database_type: &'static str,
    pub installed_schema: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: String,
}

pub(crate) async fn backup_sqlite(destination: &Path) -> anyhow::Result<BackupManifest> {
    use sea_orm::sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use sha2::{Digest, Sha256};

    anyhow::ensure!(
        Path::new(SQLITE_DATABASE_PATH).is_file(),
        "Console SQLite database is missing or is not a regular file"
    );

    let mut directory = DirBuilder::new();
    let mut files = OpenOptions::new();
    files.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        directory.mode(0o700);
        files.mode(0o600);
    }
    directory
        .create(destination)
        .context("could not create a new backup directory")?;

    let result = async {
        const BACKUP_FILE: &str = "rustdeskapi.db";
        let path = destination.canonicalize()?.join(BACKUP_FILE);
        let filename = path.to_str().context("backup path must be valid UTF-8")?;
        let file = files.open(&path)?;
        let source = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(SQLITE_DATABASE_PATH)
                    .read_only(true),
            )
            .await
            .context("could not open the Console SQLite database")?;
        sea_orm::sqlx::query("VACUUM INTO ?")
            .bind(filename)
            .execute(&source)
            .await
            .context("could not take a consistent SQLite snapshot")?;
        source.close().await;
        file.sync_all()?;

        let snapshot = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(&path).read_only(true))
            .await
            .context("could not open the SQLite snapshot")?;
        let snapshot = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(snapshot);
        let installed_schema = latest_schema_version(&snapshot)
            .await
            .context("could not read the SQLite snapshot schema")?;
        snapshot.close().await?;

        let mut sha256 = Sha256::new();
        let bytes = io::copy(&mut File::open(&path)?, &mut sha256)?;
        let manifest = BackupManifest {
            schema_version: 1,
            component: "rustdesk-console",
            version: env!("CARGO_PKG_VERSION"),
            database_type: config::DB_TYPE_SQLITE,
            installed_schema,
            created_at: chrono::Utc::now(),
            file: BACKUP_FILE,
            bytes,
            sha256: format!("{:x}", sha256.finalize()),
        };
        let manifest_path = destination.join("manifest.json");
        let manifest_temp = destination.join("manifest.json.tmp");
        let mut manifest_file = files.open(&manifest_temp)?;
        serde_json::to_writer_pretty(&mut manifest_file, &manifest)?;
        manifest_file.write_all(b"\n")?;
        manifest_file.sync_all()?;
        drop(manifest_file);
        fs::rename(manifest_temp, manifest_path)?;
        #[cfg(unix)]
        {
            File::open(destination)?.sync_all()?;
            let parent = destination
                .parent()
                .filter(|path| !path.as_os_str().is_empty());
            File::open(parent.unwrap_or_else(|| Path::new(".")))?.sync_all()?;
        }
        Ok(manifest)
    }
    .await;

    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

pub(crate) async fn database_status(config: &Config) -> DatabaseStatus {
    let is_sqlite = database_type(config) == config::DB_TYPE_SQLITE;
    let invalid = DatabaseStatus {
        connection_ok: false,
        installed_schema: None,
        state: SchemaState::Invalid,
    };
    let url = if is_sqlite {
        match Path::new(SQLITE_DATABASE_PATH).metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return DatabaseStatus {
                    connection_ok: false,
                    installed_schema: None,
                    state: SchemaState::Empty,
                };
            }
            Ok(meta) if meta.is_file() => format!("sqlite://{SQLITE_DATABASE_PATH}?mode=ro"),
            _ => return invalid,
        }
    } else {
        database_url(config)
    };

    let mut options = ConnectOptions::new(url);
    options
        .max_connections(1)
        .connect_timeout(Duration::from_secs(10))
        .sqlx_logging(false);
    let Ok(db) = Database::connect(options).await else {
        return invalid;
    };

    let result = async {
        let transaction = db
            .begin_with_config(None, Some(AccessMode::ReadOnly))
            .await?;
        let installed = latest_schema_version(&transaction).await;
        transaction.rollback().await?;
        installed
    }
    .await;
    let installed_schema = match result {
        Ok(installed_schema) => installed_schema,
        Err(_) => {
            return DatabaseStatus {
                connection_ok: true,
                ..invalid
            }
        }
    };
    let state = match installed_schema {
        None => SchemaState::Empty,
        Some(version) if version < 0 => SchemaState::Invalid,
        Some(version) if version < DATABASE_VERSION => SchemaState::UpgradeRequired,
        Some(DATABASE_VERSION) => SchemaState::Current,
        Some(_) => SchemaState::NewerThanBinary,
    };
    DatabaseStatus {
        connection_ok: true,
        installed_schema,
        state,
    }
}

/// SQLite DDL and first-run seed are transactional. The separate file lock
/// serializes starters before they take a read snapshot and respects retries
/// after process exit, when the OS automatically releases the lock.
async fn lock_sqlite_migration(config: &Config) -> anyhow::Result<Option<File>> {
    if database_type(config) != config::DB_TYPE_SQLITE {
        return Ok(None);
    }
    tokio::task::spawn_blocking(|| {
        std::fs::create_dir_all("./data")?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(format!("{SQLITE_DATABASE_PATH}.migrate.lock"))?;
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(Some(file))
    })
    .await?
}

async fn lock_mysql_migration(config: &Config) -> anyhow::Result<sea_orm::sqlx::MySqlConnection> {
    use sea_orm::sqlx::{self, Connection, MySqlConnection};

    // DDL implicitly commits on MySQL. Hold the server advisory lock on a
    // dedicated connection for the entire apply/seed operation.
    let mut lock = tokio::time::timeout(
        Duration::from_secs(10),
        MySqlConnection::connect(&database_url(config)),
    )
    .await??;
    let acquired: Option<i32> =
        sqlx::query_scalar("SELECT GET_LOCK(CONCAT('rdc:', SHA2(DATABASE(), 224)), 10)")
            .fetch_one(&mut lock)
            .await?;
    anyhow::ensure!(
        acquired == Some(1),
        "could not acquire the schema migration lock"
    );
    Ok(lock)
}

/// Create any missing tables. Existing tables are never altered here; schema
/// evolution is handled by the ordered migrations below.
async fn create_tables(db: &impl ConnectionTrait) -> anyhow::Result<()> {
    let backend = db.get_database_backend();
    let schema = Schema::new(backend);

    macro_rules! create {
        ($ent:expr) => {{
            let stmt = schema
                .create_table_from_entity($ent)
                .if_not_exists()
                .to_owned();
            db.execute(backend.build(&stmt)).await?;
        }};
    }

    create!(version::Entity);
    create!(user::Entity);
    create!(entity::ldap_identity::Entity);
    create!(entity::ldap_setting::Entity);
    create!(user_token::Entity);
    create!(login_verification::Entity);
    create!(trusted_login_device::Entity);
    create!(tag::Entity);
    create!(address_book::Entity);
    create!(peer::Entity);
    create!(group::Entity);
    create!(user_third::Entity);
    create!(oauth::Entity);
    create!(login_log::Entity);
    create!(message::Entity);
    create!(message_read::Entity);
    create!(smtp_email_config::Entity);
    create!(share_record::Entity);
    create!(audit_conn::Entity);
    create!(audit_file::Entity);
    create!(audit_alarm::Entity);
    create!(audit_receipt::Entity);
    create!(entity::console_audit::Entity);
    create!(record_file::Entity);
    create!(deployment_token::Entity);
    create!(entity::developer_key::Entity);
    create!(deployment_event::Entity);
    create!(webhook_subscription::Entity);
    create!(entity::notification_template::Entity);
    create!(entity::notification_template_binding::Entity);
    create!(webhook_delivery::Entity);
    create!(device_presence_state::Entity);
    create!(strategy::Entity);
    create!(strategy_assignment::Entity);
    create!(entity::strategy_delivery::Entity);
    create!(system_setting::Entity);
    create!(address_book_collection::Entity);
    create!(address_book_collection_rule::Entity);
    create!(entity::address_book_trash::Entity);
    create!(server_cmd::Entity);
    create!(device_group::Entity);
    create!(entity::device_admission::Entity);
    create!(entity::routing_settings::Entity);
    create!(entity::control_role::Entity);
    create!(active_connection::Entity);
    Ok(())
}

fn migration_271<'a>(db: &'a impl ConnectionTrait, config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move { migrate_271_legacy_columns(db, config).await })
}

fn migration_290<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        let backend = db.get_database_backend();
        let statement = Schema::new(backend)
            .create_table_from_entity(entity::enrollment_invitation::Entity)
            .if_not_exists()
            .to_owned();
        db.execute(backend.build(&statement)).await?;
        Ok(())
    })
}

fn migration_286<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        let backend = db.get_database_backend();
        let statement = Schema::new(backend)
            .create_table_from_entity(entity::geo_mmdb_download::Entity)
            .if_not_exists()
            .to_owned();
        db.execute(backend.build(&statement)).await?;
        Ok(())
    })
}

fn migration_285<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        let backend = db.get_database_backend();
        if !column_exists(db, "user_thirds", "oidc_key").await? {
            let statement = Table::alter()
                .table(user_third::Entity)
                .add_column(ColumnDef::new(user_third::Column::OidcKey).string_len(64))
                .to_owned();
            db.execute(backend.build(&statement)).await?;
        }
        ensure_index(
            db,
            "user_thirds",
            "idx_user_thirds_oidc_key",
            &["oidc_key"],
            true,
        )
        .await?;
        if !column_exists(db, "login_verifications", "oauth_binding_id").await? {
            let statement = Table::alter()
                .table(login_verification::Entity)
                .add_column(ColumnDef::new(login_verification::Column::OauthBindingId).integer())
                .to_owned();
            db.execute(backend.build(&statement)).await?;
        }
        Ok(())
    })
}

fn migration_284<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        let backend = db.get_database_backend();
        let statement = Schema::new(backend)
            .create_table_from_entity(entity::trusted_browser::Entity)
            .if_not_exists()
            .to_owned();
        db.execute(backend.build(&statement)).await?;
        if !column_exists(db, "user_tokens", "trusted_browser_id").await? {
            let statement = Table::alter()
                .table(entity::user_token::Entity)
                .add_column(
                    ColumnDef::new(entity::user_token::Column::TrustedBrowserId).string_len(36),
                )
                .to_owned();
            db.execute(backend.build(&statement)).await?;
        }
        create_index(
            db,
            "trusted_browsers",
            "idx_trusted_browsers_user",
            &["user_id"],
        )
        .await?;
        create_index(
            db,
            "user_tokens",
            "idx_user_tokens_trusted_browser",
            &["user_id", "trusted_browser_id"],
        )
        .await?;
        Ok(())
    })
}

fn migration_283<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        for (table, name, column) in [
            ("peers", "idx_peers_id", "id"),
            ("peers", "idx_peers_uuid", "uuid"),
            ("peers", "idx_peers_last_online", "last_online_time"),
            (
                "active_connections",
                "idx_active_connections_peer",
                "peer_id",
            ),
            (
                "active_connections",
                "idx_active_connections_updated",
                "updated_at",
            ),
        ] {
            create_index(db, table, name, &[column]).await?;
        }
        Ok(())
    })
}

fn migration_282<'a>(db: &'a impl ConnectionTrait, config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        use crate::support::totp_vault::TotpVault;

        let backend = db.get_database_backend();
        let schema = Schema::new(backend);
        let statement = schema.create_table_from_entity(entity::totp_counter::Entity)
            .if_not_exists().to_owned();
        db.execute(backend.build(&statement)).await?;

        let accounts: Vec<(i32, String)> = user::Entity::find().select_only()
            .column(user::Column::Id).column(user::Column::TfaSecret)
            .filter(user::Column::TfaSecret.ne(""))
            .into_tuple().all(db).await?;
        if accounts.is_empty() {
            return Ok(());
        }
        let vault = TotpVault::load(config, !accounts.iter().any(|(_, seed)| TotpVault::is_sealed(seed)))?;
        // Verify existing ciphertext before writing during a retry or startup.
        for (id, seed) in &accounts {
            if TotpVault::is_sealed(seed) {
                vault.open(*id, seed)?;
            }
        }
        if backend == DatabaseBackend::Sqlite {
            db.execute(Statement::from_string(backend, "PRAGMA secure_delete=ON".to_string())).await?;
        }
        for (id, seed) in accounts {
            if !TotpVault::is_sealed(&seed) {
                user::Entity::update_many()
                    .filter(user::Column::Id.eq(id))
                    .filter(user::Column::TfaSecret.eq(&seed))
                    .col_expr(user::Column::TfaSecret, Expr::value(vault.seal(id, &seed)?))
                    .exec(db).await?;
            }
        }
        Ok(())
    })
}

fn migration_281<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        if !column_exists(db, "record_files", "storage_profile_id").await? {
            let statement = Table::alter()
                .table(record_file::Entity)
                .add_column(ColumnDef::new(record_file::Column::StorageProfileId).string_len(36))
                .to_owned();
            db.execute(db.get_database_backend().build(&statement)).await?;
        }
        Ok(())
    })
}

fn migration_280<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        let backend = db.get_database_backend();
        let schema = Schema::new(backend);
        let statement = schema
            .create_table_from_entity(entity::connection_grant::Entity)
            .if_not_exists()
            .to_owned();
        db.execute(backend.build(&statement)).await?;
        Ok(())
    })
}

fn migration_272<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move { migrate_272_address_book_notes(db).await })
}

fn migration_273<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move { migrate_273_smtp_sender_names(db).await })
}

fn migration_274<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        if !column_exists(db, "device_groups", "note").await? {
            let statement = Table::alter()
                .table(device_group::Entity)
                .add_column(
                    ColumnDef::new(device_group::Column::Note)
                        .string()
                        .not_null()
                        .default(""),
                )
                .to_owned();
            db.execute(db.get_database_backend().build(&statement))
                .await?;
        }
        Ok(())
    })
}

fn migration_275<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        if !column_exists(db, "user_tokens", "auth_level").await? {
            let statement = Table::alter()
                .table(user_token::Entity)
                .add_column(
                    ColumnDef::new(user_token::Column::AuthLevel)
                        .integer()
                        .not_null()
                        .default(0),
                )
                .to_owned();
            db.execute(db.get_database_backend().build(&statement))
                .await?;
        }
        if !column_exists(db, "user_tokens", "renewable").await? {
            let statement = Table::alter()
                .table(user_token::Entity)
                .add_column(
                    ColumnDef::new(user_token::Column::Renewable)
                        .boolean()
                        .not_null()
                        .default(true),
                )
                .to_owned();
            db.execute(db.get_database_backend().build(&statement))
                .await?;
        }
        if !column_exists(db, "login_verifications", "federated").await? {
            let statement = Table::alter()
                .table(login_verification::Entity)
                .add_column(
                    ColumnDef::new(login_verification::Column::Federated)
                        .boolean()
                        .not_null()
                        .default(false),
                )
                .to_owned();
            db.execute(db.get_database_backend().build(&statement))
                .await?;
        }
        Ok(())
    })
}

fn migration_277<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        if !column_exists(db, "address_book_collections", "note").await? {
            let statement = Table::alter()
                .table(address_book_collection::Entity)
                .add_column(
                    ColumnDef::new(address_book_collection::Column::Note)
                        .string()
                        .not_null()
                        .default(""),
                )
                .to_owned();
            db.execute(db.get_database_backend().build(&statement))
                .await?;
        }
        Ok(())
    })
}

fn migration_278<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        for (name, column) in [
            ("reported_type", audit_file::Column::ReportedType),
            ("conn_id", audit_file::Column::ConnId),
        ] {
            if !column_exists(db, "audit_files", name).await? {
                let statement = Table::alter()
                    .table(audit_file::Entity)
                    .add_column(ColumnDef::new(column).integer())
                    .to_owned();
                db.execute(db.get_database_backend().build(&statement))
                    .await?;
            }
        }

        // RustDesk reports full paths and up to ten file details in the raw JSON.
        // Widen legacy VARCHAR columns while retaining existing text-backed columns.
        let backend = db.get_database_backend();
        for column in ["info", "path"] {
            let (query, alter) = match backend {
                DatabaseBackend::MySql => (
                    format!(
                        "SELECT is_nullable AS nullable_value FROM information_schema.columns
                         WHERE table_schema = DATABASE() AND table_name = 'audit_files'
                         AND column_name = '{column}' AND data_type IN ('char', 'varchar')"
                    ),
                    format!("ALTER TABLE audit_files MODIFY COLUMN `{column}` MEDIUMTEXT NOT NULL"),
                ),
                DatabaseBackend::Postgres => (
                    format!(
                        "SELECT is_nullable AS nullable_value FROM information_schema.columns
                         WHERE table_schema = current_schema() AND table_name = 'audit_files'
                         AND column_name = '{column}' AND character_maximum_length IS NOT NULL"
                    ),
                    format!(r#"ALTER TABLE audit_files ALTER COLUMN "{column}" TYPE TEXT"#),
                ),
                DatabaseBackend::Sqlite => break,
            };
            if let Some(legacy) = db.query_one(Statement::from_string(backend, query)).await? {
                let nullable: String = legacy.try_get("", "nullable_value")?;
                if nullable == "YES" {
                    // Legacy databases can represent missing metadata as NULL.
                    // The audit entity represents the same absence as an empty string.
                    db.execute_unprepared(&format!(
                        "UPDATE audit_files SET {column} = '' WHERE {column} IS NULL"
                    ))
                    .await?;
                }
                db.execute_unprepared(&alter).await?;
            }
        }
        Ok(())
    })
}

fn migration_279<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        let backend = db.get_database_backend();
        let query = match backend {
            DatabaseBackend::MySql => {
                "SELECT is_nullable AS nullable_value FROM information_schema.columns
                 WHERE table_schema = DATABASE() AND table_name = 'system_settings'
                 AND column_name = 'value' AND data_type IN ('char', 'varchar')"
            }
            DatabaseBackend::Postgres => {
                "SELECT is_nullable AS nullable_value FROM information_schema.columns
                 WHERE table_schema = current_schema() AND table_name = 'system_settings'
                 AND column_name = 'value' AND character_maximum_length IS NOT NULL"
            }
            // SQLite accepts long strings even in a column declared VARCHAR(255).
            DatabaseBackend::Sqlite => return Ok(()),
        };
        if let Some(column) = db.query_one(Statement::from_string(backend, query)).await? {
            let nullable: String = column.try_get("", "nullable_value")?;
            let alter = match backend {
                DatabaseBackend::MySql if nullable == "YES" => {
                    "ALTER TABLE system_settings MODIFY COLUMN `value` TEXT NULL"
                }
                DatabaseBackend::MySql => {
                    "ALTER TABLE system_settings MODIFY COLUMN `value` TEXT NOT NULL"
                }
                DatabaseBackend::Postgres => {
                    r#"ALTER TABLE system_settings ALTER COLUMN "value" TYPE TEXT"#
                }
                DatabaseBackend::Sqlite => unreachable!(),
            };
            db.execute_unprepared(alter).await?;
        }
        Ok(())
    })
}

fn migration_276<'a>(db: &'a impl ConnectionTrait, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move {
        use sea_orm::sea_query::Alias;

        for (table, column, mut definition) in [
            (
                "webhook_subscriptions",
                "channel",
                ColumnDef::new(Alias::new("channel"))
                    .string()
                    .not_null()
                    .default("webhook")
                    .to_owned(),
            ),
            (
                "webhook_subscriptions",
                "topic",
                ColumnDef::new(Alias::new("topic"))
                    .string()
                    .not_null()
                    .default("")
                    .to_owned(),
            ),
            (
                "webhook_deliveries",
                "channel",
                ColumnDef::new(Alias::new("channel"))
                    .string()
                    .not_null()
                    .default("webhook")
                    .to_owned(),
            ),
            (
                "webhook_subscriptions",
                "destination_revision",
                ColumnDef::new(Alias::new("destination_revision"))
                    .string()
                    .not_null()
                    .default("")
                    .to_owned(),
            ),
            (
                "webhook_deliveries",
                "destination_revision",
                ColumnDef::new(Alias::new("destination_revision"))
                    .string()
                    .not_null()
                    .default("")
                    .to_owned(),
            ),
            (
                "webhook_deliveries",
                "execute_at",
                ColumnDef::new(Alias::new("execute_at"))
                    .big_integer()
                    .not_null()
                    .default(0)
                    .to_owned(),
            ),
        ] {
            if !column_exists(db, table, column).await? {
                let statement = Table::alter()
                    .table(Alias::new(table))
                    .add_column(&mut definition)
                    .to_owned();
                db.execute(db.get_database_backend().build(&statement))
                    .await?;
            }
        }
        create_index(
            db,
            "webhook_deliveries",
            "idx_webhook_delivery_schedule",
            &["status", "execute_at", "id"],
        )
        .await
    })
}

async fn create_index(
    db: &impl ConnectionTrait,
    table: &str,
    name: &str,
    columns: &[&str],
) -> anyhow::Result<()> {
    ensure_index(db, table, name, columns, false).await
}

async fn ensure_index(
    db: &impl ConnectionTrait,
    table: &str,
    name: &str,
    columns: &[&str],
    unique: bool,
) -> anyhow::Result<()> {
    use sea_orm::sea_query::{Alias, Index};

    let backend = db.get_database_backend();
    let mut index = Index::create();
    if backend == DatabaseBackend::MySql {
        // MySQL commits each DDL statement and has no CREATE INDEX IF NOT EXISTS.
        let exists = Statement::from_sql_and_values(
            backend,
            "SELECT 1 FROM information_schema.statistics WHERE table_schema = DATABASE() AND table_name = ? AND index_name = ? LIMIT 1",
            [table.into(), name.into()],
        );
        if db.query_one(exists).await?.is_some() {
            return Ok(());
        }
    } else {
        index.if_not_exists();
    }
    index.name(name).table(Alias::new(table));
    if unique {
        index.unique();
    }
    for column in columns {
        index.col(Alias::new(*column));
    }
    db.execute(backend.build(&index)).await?;
    Ok(())
}

async fn migrate_271_legacy_columns(
    db: &impl ConnectionTrait,
    config: &Config,
) -> anyhow::Result<()> {
    let backend = db.get_database_backend();

    if !column_exists(db, "users", "must_change_password").await? {
        let stmt = Table::alter()
            .table(user::Entity)
            .add_column(
                ColumnDef::new(user::Column::MustChangePassword)
                    .boolean()
                    .not_null()
                    .default(false),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "users", "tfa_secret").await? {
        let stmt = Table::alter()
            .table(user::Entity)
            .add_column(
                ColumnDef::new(user::Column::TfaSecret)
                    .string()
                    .not_null()
                    .default(""),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "users", "tfa_enabled").await? {
        let stmt = Table::alter()
            .table(user::Entity)
            .add_column(
                ColumnDef::new(user::Column::TfaEnabled)
                    .boolean()
                    .not_null()
                    .default(false),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "users", "tfa_enforced").await? {
        let stmt = Table::alter()
            .table(user::Entity)
            .add_column(
                ColumnDef::new(user::Column::TfaEnforced)
                    .boolean()
                    .not_null()
                    .default(false),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "users", "email_verification_enabled").await? {
        let stmt = Table::alter()
            .table(user::Entity)
            .add_column(
                ColumnDef::new(user::Column::EmailVerificationEnabled)
                    .boolean()
                    .not_null()
                    .default(false),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "users", "login_device_verification_enabled").await? {
        let stmt = Table::alter()
            .table(user::Entity)
            .add_column(
                ColumnDef::new(user::Column::LoginDeviceVerificationEnabled)
                    .boolean()
                    .not_null()
                    .default(false),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "message_reads", "deleted_at").await? {
        let stmt = Table::alter()
            .table(message_read::Entity)
            .add_column(ColumnDef::new(message_read::Column::DeletedAt).date_time())
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "audit_conns", "note").await? {
        let stmt = Table::alter()
            .table(audit_conn::Entity)
            .add_column(
                ColumnDef::new(audit_conn::Column::Note)
                    .string()
                    .not_null()
                    .default(""),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "record_files", "storage_backend").await? {
        let stmt = Table::alter()
            .table(record_file::Entity)
            .add_column(
                ColumnDef::new(record_file::Column::StorageBackend)
                    .string()
                    .not_null()
                    .default(record_file::STORAGE_LOCAL),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "record_files", "storage_key").await? {
        let stmt = Table::alter()
            .table(record_file::Entity)
            .add_column(
                ColumnDef::new(record_file::Column::StorageKey)
                    .string()
                    .not_null()
                    .default(""),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "peers", "pk").await? {
        let stmt = Table::alter()
            .table(peer::Entity)
            .add_column(
                ColumnDef::new(peer::Column::Pk)
                    .string()
                    .not_null()
                    .default(""),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "peers", "guid").await? {
        let stmt = Table::alter()
            .table(peer::Entity)
            .add_column(
                ColumnDef::new(peer::Column::Guid)
                    .string()
                    .not_null()
                    .default(""),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "peers", "status").await? {
        let stmt = Table::alter()
            .table(peer::Entity)
            .add_column(
                ColumnDef::new(peer::Column::Status)
                    .integer()
                    .not_null()
                    .default(user::STATUS_ENABLE),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    if !column_exists(db, "peers", "force_sysinfo_refresh").await? {
        let stmt = Table::alter()
            .table(peer::Entity)
            .add_column(
                ColumnDef::new(peer::Column::ForceSysinfoRefresh)
                    .boolean()
                    .not_null()
                    .default(false),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    backfill_record_storage_keys(db, config).await?;
    Ok(())
}

async fn migrate_272_address_book_notes(db: &impl ConnectionTrait) -> anyhow::Result<()> {
    let backend = db.get_database_backend();

    if !column_exists(db, "address_books", "note").await? {
        let stmt = Table::alter()
            .table(address_book::Entity)
            .add_column(
                ColumnDef::new(address_book::Column::Note)
                    .string()
                    .not_null()
                    .default(""),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    Ok(())
}

async fn migrate_273_smtp_sender_names(db: &impl ConnectionTrait) -> anyhow::Result<()> {
    let backend = db.get_database_backend();

    if !column_exists(db, "smtp_email_configs", "from_name").await? {
        let stmt = Table::alter()
            .table(smtp_email_config::Entity)
            .add_column(
                ColumnDef::new(smtp_email_config::Column::FromName)
                    .string()
                    .not_null()
                    .default(""),
            )
            .to_owned();
        db.execute(backend.build(&stmt)).await?;
    }

    Ok(())
}

async fn column_exists(
    db: &impl ConnectionTrait,
    table_name: &str,
    column_name: &str,
) -> anyhow::Result<bool> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DatabaseBackend::MySql => format!(
            "SELECT 1 AS exists_col FROM information_schema.columns \
             WHERE table_schema = DATABASE() AND table_name = '{table_name}' \
             AND column_name = '{column_name}' LIMIT 1"
        ),
        DatabaseBackend::Postgres => format!(
            "SELECT 1 AS exists_col FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = '{table_name}' \
             AND column_name = '{column_name}' LIMIT 1"
        ),
        DatabaseBackend::Sqlite => format!(
            "SELECT 1 AS exists_col FROM pragma_table_info('{table_name}') \
             WHERE name = '{column_name}' LIMIT 1"
        ),
    };
    Ok(db
        .query_one(Statement::from_string(backend, sql))
        .await?
        .is_some())
}

async fn backfill_record_storage_keys(
    db: &impl ConnectionTrait,
    config: &Config,
) -> anyhow::Result<()> {
    // This legacy step also runs before storage_profile_id exists on upgraded databases.
    let rows: Vec<(i32, String, String)> = record_file::Entity::find()
        .select_only()
        .column(record_file::Column::Id)
        .column(record_file::Column::Filename)
        .column(record_file::Column::StorageKey)
        .into_tuple()
        .all(db)
        .await?;
    let root = services::record_file::record_root_for_config(
        &config.gin.resources_path,
        &config.record_storage.local_dir,
    );
    for (id, filename, key) in rows {
        if key.trim().is_empty() {
            record_file::Entity::update_many()
                .filter(record_file::Column::Id.eq(id))
                .col_expr(record_file::Column::StorageBackend, Expr::value(record_file::STORAGE_LOCAL))
                .col_expr(record_file::Column::StorageKey, Expr::value(root.join(filename).to_string_lossy().to_string()))
                .exec(db)
                .await?;
        }
    }
    Ok(())
}

async fn latest_schema_version(db: &impl ConnectionTrait) -> anyhow::Result<Option<i32>> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DatabaseBackend::MySql => "SELECT 1 FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = 'versions' AND table_type = 'BASE TABLE'",
        DatabaseBackend::Postgres => "SELECT 1 FROM information_schema.tables WHERE table_schema = current_schema() AND table_name = 'versions' AND table_type = 'BASE TABLE'",
        DatabaseBackend::Sqlite => "SELECT 1 FROM sqlite_schema WHERE name = 'versions' AND type = 'table'",
    };
    if db
        .query_one(Statement::from_string(backend, sql))
        .await?
        .is_none()
    {
        return Ok(None);
    }
    Ok(version::Entity::find()
        .select_only()
        .column(version::Column::Version)
        .order_by_desc(version::Column::Version)
        .into_tuple::<i32>()
        .one(db)
        .await?)
}

pub(crate) async fn require_compatible_schema(
    db: &impl ConnectionTrait,
) -> anyhow::Result<Option<i32>> {
    let installed_schema = latest_schema_version(db)
        .await
        .map_err(|_| SchemaError::Invalid)?;
    if let Some(version) = installed_schema {
        if version > DATABASE_VERSION {
            return Err(SchemaError::Future(version).into());
        }
        if version < 0 {
            return Err(SchemaError::Invalid.into());
        }
    }
    Ok(installed_schema)
}

async fn record_schema_version(
    db: &impl ConnectionTrait,
    schema_version: i32,
) -> anyhow::Result<()> {
    let am = version::ActiveModel {
        version: Set(schema_version),
        created_at: Set(services::now()),
        updated_at: Set(services::now()),
        ..Default::default()
    };
    am.insert(db).await?;
    Ok(())
}

async fn apply_schema(
    db: &impl ConnectionTrait,
    config: &Config,
) -> anyhow::Result<MigrationOutcome> {
    let from_schema = require_compatible_schema(db).await?;
    let migrations = schema_migrations();
    let declared_latest = migrations.last().map_or(0, |migration| migration.version);
    anyhow::ensure!(
        declared_latest == DATABASE_VERSION,
        "schema migration list latest version {declared_latest} does not match DATABASE_VERSION {DATABASE_VERSION}"
    );

    create_tables(db).await?;
    if from_schema.is_none()
        && user::Entity::find().count(db).await? == 0
        && first_run_seed_marker(db).await?.is_none()
    {
        system_setting::ActiveModel {
            key: Set(FIRST_RUN_SEED_PENDING.to_owned()),
            value: Set("true".to_owned()),
            created_at: Set(services::now()),
            updated_at: Set(services::now()),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }
    for migration in migrations {
        let pending = migration.version > from_schema.unwrap_or(0);
        if pending {
            tracing::info!("Applying schema migration {}", migration);
        }
        (migration.apply)(db, config).await?;
        if pending {
            record_schema_version(db, migration.version).await?;
        }
    }
    Ok(MigrationOutcome {
        from_schema,
        installed_schema: DATABASE_VERSION,
    })
}

async fn initialize_database(
    db: &DatabaseConnection,
    config: &Config,
    seed_locale: Option<(&I18n, &str)>,
) -> anyhow::Result<MigrationOutcome> {
    let backend = db.get_database_backend();
    if backend == DatabaseBackend::MySql {
        let _lock = lock_mysql_migration(config).await?;
        let outcome = apply_schema(db, config).await?;
        if let Some((i18n, lang)) = seed_locale {
            let transaction = db.begin().await?;
            seed_if_needed(&transaction, config, i18n, lang).await?;
            transaction.commit().await?;
        }
        return Ok(outcome);
    }

    let transaction = if backend == DatabaseBackend::Postgres {
        // The version read must see the holder's committed migration after
        // waiting on its advisory lock.
        let transaction = db
            .begin_with_config(Some(IsolationLevel::ReadCommitted), None)
            .await?;
        transaction
            .query_one(Statement::from_string(
                backend,
                "SELECT pg_advisory_xact_lock(1866693622, hashtext(current_schema()))",
            ))
            .await?;
        transaction
    } else {
        db.begin().await?
    };
    let outcome = apply_schema(&transaction, config).await?;
    if let Some((i18n, lang)) = seed_locale {
        seed_if_needed(&transaction, config, i18n, lang).await?;
    }
    transaction.commit().await?;
    Ok(outcome)
}

/// Apply the schema without creating default groups or user accounts.
pub(crate) async fn migrate_database(config: &Config) -> anyhow::Result<MigrationOutcome> {
    let _lock = lock_sqlite_migration(config).await?;
    let db = connect(config).await?;
    initialize_database(&db, config, None).await
}

/// Run schema sync + version bookkeeping + first-run seed.
pub async fn migrate_and_seed(
    db: &DatabaseConnection,
    config: &Config,
    i18n: &I18n,
    lang: &str,
) -> anyhow::Result<()> {
    initialize_database(db, config, Some((i18n, lang))).await?;
    Ok(())
}

async fn first_run_seed_marker(
    db: &impl ConnectionTrait,
) -> Result<Option<system_setting::Model>, sea_orm::DbErr> {
    system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(FIRST_RUN_SEED_PENDING))
        .one(db)
        .await
}

async fn seed_if_needed(
    db: &impl ConnectionTrait,
    config: &Config,
    i18n: &I18n,
    lang: &str,
) -> anyhow::Result<()> {
    // A schema-only first run defers application data to the next startup.
    // Keep the marker transactional with the seed so interrupted starts retry
    // without treating deliberately emptied existing installations as new.
    if let Some(marker) = first_run_seed_marker(db).await? {
        if user::Entity::find().count(db).await? == 0 {
            seed(db, config, i18n, lang).await?;
        }
        system_setting::Entity::delete_by_id(marker.id)
            .exec(db)
            .await?;
    }
    Ok(())
}

async fn seed(
    db: &impl ConnectionTrait,
    config: &Config,
    i18n: &I18n,
    lang: &str,
) -> anyhow::Result<()> {
    // default + share groups
    let default_name = i18n.translate(lang, "DefaultGroup");
    let default_name = if default_name == "DefaultGroup" {
        "Default".to_string()
    } else {
        default_name
    };
    let share_name = i18n.translate(lang, "ShareGroup");
    let share_name = if share_name == "ShareGroup" {
        "Share".to_string()
    } else {
        share_name
    };
    let default_group = services::group::create(db, &default_name, group::TYPE_DEFAULT).await?;
    services::group::create(db, &share_name, group::TYPE_SHARE).await?;

    if config.admin.password.is_empty() {
        tracing::info!("Admin account not initialized. Complete setup from the web console.");
        return Ok(());
    }

    tracing::info!("Admin Username Is: {}", config.admin.username);
    tracing::info!("Admin password loaded from config/env.");
    let hash = password::encrypt_password(&config.admin.password)?;
    let admin = user::ActiveModel {
        username: Set(config.admin.username.clone()),
        nickname: Set("Admin".to_string()),
        status: Set(user::STATUS_ENABLE),
        is_admin: Set(Some(true)),
        group_id: Set(default_group.id),
        password: Set(hash),
        must_change_password: Set(config.admin.force_change_password),
        created_at: Set(services::now()),
        updated_at: Set(services::now()),
        ..Default::default()
    };
    services::login_security::insert_user_with_enrollment_policy(db, admin).await?;
    Ok(())
}

/// Build the full application state from a parsed config.
pub async fn build_state(config: Config, config_path: PathBuf) -> anyhow::Result<AppState> {
    let jwt = Jwt::try_new(&config.jwt.key, config.jwt.expire_duration())?;
    let i18n = I18n::load(&config.lang);
    let db = {
        let _lock = lock_sqlite_migration(&config).await?;
        let db = connect(&config).await?;
        migrate_and_seed(&db, &config, &i18n, &config.lang).await?;
        db
    };

    let limiter = LoginLimiter::new(SecurityPolicy {
        captcha_threshold: config.app.captcha_threshold,
        ban_threshold: config.app.ban_threshold,
        attempts_window: Duration::from_secs(10 * 60),
        ban_duration: Duration::from_secs(30 * 60),
    });

    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let start_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let record_storage = config.record_storage.clone();
    let admin_panel_config = AdminPanelConfig::from(&config.admin);
    let external_webclient = ExternalWebClient::try_load_from_default_zip()
        .await
        .map(Arc::new);

    Ok(AppState {
        features: Arc::new(crate::features::FeatureStore::new(&config_path)),
        db,
        admin_config: Arc::new(AdminConfigStore::new(
            config_path.clone(),
            admin_panel_config,
        )),
        webclient_config: Arc::new(WebClientConfigStore::new(
            config_path.clone(),
            WebClientConfig::from(&config.rustdesk),
        )),
        record_storage_config: Arc::new(
            RecordStorageConfigStore::new(config_path, record_storage)
                .with_profiles_path(config.storage_profiles_path()),
        ),
        config: Arc::new(config),
        jwt: Arc::new(jwt),
        limiter: Arc::new(limiter),
        i18n: Arc::new(i18n),
        oauth_cache: Arc::new(crate::support::oauth_cache::OauthCache::new()),
        disconnect_store: Arc::new(crate::support::disconnect_store::DisconnectStore::new()),
        external_webclient,
        start_time: Arc::new(start_time),
        version: Arc::new(version),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, DbBackend, EntityTrait};

    #[test]
    fn database_credentials_cannot_change_url_structure() {
        let mut config = Config::default();
        for kind in [config::DB_TYPE_MYSQL, config::DB_TYPE_POSTGRESQL] {
            config.db.r#type = kind.to_string();
            config.mysql.username = "user@/#?%:".to_string();
            config.mysql.password = "pass@/#?%:".to_string();
            config.mysql.addr = "localhost:3306".to_string();
            config.postgresql.user = config.mysql.username.clone();
            config.postgresql.password = config.mysql.password.clone();
            config.postgresql.host = "localhost".to_string();
            let parsed = url::Url::parse(&database_url(&config)).unwrap();
            assert_eq!(parsed.host_str(), Some("localhost"));
            assert_eq!(
                urlencoding::decode(parsed.username()).unwrap(),
                "user@/#?%:"
            );
            assert_eq!(
                urlencoding::decode(parsed.password().unwrap()).unwrap(),
                "pass@/#?%:"
            );
        }
    }

    async fn memory_db() -> DatabaseConnection {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.sqlx_logging(false);
        Database::connect(options).await.unwrap()
    }

    fn test_config(admin_password: &str) -> Config {
        let mut cfg = Config::default();
        cfg.admin.password = admin_password.to_string();
        cfg.admin.init();
        cfg
    }

    #[tokio::test]
    async fn schema_only_migration_is_idempotent_and_leaves_first_run_for_startup() {
        let db = memory_db().await;
        let cfg = test_config("change-me");
        let i18n = I18n::load("en");

        let first = initialize_database(&db, &cfg, None).await.unwrap();
        assert_eq!(first.from_schema, None);
        assert_eq!(first.installed_schema, DATABASE_VERSION);
        assert_eq!(group::Entity::find().count(&db).await.unwrap(), 0);
        assert_eq!(user::Entity::find().count(&db).await.unwrap(), 0);

        let repeat = initialize_database(&db, &cfg, None).await.unwrap();
        assert_eq!(repeat.from_schema, Some(DATABASE_VERSION));
        assert_eq!(
            version::Entity::find().count(&db).await.unwrap(),
            schema_migrations::<DatabaseConnection>().len() as u64,
        );

        migrate_and_seed(&db, &cfg, &i18n, "en").await.unwrap();
        migrate_and_seed(&db, &cfg, &i18n, "en").await.unwrap();
        assert_eq!(group::Entity::find().count(&db).await.unwrap(), 2);
        let admin = user::Entity::find().one(&db).await.unwrap().unwrap();
        assert!(admin.is_admin());
        assert_eq!(admin.username, "admin");
    }

    #[tokio::test]
    async fn startup_never_creates_tables_on_a_future_schema() {
        let db = memory_db().await;
        let future_version = DATABASE_VERSION + 1;
        db.execute_unprepared("CREATE TABLE versions (version INTEGER NOT NULL)")
            .await
            .unwrap();
        db.execute_unprepared(&format!(
            "INSERT INTO versions VALUES ({future_version}), (1)"
        ))
        .await
        .unwrap();
        let cfg = test_config("change-me");
        let error = migrate_and_seed(&db, &cfg, &I18n::load("en"), "en")
            .await
            .unwrap_err();
        assert!(
            matches!(error.downcast_ref::<SchemaError>(), Some(SchemaError::Future(version)) if *version == future_version)
        );
        let tables = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_schema WHERE type = 'table'",
            ))
            .await
            .unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].try_get::<String>("", "name").unwrap(), "versions");
    }

    #[tokio::test]
    async fn first_seed_without_admin_password_waits_for_web_setup() {
        let db = memory_db().await;
        let cfg = test_config("");
        let i18n = I18n::load("en");

        migrate_and_seed(&db, &cfg, &i18n, "en").await.unwrap();

        assert_eq!(user::Entity::find().count(&db).await.unwrap(), 0);
        assert_eq!(group::Entity::find().count(&db).await.unwrap(), 2);
        assert_eq!(
            latest_schema_version(&db).await.unwrap(),
            Some(DATABASE_VERSION)
        );
    }

    #[tokio::test]
    async fn first_seed_with_admin_password_creates_admin() {
        let db = memory_db().await;
        let cfg = test_config("change-me");
        let i18n = I18n::load("en");

        migrate_and_seed(&db, &cfg, &i18n, "en").await.unwrap();

        let admin = user::Entity::find().one(&db).await.unwrap().unwrap();
        assert_eq!(admin.username, "admin");
        assert!(admin.is_admin());
        assert!(crate::support::password::verify_password(&admin.password, "change-me").0);
    }

    #[test]
    fn fresh_mysql_schema_supports_long_system_setting_values() {
        let backend = DatabaseBackend::MySql;
        let statement = Schema::new(backend).create_table_from_entity(system_setting::Entity);
        let ddl = backend.build(&statement).sql;
        assert!(ddl.contains("`value` text NOT NULL"), "{ddl}");
        assert!(!ddl.contains("`value` varchar"), "{ddl}");
    }

    #[tokio::test]
    async fn totp_seed_migration_is_idempotent_and_requires_original_key() {
        use crate::support::totp_vault::TotpVault;

        let db = memory_db().await;
        let mut cfg = test_config("");
        let temp = tempfile::tempdir().unwrap();
        cfg.security.totp_key_file = temp.path().join("totp.key").display().to_string();
        create_tables(&db).await.unwrap();
        let seed = crate::services::login_security::generate_totp_secret();
        let account = user::ActiveModel {
            username: Set("existing-totp-user".into()),
            tfa_enabled: Set(true),
            tfa_secret: Set(seed.clone()),
            ..Default::default()
        }.insert(&db).await.unwrap();
        record_schema_version(&db, 281).await.unwrap();

        apply_schema(&db, &cfg).await.unwrap();
        let vault = TotpVault::load(&cfg, false).unwrap();
        let row = user::Entity::find_by_id(account.id).one(&db).await.unwrap().unwrap();
        assert!(TotpVault::is_sealed(&row.tfa_secret));
        assert_ne!(row.tfa_secret, seed);
        assert_eq!(vault.open(row.id, &row.tfa_secret).unwrap(), seed);
        assert!(vault.open(row.id + 1, &row.tfa_secret).is_err());
        apply_schema(&db, &cfg).await.unwrap();
        let reloaded = user::Entity::find_by_id(account.id).one(&db).await.unwrap().unwrap();
        assert_eq!(reloaded.tfa_secret, row.tfa_secret);

        std::fs::rename(cfg.security.totp_key_path(), temp.path().join("original-key")).unwrap();
        assert!(apply_schema(&db, &cfg).await.is_err());
        assert!(!cfg.security.totp_key_path().exists());
    }

    #[tokio::test]
    async fn recording_profile_migration_preserves_legacy_recordings() {
        let db = memory_db().await;
        let cfg = test_config("");
        create_tables(&db).await.unwrap();
        let backend = db.get_database_backend();
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE record_files DROP COLUMN storage_profile_id".to_string(),
        )).await.unwrap();
        db.execute(Statement::from_string(
            backend,
            "INSERT INTO record_files (filename, storage_key) VALUES ('legacy.webm', 'original/legacy.webm')".to_string(),
        )).await.unwrap();
        record_schema_version(&db, 280).await.unwrap();

        apply_schema(&db, &cfg).await.unwrap();

        let row = record_file::Entity::find().one(&db).await.unwrap().unwrap();
        assert_eq!(row.storage_key, "original/legacy.webm");
        assert!(row.storage_profile_id.is_none());
        assert_eq!(latest_schema_version(&db).await.unwrap(), Some(DATABASE_VERSION));
    }

    #[tokio::test]
    async fn fresh_install_records_schema_migration_versions() {
        let db = memory_db().await;
        let cfg = test_config("");
        let i18n = I18n::load("en");

        migrate_and_seed(&db, &cfg, &i18n, "en").await.unwrap();

        let versions = version::Entity::find()
            .order_by_asc(version::Column::Version)
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.version)
            .collect::<Vec<_>>();

        assert_eq!(
            versions,
            vec![
                271,
                272,
                273,
                274,
                275,
                276,
                277,
                278,
                279,
                280,
                281,
                282,
                283,
                284,
                285,
                DATABASE_VERSION
            ]
        );
    }

    #[tokio::test]
    async fn file_audit_migration_keeps_legacy_reports_and_accepts_replay() {
        let db = memory_db().await;
        db.execute_unprepared(
            "CREATE TABLE audit_files (id INTEGER PRIMARY KEY, type INTEGER, path TEXT, info TEXT);
             INSERT INTO audit_files (id,type,path,info) VALUES (1,0,'legacy path','legacy details');",
        )
        .await
        .unwrap();
        let cfg = test_config("");
        migration_278(&db, &cfg).await.unwrap();
        migration_278(&db, &cfg).await.unwrap();
        let row = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT type, path, info, reported_type, conn_id FROM audit_files WHERE id = 1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i32>("", "type").unwrap(), 0);
        assert_eq!(row.try_get::<String>("", "path").unwrap(), "legacy path");
        assert_eq!(row.try_get::<String>("", "info").unwrap(), "legacy details");
        assert_eq!(row.try_get::<Option<i32>>("", "reported_type").unwrap(), None);
        assert_eq!(row.try_get::<Option<i32>>("", "conn_id").unwrap(), None);
    }

    #[tokio::test]
    async fn existing_device_groups_preserve_rows_and_gain_developer_notes() {
        let db = memory_db().await;
        db.execute_unprepared("CREATE TABLE device_groups (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '', created_at DATETIME, updated_at DATETIME)").await.unwrap();
        db.execute_unprepared("INSERT INTO device_groups (id,name) VALUES (7,'Existing fleet')")
            .await
            .unwrap();
        let cfg = test_config("");
        migrate_and_seed(&db, &cfg, &I18n::load("en"), "en")
            .await
            .unwrap();
        let group = device_group::Entity::find_by_id(7)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(group.name, "Existing fleet");
        assert_eq!(group.note, "");
        assert!(entity::developer_key::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn existing_version_271_database_migrates_address_book_note_column() {
        let db = memory_db().await;
        let cfg = test_config("");
        let i18n = I18n::load("en");
        let backend = db.get_database_backend();

        db.execute(
            backend.build(
                &Schema::new(DbBackend::Sqlite)
                    .create_table_from_entity(version::Entity)
                    .to_owned(),
            ),
        )
        .await
        .unwrap();
        version::ActiveModel {
            version: Set(271),
            created_at: Set(services::now()),
            updated_at: Set(services::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        db.execute(Statement::from_string(
            backend,
            "CREATE TABLE address_books (row_id INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL DEFAULT '')",
        ))
        .await
        .unwrap();

        migrate_and_seed(&db, &cfg, &i18n, "en").await.unwrap();

        assert!(column_exists(&db, "address_books", "note").await.unwrap());
        assert_eq!(
            latest_schema_version(&db).await.unwrap(),
            Some(DATABASE_VERSION)
        );
        assert_eq!(user::Entity::find().count(&db).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn existing_version_272_database_migrates_smtp_sender_name_column() {
        let db = memory_db().await;
        let cfg = test_config("");
        let i18n = I18n::load("en");
        let backend = db.get_database_backend();

        db.execute(
            backend.build(
                &Schema::new(DbBackend::Sqlite)
                    .create_table_from_entity(version::Entity)
                    .to_owned(),
            ),
        )
        .await
        .unwrap();
        version::ActiveModel {
            version: Set(272),
            created_at: Set(services::now()),
            updated_at: Set(services::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        db.execute(Statement::from_string(
            backend,
            "CREATE TABLE smtp_email_configs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL DEFAULT '',
                host TEXT NOT NULL DEFAULT '',
                port INTEGER NOT NULL DEFAULT 587,
                username TEXT NOT NULL DEFAULT '',
                password TEXT NOT NULL DEFAULT '',
                from_address TEXT NOT NULL DEFAULT '',
                tls TEXT NOT NULL DEFAULT 'starttls',
                enabled BOOLEAN NOT NULL DEFAULT FALSE,
                created_at DATETIME,
                updated_at DATETIME
            )",
        ))
        .await
        .unwrap();

        migrate_and_seed(&db, &cfg, &i18n, "en").await.unwrap();

        assert!(column_exists(&db, "smtp_email_configs", "from_name")
            .await
            .unwrap());
        assert_eq!(
            latest_schema_version(&db).await.unwrap(),
            Some(DATABASE_VERSION)
        );
        assert_eq!(user::Entity::find().count(&db).await.unwrap(), 0);
    }
}

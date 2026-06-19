//! Bootstrap: build the database connection, run schema sync + seed, and
//! assemble the shared [`AppState`]. Mirrors the Go
//! `InitGlobal` / `DatabaseAutoUpdate` / `Migrate` flow.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::{fmt, future::Future};

use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{
    ActiveModelTrait, ConnectOptions, ConnectionTrait, Database, DatabaseBackend,
    DatabaseConnection, EntityTrait, PaginatorTrait, QueryOrder, Schema, Set, Statement,
};

use entity::{
    active_connection, address_book, address_book_collection, address_book_collection_rule,
    audit_conn, audit_file, deployment_event, deployment_token, device_group,
    device_presence_state, group, login_log, login_verification, message, message_read, oauth,
    peer, record_file, server_cmd, share_record, smtp_email_config, strategy, strategy_assignment,
    system_setting, tag, trusted_login_device, user, user_third, user_token, version,
    webhook_delivery, webhook_subscription,
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
pub const DATABASE_VERSION: i32 = 273;

type MigrationFuture<'a> = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;
type MigrationFn = for<'a> fn(&'a DatabaseConnection, &'a Config) -> MigrationFuture<'a>;

struct SchemaMigration {
    version: i32,
    name: &'static str,
    apply: MigrationFn,
}

impl fmt::Display for SchemaMigration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.version, self.name)
    }
}

const SCHEMA_MIGRATIONS: &[SchemaMigration] = &[
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
];

/// Connect to the configured database.
pub async fn connect(config: &Config) -> anyhow::Result<DatabaseConnection> {
    let url = match config.db.r#type.as_str() {
        config::DB_TYPE_MYSQL => format!(
            "mysql://{}:{}@{}/{}",
            config.mysql.username, config.mysql.password, config.mysql.addr, config.mysql.dbname
        ),
        config::DB_TYPE_POSTGRESQL => format!(
            "postgres://{}:{}@{}:{}/{}?sslmode={}",
            config.postgresql.user,
            config.postgresql.password,
            config.postgresql.host,
            config.postgresql.port,
            config.postgresql.dbname,
            if config.postgresql.sslmode.is_empty() {
                "disable"
            } else {
                &config.postgresql.sslmode
            },
        ),
        _ => {
            // SQLite is the default and stores its data under ./data.
            std::fs::create_dir_all("./data").ok();
            "sqlite://./data/rustdeskapi.db?mode=rwc".to_string()
        }
    };

    let mut opt = ConnectOptions::new(url);
    if config.db.max_open_conns > 0 {
        opt.max_connections(config.db.max_open_conns);
    }
    if config.db.max_idle_conns > 0 {
        opt.min_connections(config.db.max_idle_conns);
    }
    opt.connect_timeout(Duration::from_secs(10));
    let db = Database::connect(opt).await?;
    Ok(db)
}

/// Create any missing tables. Existing tables are never altered here; schema
/// evolution is handled by the ordered migrations below.
async fn create_tables(db: &DatabaseConnection) -> anyhow::Result<()> {
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
    create!(record_file::Entity);
    create!(deployment_token::Entity);
    create!(deployment_event::Entity);
    create!(webhook_subscription::Entity);
    create!(webhook_delivery::Entity);
    create!(device_presence_state::Entity);
    create!(strategy::Entity);
    create!(strategy_assignment::Entity);
    create!(system_setting::Entity);
    create!(address_book_collection::Entity);
    create!(address_book_collection_rule::Entity);
    create!(server_cmd::Entity);
    create!(device_group::Entity);
    create!(active_connection::Entity);
    Ok(())
}

fn migration_271<'a>(db: &'a DatabaseConnection, config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move { migrate_271_legacy_columns(db, config).await })
}

fn migration_272<'a>(db: &'a DatabaseConnection, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move { migrate_272_address_book_notes(db).await })
}

fn migration_273<'a>(db: &'a DatabaseConnection, _config: &'a Config) -> MigrationFuture<'a> {
    Box::pin(async move { migrate_273_smtp_sender_names(db).await })
}

async fn migrate_271_legacy_columns(
    db: &DatabaseConnection,
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

async fn migrate_272_address_book_notes(db: &DatabaseConnection) -> anyhow::Result<()> {
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

async fn migrate_273_smtp_sender_names(db: &DatabaseConnection) -> anyhow::Result<()> {
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
    db: &DatabaseConnection,
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
    db: &DatabaseConnection,
    config: &Config,
) -> anyhow::Result<()> {
    let rows = record_file::Entity::find().all(db).await?;
    let root = services::record_file::record_root_for_config(
        &config.gin.resources_path,
        &config.record_storage.local_dir,
    );
    for row in rows
        .into_iter()
        .filter(|row| row.storage_key.trim().is_empty())
    {
        let filename = row.filename.clone();
        let mut am: record_file::ActiveModel = row.into();
        am.storage_backend = Set(record_file::STORAGE_LOCAL.to_string());
        am.storage_key = Set(root.join(filename).to_string_lossy().to_string());
        am.update(db).await?;
    }
    Ok(())
}

async fn latest_schema_version(db: &DatabaseConnection) -> anyhow::Result<Option<i32>> {
    Ok(version::Entity::find()
        .order_by_desc(version::Column::Version)
        .order_by_desc(version::Column::Id)
        .one(db)
        .await?
        .map(|v| v.version))
}

async fn record_schema_version(db: &DatabaseConnection, schema_version: i32) -> anyhow::Result<()> {
    let am = version::ActiveModel {
        version: Set(schema_version),
        created_at: Set(services::now()),
        updated_at: Set(services::now()),
        ..Default::default()
    };
    am.insert(db).await?;
    Ok(())
}

async fn run_schema_migrations(db: &DatabaseConnection, config: &Config) -> anyhow::Result<()> {
    let declared_latest = SCHEMA_MIGRATIONS
        .last()
        .map(|migration| migration.version)
        .unwrap_or(0);
    anyhow::ensure!(
        declared_latest == DATABASE_VERSION,
        "schema migration list latest version {declared_latest} does not match DATABASE_VERSION {DATABASE_VERSION}"
    );

    let current_version = latest_schema_version(db).await?.unwrap_or(0);
    for migration in SCHEMA_MIGRATIONS {
        if migration.version > current_version {
            tracing::info!("Applying schema migration {}", migration);
            (migration.apply)(db, config).await?;
            record_schema_version(db, migration.version).await?;
        } else {
            (migration.apply)(db, config).await?;
        }
    }
    Ok(())
}

/// Run schema sync + version bookkeeping + first-run seed.
pub async fn migrate_and_seed(
    db: &DatabaseConnection,
    config: &Config,
    i18n: &I18n,
    lang: &str,
) -> anyhow::Result<()> {
    let had_versions = version::Entity::find().count(db).await.unwrap_or(0) > 0;

    create_tables(db).await?;
    run_schema_migrations(db, config).await?;

    // First run ever: seed default groups + admin user. This is based on users
    // rather than version rows because fresh installs now record every schema
    // migration in the version table.
    let user_count = user::Entity::find().count(db).await.unwrap_or(0);
    if !had_versions && user_count == 0 {
        seed(db, config, i18n, lang).await?;
    }

    Ok(())
}

async fn seed(
    db: &DatabaseConnection,
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
    services::group::create(db, &default_name, group::TYPE_DEFAULT).await?;
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
        group_id: Set(1),
        password: Set(hash),
        must_change_password: Set(config.admin.force_change_password),
        created_at: Set(services::now()),
        updated_at: Set(services::now()),
        ..Default::default()
    };
    admin.insert(db).await?;
    Ok(())
}

/// Build the full application state from a parsed config.
pub async fn build_state(config: Config, config_path: PathBuf) -> anyhow::Result<AppState> {
    let db = connect(&config).await?;
    let i18n = I18n::load(&config.lang);

    migrate_and_seed(&db, &config, &i18n, &config.lang).await?;

    let jwt = Jwt::new(&config.jwt.key, config.jwt.expire_duration());
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
        db,
        admin_config: Arc::new(AdminConfigStore::new(
            config_path.clone(),
            admin_panel_config,
        )),
        webclient_config: Arc::new(WebClientConfigStore::new(
            config_path.clone(),
            WebClientConfig::from(&config.rustdesk),
        )),
        record_storage_config: Arc::new(RecordStorageConfigStore::new(config_path, record_storage)),
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

        assert_eq!(versions, vec![271, 272, DATABASE_VERSION]);
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

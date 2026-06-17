//! Bootstrap: build the database connection, run the AutoMigrate-equivalent
//! schema sync + seed, and assemble the shared [`AppState`]. Mirrors the Go
//! `InitGlobal` / `DatabaseAutoUpdate` / `Migrate` flow.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{
    ActiveModelTrait, ConnectOptions, ConnectionTrait, Database, DatabaseBackend,
    DatabaseConnection, EntityTrait, PaginatorTrait, QueryOrder, Schema, Set, Statement,
};

use entity::{
    active_connection, address_book, address_book_collection, address_book_collection_rule,
    audit_conn, audit_file, device_group, group, login_log, oauth, peer, record_file, server_cmd,
    share_record, tag, user, user_third, user_token, version,
};

use crate::config::{self, Config};
use crate::i18n::I18n;
use crate::services;
use crate::state::AppState;
use crate::support::jwt::Jwt;
use crate::support::login_limiter::{LoginLimiter, SecurityPolicy};
use crate::support::record_storage_config::RecordStorageConfigStore;
use crate::support::webclient_config::{WebClientConfig, WebClientConfigStore};
use crate::support::{external_webclient::ExternalWebClient, password, random};

/// The schema version the binary expects (mirrors Go `DatabaseVersion`).
pub const DATABASE_VERSION: i32 = 268;

/// Connect to the configured database.
pub async fn connect(config: &Config) -> anyhow::Result<DatabaseConnection> {
    let url = match config.gorm.r#type.as_str() {
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
            // sqlite, matching the Go default ./data/rustdeskapi.db
            std::fs::create_dir_all("./data").ok();
            "sqlite://./data/rustdeskapi.db?mode=rwc".to_string()
        }
    };

    let mut opt = ConnectOptions::new(url);
    if config.gorm.max_open_conns > 0 {
        opt.max_connections(config.gorm.max_open_conns);
    }
    if config.gorm.max_idle_conns > 0 {
        opt.min_connections(config.gorm.max_idle_conns);
    }
    opt.connect_timeout(Duration::from_secs(10));
    let db = Database::connect(opt).await?;
    Ok(db)
}

/// Create any missing tables (≈ GORM AutoMigrate) and return whether the
/// `versions` table existed beforehand.
async fn create_tables(db: &DatabaseConnection, config: &Config) -> anyhow::Result<()> {
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
    create!(tag::Entity);
    create!(address_book::Entity);
    create!(peer::Entity);
    create!(group::Entity);
    create!(user_third::Entity);
    create!(oauth::Entity);
    create!(login_log::Entity);
    create!(share_record::Entity);
    create!(audit_conn::Entity);
    create!(audit_file::Entity);
    create!(record_file::Entity);
    create!(address_book_collection::Entity);
    create!(address_book_collection_rule::Entity);
    create!(server_cmd::Entity);
    create!(device_group::Entity);
    create!(active_connection::Entity);
    add_missing_columns(db).await?;
    backfill_record_storage_keys(db, config).await?;
    Ok(())
}

async fn add_missing_columns(db: &DatabaseConnection) -> anyhow::Result<()> {
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

/// Run schema sync + version bookkeeping + first-run seed.
pub async fn migrate_and_seed(
    db: &DatabaseConnection,
    config: &Config,
    i18n: &I18n,
    lang: &str,
) -> anyhow::Result<()> {
    let had_versions = version::Entity::find().count(db).await.unwrap_or(0) > 0;

    create_tables(db, config).await?;

    let latest = version::Entity::find()
        .order_by_desc(version::Column::Id)
        .one(db)
        .await?;

    let need_version_row = match &latest {
        None => true,
        Some(v) => v.version < DATABASE_VERSION,
    };

    if need_version_row {
        tracing::info!("Migrating.... {}", DATABASE_VERSION);
        let am = version::ActiveModel {
            version: Set(DATABASE_VERSION),
            created_at: Set(services::now()),
            updated_at: Set(services::now()),
            ..Default::default()
        };
        am.insert(db).await?;
    }

    // First run ever: seed default groups + admin user.
    let version_count = version::Entity::find().count(db).await?;
    if !had_versions && version_count == 1 {
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

    let configured_password = !config.admin.password.is_empty();
    let pwd = if configured_password {
        config.admin.password.clone()
    } else {
        random::random_string(8)
    };
    tracing::info!("Admin Username Is: {}", config.admin.username);
    if configured_password {
        tracing::info!("Admin password loaded from config/env.");
    } else {
        tracing::info!("Admin Password Is: {}", pwd);
    }
    let hash = password::encrypt_password(&pwd)?;
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
    let external_webclient = ExternalWebClient::try_load_from_default_zip()
        .await
        .map(Arc::new);

    Ok(AppState {
        db,
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

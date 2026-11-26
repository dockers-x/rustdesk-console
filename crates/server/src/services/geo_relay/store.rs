use entity::{geo_mmdb_download, system_setting};
use sea_orm::{
    sea_query::OnConflict, ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter, Set, TransactionTrait,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{config::Config, services::now, support::totp_vault::TotpVault};

use super::types::{
    validate_settings, GeoSettings, MmdbKind, MmdbSourceMode, MmdbSourceResult, MmdbSourceUpdate,
    MmdbSourceView, MmdbSources, MmdbUpdatePolicy,
};

const GEO_SETTINGS_KEY: &str = "rustdesk_geo_settings";
const MMDB_SOURCES_KEY: &str = "rustdesk_mmdb_sources";
const MMDB_UPDATE_POLICY_KEY: &str = "rustdesk_mmdb_update_policy";

const SOURCE_CONFIG_ID: i32 = 1;
const MAXMIND_KEY_BINDING: &[u8] = b"rustdesk-geo:maxmind-license-key:v1";

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct SourceConfig {
    pub mode: MmdbSourceMode,
    pub custom: MmdbSources,
    account_id: String,
    sealed_license_key: String,
}

impl SourceConfig {
    pub fn view(&self) -> MmdbSourceView {
        MmdbSourceView {
            mode: self.mode,
            maxmind_account_id: self.account_id.clone(),
            maxmind_key_configured: !self.sealed_license_key.is_empty(),
        }
    }

    pub fn maxmind_credentials(&self, config: &Config) -> Result<(&str, String), String> {
        if self.account_id.is_empty() || self.sealed_license_key.is_empty() {
            return Err(
                "Configure a MaxMind account ID and license key under Download source".to_owned(),
            );
        }
        let license_key = TotpVault::load(config, false)
            .and_then(|vault| vault.open_bound(MAXMIND_KEY_BINDING, &self.sealed_license_key))
            .map_err(|_| "Cannot decrypt the configured MaxMind license key".to_owned())?;
        Ok((&self.account_id, license_key))
    }

    fn update_maxmind(
        &mut self,
        config: &Config,
        account_id: Option<&str>,
        license_key: Option<&str>,
    ) -> Result<(), String> {
        let next_account = account_id.map(str::trim).unwrap_or(&self.account_id);
        if next_account.is_empty()
            || next_account.len() > 20
            || !next_account.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err("Enter a numeric MaxMind account ID".to_owned());
        }
        if let Some(license_key) = license_key.map(str::trim).filter(|key| !key.is_empty()) {
            if license_key.len() > 256 || !license_key.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err("Enter a valid MaxMind license key".to_owned());
            }
            self.sealed_license_key = TotpVault::load(config, true)
                .and_then(|vault| vault.seal_bound(MAXMIND_KEY_BINDING, license_key))
                .map_err(|_| "Could not encrypt the MaxMind license key".to_owned())?;
        } else if self.sealed_license_key.is_empty() || next_account != self.account_id {
            return Err("Enter the license key for this MaxMind account".to_owned());
        }
        self.account_id = next_account.to_owned();
        Ok(())
    }
}

pub(super) async fn load_source_config(
    connection: &impl ConnectionTrait,
) -> Result<SourceConfig, String> {
    let row = geo_mmdb_download::Entity::find_by_id(SOURCE_CONFIG_ID)
        .one(connection)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(row) = row.filter(|row| row.value != "{}") {
        return serde_json::from_str(&row.value)
            .map_err(|_| "Invalid stored GeoIP download configuration".to_owned());
    }
    Ok(SourceConfig {
        custom: load_json(connection, MMDB_SOURCES_KEY).await?,
        ..SourceConfig::default()
    })
}

pub(super) async fn save_source_config(
    db: &DatabaseConnection,
    config: &Config,
    update: &MmdbSourceUpdate,
) -> Result<MmdbSourceResult, String> {
    if update.mode == MmdbSourceMode::Custom {
        validate_sources(&update.custom)?;
    }
    let transaction = db.begin().await.map_err(|error| error.to_string())?;
    let mut source = lock_source_config(&transaction).await?;
    source.mode = update.mode;
    match update.mode {
        MmdbSourceMode::Custom => merge_sources(&mut source.custom, &update.custom),
        MmdbSourceMode::Maxmind => source.update_maxmind(
            config,
            update.maxmind_account_id.as_deref(),
            update.maxmind_license_key.as_deref(),
        )?,
    }
    write_source_config(&transaction, &source).await?;
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())?;
    Ok(MmdbSourceResult {
        source: source.view(),
        custom: source.custom,
    })
}

pub async fn load_settings(db: &DatabaseConnection) -> Result<GeoSettings, String> {
    load_json(db, GEO_SETTINGS_KEY).await
}

pub async fn load_update_policy(db: &DatabaseConnection) -> Result<MmdbUpdatePolicy, String> {
    load_json(db, MMDB_UPDATE_POLICY_KEY).await
}

pub async fn save_update_policy(
    db: &DatabaseConnection,
    policy: MmdbUpdatePolicy,
) -> Result<MmdbUpdatePolicy, String> {
    policy.validate()?;
    upsert_json(db, MMDB_UPDATE_POLICY_KEY, &policy).await?;
    Ok(policy)
}

pub async fn save_settings(
    db: &DatabaseConnection,
    mut input: GeoSettings,
    relay_pool: &[String],
) -> Result<GeoSettings, String> {
    validate_settings(&mut input, relay_pool)?;
    let transaction = db.begin().await.map_err(|error| error.to_string())?;
    let current_row = system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(GEO_SETTINGS_KEY))
        .one(&transaction)
        .await
        .map_err(|error| error.to_string())?;
    let current = match &current_row {
        Some(row) => serde_json::from_str::<GeoSettings>(&row.value)
            .map_err(|error| format!("stored setting '{GEO_SETTINGS_KEY}' is invalid: {error}"))?,
        None => GeoSettings::default(),
    };
    if input.revision != current.revision {
        return Err(format!(
            "settings revision conflict: submitted {}, current {}",
            input.revision, current.revision
        ));
    }
    if same_content(&input, &current) {
        transaction
            .commit()
            .await
            .map_err(|error| error.to_string())?;
        return Ok(current);
    }
    input.revision = current
        .revision
        .checked_add(1)
        .ok_or_else(|| "settings revision is exhausted".to_owned())?;
    let encoded = serde_json::to_string(&input).map_err(|error| error.to_string())?;
    match current_row {
        Some(row) => {
            let result = system_setting::Entity::update_many()
                .col_expr(
                    system_setting::Column::Value,
                    sea_orm::sea_query::Expr::value(encoded),
                )
                .col_expr(
                    system_setting::Column::UpdatedAt,
                    sea_orm::sea_query::Expr::value(now()),
                )
                .filter(system_setting::Column::Id.eq(row.id))
                .filter(system_setting::Column::Value.eq(row.value))
                .exec(&transaction)
                .await
                .map_err(|error| error.to_string())?;
            if result.rows_affected != 1 {
                return Err("settings revision conflict; reload and try again".to_owned());
            }
        }
        None => {
            system_setting::ActiveModel {
                key: Set(GEO_SETTINGS_KEY.to_owned()),
                value: Set(encoded),
                created_at: Set(now()),
                updated_at: Set(now()),
                ..Default::default()
            }
            .insert(&transaction)
            .await
            .map_err(|_| "settings revision conflict; reload and try again".to_owned())?;
        }
    }
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())?;
    Ok(input)
}

pub(super) async fn save_sources(
    db: &DatabaseConnection,
    changes: &MmdbSources,
) -> Result<MmdbSources, String> {
    validate_sources(changes)?;
    let transaction = db.begin().await.map_err(|error| error.to_string())?;
    let mut source = lock_source_config(&transaction).await?;
    merge_sources(&mut source.custom, changes);
    write_source_config(&transaction, &source).await?;
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())?;
    Ok(source.custom)
}

fn validate_sources(changes: &MmdbSources) -> Result<(), String> {
    for kind in MmdbKind::ALL {
        if let Some(url) = changes
            .get(kind)
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            super::mmdb::validate_url(url)
                .map_err(|error| format!("{}: {error}", kind.file_name()))?;
        }
    }
    Ok(())
}

fn merge_sources(sources: &mut MmdbSources, changes: &MmdbSources) {
    for kind in MmdbKind::ALL {
        if let Some(url) = changes.get(kind).map(str::trim) {
            sources.set(kind, (!url.is_empty()).then(|| url.to_owned()));
        }
    }
}

async fn lock_source_config(
    transaction: &sea_orm::DatabaseTransaction,
) -> Result<SourceConfig, String> {
    // The singleton primary key serializes the first write and later edits
    // on all supported backends, including databases without unique setting keys.
    geo_mmdb_download::Entity::insert(geo_mmdb_download::ActiveModel {
        id: Set(SOURCE_CONFIG_ID),
        value: Set("{}".to_owned()),
    })
    .on_conflict(
        OnConflict::column(geo_mmdb_download::Column::Id)
            .update_column(geo_mmdb_download::Column::Id)
            .to_owned(),
    )
    .exec_without_returning(transaction)
    .await
    .map_err(|error| error.to_string())?;
    load_source_config(transaction).await
}

async fn write_source_config(
    transaction: &sea_orm::DatabaseTransaction,
    source: &SourceConfig,
) -> Result<(), String> {
    let value = serde_json::to_string(source).map_err(|error| error.to_string())?;
    geo_mmdb_download::ActiveModel {
        id: Set(SOURCE_CONFIG_ID),
        value: Set(value),
    }
    .update(transaction)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

async fn load_json<C, T>(connection: &C, key: &str) -> Result<T, String>
where
    C: ConnectionTrait,
    T: Default + DeserializeOwned,
{
    let row = system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(key))
        .one(connection)
        .await
        .map_err(|error| error.to_string())?;
    match row {
        Some(row) => serde_json::from_str(&row.value)
            .map_err(|error| format!("stored setting '{key}' is invalid: {error}")),
        None => Ok(T::default()),
    }
}

async fn upsert_json<C, T>(connection: &C, key: &str, value: &T) -> Result<(), String>
where
    C: ConnectionTrait,
    T: Serialize,
{
    let encoded = serde_json::to_string(value).map_err(|error| error.to_string())?;
    let row = system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(key))
        .one(connection)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(row) = row {
        let mut active: system_setting::ActiveModel = row.into();
        active.value = Set(encoded);
        active.updated_at = Set(now());
        active
            .update(connection)
            .await
            .map_err(|error| error.to_string())?;
    } else {
        system_setting::ActiveModel {
            key: Set(key.to_owned()),
            value: Set(encoded),
            created_at: Set(now()),
            updated_at: Set(now()),
            ..Default::default()
        }
        .insert(connection)
        .await
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn same_content(left: &GeoSettings, right: &GeoSettings) -> bool {
    left.version == right.version && left.enabled == right.enabled && left.rules == right.rules
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::geo_relay::types::{EndpointExpressions, GeoRule};
    use sea_orm::{ConnectionTrait, Database, Schema};

    async fn test_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(db.get_database_backend());
        for statement in [
            schema.create_table_from_entity(system_setting::Entity),
            schema.create_table_from_entity(geo_mmdb_download::Entity),
        ] {
            db.execute(db.get_database_backend().build(&statement))
                .await
                .unwrap();
        }
        db
    }

    fn settings(revision: u64) -> GeoSettings {
        GeoSettings {
            revision,
            enabled: true,
            rules: vec![GeoRule {
                name: "Mainland".to_owned(),
                symmetric: true,
                expressions: EndpointExpressions {
                    client_a: "country:CN".to_owned(),
                    client_b: "*".to_owned(),
                },
                relays: vec!["relay.example.com:21117".to_owned()],
            }],
            ..GeoSettings::default()
        }
    }

    #[tokio::test]
    async fn maxmind_credentials_are_sealed_and_custom_sources_survive_switching() {
        let db = test_db().await;
        let directory =
            std::env::temp_dir().join(format!("rd-maxmind-key-{}", uuid::Uuid::new_v4()));
        let mut config = Config::default();
        config.security.totp_key_file = directory.join("totp.key").to_string_lossy().into_owned();
        let city = "https://files.example.test/GeoLite2-City.mmdb";
        upsert_json(
            &db,
            MMDB_SOURCES_KEY,
            &MmdbSources {
                city: Some(city.to_owned()),
                ..MmdbSources::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            load_source_config(&db)
                .await
                .unwrap()
                .custom
                .city
                .as_deref(),
            Some(city)
        );
        let mut official = MmdbSourceUpdate {
            mode: MmdbSourceMode::Maxmind,
            maxmind_account_id: Some("12345".into()),
            maxmind_license_key: Some("fixture-license".into()),
            ..MmdbSourceUpdate::default()
        };
        let saved = save_source_config(&db, &config, &official).await.unwrap();
        assert_eq!(saved.custom.city.as_deref(), Some(city));
        assert!(saved.source.maxmind_key_configured);
        assert!(!serde_json::to_string(&saved)
            .unwrap()
            .contains("fixture-license"));
        let row = geo_mmdb_download::Entity::find_by_id(SOURCE_CONFIG_ID)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(!row.value.contains("fixture-license"));
        assert_eq!(
            load_source_config(&db)
                .await
                .unwrap()
                .maxmind_credentials(&config)
                .unwrap(),
            ("12345", "fixture-license".into())
        );
        save_source_config(&db, &config, &MmdbSourceUpdate::default())
            .await
            .unwrap();
        official.maxmind_license_key = None;
        save_source_config(&db, &config, &official).await.unwrap();
        official.maxmind_account_id = Some("98765".into());
        assert!(save_source_config(&db, &config, &official).await.is_err());
        assert_eq!(
            load_source_config(&db)
                .await
                .unwrap()
                .view()
                .maxmind_account_id,
            "12345"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn source_updates_validate_all_addresses_before_merging() {
        let db = test_db().await;
        let country = "https://files.example.test/GeoLite2-Country.mmdb";
        let city = "https://files.example.test/GeoLite2-City.mmdb";
        let mut changes = MmdbSources {
            country: Some(format!(" {country} ")),
            ..MmdbSources::default()
        };
        let saved = save_sources(&db, &changes).await.unwrap();
        assert_eq!(saved.country.as_deref(), Some(country));

        changes.city = Some(city.to_owned());
        changes.asn = Some("http://files.example.test/GeoLite2-ASN.mmdb".to_owned());
        assert!(save_sources(&db, &changes).await.is_err());
        assert_eq!(load_source_config(&db).await.unwrap().custom, saved);

        let changes = MmdbSources {
            city: Some(city.to_owned()),
            ..MmdbSources::default()
        };
        let saved = save_sources(&db, &changes).await.unwrap();
        assert_eq!(saved.country.as_deref(), Some(country));
        assert_eq!(saved.city.as_deref(), Some(city));

        let saved = save_sources(
            &db,
            &MmdbSources {
                country: Some(String::new()),
                ..MmdbSources::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(saved.country, None);
        assert_eq!(saved.city.as_deref(), Some(city));
        assert_eq!(load_source_config(&db).await.unwrap().custom, saved);
    }

    #[tokio::test]
    async fn saves_settings_and_rejects_stale_revision() {
        let db = test_db().await;
        let relays = ["relay.example.com:21117".to_owned()];
        let saved = save_settings(&db, settings(0), &relays).await.unwrap();
        assert_eq!(saved.revision, 1);
        assert_eq!(load_settings(&db).await.unwrap(), saved);

        let error = save_settings(&db, settings(0), &relays).await.unwrap_err();
        assert!(error.contains("revision conflict"));
        assert_eq!(load_settings(&db).await.unwrap(), saved);
    }

    #[tokio::test]
    async fn invalid_settings_do_not_write_database() {
        let db = test_db().await;
        let mut invalid = settings(0);
        invalid.rules[0].expressions.client_a = "unsupported:value".to_owned();
        assert!(
            save_settings(&db, invalid, &["relay.example.com:21117".to_owned()])
                .await
                .is_err()
        );
        assert_eq!(load_settings(&db).await.unwrap(), GeoSettings::default());
    }

    #[tokio::test]
    async fn relay_pool_changes_must_preserve_geo_rule_references() {
        let db = test_db().await;
        let original_relays = ["relay.example.com:21117".to_owned()];
        save_settings(&db, settings(0), &original_relays)
            .await
            .unwrap();

        assert!(crate::services::geo_relay::validate_relay_pool_change(
            &db,
            &["other.example.com:21117".to_owned()]
        )
        .await
        .is_err());
        assert!(
            crate::services::geo_relay::validate_relay_pool_change(&db, &original_relays)
                .await
                .is_ok()
        );
    }
}

mod handoff;
pub mod mmdb;
mod store;
pub mod types;

use serde::{Deserialize, Serialize};

use crate::{config::Config, services::server_cmd};
use sea_orm::DatabaseConnection;

pub use types::{
    GeoOverview, GeoSettings, MmdbKind, MmdbSourceUpdate, MmdbStatus, MmdbUpdatePolicy,
    SaveSettingsResult,
};

pub async fn overview(db: &DatabaseConnection, config: &Config) -> Result<GeoOverview, String> {
    let settings = store::load_settings(db).await?;
    let update_policy = store::load_update_policy(db).await?;
    let source = store::load_source_config(db).await?;
    let databases = MmdbKind::ALL
        .into_iter()
        .map(
            |kind| match mmdb::database_status(config, kind, Some(&source.custom)) {
                Ok(status) => status,
                Err(error) => MmdbStatus {
                    kind,
                    path: None,
                    is_present: false,
                    size_bytes: None,
                    modified_at: None,
                    database_type: None,
                    build_epoch: None,
                    has_backup: false,
                    source_url: source.custom.get(kind).map(str::to_owned),
                    error: Some(error),
                },
            },
        )
        .collect();
    let runtime = runtime_status(config).await;
    Ok(GeoOverview {
        settings,
        databases,
        update_policy,
        source: source.view(),
        runtime,
    })
}

pub async fn save_update_policy(
    db: &DatabaseConnection,
    policy: MmdbUpdatePolicy,
) -> Result<MmdbUpdatePolicy, String> {
    store::save_update_policy(db, policy).await
}

pub async fn save_sources(
    db: &DatabaseConnection,
    config: &Config,
    update: &MmdbSourceUpdate,
) -> Result<types::MmdbSourceResult, String> {
    store::save_source_config(db, config, update).await
}

pub async fn validate_relay_pool_change(
    db: &DatabaseConnection,
    relay_servers: &[String],
) -> Result<(), String> {
    let mut settings = store::load_settings(db).await?;
    types::validate_settings(&mut settings, relay_servers).map_err(|error| {
        format!("Relay Pool would invalidate the saved Geo routing rules: {error}")
    })
}

pub async fn save_and_apply(
    db: &DatabaseConnection,
    config: &Config,
    input: GeoSettings,
) -> Result<SaveSettingsResult, String> {
    let _guard = server_cmd::acquire_relay_configuration_lock(config)?;
    let relay_pool = server_cmd::load_relay_pool(db)
        .await
        .map_err(|error| error.to_string())?;
    let mut input = input;
    types::validate_settings(&mut input, &relay_pool.servers)?;
    handoff::validate_size(&input, &relay_pool.servers)?;
    let settings = store::save_settings(db, input, &relay_pool.servers).await?;
    match handoff::apply(config, &settings, &relay_pool.servers).await {
        Ok(message) => Ok(SaveSettingsResult {
            settings,
            is_persisted: true,
            is_applied: true,
            apply_message: Some(message),
        }),
        Err(error) => Ok(SaveSettingsResult {
            settings,
            is_persisted: true,
            is_applied: false,
            apply_message: Some(error),
        }),
    }
}

pub async fn apply_persisted(db: &DatabaseConnection, config: &Config) -> Result<String, String> {
    let _guard = server_cmd::acquire_relay_configuration_lock(config)?;
    let settings = store::load_settings(db).await?;
    if settings.revision == 0 {
        return Ok("Geo settings have not been configured".to_owned());
    }
    let relay_pool = server_cmd::load_relay_pool(db)
        .await
        .map_err(|error| error.to_string())?;
    handoff::apply(config, &settings, &relay_pool.servers).await
}

pub async fn download_database(
    db: &DatabaseConnection,
    config: &Config,
    kind: MmdbKind,
    source_url: Option<&str>,
) -> Result<(MmdbStatus, bool, Option<String>, bool, Option<String>), String> {
    let _guard = mmdb::acquire_mutation_lock(config, kind)?;
    let source = store::load_source_config(db).await?;
    if source.mode == types::MmdbSourceMode::Maxmind {
        let (account_id, license_key) = source.maxmind_credentials(config)?;
        let status = mmdb::download(
            config,
            kind,
            mmdb::DownloadSource::Maxmind {
                account_id,
                license_key: &license_key,
            },
        )
        .await?;
        let (is_reloaded, reload_message) = reload_database(config, kind).await;
        return Ok((status, is_reloaded, reload_message, true, None));
    }

    let source_url = source_url
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .or_else(|| source.custom.get(kind))
        .ok_or_else(|| "Configure a custom download URL for this database".to_owned())?;
    let mut status = mmdb::download(config, kind, mmdb::DownloadSource::Custom(source_url)).await?;
    let mut change = types::MmdbSources::default();
    change.set(kind, Some(source_url.to_owned()));
    let (is_source_saved, source_message) = match store::save_sources(db, &change).await {
        Ok(sources) => {
            status.source_url = sources.get(kind).map(str::to_owned);
            (true, None)
        }
        Err(error) => (
            false,
            Some(format!(
                "MMDB was installed, but its source URL could not be saved: {error}"
            )),
        ),
    };
    let (is_reloaded, reload_message) = reload_database(config, kind).await;
    Ok((
        status,
        is_reloaded,
        reload_message,
        is_source_saved,
        source_message,
    ))
}

pub async fn restore_database(
    config: &Config,
    kind: MmdbKind,
) -> Result<(MmdbStatus, bool, Option<String>), String> {
    let _guard = mmdb::acquire_mutation_lock(config, kind)?;
    let status = mmdb::restore_backup(config, kind)?;
    let (is_reloaded, reload_message) = reload_database(config, kind).await;
    Ok((status, is_reloaded, reload_message))
}

async fn reload_database(config: &Config, kind: MmdbKind) -> (bool, Option<String>) {
    let response = match server_cmd::send_target_cmd(
        config,
        entity::server_cmd::TARGET_ID_SERVER,
        "reload-geo",
        "",
    )
    .await
    {
        Ok(response) if !response.trim().is_empty() => response,
        Ok(_) => {
            return (
                false,
                Some("HBBS did not recognize the Geo reload command".to_owned()),
            )
        }
        Err(error) => return (false, Some(error)),
    };
    let raw_status = match server_cmd::send_target_cmd(
        config,
        entity::server_cmd::TARGET_ID_SERVER,
        "geo-status",
        "",
    )
    .await
    {
        Ok(status) => status,
        Err(error) => {
            return (
                false,
                Some(format!("{response}; status check failed: {error}")),
            )
        }
    };
    match serde_json::from_str::<HbbsGeoStatus>(raw_status.trim()) {
        Ok(status) if status.database_available(kind) => (true, Some(response)),
        Ok(_) => (
            false,
            Some(format!(
                "{response}; HBBS did not report the {} database as loaded",
                kind.file_name()
            )),
        ),
        Err(_) => (
            false,
            Some(format!(
                "{response}; HBBS returned an unrecognized Geo status"
            )),
        ),
    }
}

pub async fn test_rule(
    config: &Config,
    client_a: std::net::IpAddr,
    client_b: Option<std::net::IpAddr>,
) -> Result<String, String> {
    let argument = match client_b {
        Some(client_b) => format!("{client_a} {client_b}"),
        None => client_a.to_string(),
    };
    server_cmd::send_target_cmd(
        config,
        entity::server_cmd::TARGET_ID_SERVER,
        "test-geo",
        &argument,
    )
    .await
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum PreviewReason {
    RuleMatch,
    Disabled,
    NoEligibleMatch,
    NoRelay,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoPreview {
    revision: u64,
    reason: PreviewReason,
    relay: Option<String>,
    rule: Option<String>,
    candidates: Vec<String>,
    warnings: Vec<String>,
}

pub async fn preview_rule(
    config: &Config,
    client_a: std::net::IpAddr,
    client_b: Option<std::net::IpAddr>,
) -> Result<Option<GeoPreview>, String> {
    let client_b = client_b.unwrap_or(client_a);
    let raw = server_cmd::send_target_cmd(
        config,
        entity::server_cmd::TARGET_ID_SERVER,
        "geo-preview",
        &format!("{client_a} {client_b}"),
    )
    .await?;
    // Older HBBS builds reply with an empty response to unknown commands.
    if raw.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|error| format!("HBBS returned an invalid Geo preview: {error}"))
}

pub fn cleanup_stale_handoffs(config: &Config) {
    match handoff::cleanup_stale(config) {
        Ok(count) if count > 0 => tracing::info!("removed {count} stale Geo handoff files"),
        Ok(_) => {}
        Err(error) => tracing::debug!("Geo handoff cleanup skipped: {error}"),
    }
}

pub fn spawn_startup_sync(db: DatabaseConnection, config: std::sync::Arc<Config>) {
    tokio::spawn(async move {
        for attempt in 1..=6 {
            match apply_persisted(&db, &config).await {
                Ok(message) => {
                    tracing::info!("Geo settings startup sync: {message}");
                    return;
                }
                Err(error) if attempt < 6 => {
                    tracing::debug!("Geo settings startup sync attempt {attempt} failed: {error}");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                Err(error) => {
                    tracing::warn!("Geo settings startup sync failed: {error}");
                    return;
                }
            }
        }
    });
}

pub fn spawn_mmdb_update_worker(db: DatabaseConnection, config: std::sync::Arc<Config>) {
    tokio::spawn(async move {
        let mut last_attempts = std::collections::HashMap::new();
        loop {
            if let Err(error) = update_due_databases(&db, &config, &mut last_attempts).await {
                tracing::warn!("automatic MMDB update check failed: {error}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(15 * 60)).await;
        }
    });
}

async fn update_due_databases(
    db: &DatabaseConnection,
    config: &Config,
    last_attempts: &mut std::collections::HashMap<MmdbKind, std::time::Instant>,
) -> Result<(), String> {
    let policy = store::load_update_policy(db).await?;
    if !policy.enabled {
        return Ok(());
    }
    let source = store::load_source_config(db).await?;
    let interval = std::time::Duration::from_secs(u64::from(policy.interval_hours) * 60 * 60);
    for kind in MmdbKind::ALL {
        if source.mode == types::MmdbSourceMode::Custom && source.custom.get(kind).is_none() {
            continue;
        }
        if !mmdb::is_update_due(config, kind, interval)? {
            continue;
        }
        if last_attempts
            .get(&kind)
            .map(|attempt| attempt.elapsed() < interval)
            .unwrap_or(false)
        {
            continue;
        }
        last_attempts.insert(kind, std::time::Instant::now());
        // Reuse the selected source without writing the download snapshot
        // back over an administrator's source edits.
        let result = match source.mode {
            types::MmdbSourceMode::Custom => {
                let Some(url) = source.custom.get(kind) else {
                    continue;
                };
                download_automatic_update(config, kind, mmdb::DownloadSource::Custom(url)).await
            }
            types::MmdbSourceMode::Maxmind => match source.maxmind_credentials(config) {
                Ok((account_id, license_key)) => {
                    download_automatic_update(
                        config,
                        kind,
                        mmdb::DownloadSource::Maxmind {
                            account_id,
                            license_key: &license_key,
                        },
                    )
                    .await
                }
                Err(error) => Err(error),
            },
        };
        match result {
            Ok((is_reloaded, reload_message)) => {
                tracing::info!(
                    database_kind = ?kind,
                    reloaded = is_reloaded,
                    reload_message,
                    "automatic MMDB update completed"
                );
            }
            Err(error) => {
                if error.starts_with("another ") {
                    last_attempts.remove(&kind);
                }
                tracing::warn!(database_kind = ?kind, "automatic MMDB update failed: {error}");
            }
        }
    }
    Ok(())
}

async fn download_automatic_update(
    config: &Config,
    kind: MmdbKind,
    source: mmdb::DownloadSource<'_>,
) -> Result<(bool, Option<String>), String> {
    let _guard = mmdb::acquire_mutation_lock(config, kind)?;
    mmdb::download(config, kind, source).await?;
    Ok(reload_database(config, kind).await)
}

async fn runtime_status(config: &Config) -> types::RuntimeStatus {
    let raw = match server_cmd::send_target_cmd(
        config,
        entity::server_cmd::TARGET_ID_SERVER,
        "geo-status",
        "",
    )
    .await
    {
        Ok(raw) => raw,
        Err(error) => {
            return types::RuntimeStatus {
                error: Some(error),
                ..types::RuntimeStatus::default()
            }
        }
    };
    match serde_json::from_str::<HbbsGeoStatus>(raw.trim()) {
        Ok(status) => types::RuntimeStatus {
            is_available: true,
            applied_revision: Some(status.revision),
            is_geo_enabled: Some(status.enabled),
            rule_count: Some(status.rule_count),
            country_available: Some(status.country_available),
            city_available: Some(status.city_available),
            asn_available: Some(status.asn_available),
            warnings: status.warnings,
            raw: Some(raw),
            error: None,
        },
        Err(_) => types::RuntimeStatus {
            is_available: true,
            raw: Some(raw),
            error: Some("HBBS returned an unrecognized Geo status".to_owned()),
            ..types::RuntimeStatus::default()
        },
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HbbsGeoStatus {
    revision: u64,
    enabled: bool,
    rule_count: usize,
    country_available: bool,
    city_available: bool,
    asn_available: bool,
    #[serde(default)]
    warnings: Vec<String>,
}

impl HbbsGeoStatus {
    fn database_available(&self, kind: MmdbKind) -> bool {
        match kind {
            MmdbKind::Country => self.country_available,
            MmdbKind::City => self.city_available,
            MmdbKind::Asn => self.asn_available,
        }
    }
}

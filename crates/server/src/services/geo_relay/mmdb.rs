use chrono::{DateTime, Utc};
use fs2::FileExt;
use maxminddb::Reader;
use reqwest::{redirect::Policy, Client, Url};
use std::{
    fs::OpenOptions,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tokio::{io::AsyncWriteExt, net::lookup_host};
use uuid::Uuid;

use crate::config::Config;

use super::{
    handoff,
    types::{MmdbKind, MmdbSources, MmdbStatus},
};

const MAX_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SOURCE_URL_BYTES: usize = 4096;

pub fn acquire_mutation_lock(config: &Config, kind: MmdbKind) -> Result<std::fs::File, String> {
    let directory = handoff::data_dir(config)?;
    let path = directory.join(".geo-mmdb.lock");
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("cannot open MMDB operation lock: {error}"))?;
    file.try_lock_exclusive().map_err(|error| {
        if error.kind() == std::io::ErrorKind::WouldBlock {
            format!(
                "another {} MMDB operation is already running",
                kind.file_name()
            )
        } else {
            format!("cannot lock MMDB operations: {error}")
        }
    })?;
    Ok(file)
}

pub async fn download(
    config: &Config,
    kind: MmdbKind,
    source_url: &str,
) -> Result<MmdbStatus, String> {
    let url = validate_url(source_url)?;
    let host = url
        .host_str()
        .ok_or_else(|| "MMDB source URL has no hostname".to_owned())?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "MMDB source URL has no port".to_owned())?;
    let addresses = resolve_public_addresses(host, port).await?;
    let client = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|_| "cannot initialize the MMDB download client".to_owned())?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "MMDB download request failed".to_owned())?;
    if response.status().is_redirection() {
        return Err("MMDB source redirects are not allowed".to_owned());
    }
    if !response.status().is_success() {
        return Err(format!(
            "MMDB source returned HTTP {}",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .map(|length| length > MAX_DOWNLOAD_BYTES)
        .unwrap_or(false)
    {
        return Err(format!(
            "MMDB download exceeds the {MAX_DOWNLOAD_BYTES}-byte limit"
        ));
    }

    let directory = handoff::data_dir(config)?;
    let target = directory.join(kind.file_name());
    let temporary = directory.join(format!(
        ".{}.{}.tmp",
        kind.file_name(),
        Uuid::new_v4().simple()
    ));
    let mut temporary_guard = TemporaryGuard::new(temporary.clone());
    let mut file = tokio::fs::File::from_std(private_file(&temporary)?);
    let mut downloaded = 0_u64;
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "MMDB download stream failed".to_owned())?
    {
        downloaded = downloaded
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| "MMDB download size overflow".to_owned())?;
        if downloaded > MAX_DOWNLOAD_BYTES {
            return Err(format!(
                "MMDB download exceeds the {MAX_DOWNLOAD_BYTES}-byte limit"
            ));
        }
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("cannot write MMDB temporary file: {error}"))?;
    }
    file.sync_all()
        .await
        .map_err(|error| format!("cannot sync MMDB temporary file: {error}"))?;
    drop(file);
    let validation_path = temporary.clone();
    tokio::task::spawn_blocking(move || validate_database(&validation_path, kind))
        .await
        .map_err(|error| format!("MMDB validation task failed: {error}"))??;
    let warning = replace_with_backup(&temporary, &target, kind)?;
    temporary_guard.disarm();
    let status_config = config.clone();
    let mut status =
        tokio::task::spawn_blocking(move || database_status(&status_config, kind, None))
            .await
            .map_err(|error| format!("MMDB status task failed: {error}"))??;
    status.error = warning;
    Ok(status)
}

pub fn database_status(
    config: &Config,
    kind: MmdbKind,
    sources: Option<&MmdbSources>,
) -> Result<MmdbStatus, String> {
    let directory = handoff::data_dir(config)?;
    let path = directory.join(kind.file_name());
    let backup = backup_path(&path);
    let source_url = sources
        .and_then(|sources| sources.get(kind))
        .map(str::to_owned);
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => {
            return Ok(MmdbStatus {
                kind,
                path: Some(path.display().to_string()),
                is_present: false,
                size_bytes: None,
                modified_at: None,
                database_type: None,
                build_epoch: None,
                has_backup: backup.is_file(),
                source_url,
                error: Some("path exists but is not a regular file".to_owned()),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MmdbStatus {
                kind,
                path: Some(path.display().to_string()),
                is_present: false,
                size_bytes: None,
                modified_at: None,
                database_type: None,
                build_epoch: None,
                has_backup: backup.is_file(),
                source_url,
                error: None,
            })
        }
        Err(error) => return Err(format!("cannot inspect MMDB file: {error}")),
    };
    let modified_at = metadata.modified().ok().map(format_system_time);
    match Reader::open_readfile(&path) {
        Ok(reader) => {
            let mmdb = reader.metadata();
            let database_type = mmdb.database_type.clone();
            let type_error = (!database_type_matches(&database_type, kind)).then(|| {
                format!(
                    "MMDB type '{database_type}' does not match {}",
                    kind.expected_database_marker()
                )
            });
            Ok(MmdbStatus {
                kind,
                path: Some(path.display().to_string()),
                is_present: true,
                size_bytes: Some(metadata.len()),
                modified_at,
                database_type: Some(database_type),
                build_epoch: Some(mmdb.build_epoch),
                has_backup: backup.is_file(),
                source_url,
                error: type_error,
            })
        }
        Err(error) => Ok(MmdbStatus {
            kind,
            path: Some(path.display().to_string()),
            is_present: true,
            size_bytes: Some(metadata.len()),
            modified_at,
            database_type: None,
            build_epoch: None,
            has_backup: backup.is_file(),
            source_url,
            error: Some(format!("invalid MMDB: {error}")),
        }),
    }
}

pub fn is_update_due(config: &Config, kind: MmdbKind, interval: Duration) -> Result<bool, String> {
    let path = handoff::data_dir(config)?.join(kind.file_name());
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(metadata
            .modified()
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .map(|age| age >= interval)
            .unwrap_or(true)),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(format!("cannot inspect MMDB update age: {error}")),
    }
}

pub fn restore_backup(config: &Config, kind: MmdbKind) -> Result<MmdbStatus, String> {
    let directory = handoff::data_dir(config)?;
    let target = directory.join(kind.file_name());
    let backup = backup_path(&target);
    if !backup.is_file() {
        return Err("MMDB backup is unavailable".to_owned());
    }
    validate_database(&backup, kind)?;
    let restored = unique_temporary(&directory, kind, "restore");
    let mut restored_guard = TemporaryGuard::new(restored.clone());
    copy_private(&backup, &restored)?;
    let previous = target
        .is_file()
        .then(|| unique_temporary(&directory, kind, "previous"));
    let mut previous_guard = previous.clone().map(TemporaryGuard::new);
    if let Some(previous) = &previous {
        copy_private(&target, previous)?;
    }
    atomic_replace(&restored, &target)
        .map_err(|error| format!("cannot restore MMDB backup: {error}"))?;
    restored_guard.disarm();
    let mut warnings = Vec::new();
    if let Err(error) = sync_directory(&directory) {
        warnings.push(format!("restored MMDB directory sync failed: {error}"));
    }
    if let Some(previous) = previous {
        match atomic_replace(&previous, &backup) {
            Ok(()) => {
                if let Some(guard) = previous_guard.as_mut() {
                    guard.disarm();
                }
                if let Err(error) = sync_directory(&directory) {
                    warnings.push(format!("restored MMDB backup sync failed: {error}"));
                }
            }
            Err(error) => {
                warnings.push(format!(
                    "MMDB was restored, but the replaced database could not be retained: {error}"
                ));
            }
        }
    }
    let mut status = database_status(config, kind, None)?;
    if !warnings.is_empty() {
        status.error = Some(warnings.join("; "));
    }
    Ok(status)
}

fn unique_temporary(directory: &Path, kind: MmdbKind, label: &str) -> PathBuf {
    directory.join(format!(
        ".{}.{}-{}.tmp",
        kind.file_name(),
        label,
        Uuid::new_v4().simple()
    ))
}

fn validate_url(raw: &str) -> Result<Url, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > MAX_SOURCE_URL_BYTES {
        return Err(format!(
            "MMDB source URL must contain 1 to {MAX_SOURCE_URL_BYTES} bytes"
        ));
    }
    let url = Url::parse(raw).map_err(|_| "invalid MMDB source URL".to_owned())?;
    if url.scheme() != "https" {
        return Err("MMDB source URL must use HTTPS".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("MMDB source URL credentials are not allowed".to_owned());
    }
    if url.fragment().is_some() {
        return Err("MMDB source URL fragments are not allowed".to_owned());
    }
    if !matches!(url.host(), Some(url::Host::Domain(_))) {
        return Err("MMDB source URL must use a DNS hostname".to_owned());
    }
    Ok(url)
}

async fn resolve_public_addresses(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let mut addresses = Vec::new();
    let resolved = lookup_host((host, port))
        .await
        .map_err(|_| "cannot resolve the MMDB source hostname".to_owned())?;
    for address in resolved {
        if !is_public_ip(address.ip()) {
            return Err("MMDB source hostname resolves to a non-public address".to_owned());
        }
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    if addresses.is_empty() {
        return Err("MMDB source hostname has no addresses".to_owned());
    }
    Ok(addresses)
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !(ip.is_unspecified()
        || ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || octets[0] == 0
        || octets[0] >= 240
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113))
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(ipv4) = ip.to_ipv4_mapped() {
        return is_public_ipv4(ipv4);
    }
    let segments = ip.segments();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || (segments[0] & 0xe000) != 0x2000)
}

fn validate_database(path: &Path, kind: MmdbKind) -> Result<(), String> {
    let reader = Reader::open_readfile(path)
        .map_err(|error| format!("downloaded file is not a valid MMDB: {error}"))?;
    reader
        .verify()
        .map_err(|error| format!("MMDB integrity verification failed: {error}"))?;
    let database_type = &reader.metadata().database_type;
    if !database_type_matches(database_type, kind) {
        return Err(format!(
            "MMDB type '{database_type}' does not match {}",
            kind.expected_database_marker()
        ));
    }
    Ok(())
}

fn database_type_matches(database_type: &str, kind: MmdbKind) -> bool {
    database_type
        .rsplit('-')
        .next()
        .map(|value| value.eq_ignore_ascii_case(kind.expected_database_marker()))
        .unwrap_or(false)
}

fn replace_with_backup(
    temporary: &Path,
    target: &Path,
    kind: MmdbKind,
) -> Result<Option<String>, String> {
    let backup = backup_path(target);
    let directory = target
        .parent()
        .ok_or_else(|| "MMDB target has no parent directory".to_owned())?;
    if target.is_file() {
        let temporary_backup = unique_temporary(directory, kind, "backup");
        let mut backup_guard = TemporaryGuard::new(temporary_backup.clone());
        copy_private(target, &temporary_backup)?;
        atomic_replace(&temporary_backup, &backup)
            .map_err(|error| format!("cannot install the MMDB backup: {error}"))?;
        backup_guard.disarm();
        sync_directory(directory)?;
    }
    atomic_replace(temporary, target)
        .map_err(|error| format!("cannot install the MMDB file: {error}"))?;
    Ok(sync_directory(directory)
        .err()
        .map(|error| format!("MMDB was installed, but its directory sync failed: {error}")))
}

fn atomic_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(not(unix))]
    if target.exists() {
        std::fs::remove_file(target)?;
    }
    std::fs::rename(source, target)
}

fn copy_private(source: &Path, target: &Path) -> Result<(), String> {
    let mut source = std::fs::File::open(source)
        .map_err(|error| format!("cannot read MMDB file for backup: {error}"))?;
    let mut target_file = private_file(target)?;
    std::io::copy(&mut source, &mut target_file)
        .map_err(|error| format!("cannot copy MMDB backup: {error}"))?;
    target_file
        .sync_all()
        .map_err(|error| format!("cannot sync MMDB backup: {error}"))
}

fn private_file(path: &Path) -> Result<std::fs::File, String> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|error| format!("cannot create MMDB temporary file: {error}"))
}

fn backup_path(target: &Path) -> PathBuf {
    let mut value = target.as_os_str().to_os_string();
    value.push(".bak");
    PathBuf::from(value)
}

fn sync_directory(directory: &Path) -> Result<(), String> {
    let file = std::fs::File::open(directory)
        .map_err(|error| format!("cannot open MMDB directory for sync: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("cannot sync MMDB directory: {error}"))
}

fn format_system_time(time: SystemTime) -> String {
    DateTime::<Utc>::from(time).to_rfc3339()
}

struct TemporaryGuard {
    path: PathBuf,
    active: bool,
}

impl TemporaryGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for TemporaryGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_source_urls() {
        for url in [
            "http://example.com/db.mmdb",
            "https://user:pass@example.com/db.mmdb",
            "https://example.com/db.mmdb#fragment",
            "https://127.0.0.1/db.mmdb",
        ] {
            assert!(validate_url(url).is_err(), "{url}");
        }
        assert!(validate_url("https://download.example.com/db.mmdb").is_ok());
    }

    #[test]
    fn rejects_non_public_addresses() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "192.0.2.1",
            "::1",
            "fe80::1",
            "fc00::1",
            "2001:db8::1",
        ] {
            let parsed = ip.parse::<IpAddr>().unwrap();
            assert!(!is_public_ip(parsed), "{ip}");
        }
        assert!(is_public_ip("8.8.8.8".parse().unwrap()));
        assert!(is_public_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn requires_an_exact_mmdb_type_suffix() {
        assert!(database_type_matches("GeoLite2-Country", MmdbKind::Country));
        assert!(database_type_matches("GeoIP2-City", MmdbKind::City));
        assert!(database_type_matches("GeoLite2-ASN", MmdbKind::Asn));
        assert!(!database_type_matches("NotASN", MmdbKind::Asn));
        assert!(!database_type_matches(
            "GeoLite2-CountryAnything",
            MmdbKind::Country
        ));
    }

    #[test]
    fn replacement_keeps_the_previous_database_as_backup() {
        let directory = std::env::temp_dir().join(format!(
            "rustdesk-console-mmdb-replace-{}",
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&directory).unwrap();
        let target = directory.join(MmdbKind::Country.file_name());
        let temporary = directory.join("download.tmp");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&temporary, b"new").unwrap();

        replace_with_backup(&temporary, &target, MmdbKind::Country).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read(backup_path(&target)).unwrap(), b"old");
        assert!(!temporary.exists());
        std::fs::remove_dir_all(&directory).unwrap();
    }
}

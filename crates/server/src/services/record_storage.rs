use std::path::{Path, PathBuf};

use axum::body::Bytes;
use hmac::{Hmac, Mac};
use reqwest::{Client, Method};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use url::Url;

use crate::config::{
    RecordStorage, RecordStorageS3, RecordStorageWebDav, RECORD_STORAGE_LOCAL, RECORD_STORAGE_S3,
    RECORD_STORAGE_WEBDAV,
};
use crate::services::record_file;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct RecordStorageLocation {
    pub backend: String,
    pub key: String,
}

#[derive(Debug, Clone)]
pub struct RecordStorageWrite {
    pub size: i64,
}

pub async fn start_upload(
    cfg: &RecordStorage,
    resources_path: &str,
    filename: &str,
) -> Result<RecordStorageLocation, String> {
    let location = location_for(cfg, resources_path, filename)?;
    match location.backend.as_str() {
        RECORD_STORAGE_LOCAL => {
            let path = local_path(resources_path, &cfg.local_dir, filename, &location.key)?;
            ensure_parent(&path).await?;
            tokio::fs::File::create(path)
                .await
                .map_err(|_| "failed to create record file".to_string())?;
        }
        RECORD_STORAGE_S3 | RECORD_STORAGE_WEBDAV => {
            let path = staging_path(resources_path, &cfg.temp_dir, filename)?;
            ensure_parent(&path).await?;
            tokio::fs::File::create(path)
                .await
                .map_err(|_| "failed to create record staging file".to_string())?;
        }
        _ => return Err("unsupported record storage backend".to_string()),
    }
    Ok(location)
}

pub async fn write_chunk(
    cfg: &RecordStorage,
    resources_path: &str,
    location: &RecordStorageLocation,
    filename: &str,
    offset: u64,
    body: Bytes,
    complete: bool,
) -> Result<RecordStorageWrite, String> {
    let path = match location.backend.as_str() {
        RECORD_STORAGE_LOCAL => {
            local_path(resources_path, &cfg.local_dir, filename, &location.key)?
        }
        RECORD_STORAGE_S3 | RECORD_STORAGE_WEBDAV => {
            staging_path(resources_path, &cfg.temp_dir, filename)?
        }
        _ => return Err("unsupported record storage backend".to_string()),
    };
    write_at(&path, offset, &body).await?;
    let size = record_file::file_size(&path).await;

    if complete {
        match location.backend.as_str() {
            RECORD_STORAGE_LOCAL => {}
            RECORD_STORAGE_S3 => {
                validate_s3(&cfg.s3)?;
                let bytes = tokio::fs::read(&path)
                    .await
                    .map_err(|_| "failed to read record staging file".to_string())?;
                s3_request(&cfg.s3, Method::PUT, &location.key, Some(bytes)).await?;
                remove_if_exists(&path).await?;
            }
            RECORD_STORAGE_WEBDAV => {
                validate_webdav(&cfg.webdav)?;
                let bytes = tokio::fs::read(&path)
                    .await
                    .map_err(|_| "failed to read record staging file".to_string())?;
                webdav_put(&cfg.webdav, &location.key, bytes).await?;
                remove_if_exists(&path).await?;
            }
            _ => return Err("unsupported record storage backend".to_string()),
        }
    }

    Ok(RecordStorageWrite { size })
}

pub async fn read_object(
    cfg: &RecordStorage,
    resources_path: &str,
    backend: &str,
    key: &str,
    filename: &str,
) -> Result<Vec<u8>, String> {
    match normalize_backend(backend) {
        RECORD_STORAGE_LOCAL => {
            let path = local_path(resources_path, &cfg.local_dir, filename, key)?;
            tokio::fs::read(path)
                .await
                .map_err(|_| "record file not found".to_string())
        }
        RECORD_STORAGE_S3 => {
            validate_s3(&cfg.s3)?;
            s3_request(&cfg.s3, Method::GET, remote_key(key, filename), None).await
        }
        RECORD_STORAGE_WEBDAV => {
            validate_webdav(&cfg.webdav)?;
            webdav_request(&cfg.webdav, Method::GET, remote_key(key, filename), None).await
        }
        _ => Err("unsupported record storage backend".to_string()),
    }
}

pub async fn delete_object(
    cfg: &RecordStorage,
    resources_path: &str,
    backend: &str,
    key: &str,
    filename: &str,
) -> Result<(), String> {
    match normalize_backend(backend) {
        RECORD_STORAGE_LOCAL => {
            let path = local_path(resources_path, &cfg.local_dir, filename, key)?;
            remove_if_exists(&path).await
        }
        RECORD_STORAGE_S3 => {
            validate_s3(&cfg.s3)?;
            let _ = s3_request(&cfg.s3, Method::DELETE, remote_key(key, filename), None).await?;
            Ok(())
        }
        RECORD_STORAGE_WEBDAV => {
            validate_webdav(&cfg.webdav)?;
            let response =
                webdav_raw_request(&cfg.webdav, Method::DELETE, remote_key(key, filename), None)
                    .await?;
            if !response.status().is_success() && response.status().as_u16() != 404 {
                return Err(format!(
                    "WebDAV record storage returned HTTP {}",
                    response.status().as_u16()
                ));
            }
            Ok(())
        }
        _ => Err("unsupported record storage backend".to_string()),
    }
}

pub async fn cleanup_staging(
    cfg: &RecordStorage,
    resources_path: &str,
    backend: &str,
    filename: &str,
) -> Result<(), String> {
    if normalize_backend(backend) == RECORD_STORAGE_LOCAL {
        return Ok(());
    }
    let path = staging_path(resources_path, &cfg.temp_dir, filename)?;
    remove_if_exists(&path).await
}

pub fn location_for(
    cfg: &RecordStorage,
    resources_path: &str,
    filename: &str,
) -> Result<RecordStorageLocation, String> {
    let filename = record_file::sanitize_filename(filename)?;
    match cfg.normalized_type() {
        RECORD_STORAGE_S3 => {
            validate_s3(&cfg.s3)?;
            Ok(RecordStorageLocation {
                backend: RECORD_STORAGE_S3.to_string(),
                key: format!("{}{}", cfg.s3.prefix, filename),
            })
        }
        RECORD_STORAGE_WEBDAV => {
            validate_webdav(&cfg.webdav)?;
            Ok(RecordStorageLocation {
                backend: RECORD_STORAGE_WEBDAV.to_string(),
                key: format!("{}{}", cfg.webdav.prefix, filename),
            })
        }
        _ => {
            let root = record_file::record_root_for_config(resources_path, &cfg.local_dir);
            Ok(RecordStorageLocation {
                backend: RECORD_STORAGE_LOCAL.to_string(),
                key: root.join(filename).to_string_lossy().to_string(),
            })
        }
    }
}

pub fn normalize_backend(backend: &str) -> &str {
    match backend.trim() {
        RECORD_STORAGE_S3 => RECORD_STORAGE_S3,
        RECORD_STORAGE_WEBDAV => RECORD_STORAGE_WEBDAV,
        _ => RECORD_STORAGE_LOCAL,
    }
}

fn remote_key<'a>(key: &'a str, filename: &'a str) -> &'a str {
    if key.trim().is_empty() {
        filename
    } else {
        key
    }
}

fn local_path(
    resources_path: &str,
    local_dir: &str,
    filename: &str,
    key: &str,
) -> Result<PathBuf, String> {
    let filename = record_file::sanitize_filename(filename)?;
    if key.trim().is_empty() {
        return Ok(record_file::record_root_for_config(resources_path, local_dir).join(filename));
    }
    Ok(PathBuf::from(key))
}

fn staging_path(resources_path: &str, temp_dir: &str, filename: &str) -> Result<PathBuf, String> {
    Ok(record_file::record_temp_root(resources_path, temp_dir)
        .join(record_file::sanitize_filename(filename)?))
}

async fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| "failed to create record directory".to_string())?;
    }
    Ok(())
}

async fn write_at(path: &Path, offset: u64, body: &[u8]) -> Result<(), String> {
    ensure_parent(path).await?;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .await
        .map_err(|_| "failed to open record file".to_string())?;
    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|_| "failed to seek record file".to_string())?;
    file.write_all(body)
        .await
        .map_err(|_| "failed to write record file".to_string())?;
    Ok(())
}

async fn remove_if_exists(path: &Path) -> Result<(), String> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("failed to remove record file".to_string()),
    }
}

fn validate_s3(cfg: &RecordStorageS3) -> Result<(), String> {
    if cfg.endpoint.trim().is_empty()
        || cfg.bucket.trim().is_empty()
        || cfg.access_key_id.trim().is_empty()
        || cfg.secret_access_key.trim().is_empty()
    {
        return Err("S3 record storage is not fully configured".to_string());
    }
    Ok(())
}

fn validate_webdav(cfg: &RecordStorageWebDav) -> Result<(), String> {
    if cfg.url.trim().is_empty() {
        return Err("WebDAV record storage is not fully configured".to_string());
    }
    Ok(())
}

async fn s3_request(
    cfg: &RecordStorageS3,
    method: Method,
    key: &str,
    body: Option<Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let body = body.unwrap_or_default();
    let url = s3_url(cfg, key)?;
    let payload_hash = hex_sha256(&body);
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let region = if cfg.region.trim().is_empty() || cfg.region.trim() == "auto" {
        "us-east-1"
    } else {
        cfg.region.trim()
    };
    let host = canonical_host(&url)?;
    let canonical_uri = url.path();
    let canonical_headers =
        format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
    let signed_headers = "host;x-amz-content-sha256;x-amz-date";
    let canonical_request = format!(
        "{}\n{}\n\n{}\n{}\n{}",
        method.as_str(),
        canonical_uri,
        canonical_headers,
        signed_headers,
        payload_hash
    );
    let scope = format!("{date}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex_encode(&Sha256::digest(canonical_request.as_bytes()))
    );
    let signing_key = s3_signing_key(&cfg.secret_access_key, &date, region)?;
    let signature = hex_hmac(&signing_key, string_to_sign.as_bytes())?;
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        cfg.access_key_id, scope, signed_headers, signature
    );

    let client = Client::new();
    let response = client
        .request(method, url)
        .header("authorization", authorization)
        .header("x-amz-content-sha256", payload_hash)
        .header("x-amz-date", amz_date)
        .body(body)
        .send()
        .await
        .map_err(|_| "failed to access S3 record storage".to_string())?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "failed to read S3 record storage response".to_string())?;
    if !status.is_success() {
        return Err(format!(
            "S3 record storage returned HTTP {}",
            status.as_u16()
        ));
    }
    Ok(bytes.to_vec())
}

fn s3_url(cfg: &RecordStorageS3, key: &str) -> Result<Url, String> {
    let mut url = Url::parse(&cfg.endpoint).map_err(|_| "invalid S3 endpoint".to_string())?;
    url.set_path("");
    if cfg.force_path_style {
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| "invalid S3 endpoint".to_string())?;
            segments.push(&cfg.bucket);
            for segment in key.split('/').filter(|segment| !segment.is_empty()) {
                segments.push(segment);
            }
        }
        return Ok(url);
    }
    let host = url
        .host_str()
        .ok_or_else(|| "invalid S3 endpoint".to_string())?;
    url.set_host(Some(&format!("{}.{}", cfg.bucket, host)))
        .map_err(|_| "invalid S3 endpoint".to_string())?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| "invalid S3 endpoint".to_string())?;
        for segment in key.split('/').filter(|segment| !segment.is_empty()) {
            segments.push(segment);
        }
    }
    Ok(url)
}

fn canonical_host(url: &Url) -> Result<String, String> {
    let host = url
        .host_str()
        .ok_or_else(|| "invalid S3 endpoint".to_string())?;
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

fn s3_signing_key(secret: &str, date: &str, region: &str) -> Result<Vec<u8>, String> {
    let k_date = hmac_bytes(format!("AWS4{secret}").as_bytes(), date.as_bytes())?;
    let k_region = hmac_bytes(&k_date, region.as_bytes())?;
    let k_service = hmac_bytes(&k_region, b"s3")?;
    hmac_bytes(&k_service, b"aws4_request")
}

fn hmac_bytes(key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|_| "failed to sign S3 record storage request".to_string())?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn hex_hmac(key: &[u8], data: &[u8]) -> Result<String, String> {
    Ok(hex_encode(&hmac_bytes(key, data)?))
}

async fn webdav_put(
    cfg: &RecordStorageWebDav,
    key: &str,
    body: Vec<u8>,
) -> Result<Vec<u8>, String> {
    ensure_webdav_collections(cfg, key).await?;
    webdav_request(cfg, Method::PUT, key, Some(body)).await
}

async fn ensure_webdav_collections(cfg: &RecordStorageWebDav, key: &str) -> Result<(), String> {
    let Some((prefix, _filename)) = key.rsplit_once('/') else {
        return Ok(());
    };
    let mut current = String::new();
    let method = Method::from_bytes(b"MKCOL").map_err(|_| "invalid WebDAV method".to_string())?;
    for segment in prefix.split('/').filter(|segment| !segment.is_empty()) {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(segment);
        let response = webdav_raw_request(cfg, method.clone(), &current, None).await?;
        if !response.status().is_success()
            && response.status().as_u16() != 405
            && response.status().as_u16() != 409
        {
            return Err(format!(
                "WebDAV record storage returned HTTP {}",
                response.status().as_u16()
            ));
        }
    }
    Ok(())
}

async fn webdav_request(
    cfg: &RecordStorageWebDav,
    method: Method,
    key: &str,
    body: Option<Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let response = webdav_raw_request(cfg, method, key, body).await?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "failed to read WebDAV record storage response".to_string())?;
    if !status.is_success() && status.as_u16() != 404 {
        return Err(format!(
            "WebDAV record storage returned HTTP {}",
            status.as_u16()
        ));
    }
    if status.as_u16() == 404 {
        return Err("record file not found".to_string());
    }
    Ok(bytes.to_vec())
}

async fn webdav_raw_request(
    cfg: &RecordStorageWebDav,
    method: Method,
    key: &str,
    body: Option<Vec<u8>>,
) -> Result<reqwest::Response, String> {
    let client = Client::new();
    let mut request = client.request(method, webdav_url(cfg, key)?);
    if !cfg.username.is_empty() || !cfg.password.is_empty() {
        request = request.basic_auth(&cfg.username, Some(&cfg.password));
    }
    if let Some(body) = body {
        request = request.body(body);
    }
    request
        .send()
        .await
        .map_err(|_| "failed to access WebDAV record storage".to_string())
}

fn webdav_url(cfg: &RecordStorageWebDav, key: &str) -> Result<Url, String> {
    let mut url = Url::parse(&cfg.url).map_err(|_| "invalid WebDAV URL".to_string())?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| "invalid WebDAV URL".to_string())?;
        for segment in key.split('/').filter(|segment| !segment.is_empty()) {
            segments.push(segment);
        }
    }
    Ok(url)
}

fn hex_sha256(data: &[u8]) -> String {
    hex_encode(&Sha256::digest(data))
}

fn hex_encode(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_for_remote_uses_normalized_prefix() {
        let mut cfg = RecordStorage {
            r#type: RECORD_STORAGE_S3.to_string(),
            ..Default::default()
        };
        cfg.s3.endpoint = "https://s3.example.com".to_string();
        cfg.s3.bucket = "bucket".to_string();
        cfg.s3.prefix = "record/".to_string();
        cfg.s3.access_key_id = "ak".to_string();
        cfg.s3.secret_access_key = "sk".to_string();
        let location = location_for(&cfg, "resources", "incoming_123.webm").unwrap();
        assert_eq!(location.backend, RECORD_STORAGE_S3);
        assert_eq!(location.key, "record/incoming_123.webm");
    }

    #[test]
    fn local_location_stores_the_resolved_file_path() {
        let cfg = RecordStorage {
            r#type: RECORD_STORAGE_LOCAL.to_string(),
            local_dir: "/var/record".to_string(),
            ..Default::default()
        };
        let location = location_for(&cfg, "resources", "incoming_123.webm").unwrap();
        assert_eq!(location.backend, RECORD_STORAGE_LOCAL);
        assert_eq!(location.key, "/var/record/incoming_123.webm");
    }
}

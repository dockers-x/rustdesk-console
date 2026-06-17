use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context as _};
use zip::ZipArchive;

const DEFAULT_WEBCLIENT_ZIP: &str = "./data/web.zip";
const MAX_ZIP_BYTES: u64 = 128 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug)]
pub struct ExternalWebClient {
    root: PathBuf,
}

impl ExternalWebClient {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn try_load_from_default_zip() -> Option<Self> {
        match Self::load_from_zip_path(DEFAULT_WEBCLIENT_ZIP).await {
            Ok(Some(client)) => {
                tracing::info!(
                    zip = DEFAULT_WEBCLIENT_ZIP,
                    root = %client.root.display(),
                    "external WebClient loaded"
                );
                Some(client)
            }
            Ok(None) => None,
            Err(err) => {
                tracing::warn!(
                    zip = DEFAULT_WEBCLIENT_ZIP,
                    error = %err,
                    "external WebClient unavailable; falling back to embedded resources"
                );
                None
            }
        }
    }

    pub async fn load_from_zip_path(path: impl AsRef<Path>) -> anyhow::Result<Option<Self>> {
        let zip_path = path.as_ref().to_path_buf();
        let metadata = match tokio::fs::metadata(&zip_path).await {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(err).with_context(|| format!("read {}", zip_path.display()));
            }
        };

        if !metadata.is_file() {
            bail!("{} is not a file", zip_path.display());
        }
        if metadata.len() > MAX_ZIP_BYTES {
            bail!(
                "{} exceeds max zip size of {} bytes",
                zip_path.display(),
                MAX_ZIP_BYTES
            );
        }

        let root = default_temp_root();
        let extract_root = root.clone();
        tokio::task::spawn_blocking(move || extract_zip(&zip_path, &extract_root))
            .await
            .context("external WebClient extraction task failed")??;

        Ok(Some(Self { root }))
    }
}

impl Drop for ExternalWebClient {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.root) {
            if err.kind() != io::ErrorKind::NotFound {
                tracing::debug!(
                    root = %self.root.display(),
                    error = %err,
                    "failed to remove external WebClient temp directory"
                );
            }
        }
    }
}

fn default_temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("rustdesk-console-webclient-{}", std::process::id()))
}

fn extract_zip(zip_path: &Path, root: &Path) -> anyhow::Result<()> {
    if root.exists() {
        fs::remove_dir_all(root)
            .with_context(|| format!("remove previous temp dir {}", root.display()))?;
    }
    fs::create_dir_all(root).with_context(|| format!("create temp dir {}", root.display()))?;

    let result = extract_zip_inner(zip_path, root);
    if result.is_err() {
        let _ = fs::remove_dir_all(root);
    }
    result
}

fn extract_zip_inner(zip_path: &Path, root: &Path) -> anyhow::Result<()> {
    let file =
        fs::File::open(zip_path).with_context(|| format!("open zip {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file).context("open zip archive")?;
    let mut total_size = 0_u64;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("read zip entry #{index}"))?;
        let raw_name = entry.name().to_string();
        let Some(relative_path) = safe_zip_entry_path(&raw_name)? else {
            continue;
        };

        if entry.is_symlink() {
            tracing::warn!(
                entry = raw_name,
                "skipping symlink in external WebClient zip"
            );
            continue;
        }

        if entry.is_dir() {
            fs::create_dir_all(root.join(relative_path))
                .with_context(|| format!("create directory from zip entry {raw_name}"))?;
            continue;
        }

        if !entry.is_file() {
            continue;
        }

        let file_size = entry.size();
        if file_size > MAX_FILE_BYTES {
            bail!("zip entry {raw_name} exceeds max file size of {MAX_FILE_BYTES} bytes");
        }
        total_size = total_size
            .checked_add(file_size)
            .context("external WebClient zip total size overflow")?;
        if total_size > MAX_TOTAL_BYTES {
            bail!("external WebClient zip exceeds max total size of {MAX_TOTAL_BYTES} bytes");
        }

        let output_path = root.join(&relative_path);
        if !output_path.starts_with(root) {
            bail!("zip entry {raw_name} escapes extraction root");
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create parent directory {}", parent.display()))?;
        }

        let mut output = fs::File::create(&output_path)
            .with_context(|| format!("create {}", output_path.display()))?;
        copy_with_limit(&mut entry, &mut output, MAX_FILE_BYTES)
            .with_context(|| format!("extract zip entry {raw_name}"))?;
    }

    let index = root.join("index.html");
    if !index.is_file() {
        bail!("external WebClient zip must contain root index.html");
    }

    Ok(())
}

fn safe_zip_entry_path(raw_name: &str) -> anyhow::Result<Option<PathBuf>> {
    if raw_name.trim().is_empty() {
        return Ok(None);
    }
    if raw_name.contains('\0') {
        bail!("zip entry contains NUL byte");
    }
    if raw_name.contains('\\') {
        bail!("zip entry {raw_name} uses backslash path separators");
    }
    if looks_like_windows_drive_path(raw_name) {
        bail!("zip entry {raw_name} uses a Windows drive path");
    }

    let path = Path::new(raw_name);
    if path.is_absolute() {
        bail!("zip entry {raw_name} uses an absolute path");
    }

    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir => bail!("zip entry {raw_name} contains parent traversal"),
            Component::RootDir | Component::Prefix(_) => {
                bail!("zip entry {raw_name} is not a relative path")
            }
        }
    }

    if clean.as_os_str().is_empty() {
        Ok(None)
    } else {
        Ok(Some(clean))
    }
}

fn looks_like_windows_drive_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

fn copy_with_limit<R, W>(reader: &mut R, writer: &mut W, limit: u64) -> anyhow::Result<u64>
where
    R: io::Read,
    W: io::Write,
{
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(copied);
        }
        copied = copied
            .checked_add(read as u64)
            .context("copied file size overflow")?;
        if copied > limit {
            bail!("extracted file exceeds max file size of {limit} bytes");
        }
        writer.write_all(&buffer[..read])?;
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use zip::write::SimpleFileOptions;

    use super::*;

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn extracts_valid_zip_to_target_root() {
        let dir = test_dir("valid");
        let zip_path = dir.join("web.zip");
        write_zip(
            &zip_path,
            &[
                ("index.html", b"<html>ok</html>".as_slice(), 0o100644),
                ("assets/app.js", b"console.log('ok')".as_slice(), 0o100644),
            ],
        );
        let root = dir.join("out");

        extract_zip(&zip_path, &root).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("index.html")).unwrap(),
            "<html>ok</html>"
        );
        assert_eq!(
            fs::read_to_string(root.join("assets/app.js")).unwrap(),
            "console.log('ok')"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_parent_traversal_entries() {
        let dir = test_dir("traversal");
        let zip_path = dir.join("web.zip");
        write_zip(
            &zip_path,
            &[
                ("index.html", b"ok".as_slice(), 0o100644),
                ("../escape.txt", b"no".as_slice(), 0o100644),
            ],
        );

        let err = extract_zip(&zip_path, &dir.join("out")).unwrap_err();

        assert!(err.to_string().contains("parent traversal"));
        assert!(!dir.join("escape.txt").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_windows_drive_entries() {
        let dir = test_dir("windows-drive");
        let zip_path = dir.join("web.zip");
        write_zip(
            &zip_path,
            &[
                ("index.html", b"ok".as_slice(), 0o100644),
                ("C:/escape.txt", b"no".as_slice(), 0o100644),
            ],
        );

        let err = extract_zip(&zip_path, &dir.join("out")).unwrap_err();

        assert!(err.to_string().contains("Windows drive"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_backslash_entries() {
        let dir = test_dir("backslash");
        let zip_path = dir.join("web.zip");
        write_zip(
            &zip_path,
            &[
                ("index.html", b"ok".as_slice(), 0o100644),
                ("assets\\app.js", b"no".as_slice(), 0o100644),
            ],
        );

        let err = extract_zip(&zip_path, &dir.join("out")).unwrap_err();

        assert!(err.to_string().contains("backslash"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn requires_root_index_html() {
        let dir = test_dir("missing-index");
        let zip_path = dir.join("web.zip");
        write_zip(&zip_path, &[("assets/app.js", b"ok".as_slice(), 0o100644)]);

        let err = extract_zip(&zip_path, &dir.join("out")).unwrap_err();

        assert!(err.to_string().contains("index.html"));
        let _ = fs::remove_dir_all(dir);
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8], u32)]) {
        let file = fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, bytes, mode) in entries {
            let options = SimpleFileOptions::default().unix_permissions(*mode);
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    fn test_dir(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "rustdesk-console-external-webclient-test-{name}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}

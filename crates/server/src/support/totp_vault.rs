use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::Path,
};

use anyhow::{anyhow, ensure, Context};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN},
    rand::{SecureRandom as _, SystemRandom},
};

use crate::config::Config;

const FORMAT: &str = "v1:";

pub(crate) struct TotpVault(LessSafeKey);

impl TotpVault {
    pub(crate) fn load(config: &Config, create: bool) -> anyhow::Result<Self> {
        let path = config.security.totp_key_path();
        let encoded = match fs::read_to_string(path) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
                provision_key(path)?;
                fs::read_to_string(path).context("Could not read the TOTP encryption key")?
            }
            Err(error) => return Err(error).context("Could not read the TOTP encryption key"),
        };
        let key = STANDARD
            .decode(encoded.trim())
            .map_err(|_| anyhow!("Invalid TOTP encryption key"))?;
        let key = UnboundKey::new(&AES_256_GCM, &key)
            .map_err(|_| anyhow!("Invalid TOTP encryption key"))?;
        Ok(Self(LessSafeKey::new(key)))
    }

    pub(crate) fn is_sealed(value: &str) -> bool {
        value.starts_with(FORMAT)
    }

    pub(crate) fn seal(&self, user_id: i32, secret: &str) -> anyhow::Result<String> {
        self.seal_bound(&user_id.to_be_bytes(), secret)
    }

    pub(crate) fn seal_bound(&self, binding: &[u8], value: &str) -> anyhow::Result<String> {
        let mut nonce = [0; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| anyhow!("Could not generate a TOTP encryption nonce"))?;
        let mut data = value.as_bytes().to_vec();
        self.0
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(binding),
                &mut data,
            )
            .map_err(|_| anyhow!("Could not encrypt the TOTP secret"))?;
        let mut envelope = nonce.to_vec();
        envelope.extend_from_slice(&data);
        Ok(format!("{FORMAT}{}", STANDARD.encode(envelope)))
    }

    pub(crate) fn open(&self, user_id: i32, value: &str) -> anyhow::Result<String> {
        self.open_bound(&user_id.to_be_bytes(), value)
    }

    pub(crate) fn open_bound(&self, binding: &[u8], value: &str) -> anyhow::Result<String> {
        let payload = value
            .strip_prefix(FORMAT)
            .context("Invalid encrypted TOTP secret")?;
        let mut data = STANDARD
            .decode(payload)
            .map_err(|_| anyhow!("Invalid encrypted TOTP secret"))?;
        ensure!(data.len() > NONCE_LEN, "Invalid encrypted TOTP secret");
        let (nonce, data) = data.split_at_mut(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce)
            .map_err(|_| anyhow!("Invalid TOTP encryption nonce"))?;
        let secret = self
            .0
            .open_in_place(nonce, Aad::from(binding), data)
            .map_err(|_| anyhow!("Could not decrypt the TOTP secret with the configured key"))?;
        Ok(std::str::from_utf8(secret)
            .context("Invalid TOTP secret")?
            .to_owned())
    }
}

fn provision_key(path: &Path) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".totp-key-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut key = [0; 32];
    SystemRandom::new()
        .fill(&mut key)
        .map_err(|_| anyhow!("Could not generate a TOTP encryption key"))?;
    let result = (|| -> anyhow::Result<()> {
        let mut file = options.open(&temporary)?;
        writeln!(file, "{}", STANDARD.encode(key))?;
        file.sync_all()?;
        drop(file);
        match fs::hard_link(&temporary, path) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result.context("Could not provision the TOTP encryption key")
}

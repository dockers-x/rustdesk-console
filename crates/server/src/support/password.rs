//! Password hashing, mirroring `utils/password.go`.
//!
//! New hashes use bcrypt (default cost). Verification falls back to the legacy
//! MD5 scheme `md5(input + "rustdesk-api")`; when a legacy hash matches, a fresh
//! bcrypt hash is returned so the caller can transparently upgrade it.

use md5::{Digest, Md5};

const LEGACY_SALT: &str = "rustdesk-api";

pub fn encrypt_password(password: &str) -> Result<String, bcrypt::BcryptError> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST)
}

/// Returns `(ok, new_hash)`. `new_hash` is `Some` only when a legacy MD5 hash
/// matched and was upgraded to bcrypt.
pub fn verify_password(hash: &str, input: &str) -> (bool, Option<String>) {
    match bcrypt::verify(input, hash) {
        Ok(true) => (true, None),
        Ok(false) => (false, None),
        Err(_) => {
            // Not a valid bcrypt hash — try the legacy MD5 fallback.
            if hash == md5_hex(&format!("{input}{LEGACY_SALT}")) {
                match encrypt_password(input) {
                    Ok(new_hash) => (true, Some(new_hash)),
                    Err(_) => (true, None),
                }
            } else {
                (false, None)
            }
        }
    }
}

pub fn md5_hex(s: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(s.as_bytes());
    let out = hasher.finalize();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcrypt_round_trip() {
        let hash = encrypt_password("secret123").unwrap();
        assert!(hash.starts_with("$2"));
        let (ok, new) = verify_password(&hash, "secret123");
        assert!(ok && new.is_none());
        let (bad, _) = verify_password(&hash, "wrong");
        assert!(!bad);
    }

    #[test]
    fn legacy_md5_upgrades_to_bcrypt() {
        // Go's legacy scheme: md5(input + "rustdesk-api").
        let legacy = md5_hex("hunter2rustdesk-api");
        let (ok, upgraded) = verify_password(&legacy, "hunter2");
        assert!(ok);
        let upgraded = upgraded.expect("legacy match should yield a fresh bcrypt hash");
        assert!(upgraded.starts_with("$2"));
        // and the upgraded hash verifies the same password
        assert!(verify_password(&upgraded, "hunter2").0);
    }

    #[test]
    fn md5_matches_known_vector() {
        // md5("") = d41d8cd98f00b204e9800998ecf8427e
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
    }
}

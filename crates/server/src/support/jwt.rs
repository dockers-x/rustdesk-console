//! JWT generation/validation, mirroring `lib/jwt/jwt.go` (HS256, `user_id` claim).

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    user_id: u32,
    exp: usize,
}

#[derive(Clone)]
pub struct Jwt {
    key: Vec<u8>,
    expire: std::time::Duration,
}

impl Jwt {
    pub fn try_new(key: &str, expire: std::time::Duration) -> anyhow::Result<Self> {
        if !key.is_empty() {
            let duration = chrono::TimeDelta::from_std(expire)
                .map_err(|error| anyhow::anyhow!("invalid JWT expiration duration: {error}"))?;
            if chrono::Utc::now().checked_add_signed(duration).is_none() {
                anyhow::bail!("JWT expiration duration exceeds the supported timestamp range");
            }
        }
        Ok(Self::new(key, expire))
    }

    pub fn new(key: &str, expire: std::time::Duration) -> Self {
        Self {
            key: key.as_bytes().to_vec(),
            expire,
        }
    }

    pub fn has_key(&self) -> bool {
        !self.key.is_empty()
    }

    /// Returns "" when no key is configured (matching the Go behaviour).
    pub fn generate_token(&self, user_id: u32) -> String {
        if self.key.is_empty() {
            return String::new();
        }
        let exp = (chrono::Utc::now() + self.expire).timestamp() as usize;
        let claims = Claims { user_id, exp };
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(&self.key),
        )
        .unwrap_or_default()
    }

    pub fn parse_token(&self, token: &str) -> Result<u32, jsonwebtoken::errors::Error> {
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(&self.key),
            &Validation::new(Algorithm::HS256),
        )?;
        Ok(data.claims.user_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn checked_constructor_rejects_unrepresentable_expiration() {
        assert!(Jwt::try_new("a-secret-key", Duration::from_secs(u64::MAX)).is_err());
        assert!(Jwt::try_new("a-secret-key", Duration::from_secs(10_000_000_000_000)).is_err());
    }

    #[test]
    fn round_trip() {
        let jwt = Jwt::try_new("a-secret-key", Duration::from_secs(1000)).unwrap();
        let token = jwt.generate_token(999);
        assert!(!token.is_empty());
        assert_eq!(jwt.parse_token(&token).unwrap(), 999);
    }

    #[test]
    fn empty_key_yields_empty_token() {
        let jwt = Jwt::try_new("", Duration::from_secs(u64::MAX)).unwrap();
        assert!(!jwt.has_key());
        assert_eq!(jwt.generate_token(1), "");
    }
}

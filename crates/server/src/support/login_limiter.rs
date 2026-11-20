//! Login rate-limiter + captcha, mirroring `utils/login_limiter.go`.
//!
//! Failed attempts per client IP are tracked in a sliding window. Once the
//! captcha threshold is reached a captcha is required; once the ban threshold
//! is reached the IP is banned. Captcha images are generated with the `captcha`
//! crate and returned as `data:image/png;base64,...` (matching base64Captcha).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;

const MAX_CAPTCHAS: usize = 4096;
const MAX_PASSWORD_KEYS: usize = 8192;
const PASSWORD_SOURCE_LIMIT: u16 = 60;
const PASSWORD_ACCOUNT_LIMIT: u16 = 10;
const PASSWORD_WINDOW: Duration = Duration::from_secs(60);

#[derive(Hash, PartialEq, Eq)]
enum PasswordKey {
    Source(String),
    Account(String),
}

#[derive(Clone, Copy)]
pub struct SecurityPolicy {
    /// `< 0` disabled, `0` always, `> 0` enabled at this many attempts.
    pub captcha_threshold: i32,
    /// `0` disabled, `> 0` ban at this many attempts.
    pub ban_threshold: i32,
    pub attempts_window: Duration,
    pub ban_duration: Duration,
}

struct CaptchaMetaInternal {
    answer: String,
    expires_at: Instant,
}

/// Returned to controllers: the id + ready-to-display base64 PNG.
pub struct Captcha {
    pub id: String,
    pub b64: String,
}

struct Inner {
    attempts: HashMap<String, Vec<Instant>>,
    password_counts: HashMap<PasswordKey, u16>,
    password_window_start: Instant,
    failure_pruned_at: Instant,
    captchas: HashMap<String, CaptchaMetaInternal>,
    banned: HashMap<String, Instant>,
}

pub struct LoginLimiter {
    policy: SecurityPolicy,
    captcha_expiration: Duration,
    inner: Mutex<Inner>,
}

impl LoginLimiter {
    pub fn new(policy: SecurityPolicy) -> Self {
        Self {
            policy,
            captcha_expiration: Duration::from_secs(5 * 60),
            inner: Mutex::new(Inner {
                attempts: HashMap::new(),
                password_counts: HashMap::new(),
                password_window_start: Instant::now(),
                failure_pruned_at: Instant::now(),
                captchas: HashMap::new(),
                banned: HashMap::new(),
            }),
        }
    }

    /// Reserve both budgets before any expensive password verification starts.
    pub fn reserve_password_attempt(&self, ip: &str, username: &str) -> bool {
        let mut inner = self.inner.lock().expect("login limiter mutex poisoned");
        if inner.password_window_start.elapsed() >= PASSWORD_WINDOW {
            inner.password_counts.clear();
            inner.password_window_start = Instant::now();
        }
        let source = PasswordKey::Source(ip.to_owned());
        let account = PasswordKey::Account(crate::services::user::format_username(username));
        let source_count = inner.password_counts.get(&source).copied().unwrap_or(0);
        let account_count = inner.password_counts.get(&account).copied().unwrap_or(0);
        let new_keys = usize::from(source_count == 0) + usize::from(account_count == 0);
        if source_count >= PASSWORD_SOURCE_LIMIT
            || account_count >= PASSWORD_ACCOUNT_LIMIT
            || inner.password_counts.len() + new_keys > MAX_PASSWORD_KEYS
        {
            return false;
        }
        inner.password_counts.insert(source, source_count + 1);
        inner.password_counts.insert(account, account_count + 1);
        true
    }

    fn is_disabled(&self) -> bool {
        self.policy.captcha_threshold < 0 && self.policy.ban_threshold == 0
    }

    pub fn record_failed_attempt(&self, ip: &str) {
        if self.is_disabled() {
            return;
        }
        let mut g = self.inner.lock().unwrap();
        if Self::is_banned(&mut g, ip) {
            return;
        }
        let now = Instant::now();
        let cutoff = now.checked_sub(self.policy.attempts_window).unwrap_or(now);
        if g.failure_pruned_at.elapsed() >= PASSWORD_WINDOW {
            g.attempts.retain(|_, attempts| {
                attempts.retain(|attempt| *attempt > cutoff);
                !attempts.is_empty()
            });
            g.banned.retain(|_, expires| *expires > now);
            g.failure_pruned_at = now;
        }
        if !g.attempts.contains_key(ip) && g.attempts.len() + g.banned.len() >= MAX_PASSWORD_KEYS {
            return;
        }
        let mut valid = Self::prune_attempts(&mut g, ip, cutoff);
        valid.push(now);
        g.attempts.insert(ip.to_string(), valid.clone());
        if self.policy.ban_threshold > 0 && valid.len() as i32 >= self.policy.ban_threshold {
            Self::ban_ip(&mut g, ip, self.policy.ban_duration);
        }
    }

    pub fn remove_attempts(&self, ip: &str) {
        let mut g = self.inner.lock().unwrap();
        g.attempts.remove(ip);
    }

    /// Returns `(banned, captcha_required)`.
    pub fn check_security_status(&self, ip: &str) -> (bool, bool) {
        if self.is_disabled() {
            return (false, false);
        }
        let mut g = self.inner.lock().unwrap();
        if Self::is_banned(&mut g, ip) {
            return (true, false);
        }
        let now = Instant::now();
        let cutoff = now.checked_sub(self.policy.attempts_window).unwrap_or(now);
        Self::prune_attempts(&mut g, ip, cutoff);
        let count = g.attempts.get(ip).map(|v| v.len()).unwrap_or(0) as i32;
        let captcha_required =
            self.policy.captcha_threshold >= 0 && count >= self.policy.captcha_threshold;
        (false, captcha_required)
    }

    /// Generate a captcha and return its id + rendered base64 PNG.
    pub fn require_captcha(&self) -> Result<Captcha, String> {
        self.check_captcha_capacity()?;
        let mut c = captcha::Captcha::new();
        c.add_chars(4)
            .apply_filter(captcha::filters::Noise::new(0.1))
            .view(150, 50);
        let answer = c.chars_as_string().to_lowercase();
        let png = c
            .as_png()
            .ok_or_else(|| "captcha render failed".to_string())?;
        let b64 = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        let id = uuid::Uuid::new_v4().to_string();
        let mut g = self.inner.lock().unwrap();
        if g.captchas.len() >= MAX_CAPTCHAS {
            return Err("captcha capacity reached".to_string());
        }
        g.captchas.insert(
            id.clone(),
            CaptchaMetaInternal {
                answer,
                expires_at: Instant::now() + self.captcha_expiration,
            },
        );
        Ok(Captcha { id, b64 })
    }

    fn check_captcha_capacity(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();
        inner.captchas.retain(|_, meta| meta.expires_at > now);
        if inner.captchas.len() >= MAX_CAPTCHAS {
            return Err("captcha capacity reached".to_string());
        }
        Ok(())
    }

    pub fn verify_captcha(&self, id: &str, answer: &str) -> bool {
        let mut g = self.inner.lock().unwrap();
        let Some(meta) = g.captchas.get(id) else {
            return false;
        };
        if Instant::now() > meta.expires_at {
            g.captchas.remove(id);
            return false;
        }
        if answer.to_lowercase() == meta.answer {
            g.captchas.remove(id);
            return true;
        }
        false
    }

    // --- internal helpers ---

    fn is_banned(g: &mut Inner, ip: &str) -> bool {
        if let Some(&expires) = g.banned.get(ip) {
            if Instant::now() > expires {
                g.banned.remove(ip);
                return false;
            }
            return true;
        }
        false
    }

    fn ban_ip(g: &mut Inner, ip: &str, dur: Duration) {
        g.banned.insert(ip.to_string(), Instant::now() + dur);
        g.attempts.remove(ip);
    }

    fn prune_attempts(g: &mut Inner, ip: &str, cutoff: Instant) -> Vec<Instant> {
        let valid: Vec<Instant> = g
            .attempts
            .get(ip)
            .map(|v| v.iter().copied().filter(|t| *t > cutoff).collect())
            .unwrap_or_default();
        if valid.is_empty() {
            g.attempts.remove(ip);
        } else {
            g.attempts.insert(ip.to_string(), valid.clone());
        }
        valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_budget_is_shared_by_normalized_account_across_sources() {
        let limiter = LoginLimiter::new(SecurityPolicy {
            captcha_threshold: -1,
            ban_threshold: 0,
            attempts_window: PASSWORD_WINDOW,
            ban_duration: PASSWORD_WINDOW,
        });
        for source in 0..PASSWORD_ACCOUNT_LIMIT {
            assert!(limiter.reserve_password_attempt(&format!("test-{source}"), "Admin"));
        }
        assert!(!limiter.reserve_password_attempt("another-source", " a d m i n "));
        assert!(limiter.reserve_password_attempt("another-source", "different-account"));
        limiter.inner.lock().unwrap().password_window_start = Instant::now() - PASSWORD_WINDOW;
        assert!(limiter.reserve_password_attempt("another-source", "admin"));
    }

    #[test]
    fn captcha_capacity_rejects_requests_until_entries_expire() {
        let limiter = LoginLimiter::new(SecurityPolicy {
            captcha_threshold: 0,
            ban_threshold: 0,
            attempts_window: Duration::from_secs(60),
            ban_duration: Duration::from_secs(60),
        });
        {
            let mut inner = limiter.inner.lock().unwrap();
            for index in 0..MAX_CAPTCHAS {
                inner.captchas.insert(
                    index.to_string(),
                    CaptchaMetaInternal {
                        answer: "test".to_string(),
                        expires_at: Instant::now() + Duration::from_secs(60),
                    },
                );
            }
        }
        assert!(limiter.require_captcha().is_err());
        {
            let mut inner = limiter.inner.lock().unwrap();
            for meta in inner.captchas.values_mut() {
                meta.expires_at = Instant::now() - Duration::from_secs(1);
            }
        }
        assert!(limiter.require_captcha().is_ok());
    }

    #[test]
    fn disabled_captcha_stays_disabled_when_bans_are_enabled() {
        let limiter = LoginLimiter::new(SecurityPolicy {
            captcha_threshold: -1,
            ban_threshold: 2,
            attempts_window: Duration::from_secs(60),
            ban_duration: Duration::from_secs(60),
        });
        assert_eq!(limiter.check_security_status("test-ip"), (false, false));
        limiter.record_failed_attempt("test-ip");
        assert_eq!(limiter.check_security_status("test-ip"), (false, false));
        limiter.record_failed_attempt("test-ip");
        assert_eq!(limiter.check_security_status("test-ip"), (true, false));
    }
}

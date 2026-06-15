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
                captchas: HashMap::new(),
                banned: HashMap::new(),
            }),
        }
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
        let cutoff = now
            .checked_sub(self.policy.attempts_window)
            .unwrap_or(now);
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
        let cutoff = now
            .checked_sub(self.policy.attempts_window)
            .unwrap_or(now);
        Self::prune_attempts(&mut g, ip, cutoff);
        let count = g.attempts.get(ip).map(|v| v.len()).unwrap_or(0) as i32;
        let captcha_required = count >= self.policy.captcha_threshold;
        (false, captcha_required)
    }

    /// Generate a captcha and return its id + rendered base64 PNG.
    pub fn require_captcha(&self) -> Result<Captcha, String> {
        let mut c = captcha::Captcha::new();
        c.add_chars(4)
            .apply_filter(captcha::filters::Noise::new(0.1))
            .view(150, 50);
        let answer = c.chars_as_string().to_lowercase();
        let png = c.as_png().ok_or_else(|| "captcha render failed".to_string())?;
        let b64 = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        let id = uuid::Uuid::new_v4().to_string();
        let mut g = self.inner.lock().unwrap();
        g.captchas.insert(
            id.clone(),
            CaptchaMetaInternal {
                answer,
                expires_at: Instant::now() + self.captcha_expiration,
            },
        );
        Ok(Captcha { id, b64 })
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

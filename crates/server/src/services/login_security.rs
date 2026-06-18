use chrono::Utc;
use hmac::{Hmac, Mac};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use rand::RngCore;
use sea_orm::*;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use ::entity::{login_verification, smtp_email_config, system_setting, trusted_login_device, user};

use crate::services::now;

const LOGIN_SECURITY_KEY: &str = "login_security";
const TOTP_PERIOD: i64 = 30;
const TOTP_DIGITS: u32 = 6;
const CHALLENGE_TTL_SECS: i64 = 10 * 60;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoginSecuritySettings {
    pub require_totp: bool,
    pub require_email_verification: bool,
    pub require_device_verification: bool,
    pub allow_trusted_login_devices: bool,
}

impl Default for LoginSecuritySettings {
    fn default() -> Self {
        Self {
            require_totp: false,
            require_email_verification: false,
            require_device_verification: false,
            allow_trusted_login_devices: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmailSettings {
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    pub from: String,
    pub tls: String,
}

impl Default for EmailSettings {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 587,
            username: String::new(),
            password: String::new(),
            from: String::new(),
            tls: "starttls".to_string(),
        }
    }
}

impl EmailSettings {
    fn from_model(row: &smtp_email_config::Model) -> Self {
        Self {
            host: row.host.clone(),
            port: row.port.clamp(0, u16::MAX as i32) as u16,
            username: row.username.clone(),
            password: row.password.clone(),
            from: row.from_address.clone(),
            tls: row.tls.clone(),
        }
        .normalized()
    }

    pub fn normalized(mut self) -> Self {
        self.host = self.host.trim().to_string();
        self.username = self.username.trim().to_string();
        self.from = self.from.trim().to_string();
        self.tls = match self.tls.trim() {
            "none" => "none",
            "tls" => "tls",
            _ => "starttls",
        }
        .to_string();
        if self.port == 0 {
            self.port = if self.tls == "tls" { 465 } else { 587 };
        }
        self
    }

    pub fn is_configured(&self) -> bool {
        !self.host.trim().is_empty() && !self.from.trim().is_empty()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SmtpEmailConfigUpdate {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub clear_password: bool,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub tls: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmtpEmailConfigView {
    pub id: i32,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub from: String,
    pub tls: String,
    pub enabled: bool,
    pub password_set: bool,
    pub configured: bool,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

impl SmtpEmailConfigView {
    fn from_model(row: &smtp_email_config::Model) -> Self {
        let settings = EmailSettings::from_model(row);
        let configured = settings.is_configured();
        Self {
            id: row.id,
            name: row.name.clone(),
            host: settings.host,
            port: settings.port,
            username: settings.username,
            from: settings.from,
            tls: settings.tls,
            enabled: row.enabled,
            password_set: !row.password.is_empty(),
            configured,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginSecurityConfigView {
    pub login: LoginSecuritySettings,
}

#[derive(Debug, Clone)]
pub struct DeviceLoginInfo {
    pub id: String,
    pub uuid: String,
    pub name: String,
    pub os: String,
    pub client_type: String,
    pub ip: String,
    pub auto_login: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationKind {
    Totp,
    Email,
}

#[derive(Debug, Clone)]
pub struct LoginChallenge {
    pub kind: VerificationKind,
    pub secret: String,
}

pub async fn config_view(db: &DatabaseConnection) -> Result<LoginSecurityConfigView, DbErr> {
    let login = login_settings(db).await?;
    Ok(LoginSecurityConfigView { login })
}

pub async fn login_settings(db: &DatabaseConnection) -> Result<LoginSecuritySettings, DbErr> {
    load_json_setting(db, LOGIN_SECURITY_KEY)
        .await
        .map(|v| v.unwrap_or_default())
}

pub async fn save_login_settings(
    db: &DatabaseConnection,
    settings: LoginSecuritySettings,
) -> Result<(), DbErr> {
    save_json_setting(db, LOGIN_SECURITY_KEY, &settings).await
}

pub async fn list_smtp_email_configs(
    db: &DatabaseConnection,
) -> Result<Vec<SmtpEmailConfigView>, DbErr> {
    smtp_email_config::Entity::find()
        .order_by_desc(smtp_email_config::Column::Enabled)
        .order_by_asc(smtp_email_config::Column::Id)
        .all(db)
        .await
        .map(|rows| rows.iter().map(SmtpEmailConfigView::from_model).collect())
}

pub async fn save_smtp_email_config(
    db: &DatabaseConnection,
    id: Option<i32>,
    update: SmtpEmailConfigUpdate,
) -> Result<SmtpEmailConfigView, DbErr> {
    let settings = EmailSettings {
        host: update.host.trim().to_string(),
        port: update.port,
        username: update.username.trim().to_string(),
        password: update.password,
        from: update.from.trim().to_string(),
        tls: update.tls.trim().to_string(),
    }
    .normalized();
    let name = update.name.trim().to_string();

    let row = if let Some(id) = id.filter(|id| *id > 0) {
        let row = smtp_email_config::Entity::find_by_id(id)
            .one(db)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound(format!("smtp_email_config {id}")))?;
        let current_password = row.password.clone();
        let mut am: smtp_email_config::ActiveModel = row.into();
        am.name = Set(name);
        am.host = Set(settings.host);
        am.port = Set(i32::from(settings.port));
        am.username = Set(settings.username);
        am.password = Set(if update.clear_password {
            String::new()
        } else if settings.password.is_empty() {
            current_password
        } else {
            settings.password
        });
        am.from_address = Set(settings.from);
        am.tls = Set(settings.tls);
        am.updated_at = Set(now());
        am.update(db).await?
    } else {
        let enabled = smtp_email_config::Entity::find().count(db).await? == 0;
        smtp_email_config::ActiveModel {
            name: Set(name),
            host: Set(settings.host),
            port: Set(i32::from(settings.port)),
            username: Set(settings.username),
            password: Set(settings.password),
            from_address: Set(settings.from),
            tls: Set(settings.tls),
            enabled: Set(enabled),
            created_at: Set(now()),
            updated_at: Set(now()),
            ..Default::default()
        }
        .insert(db)
        .await?
    };
    Ok(SmtpEmailConfigView::from_model(&row))
}

pub async fn enable_smtp_email_config(
    db: &DatabaseConnection,
    id: i32,
) -> Result<SmtpEmailConfigView, DbErr> {
    let row = smtp_email_config::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound(format!("smtp_email_config {id}")))?;
    let enabled_rows = smtp_email_config::Entity::find()
        .filter(smtp_email_config::Column::Enabled.eq(true))
        .all(db)
        .await?;
    for enabled in enabled_rows {
        if enabled.id == id {
            continue;
        }
        let mut am: smtp_email_config::ActiveModel = enabled.into();
        am.enabled = Set(false);
        am.updated_at = Set(now());
        am.update(db).await?;
    }
    let mut am: smtp_email_config::ActiveModel = row.into();
    am.enabled = Set(true);
    am.updated_at = Set(now());
    let row = am.update(db).await?;
    Ok(SmtpEmailConfigView::from_model(&row))
}

pub async fn delete_smtp_email_config(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    let row = smtp_email_config::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound(format!("smtp_email_config {id}")))?;
    let was_enabled = row.enabled;
    smtp_email_config::Entity::delete_by_id(id).exec(db).await?;
    if was_enabled {
        let has_enabled = smtp_email_config::Entity::find()
            .filter(smtp_email_config::Column::Enabled.eq(true))
            .one(db)
            .await?
            .is_some();
        if !has_enabled {
            if let Some(next) = smtp_email_config::Entity::find()
                .order_by_asc(smtp_email_config::Column::Id)
                .one(db)
                .await?
            {
                let mut am: smtp_email_config::ActiveModel = next.into();
                am.enabled = Set(true);
                am.updated_at = Set(now());
                am.update(db).await?;
            }
        }
    }
    Ok(())
}

async fn email_settings(db: &DatabaseConnection) -> Result<EmailSettings, DbErr> {
    smtp_email_config::Entity::find()
        .filter(smtp_email_config::Column::Enabled.eq(true))
        .one(db)
        .await
        .map(|row| {
            row.map(|row| EmailSettings::from_model(&row))
                .unwrap_or_default()
        })
}

async fn email_settings_by_id(db: &DatabaseConnection, id: i32) -> Result<EmailSettings, DbErr> {
    smtp_email_config::Entity::find_by_id(id)
        .one(db)
        .await?
        .map(|row| EmailSettings::from_model(&row))
        .ok_or_else(|| DbErr::RecordNotFound(format!("smtp_email_config {id}")))
}

pub async fn send_test_email(
    db: &DatabaseConnection,
    config_id: Option<i32>,
    to: &str,
    title: &str,
    body: &str,
) -> Result<(), String> {
    let settings = match config_id.filter(|id| *id > 0) {
        Some(id) => email_settings_by_id(db, id).await,
        None => email_settings(db).await,
    }
    .map_err(|e| e.to_string())?;
    send_email(&settings, to, title, body).await
}

pub async fn required_verification(
    db: &DatabaseConnection,
    u: &user::Model,
    device: &DeviceLoginInfo,
) -> Result<Option<VerificationKind>, DbErr> {
    let settings = login_settings(db).await?;
    if u.tfa_enabled && (u.tfa_enforced || settings.require_totp) {
        return Ok(Some(VerificationKind::Totp));
    }
    if settings.require_email_verification || u.email_verification_enabled {
        return Ok(Some(VerificationKind::Email));
    }
    if settings.require_device_verification || u.login_device_verification_enabled {
        if !settings.allow_trusted_login_devices {
            return Ok(Some(VerificationKind::Email));
        }
        if !is_trusted_device(db, u.id, &device.id, &device.uuid).await? {
            return Ok(Some(VerificationKind::Email));
        }
    }
    Ok(None)
}

pub async fn create_login_challenge(
    db: &DatabaseConnection,
    u: &user::Model,
    device: &DeviceLoginInfo,
    kind: VerificationKind,
) -> Result<LoginChallenge, String> {
    let secret = crate::support::random::random_string(40);
    let mut code_hash = String::new();
    if kind == VerificationKind::Email {
        if u.email.trim().is_empty() {
            return Err("EmailNotConfigured".to_string());
        }
        let email = email_settings(db).await.map_err(|e| e.to_string())?;
        if !email.is_configured() {
            return Err("EmailNotConfigured".to_string());
        }
        let code = random_email_code();
        code_hash = hash_code(&secret, &code);
        let body = format!(
            "Your RustDesk Console verification code is: {code}\n\nThis code expires in 10 minutes."
        );
        send_email(
            &email,
            &u.email,
            "RustDesk Console verification code",
            &body,
        )
        .await?;
    }

    login_verification::ActiveModel {
        user_id: Set(u.id),
        secret: Set(secret.clone()),
        kind: Set(kind.as_str().to_string()),
        code_hash: Set(code_hash),
        device_id: Set(device.id.clone()),
        device_uuid: Set(device.uuid.clone()),
        device_name: Set(device.name.clone()),
        device_os: Set(device.os.clone()),
        device_type: Set(device.client_type.clone()),
        ip: Set(device.ip.clone()),
        auto_login: Set(device.auto_login),
        expires_at: Set(Utc::now().timestamp() + CHALLENGE_TTL_SECS),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(LoginChallenge { kind, secret })
}

pub async fn verify_login_challenge(
    db: &DatabaseConnection,
    secret: &str,
    email_code: Option<&str>,
    totp_code: Option<&str>,
) -> Result<(user::Model, DeviceLoginInfo), String> {
    let now_ts = Utc::now().timestamp();
    let challenge = login_verification::Entity::find()
        .filter(login_verification::Column::Secret.eq(secret.trim()))
        .filter(login_verification::Column::VerifiedAt.eq(0))
        .one(db)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "The verification code is incorrect or has expired".to_string())?;
    if challenge.expires_at < now_ts {
        return Err("The verification code is incorrect or has expired".to_string());
    }
    let u = user::Entity::find_by_id(challenge.user_id)
        .one(db)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "UserNotFound".to_string())?;
    match challenge.kind.as_str() {
        login_verification::KIND_TOTP => {
            let code = totp_code
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| "The verification code is incorrect or has expired".to_string())?;
            if !verify_totp(&u.tfa_secret, code) {
                return Err("The verification code is incorrect or has expired".to_string());
            }
        }
        login_verification::KIND_EMAIL => {
            let code = email_code
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| "The verification code is incorrect or has expired".to_string())?;
            if hash_code(&challenge.secret, code) != challenge.code_hash {
                return Err("The verification code is incorrect or has expired".to_string());
            }
        }
        _ => return Err("The verification code is incorrect or has expired".to_string()),
    }

    let mut am: login_verification::ActiveModel = challenge.clone().into();
    am.verified_at = Set(now_ts);
    am.updated_at = Set(now());
    am.update(db).await.map_err(|e| e.to_string())?;

    let device = DeviceLoginInfo {
        id: challenge.device_id,
        uuid: challenge.device_uuid,
        name: challenge.device_name,
        os: challenge.device_os,
        client_type: challenge.device_type,
        ip: challenge.ip,
        auto_login: challenge.auto_login,
    };
    let settings = login_settings(db).await.map_err(|e| e.to_string())?;
    if settings.allow_trusted_login_devices && device.auto_login {
        let _ = trust_device(db, u.id, &device).await;
    }
    Ok((u, device))
}

pub fn generate_totp_secret() -> String {
    let mut bytes = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut bytes);
    base32_encode(&bytes)
}

pub fn totp_uri(username: &str, secret: &str) -> String {
    let label_raw = format!("RustDesk Console:{username}");
    let label = urlencoding::encode(&label_raw);
    let issuer = urlencoding::encode("RustDesk Console");
    format!("otpauth://totp/{label}?secret={secret}&issuer={issuer}&digits=6&period=30")
}

pub fn verify_totp(secret: &str, code: &str) -> bool {
    let code = code.trim();
    if code.len() != TOTP_DIGITS as usize || !code.chars().all(|ch| ch.is_ascii_digit()) {
        return false;
    }
    let Ok(secret_bytes) = base32_decode(secret) else {
        return false;
    };
    let counter = Utc::now().timestamp() / TOTP_PERIOD;
    (-1..=1).any(|offset| totp_at(&secret_bytes, counter + offset) == code)
}

pub async fn enable_user_totp(
    db: &DatabaseConnection,
    u: &user::Model,
    secret: &str,
    code: &str,
) -> Result<(), String> {
    if !verify_totp(secret, code) {
        return Err("The verification code is incorrect or has expired".to_string());
    }
    let mut am: user::ActiveModel = u.clone().into();
    am.tfa_secret = Set(secret.trim().to_string());
    am.tfa_enabled = Set(true);
    am.updated_at = Set(now());
    am.update(db).await.map(|_| ()).map_err(|e| e.to_string())
}

pub async fn disable_user_totp(
    db: &DatabaseConnection,
    u: &user::Model,
    code: Option<&str>,
) -> Result<(), String> {
    if u.tfa_enforced {
        return Err("Two-factor authentication is enforced for this user".to_string());
    }
    if u.tfa_enabled && !verify_totp(&u.tfa_secret, code.unwrap_or_default()) {
        return Err("The verification code is incorrect or has expired".to_string());
    }
    reset_user_totp(db, u).await
}

pub async fn reset_user_totp(db: &DatabaseConnection, u: &user::Model) -> Result<(), String> {
    let mut am: user::ActiveModel = u.clone().into();
    am.tfa_secret = Set(String::new());
    am.tfa_enabled = Set(false);
    am.tfa_enforced = Set(false);
    am.updated_at = Set(now());
    am.update(db).await.map(|_| ()).map_err(|e| e.to_string())
}

pub async fn update_user_login_security(
    db: &DatabaseConnection,
    u: &user::Model,
    tfa_enforced: bool,
    email_verification_enabled: bool,
    login_device_verification_enabled: bool,
) -> Result<(), String> {
    let mut am: user::ActiveModel = u.clone().into();
    am.tfa_enforced = Set(tfa_enforced);
    am.email_verification_enabled = Set(email_verification_enabled);
    am.login_device_verification_enabled = Set(login_device_verification_enabled);
    am.updated_at = Set(now());
    am.update(db).await.map(|_| ()).map_err(|e| e.to_string())
}

pub async fn trusted_devices_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<trusted_login_device::Model>, DbErr> {
    trusted_login_device::Entity::find()
        .filter(trusted_login_device::Column::UserId.eq(user_id))
        .order_by_desc(trusted_login_device::Column::LastSeenAt)
        .all(db)
        .await
}

pub async fn trusted_device_count_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<u64, DbErr> {
    trusted_login_device::Entity::find()
        .filter(trusted_login_device::Column::UserId.eq(user_id))
        .count(db)
        .await
}

pub async fn delete_trusted_device(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    id: i32,
) -> Result<(), DbErr> {
    let mut delete =
        trusted_login_device::Entity::delete_many().filter(trusted_login_device::Column::Id.eq(id));
    if let Some(user_id) = user_id {
        delete = delete.filter(trusted_login_device::Column::UserId.eq(user_id));
    }
    delete.exec(db).await?;
    Ok(())
}

async fn is_trusted_device(
    db: &DatabaseConnection,
    user_id: i32,
    device_id: &str,
    uuid: &str,
) -> Result<bool, DbErr> {
    let devices = trusted_devices_for_user(db, user_id).await?;
    Ok(devices.into_iter().any(|device| {
        (!uuid.is_empty() && device.device_uuid == uuid)
            || (!device_id.is_empty() && device.device_id == device_id)
    }))
}

async fn trust_device(
    db: &DatabaseConnection,
    user_id: i32,
    device: &DeviceLoginInfo,
) -> Result<(), DbErr> {
    if device.id.trim().is_empty() && device.uuid.trim().is_empty() {
        return Ok(());
    }
    let existing = trusted_devices_for_user(db, user_id)
        .await?
        .into_iter()
        .find(|row| {
            (!device.uuid.is_empty() && row.device_uuid == device.uuid)
                || (!device.id.is_empty() && row.device_id == device.id)
        });
    let now_ts = Utc::now().timestamp();
    match existing {
        Some(row) => {
            let mut am: trusted_login_device::ActiveModel = row.into();
            am.device_id = Set(device.id.clone());
            am.device_uuid = Set(device.uuid.clone());
            am.device_name = Set(device.name.clone());
            am.device_os = Set(device.os.clone());
            am.device_type = Set(device.client_type.clone());
            am.ip = Set(device.ip.clone());
            am.last_seen_at = Set(now_ts);
            am.updated_at = Set(now());
            am.update(db).await?;
        }
        None => {
            trusted_login_device::ActiveModel {
                user_id: Set(user_id),
                device_id: Set(device.id.clone()),
                device_uuid: Set(device.uuid.clone()),
                device_name: Set(device.name.clone()),
                device_os: Set(device.os.clone()),
                device_type: Set(device.client_type.clone()),
                ip: Set(device.ip.clone()),
                last_seen_at: Set(now_ts),
                created_at: Set(now()),
                updated_at: Set(now()),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

async fn load_json_setting<T: for<'de> Deserialize<'de>>(
    db: &DatabaseConnection,
    key: &str,
) -> Result<Option<T>, DbErr> {
    let Some(row) = system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(key))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    serde_json::from_str(&row.value)
        .map(Some)
        .map_err(|e| DbErr::Custom(e.to_string()))
}

async fn save_json_setting<T: Serialize>(
    db: &DatabaseConnection,
    key: &str,
    value: &T,
) -> Result<(), DbErr> {
    let encoded = serde_json::to_string(value).map_err(|e| DbErr::Custom(e.to_string()))?;
    match system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(key))
        .one(db)
        .await?
    {
        Some(row) => {
            let mut am: system_setting::ActiveModel = row.into();
            am.value = Set(encoded);
            am.updated_at = Set(now());
            am.update(db).await?;
        }
        None => {
            system_setting::ActiveModel {
                key: Set(key.to_string()),
                value: Set(encoded),
                created_at: Set(now()),
                updated_at: Set(now()),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

async fn send_email(
    settings: &EmailSettings,
    to: &str,
    title: &str,
    body: &str,
) -> Result<(), String> {
    let settings = settings.clone().normalized();
    if !settings.is_configured() {
        return Err("EmailNotConfigured".to_string());
    }
    let to = to.trim().to_string();
    let title = title.to_string();
    let body = body.to_string();
    tokio::task::spawn_blocking(move || {
        let email = Message::builder()
            .from(
                settings
                    .from
                    .parse()
                    .map_err(|e| format!("invalid sender: {e}"))?,
            )
            .to(to.parse().map_err(|e| format!("invalid recipient: {e}"))?)
            .subject(title)
            .body(body)
            .map_err(|e| e.to_string())?;
        let mut builder = match settings.tls.as_str() {
            "none" => SmtpTransport::builder_dangerous(settings.host.as_str()),
            "tls" => SmtpTransport::relay(settings.host.as_str()).map_err(|e| e.to_string())?,
            _ => {
                SmtpTransport::starttls_relay(settings.host.as_str()).map_err(|e| e.to_string())?
            }
        };
        builder = builder.port(settings.port);
        if !settings.username.is_empty() {
            builder = builder.credentials(Credentials::new(settings.username, settings.password));
        }
        builder.build().send(&email).map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn random_email_code() -> String {
    let mut rng = rand::thread_rng();
    format!("{:06}", rng.next_u32() % 1_000_000)
}

fn hash_code(secret: &str, code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b":");
    hasher.update(code.trim().as_bytes());
    hex_encode(&hasher.finalize())
}

fn totp_at(secret: &[u8], counter: i64) -> String {
    if counter < 0 {
        return String::new();
    }
    type HmacSha1 = Hmac<Sha1>;
    let mut mac = HmacSha1::new_from_slice(secret).unwrap();
    mac.update(&(counter as u64).to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let binary = (((digest[offset] & 0x7f) as u32) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | (digest[offset + 3] as u32);
    format!("{:06}", binary % 10u32.pow(TOTP_DIGITS))
}

fn base32_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let mut buffer = 0u32;
    let mut bits = 0u8;
    for byte in bytes {
        buffer = (buffer << 8) | (*byte as u32);
        bits += 8;
        while bits >= 5 {
            let index = ((buffer >> (bits - 5)) & 0x1f) as usize;
            out.push(ALPHABET[index] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 0x1f) as usize;
        out.push(ALPHABET[index] as char);
    }
    out
}

fn base32_decode(value: &str) -> Result<Vec<u8>, ()> {
    let mut buffer = 0u32;
    let mut bits = 0u8;
    let mut out = Vec::new();
    for ch in value.chars().filter(|ch| !ch.is_whitespace() && *ch != '=') {
        let val = match ch.to_ascii_uppercase() {
            'A'..='Z' => ch.to_ascii_uppercase() as u8 - b'A',
            '2'..='7' => ch as u8 - b'2' + 26,
            _ => return Err(()),
        } as u32;
        buffer = (buffer << 5) | val;
        bits += 5;
        if bits >= 8 {
            out.push(((buffer >> (bits - 8)) & 0xff) as u8);
            bits -= 8;
        }
    }
    Ok(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

impl VerificationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VerificationKind::Totp => login_verification::KIND_TOTP,
            VerificationKind::Email => login_verification::KIND_EMAIL,
        }
    }

    pub fn response_tfa_type(self) -> &'static str {
        match self {
            VerificationKind::Totp => "tfa_check",
            VerificationKind::Email => "email_check",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Schema};

    async fn smtp_test_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(db.get_database_backend());
        db.execute(
            db.get_database_backend()
                .build(&schema.create_table_from_entity(smtp_email_config::Entity)),
        )
        .await
        .unwrap();
        db
    }

    #[test]
    fn base32_round_trips_secret_bytes() {
        let raw = b"12345678901234567890";
        let encoded = base32_encode(raw);
        assert_eq!(base32_decode(&encoded).unwrap(), raw);
    }

    #[test]
    fn totp_matches_rfc_6238_sample_for_sha1_8_digits_truncated_to_current_helper() {
        let secret = b"12345678901234567890";
        assert_eq!(totp_at(secret, 1), "287082");
    }

    #[tokio::test]
    async fn smtp_configs_enable_switch_and_delete_fallback() {
        let db = smtp_test_db().await;
        let first = save_smtp_email_config(
            &db,
            None,
            SmtpEmailConfigUpdate {
                name: "Primary".to_string(),
                host: "smtp1.example.com".to_string(),
                port: 587,
                username: "u1".to_string(),
                password: "p1".to_string(),
                from: "noreply1@example.com".to_string(),
                tls: "starttls".to_string(),
                clear_password: false,
            },
        )
        .await
        .unwrap();
        assert!(first.enabled);
        assert!(first.password_set);

        let second = save_smtp_email_config(
            &db,
            None,
            SmtpEmailConfigUpdate {
                name: "Backup".to_string(),
                host: "smtp2.example.com".to_string(),
                port: 465,
                username: "u2".to_string(),
                password: "p2".to_string(),
                from: "noreply2@example.com".to_string(),
                tls: "tls".to_string(),
                clear_password: false,
            },
        )
        .await
        .unwrap();
        assert!(!second.enabled);

        enable_smtp_email_config(&db, second.id).await.unwrap();
        let rows = list_smtp_email_configs(&db).await.unwrap();
        assert_eq!(rows.iter().filter(|row| row.enabled).count(), 1);
        assert!(rows.iter().any(|row| row.id == second.id && row.enabled));

        delete_smtp_email_config(&db, second.id).await.unwrap();
        let rows = list_smtp_email_configs(&db).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, first.id);
        assert!(rows[0].enabled);
    }
}

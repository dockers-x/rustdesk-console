use chrono::Utc;
use hmac::{Hmac, Mac};
use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, Message, SmtpTransport, Transport};
use rand::RngCore;
use sea_orm::{sea_query::Expr, *};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use ::entity::{login_verification, smtp_email_config, system_setting, trusted_login_device, user};

use super::mail_templates::{self, MailKind, MailMessage};
use crate::services::now;

const LOGIN_SECURITY_KEY: &str = "login_security";
const DEFAULT_TOTP_ISSUER: &str = "RustDesk Console";
const TOTP_PERIOD: i64 = 30;
const TOTP_DIGITS: u32 = 6;
const CHALLENGE_TTL_SECS: i64 = 10 * 60;
const PASSWORD_RESET_TTL_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoginSecuritySettings {
    pub require_totp: bool,
    #[serde(default)]
    pub new_user_require_totp: bool,
    #[serde(default)]
    pub totp_issuer: String,
    pub require_email_verification: bool,
    pub require_device_verification: bool,
    pub allow_trusted_login_devices: bool,
    #[serde(default)]
    pub allow_registration: Option<bool>,
}

impl Default for LoginSecuritySettings {
    fn default() -> Self {
        Self {
            require_totp: false,
            new_user_require_totp: false,
            totp_issuer: String::new(),
            require_email_verification: false,
            require_device_verification: false,
            allow_trusted_login_devices: true,
            allow_registration: None,
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
    #[serde(default)]
    pub from_name: String,
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
            from_name: String::new(),
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
            from_name: row.from_name.clone(),
            tls: row.tls.clone(),
        }
        .normalized()
    }

    pub fn normalized(mut self) -> Self {
        self.host = self.host.trim().to_string();
        self.username = self.username.trim().to_string();
        self.from = self.from.trim().to_string();
        self.from_name = self.from_name.trim().to_string();
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
    pub from_name: String,
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
    pub from_name: String,
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
            from_name: settings.from_name,
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

pub async fn config_view(
    db: &DatabaseConnection,
    registration_default: bool,
) -> Result<LoginSecurityConfigView, DbErr> {
    let mut login = login_settings(db).await?;
    login.allow_registration = Some(login.allow_registration.unwrap_or(registration_default));
    Ok(LoginSecurityConfigView { login })
}

pub async fn login_settings(db: &impl ConnectionTrait) -> Result<LoginSecuritySettings, DbErr> {
    load_json_setting(db, LOGIN_SECURITY_KEY)
        .await
        .map(|v| v.unwrap_or_default())
}

pub async fn registration_enabled(
    db: &DatabaseConnection,
    configured: bool,
) -> Result<bool, DbErr> {
    Ok(login_settings(db)
        .await?
        .allow_registration
        .unwrap_or(configured))
}

pub async fn save_login_settings(
    db: &DatabaseConnection,
    mut settings: LoginSecuritySettings,
) -> Result<(), DbErr> {
    let issuer = settings.totp_issuer.trim();
    if issuer.chars().count() > 80 || issuer.contains(':') || issuer.chars().any(char::is_control) {
        return Err(DbErr::Custom(
            "The TOTP issuer must be at most 80 characters, without a colon or control characters".into(),
        ));
    }
    settings.totp_issuer = issuer.to_owned();
    save_json_setting(db, LOGIN_SECURITY_KEY, &settings).await
}

pub async fn insert_user_with_enrollment_policy(
    db: &impl ConnectionTrait,
    mut account: user::ActiveModel,
) -> Result<user::Model, DbErr> {
    if login_settings(db).await?.new_user_require_totp {
        account.tfa_enforced = Set(true);
    }
    account.insert(db).await
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
        from_name: update.from_name.trim().to_string(),
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
        am.from_name = Set(settings.from_name);
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
            from_name: Set(settings.from_name),
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
    locale: &str,
) -> Result<(), String> {
    let settings = match config_id.filter(|id| *id > 0) {
        Some(id) => email_settings_by_id(db, id).await,
        None => email_settings(db).await,
    }
    .map_err(|e| e.to_string())?;
    send_email(
        &settings,
        to,
        mail_templates::compose(locale, MailKind::SmtpTest),
    )
    .await
}

pub fn totp_required(u: &user::Model, settings: &LoginSecuritySettings) -> bool {
    u.is_admin() || u.tfa_enabled || u.tfa_enforced || settings.require_totp
}

pub async fn required_verification(
    db: &DatabaseConnection,
    u: &user::Model,
    _device: &DeviceLoginInfo,
) -> Result<Option<VerificationKind>, DbErr> {
    let settings = login_settings(db).await?;
    Ok(verification_for(u, &settings))
}

pub(crate) fn verification_for(
    u: &user::Model,
    settings: &LoginSecuritySettings,
) -> Option<VerificationKind> {
    if u.tfa_enabled && totp_required(u, settings) {
        return Some(VerificationKind::Totp);
    }
    if settings.require_email_verification || u.email_verification_enabled {
        return Some(VerificationKind::Email);
    }
    if settings.require_device_verification || u.login_device_verification_enabled {
        // Reported IDs/UUIDs are public identifiers, not proof of device possession.
        return Some(VerificationKind::Email);
    }
    None
}

pub async fn create_login_challenge(
    db: &DatabaseConnection,
    u: &user::Model,
    device: &DeviceLoginInfo,
    kind: VerificationKind,
    locale: &str,
) -> Result<LoginChallenge, String> {
    create_challenge(db, u, device, kind, false, locale).await
}

pub async fn create_federated_challenge(
    db: &DatabaseConnection,
    u: &user::Model,
    device: &DeviceLoginInfo,
    kind: VerificationKind,
    locale: &str,
) -> Result<LoginChallenge, String> {
    create_challenge(db, u, device, kind, true, locale).await
}

async fn create_challenge(
    db: &DatabaseConnection,
    u: &user::Model,
    device: &DeviceLoginInfo,
    kind: VerificationKind,
    federated: bool,
    locale: &str,
) -> Result<LoginChallenge, String> {
    let secret = crate::support::random::random_string(40);
    let mut code_hash = String::new();
    let email_message = if kind == VerificationKind::Email {
        if u.email.trim().is_empty() {
            return Err("EmailNotConfigured".to_string());
        }
        let email = email_settings(db).await.map_err(|e| e.to_string())?;
        if !email.is_configured() {
            return Err("EmailNotConfigured".to_string());
        }
        let code = random_email_code();
        code_hash = hash_code(&secret, &code);
        Some((
            email,
            mail_templates::compose(locale, MailKind::LoginCode(&code)),
        ))
    } else {
        None
    };

    // Serialize issuance on the account row across workers and database backends.
    // The quota counts consumed as well as outstanding challenges, so changing
    // IP or requesting a fresh secret cannot reset the guessing budget.
    let transaction = db.begin().await.map_err(|e| e.to_string())?;
    user::Entity::update_many()
        .col_expr(user::Column::Id, Expr::col(user::Column::Id).into())
        .filter(user::Column::Id.eq(u.id))
        .exec(&transaction)
        .await
        .map_err(|e| e.to_string())?;
    let current = user::Entity::find_by_id(u.id)
        .one(&transaction)
        .await
        .map_err(|e| e.to_string())?;
    if !current.is_some_and(|current| {
        current.is_enabled()
            && current.password == u.password
            && current.tfa_secret == u.tfa_secret
            && current.tfa_enabled == u.tfa_enabled
    }) {
        return Err("account changed during authentication".into());
    }
    let recent = login_verification::Entity::find()
        .filter(login_verification::Column::UserId.eq(u.id))
        .filter(login_verification::Column::Kind.is_in([
            login_verification::KIND_EMAIL,
            login_verification::KIND_TOTP,
        ]))
        .filter(login_verification::Column::ExpiresAt.gt(Utc::now().timestamp()))
        .count(&transaction)
        .await
        .map_err(|e| e.to_string())?;
    if recent >= 5 {
        return Err("Too many verification attempts; try again in 10 minutes".to_string());
    }

    login_verification::ActiveModel {
        user_id: Set(u.id),
        secret: Set(secret.clone()),
        kind: Set(kind.as_str().to_string()),
        federated: Set(federated),
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
    .insert(&transaction)
    .await
    .map_err(|e| e.to_string())?;

    transaction.commit().await.map_err(|e| e.to_string())?;
    if let Some((settings, message)) = email_message {
        send_email(&settings, &u.email, message).await?;
    }
    Ok(LoginChallenge { kind, secret })
}

const PASSWORD_RESET_KINDS: [&str; 2] = [
    login_verification::KIND_PASSWORD_RESET,
    login_verification::KIND_ADMIN_PASSWORD_RESET,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum PasswordResetDelivery {
    VerificationCode,
    MailToken,
}

pub async fn request_password_reset(
    db: &DatabaseConnection,
    account: &str,
    ip: &str,
    locale: &str,
) -> Result<String, String> {
    let fake_secret = || crate::support::random::random_string(40);
    let account = account.trim();
    if account.is_empty() {
        return Ok(fake_secret());
    }
    let candidate = user::Entity::find()
        .filter(
            Condition::any()
                .add(user::Column::Username.eq(account))
                .add(user::Column::Email.eq(account.to_lowercase())),
        )
        .one(db)
        .await
        .map_err(|e| e.to_string())?;
    let Some(target) = candidate else {
        return Ok(fake_secret());
    };
    match issue_password_reset(
        db,
        &target,
        ip,
        PasswordResetDelivery::VerificationCode,
        locale,
    )
    .await
    {
        Ok(secret) => Ok(secret),
        Err(_) => Ok(fake_secret()),
    }
}

pub async fn send_user_password_reset(
    db: &DatabaseConnection,
    target: &user::Model,
    ip: &str,
    locale: &str,
) -> Result<(), String> {
    issue_password_reset(db, target, ip, PasswordResetDelivery::MailToken, locale)
        .await
        .map(|_| ())
}

fn password_reset_hash(secret: &str, code: &str, recipient: Option<&str>) -> String {
    match recipient {
        Some(email) => hash_code(&format!("{secret}\0{email}"), code),
        None => hash_code(secret, code),
    }
}

async fn issue_password_reset(
    db: &DatabaseConnection,
    target: &user::Model,
    ip: &str,
    delivery: PasswordResetDelivery,
    locale: &str,
) -> Result<String, String> {
    if !target.is_enabled() {
        return Err("UserDisabled".into());
    }
    if target.email.trim().is_empty() {
        return Err("EmailNotConfigured".into());
    }
    if super::ldap::is_directory_user(db, target.id)
        .await
        .map_err(|_| "OperationFailed")?
    {
        return Err("DirectoryPasswordManaged".into());
    }
    let email = email_settings(db).await.map_err(|_| "OperationFailed")?;
    if !email.is_configured() {
        return Err("ResetMailSmtpRequired".into());
    }
    let secret = crate::support::random::random_string(40);
    let code = random_email_code();
    let (kind, recipient, message) = match delivery {
        PasswordResetDelivery::VerificationCode => (
            login_verification::KIND_PASSWORD_RESET,
            None,
            mail_templates::compose(locale, MailKind::ResetCode(&code)),
        ),
        PasswordResetDelivery::MailToken => (
            login_verification::KIND_ADMIN_PASSWORD_RESET,
            Some(target.email.as_str()),
            mail_templates::compose(
                locale,
                MailKind::AdminResetToken(&format!("{secret}.{code}")),
            ),
        ),
    };
    let tx = db.begin().await.map_err(|_| "OperationFailed")?;
    let current = super::user::lock_account(&tx, target.id)
        .await
        .map_err(|_| "OperationFailed")?;
    if !current.is_some_and(|account| {
        account.is_enabled()
            && account.password == target.password
            && account.email == target.email
            && account.updated_at == target.updated_at
            && account.is_admin == target.is_admin
    }) {
        return Err("OperationFailed".into());
    }
    if login_verification::Entity::find()
        .filter(login_verification::Column::UserId.eq(target.id))
        .filter(login_verification::Column::Kind.is_in(PASSWORD_RESET_KINDS))
        .filter(login_verification::Column::VerifiedAt.eq(0))
        .filter(login_verification::Column::ExpiresAt.gt(Utc::now().timestamp()))
        .one(&tx)
        .await
        .map_err(|_| "OperationFailed")?
        .is_some()
    {
        return Err("ResetMailCooldown".into());
    }
    let challenge = login_verification::ActiveModel {
        user_id: Set(target.id),
        secret: Set(secret.clone()),
        kind: Set(kind.into()),
        code_hash: Set(password_reset_hash(&secret, &code, recipient)),
        ip: Set(ip.into()),
        expires_at: Set(Utc::now().timestamp() + PASSWORD_RESET_TTL_SECS),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    }
    .insert(&tx)
    .await
    .map_err(|_| "OperationFailed")?;
    tx.commit().await.map_err(|_| "OperationFailed")?;

    if send_email(&email, &target.email, message).await.is_err() {
        tracing::warn!(user_id = target.id, "failed to send password reset email");
        login_verification::Entity::delete_by_id(challenge.id)
            .exec(db)
            .await
            .map_err(|_| "OperationFailed")?;
        return Err("ResetMailDeliveryFailed".into());
    }
    Ok(secret)
}

pub async fn confirm_password_reset(
    db: &DatabaseConnection,
    secret: &str,
    code: &str,
    new_password: &str,
) -> Result<i32, String> {
    if new_password.len() < 4 {
        return Err("ParamsError".into());
    }
    let now_ts = Utc::now().timestamp();
    let challenge = login_verification::Entity::find()
        .filter(login_verification::Column::Secret.eq(secret.trim()))
        .filter(login_verification::Column::Kind.is_in(PASSWORD_RESET_KINDS))
        .filter(login_verification::Column::VerifiedAt.eq(0))
        .one(db)
        .await
        .map_err(|_| "OperationFailed")?
        .ok_or("ResetCodeInvalid")?;
    if challenge.expires_at <= now_ts {
        return Err("ResetCodeInvalid".into());
    }
    let u = user::Entity::find_by_id(challenge.user_id)
        .one(db)
        .await
        .map_err(|_| "OperationFailed")?
        .filter(user::Model::is_enabled)
        .ok_or("ResetCodeInvalid")?;
    if super::ldap::is_directory_user(db, u.id)
        .await
        .map_err(|_| "OperationFailed")?
    {
        return Err("ResetCodeInvalid".into());
    }
    let recipient = (challenge.kind == login_verification::KIND_ADMIN_PASSWORD_RESET)
        .then_some(u.email.as_str());
    let claimed = login_verification::Entity::update_many()
        .col_expr(login_verification::Column::VerifiedAt, Expr::value(now_ts))
        .filter(login_verification::Column::Id.eq(challenge.id))
        .filter(login_verification::Column::VerifiedAt.eq(0))
        .exec(db)
        .await
        .map_err(|_| "OperationFailed")?;
    if claimed.rows_affected != 1
        || password_reset_hash(&challenge.secret, code, recipient) != challenge.code_hash
    {
        return Err("ResetCodeInvalid".into());
    }

    crate::services::user::update_password(db, &u, new_password)
        .await
        .map_err(|_| "OperationFailed")?;
    Ok(u.id)
}

#[derive(Debug, Clone, Copy)]
pub enum ChallengeSources {
    All,
    FederatedOnly,
}

#[derive(Debug)]
pub struct VerifiedChallenge {
    pub account: user::Model,
    pub device: DeviceLoginInfo,
    pub kind: VerificationKind,
    pub federated: bool,
}

pub async fn verify_login_challenge(
    db: &DatabaseConnection,
    config: &crate::config::Config,
    secret: &str,
    email_code: Option<&str>,
    totp_code: Option<&str>,
    sources: ChallengeSources,
) -> Result<VerifiedChallenge, String> {
    let now_ts = Utc::now().timestamp();
    let mut query = login_verification::Entity::find()
        .filter(login_verification::Column::Secret.eq(secret.trim()))
        .filter(login_verification::Column::Kind.is_in([
            login_verification::KIND_EMAIL,
            login_verification::KIND_TOTP,
        ]))
        .filter(login_verification::Column::VerifiedAt.eq(0));
    if let ChallengeSources::FederatedOnly = sources {
        query = query.filter(login_verification::Column::Federated.eq(true));
    }
    let challenge = query
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
    let claimed = login_verification::Entity::update_many()
        .col_expr(
            login_verification::Column::VerifiedAt,
            sea_orm::sea_query::Expr::value(now_ts),
        )
        .filter(login_verification::Column::Id.eq(challenge.id))
        .filter(login_verification::Column::VerifiedAt.eq(0))
        .exec(db)
        .await
        .map_err(|e| e.to_string())?;
    if claimed.rows_affected != 1 {
        return Err("The verification code is incorrect or has expired".to_string());
    }

    let kind = match challenge.kind.as_str() {
        login_verification::KIND_TOTP => {
            let code = totp_code
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| "The verification code is incorrect or has expired".to_string())?;
            consume_totp(db, config, &u, code).await?;
            VerificationKind::Totp
        }
        login_verification::KIND_EMAIL => {
            let code = email_code
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| "The verification code is incorrect or has expired".to_string())?;
            if hash_code(&challenge.secret, code) != challenge.code_hash {
                return Err("The verification code is incorrect or has expired".to_string());
            }
            VerificationKind::Email
        }
        _ => return Err("The verification code is incorrect or has expired".to_string()),
    };

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
    Ok(VerifiedChallenge {
        account: u,
        device,
        kind,
        federated: challenge.federated,
    })
}

pub fn generate_totp_secret() -> String {
    let mut bytes = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut bytes);
    base32_encode(&bytes)
}

pub fn totp_uri(username: &str, secret: &str, issuer: &str) -> String {
    let issuer = if issuer.is_empty() { DEFAULT_TOTP_ISSUER } else { issuer };
    let label_raw = format!("{issuer}:{username}");
    let label = urlencoding::encode(&label_raw);
    let issuer = urlencoding::encode(issuer);
    format!("otpauth://totp/{label}?secret={secret}&issuer={issuer}&digits=6&period=30")
}

const INVALID_TOTP: &str = "TotpCodeInvalidOrUsed";

fn totp_step(secret: &str, code: &str) -> Option<i64> {
    let code = code.trim();
    if code.len() != TOTP_DIGITS as usize || !code.bytes().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let secret_bytes = base32_decode(secret).ok()?;
    let current = Utc::now().timestamp() / TOTP_PERIOD;
    [current, current - 1, current + 1]
        .into_iter()
        .find(|step| *step >= 0 && totp_at(&secret_bytes, *step) == code)
}

#[cfg(test)]
pub(crate) fn test_totp_code(secret: &str, offset: i64) -> String {
    totp_at(
        &base32_decode(secret).unwrap(),
        Utc::now().timestamp() / TOTP_PERIOD + offset,
    )
}

async fn consume_totp(
    db: &DatabaseConnection,
    config: &crate::config::Config,
    account: &user::Model,
    code: &str,
) -> Result<(), String> {
    let transaction = db.begin().await.map_err(|error| error.to_string())?;
    let current = super::user::lock_account(&transaction, account.id).await
        .map_err(|error| error.to_string())?
        .filter(|current| current.is_enabled() && current.tfa_enabled
            && current.tfa_secret == account.tfa_secret && current.updated_at == account.updated_at)
        .ok_or(INVALID_TOTP)?;
    consume_totp_in_transaction(&transaction, config, &current, code).await?;
    transaction.commit().await.map_err(|error| error.to_string())
}

/// Call while holding the account write lock. All consumers share one counter.
async fn consume_totp_in_transaction(
    transaction: &sea_orm::DatabaseTransaction,
    config: &crate::config::Config,
    account: &user::Model,
    code: &str,
) -> Result<(), String> {
    use ::entity::totp_counter;
    let seed = crate::support::totp_vault::TotpVault::load(config, false)
        .and_then(|vault| vault.open(account.id, &account.tfa_secret))
        .map_err(|error| {
            tracing::warn!(%error, user_id = account.id, "Could not read TOTP seed");
            INVALID_TOTP
        })?;
    let step = totp_step(&seed, code).ok_or(INVALID_TOTP)?;
    let row = totp_counter::Entity::find_by_id(account.id).one(transaction).await
        .map_err(|error| error.to_string())?;
    if row.is_some_and(|row| row.last_step >= step) {
        return Err(INVALID_TOTP.to_string());
    }
    set_totp_step(transaction, account.id, step).await.map_err(|error| error.to_string())
}

async fn set_totp_step(db: &impl sea_orm::ConnectionTrait, user_id: i32, step: i64) -> Result<(), sea_orm::DbErr> {
    use ::entity::totp_counter;
    use sea_orm::sea_query::OnConflict;

    totp_counter::Entity::insert(totp_counter::ActiveModel { user_id: Set(user_id), last_step: Set(step) })
        .on_conflict(OnConflict::column(totp_counter::Column::UserId)
            .update_column(totp_counter::Column::LastStep).to_owned())
        .exec_without_returning(db).await?;
    Ok(())
}

pub async fn enable_user_totp(
    db: &DatabaseConnection,
    config: &crate::config::Config,
    u: &user::Model,
    secret: &str,
    code: &str,
) -> Result<(), String> {
    let step = totp_step(secret, code).filter(|_| base32_decode(secret).is_ok_and(|decoded| decoded.len() == 20))
        .ok_or(INVALID_TOTP)?;
    let encrypted = crate::support::totp_vault::TotpVault::load(config, true)
        .and_then(|vault| vault.seal(u.id, secret.trim())).map_err(|error| error.to_string())?;
    let transaction = db.begin().await.map_err(|e| e.to_string())?;
    let updated = user::Entity::update_many()
        .col_expr(user::Column::TfaSecret, Expr::value(encrypted))
        .col_expr(user::Column::TfaEnabled, Expr::value(true))
        .col_expr(user::Column::UpdatedAt, Expr::value(now()))
        .filter(user::Column::Id.eq(u.id))
        .filter(user::Column::TfaEnabled.eq(false))
        .filter(user::Column::Status.eq(user::STATUS_ENABLE))
        .filter(user::Column::Password.eq(&u.password))
        .exec(&transaction)
        .await
        .map_err(|e| e.to_string())?;
    if updated.rows_affected != 1 {
        return Err("TOTP is already configured or the account has changed".to_string());
    }
    set_totp_step(&transaction, u.id, step).await.map_err(|e| e.to_string())?;
    super::user::revoke_credentials(&transaction, u.id)
        .await
        .map_err(|e| e.to_string())?;
    transaction.commit().await.map_err(|e| e.to_string())
}

pub async fn disable_user_totp(
    db: &DatabaseConnection,
    config: &crate::config::Config,
    u: &user::Model,
    code: Option<&str>,
) -> Result<(), String> {
    let transaction = db.begin().await.map_err(|error| error.to_string())?;
    let current = super::user::lock_account(&transaction, u.id).await
        .map_err(|error| error.to_string())?
        .filter(|current| current.is_enabled() && current.tfa_secret == u.tfa_secret && current.updated_at == u.updated_at)
        .ok_or(INVALID_TOTP)?;
    let settings = login_settings(&transaction).await.map_err(|e| e.to_string())?;
    if current.is_admin() || current.tfa_enforced || settings.require_totp {
        return Err("Two-factor authentication is enforced for this user".to_string());
    }
    if current.tfa_enabled {
        consume_totp_in_transaction(&transaction, config, &current, code.unwrap_or_default()).await?;
    }
    clear_totp(&transaction, current).await?;
    transaction.commit().await.map_err(|e| e.to_string())
}

pub async fn reset_user_totp(db: &DatabaseConnection, u: &user::Model) -> Result<(), String> {
    let transaction = db.begin().await.map_err(|e| e.to_string())?;
    let current = super::user::lock_account(&transaction, u.id).await
        .map_err(|error| error.to_string())?.ok_or("UserNotFound")?;
    clear_totp(&transaction, current).await?;
    transaction.commit().await.map_err(|e| e.to_string())
}

async fn clear_totp(transaction: &sea_orm::DatabaseTransaction, account: user::Model) -> Result<(), String> {
    let user_id = account.id;
    let mut row: user::ActiveModel = account.into();
    row.tfa_secret = Set(String::new());
    row.tfa_enabled = Set(false);
    row.updated_at = Set(now());
    row.update(transaction).await.map_err(|error| error.to_string())?;
    ::entity::totp_counter::Entity::delete_by_id(user_id).exec(transaction).await
        .map_err(|error| error.to_string())?;
    super::user::revoke_credentials(transaction, user_id).await
        .map_err(|error| error.to_string())
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
    db: &impl ConnectionTrait,
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
    message: MailMessage,
) -> Result<(), String> {
    let settings = settings.clone().normalized();
    if !settings.is_configured() {
        return Err("EmailNotConfigured".to_string());
    }
    let to = to.trim().to_string();
    tokio::task::spawn_blocking(move || {
        let from = sender_mailbox(&settings)?;
        let email = Message::builder()
            .from(from)
            .to(to.parse().map_err(|e| format!("invalid recipient: {e}"))?)
            .subject(message.subject)
            .multipart(
                MultiPart::alternative()
                    .singlepart(SinglePart::plain(message.text))
                    .singlepart(SinglePart::html(message.html)),
            )
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

fn sender_mailbox(settings: &EmailSettings) -> Result<Mailbox, String> {
    let settings = settings.clone().normalized();
    let address: Address = settings
        .from
        .parse()
        .map_err(|e| format!("invalid sender: {e}"))?;
    if settings.from_name.chars().any(char::is_control) {
        return Err("invalid sender name".to_string());
    }
    let name = (!settings.from_name.is_empty()).then_some(settings.from_name);
    Ok(Mailbox::new(name, address))
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

    #[tokio::test]
    async fn account_counter_accepts_only_one_use_of_each_totp_step() {
        let db = smtp_test_db().await;
        let backend = db.get_database_backend();
        let schema = Schema::new(backend);
        for table in [
            schema.create_table_from_entity(user::Entity),
            schema.create_table_from_entity(::entity::totp_counter::Entity),
        ] {
            db.execute(backend.build(&table)).await.unwrap();
        }
        let temporary = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.security.totp_key_file = temporary.path().join("totp.key").display().to_string();
        let seed = generate_totp_secret();
        let vault = crate::support::totp_vault::TotpVault::load(&config, true).unwrap();
        let encrypted = vault.seal(42, &seed).unwrap();
        let account = user::ActiveModel {
            id: Set(42),
            status: Set(user::STATUS_ENABLE),
            tfa_enabled: Set(true),
            tfa_secret: Set(encrypted),
            ..Default::default()
        }.insert(&db).await.unwrap();

        let step = Utc::now().timestamp() / TOTP_PERIOD;
        let bytes = base32_decode(&seed).unwrap();
        let previous = totp_at(&bytes, step - 1);
        let current = totp_at(&bytes, step);
        let (a, b) = tokio::join!(
            consume_totp(&db, &config, &account, &previous),
            consume_totp(&db, &config, &account, &previous),
        );
        assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
        consume_totp(&db, &config, &account, &current).await.unwrap();
        assert!(consume_totp(&db, &config, &account, &previous).await.is_err());
        assert!(consume_totp(&db, &config, &account, &current).await.is_err());
        let last = ::entity::totp_counter::Entity::find_by_id(42).one(&db).await.unwrap().unwrap();
        assert_eq!(last.last_step, step);
    }

    #[tokio::test]
    async fn login_challenges_are_single_attempt_even_when_code_is_wrong() {
        let db = smtp_test_db().await;
        let schema = Schema::new(db.get_database_backend());
        for table in [
            schema.create_table_from_entity(user::Entity),
            schema.create_table_from_entity(login_verification::Entity),
        ] {
            db.execute(db.get_database_backend().build(&table))
                .await
                .unwrap();
        }
        let account = user::ActiveModel {
            username: Set("mfa-user".into()),
            status: Set(1),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        login_verification::ActiveModel {
            user_id: Set(account.id),
            secret: Set("challenge".into()),
            kind: Set(login_verification::KIND_EMAIL.into()),
            code_hash: Set(hash_code("challenge", "123456")),
            expires_at: Set(Utc::now().timestamp() + 60),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert!(verify_login_challenge(
            &db,
            &crate::config::Config::default(),
            "challenge",
            Some("wrong"),
            None,
            ChallengeSources::All
        )
        .await
        .is_err());
        assert!(verify_login_challenge(
            &db,
            &crate::config::Config::default(),
            "challenge",
            Some("123456"),
            None,
            ChallengeSources::All
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn fresh_login_challenges_share_a_persistent_account_quota() {
        let db = smtp_test_db().await;
        let schema = Schema::new(db.get_database_backend());
        for table in [
            schema.create_table_from_entity(user::Entity),
            schema.create_table_from_entity(login_verification::Entity),
        ] {
            db.execute(db.get_database_backend().build(&table))
                .await
                .unwrap();
        }
        let account = user::ActiveModel {
            username: Set("quota-user".into()),
            status: Set(1),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let mut device = DeviceLoginInfo {
            id: "device".into(),
            uuid: "uuid".into(),
            name: String::new(),
            os: String::new(),
            client_type: "app".into(),
            ip: "192.0.2.1".into(),
            auto_login: false,
        };
        for index in 0..5 {
            device.ip = format!("192.0.2.{index}");
            let challenge =
                create_login_challenge(&db, &account, &device, VerificationKind::Totp, "en")
                    .await
                    .unwrap();
            assert!(verify_login_challenge(
                &db,
                &crate::config::Config::default(),
                &challenge.secret,
                None,
                Some("invalid"),
                ChallengeSources::All
            )
            .await
            .is_err());
        }
        assert!(
            create_login_challenge(&db, &account, &device, VerificationKind::Totp, "en")
                .await
                .is_err()
        );
    }

    #[test]
    fn base32_round_trips_secret_bytes() {
        let raw = b"12345678901234567890";
        let encoded = base32_encode(raw);
        assert_eq!(base32_decode(&encoded).unwrap(), raw);
    }

    #[tokio::test]
    async fn password_reset_hides_active_secret_and_rejects_retries_after_wrong_code() {
        let db = smtp_test_db().await;
        let backend = db.get_database_backend();
        let schema = Schema::new(backend);
        db.execute(backend.build(&schema.create_table_from_entity(user::Entity)))
            .await
            .unwrap();
        db.execute(backend.build(&schema.create_table_from_entity(login_verification::Entity)))
            .await
            .unwrap();
        db.execute(
            backend.build(&schema.create_table_from_entity(::entity::ldap_identity::Entity)),
        )
        .await
        .unwrap();
        let user = user::ActiveModel {
            username: Set("reset-user".into()),
            email: Set("reset@example.com".into()),
            status: Set(user::STATUS_ENABLE),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let challenge = login_verification::ActiveModel {
            user_id: Set(user.id),
            secret: Set("original-secret".into()),
            kind: Set(login_verification::KIND_PASSWORD_RESET.into()),
            code_hash: Set(hash_code("original-secret", "123456")),
            expires_at: Set(Utc::now().timestamp() + 60),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert_ne!(
            request_password_reset(&db, "reset-user", "other-ip", "en")
                .await
                .unwrap(),
            "original-secret"
        );
        assert!(
            confirm_password_reset(&db, "original-secret", "wrong", "new-password")
                .await
                .is_err()
        );
        assert!(
            login_verification::Entity::find_by_id(challenge.id)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .verified_at
                > 0
        );
        assert!(
            confirm_password_reset(&db, "original-secret", "123456", "new-password")
                .await
                .is_err()
        );
    }

    #[test]
    fn totp_matches_rfc_6238_sample_for_sha1_8_digits_truncated_to_current_helper() {
        let secret = b"12345678901234567890";
        assert_eq!(totp_at(secret, 1), "287082");
    }

    #[test]
    fn sender_mailbox_uses_optional_display_name() {
        let mailbox = sender_mailbox(&EmailSettings {
            from: "noreply@example.com".to_string(),
            from_name: " RustDesk Console ".to_string(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(mailbox.name.as_deref(), Some("RustDesk Console"));
        assert_eq!(mailbox.email.to_string(), "noreply@example.com");

        let mailbox = sender_mailbox(&EmailSettings {
            from: "noreply@example.com".to_string(),
            from_name: " ".to_string(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(mailbox.name, None);
        assert_eq!(mailbox.email.to_string(), "noreply@example.com");
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
                from_name: "RustDesk Console".to_string(),
                tls: "starttls".to_string(),
                clear_password: false,
            },
        )
        .await
        .unwrap();
        assert!(first.enabled);
        assert!(first.password_set);
        assert_eq!(first.from_name, "RustDesk Console");

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
                from_name: "Backup Console".to_string(),
                tls: "tls".to_string(),
                clear_password: false,
            },
        )
        .await
        .unwrap();
        assert!(!second.enabled);
        assert_eq!(second.from_name, "Backup Console");

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

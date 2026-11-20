use ::entity::{control_role, deployment_event, device_admission, routing_settings};
use base64::{Engine, engine::general_purpose::STANDARD};
use sea_orm::{
    sea_query::{Expr, OnConflict},
    *,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{now, paginate};

const PENDING: i32 = 1;
const APPROVED: i32 = 2;
const REVOKED: i32 = 3;

pub fn token_is_configured(token: &str) -> bool {
    (32..=256).contains(&token.len())
        && token.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'=' | b'+' | b'/')
        })
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceKey {
    pub id: String,
    pub uuid: String,
    pub public_key: String,
}

impl DeviceKey {
    pub fn valid(&self) -> bool {
        (6..=128).contains(&self.id.len())
            && !self.id.chars().any(char::is_control)
            && self.uuid.len() <= 344
            && STANDARD
                .decode(&self.uuid)
                .is_ok_and(|bytes| !bytes.is_empty())
            && STANDARD
                .decode(&self.public_key)
                .is_ok_and(|bytes| bytes.len() == 32)
    }

    fn matches(&self, known: &device_admission::Model) -> bool {
        self.uuid == known.uuid && self.public_key == known.public_key
    }

    fn active(&self, status: i32) -> device_admission::ActiveModel {
        device_admission::ActiveModel {
            device_id: Set(self.id.clone()),
            uuid: Set(self.uuid.clone()),
            public_key: Set(self.public_key.clone()),
            status: Set(status),
            updated_at: Set(chrono::Utc::now().naive_utc()),
        }
    }
}

pub async fn required(db: &DatabaseConnection) -> Result<bool, DbErr> {
    Ok(routing_settings::Entity::find_by_id(1)
        .one(db)
        .await?
        .is_some_and(|settings| settings.require_deployment))
}

pub async fn set_required(db: &DatabaseConnection, enabled: bool) -> Result<(), DbErr> {
    routing_settings::Entity::insert(routing_settings::ActiveModel {
        id: Set(1),
        require_deployment: Set(enabled),
    })
    .on_conflict(
        OnConflict::column(routing_settings::Column::Id)
            .update_column(routing_settings::Column::RequireDeployment)
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;
    Ok(())
}

pub async fn allowed(
    db: &DatabaseConnection,
    key: &DeviceKey,
    observe: bool,
) -> Result<bool, DbErr> {
    if !required(db).await? {
        return Ok(true);
    }
    if !key.valid() {
        return Ok(false);
    }
    if let Some(known) = device_admission::Entity::find_by_id(&key.id)
        .one(db)
        .await?
    {
        return Ok(known.status == APPROVED && key.matches(&known));
    }
    // Earlier versions kept a successful deployment receipt plus the
    // deployed device's UUID and public key.
    let prior_deployment = super::peer::find_by_id(db, &key.id)
        .await?
        .is_some_and(|peer| {
            peer.status == ::entity::user::STATUS_ENABLE
                && peer.uuid == key.uuid
                && peer.pk == key.public_key
        })
        && deployment_event::Entity::find()
            .filter(deployment_event::Column::PeerId.eq(&key.id))
            .filter(deployment_event::Column::Uuid.eq(&key.uuid))
            .filter(deployment_event::Column::Action.eq(deployment_event::ACTION_DEPLOY))
            .filter(deployment_event::Column::Result.eq("OK"))
            .one(db)
            .await?
            .is_some();
    if prior_deployment || observe {
        device_admission::Entity::insert(key.active(if prior_deployment {
            APPROVED
        } else {
            PENDING
        }))
        .on_conflict(
            OnConflict::column(device_admission::Column::DeviceId)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    }
    Ok(device_admission::Entity::find_by_id(&key.id)
        .one(db)
        .await?
        .is_some_and(|row| row.status == APPROVED && key.matches(&row)))
}

pub async fn record_deployment(db: &DatabaseConnection, key: &DeviceKey) -> Result<(), DbErr> {
    if !key.valid() {
        return Ok(());
    }
    let tx = db.begin().await?;
    device_admission::Entity::insert(key.active(APPROVED))
        .on_conflict(
            OnConflict::column(device_admission::Column::DeviceId)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(&tx)
        .await?;
    device_admission::Entity::update_many()
        .col_expr(device_admission::Column::Status, Expr::value(APPROVED))
        .col_expr(device_admission::Column::Uuid, Expr::value(&key.uuid))
        .col_expr(
            device_admission::Column::PublicKey,
            Expr::value(&key.public_key),
        )
        .col_expr(device_admission::Column::UpdatedAt, Expr::value(now()))
        .filter(device_admission::Column::DeviceId.eq(&key.id))
        .filter(device_admission::Column::Status.ne(REVOKED))
        .exec(&tx)
        .await?;
    tx.commit().await
}

#[derive(Serialize)]
pub struct AdmissionView {
    pub device_id: String,
    pub fingerprint: String,
    pub status: &'static str,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    size: u64,
    device_id: Option<String>,
) -> Result<::entity::ListResult<AdmissionView>, DbErr> {
    let (page, size) = paginate(page, size);
    let mut query = device_admission::Entity::find();
    if let Some(id) = device_id.filter(|id| !id.trim().is_empty()) {
        query = query.filter(device_admission::Column::DeviceId.contains(id.trim()));
    }
    let paginated = query
        .order_by_desc(device_admission::Column::UpdatedAt)
        .paginate(db, size);
    let total = paginated.num_items().await?;
    let rows = paginated.fetch_page(page - 1).await?;
    let views = rows
        .into_iter()
        .map(|row| AdmissionView {
            device_id: row.device_id,
            fingerprint: STANDARD
                .decode(row.public_key)
                .map(|pk| format!("{:x}", Sha256::digest(pk)))
                .unwrap_or_default(),
            status: match row.status {
                APPROVED => "approved",
                REVOKED => "revoked",
                _ => "pending",
            },
            updated_at: row.updated_at.and_utc(),
        })
        .collect();
    Ok(::entity::ListResult::new(
        views,
        page as i64,
        size as i64,
        total as i64,
    ))
}

pub async fn review(
    db: &DatabaseConnection,
    id: &str,
    fingerprint: &str,
    approved: bool,
) -> Result<bool, DbErr> {
    let Some(row) = device_admission::Entity::find_by_id(id).one(db).await? else {
        return Ok(false);
    };
    let Ok(pk) = STANDARD.decode(&row.public_key) else {
        return Ok(false);
    };
    if format!("{:x}", Sha256::digest(pk)) != fingerprint {
        return Ok(false);
    }
    let result = device_admission::Entity::update_many()
        .col_expr(
            device_admission::Column::Status,
            Expr::value(if approved { APPROVED } else { REVOKED }),
        )
        .col_expr(device_admission::Column::UpdatedAt, Expr::value(now()))
        .filter(device_admission::Column::DeviceId.eq(id))
        .filter(device_admission::Column::PublicKey.eq(row.public_key))
        .filter(device_admission::Column::Uuid.eq(row.uuid))
        .exec(db)
        .await?;
    Ok(result.rows_affected == 1)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRole {
    pub deny_keyboard: bool,
    pub deny_clipboard: bool,
    pub deny_file: bool,
}

impl ControlRole {
    pub fn from_row(row: control_role::Model) -> Self {
        Self {
            deny_keyboard: row.deny_keyboard,
            deny_clipboard: row.deny_clipboard,
            deny_file: row.deny_file,
        }
    }

    pub fn mask(self) -> u64 {
        // RustDesk uses two bits per permission. Only the value 1 (deny)
        // is permitted here. Keyboard, clipboard, and file occupy 0, 2, 3.
        u64::from(self.deny_keyboard)
            | (u64::from(self.deny_clipboard) << 4)
            | (u64::from(self.deny_file) << 6)
    }
}

pub async fn control_role(db: &DatabaseConnection, user_id: i32) -> Result<ControlRole, DbErr> {
    Ok(control_role::Entity::find_by_id(user_id)
        .one(db)
        .await?
        .map(ControlRole::from_row)
        .unwrap_or_default())
}

pub async fn save_control_role(
    db: &DatabaseConnection,
    user_id: i32,
    role: ControlRole,
) -> Result<(), DbErr> {
    if role.mask() == 0 {
        control_role::Entity::delete_by_id(user_id).exec(db).await?;
        return Ok(());
    }
    control_role::Entity::insert(control_role::ActiveModel {
        user_id: Set(user_id),
        deny_keyboard: Set(role.deny_keyboard),
        deny_clipboard: Set(role.deny_clipboard),
        deny_file: Set(role.deny_file),
    })
    .on_conflict(
        OnConflict::column(control_role::Column::UserId)
            .update_columns([
                control_role::Column::DenyKeyboard,
                control_role::Column::DenyClipboard,
                control_role::Column::DenyFile,
            ])
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;
    Ok(())
}

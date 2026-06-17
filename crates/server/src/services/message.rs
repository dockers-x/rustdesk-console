use std::collections::HashSet;

use sea_orm::prelude::DateTime;
use sea_orm::*;
use serde::Serialize;

use ::entity::{message, message_read, user};

use crate::services::{now, paginate};

const MAX_TITLE_LEN: usize = 120;
const MAX_BODY_LEN: usize = 5000;

pub struct MessageListResult {
    pub list: Vec<MessagePayload>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessagePayload {
    pub id: i32,
    pub sender_id: i32,
    pub sender_name: String,
    pub recipient_id: i32,
    pub recipient_name: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub status: i32,
    pub is_read: bool,
    #[serde(serialize_with = "::entity::datetime::serialize_opt")]
    pub created_at: Option<DateTime>,
    #[serde(serialize_with = "::entity::datetime::serialize_opt")]
    pub updated_at: Option<DateTime>,
}

pub struct CreateInput {
    pub sender: user::Model,
    pub kind: String,
    pub recipient_id: i32,
    pub title: String,
    pub body: String,
    pub status: i32,
    pub admin_mode: bool,
}

pub async fn admin_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    kind: Option<String>,
) -> Result<MessageListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = message::Entity::find();
    if let Some(kind) = kind.and_then(|v| normalize_kind(&v)) {
        q = q.filter(message::Column::Kind.eq(kind));
    }
    let total = q.clone().count(db).await? as i64;
    let rows = q
        .order_by_desc(message::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(MessageListResult {
        list: rows.into_iter().map(|row| payload(row, false)).collect(),
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn user_list(
    db: &DatabaseConnection,
    user_id: i32,
    page: u64,
    page_size: u64,
    folder: Option<String>,
) -> Result<MessageListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let folder = folder.unwrap_or_default();
    let mut q = message::Entity::find().filter(message::Column::Status.eq(message::STATUS_ENABLE));
    q = match folder.as_str() {
        "sent" => q.filter(message::Column::SenderId.eq(user_id)),
        "private" => q.filter(
            Condition::any()
                .add(message::Column::SenderId.eq(user_id))
                .add(message::Column::RecipientId.eq(user_id)),
        ),
        "announcements" => q.filter(message::Column::Kind.eq(message::KIND_ANNOUNCEMENT)),
        _ => q.filter(visible_to_user(user_id)),
    };
    let deleted_ids = deleted_message_ids(db, user_id).await?;
    if !deleted_ids.is_empty() {
        q = q.filter(message::Column::Id.is_not_in(deleted_ids));
    }
    let total = q.clone().count(db).await? as i64;
    let rows = q
        .order_by_desc(message::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    let read_ids = read_message_ids(db, user_id, rows.iter().map(|row| row.id)).await?;
    Ok(MessageListResult {
        list: rows
            .into_iter()
            .map(|row| {
                let id = row.id;
                payload(row, read_ids.contains(&id))
            })
            .collect(),
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn latest_for_user(
    db: &DatabaseConnection,
    user_id: i32,
    limit: u64,
) -> Result<Vec<MessagePayload>, DbErr> {
    let deleted_ids = deleted_message_ids(db, user_id).await?;
    let mut query = message::Entity::find()
        .filter(message::Column::Status.eq(message::STATUS_ENABLE))
        .filter(visible_to_user(user_id));
    if !deleted_ids.is_empty() {
        query = query.filter(message::Column::Id.is_not_in(deleted_ids));
    }
    let rows = query
        .order_by_desc(message::Column::Id)
        .limit(limit.max(1).min(10))
        .all(db)
        .await?;
    let read_ids = read_message_ids(db, user_id, rows.iter().map(|row| row.id)).await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let id = row.id;
            payload(row, read_ids.contains(&id))
        })
        .collect())
}

pub async fn unread_count(db: &DatabaseConnection, user_id: i32) -> Result<u64, DbErr> {
    let deleted_ids = deleted_message_ids(db, user_id).await?;
    let mut query = message::Entity::find()
        .select_only()
        .column(message::Column::Id)
        .filter(message::Column::Status.eq(message::STATUS_ENABLE))
        .filter(visible_to_user(user_id));
    if !deleted_ids.is_empty() {
        query = query.filter(message::Column::Id.is_not_in(deleted_ids));
    }
    let ids: Vec<i32> = query.into_tuple().all(db).await?;
    if ids.is_empty() {
        return Ok(0);
    }
    let read_ids = read_message_ids(db, user_id, ids.iter().copied()).await?;
    Ok(ids.into_iter().filter(|id| !read_ids.contains(id)).count() as u64)
}

pub async fn info_by_id(db: &DatabaseConnection, id: i32) -> Result<Option<message::Model>, DbErr> {
    message::Entity::find_by_id(id).one(db).await
}

pub fn is_visible_to_user(row: &message::Model, user_id: i32) -> bool {
    row.status == message::STATUS_ENABLE
        && (row.kind == message::KIND_ANNOUNCEMENT
            || row.kind == message::KIND_BROADCAST
            || row.sender_id == user_id
            || row.recipient_id == user_id)
}

pub async fn create(db: &DatabaseConnection, input: CreateInput) -> Result<message::Model, DbErr> {
    let kind = normalize_kind(&input.kind)
        .ok_or_else(|| DbErr::Custom("invalid message kind".to_string()))?;
    if !input.admin_mode && kind != message::KIND_PRIVATE {
        return Err(DbErr::Custom(
            "only admins can send this message type".to_string(),
        ));
    }
    let title = input.title.trim();
    let body = input.body.trim();
    if title.is_empty() || body.is_empty() {
        return Err(DbErr::Custom(
            "message title and body are required".to_string(),
        ));
    }
    if title.chars().count() > MAX_TITLE_LEN {
        return Err(DbErr::Custom("message title is too long".to_string()));
    }
    if body.chars().count() > MAX_BODY_LEN {
        return Err(DbErr::Custom("message body is too long".to_string()));
    }

    let (recipient_id, recipient_name) = if kind == message::KIND_PRIVATE {
        if input.recipient_id <= 0 || input.recipient_id == input.sender.id {
            return Err(DbErr::Custom("valid recipient is required".to_string()));
        }
        let recipient = user::Entity::find_by_id(input.recipient_id)
            .one(db)
            .await?
            .ok_or_else(|| DbErr::Custom("recipient not found".to_string()))?;
        if !recipient.is_enabled() {
            return Err(DbErr::Custom("recipient is disabled".to_string()));
        }
        (recipient.id, display_name(&recipient))
    } else {
        (0, String::new())
    };

    let status = if input.admin_mode && input.status == message::STATUS_DISABLED {
        message::STATUS_DISABLED
    } else {
        message::STATUS_ENABLE
    };

    message::ActiveModel {
        sender_id: Set(input.sender.id),
        sender_name: Set(display_name(&input.sender)),
        recipient_id: Set(recipient_id),
        recipient_name: Set(recipient_name),
        kind: Set(kind.to_string()),
        title: Set(title.to_string()),
        body: Set(body.to_string()),
        status: Set(status),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    }
    .insert(db)
    .await
}

pub async fn set_status(
    db: &DatabaseConnection,
    row: message::Model,
    status: i32,
) -> Result<message::Model, DbErr> {
    let mut am: message::ActiveModel = row.into();
    am.status = Set(if status == message::STATUS_DISABLED {
        message::STATUS_DISABLED
    } else {
        message::STATUS_ENABLE
    });
    am.updated_at = Set(now());
    am.update(db).await
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    message_read::Entity::delete_many()
        .filter(message_read::Column::MessageId.eq(id))
        .exec(db)
        .await?;
    message::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn mark_read(
    db: &DatabaseConnection,
    message_id: i32,
    user_id: i32,
) -> Result<(), DbErr> {
    let exists = message_read::Entity::find()
        .filter(message_read::Column::MessageId.eq(message_id))
        .filter(message_read::Column::UserId.eq(user_id))
        .one(db)
        .await?;
    if let Some(row) = exists {
        if row.read_at.is_some() {
            return Ok(());
        }
        let mut am: message_read::ActiveModel = row.into();
        am.read_at = Set(now());
        am.updated_at = Set(now());
        am.update(db).await?;
        return Ok(());
    }
    message_read::ActiveModel {
        message_id: Set(message_id),
        user_id: Set(user_id),
        read_at: Set(now()),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

pub async fn mark_deleted(
    db: &DatabaseConnection,
    message_id: i32,
    user_id: i32,
) -> Result<(), DbErr> {
    let exists = message_read::Entity::find()
        .filter(message_read::Column::MessageId.eq(message_id))
        .filter(message_read::Column::UserId.eq(user_id))
        .one(db)
        .await?;
    if let Some(row) = exists {
        let already_read = row.read_at.is_some();
        let mut am: message_read::ActiveModel = row.into();
        if !already_read {
            am.read_at = Set(now());
        }
        am.deleted_at = Set(now());
        am.updated_at = Set(now());
        am.update(db).await?;
        return Ok(());
    }
    message_read::ActiveModel {
        message_id: Set(message_id),
        user_id: Set(user_id),
        read_at: Set(now()),
        deleted_at: Set(now()),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

fn visible_to_user(user_id: i32) -> Condition {
    Condition::any()
        .add(message::Column::Kind.eq(message::KIND_ANNOUNCEMENT))
        .add(message::Column::Kind.eq(message::KIND_BROADCAST))
        .add(message::Column::SenderId.eq(user_id))
        .add(message::Column::RecipientId.eq(user_id))
}

async fn read_message_ids<I>(
    db: &DatabaseConnection,
    user_id: i32,
    ids: I,
) -> Result<HashSet<i32>, DbErr>
where
    I: IntoIterator<Item = i32>,
{
    let ids: Vec<i32> = ids.into_iter().collect();
    if ids.is_empty() {
        return Ok(HashSet::new());
    }
    let rows = message_read::Entity::find()
        .filter(message_read::Column::UserId.eq(user_id))
        .filter(message_read::Column::MessageId.is_in(ids))
        .filter(message_read::Column::ReadAt.is_not_null())
        .all(db)
        .await?;
    Ok(rows.into_iter().map(|row| row.message_id).collect())
}

async fn deleted_message_ids(db: &DatabaseConnection, user_id: i32) -> Result<Vec<i32>, DbErr> {
    message_read::Entity::find()
        .select_only()
        .column(message_read::Column::MessageId)
        .filter(message_read::Column::UserId.eq(user_id))
        .filter(message_read::Column::DeletedAt.is_not_null())
        .into_tuple()
        .all(db)
        .await
}

fn payload(row: message::Model, is_read: bool) -> MessagePayload {
    MessagePayload {
        id: row.id,
        sender_id: row.sender_id,
        sender_name: row.sender_name,
        recipient_id: row.recipient_id,
        recipient_name: row.recipient_name,
        kind: row.kind,
        title: row.title,
        body: row.body,
        status: row.status,
        is_read,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn normalize_kind(kind: &str) -> Option<&'static str> {
    match kind.trim() {
        message::KIND_ANNOUNCEMENT => Some(message::KIND_ANNOUNCEMENT),
        message::KIND_BROADCAST => Some(message::KIND_BROADCAST),
        message::KIND_PRIVATE => Some(message::KIND_PRIVATE),
        _ => None,
    }
}

fn display_name(user: &user::Model) -> String {
    if !user.nickname.trim().is_empty() {
        return user.nickname.trim().to_string();
    }
    user.username.trim().to_string()
}

//! Editable notifications use typed variables and render to a snapshot at enqueue time.

use std::borrow::Cow;
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use ::entity::{notification_template as template, notification_template_binding as binding};

use super::{now, paginate};

const TITLE_LIMIT: usize = 256;
const BODY_LIMIT: usize = 4000;
const MAX_PARTS: usize = 128;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Variable {
    DeviceId,
    DeviceName,
    Hostname,
    Platform,
    Username,
    DeviceGroup,
    EventType,
    State,
    PreviousState,
    OccurredAt,
    LastSeenAt,
}

const VARIABLES: [Variable; 11] = [
    Variable::DeviceId,
    Variable::DeviceName,
    Variable::Hostname,
    Variable::Platform,
    Variable::Username,
    Variable::DeviceGroup,
    Variable::EventType,
    Variable::State,
    Variable::PreviousState,
    Variable::OccurredAt,
    Variable::LastSeenAt,
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text { text: String },
    Variable { name: Variable },
}

#[derive(Debug, Deserialize)]
pub struct Input {
    #[serde(default)]
    pub id: i32,
    pub name: String,
    pub title: Vec<Part>,
    pub body: Vec<Part>,
}

#[derive(Debug, Serialize)]
pub struct View {
    pub id: i32,
    pub name: String,
    pub title: Vec<Part>,
    pub body: Vec<Part>,
}

#[derive(Debug, Serialize)]
pub struct Page {
    list: Vec<View>,
    total: u64,
    page: u64,
    page_size: u64,
    variables: [Variable; 11],
}

impl TryFrom<template::Model> for View {
    type Error = DbErr;

    fn try_from(row: template::Model) -> Result<Self, Self::Error> {
        let decode = |value| {
            serde_json::from_value(value)
                .map_err(|_| DbErr::Custom("Invalid stored notification template".into()))
        };
        Ok(Self {
            id: row.id,
            name: row.name,
            title: decode(row.title)?,
            body: decode(row.body)?,
        })
    }
}

pub async fn list(db: &DatabaseConnection, page: u64, page_size: u64) -> Result<Page, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let query = template::Entity::find();
    let total = query.clone().count(db).await?;
    let rows = query
        .order_by_asc(template::Column::Name)
        .order_by_asc(template::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(Page {
        list: rows
            .into_iter()
            .map(View::try_from)
            .collect::<Result<_, _>>()?,
        total,
        page,
        page_size,
        variables: VARIABLES,
    })
}

pub async fn save(db: &DatabaseConnection, input: Input) -> Result<View, String> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 255 {
        return Err("Enter a template name between 1 and 255 characters".into());
    }
    validate_parts(&input.title, TITLE_LIMIT)?;
    validate_parts(&input.body, BODY_LIMIT)?;
    let title = serde_json::to_value(input.title).map_err(|_| "Invalid template title")?;
    let body = serde_json::to_value(input.body).map_err(|_| "Invalid template body")?;
    let mut active = template::ActiveModel {
        name: Set(name.to_owned()),
        title: Set(title),
        body: Set(body),
        updated_at: Set(now()),
        ..Default::default()
    };
    let row = if input.id > 0 {
        active.id = Set(input.id);
        active.update(db).await
    } else {
        active.created_at = Set(now());
        active.insert(db).await
    }
    .map_err(|_| "Failed to save notification template".to_owned())?;
    View::try_from(row).map_err(|_| "Invalid stored notification template".into())
}

fn validate_parts(parts: &[Part], limit: usize) -> Result<(), String> {
    if parts.len() > MAX_PARTS {
        return Err(format!("Use at most {MAX_PARTS} parts per template field"));
    }
    if !parts.iter().any(|part| match part {
        Part::Text { text } => !text.trim().is_empty(),
        Part::Variable { .. } => true,
    }) {
        return Err("Add text or a variable to each template field".into());
    }
    let text_size = parts
        .iter()
        .map(|part| match part {
            Part::Text { text } => text.chars().count(),
            Part::Variable { .. } => 0,
        })
        .sum::<usize>();
    if text_size > limit {
        return Err(format!("Use at most {limit} text characters in this field"));
    }
    Ok(())
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), String> {
    let bindings = binding::Entity::find()
        .filter(binding::Column::TemplateId.eq(id))
        .count(db)
        .await
        .map_err(|_| "Failed to check template usage")?;
    if bindings != 0 {
        return Err("This template is linked to a notification rule".into());
    }
    let deleted = template::Entity::delete_by_id(id)
        .exec(db)
        .await
        .map_err(|error| match error.sql_err() {
            Some(SqlErr::ForeignKeyConstraintViolation(_)) => {
                "This template is linked to a notification rule"
            }
            _ => "Failed to delete notification template",
        })?;
    if deleted.rows_affected != 1 {
        return Err("Notification template not found".into());
    }
    Ok(())
}

/// An omitted template field preserves a legacy API client's existing selection.
pub(super) async fn set_binding(
    db: &DatabaseTransaction,
    subscription_id: i32,
    selected: Option<i32>,
) -> Result<(), String> {
    let Some(selected) = selected else {
        return Ok(());
    };
    binding::Entity::delete_by_id(subscription_id)
        .exec(db)
        .await
        .map_err(|_| "Failed to update template selection")?;
    if selected != 0 {
        binding::ActiveModel {
            subscription_id: Set(subscription_id),
            template_id: Set(selected),
        }
        .insert(db)
        .await
        .map_err(|_| "Selected notification template is unavailable")?;
    }
    Ok(())
}

pub(super) async fn load(db: &impl ConnectionTrait, id: i32) -> Result<Option<View>, DbErr> {
    if id <= 0 {
        return Ok(None);
    }
    template::Entity::find_by_id(id)
        .one(db)
        .await?
        .map(View::try_from)
        .transpose()
}

pub(super) async fn for_subscriptions(
    db: &impl ConnectionTrait,
    ids: Vec<i32>,
) -> Result<HashMap<i32, View>, DbErr> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = binding::Entity::find()
        .filter(binding::Column::SubscriptionId.is_in(ids))
        .find_also_related(template::Entity)
        .all(db)
        .await?;
    rows.into_iter()
        .filter_map(|(row, template)| {
            let template = template?;
            Some(View::try_from(template).map(|view| (row.subscription_id, view)))
        })
        .collect()
}

pub(super) fn rendered(template: &View, event: &Value) -> Value {
    json!({
        "title": render_field(&template.title, event, TITLE_LIMIT),
        "body": render_field(&template.body, event, BODY_LIMIT),
    })
}

fn render_field(parts: &[Part], event: &Value, mut remaining: usize) -> String {
    let mut output = String::new();
    for part in parts {
        let value = match part {
            Part::Text { text } => Cow::Borrowed(text.as_str()),
            Part::Variable { name } => value(*name, event),
        };
        for ch in value.chars().take(remaining) {
            output.push(ch);
            remaining -= 1;
        }
        if remaining == 0 {
            break;
        }
    }
    output
}

fn value(variable: Variable, event: &Value) -> Cow<'_, str> {
    let at = |path| {
        event
            .pointer(path)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let plain = match variable {
        Variable::DeviceId => at("/resource/id"),
        Variable::DeviceName => {
            let name = at("/resource/name");
            if name.is_empty() {
                at("/resource/id")
            } else {
                name
            }
        }
        Variable::Hostname => at("/resource/hostname"),
        Variable::Platform => at("/resource/platform"),
        Variable::Username => at("/resource/username"),
        Variable::DeviceGroup => at("/device/group_name"),
        Variable::EventType => at("/event_type"),
        Variable::State => at("/transition/to"),
        Variable::PreviousState => at("/transition/from"),
        Variable::OccurredAt => at("/occurred_at"),
        Variable::LastSeenAt => {
            return event
                .pointer("/transition/current_seen_at")
                .and_then(Value::as_i64)
                .filter(|timestamp| *timestamp > 0)
                .and_then(DateTime::<Utc>::from_timestamp_secs)
                .map_or(Cow::Borrowed(""), |time| Cow::Owned(time.to_rfc3339()));
        }
    };
    Cow::Borrowed(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_expands_only_selected_variables_and_copies_typed_braces() {
        let parts = [
            Part::Text {
                text: "{{device_id}} ".into(),
            },
            Part::Variable {
                name: Variable::DeviceName,
            },
            Part::Text { text: " ".into() },
            Part::Variable {
                name: Variable::State,
            },
        ];
        let dataset = json!({
            "resource": {"id":"42", "name":"{{state}}"},
            "transition": {"to":"online"},
        });
        assert_eq!(
            render_field(&parts, &dataset, 256),
            "{{device_id}} {{state}} online"
        );
        assert_eq!(
            render_field(
                &[Part::Text {
                    text: "设备在线".into()
                }],
                &dataset,
                2
            ),
            "设备"
        );
    }

    #[test]
    fn templates_reject_unknown_variables_and_excess_text() {
        assert!(
            serde_json::from_value::<Part>(json!({"type":"variable","name":"secret"})).is_err()
        );
        assert!(validate_parts(&[Part::Text { text: "".into() }], TITLE_LIMIT).is_err());
        assert!(validate_parts(
            &[Part::Text {
                text: "设备".repeat(130)
            }],
            TITLE_LIMIT
        )
        .is_err());
    }
}

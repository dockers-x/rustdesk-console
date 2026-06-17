use std::collections::HashMap;

use sea_orm::*;

use ::entity::{peer, strategy, strategy_assignment};

use crate::services::{now, paginate};

pub const DEFAULT_PRIORITY: i32 = 100;

pub const ALLOWED_CONFIG_OPTIONS: &[&str] = &[
    "enable-keyboard",
    "enable-clipboard",
    "enable-file-transfer",
    "enable-camera",
    "enable-terminal",
    "enable-audio",
    "enable-tunnel",
    "enable-remote-restart",
    "enable-record-session",
    "enable-block-input",
    "enable-privacy-mode",
    "approve-mode",
    "verification-method",
    "allow-auto-disconnect",
    "auto-disconnect-timeout",
    "whitelist",
    "allow-remote-config-modification",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveStrategy {
    pub id: i32,
    pub modified_at: i64,
    pub config_options: HashMap<String, String>,
    pub extra: HashMap<String, String>,
}

pub struct StrategyListResult {
    pub list: Vec<strategy::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    name: Option<String>,
) -> Result<StrategyListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = strategy::Entity::find();
    if let Some(v) = name.filter(|s| !s.trim().is_empty()) {
        q = q.filter(strategy::Column::Name.contains(v.trim()));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(strategy::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(StrategyListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<strategy::Model>, DbErr> {
    strategy::Entity::find_by_id(id).one(db).await
}

pub async fn info_by_guid(
    db: &DatabaseConnection,
    guid: &str,
) -> Result<Option<strategy::Model>, DbErr> {
    strategy::Entity::find()
        .filter(strategy::Column::Guid.eq(guid.trim()))
        .one(db)
        .await
}

pub async fn info_by_name(
    db: &DatabaseConnection,
    name: &str,
) -> Result<Option<strategy::Model>, DbErr> {
    strategy::Entity::find()
        .filter(strategy::Column::Name.eq(name.trim()))
        .one(db)
        .await
}

pub async fn create(
    db: &DatabaseConnection,
    name: &str,
    note: &str,
    status: i32,
    config_options: HashMap<String, String>,
    extra: HashMap<String, String>,
) -> Result<strategy::Model, DbErr> {
    let name = name.trim();
    if name.is_empty() {
        return Err(DbErr::Custom("strategy name is required".to_string()));
    }
    let config_options = encode_config_options(config_options)?;
    let extra = encode_string_map(extra)?;
    let now_ts = chrono::Utc::now().timestamp();
    let am = strategy::ActiveModel {
        guid: Set(uuid::Uuid::new_v4().to_string()),
        name: Set(name.to_string()),
        note: Set(note.trim().to_string()),
        status: Set(normalize_status(status)),
        config_options: Set(config_options),
        extra: Set(extra),
        modified_at: Set(now_ts),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await
}

pub async fn update(
    db: &DatabaseConnection,
    row: strategy::Model,
    name: &str,
    note: &str,
    status: i32,
    config_options: HashMap<String, String>,
    extra: HashMap<String, String>,
) -> Result<(), DbErr> {
    let name = name.trim();
    if name.is_empty() {
        return Err(DbErr::Custom("strategy name is required".to_string()));
    }
    let mut am: strategy::ActiveModel = row.into();
    am.name = Set(name.to_string());
    am.note = Set(note.trim().to_string());
    am.status = Set(normalize_status(status));
    am.config_options = Set(encode_config_options(config_options)?);
    am.extra = Set(encode_string_map(extra)?);
    am.modified_at = Set(chrono::Utc::now().timestamp());
    am.updated_at = Set(now());
    am.update(db).await?;
    Ok(())
}

pub async fn set_status(
    db: &DatabaseConnection,
    row: strategy::Model,
    enabled: bool,
) -> Result<(), DbErr> {
    let mut am: strategy::ActiveModel = row.into();
    am.status = Set(if enabled {
        strategy::STATUS_ENABLE
    } else {
        strategy::STATUS_DISABLED
    });
    am.modified_at = Set(chrono::Utc::now().timestamp());
    am.updated_at = Set(now());
    am.update(db).await?;
    Ok(())
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    strategy_assignment::Entity::delete_many()
        .filter(strategy_assignment::Column::StrategyId.eq(id))
        .exec(db)
        .await?;
    strategy::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn assign(
    db: &DatabaseConnection,
    strategy_id: i32,
    target_type: &str,
    target_id: &str,
    priority: i32,
) -> Result<(), DbErr> {
    let target_type = normalize_target_type(target_type)?;
    let target_id = target_id.trim();
    if target_id.is_empty() {
        return Err(DbErr::Custom("assignment target is required".to_string()));
    }
    strategy_assignment::Entity::delete_many()
        .filter(strategy_assignment::Column::TargetType.eq(target_type))
        .filter(strategy_assignment::Column::TargetId.eq(target_id))
        .exec(db)
        .await?;
    if strategy_id <= 0 {
        return Ok(());
    }
    let am = strategy_assignment::ActiveModel {
        strategy_id: Set(strategy_id),
        target_type: Set(target_type.to_string()),
        target_id: Set(target_id.to_string()),
        priority: Set(priority),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await?;
    Ok(())
}

pub async fn assign_peer_by_strategy_name(
    db: &DatabaseConnection,
    peer_id: &str,
    strategy_name: &str,
) -> Result<(), DbErr> {
    let strategy = info_by_name(db, strategy_name)
        .await?
        .ok_or_else(|| DbErr::Custom(format!("Strategy not found: {strategy_name}")))?;
    assign(
        db,
        strategy.id,
        strategy_assignment::TARGET_PEER,
        peer_id,
        DEFAULT_PRIORITY,
    )
    .await
}

pub async fn effective_for_peer(
    db: &DatabaseConnection,
    p: &peer::Model,
) -> Result<Option<EffectiveStrategy>, DbErr> {
    let user_id = p.user_id.to_string();
    let group_id = p.group_id.to_string();
    let candidates = [
        (strategy_assignment::TARGET_PEER, p.id.as_str()),
        (strategy_assignment::TARGET_USER, user_id.as_str()),
        (strategy_assignment::TARGET_DEVICE_GROUP, group_id.as_str()),
    ];
    for (target_type, target_id) in candidates {
        if target_id.is_empty() || target_id == "0" {
            continue;
        }
        if let Some(strategy) = effective_for_target(db, target_type, target_id).await? {
            return Ok(Some(strategy));
        }
    }
    Ok(None)
}

async fn effective_for_target(
    db: &DatabaseConnection,
    target_type: &str,
    target_id: &str,
) -> Result<Option<EffectiveStrategy>, DbErr> {
    let assignments = strategy_assignment::Entity::find()
        .filter(strategy_assignment::Column::TargetType.eq(target_type))
        .filter(strategy_assignment::Column::TargetId.eq(target_id))
        .order_by_asc(strategy_assignment::Column::Priority)
        .order_by_desc(strategy_assignment::Column::Id)
        .all(db)
        .await?;
    for assignment in assignments {
        if assignment.strategy_id <= 0 {
            continue;
        }
        let Some(row) = info_by_id(db, assignment.strategy_id).await? else {
            continue;
        };
        if !row.is_enabled() {
            continue;
        }
        return Ok(Some(to_effective(row)?));
    }
    Ok(None)
}

pub fn to_effective(row: strategy::Model) -> Result<EffectiveStrategy, DbErr> {
    Ok(EffectiveStrategy {
        id: row.id,
        modified_at: row.modified_at,
        config_options: decode_string_map(&row.config_options)?,
        extra: decode_string_map(&row.extra)?,
    })
}

fn normalize_status(status: i32) -> i32 {
    if status == strategy::STATUS_DISABLED {
        strategy::STATUS_DISABLED
    } else {
        strategy::STATUS_ENABLE
    }
}

fn normalize_target_type(target_type: &str) -> Result<&'static str, DbErr> {
    match target_type.trim() {
        strategy_assignment::TARGET_PEER => Ok(strategy_assignment::TARGET_PEER),
        strategy_assignment::TARGET_USER => Ok(strategy_assignment::TARGET_USER),
        strategy_assignment::TARGET_DEVICE_GROUP => Ok(strategy_assignment::TARGET_DEVICE_GROUP),
        _ => Err(DbErr::Custom("invalid strategy target type".to_string())),
    }
}

fn encode_config_options(options: HashMap<String, String>) -> Result<String, DbErr> {
    for key in options.keys() {
        if !ALLOWED_CONFIG_OPTIONS.contains(&key.as_str()) {
            return Err(DbErr::Custom(format!("unsupported strategy option: {key}")));
        }
    }
    encode_string_map(options)
}

fn encode_string_map(map: HashMap<String, String>) -> Result<String, DbErr> {
    serde_json::to_string(&map).map_err(|e| DbErr::Custom(e.to_string()))
}

fn decode_string_map(raw: &str) -> Result<HashMap<String, String>, DbErr> {
    if raw.trim().is_empty() {
        return Ok(HashMap::new());
    }
    serde_json::from_str(raw).map_err(|e| DbErr::Custom(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsupported_config_options() {
        let mut options = HashMap::new();
        options.insert("unknown-option".to_string(), "Y".to_string());
        let err = encode_config_options(options).unwrap_err();
        assert!(err.to_string().contains("unsupported strategy option"));
    }

    #[test]
    fn decodes_empty_maps() {
        assert!(decode_string_map("").unwrap().is_empty());
        assert!(decode_string_map("{}").unwrap().is_empty());
    }
}

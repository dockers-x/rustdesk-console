//! Administration of explicit cross-group connection grants.

use axum::{
    extract::{Path, State},
    response::Response,
    Json,
};
use entity::{connection_grant as grant, device_group, group};
use sea_orm::{
    sea_query::OnConflict, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter,
    QueryOrder, Set,
};
use serde::Deserialize;

use crate::{services, state::AppState};

use super::{middleware::AdminUser, response as resp, security, security_audit::AuditContext};

pub(crate) async fn list(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(user_group_id): Path<i32>,
) -> Response {
    match grant::Entity::find()
        .filter(grant::Column::UserGroupId.eq(user_group_id))
        .order_by_asc(grant::Column::DeviceGroupId)
        .all(&state.db)
        .await
    {
        Ok(grants) => resp::no_store(resp::success(grants)),
        Err(error) => database_failure(error),
    }
}

pub(crate) async fn create(
    State(state): State<AppState>,
    _admin: AdminUser,
    audit: AuditContext,
    Path((user_group_id, device_group_id)): Path<(i32, i32)>,
) -> Response {
    audit.subject(device_group_id);
    match insert(&state.db, user_group_id, device_group_id).await {
        Ok(true) => resp::success(()),
        Ok(false) => resp::fail(101, "Select existing user and device groups"),
        Err(error) => database_failure(error),
    }
}

async fn insert(
    db: &DatabaseConnection,
    user_group_id: i32,
    device_group_id: i32,
) -> Result<bool, DbErr> {
    if user_group_id <= 0 || device_group_id <= 0 {
        return Ok(false);
    }
    let transaction = services::console_roles::authority_transaction(db).await?;
    if group::Entity::find_by_id(user_group_id)
        .one(&transaction)
        .await?
        .is_none()
        || device_group::Entity::find_by_id(device_group_id)
            .one(&transaction)
            .await?
            .is_none()
    {
        transaction.rollback().await?;
        return Ok(false);
    }
    grant::Entity::insert(grant::ActiveModel {
        user_group_id: Set(user_group_id),
        device_group_id: Set(device_group_id),
    })
    .on_conflict(
        OnConflict::columns([grant::Column::UserGroupId, grant::Column::DeviceGroupId])
            .update_column(grant::Column::DeviceGroupId)
            .to_owned(),
    )
    .exec_without_returning(&transaction)
    .await?;
    transaction.commit().await?;
    Ok(true)
}

pub(crate) async fn remove(
    State(state): State<AppState>,
    _admin: AdminUser,
    audit: AuditContext,
    Path(ids): Path<(i32, i32)>,
) -> Response {
    audit.subject(ids.1);
    match grant::Entity::delete_by_id(ids).exec(&state.db).await {
        Ok(_) => resp::success(()),
        Err(error) => database_failure(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Preview {
    username: String,
    device_id: String,
}

pub(crate) async fn preview(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(form): Json<Preview>,
) -> Response {
    let result = async {
        let user = services::user::info_by_username(
            &state.db,
            &services::user::format_username(&form.username),
        )
        .await?;
        let Some(user) = user else {
            return Ok(None);
        };
        security::evaluate_connection(&state, &user, form.device_id.trim())
            .await
            .map(Some)
    }
    .await;
    match result {
        Ok(Some(reason)) => resp::no_store(resp::success(serde_json::json!({
            "allowed": reason.allowed(),
            "reason": reason,
        }))),
        Ok(None) => resp::fail(101, "User not found"),
        Err(error) => database_failure(error),
    }
}

fn database_failure(error: DbErr) -> Response {
    tracing::warn!(%error, "Connection grant operation failed");
    resp::fail(101, "Connection grant operation failed")
}

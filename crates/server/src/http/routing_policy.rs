use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;

use super::{middleware::AdminUser, response as resp, security_audit::AuditContext};
use crate::{
    services::device_admission::{self, ControlRole, DeviceKey},
    state::AppState,
};

pub(super) fn authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    let configured = &state.config.security.hbbs_admission_token;
    if !device_admission::token_is_configured(configured) {
        return false;
    }
    let Some(presented) = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| device_admission::token_is_configured(value))
    else {
        return false;
    };
    let (Ok(mut expected), Ok(mut provided)) = (
        Hmac::<Sha256>::new_from_slice(configured.as_bytes()),
        Hmac::<Sha256>::new_from_slice(presented.as_bytes()),
    ) else {
        return false;
    };
    const PROOF: &[u8] = b"rustdesk-hbbs-admission-v1";
    expected.update(PROOF);
    provided.update(PROOF);
    expected
        .verify_slice(&provided.finalize().into_bytes())
        .is_ok()
}

#[derive(Deserialize)]
pub struct DeviceCheck {
    #[serde(flatten)]
    pub key: DeviceKey,
    pub observe: bool,
}

pub async fn hbbs_device_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(check): Json<DeviceCheck>,
) -> Response {
    if !authenticated(&state, &headers) {
        return resp::unauthorized();
    }
    match device_admission::allowed(&state.db, &check.key, check.observe).await {
        Ok(allowed) => resp::no_store(Json(json!({ "allowed": allowed })).into_response()),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

pub async fn hbbs_pairing(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authenticated(&state, &headers) {
        return resp::unauthorized();
    }
    match device_admission::record_pairing(&state.db).await {
        Ok(()) => resp::no_store(Json(json!({ "allowed": true })).into_response()),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

pub async fn settings(State(state): State<AppState>, _user: AdminUser) -> Response {
    match device_admission::required(&state.db).await {
        Ok(required) => resp::success(json!({
            "require_deployment": required,
            "hbbs_connected": device_admission::ready(&state.db).await.unwrap_or(false),
            "paired_token_configured": device_admission::token_is_configured(
                &state.config.security.hbbs_admission_token
            ),
        })),
        Err(_) => resp::fail(101, "OperationFailed"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsForm {
    pub require_deployment: bool,
}

pub async fn update_settings(
    State(state): State<AppState>,
    _user: AdminUser,
    audit: AuditContext,
    Json(form): Json<SettingsForm>,
) -> Response {
    if form.require_deployment
        && !device_admission::token_is_configured(&state.config.security.hbbs_admission_token)
    {
        return resp::fail(101, "Configure the shared hbbs admission token first");
    }
    if form.require_deployment && !device_admission::ready(&state.db).await.unwrap_or(false) {
        return resp::fail(
            101,
            "Wait for a paired hbbs to connect before enabling device admission",
        );
    }
    match device_admission::set_required(&state.db, form.require_deployment).await {
        Ok(()) => {
            audit.decision("device_admission", "accepted", "admission_policy_change");
            resp::success(Value::Null)
        }
        Err(_) => resp::fail(101, "OperationFailed"),
    }
}

#[derive(Deserialize)]
pub struct AdmissionQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub device_id: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    _user: AdminUser,
    Query(query): Query<AdmissionQuery>,
) -> Response {
    match device_admission::list(
        &state.db,
        query.page.unwrap_or(1),
        query.page_size.unwrap_or(20),
        query.device_id,
    )
    .await
    {
        Ok(rows) => resp::success(rows),
        Err(_) => resp::fail(101, "OperationFailed"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewForm {
    pub device_id: String,
    pub fingerprint: String,
    pub approved: bool,
}

pub async fn review(
    State(state): State<AppState>,
    _user: AdminUser,
    audit: AuditContext,
    Json(form): Json<ReviewForm>,
) -> Response {
    match device_admission::review(&state.db, &form.device_id, &form.fingerprint, form.approved)
        .await
    {
        Ok(true) => {
            audit.decision(
                "device_admission",
                "accepted",
                if form.approved {
                    "device_approved"
                } else {
                    "device_revoked"
                },
            );
            resp::success(Value::Null)
        }
        Ok(false) => resp::fail(101, "Device record changed. Reload and try again."),
        Err(_) => resp::fail(101, "OperationFailed"),
    }
}

pub async fn role(
    State(state): State<AppState>,
    _user: AdminUser,
    Path(user_id): Path<i32>,
) -> Response {
    match device_admission::control_role(&state.db, user_id).await {
        Ok(role) => resp::success(role),
        Err(_) => resp::fail(101, "OperationFailed"),
    }
}

pub async fn update_role(
    State(state): State<AppState>,
    _user: AdminUser,
    audit: AuditContext,
    Path(user_id): Path<i32>,
    Json(form): Json<ControlRole>,
) -> Response {
    audit.subject(user_id);
    if !matches!(
        crate::services::user::info_by_id(&state.db, user_id).await,
        Ok(Some(_))
    ) {
        return resp::fail(101, "ItemNotFound");
    }
    match device_admission::save_control_role(&state.db, user_id, form).await {
        Ok(()) => {
            audit.decision("control_role", "accepted", "control_role_changed");
            resp::success(Value::Null)
        }
        Err(_) => resp::fail(101, "OperationFailed"),
    }
}

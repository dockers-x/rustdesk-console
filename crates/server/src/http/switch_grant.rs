use std::time::Duration;

use axum::{extract::State, Json};
use base64::{engine::general_purpose::STANDARD, Engine};
use rustdesk_server_control::TARGET_ID_SERVER;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::{services::server_cmd, state::AppState};

const COMMAND: &str = "register-switch-grant";
static FORWARD_SLOTS: Semaphore = Semaphore::const_new(32);

#[derive(Deserialize, Serialize)]
pub struct SignedGrant {
    id: String,
    switch_code_verifier: String,
    timestamp: String,
    signature: String,
}

#[derive(Default, Deserialize, Serialize)]
pub struct GrantResponse {
    accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_time: Option<i64>,
}

pub async fn register(
    State(state): State<AppState>,
    Json(grant): Json<SignedGrant>,
) -> Json<GrantResponse> {
    let Some(argument) = command_argument(&grant) else {
        return Json(GrantResponse::default());
    };
    let Ok(_slot) = FORWARD_SLOTS.try_acquire() else {
        return Json(GrantResponse::default());
    };
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        server_cmd::send_target_cmd(&state.config, TARGET_ID_SERVER, COMMAND, &argument),
    )
    .await;
    Json(match response {
        Ok(Ok(body)) => parse_response(&body),
        _ => GrantResponse::default(),
    })
}

fn command_argument(grant: &SignedGrant) -> Option<String> {
    if grant.id.is_empty()
        || grant.id.len() > 128
        || grant.id.contains('\0')
        || grant.switch_code_verifier.len() != 44
        || grant.signature.len() != 88
        || grant.timestamp.is_empty()
        || grant.timestamp.len() > 20
        || !grant.timestamp.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let argument = STANDARD.encode(serde_json::to_vec(grant).ok()?);
    (COMMAND.len() + 1 + argument.len() <= 1024).then_some(argument)
}

fn parse_response(body: &str) -> GrantResponse {
    let mut response = serde_json::from_str::<GrantResponse>(body).unwrap_or_default();
    if response.accepted {
        response.server_time = None;
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_typed_server_acknowledgement_accepts_registration() {
        for body in [
            "",
            "Unknown command",
            "null",
            "{}",
            r#"{"accepted":"true"}"#,
        ] {
            assert!(!parse_response(body).accepted);
        }
        assert!(parse_response(r#"{"accepted":true}"#).accepted);
        let skew = parse_response(r#"{"accepted":false,"server_time":1793232000}"#);
        assert!(!skew.accepted);
        assert_eq!(skew.server_time, Some(1793232000));
    }

    #[test]
    fn forwarding_keeps_signed_fields_intact_and_bounds_control_command() {
        let mut grant = SignedGrant {
            id: "123456789".into(),
            switch_code_verifier: STANDARD.encode([0u8; 32]),
            timestamp: "1793232000".into(),
            signature: STANDARD.encode([0u8; 64]),
        };
        let argument = command_argument(&grant).unwrap();
        let decoded: serde_json::Value =
            serde_json::from_slice(&STANDARD.decode(argument).unwrap()).unwrap();
        assert_eq!(decoded["timestamp"], grant.timestamp);
        assert_eq!(decoded["signature"], grant.signature);
        grant.id = "x".repeat(129);
        assert!(command_argument(&grant).is_none());
    }
}

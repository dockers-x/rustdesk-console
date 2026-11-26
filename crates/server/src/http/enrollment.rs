use axum::{
    extract::{DefaultBodyLimit, Path, State},
    response::Response,
    routing::{delete, post},
    Json, Router,
};
use chrono::Utc;
use entity::enrollment_invitation as invitation;
use rand::{rngs::OsRng, Rng};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set,
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{middleware::AdminUser, response as resp, security_audit::AuditContext};
use crate::{state::AppState, support::deployment_config};

const ALPHABET: &[u8; 32] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const CODE_LENGTH: usize = 12;
const LIFETIME: i64 = 10 * 60;
const DOWNLOAD_PREFIX: &str = "/client-downloads/";

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/enrollment-invitations",
            post(create).layer(DefaultBodyLimit::max(1024)),
        )
        .route("/api/admin/enrollment-invitations/:id", delete(revoke))
        .route(
            "/api/enrollment-invitations/redeem",
            post(redeem).layer(DefaultBodyLimit::max(256)),
        )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    #[serde(default)]
    download_path: String,
}

async fn create(
    State(state): State<AppState>,
    _admin: AdminUser,
    audit: AuditContext,
    Json(request): Json<CreateRequest>,
) -> Response {
    let download_path = request.download_path.trim();
    if !download_path.is_empty() && !valid_download_path(download_path) {
        return resp::fail(101, "Invalid download portal link");
    }
    if state
        .webclient_config
        .get()
        .await
        .id_server
        .trim()
        .is_empty()
    {
        return resp::fail(101, "Set the RustDesk ID server in Client settings first");
    }

    match issue(&state.db, download_path, Utc::now().timestamp()).await {
        Ok((row, code)) => {
            audit.subject(row.id);
            audit.decision("enrollment_invitation", "accepted", "invitation_created");
            resp::no_store(resp::success(json!({
                "id": row.id,
                "code": code,
                "expires_at": row.expires_at,
            })))
        }
        Err(error) => database_failure(error),
    }
}

async fn revoke(
    State(state): State<AppState>,
    _admin: AdminUser,
    audit: AuditContext,
    Path(id): Path<i32>,
) -> Response {
    audit.subject(id);
    match invitation::Entity::delete_by_id(id).exec(&state.db).await {
        Ok(_) => {
            audit.decision("enrollment_invitation", "accepted", "invitation_revoked");
            resp::success(())
        }
        Err(error) => database_failure(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedeemRequest {
    code: String,
}

async fn redeem(State(state): State<AppState>, Json(request): Json<RedeemRequest>) -> Response {
    let cfg = state.webclient_config.get().await;
    if cfg.id_server.trim().is_empty() {
        return resp::no_store(resp::fail(101, "Enrollment is temporarily unavailable"));
    }
    let now = Utc::now().timestamp();
    let row = match claim(&state.db, &request.code, now).await {
        Ok(Some(row)) => row,
        Ok(None) => return resp::no_store(resp::fail(101, "Invitation is invalid or expired")),
        Err(error) => return database_failure(error),
    };
    let config = deployment_config::build(&cfg);
    resp::no_store(resp::success(json!({
        "encoded_config": config.encoded_config,
        "mobile_config_uri": config.mobile_config_uri,
        "config_command": config.config_command,
        "download_path": row.download_path,
    })))
}

async fn issue(
    db: &DatabaseConnection,
    download_path: &str,
    now: i64,
) -> Result<(invitation::Model, String), DbErr> {
    invitation::Entity::delete_many()
        .filter(invitation::Column::ExpiresAt.lte(now))
        .exec(db)
        .await?;
    let code = (0..CODE_LENGTH)
        .map(|_| char::from(ALPHABET[OsRng.gen_range(0..ALPHABET.len())]))
        .collect::<String>();
    let row = invitation::ActiveModel {
        code_hash: Set(hash_code(&code)),
        download_path: Set(download_path.to_owned()),
        expires_at: Set(now + LIFETIME),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok((row, code))
}

async fn claim(
    db: &DatabaseConnection,
    code: &str,
    now: i64,
) -> Result<Option<invitation::Model>, DbErr> {
    let Some(code) = normalize_code(code) else {
        return Ok(None);
    };
    let hash = hash_code(&code);
    let Some(row) = invitation::Entity::find()
        .filter(invitation::Column::CodeHash.eq(&hash))
        .filter(invitation::Column::ExpiresAt.gt(now))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let affected = invitation::Entity::delete_many()
        .filter(invitation::Column::Id.eq(row.id))
        .filter(invitation::Column::CodeHash.eq(hash))
        .filter(invitation::Column::ExpiresAt.gt(now))
        .exec(db)
        .await?
        .rows_affected;
    Ok((affected == 1).then_some(row))
}

fn normalize_code(code: &str) -> Option<String> {
    let code = code
        .bytes()
        .filter(|byte| !matches!(byte, b' ' | b'-'))
        .map(|byte| byte.to_ascii_uppercase())
        .collect::<Vec<_>>();
    if code.len() == CODE_LENGTH && code.iter().all(|byte| ALPHABET.contains(byte)) {
        String::from_utf8(code).ok()
    } else {
        None
    }
}

fn hash_code(code: &str) -> String {
    format!("{:x}", Sha256::digest(code.as_bytes()))
}

fn valid_download_path(path: &str) -> bool {
    path.strip_prefix(DOWNLOAD_PREFIX).is_some_and(|token| {
        token.len() == 43
            && token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    })
}

fn database_failure(error: DbErr) -> Response {
    tracing::warn!(%error, "Enrollment invitation operation failed");
    resp::no_store(resp::fail(101, "OperationFailed"))
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Schema};

    use super::*;

    #[tokio::test]
    async fn invitation_is_single_use_even_with_concurrent_redeems() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let table = Schema::new(DbBackend::Sqlite).create_table_from_entity(invitation::Entity);
        db.execute(DbBackend::Sqlite.build(&table)).await.unwrap();
        let path = format!("{DOWNLOAD_PREFIX}{}", "Y".repeat(43));
        let (record, code) = issue(&db, &path, 100).await.unwrap();
        assert_ne!(record.code_hash, code);
        let formatted = format!("{}-{}", &code[..4], code[4..].to_ascii_lowercase());
        let (first, second) = tokio::join!(claim(&db, &code, 101), claim(&db, &formatted, 101));
        let redeemed: Vec<_> = [first.unwrap(), second.unwrap()]
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(redeemed.len(), 1);
        assert_eq!(redeemed[0].download_path, path);

        let (record, code) = issue(&db, "", 200).await.unwrap();
        assert!(claim(&db, &code, record.expires_at)
            .await
            .unwrap()
            .is_none());
        invitation::Entity::delete_by_id(record.id)
            .exec(&db)
            .await
            .unwrap();
        assert!(claim(&db, &code, 201).await.unwrap().is_none());
    }

    #[test]
    fn download_path_cannot_leave_the_same_origin_portal() {
        assert!(valid_download_path(&format!(
            "{DOWNLOAD_PREFIX}{}",
            "_".repeat(43)
        )));
        for path in [
            "//example.com",
            "/client-downloads/../admin",
            "https://example.com/client-downloads/a",
            "/client-downloads/abc",
        ] {
            assert!(!valid_download_path(path));
        }
    }
}

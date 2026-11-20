//! Console role assignment, audit list, and opt-in retention endpoints.

use axum::{
    extract::{Path, Query, State},
    response::Response,
    Json,
};
use chrono::Utc;
use serde::Deserialize;

use crate::services::{
    console_audit::{self, Retention, Search},
    console_roles::{self, ConsoleRole},
};
use crate::state::AppState;

use super::{
    middleware::{AdminUser, AuditReader},
    response as resp,
    security_audit::AuditContext,
};

pub(crate) async fn role(
    State(state): State<AppState>,
    _actor: AdminUser,
    Path(id): Path<i32>,
) -> Response {
    match console_roles::for_user(&state.db, id).await {
        Ok(role) => resp::success(role),
        Err(_) => resp::fail(101, "Unable to load Console role"),
    }
}

pub(crate) async fn assign_role(
    State(state): State<AppState>,
    _actor: AdminUser,
    audit: AuditContext,
    Path(id): Path<i32>,
    Json(role): Json<ConsoleRole>,
) -> Response {
    audit.management("roles.assign");
    audit.subject(id);
    match console_roles::assign(&state.db, id, role).await {
        Ok(()) => resp::success(()),
        Err(err) => application_error(err, "Unable to assign Console role"),
    }
}

pub(crate) async fn list(
    State(state): State<AppState>,
    _actor: AuditReader,
    Query(search): Query<Search>,
) -> Response {
    match console_audit::list(&state.db, search).await {
        Ok(list) => resp::no_store(resp::success(list)),
        Err(_) => resp::fail(101, "Unable to load Console audit records"),
    }
}

pub(crate) async fn retention(State(state): State<AppState>, _actor: AdminUser) -> Response {
    match console_audit::retention(&state.db).await {
        Ok(settings) => resp::success(settings),
        Err(_) => resp::fail(101, "Unable to load audit retention"),
    }
}

pub(crate) async fn set_retention(
    State(state): State<AppState>,
    _actor: AdminUser,
    Json(settings): Json<Retention>,
) -> Response {
    match console_audit::set_retention(&state.db, settings).await {
        Ok(()) => resp::success(()),
        Err(err) => application_error(err, "Unable to update audit retention"),
    }
}

pub(crate) async fn preview(State(state): State<AppState>, _actor: AdminUser) -> Response {
    match console_audit::preview(&state.db, Utc::now()).await {
        Ok(result) => resp::success(result),
        Err(_) => resp::fail(101, "Unable to preview audit retention"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cleanup {
    confirm: bool,
}

pub(crate) async fn cleanup(
    State(state): State<AppState>,
    _actor: AdminUser,
    Json(input): Json<Cleanup>,
) -> Response {
    if !input.confirm {
        return resp::fail(101, "Cleanup requires confirmation");
    }
    match console_audit::cleanup(&state.db, Utc::now()).await {
        Ok(result) => resp::success(result),
        Err(err) => application_error(err, "Unable to clean up audit records"),
    }
}

fn application_error(err: sea_orm::DbErr, fallback: &str) -> Response {
    match err {
        sea_orm::DbErr::Custom(message) | sea_orm::DbErr::RecordNotFound(message) => {
            resp::fail(101, message)
        }
        _ => resp::fail(101, fallback),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
        Router,
    };
    use chrono::{Duration, NaiveDate};
    use entity::{
        active_connection, audit_conn, audit_file, console_audit as rows, device_group, login_log,
        peer, record_file, system_setting, user, user_token,
    };
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, ConnectionTrait, Database, DatabaseConnection, DbBackend,
        EntityTrait, PaginatorTrait, QueryFilter, Schema, Set,
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    async fn schema(db: &DatabaseConnection) {
        let schema = Schema::new(DbBackend::Sqlite);
        for statement in [
            schema.create_table_from_entity(user::Entity),
            schema.create_table_from_entity(user_token::Entity),
            schema.create_table_from_entity(system_setting::Entity),
            schema.create_table_from_entity(device_group::Entity),
            schema.create_table_from_entity(peer::Entity),
            schema.create_table_from_entity(rows::Entity),
            schema.create_table_from_entity(login_log::Entity),
            schema.create_table_from_entity(active_connection::Entity),
            schema.create_table_from_entity(audit_conn::Entity),
            schema.create_table_from_entity(audit_file::Entity),
            schema.create_table_from_entity(record_file::Entity),
        ] {
            db.execute(DbBackend::Sqlite.build(&statement))
                .await
                .unwrap();
        }
    }

    async fn account(db: &DatabaseConnection, id: i32, admin: bool) -> user::Model {
        user::ActiveModel {
            id: Set(id),
            username: Set(format!("user{id}")),
            is_admin: Set(Some(admin)),
            status: Set(user::STATUS_ENABLE),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn session(db: &DatabaseConnection, id: i32, admin: bool) {
        user_token::ActiveModel {
            id: Set(id),
            user_id: Set(id),
            token: Set(format!("token{id}")),
            auth_level: Set(if admin {
                crate::services::user::AUTH_CONSOLE_ADMIN
            } else {
                0
            }),
            expired_at: Set(Utc::now().timestamp() + 300),
            created_at: Set(crate::services::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn call(app: &Router, method: &str, path: &str, actor: i32, body: Value) -> Value {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("api-token", format!("token{actor}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn granted_roles_read_and_mutate_only_at_their_operation_boundaries() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        schema(&db).await;
        for id in 1..=4 {
            account(&db, id, id == 1).await;
            session(&db, id, id == 1).await;
        }
        for group in 1..=3 {
            device_group::ActiveModel {
                id: Set(group),
                name: Set(format!("Group {group}")),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
            peer::ActiveModel {
                row_id: Set(group),
                id: Set(format!("peer{group}")),
                group_id: Set(group),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let app =
            crate::http::router::build(crate::http::api::deployment_tests::test_state(db.clone()));
        let audit = "/api/admin/console-audit/list";
        let peers = "/api/admin/peer/list";
        let update = "/api/admin/peer/update";
        let assign = |id| format!("/api/admin/console-role/{id}");
        assert_eq!(
            call(&app, "PUT", &assign(2), 1, json!({"role":"audit_reader"})).await["code"],
            0
        );
        assert_eq!(
            call(
                &app,
                "PUT",
                &assign(3),
                1,
                json!({"role":"device_group_operator","group_ids":[1,3]})
            )
            .await["code"],
            0
        );
        assert_ne!(
            call(&app, "PUT", &assign(1), 1, json!({"role":"audit_reader"})).await["code"],
            0
        );
        assert_ne!(
            call(
                &app,
                "PUT",
                &assign(3),
                1,
                json!({"role":"device_group_operator","group_ids":[999]})
            )
            .await["code"],
            0
        );

        for (id, marker) in [(2, "AuditReader"), (3, "DeviceGroupOperator")] {
            let current = call(&app, "GET", "/api/admin/user/current", id, Value::Null).await;
            assert_eq!(current["data"]["route_names"], json!([marker]));
        }

        for path in [
            audit,
            "/api/admin/login_log/list",
            "/api/admin/audit_conn/list",
            "/api/admin/audit_file/list",
        ] {
            assert_eq!(
                call(&app, "GET", path, 2, Value::Null).await["code"],
                0,
                "{path}"
            );
            assert_eq!(
                call(&app, "GET", path, 3, Value::Null).await["code"],
                401,
                "{path}"
            );
        }
        for path in [
            "/api/admin/login_log/delete",
            "/api/admin/audit_conn/batchDelete",
            "/api/admin/console-audit/cleanup",
            update,
        ] {
            assert_eq!(
                call(&app, "POST", path, 2, json!({"id":1,"confirm":true})).await["code"],
                401,
                "{path}"
            );
        }
        for path in [
            "/api/admin/user/list",
            "/api/admin/config/server",
            "/api/admin/overview",
            "/api/admin/record_file/list",
        ] {
            assert_eq!(
                call(&app, "GET", path, 3, Value::Null).await["code"],
                401,
                "{path}"
            );
        }
        assert_eq!(call(&app, "GET", audit, 4, Value::Null).await["code"], 401);
        assert_eq!(
            call(&app, "GET", peers, 1, Value::Null).await["data"]["total"],
            3
        );
        assert_eq!(
            call(&app, "GET", peers, 3, Value::Null).await["data"]["total"],
            2
        );
        assert_eq!(
            call(&app, "GET", &format!("{peers}?id=peer2"), 3, Value::Null).await["data"]["total"],
            0
        );
        assert_eq!(
            call(&app, "GET", "/api/admin/peer/detail/1", 3, Value::Null).await["code"],
            0
        );
        assert_ne!(
            call(&app, "GET", "/api/admin/peer/detail/2", 3, Value::Null).await["code"],
            0
        );
        for input in [
            json!({"row_id":2,"alias":"cross-group"}),
            json!({"row_id":1,"group_id":2}),
            json!({"row_id":1,"user_id":2}),
            json!({"row_id":1,"id":"replacement"}),
        ] {
            assert_eq!(call(&app, "POST", update, 3, input).await["code"], 401);
        }
        assert_eq!(
            call(
                &app,
                "POST",
                update,
                3,
                json!({"row_id":1,"alias":"secret-alias","group_id":3})
            )
            .await["code"],
            0
        );
        assert_eq!(
            call(&app, "POST", "/api/admin/peer/delete", 3, json!({"id":1})).await["code"],
            401
        );
        assert_eq!(
            peer::Entity::find_by_id(1)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .group_id,
            3
        );

        let filtered = call(
            &app,
            "GET",
            &format!("{audit}?action=devices.update&result=success&actor_id=3&subject_id=1"),
            2,
            Value::Null,
        )
        .await;
        assert_eq!(filtered["data"]["total"], 1);
        assert_eq!(filtered["data"]["list"][0]["action"], "devices.update");
        assert!(!filtered.to_string().contains("secret-alias"));
        let events = call(
            &app,
            "GET",
            &format!("{audit}?action=devices.update&actor_id=3"),
            2,
            Value::Null,
        )
        .await;
        let ids = events["data"]["list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_i64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 5);
        assert!(ids.windows(2).all(|pair| pair[0] > pair[1]));

        crate::services::group::device_group_delete(&db, 3)
            .await
            .unwrap();
        assert_eq!(
            call(&app, "GET", peers, 3, Value::Null).await["data"]["total"],
            0
        );
        device_group::ActiveModel {
            id: Set(3),
            name: Set("Recreated".to_owned()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert_eq!(
            call(&app, "GET", peers, 3, Value::Null).await["data"]["total"],
            0
        );

        assert_eq!(
            call(&app, "PUT", &assign(3), 1, json!({"role":"member"})).await["code"],
            0
        );
        assert_eq!(
            call(
                &app,
                "POST",
                update,
                3,
                json!({"row_id":1,"alias":"revoked"})
            )
            .await["code"],
            401
        );
    }

    #[tokio::test]
    async fn concurrent_deletion_and_demotion_leave_one_enabled_admin() {
        let path = std::env::temp_dir().join(format!(
            "rustdesk-console-admin-{}.db",
            uuid::Uuid::new_v4()
        ));
        let db = Database::connect(format!("sqlite://{}?mode=rwc", path.display()))
            .await
            .unwrap();
        schema(&db).await;
        let mut first = account(&db, 1, true).await;
        let second = account(&db, 2, true).await;
        account(&db, 3, true).await;
        user::Entity::update_many()
            .col_expr(
                user::Column::Status,
                sea_orm::sea_query::Expr::value(user::STATUS_DISABLED),
            )
            .filter(user::Column::Id.eq(3))
            .exec(&db)
            .await
            .unwrap();
        first.is_admin = Some(false);
        let (demotion, deletion) = tokio::join!(
            crate::services::user::update(&db, &first),
            crate::services::user::delete(&db, &second),
        );
        assert_ne!(demotion.is_ok(), deletion.is_ok());
        let enabled = user::Entity::find()
            .filter(user::Column::IsAdmin.eq(true))
            .filter(user::Column::Status.eq(user::STATUS_ENABLE))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(enabled.len(), 1);
        let enabled = &enabled[0];
        assert!(crate::services::user::delete(&db, enabled).await.is_err());
        let mut disabled = enabled.clone();
        disabled.status = 0;
        assert!(crate::services::user::update(&db, &disabled).await.is_err());
        db.close().await.unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn retention_respects_cutoff_active_references_and_batch_bound() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        schema(&db).await;
        let now = NaiveDate::from_ymd_opt(2026, 1, 30)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();
        let cutoff = now - Duration::days(7);
        let old = (cutoff - Duration::seconds(1)).naive_utc();
        assert!(
            serde_json::to_value(console_audit::preview(&db, now).await.unwrap()).unwrap()
                ["cutoff"]
                .is_null()
        );
        assert!(console_audit::cleanup(&db, now).await.is_err());
        console_audit::set_retention(&db, Retention { days: Some(7) })
            .await
            .unwrap();

        for _ in 0..101 {
            rows::ActiveModel {
                action: Set("roles.assign".to_owned()),
                result: Set("success".to_owned()),
                actor_id: Set(1),
                created_at: Set(old),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let boundary = rows::ActiveModel {
            action: Set("roles.assign".to_owned()),
            result: Set("success".to_owned()),
            actor_id: Set(1),
            created_at: Set(cutoff.naive_utc()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        user_token::ActiveModel {
            id: Set(1),
            user_id: Set(1),
            expired_at: Set(now.timestamp() + 1),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        for id in [1, 2] {
            login_log::ActiveModel {
                id: Set(id),
                user_token_id: Set(id),
                created_at: Set(Some(old)),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        active_connection::ActiveModel {
            peer_id: Set("active".to_owned()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        record_file::ActiveModel {
            peer_id: Set("recorded".to_owned()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        for (id, peer, origin, close) in [
            (1, "eligible", "", old.and_utc().timestamp()),
            (2, "active", "", old.and_utc().timestamp()),
            (3, "remote", "recorded", old.and_utc().timestamp()),
            (4, "unclosed", "", 0),
        ] {
            audit_conn::ActiveModel {
                id: Set(id),
                peer_id: Set(peer.to_owned()),
                from_peer: Set(origin.to_owned()),
                close_time: Set(close),
                updated_at: Set(Some(old)),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
            audit_file::ActiveModel {
                id: Set(id),
                peer_id: Set(peer.to_owned()),
                from_peer: Set(origin.to_owned()),
                created_at: Set(Some(old)),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let preview =
            serde_json::to_value(console_audit::preview(&db, now).await.unwrap()).unwrap();
        assert_eq!(preview["console"], 101);
        assert_eq!(preview["login"], 1);
        assert_eq!(preview["connection"], 1);
        assert_eq!(preview["file"], 1);
        let first = serde_json::to_value(console_audit::cleanup(&db, now).await.unwrap()).unwrap();
        assert_eq!(first["console"], 100);
        assert_eq!(first["login"], 1);
        assert_eq!(first["connection"], 1);
        assert_eq!(first["file"], 1);
        console_audit::cleanup(&db, now).await.unwrap();
        assert_eq!(rows::Entity::find().count(&db).await.unwrap(), 1);
        assert!(rows::Entity::find_by_id(boundary.id)
            .one(&db)
            .await
            .unwrap()
            .is_some());
        assert!(login_log::Entity::find_by_id(1)
            .one(&db)
            .await
            .unwrap()
            .is_some());
        for id in [2, 3, 4] {
            assert!(audit_conn::Entity::find_by_id(id)
                .one(&db)
                .await
                .unwrap()
                .is_some());
        }
        for id in [2, 3, 4] {
            assert!(audit_file::Entity::find_by_id(id)
                .one(&db)
                .await
                .unwrap()
                .is_some());
        }
        assert_eq!(record_file::Entity::find().count(&db).await.unwrap(), 1);
        assert_eq!(
            active_connection::Entity::find().count(&db).await.unwrap(),
            1
        );
    }
}

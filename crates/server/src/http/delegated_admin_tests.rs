use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use chrono::Utc;
use entity::{
    active_connection, address_book, address_book_collection as collection,
    address_book_collection_rule as rule, address_book_trash, audit_conn, audit_file,
    console_audit, group, login_verification, system_setting, tag, user, user_token,
};
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, Database, DatabaseConnection, DbBackend, EntityTrait,
    Schema, Set,
};
use serde_json::{json, Value};
use tower::ServiceExt;

use super::super::api::deployment_tests::test_state;

const SCOPES: &str = "/api/admin/delegated/scopes";
const ROLE: &str = "/api/admin/console-role/2";
const GROUP_USERS: &str = "/api/admin/delegated/groups/10/users";
const USER_STATUS: &str = "/api/admin/delegated/groups/10/users/status";
const COLLECTION: &str = "/api/admin/delegated/collections/10";
const ENTRIES: &str = "/api/admin/delegated/collections/10/entries";
const ENTRY: &str = "/api/admin/delegated/collections/10/entries/100";

async fn session(db: &DatabaseConnection, id: i32) {
    user_token::ActiveModel {
        id: Set(id),
        user_id: Set(id),
        token: Set(format!("token{id}")),
        auth_level: Set(if id == 1 {
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

async fn share(db: &DatabaseConnection, id: i32, collection_id: i32, owner: i32) {
    rule::ActiveModel {
        id: Set(id),
        user_id: Set(owner),
        collection_id: Set(collection_id),
        rule: Set(rule::RULE_READ),
        r#type: Set(rule::RULE_TYPE_GROUP),
        to_id: Set(0),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

async fn setup() -> (DatabaseConnection, Router) {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let schema = Schema::new(DbBackend::Sqlite);
    for table in [
        schema.create_table_from_entity(user::Entity),
        schema.create_table_from_entity(user_token::Entity),
        schema.create_table_from_entity(group::Entity),
        schema.create_table_from_entity(collection::Entity),
        schema.create_table_from_entity(rule::Entity),
        schema.create_table_from_entity(address_book::Entity),
        schema.create_table_from_entity(address_book_trash::Entity),
        schema.create_table_from_entity(tag::Entity),
        schema.create_table_from_entity(console_audit::Entity),
        schema.create_table_from_entity(login_verification::Entity),
        schema.create_table_from_entity(system_setting::Entity),
        schema.create_table_from_entity(audit_conn::Entity),
        schema.create_table_from_entity(audit_file::Entity),
        schema.create_table_from_entity(active_connection::Entity),
    ] {
        db.execute(DbBackend::Sqlite.build(&table)).await.unwrap();
    }
    for id in 1..=6 {
        user::ActiveModel {
            id: Set(id),
            username: Set(format!("user{id}")),
            group_id: Set(if id == 4 { 20 } else { 10 }),
            is_admin: Set(Some(id == 1)),
            status: Set(user::STATUS_ENABLE),
            password: Set("never returned".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        session(&db, id).await;
    }
    for id in [10, 20] {
        group::ActiveModel {
            id: Set(id),
            name: Set(format!("Group {id}")),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }
    for (id, owner) in [(10, 1), (20, 1), (30, 5)] {
        collection::ActiveModel {
            id: Set(id),
            user_id: Set(owner),
            name: Set(format!("Book {id}")),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }
    share(&db, 10, 10, 1).await;
    share(&db, 20, 20, 1).await;
    share(&db, 30, 30, 1).await; // Rule owner does not match the collection.
    for (row_id, collection_id, owner) in [(100, 10, 1), (200, 20, 1), (300, 10, 4), (400, 0, 1)] {
        address_book::ActiveModel {
            row_id: Set(row_id),
            id: Set(row_id.to_string()),
            collection_id: Set(collection_id),
            user_id: Set(owner),
            password: Set("never returned".into()),
            tags: Set(json!([])),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }
    let app = crate::http::router::build(test_state(db.clone()));
    (db, app)
}

#[tokio::test]
async fn audit_exports_grant_the_same_access_as_lists() {
    let (db, app) = setup().await;
    let (_, body) = request(
        &app,
        "PUT",
        "/api/admin/console-role/6",
        1,
        json!({"role":"audit_reader"}),
    )
    .await;
    assert_eq!(body["code"], 0);
    user_token::Entity::delete_by_id(6).exec(&db).await.unwrap();
    session(&db, 6).await;
    for kind in ["audit_conn", "audit_file"] {
        for path in ["list", "export"] {
            let denied = format!("/api/admin/{kind}/{path}");
            let (_, body) = request(&app, "GET", &denied, 3, Value::Null).await;
            assert_eq!(body["code"], 401, "{denied}: {body}");
        }
        let list = format!("/api/admin/{kind}/list");
        let (_, body) = request(&app, "GET", &list, 6, Value::Null).await;
        assert_eq!(body["code"], 0, "{list}: {body}");
        for actor in [1, 6] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/admin/{kind}/export"))
                        .header("api-token", format!("token{actor}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let (parts, body) = response.into_parts();
            let csv = to_bytes(body, 1024).await.unwrap();
            assert_eq!(parts.status, StatusCode::OK);
            assert_eq!(
                parts.headers["content-type"],
                "text/csv; charset=utf-8",
                "{kind} for actor {actor}: {}",
                String::from_utf8_lossy(&csv)
            );
            assert!(parts.headers["content-disposition"]
                .to_str()
                .unwrap()
                .ends_with(".csv"));
            assert_eq!(parts.headers["cache-control"], "no-store");
            assert!(csv.starts_with("\u{feff}\"id\",\"peer_id\"".as_bytes()));
        }
    }
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    actor: i32,
    input: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("api-token", format!("token{actor}"))
                .header("content-type", "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn assign(app: &Router, db: &DatabaseConnection) {
    let (_, body) = request(
        app,
        "PUT",
        ROLE,
        1,
        json!({
            "role": "scoped_administrator", "group_ids":[10,10], "collection_ids":[10],
        }),
    )
    .await;
    assert_eq!(body["code"], 0, "{body}");
    assert!(user_token::Entity::find_by_id(2)
        .one(db)
        .await
        .unwrap()
        .is_none());
    session(db, 2).await;
}

#[tokio::test]
async fn delegated_routes_filter_lists_and_check_every_target_before_status_updates() {
    let (db, app) = setup().await;
    let (_, body) = request(&app, "GET", SCOPES, 3, Value::Null).await;
    assert_eq!(body["code"], 401);
    assign(&app, &db).await;
    let (_, body) = request(
        &app,
        "PUT",
        "/api/admin/console-role/6",
        1,
        json!({"role":"audit_reader"}),
    )
    .await;
    assert_eq!(body["code"], 0);
    let (_, body) = request(&app, "GET", "/api/admin/user/current", 2, Value::Null).await;
    assert_eq!(body["data"]["route_names"], json!(["ScopedAdministrator"]));
    let (_, body) = request(&app, "GET", SCOPES, 2, Value::Null).await;
    assert_eq!(body["data"]["groups"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["groups"][0]["id"], 10);
    assert_eq!(body["data"]["collections"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["collections"][0]["id"], 10);

    let (_, body) = request(&app, "GET", GROUP_USERS, 2, Value::Null).await;
    assert_eq!(body["code"], 0, "{body}");
    assert_eq!(body["data"]["total"], 2);
    assert_eq!(body["data"]["list"][0]["id"], 3);
    assert_eq!(body["data"]["list"][1]["id"], 5);
    assert!(body["data"]["list"][0].get("password").is_none());
    for path in [
        "/api/admin/delegated/groups/20/users",
        "/api/admin/delegation-collections",
        "/api/admin/user/list",
        "/api/admin/console-audit/list",
    ] {
        let (_, body) = request(&app, "GET", path, 2, Value::Null).await;
        assert_eq!(body["code"], 401, "{path}: {body}");
    }
    for path in [
        ROLE,
        "/api/admin/user/changePwd",
        "/api/admin/user/tfa/reset",
    ] {
        let method = if path == ROLE { "PUT" } else { "POST" };
        let (_, body) = request(&app, method, path, 2, json!({})).await;
        assert_eq!(body["code"], 401, "{path}: {body}");
    }

    for forbidden in [1, 2, 4, 6, 999] {
        let (_, body) = request(
            &app,
            "PUT",
            USER_STATUS,
            2,
            json!({"user_ids":[3,forbidden],"enabled":false}),
        )
        .await;
        assert_eq!(body["code"], 401, "{body}");
        assert_eq!(
            user::Entity::find_by_id(3)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .status,
            1
        );
        assert!(user_token::Entity::find_by_id(3)
            .one(&db)
            .await
            .unwrap()
            .is_some());
    }
    let (_, body) = request(
        &app,
        "PUT",
        USER_STATUS,
        2,
        json!({"user_ids":[3,5],"enabled":false}),
    )
    .await;
    assert_eq!(body["code"], 0, "{body}");
    for id in [3, 5] {
        assert_eq!(
            user::Entity::find_by_id(id)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .status,
            2
        );
        assert!(user_token::Entity::find_by_id(id)
            .one(&db)
            .await
            .unwrap()
            .is_none());
    }
    let (_, body) = request(
        &app,
        "PUT",
        USER_STATUS,
        2,
        json!({"user_ids":[3,5],"enabled":true}),
    )
    .await;
    assert_eq!(body["code"], 0, "{body}");
}

#[tokio::test]
async fn scoped_collection_edits_exclude_private_and_cross_collection_references() {
    let (db, app) = setup().await;
    let (_, body) = request(
        &app,
        "GET",
        "/api/admin/delegation-collections",
        1,
        Value::Null,
    )
    .await;
    assert_eq!(body["data"]["total"], 2);
    let (_, body) = request(
        &app,
        "PUT",
        ROLE,
        1,
        json!({
            "role":"scoped_administrator","group_ids":[],"collection_ids":[30],
        }),
    )
    .await;
    assert_ne!(body["code"], 0);
    assign(&app, &db).await;
    let (_, body) = request(&app, "GET", ENTRIES, 2, Value::Null).await;
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["list"][0]["row_id"], 100);
    assert!(body["data"]["list"][0].get("password").is_none());

    for path in [
        "/api/admin/delegated/collections/20/entries/200",
        "/api/admin/delegated/collections/10/entries/200",
        "/api/admin/delegated/collections/10/entries/300",
        "/api/admin/delegated/collections/10/entries/400",
    ] {
        let (_, body) = request(&app, "PUT", path, 2, json!({"alias":"attempted","note":""})).await;
        assert_eq!(body["code"], 401, "{path}: {body}");
    }
    let (status, _) = request(
        &app,
        "PUT",
        COLLECTION,
        2,
        json!({"name":"New","note":"","user_id":2}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, body) = request(
        &app,
        "POST",
        "/api/admin/address_book_collection_rule/create",
        2,
        json!({}),
    )
    .await;
    assert_eq!(body["code"], 401);
    let (_, body) = request(
        &app,
        "PUT",
        COLLECTION,
        2,
        json!({"name":"New book","note":"shared"}),
    )
    .await;
    assert_eq!(body["code"], 0, "{body}");
    let (_, body) = request(
        &app,
        "PUT",
        ENTRY,
        2,
        json!({"alias":"New alias","note":"scoped"}),
    )
    .await;
    assert_eq!(body["code"], 0, "{body}");
    let entry = address_book::Entity::find_by_id(100)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.alias, "New alias");
    assert_eq!(entry.password, "never returned");
}

#[tokio::test]
async fn removing_roles_and_retiring_resources_invalidates_stale_grants() {
    use crate::services;

    let (db, app) = setup().await;
    assign(&app, &db).await;
    services::address_book::delete_rule(&db, 10).await.unwrap();
    share(&db, 10, 10, 1).await;
    let (_, body) = request(&app, "GET", ENTRIES, 2, Value::Null).await;
    assert_eq!(body["code"], 401);
    let (_, body) = request(&app, "GET", ROLE, 1, Value::Null).await;
    assert_eq!(body["data"]["collection_ids"], json!([]));

    let (_, body) = request(
        &app,
        "PUT",
        ROLE,
        1,
        json!({"role":"scoped_administrator","group_ids":[10],"collection_ids":[10]}),
    )
    .await;
    assert_eq!(body["code"], 0);
    session(&db, 2).await;
    let original = collection::Entity::find_by_id(10)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    services::address_book::update_collection_profile(&db, &original, 3, "Transferred", None)
        .await
        .unwrap();
    let (_, body) = request(&app, "GET", ENTRIES, 2, Value::Null).await;
    assert_eq!(body["code"], 401);

    services::group::delete(&db, 10).await.unwrap();
    group::ActiveModel {
        id: Set(10),
        name: Set("Reused ID".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let (_, body) = request(&app, "GET", GROUP_USERS, 2, Value::Null).await;
    assert_eq!(body["code"], 401);
    let (_, body) = request(&app, "GET", ROLE, 1, Value::Null).await;
    assert_eq!(body["data"]["group_ids"], json!([]));

    let (_, body) = request(&app, "PUT", ROLE, 1, json!({"role":"member"})).await;
    assert_eq!(body["code"], 0);
    let (_, body) = request(&app, "GET", SCOPES, 2, Value::Null).await;
    assert_eq!(body["code"], 403);
    session(&db, 2).await;
    let (_, body) = request(&app, "GET", SCOPES, 2, Value::Null).await;
    assert_eq!(body["code"], 401);
}

#[tokio::test]
async fn persists_full_size_scoped_assignment() {
    let (db, app) = setup().await;
    let ids = (1_000_000_000..1_000_000_050).collect::<Vec<_>>();
    for &id in &ids {
        group::ActiveModel {
            id: Set(id),
            name: Set(format!("Group {id}")),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        collection::ActiveModel {
            id: Set(id),
            user_id: Set(1),
            name: Set(format!("Collection {id}")),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        share(&db, id, id, 1).await;
    }
    let assignment = json!({
        "role":"scoped_administrator", "group_ids":ids, "collection_ids":ids,
    });
    let (_, saved) = request(&app, "PUT", ROLE, 1, assignment.clone()).await;
    assert_eq!(saved["code"], 0, "{saved}");
    let (_, loaded) = request(&app, "GET", ROLE, 1, Value::Null).await;
    assert_eq!(loaded["data"], assignment);
}

#[tokio::test]
async fn deleting_sharing_recipients_only_revokes_collections_that_lose_their_last_share() {
    use crate::services;

    for (kind, recipient) in [(rule::RULE_TYPE_GROUP, 20), (rule::RULE_TYPE_PERSONAL, 4)] {
        let (db, app) = setup().await;
        rule::ActiveModel {
            id: Set(10),
            r#type: Set(kind),
            to_id: Set(recipient),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();
        rule::ActiveModel {
            id: Set(21),
            user_id: Set(1),
            collection_id: Set(20),
            rule: Set(rule::RULE_READ),
            r#type: Set(kind),
            to_id: Set(recipient),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let (_, saved) = request(
            &app,
            "PUT",
            ROLE,
            1,
            json!({
                "role":"scoped_administrator", "group_ids":[], "collection_ids":[10,20],
            }),
        )
        .await;
        assert_eq!(saved["code"], 0, "{saved}");
        session(&db, 2).await;

        if kind == rule::RULE_TYPE_GROUP {
            services::group::delete(&db, recipient).await.unwrap();
        } else {
            let account = user::Entity::find_by_id(recipient)
                .one(&db)
                .await
                .unwrap()
                .unwrap();
            services::user::delete(&db, &account).await.unwrap();
        }
        let (_, role) = request(&app, "GET", ROLE, 1, Value::Null).await;
        assert_eq!(role["data"]["collection_ids"], json!([20]));

        // A new share must never reactivate the old delegation grant.
        share(&db, 11, 10, 1).await;
        let (_, denied) = request(&app, "GET", ENTRIES, 2, Value::Null).await;
        assert_eq!(denied["code"], 401);
        let (_, preserved) = request(
            &app,
            "GET",
            "/api/admin/delegated/collections/20/entries",
            2,
            Value::Null,
        )
        .await;
        assert_eq!(preserved["code"], 0);
    }
}

#[tokio::test]
async fn deleting_a_collection_owner_retires_the_delegation() {
    let (db, app) = setup().await;
    share(&db, 31, 30, 5).await;
    let (_, saved) = request(
        &app,
        "PUT",
        ROLE,
        1,
        json!({
            "role":"scoped_administrator", "group_ids":[], "collection_ids":[30],
        }),
    )
    .await;
    assert_eq!(saved["code"], 0, "{saved}");
    session(&db, 2).await;

    let owner = user::Entity::find_by_id(5).one(&db).await.unwrap().unwrap();
    crate::services::user::delete(&db, &owner).await.unwrap();
    let (_, role) = request(&app, "GET", ROLE, 1, Value::Null).await;
    assert_eq!(role["data"]["collection_ids"], json!([]));

    user::ActiveModel {
        id: Set(owner.id),
        username: Set("replacement-owner".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let (_, denied) = request(
        &app,
        "GET",
        "/api/admin/delegated/collections/30/entries",
        2,
        Value::Null,
    )
    .await;
    assert_eq!(denied["code"], 401);
}

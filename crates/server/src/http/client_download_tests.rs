use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use entity::{user, user_token};
use sea_orm::{Database, DatabaseConnection};
use serde_json::{json, Value};
use tower::ServiceExt;

use super::*;

const ADMIN: &str = "/api/admin/client-downloads";
const SETTINGS: &str = "/api/admin/client-downloads/settings";
const SIGNING: &str = "/api/admin/features/self-signed-client";
const SOURCE: &str = "https://packages.example.org/rustdesk&test.exe";

fn release() -> Value {
    json!({
        "version": "1.4.0",
        "packages": [{
            "platform": "windows",
            "architecture": "x86_64",
            "url": SOURCE,
            "sha256": "AB".repeat(32)
        }, {
            "platform": "macos",
            "architecture": "universal",
            "url": "https://packages.example.org/rustdesk.dmg",
            "sha256": "cd".repeat(32)
        }]
    })
}

#[test]
fn rejects_unsafe_urls_and_checks_release_metadata() {
    for url in [
        "http://packages.example.org/client",
        "//packages.example.org/client",
        "jAvAsCrIpT:alert(1)",
        "https://user:secret@packages.example.org/client",
        "https://packages.example.org/client?token=secret",
        "https://packages.example.org/client#secret",
        "https://packages.example.org/client\nanother",
    ] {
        assert!(package_url(url).is_none(), "{url}");
    }
    for (field, value, error) in [
        (
            "/version",
            json!("<script>"),
            "ClientDownloadsInvalidVersion",
        ),
        ("/packages", json!([]), "ClientDownloadsInvalidPackages"),
        (
            "/packages/0/sha256",
            json!("ab"),
            "ClientDownloadsInvalidChecksum",
        ),
    ] {
        let mut payload = release();
        *payload.pointer_mut(field).unwrap() = value;
        let release: Release = serde_json::from_value(payload).unwrap();
        assert_eq!(release.validate().err(), Some(error), "{field}");
    }
}

async fn session(db: &DatabaseConnection, id: i32, admin: bool, level: i32) {
    user::ActiveModel {
        id: Set(id),
        username: Set(format!("user{id}")),
        is_admin: Set(Some(admin)),
        tfa_enabled: Set(level == crate::services::user::AUTH_TOTP),
        status: Set(user::STATUS_ENABLE),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    user_token::ActiveModel {
        id: Set(id),
        user_id: Set(id),
        token: Set(format!("session{id}")),
        auth_level: Set(level),
        expired_at: Set(chrono::Utc::now().timestamp() + 300),
        created_at: Set(crate::services::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

async fn request(app: &Router, method: &str, path: &str, actor: i32, payload: Value) -> Response {
    let mut builder = Request::builder().method(method).uri(path);
    if actor > 0 {
        builder = builder.header("api-token", format!("session{actor}"));
    }
    app.clone()
        .oneshot(
            builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

async fn envelope(response: Response) -> Value {
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_str(&body(response).await).unwrap()
}

async fn call(app: &Router, method: &str, path: &str, actor: i32, payload: Value) -> Value {
    envelope(request(app, method, path, actor, payload).await).await
}

#[tokio::test]
async fn requires_admin_and_both_switches_and_revokes_public_links() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let mut state = crate::http::api::deployment_tests::test_state(db.clone());
    let config_path = dir.path().join("config.yaml");
    state.features = Arc::new(crate::features::FeatureStore::new(&config_path));
    crate::bootstrap::migrate_and_seed(&db, &state.config, &state.i18n, "en")
        .await
        .unwrap();
    session(&db, 1, true, crate::services::user::AUTH_TOTP).await;
    session(&db, 2, false, 0).await;
    session(&db, 3, true, crate::services::user::AUTH_ENROLLMENT).await;
    let app = crate::http::router::build(state.clone());
    let on = json!({"enabled": true});
    let off = json!({"enabled": false});

    for (method, path, payload) in [
        ("POST", ADMIN, release()),
        ("GET", ADMIN, Value::Null),
        ("PUT", SETTINGS, on.clone()),
        ("DELETE", "/api/admin/client-downloads/1", Value::Null),
    ] {
        for (actor, code) in [(0, 403), (2, 401), (3, 114)] {
            assert_eq!(
                call(&app, method, path, actor, payload.clone()).await["code"],
                code,
                "{method} {path} actor {actor}"
            );
        }
    }
    assert_eq!(
        call(&app, "PUT", SETTINGS, 1, on.clone()).await["code"],
        101
    );
    assert_eq!(
        call(&app, "PUT", SIGNING, 1, on.clone()).await["data"]["download_portal_enabled"],
        false
    );
    assert_eq!(call(&app, "POST", ADMIN, 1, release()).await["code"], 101);
    assert_eq!(call(&app, "PUT", SETTINGS, 1, on.clone()).await["code"], 0);

    let mut secret_bearing = release();
    secret_bearing["deployment_token"] = json!("deployment-secret");
    let rejected = request(&app, "POST", ADMIN, 1, secret_bearing).await;
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!body(rejected).await.contains("deployment-secret"));

    let published = request(&app, "POST", ADMIN, 1, release()).await;
    assert_eq!(published.headers()[header::CACHE_CONTROL], "no-store");
    let published = envelope(published).await;
    assert_eq!(published["code"], 0, "{published}");
    let portal = &published["data"];
    assert_eq!(portal["packages"][0]["sha256"], "ab".repeat(32));
    assert_eq!(portal["packages"][0]["architecture"], "x86_64");
    let path = portal["path"].as_str().unwrap();
    let token = path.strip_prefix(PUBLIC_PATH).unwrap();
    assert!(valid_token(token));
    let revoke_path = format!("{ADMIN}/{}", portal["id"]);
    let list = request(&app, "GET", ADMIN, 1, Value::Null).await;
    assert_eq!(list.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(&envelope(list).await["data"][0], portal);

    let page = request(&app, "GET", path, 0, Value::Null).await;
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(page.headers()[header::REFERRER_POLICY], "no-referrer");
    assert_eq!(page.headers()["x-robots-tag"], "noindex, nofollow");
    assert!(page.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .contains("default-src 'none'"));
    let html = body(page).await;
    assert!(html.contains("RustDesk client downloads"));
    assert!(html.contains(r#"href="https://packages.example.org/rustdesk&amp;test.exe""#));
    assert!(html.contains("1.4.0"));
    assert!(html.contains("Universal"));
    assert!(html.contains(&"ab".repeat(32)));
    assert!(!html.contains(token));
    assert!(!html.contains("session1"));
    let unknown = format!(
        "{PUBLIC_PATH}{}{}",
        if token.starts_with('a') { "b" } else { "a" },
        &token[1..]
    );
    assert_eq!(
        request(&app, "GET", &unknown, 0, Value::Null)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    assert_eq!(call(&app, "PUT", SETTINGS, 1, off.clone()).await["code"], 0);
    assert_eq!(
        request(&app, "GET", path, 0, Value::Null).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(call(&app, "GET", ADMIN, 1, Value::Null).await["code"], 101);
    assert_eq!(
        call(&app, "DELETE", &revoke_path, 1, Value::Null).await["code"],
        101
    );
    assert_eq!(call(&app, "PUT", SETTINGS, 1, on.clone()).await["code"], 0);
    assert_eq!(
        call(&app, "PUT", SIGNING, 1, off).await["data"]["download_portal_enabled"],
        false
    );
    assert_eq!(
        request(&app, "GET", path, 0, Value::Null).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "PUT", SIGNING, 1, on).await["data"]["download_portal_enabled"],
        true
    );

    state.features = Arc::new(crate::features::FeatureStore::new(&config_path));
    let app = crate::http::router::build(state);
    let zh_page = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header(header::ACCEPT_LANGUAGE, "zh-CN")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(zh_page.status(), StatusCode::OK);
    assert!(body(zh_page).await.contains(r#"<html lang="zh-CN">"#));
    assert_eq!(
        call(&app, "DELETE", &revoke_path, 1, Value::Null).await["code"],
        0
    );
    let page = request(&app, "GET", path, 0, Value::Null).await;
    assert_eq!(page.status(), StatusCode::NOT_FOUND);
    assert!(!body(page).await.contains(SOURCE));
    assert_eq!(
        call(&app, "GET", ADMIN, 1, Value::Null).await["data"],
        json!([])
    );
}

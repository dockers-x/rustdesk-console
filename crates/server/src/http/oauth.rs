//! OAuth/OIDC login endpoints, ports of `http/controller/api/ouath.go` and the
//! admin `Login` OAuth methods + `admin/oauth.go` bind/confirm/info. Drives the
//! authorize → callback → poll(auth-query) handshake via the in-memory cache.

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::http::api::request_public_base;
use crate::http::middleware::{AcceptLang, BackendUser, ClientIp};
use crate::http::payloads::{AdminLoginPayload, LoginRes, UserPayload};
use crate::http::response as resp;
use crate::services;
use crate::state::AppState;
use crate::support::oauth_cache::{OauthCacheItem, ACTION_BIND, ACTION_LOGIN};

#[derive(Deserialize, Default)]
pub struct DeviceInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub os: String,
    #[serde(default, rename = "type")]
    pub r#type: String,
}

#[derive(Deserialize, Default)]
pub struct OidcAuthRequest {
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default, rename = "deviceInfo")]
    pub device_info: DeviceInfo,
}

async fn begin(state: &AppState, f: &OidcAuthRequest, device_type: &str) -> Response {
    match begin_value(state, f, device_type).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

async fn begin_value(
    state: &AppState,
    f: &OidcAuthRequest,
    device_type: &str,
) -> Result<serde_json::Value, Response> {
    let (st, verifier, nonce, url) =
        match services::oauth::begin_auth(&state.config, &state.db, &f.op).await {
            Ok(v) => v,
            Err(e) => return Err(resp::error(e)),
        };
    state.oauth_cache.set(
        &st,
        OauthCacheItem {
            action: ACTION_LOGIN.into(),
            id: f.id.clone(),
            op: f.op.clone(),
            uuid: f.uuid.clone(),
            device_name: f.device_info.name.clone(),
            device_os: f.device_info.os.clone(),
            device_type: device_type.to_string(),
            verifier,
            nonce,
            ..Default::default()
        },
        5 * 60,
    );
    Ok(json!({ "code": st, "url": url }))
}

// ---------- client ----------

pub async fn oidc_auth(State(state): State<AppState>, Json(f): Json<OidcAuthRequest>) -> Response {
    begin(&state, &f, "").await
}

#[derive(Deserialize, Default)]
pub struct OidcAuthQuery {
    #[serde(default)]
    pub code: String,
}

/// Poll for completion: once the callback set `user_id`, log the device in.
/// Returns Ok((user, token)) or Err(response) (error / still-waiting).
async fn auth_query_pre(
    state: &AppState,
    lang: &str,
    ip: &str,
    code: &str,
) -> Result<(entity::user::Model, entity::user_token::Model), Response> {
    let Some(v) = state.oauth_cache.get(code) else {
        return Err(resp::error(state.tr(lang, "OauthExpired")));
    };
    if v.action != ACTION_LOGIN {
        return Err(resp::error(state.tr(lang, "ParamsError")));
    }
    if v.user_id == 0 {
        return Err(Json(json!({
            "message": "Authorization in progress, please login and bind",
            "error": "No authed oidc is found"
        }))
        .into_response());
    }
    let Some(u) = services::user::info_by_id(&state.db, v.user_id)
        .await
        .ok()
        .flatten()
    else {
        return Err(resp::error(state.tr(lang, "UserNotFound")));
    };
    if !u.is_enabled() {
        return Err(resp::error(state.tr(lang, "UserDisabled")));
    }
    state.oauth_cache.delete(code);
    let ut = services::user::login(
        &state.db,
        &state.jwt,
        &state.config,
        &u,
        services::user::LoginEvent {
            client: v.device_type.clone(),
            device_id: v.id.clone(),
            uuid: v.uuid.clone(),
            ip: ip.to_string(),
            login_type: entity::login_log::TYPE_OAUTH.to_string(),
            platform: v.device_os.clone(),
        },
    )
    .await
    .map_err(|_| resp::error(state.tr(lang, "LoginFailed")))?;
    Ok((u, ut))
}

pub async fn oidc_auth_query(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Query(q): Query<OidcAuthQuery>,
) -> Response {
    let avatar_base_url = request_public_base(&headers, &state);
    match auth_query_pre(&state, &lang, &ip, &q.code).await {
        Ok((u, ut)) => Json(LoginRes {
            r#type: "access_token".into(),
            access_token: ut.token,
            user: UserPayload::from_user_with_avatar_base(&u, &avatar_base_url),
            secret: String::new(),
            tfa_type: String::new(),
        })
        .into_response(),
        Err(r) => r,
    }
}

#[derive(Deserialize, Default)]
pub struct CallbackQuery {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub code: String,
}

fn render_template(state: &AppState, name: &str, message: &str, sub: &str) -> Response {
    let tpl = crate::assets::Resources::read_string(&format!("templates/{name}"))
        .unwrap_or_else(|| "<html><body>{{.message}}</body></html>".to_string());
    let message = json!(message).to_string().replace('<', "\\u003c");
    let html = tpl
        .replace("{{.message}}", &message)
        .replace("{{.sub_message}}", sub);
    let _ = state;
    Html(html).into_response()
}

pub async fn oauth_callback(
    State(state): State<AppState>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    if q.state.is_empty() {
        return render_template(&state, "oauth_fail.html", "ParamIsEmpty", "state");
    }
    let Some(mut cache) = state.oauth_cache.get(&q.state) else {
        return render_template(&state, "oauth_fail.html", "OauthExpired", "");
    };
    let oauth_user = match services::oauth::callback(
        &state.config,
        &state.db,
        &q.code,
        &cache.verifier,
        &cache.nonce,
        &cache.op,
    )
    .await
    {
        Ok(u) => u,
        Err(e) => return render_template(&state, "oauth_fail.html", "OauthFailed", &e),
    };
    let openid = oauth_user.open_id.clone();

    if cache.action == ACTION_BIND {
        if let Ok(Some(ut)) = services::oauth::user_third_info(&state.db, &cache.op, &openid).await
        {
            if ut.user_id > 0 {
                return render_template(&state, "oauth_fail.html", "OauthHasBindOtherUser", "");
            }
        }
        if services::user::info_by_id(&state.db, cache.user_id)
            .await
            .ok()
            .flatten()
            .is_none()
        {
            return render_template(&state, "oauth_fail.html", "ItemNotFound", "");
        }
        let oauth_type = services::oauth::info_by_op(&state.db, &cache.op)
            .await
            .ok()
            .flatten()
            .map(|o| o.oauth_type)
            .unwrap_or_else(|| cache.op.clone());
        if services::oauth::bind_user(
            &state.db,
            cache.user_id,
            &oauth_user,
            &oauth_type,
            &cache.op,
        )
        .await
        .is_err()
        {
            return render_template(&state, "oauth_fail.html", "BindFail", "");
        }
        return render_template(&state, "oauth_success.html", "BindSuccess", "");
    }

    if cache.action == ACTION_LOGIN {
        if cache.user_id != 0 {
            return render_template(&state, "oauth_fail.html", "OauthHasBeenSuccess", "");
        }
        // find existing bound user
        let mut user = None;
        if let Ok(Some(ut)) = services::oauth::user_third_info(&state.db, &cache.op, &openid).await
        {
            user = services::user::info_by_id(&state.db, ut.user_id)
                .await
                .ok()
                .flatten();
        }
        let user = match user {
            Some(u) => u,
            None => {
                let auto = services::oauth::info_by_op(&state.db, &cache.op)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|o| o.auto_register)
                    .unwrap_or(false);
                if !auto {
                    // store oauth user info and redirect to the bind page
                    cache.open_id = oauth_user.open_id.clone();
                    cache.username = oauth_user.username.clone();
                    cache.name = oauth_user.name.clone();
                    cache.email = oauth_user.email.clone();
                    state.oauth_cache.set(&q.state, cache.clone(), 5 * 60);
                    return Redirect::to(&format!("/_admin/#/oauth/bind/{}", q.state))
                        .into_response();
                }
                match services::oauth::register_or_login(&state.db, &oauth_user, &cache.op).await {
                    Ok(u) => u,
                    Err(e) => return render_template(&state, "oauth_fail.html", &e, ""),
                }
            }
        };
        cache.user_id = user.id;
        state.oauth_cache.set(&q.state, cache.clone(), 5 * 60);
        if cache.device_type == entity::login_log::CLIENT_WEB_ADMIN {
            return Redirect::to("/_admin/#/").into_response();
        }
        return render_template(&state, "oauth_success.html", "OauthSuccess", "");
    }

    render_template(&state, "oauth_fail.html", "ParamsError", "")
}

#[derive(Deserialize, Default)]
pub struct MessageParams {
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub msg: String,
}

pub async fn message(State(state): State<AppState>, Query(mp): Query<MessageParams>) -> Response {
    let mut res = String::new();
    if !mp.title.is_empty() {
        let t = state.i18n.translate(&mp.lang, &mp.title);
        res.push_str(&format!(";title={};", json!(t)));
    }
    if !mp.msg.is_empty() {
        let m = state.i18n.translate(&mp.lang, &mp.msg);
        res.push_str(&format!("msg = {};", json!(m)));
    }
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        res,
    )
        .into_response()
}

// ---------- admin ----------

pub async fn admin_oidc_auth(
    State(state): State<AppState>,
    Json(f): Json<OidcAuthRequest>,
) -> Response {
    match begin_value(&state, &f, entity::login_log::CLIENT_WEB_ADMIN).await {
        Ok(v) => resp::success(v),
        Err(r) => r,
    }
}

pub async fn admin_oidc_auth_query(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    ClientIp(ip): ClientIp,
    Query(q): Query<OidcAuthQuery>,
) -> Response {
    match auth_query_pre(&state, &lang, &ip, &q.code).await {
        Ok((u, ut)) => resp::success(AdminLoginPayload::from_user(&u, ut.token)),
        Err(r) => r,
    }
}

#[derive(Deserialize, Default)]
pub struct CodeForm {
    #[serde(default)]
    pub code: String,
}

/// Attach the current admin user to a webauth login request (≈ `Confirm`).
pub async fn admin_confirm(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<CodeForm>,
) -> Response {
    if f.code.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    let Some(mut v) = state.oauth_cache.get(&f.code) else {
        return resp::fail(101, state.tr(&lang, "OauthExpired"));
    };
    if v.action != ACTION_LOGIN || v.user_id != 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    v.user_id = user.user.id;
    state.oauth_cache.set(&f.code, v.clone(), 0);
    resp::success(v)
}

#[derive(Deserialize, Default)]
pub struct BindForm {
    #[serde(default)]
    pub op: String,
}

pub async fn admin_to_bind(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<BindForm>,
) -> Response {
    if let Ok(Some(_)) =
        services::oauth::user_third_by_user_and_op(&state.db, user.user.id, &f.op).await
    {
        return resp::fail(101, state.tr(&lang, "OauthHasBindOtherUser"));
    }
    let (st, verifier, nonce, url) =
        match services::oauth::begin_auth(&state.config, &state.db, &f.op).await {
            Ok(v) => v,
            Err(e) => return resp::error(state.tr(&lang, &e)),
        };
    state.oauth_cache.set(
        &st,
        OauthCacheItem {
            action: ACTION_BIND.into(),
            op: f.op.clone(),
            user_id: user.user.id,
            verifier,
            nonce,
            ..Default::default()
        },
        5 * 60,
    );
    resp::success(json!({ "code": st, "url": url }))
}

pub async fn admin_bind_confirm(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    user: BackendUser,
    Json(f): Json<CodeForm>,
) -> Response {
    if f.code.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    let Some(mut v) = state.oauth_cache.get(&f.code) else {
        return resp::fail(101, state.tr(&lang, "OauthExpired"));
    };
    if v.action != ACTION_LOGIN || v.open_id.is_empty() || v.user_id != 0 {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match services::oauth::user_third_info(&state.db, &v.op, &v.open_id).await {
        Ok(Some(_)) => return resp::fail(101, state.tr(&lang, "OauthHasBindOtherUser")),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(%error, "failed to look up OAuth identity before binding");
            return resp::fail(101, state.tr(&lang, "BindFail"));
        }
    }
    match services::oauth::user_third_by_user_and_op(&state.db, user.user.id, &v.op).await {
        Ok(Some(_)) => return resp::fail(101, state.tr(&lang, "OauthHasBindOtherUser")),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(%error, "failed to look up user OAuth binding");
            return resp::fail(101, state.tr(&lang, "BindFail"));
        }
    }
    let ou = services::oauth::OauthUser {
        open_id: v.open_id.clone(),
        username: v.username.clone(),
        name: v.name.clone(),
        email: v.email.clone(),
        ..Default::default()
    };
    let oauth_type = services::oauth::info_by_op(&state.db, &v.op)
        .await
        .ok()
        .flatten()
        .map(|o| o.oauth_type)
        .unwrap_or_else(|| v.op.clone());
    if services::oauth::bind_user(&state.db, user.user.id, &ou, &oauth_type, &v.op)
        .await
        .is_err()
    {
        return resp::fail(101, state.tr(&lang, "BindFail"));
    }
    v.user_id = user.user.id;
    state.oauth_cache.set(&f.code, v.clone(), 0);
    resp::success(v)
}

pub async fn admin_info(
    State(state): State<AppState>,
    AcceptLang(lang): AcceptLang,
    Query(q): Query<OidcAuthQuery>,
) -> Response {
    if q.code.is_empty() {
        return resp::fail(101, state.tr(&lang, "ParamsError"));
    }
    match state.oauth_cache.get(&q.code) {
        Some(v) => resp::success(v),
        None => resp::fail(101, state.tr(&lang, "ItemNotFound")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use sea_orm::{
        ActiveModelTrait, ConnectionTrait, Database, DatabaseConnection, DbBackend, Schema, Set,
    };
    use serde_json::Value;

    async fn response_body(response: Response) -> Value {
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn review_oauth_message_quotes_untrusted_text_as_one_js_string() {
        let state =
            crate::http::api::deployment_tests::test_state(DatabaseConnection::Disconnected);
        let text = "';alert(1);//\ntranslation";
        let response = message(
            State(state),
            Query(MessageParams {
                title: text.into(),
                msg: text.into(),
                ..Default::default()
            }),
        )
        .await;
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        let quoted = serde_json::to_string(text).unwrap();
        assert_eq!(body, format!(";title={quoted};msg = {quoted};"));
    }

    #[tokio::test]
    async fn oauth_templates_preserve_error_text_inside_the_inline_script() {
        let state =
            crate::http::api::deployment_tests::test_state(DatabaseConnection::Disconnected);
        let message = "Database error for 'name'\n</script><script>alert(1)</script>";
        for name in ["oauth_fail.html", "oauth_success.html"] {
            let response = render_template(&state, name, message, "");
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let html = String::from_utf8(body.to_vec()).unwrap();
            let literal = html
                .lines()
                .find_map(|line| line.trim().strip_prefix("var msg = "))
                .unwrap();
            assert!(!literal.contains('<'), "{name} must not close the script");
            assert_eq!(serde_json::from_str::<String>(literal).unwrap(), message);
        }
    }

    #[tokio::test]
    async fn review_oauth_rejects_wrong_actions_and_unfinished_binding() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for statement in [
            schema.create_table_from_entity(entity::user::Entity),
            schema.create_table_from_entity(entity::user_token::Entity),
            schema.create_table_from_entity(entity::user_third::Entity),
            schema.create_table_from_entity(entity::oauth::Entity),
        ] {
            db.execute(db.get_database_backend().build(&statement))
                .await
                .unwrap();
        }
        let current = entity::user::ActiveModel {
            id: Set(1),
            status: Set(entity::user::STATUS_ENABLE),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let token = entity::user_token::ActiveModel {
            user_id: Set(1),
            token: Set("token".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let state = crate::http::api::deployment_tests::test_state(db);
        state.oauth_cache.set(
            "bind",
            OauthCacheItem {
                action: ACTION_BIND.into(),
                user_id: 1,
                ..Default::default()
            },
            300,
        );
        let response = auth_query_pre(&state, "en", "ip", "bind")
            .await
            .err()
            .unwrap();
        assert_eq!(
            response_body(response).await["error"],
            state.tr("en", "ParamsError")
        );
        for item in [
            OauthCacheItem {
                action: ACTION_LOGIN.into(),
                ..Default::default()
            },
            OauthCacheItem {
                action: ACTION_BIND.into(),
                open_id: "identity".into(),
                ..Default::default()
            },
            OauthCacheItem {
                action: ACTION_LOGIN.into(),
                open_id: "identity".into(),
                user_id: 2,
                ..Default::default()
            },
        ] {
            state.oauth_cache.set("invalid", item, 300);
            let response = admin_bind_confirm(
                State(state.clone()),
                AcceptLang("en".into()),
                BackendUser {
                    user: current.clone(),
                    token: String::new(),
                    user_token: token.clone(),
                },
                Json(CodeForm {
                    code: "invalid".into(),
                }),
            )
            .await;
            assert_eq!(
                response_body(response).await["message"],
                state.tr("en", "ParamsError")
            );
        }
        state.oauth_cache.set(
            "ready",
            OauthCacheItem {
                action: ACTION_LOGIN.into(),
                open_id: "identity".into(),
                op: "oidc".into(),
                ..Default::default()
            },
            300,
        );
        for expected_code in [0, 101] {
            let response = admin_bind_confirm(
                State(state.clone()),
                AcceptLang("en".into()),
                BackendUser {
                    user: current.clone(),
                    token: String::new(),
                    user_token: token.clone(),
                },
                Json(CodeForm {
                    code: "ready".into(),
                }),
            )
            .await;
            assert_eq!(response_body(response).await["code"], expected_code);
        }
    }
}

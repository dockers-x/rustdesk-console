use async_trait::async_trait;
use axum::{
    extract::{FromRequest, FromRequestParts, Path, Query, Request, State},
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use entity::{address_book, device_group, peer, strategy, user};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DbErr, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};

use crate::{
    http::{middleware::BackendUser, response},
    services::{
        self,
        developer::{self, KeyError, Principal},
    },
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/developer/scopes", get(scopes))
        .route("/api/admin/developer/keys", get(keys).post(create_key))
        .route("/api/admin/developer/keys/:id/revoke", post(revoke_key))
        .nest("/api/v1", management_routes())
}

fn management_routes() -> Router<AppState> {
    Router::new()
        .route("/openapi.json", get(openapi))
        .route("/me", get(me))
        .route("/devices", get(devices))
        .route(
            "/devices/:id",
            get(device).patch(update_device).delete(delete_device),
        )
        .route("/users", get(users).post(create_user))
        .route(
            "/users/:id",
            get(get_user).patch(update_user).delete(delete_user),
        )
        .route(
            "/device-groups",
            get(device_groups).post(create_device_group),
        )
        .route("/strategies", get(strategies))
        .route(
            "/address-books",
            get(address_books).post(create_address_book),
        )
        .route(
            "/address-books/:id",
            axum::routing::delete(delete_address_book),
        )
        .fallback(api_not_found)
        .method_not_allowed_fallback(api_method_not_allowed)
}

async fn api_not_found() -> ApiError {
    ApiError::missing()
}
async fn api_method_not_allowed() -> ApiError {
    ApiError {
        status: StatusCode::METHOD_NOT_ALLOWED,
        code: "method_not_allowed",
        message: "Method not allowed",
    }
}

pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl ApiError {
    fn bad(message: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_request",
            message,
        }
    }
    fn forbidden() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            message: "The API key does not grant this operation",
        }
    }
    fn missing() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: "Resource not found",
        }
    }
    fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: "The operation could not be completed",
        }
    }
}

impl From<DbErr> for ApiError {
    fn from(_: DbErr) -> Self {
        Self::internal()
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error":{"code":self.code,"message":self.message}})),
        )
            .into_response()
    }
}

pub struct ApiAuth(Principal);

#[async_trait]
impl FromRequestParts<AppState> for ApiAuth {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let key = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or_default();
        developer::authenticate(&state.db, key)
            .await?
            .map(Self)
            .ok_or(ApiError {
                status: StatusCode::UNAUTHORIZED,
                code: "unauthorized",
                message: "A valid API key is required",
            })
    }
}

impl ApiAuth {
    fn require(&self, scope: &str) -> Result<(), ApiError> {
        if self.0.permits(scope) {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }
    fn owner_id(&self) -> i32 {
        self.0.owner.id
    }
}

pub struct ApiBody<T>(T);

#[async_trait]
impl<T: DeserializeOwned + Send> FromRequest<AppState> for ApiBody<T> {
    type Rejection = ApiError;
    async fn from_request(request: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(body)| Self(body))
            .map_err(|_| ApiError::bad("Invalid JSON request body"))
    }
}

pub struct RowId(i32);

#[async_trait]
impl FromRequestParts<AppState> for RowId {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Path(id) = Path::<i32>::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::bad("Invalid resource ID"))?;
        if id <= 0 {
            return Err(ApiError::bad("Resource ID must be positive"));
        }
        Ok(Self(id))
    }
}

#[derive(Deserialize)]
struct PageInput {
    page: Option<u64>,
    page_size: Option<u64>,
    id: Option<String>,
}
pub struct Page {
    page: u64,
    size: u64,
    id: Option<String>,
}

#[async_trait]
impl FromRequestParts<AppState> for Page {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Query(query) = Query::<PageInput>::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::bad("Invalid pagination"))?;
        let page = query.page.unwrap_or(1).max(1);
        let size = query.page_size.unwrap_or(30).clamp(1, 100);
        if page > i64::MAX as u64 / size {
            return Err(ApiError::bad("Page offset is too large"));
        }
        Ok(Self {
            page,
            size,
            id: query.id,
        })
    }
}

fn data(value: Value) -> Response {
    Json(json!({"data":value})).into_response()
}
fn page_data(value: Vec<Value>, total: u64, page: Page) -> Response {
    Json(json!({"data":value,"total":total,"page":page.page,"page_size":page.size})).into_response()
}
fn user_json(row: &user::Model) -> Value {
    json!({"id":row.id,"username":row.username,"email":row.email,"nickname":row.nickname,"is_admin":row.is_admin(),"status":row.status})
}
fn device_json(row: &peer::Model) -> Value {
    json!({"row_id":row.row_id,"id":row.id,"alias":row.alias,"hostname":row.hostname,"username":row.username,"os":row.os,"user_id":row.user_id,"group_id":row.group_id,"status":row.status,"last_online_time":row.last_online_time})
}
fn address_json(row: &address_book::Model) -> Value {
    json!({"row_id":row.row_id,"id":row.id,"alias":row.alias,"hostname":row.hostname,"platform":row.platform,"note":row.note})
}
fn group_json(row: &device_group::Model) -> Value {
    json!({"id":row.id,"name":row.name,"note":row.note})
}

async fn scopes(user: BackendUser) -> Response {
    response::success(
        developer::SCOPES
            .iter()
            .filter(|scope| !scope.admin_only || user.user.is_admin())
            .collect::<Vec<_>>(),
    )
}
async fn keys(State(state): State<AppState>, user: BackendUser) -> Response {
    match developer::list_keys(&state.db, user.user.id).await {
        Ok(keys) => response::success(keys),
        Err(_) => response::fail(101, "Could not list API keys"),
    }
}
async fn create_key(
    State(state): State<AppState>,
    user: BackendUser,
    body: Result<Json<developer::CreateKey>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return response::fail(101, "Invalid API key request");
    };
    let mut result = match developer::create_key(&state.db, &user.user, body).await {
        Ok(key) => response::success(key),
        Err(KeyError::Invalid(message)) => response::fail(101, message),
        Err(_) => response::fail(101, "Could not create API key"),
    };
    result.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    result
}
async fn revoke_key(
    State(state): State<AppState>,
    user: BackendUser,
    path: Result<Path<i32>, axum::extract::rejection::PathRejection>,
) -> Response {
    let Ok(Path(id)) = path else {
        return response::fail(101, "Invalid API key ID");
    };
    if id <= 0 {
        return response::fail(101, "Invalid API key ID");
    }
    match developer::revoke_key(&state.db, user.user.id, id).await {
        Ok(true) => response::success(Value::Null),
        Ok(false) => response::fail(101, "API key not found"),
        Err(_) => response::fail(101, "Could not revoke API key"),
    }
}
async fn me(auth: ApiAuth) -> Result<Response, ApiError> {
    auth.require("profile:read")?;
    Ok(data(user_json(&auth.0.owner)))
}

async fn devices(
    State(state): State<AppState>,
    auth: ApiAuth,
    page: Page,
) -> Result<Response, ApiError> {
    auth.require("devices:read")?;
    let mut query = peer::Entity::find();
    if !auth.0.owner.is_admin() {
        query = query.filter(peer::Column::UserId.eq(auth.owner_id()));
    }
    if let Some(id) = &page.id {
        query = query.filter(peer::Column::Id.eq(id));
    }
    let total = query.clone().count(&state.db).await?;
    let rows = query
        .order_by_asc(peer::Column::RowId)
        .offset((page.page - 1) * page.size)
        .limit(page.size)
        .all(&state.db)
        .await?;
    Ok(page_data(
        rows.iter().map(device_json).collect(),
        total,
        page,
    ))
}
async fn visible_device(
    state: &AppState,
    auth: &ApiAuth,
    id: i32,
) -> Result<peer::Model, ApiError> {
    let mut query = peer::Entity::find_by_id(id);
    if !auth.0.owner.is_admin() {
        query = query.filter(peer::Column::UserId.eq(auth.owner_id()));
    }
    query.one(&state.db).await?.ok_or_else(ApiError::missing)
}
async fn device(
    State(state): State<AppState>,
    auth: ApiAuth,
    RowId(id): RowId,
) -> Result<Response, ApiError> {
    auth.require("devices:read")?;
    Ok(data(device_json(&visible_device(&state, &auth, id).await?)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceUpdate {
    alias: Option<String>,
    user_id: Option<i32>,
    group_id: Option<i32>,
}
async fn update_device(
    State(state): State<AppState>,
    auth: ApiAuth,
    RowId(id): RowId,
    ApiBody(body): ApiBody<DeviceUpdate>,
) -> Result<Response, ApiError> {
    auth.require("devices:write")?;
    visible_device(&state, &auth, id).await?;
    let mut update = peer::ActiveModel {
        row_id: Set(id),
        ..Default::default()
    };
    if let Some(alias) = body.alias {
        if alias.len() > 255 {
            return Err(ApiError::bad("Alias is too long"));
        }
        update.alias = Set(alias);
    }
    if let Some(owner_id) = body.user_id {
        if owner_id < 0
            || (owner_id > 0
                && user::Entity::find_by_id(owner_id)
                    .one(&state.db)
                    .await?
                    .is_none())
        {
            return Err(ApiError::bad("Unknown user ID"));
        }
        update.user_id = Set(owner_id);
    }
    if let Some(group_id) = body.group_id {
        if group_id < 0
            || (group_id > 0
                && device_group::Entity::find_by_id(group_id)
                    .one(&state.db)
                    .await?
                    .is_none())
        {
            return Err(ApiError::bad("Unknown device group ID"));
        }
        update.group_id = Set(group_id);
    }
    services::peer::update(&state.db, update).await?;
    Ok(data(device_json(&visible_device(&state, &auth, id).await?)))
}
async fn delete_device(
    State(state): State<AppState>,
    auth: ApiAuth,
    RowId(id): RowId,
) -> Result<Response, ApiError> {
    auth.require("devices:write")?;
    let row = visible_device(&state, &auth, id).await?;
    services::peer::delete(&state.db, &row).await?;
    Ok(data(Value::Null))
}

async fn users(
    State(state): State<AppState>,
    auth: ApiAuth,
    page: Page,
) -> Result<Response, ApiError> {
    auth.require("users:read")?;
    let query = user::Entity::find();
    let total = query.clone().count(&state.db).await?;
    let rows = query
        .order_by_asc(user::Column::Id)
        .offset((page.page - 1) * page.size)
        .limit(page.size)
        .all(&state.db)
        .await?;
    Ok(page_data(rows.iter().map(user_json).collect(), total, page))
}
async fn find_user(state: &AppState, id: i32) -> Result<user::Model, ApiError> {
    user::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(ApiError::missing)
}
async fn get_user(
    State(state): State<AppState>,
    auth: ApiAuth,
    RowId(id): RowId,
) -> Result<Response, ApiError> {
    auth.require("users:read")?;
    Ok(data(user_json(&find_user(&state, id).await?)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserCreate {
    username: String,
    password: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    nickname: String,
}
async fn create_user(
    State(state): State<AppState>,
    auth: ApiAuth,
    ApiBody(body): ApiBody<UserCreate>,
) -> Result<Response, ApiError> {
    auth.require("users:write")?;
    let username = body.username.replace(' ', "").to_lowercase();
    if username.is_empty()
        || username.len() > 120
        || body.password.len() < 8
        || body.password.len() > 72
    {
        return Err(ApiError::bad(
            "Username must contain 1 to 120 bytes and password 8 to 72 bytes",
        ));
    }
    if services::user::is_username_exists(&state.db, &username).await? {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            code: "conflict",
            message: "Username already exists",
        });
    }
    let password = tokio::task::spawn_blocking(move || {
        crate::support::password::encrypt_password(&body.password)
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(|_| ApiError::internal())?;
    let row = user::ActiveModel {
        username: Set(username),
        password: Set(password),
        email: Set(body.email),
        nickname: Set(body.nickname),
        is_admin: Set(Some(false)),
        status: Set(user::STATUS_ENABLE),
        group_id: Set(1),
        created_at: Set(services::now()),
        updated_at: Set(services::now()),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(json!({"data":user_json(&row)}))).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserUpdate {
    email: Option<String>,
    nickname: Option<String>,
    status: Option<i32>,
}
async fn update_user(
    State(state): State<AppState>,
    auth: ApiAuth,
    RowId(id): RowId,
    ApiBody(body): ApiBody<UserUpdate>,
) -> Result<Response, ApiError> {
    auth.require("users:write")?;
    let mut row = find_user(&state, id).await?;
    if let Some(email) = body.email {
        row.email = email;
    }
    if let Some(nickname) = body.nickname {
        row.nickname = nickname;
    }
    if let Some(status) = body.status {
        if ![1, 2].contains(&status) {
            return Err(ApiError::bad("Status must be 1 or 2"));
        }
        row.status = status;
    }
    services::user::update(&state.db, &row)
        .await
        .map_err(|error| {
            if error == "The last admin user cannot be disabled or demoted" {
                ApiError::bad("The last administrator cannot be disabled")
            } else {
                ApiError::internal()
            }
        })?;
    Ok(data(user_json(&row)))
}
async fn delete_user(
    State(state): State<AppState>,
    auth: ApiAuth,
    RowId(id): RowId,
) -> Result<Response, ApiError> {
    auth.require("users:write")?;
    if id == auth.owner_id() {
        return Err(ApiError::bad("Cannot delete the API key owner"));
    }
    let row = find_user(&state, id).await?;
    services::user::delete(&state.db, &row)
        .await
        .map_err(|error| {
            if error == "The last admin user cannot be deleted" {
                ApiError::bad("The last administrator cannot be deleted")
            } else {
                ApiError::internal()
            }
        })?;
    Ok(data(Value::Null))
}

async fn device_groups(
    State(state): State<AppState>,
    auth: ApiAuth,
    page: Page,
) -> Result<Response, ApiError> {
    auth.require("device-groups:read")?;
    let query = device_group::Entity::find();
    let total = query.clone().count(&state.db).await?;
    let rows = query
        .order_by_asc(device_group::Column::Id)
        .offset((page.page - 1) * page.size)
        .limit(page.size)
        .all(&state.db)
        .await?;
    Ok(page_data(
        rows.iter().map(group_json).collect(),
        total,
        page,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupCreate {
    name: String,
    #[serde(default)]
    note: String,
}
async fn create_device_group(
    State(state): State<AppState>,
    auth: ApiAuth,
    ApiBody(body): ApiBody<GroupCreate>,
) -> Result<Response, ApiError> {
    auth.require("device-groups:write")?;
    if body.name.trim().is_empty() || body.name.len() > 120 {
        return Err(ApiError::bad("Name must contain 1 to 120 bytes"));
    }
    let row = device_group::ActiveModel {
        name: Set(body.name.trim().to_string()),
        note: Set(body.note),
        created_at: Set(services::now()),
        updated_at: Set(services::now()),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(json!({"data":group_json(&row)}))).into_response())
}
async fn strategies(
    State(state): State<AppState>,
    auth: ApiAuth,
    page: Page,
) -> Result<Response, ApiError> {
    auth.require("strategies:read")?;
    let query = strategy::Entity::find();
    let total = query.clone().count(&state.db).await?;
    let rows = query
        .order_by_asc(strategy::Column::Id)
        .offset((page.page - 1) * page.size)
        .limit(page.size)
        .all(&state.db)
        .await?;
    Ok(page_data(
        rows.iter()
            .map(|row| json!({"id":row.id,"name":row.name}))
            .collect(),
        total,
        page,
    ))
}
async fn address_books(
    State(state): State<AppState>,
    auth: ApiAuth,
    page: Page,
) -> Result<Response, ApiError> {
    auth.require("address-books:read")?;
    let query =
        address_book::Entity::find().filter(address_book::Column::UserId.eq(auth.owner_id()));
    let total = query.clone().count(&state.db).await?;
    let rows = query
        .order_by_asc(address_book::Column::RowId)
        .offset((page.page - 1) * page.size)
        .limit(page.size)
        .all(&state.db)
        .await?;
    Ok(page_data(
        rows.iter().map(address_json).collect(),
        total,
        page,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddressCreate {
    id: String,
    #[serde(default)]
    alias: String,
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    platform: String,
    #[serde(default)]
    note: String,
}
async fn create_address_book(
    State(state): State<AppState>,
    auth: ApiAuth,
    ApiBody(body): ApiBody<AddressCreate>,
) -> Result<Response, ApiError> {
    auth.require("address-books:write")?;
    if body.id.trim().is_empty() || body.id.len() > 255 {
        return Err(ApiError::bad("Device ID must contain 1 to 255 bytes"));
    }
    let row = address_book::ActiveModel {
        id: Set(body.id),
        alias: Set(body.alias),
        hostname: Set(body.hostname),
        platform: Set(body.platform),
        note: Set(body.note),
        user_id: Set(auth.owner_id()),
        collection_id: Set(0),
        tags: Set(json!([])),
        created_at: Set(services::now()),
        updated_at: Set(services::now()),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"data":address_json(&row)})),
    )
        .into_response())
}
async fn delete_address_book(
    State(state): State<AppState>,
    auth: ApiAuth,
    RowId(id): RowId,
) -> Result<Response, ApiError> {
    auth.require("address-books:write")?;
    let row = address_book::Entity::find_by_id(id)
        .filter(address_book::Column::UserId.eq(auth.owner_id()))
        .one(&state.db)
        .await?
        .ok_or_else(ApiError::missing)?;
    services::address_book::delete(&state.db, row.row_id).await?;
    Ok(data(Value::Null))
}

async fn openapi() -> Json<Value> {
    Json(super::developer_openapi::document())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use entity::{developer_key, user_token};
    use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Schema};
    use tower::ServiceExt;

    async fn setup() -> (Router, DatabaseConnection, String, String) {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for statement in [
            schema.create_table_from_entity(user::Entity),
            schema.create_table_from_entity(user_token::Entity),
            schema.create_table_from_entity(developer_key::Entity),
            schema.create_table_from_entity(peer::Entity),
            schema.create_table_from_entity(address_book::Entity),
            schema.create_table_from_entity(device_group::Entity),
            schema.create_table_from_entity(strategy::Entity),
        ] {
            db.execute(db.get_database_backend().build(&statement))
                .await
                .unwrap();
        }
        let admin = user::ActiveModel {
            id: Set(1),
            username: Set("admin".into()),
            is_admin: Set(Some(true)),
            status: Set(1),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let owner = user::ActiveModel {
            id: Set(2),
            username: Set("owner".into()),
            is_admin: Set(Some(false)),
            status: Set(1),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        for id in [1, 2] {
            user_token::ActiveModel {
                user_id: Set(id),
                token: Set(format!("session-{id}")),
                expired_at: Set(chrono::Utc::now().timestamp() + 3600),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        for id in [1, 2] {
            peer::ActiveModel {
                row_id: Set(id),
                id: Set(format!("device-{id}")),
                user_id: Set(id),
                uuid: Set(String::new()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let admin_key = developer::create_key(
            &db,
            &admin,
            developer::CreateKey {
                name: "admin-script".into(),
                scopes: Some(
                    developer::SCOPES
                        .iter()
                        .map(|scope| scope.id.to_string())
                        .collect(),
                ),
                expires_at: None,
            },
        )
        .await
        .unwrap()
        .key;
        let owner_key = developer::create_key(
            &db,
            &owner,
            developer::CreateKey {
                name: "owner-script".into(),
                scopes: Some(vec![
                    "profile:read".into(),
                    "devices:read".into(),
                    "address-books:read".into(),
                    "address-books:write".into(),
                ]),
                expires_at: None,
            },
        )
        .await
        .unwrap()
        .key;
        let state = crate::http::api::deployment_tests::test_state(db.clone());
        (crate::http::router::build(state), db, admin_key, owner_key)
    }

    async fn request(
        app: &Router,
        method: &str,
        path: &str,
        key: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let result = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {key}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = result.status();
        let bytes = to_bytes(result.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn keys_enforce_scopes_visibility_revocation_and_live_owner_permissions() {
        let (app, db, admin_key, owner_key) = setup().await;
        let (status, list) = request(
            &app,
            "GET",
            "/api/v1/devices?page_size=10000",
            &owner_key,
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list["total"], 1);
        assert_eq!(list["page_size"], 100);
        assert_eq!(list["data"][0]["row_id"], 2);
        assert!(list["data"][0].get("pk").is_none());
        assert_eq!(
            request(&app, "GET", "/api/v1/devices/1", &owner_key, Value::Null)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(&app, "GET", "/api/v1/users", &owner_key, Value::Null)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(
                &app,
                "PATCH",
                "/api/v1/devices/2",
                &owner_key,
                json!({"alias":"forbidden"})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(&app, "GET", "/api/v1/me", "session-2", Value::Null)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &app,
                "GET",
                "/api/v1/me",
                "rdt_not-a-developer-key",
                Value::Null
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(&app, "GET", "/api/v1/users", &admin_key, Value::Null)
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            request(&app, "POST", "/api/currentUser", &admin_key, Value::Null)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(&app, "POST", "/api/v1/devices/2", &admin_key, Value::Null)
                .await
                .1["error"]["code"],
            "method_not_allowed"
        );
        assert_eq!(
            request(
                &app,
                "GET",
                "/api/v1/unknown-route",
                &admin_key,
                Value::Null
            )
            .await
            .1["error"]["code"],
            "not_found"
        );
        user::ActiveModel {
            id: Set(1),
            is_admin: Set(Some(false)),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();
        assert_eq!(
            request(&app, "GET", "/api/v1/users", &admin_key, Value::Null)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(&app, "GET", "/api/v1/devices/2", &admin_key, Value::Null)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        for (status, must_change) in [(2, false), (1, true)] {
            user::ActiveModel {
                id: Set(2),
                status: Set(status),
                must_change_password: Set(must_change),
                ..Default::default()
            }
            .update(&db)
            .await
            .unwrap();
            assert_eq!(
                request(&app, "GET", "/api/v1/me", &owner_key, Value::Null)
                    .await
                    .0,
                StatusCode::UNAUTHORIZED
            );
        }
        user::ActiveModel {
            id: Set(2),
            status: Set(1),
            must_change_password: Set(false),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();
        let row = developer_key::Entity::find()
            .filter(developer_key::Column::OwnerId.eq(2))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let mut active: developer_key::ActiveModel = row.clone().into();
        active.expires_at = Set(1);
        active.update(&db).await.unwrap();
        assert_eq!(
            request(&app, "GET", "/api/v1/me", &owner_key, Value::Null)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        let mut active: developer_key::ActiveModel = row.into();
        active.revoked_at = Set(chrono::Utc::now().timestamp());
        active.update(&db).await.unwrap();
        assert_eq!(
            request(&app, "GET", "/api/v1/me", &owner_key, Value::Null)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        developer_key::Entity::update_many()
            .col_expr(
                developer_key::Column::RevokedAt,
                sea_orm::sea_query::Expr::value(0),
            )
            .col_expr(
                developer_key::Column::ExpiresAt,
                sea_orm::sea_query::Expr::value(chrono::Utc::now().timestamp() + 3600),
            )
            .filter(developer_key::Column::OwnerId.eq(2))
            .exec(&db)
            .await
            .unwrap();
        assert_eq!(
            request(&app, "GET", "/api/v1/me", &owner_key, Value::Null)
                .await
                .0,
            StatusCode::OK
        );
        user::Entity::delete_by_id(2).exec(&db).await.unwrap();
        assert_eq!(
            request(&app, "GET", "/api/v1/me", &owner_key, Value::Null)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn backend_key_management_is_owner_only_once_visible_and_session_only() {
        let (app, _db, _admin_key, owner_key) = setup().await;
        let result = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/developer/keys")
                    .header("api-token", "session-2")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"name":"integration"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(result.headers()[header::CACHE_CONTROL], "no-store");
        let body: Value =
            serde_json::from_slice(&to_bytes(result.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(body["code"], 0);
        let key = body["data"]["key"].as_str().unwrap();
        let id = body["data"]["metadata"]["id"].as_i64().unwrap();
        let result = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/developer/keys")
                    .header("api-token", "session-2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(result.into_body(), 65536).await.unwrap();
        assert!(!std::str::from_utf8(&bytes).unwrap().contains(key));
        assert!(!std::str::from_utf8(&bytes).unwrap().contains("token_hash"));
        for (session, expected) in [("session-1", 101), ("session-2", 0), ("session-2", 0)] {
            let result = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/admin/developer/keys/{id}/revoke"))
                        .header("api-token", session)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let body: Value =
                serde_json::from_slice(&to_bytes(result.into_body(), 65536).await.unwrap())
                    .unwrap();
            assert_eq!(body["code"], expected);
        }
        assert_eq!(
            request(&app, "GET", "/api/v1/me", key, Value::Null).await.0,
            StatusCode::UNAUTHORIZED
        );
        let result = app
            .oneshot(
                Request::builder()
                    .uri("/api/admin/developer/keys")
                    .header("api-token", &owner_key)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: Value =
            serde_json::from_slice(&to_bytes(result.into_body(), 65536).await.unwrap()).unwrap();
        assert_ne!(body["code"], 0);
    }

    #[tokio::test]
    async fn management_mutations_persist_and_report_failures_without_cross_owner_leaks() {
        let (app, db, admin_key, owner_key) = setup().await;
        let (status, created) = request(&app, "POST", "/api/v1/users", &admin_key, json!({"username":"automation","password":"test-password","email":"before@example.test"})).await;
        assert_eq!(status, StatusCode::CREATED);
        let user_id = created["data"]["id"].as_i64().unwrap();
        assert_eq!(created["data"]["is_admin"], false);
        assert!(created["data"].get("password").is_none());
        let path = format!("/api/v1/users/{user_id}");
        assert_eq!(
            request(
                &app,
                "PATCH",
                &path,
                &admin_key,
                json!({"nickname":"updated","status":2})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            request(&app, "GET", &path, &admin_key, Value::Null).await.1["data"]["nickname"],
            "updated"
        );
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/v1/users",
                &admin_key,
                json!({"username":"escalate","password":"test-password","is_admin":true})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        let (status, created) = request(
            &app,
            "POST",
            "/api/v1/device-groups",
            &admin_key,
            json!({"name":"Automation","note":"Provisioned by SDK"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let group_id = created["data"]["id"].as_i64().unwrap();
        assert_eq!(created["data"]["note"], "Provisioned by SDK");
        assert_eq!(
            request(
                &app,
                "PATCH",
                "/api/v1/devices/2",
                &admin_key,
                json!({"alias":"Managed","group_id":group_id,"user_id":user_id})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            request(&app, "GET", "/api/v1/devices/2", &admin_key, Value::Null)
                .await
                .1["data"]["alias"],
            "Managed"
        );
        assert_eq!(
            request(&app, "GET", "/api/v1/devices/2", &owner_key, Value::Null)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        let (status, created) = request(
            &app,
            "POST",
            "/api/v1/address-books",
            &owner_key,
            json!({"id":"123456789","alias":"Own device","note":"SDK entry"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let row_id = created["data"]["row_id"].as_i64().unwrap();
        assert_eq!(
            request(
                &app,
                "GET",
                "/api/v1/address-books",
                &admin_key,
                Value::Null
            )
            .await
            .1["total"],
            0
        );
        let address_path = format!("/api/v1/address-books/{row_id}");
        assert_eq!(
            request(&app, "DELETE", &address_path, &admin_key, Value::Null)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(&app, "DELETE", &address_path, &owner_key, Value::Null)
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            request(
                &app,
                "GET",
                "/api/v1/address-books",
                &owner_key,
                Value::Null
            )
            .await
            .1["total"],
            0
        );
        assert_eq!(
            request(&app, "DELETE", "/api/v1/devices/2", &admin_key, Value::Null)
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            request(&app, "DELETE", &path, &admin_key, Value::Null)
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            request(&app, "DELETE", "/api/v1/users/1", &admin_key, Value::Null)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            request(
                &app,
                "GET",
                "/api/v1/devices/not-an-id",
                &owner_key,
                Value::Null
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        db.execute_unprepared("DROP TABLE peers").await.unwrap();
        let (status, error) =
            request(&app, "GET", "/api/v1/devices", &owner_key, Value::Null).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error["error"]["code"], "internal_error");
    }
}

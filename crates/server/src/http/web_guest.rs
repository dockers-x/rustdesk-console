use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use axum::{
    extract::{ws, DefaultBodyLimit, Path, Query, State, WebSocketUpgrade},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use chrono::Utc;
use entity::{peer, user, web_guest_grant as grant, web_guest_ticket as ticket};
use futures_util::{SinkExt, StreamExt};
use jsonwebtoken::{encode, EncodingKey, Header};
use prost::Message;
use rand::{rngs::OsRng, RngCore};
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{
    net::TcpStream,
    sync::Semaphore,
    time::{interval, timeout},
};
use tokio_tungstenite::{
    client_async_with_config,
    tungstenite::{client::IntoClientRequest, protocol::WebSocketConfig, Message as WireMessage},
    WebSocketStream,
};

use super::{
    middleware::{AdminUser, ClientIp},
    response as resp,
    security_audit::AuditContext,
    web_guest_protocol::{Envelope, Punch, Relay, RequestRelay},
};
use crate::{
    services::{blocklist, device_admission, peer as peers},
    state::AppState,
};

const PROTOCOL: &str = "rustdesk-guest-v1";
const MAX_SIGNAL_MESSAGE: usize = 16 * 1024;
const MAX_RELAY_MESSAGE: usize = 32 * 1024 * 1024;
const SIGNAL_TIMEOUT: Duration = Duration::from_secs(12);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
static SIGNAL_SLOTS: Semaphore = Semaphore::const_new(16);
static RELAY_SLOTS: Semaphore = Semaphore::const_new(32);

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/web-guests",
            get(list).post(create).layer(DefaultBodyLimit::max(1024)),
        )
        .route("/api/admin/web-guests/:id", delete(revoke))
        .route(
            "/api/web-guests/connect",
            post(connect).layer(DefaultBodyLimit::max(256)),
        )
        .route("/api/web-guests/relay", get(relay))
        .route("/webguest", get(guest_index))
        .route("/webguest/", get(guest_index))
        .route("/webguest/*path", get(guest_asset))
}

fn available(state: &AppState) -> bool {
    state.config.app.web_client == 1
        && state.config.app.web_guests
        && device_admission::token_is_configured(&state.config.security.hbbs_admission_token)
}

async fn guest_index(State(state): State<AppState>) -> Response {
    if !available(&state) {
        return StatusCode::NOT_FOUND.into_response();
    }
    super::static_files::embedded_guest_asset("")
}

async fn guest_asset(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    if !available(&state) {
        return StatusCode::NOT_FOUND.into_response();
    }
    super::static_files::embedded_guest_asset(&path)
}

#[derive(Deserialize)]
struct GrantList {
    page: Option<u64>,
}

async fn list(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(query): Query<GrantList>,
) -> Response {
    if !available(&state) {
        return resp::no_store(resp::success(json!({ "available": false, "grants": [] })));
    }
    const PAGE_SIZE: u64 = 20;
    let query_builder = grant::Entity::find()
        .filter(grant::Column::ExpiresAt.gt(Utc::now().timestamp()))
        .order_by_desc(grant::Column::Id);
    let page = query.page.unwrap_or(1).clamp(1, u32::MAX as u64);
    let result = async {
        let total = query_builder.clone().count(&state.db).await?;
        let grants = query_builder
            .offset((page - 1) * PAGE_SIZE)
            .limit(PAGE_SIZE)
            .all(&state.db)
            .await?;
        Ok::<_, DbErr>((grants, total))
    }
    .await;
    match result {
        Ok((grants, total)) => resp::no_store(resp::success(json!({
            "available": true, "grants": grants, "total": total, "page_size": PAGE_SIZE,
        }))),
        Err(error) => failed(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    peer_id: String,
    duration_minutes: i64,
    max_uses: i32,
}

async fn create(
    State(state): State<AppState>,
    _admin: AdminUser,
    audit: AuditContext,
    Json(request): Json<CreateRequest>,
) -> Response {
    if !available(&state) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let peer_id = request.peer_id.trim();
    if peer_id.is_empty()
        || peer_id.len() > 128
        || !matches!(request.duration_minutes, 15 | 30 | 60 | 240 | 1440)
        || !(1..=10).contains(&request.max_uses)
    {
        return resp::fail(101, "Invalid guest session settings");
    }
    let device = match peers::find_by_id(&state.db, peer_id).await {
        Ok(Some(device))
            if device.status == user::STATUS_ENABLE
                && !device.uuid.is_empty()
                && !device.pk.is_empty() =>
        {
            device
        }
        Ok(_) => return resp::fail(101, "Choose an enabled, registered device"),
        Err(error) => return failed(error),
    };
    let now = Utc::now().timestamp();
    let result = async {
        grant::Entity::delete_many()
            .filter(grant::Column::ExpiresAt.lte(now))
            .exec(&state.db)
            .await?;
        let token = random_token();
        let row = grant::ActiveModel {
            token_hash: Set(hash(&token)),
            peer_row_id: Set(device.row_id),
            peer_id: Set(device.id),
            peer_uuid: Set(device.uuid),
            peer_key: Set(device.pk),
            expires_at: Set(now + request.duration_minutes * 60),
            max_uses: Set(request.max_uses),
            used_count: Set(0),
            ..Default::default()
        }
        .insert(&state.db)
        .await?;
        Ok::<_, DbErr>((row, token))
    }
    .await;
    match result {
        Ok((row, token)) => {
            audit.subject(row.id);
            audit.decision("web_guest", "accepted", "grant_created");
            resp::no_store(resp::success(json!({ "grant": row, "token": token })))
        }
        Err(error) => failed(error),
    }
}

async fn revoke(
    State(state): State<AppState>,
    _admin: AdminUser,
    audit: AuditContext,
    Path(id): Path<i32>,
) -> Response {
    audit.subject(id);
    match grant::Entity::delete_by_id(id).exec(&state.db).await {
        Ok(_) => {
            audit.decision("web_guest", "accepted", "grant_revoked");
            resp::success(())
        }
        Err(error) => failed(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectRequest {
    token: String,
    peer_id: String,
}

async fn connect(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    Json(request): Json<ConnectRequest>,
) -> Response {
    if !available(&state) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(token_hash) = valid_token_hash(&request.token) else {
        return denied();
    };
    let Ok(_slot) = SIGNAL_SLOTS.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let row = match grant::Entity::find()
        .filter(grant::Column::TokenHash.eq(token_hash))
        .filter(grant::Column::ExpiresAt.gt(Utc::now().timestamp()))
        .one(&state.db)
        .await
    {
        Ok(Some(row)) if row.peer_id == request.peer_id && row.used_count < row.max_uses => row,
        Ok(_) => return denied(),
        Err(error) => return failed(error),
    };
    let source = client_ip.parse().ok();
    match target_still_enabled(&state.db, &row, source).await {
        Ok(true) => {}
        Ok(false) => return denied(),
        Err(error) => return failed(error),
    }
    let key = state.webclient_config.get().await.key.trim().to_owned();
    let Ok(proof) = signal_proof(&row.peer_id, &state.config.security.hbbs_admission_token) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let punch = Punch {
        id: row.peer_id.clone(),
        nat: 2, // Symmetric NAT. The guest transport always uses the relay.
        key: key.clone(),
        token: proof,
        force_relay: true,
    };
    let Some(port) = websocket_port(state.config.admin.id_server_port) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let reply = match timeout(SIGNAL_TIMEOUT, signal(port, punch, source)).await {
        Ok(Ok(reply)) => reply,
        _ => {
            return resp::no_store(resp::fail(
                101,
                "Could not reach the guest relay for this device",
            ))
        }
    };
    // Verify the ID/key binding against the pinned peer identity before
    // allowing any relay ticket. The peer's normal login still follows.
    if !signed_peer_key_matches(&key, &reply.peer_key, &row)
        || reply.version.is_empty()
        || uuid::Uuid::parse_str(&reply.uuid).is_err()
    {
        return resp::no_store(resp::fail(
            101,
            "The device did not return a verified guest relay",
        ));
    }

    match store_ticket(&state.db, row.id, &reply.uuid, Utc::now().timestamp()).await {
        Ok(relay_ticket) => resp::no_store(resp::success(json!({
            "relay_ticket": relay_ticket,
            "peer_id": row.peer_id,
            "peer_key": STANDARD.encode(reply.peer_key),
            "server_key": key,
        }))),
        Err(error) => failed(error),
    }
}

async fn store_ticket(
    db: &DatabaseConnection,
    grant_id: i32,
    relay_uuid: &str,
    now: i64,
) -> Result<String, DbErr> {
    ticket::Entity::delete_many()
        .filter(ticket::Column::ExpiresAt.lte(now))
        .exec(db)
        .await?;
    let token = random_token();
    ticket::ActiveModel {
        token_hash: Set(hash(&token)),
        grant_id: Set(grant_id),
        relay_uuid: Set(relay_uuid.to_owned()),
        expires_at: Set(now + 15),
    }
    .insert(db)
    .await?;
    Ok(token)
}

async fn claim_use(db: &DatabaseConnection, grant_id: i32, now: i64) -> Result<bool, DbErr> {
    let used = grant::Entity::update_many()
        .col_expr(
            grant::Column::UsedCount,
            Expr::col(grant::Column::UsedCount).add(1),
        )
        .filter(grant::Column::Id.eq(grant_id))
        .filter(grant::Column::ExpiresAt.gt(now))
        .filter(Expr::col(grant::Column::UsedCount).lt(Expr::col(grant::Column::MaxUses)))
        .exec(db)
        .await?
        .rows_affected;
    Ok(used == 1)
}

async fn take_ticket(
    db: &DatabaseConnection,
    token_hash: String,
) -> Result<Option<ticket::Model>, DbErr> {
    let now = Utc::now().timestamp();
    let Some(row) = ticket::Entity::find_by_id(&token_hash)
        .filter(ticket::Column::ExpiresAt.gt(now))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let deleted = ticket::Entity::delete_by_id(token_hash)
        .exec(db)
        .await?
        .rows_affected;
    Ok((deleted == 1).then_some(row))
}

async fn relay(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !available(&state) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(token_hash) = ticket_header(&headers) else {
        return denied();
    };
    let Ok(slot) = RELAY_SLOTS.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let row = match take_ticket(&state.db, token_hash).await {
        Ok(Some(row)) => row,
        Ok(None) => return denied(),
        Err(error) => return failed(error),
    };
    let source = client_ip.parse().ok();
    let record = match active_grant(&state.db, row.grant_id, source).await {
        Ok(Some(record)) => record,
        Ok(None) => return denied(),
        Err(error) => return failed(error),
    };
    ws.protocols([PROTOCOL])
        .max_frame_size(MAX_RELAY_MESSAGE)
        .max_message_size(MAX_RELAY_MESSAGE)
        .on_upgrade(move |browser| async move {
            let _slot = slot;
            proxy_relay(browser, state, record, row.relay_uuid, source).await;
        })
}

fn ticket_header(headers: &HeaderMap) -> Option<String> {
    let protocols = headers.get(header::SEC_WEBSOCKET_PROTOCOL)?.to_str().ok()?;
    let mut protocols = protocols.split(',').map(str::trim);
    if protocols.next() != Some(PROTOCOL) {
        return None;
    }
    valid_token_hash(protocols.next()?.strip_prefix("ticket.")?)
}

async fn active_grant(
    db: &DatabaseConnection,
    id: i32,
    source: Option<IpAddr>,
) -> Result<Option<grant::Model>, DbErr> {
    let Some(row) = grant::Entity::find_by_id(id)
        .filter(grant::Column::ExpiresAt.gt(Utc::now().timestamp()))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    Ok(target_still_enabled(db, &row, source).await?.then_some(row))
}

async fn target_still_enabled(
    db: &DatabaseConnection,
    row: &grant::Model,
    source: Option<IpAddr>,
) -> Result<bool, DbErr> {
    let device = peer::Entity::find_by_id(row.peer_row_id).one(db).await?;
    if !device.is_some_and(|device| {
        device.status == user::STATUS_ENABLE
            && device.id == row.peer_id
            && device.uuid == row.peer_uuid
            && device.pk == row.peer_key
    }) {
        return Ok(false);
    }
    if blocklist::blocks_guest(db, &row.peer_id, source).await? {
        return Ok(false);
    }
    let key = device_admission::DeviceKey {
        id: row.peer_id.clone(),
        uuid: row.peer_uuid.clone(),
        public_key: row.peer_key.clone(),
    };
    device_admission::allowed(db, &key, false).await
}

fn websocket_port(daemon_port: i32) -> Option<u16> {
    u16::try_from(daemon_port).ok()?.checked_add(2)
}

async fn signal(port: u16, punch: Punch, source: Option<IpAddr>) -> anyhow::Result<Relay> {
    let mut server = local_socket(port, MAX_SIGNAL_MESSAGE, source).await?;
    let message = Envelope {
        punch: Some(punch),
        ..Default::default()
    };
    server
        .send(WireMessage::Binary(message.encode_to_vec()))
        .await?;
    while let Some(message) = server.next().await {
        if let WireMessage::Binary(bytes) = message? {
            let response = Envelope::decode(bytes.as_slice())?;
            return response
                .relay
                .filter(|relay| relay.refusal.is_empty())
                .ok_or_else(|| anyhow::anyhow!("The guest relay was refused"));
        }
    }
    anyhow::bail!("The signal connection closed")
}

async fn local_socket(
    port: u16,
    max_size: usize,
    source: Option<IpAddr>,
) -> anyhow::Result<WebSocketStream<TcpStream>> {
    let socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
    let config = WebSocketConfig {
        max_message_size: Some(max_size),
        max_frame_size: Some(max_size),
        ..Default::default()
    };
    let mut request = format!("ws://127.0.0.1:{port}").into_client_request()?;
    // Forward the address derived by Console's trusted-proxy policy.
    if let Some(ip) = source {
        request
            .headers_mut()
            .insert("x-real-ip", ip.to_string().parse()?);
    }
    let (ws, _) = client_async_with_config(request, socket, Some(config)).await?;
    Ok(ws)
}

async fn proxy_relay(
    browser: ws::WebSocket,
    state: AppState,
    record: grant::Model,
    relay_uuid: String,
    source: Option<IpAddr>,
) {
    let Some(port) = websocket_port(state.config.admin.relay_server_port) else {
        return;
    };
    let mut browser = browser;
    let mut upstream =
        match timeout(WRITE_TIMEOUT, local_socket(port, MAX_RELAY_MESSAGE, source)).await {
            Ok(Ok(socket)) => socket,
            _ => return,
        };
    let request = RequestRelay {
        uuid: relay_uuid,
        key: state.webclient_config.get().await.key.trim().to_owned(),
    };
    let message = Envelope {
        request_relay: Some(request),
        ..Default::default()
    };
    if !io_completed(
        timeout(
            WRITE_TIMEOUT,
            upstream.send(WireMessage::Binary(message.encode_to_vec())),
        )
        .await,
    ) {
        return;
    }
    // The relay UUID does not authenticate the peer. Check the real target's
    // signed key-exchange frame before sending any bytes in either direction.
    let first = match timeout(WRITE_TIMEOUT, upstream.next()).await {
        Ok(Some(Ok(WireMessage::Binary(bytes)))) if verify_peer_hello(&bytes, &record) => bytes,
        _ => return,
    };
    if !matches!(
        timeout(WRITE_TIMEOUT, active_grant(&state.db, record.id, source)).await,
        Ok(Ok(Some(_)))
    ) || !matches!(
        timeout(
            WRITE_TIMEOUT,
            claim_use(&state.db, record.id, Utc::now().timestamp())
        )
        .await,
        Ok(Ok(true))
    ) || !io_completed(timeout(WRITE_TIMEOUT, browser.send(ws::Message::Binary(first))).await)
    {
        return;
    }

    let (mut upstream_tx, mut upstream_rx) = upstream.split();
    let (mut browser_tx, mut browser_rx) = browser.split();
    let mut checks = interval(Duration::from_secs(2));
    let opened = tokio::time::Instant::now();
    tracing::info!(target: "security_audit", event = "web_guest.open", grant_id = record.id, peer_id = %record.peer_id, source_ip = ?source);
    let uplink = async {
        while let Some(Ok(msg)) = browser_rx.next().await {
            match msg {
                ws::Message::Binary(bytes) => {
                    if !io_completed(
                        timeout(WRITE_TIMEOUT, upstream_tx.send(WireMessage::Binary(bytes))).await,
                    ) {
                        break;
                    }
                }
                ws::Message::Ping(_) | ws::Message::Pong(_) => {}
                _ => break,
            }
        }
        "browser_disconnected"
    };
    let downlink = async {
        while let Some(Ok(msg)) = upstream_rx.next().await {
            match msg {
                WireMessage::Binary(bytes) => {
                    if !io_completed(
                        timeout(WRITE_TIMEOUT, browser_tx.send(ws::Message::Binary(bytes))).await,
                    ) {
                        break;
                    }
                }
                WireMessage::Ping(_) | WireMessage::Pong(_) => {}
                _ => break,
            }
        }
        "relay_disconnected"
    };
    let validity = async {
        loop {
            checks.tick().await;
            if !matches!(
                timeout(WRITE_TIMEOUT, active_grant(&state.db, record.id, source)).await,
                Ok(Ok(Some(_)))
            ) {
                break "authorization_ended";
            }
        }
    };
    let expires_in = (record.expires_at - Utc::now().timestamp()).max(0) as u64;
    let reason = tokio::select! {
        reason = uplink => reason,
        reason = downlink => reason,
        reason = validity => reason,
        _ = tokio::time::sleep(Duration::from_secs(expires_in)) => "expired",
    };
    tracing::info!(target: "security_audit", event = "web_guest.close", grant_id = record.id, peer_id = %record.peer_id, duration_ms = opened.elapsed().as_millis() as u64, reason);
}

fn io_completed<E>(outcome: Result<Result<(), E>, tokio::time::error::Elapsed>) -> bool {
    matches!(outcome, Ok(Ok(())))
}

#[derive(Serialize)]
struct SignalClaims<'a> {
    aud: &'static str,
    sub: &'a str,
    exp: i64,
    jti: uuid::Uuid,
}

fn signal_proof(peer_id: &str, secret: &str) -> jsonwebtoken::errors::Result<String> {
    let claims = SignalClaims {
        aud: PROTOCOL,
        sub: peer_id,
        exp: Utc::now().timestamp() + 15,
        jti: uuid::Uuid::new_v4(),
    };
    let derived = Sha256::new()
        .chain_update(b"rustdesk-guest-signal-v1\0")
        .chain_update(secret.as_bytes())
        .finalize();
    let jwt = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&derived),
    )?;
    Ok(format!("rdg1.{jwt}"))
}

#[derive(Clone, PartialEq, Message)]
struct SignedIdentity {
    #[prost(string, tag = "1")]
    id: String,
    #[prost(bytes = "vec", tag = "2")]
    pk: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct PeerHello {
    #[prost(message, optional, tag = "3")]
    signed_id: Option<PeerSignature>,
}

#[derive(Clone, PartialEq, Message)]
struct PeerSignature {
    #[prost(bytes = "vec", tag = "1")]
    id: Vec<u8>,
}

fn verify_peer_hello(bytes: &[u8], record: &grant::Model) -> bool {
    if bytes.len() > MAX_SIGNAL_MESSAGE {
        return false;
    }
    let Ok(peer_key) = STANDARD.decode(&record.peer_key) else {
        return false;
    };
    PeerHello::decode(bytes)
        .ok()
        .and_then(|hello| hello.signed_id)
        .and_then(|signature| verified_identity(&signature.id, &peer_key))
        .is_some_and(|identity| identity.id == record.peer_id && identity.pk.len() == 32)
}

fn signed_peer_key_matches(server_key: &str, signed: &[u8], row: &grant::Model) -> bool {
    let (Ok(key), Ok(peer_key)) = (STANDARD.decode(server_key), STANDARD.decode(&row.peer_key))
    else {
        return false;
    };
    verified_identity(signed, &key)
        .is_some_and(|identity| identity.id == row.peer_id && identity.pk == peer_key)
}

fn verified_identity(signed: &[u8], public_key: &[u8]) -> Option<SignedIdentity> {
    let (signature, data) = signed.split_at_checked(64)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
        .verify(data, signature)
        .ok()?;
    SignedIdentity::decode(data).ok()
}

fn random_token() -> String {
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn valid_token_hash(token: &str) -> Option<String> {
    if token.len() != 43
        || URL_SAFE_NO_PAD
            .decode(token)
            .map_or(true, |bytes| bytes.len() != 32)
    {
        return None;
    }
    Some(hash(token))
}

fn denied() -> Response {
    resp::no_store(resp::fail(101, "The guest link is no longer available"))
}

fn failed(error: DbErr) -> Response {
    tracing::warn!(%error, "Guest session database operation failed");
    resp::no_store(resp::fail(101, "OperationFailed"))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use sea_orm::{ConnectionTrait, Database, DbBackend, Schema};

    use super::*;

    fn fixture_grant() -> grant::Model {
        grant::Model {
            id: 1,
            token_hash: "example".to_owned(),
            peer_row_id: 2,
            peer_id: "target".to_owned(),
            peer_uuid: "registered-device".to_owned(),
            peer_key: String::new(),
            expires_at: Utc::now().timestamp() + 300,
            max_uses: 1,
            used_count: 0,
        }
    }

    #[tokio::test]
    async fn concurrent_guest_sessions_charge_one_use_and_revoke_closes_authorization() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for table in [
            schema.create_table_from_entity(peer::Entity),
            schema.create_table_from_entity(entity::blocklist_rule::Entity),
            schema.create_table_from_entity(entity::routing_settings::Entity),
            schema.create_table_from_entity(grant::Entity),
            schema.create_table_from_entity(ticket::Entity),
        ] {
            db.execute(DbBackend::Sqlite.build(&table)).await.unwrap();
        }
        let mut fixture = fixture_grant();
        fixture.peer_key = "example-peer-key".to_owned();
        let target = peer::ActiveModel {
            row_id: Set(fixture.peer_row_id),
            id: Set(fixture.peer_id.clone()),
            uuid: Set(fixture.peer_uuid.clone()),
            pk: Set(fixture.peer_key.clone()),
            status: Set(user::STATUS_ENABLE),
            ..Default::default()
        };
        target.insert(&db).await.unwrap();
        let row: grant::ActiveModel = fixture.clone().into();
        row.insert(&db).await.unwrap();
        assert!(active_grant(&db, fixture.id, None).await.unwrap().is_some());

        let now = Utc::now().timestamp();
        let token = store_ticket(&db, fixture.id, "upstream-first", now)
            .await
            .unwrap();
        let (first, second) = tokio::join!(
            claim_use(&db, fixture.id, now),
            claim_use(&db, fixture.id, now)
        );
        assert_ne!(first.unwrap(), second.unwrap());
        let token_hash = hash(&token);
        let taken = take_ticket(&db, token_hash.clone()).await.unwrap().unwrap();
        assert_eq!(taken.grant_id, fixture.id);
        assert!(take_ticket(&db, token_hash).await.unwrap().is_none());
        grant::Entity::delete_by_id(fixture.id)
            .exec(&db)
            .await
            .unwrap();
        assert!(active_grant(&db, fixture.id, None).await.unwrap().is_none());
        assert!(!claim_use(&db, fixture.id, now).await.unwrap());
    }

    #[test]
    fn guest_identity_must_be_signed_for_the_original_peer() {
        let mut record = fixture_grant();
        let secret = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let server = Ed25519KeyPair::from_pkcs8(secret.as_ref()).unwrap();
        let server_key = STANDARD.encode(server.public_key().as_ref());
        record.peer_key = STANDARD.encode([2; 32]);
        let mut identity = SignedIdentity {
            id: record.peer_id.clone(),
            pk: vec![2; 32],
        };
        let attach_signature = |id: SignedIdentity| {
            let data = id.encode_to_vec();
            let mut signed = server.sign(&data).as_ref().to_vec();
            signed.extend(data);
            signed
        };
        let signed = attach_signature(identity.clone());
        assert!(signed_peer_key_matches(&server_key, &signed, &record));
        identity.pk = vec![5; 32];
        assert!(!signed_peer_key_matches(
            &server_key,
            &attach_signature(identity),
            &record
        ));
        record.peer_id = "other".to_owned();
        assert!(!signed_peer_key_matches(&server_key, &signed, &record));
    }

    #[test]
    fn relay_hello_requires_the_pinned_device_key_and_id() {
        let mut record = fixture_grant();
        let secret = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let peer = Ed25519KeyPair::from_pkcs8(secret.as_ref()).unwrap();
        record.peer_key = STANDARD.encode(peer.public_key().as_ref());
        let identity = SignedIdentity {
            id: record.peer_id.clone(),
            pk: vec![3; 32],
        }
        .encode_to_vec();
        let signature = peer.sign(&identity);
        let frame = PeerHello {
            signed_id: Some(PeerSignature {
                id: [signature.as_ref(), &identity].concat(),
            }),
        }
        .encode_to_vec();
        assert!(verify_peer_hello(&frame, &record));
        record.peer_id = "wrong-device".to_owned();
        assert!(!verify_peer_hello(&frame, &record));
        assert!(!verify_peer_hello(b"unexpected frame", &record));
    }

    #[test]
    fn browser_ticket_must_arrive_as_a_matching_websocket_protocol() {
        let token = random_token();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_str(&format!("{PROTOCOL}, ticket.{token}")).unwrap(),
        );
        assert_eq!(ticket_header(&headers), Some(hash(&token)));
        assert!(ticket_header(&HeaderMap::new()).is_none());
        assert!(valid_token_hash("unstructured").is_none());
    }
}

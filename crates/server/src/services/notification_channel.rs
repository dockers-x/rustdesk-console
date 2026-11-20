use std::str::FromStr;

use hmac::{Hmac, Mac};
use reqwest::{Client, RequestBuilder, Response, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;

const WEBHOOK_RESPONSE_LIMIT: usize = 4_000;
const PROVIDER_RESPONSE_LIMIT: usize = 8_192;
// ntfy echoes the title and body, with JSON escaping that can expand each character.
const NTFY_RESPONSE_LIMIT: usize = 32_768;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Channel {
    #[default]
    Webhook,
    Ntfy,
    Bark,
    ServerChan,
}

impl Channel {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Webhook => "webhook",
            Self::Ntfy => "ntfy",
            Self::Bark => "bark",
            Self::ServerChan => "server_chan",
        }
    }
}

impl FromStr for Channel {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "webhook" => Ok(Self::Webhook),
            "ntfy" => Ok(Self::Ntfy),
            "bark" => Ok(Self::Bark),
            "server_chan" => Ok(Self::ServerChan),
            _ => Err("unsupported notification channel".into()),
        }
    }
}

pub(super) fn normalize_endpoint(
    channel: Channel,
    endpoint: &str,
    topic: &str,
) -> Result<String, String> {
    let endpoint = endpoint.trim();
    if channel == Channel::ServerChan {
        return if endpoint.is_empty() {
            Ok(String::new())
        } else {
            Err("Server Chan selects its endpoint from the SendKey".into())
        };
    }

    if channel == Channel::Ntfy
        && (topic.is_empty()
            || topic.len() > 64
            || !topic
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    {
        return Err("ntfy topic must contain 1–64 letters, numbers, dashes, or underscores".into());
    }

    let parsed = Url::parse(endpoint).map_err(|_| "invalid notification URL")?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("notification URL must use http:// or https://".into());
    }
    let has_userinfo = endpoint.split_once("://").is_some_and(|(_, rest)| {
        rest.split(['/', '\\', '?', '#'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    });
    if has_userinfo || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("notification URL must not contain credentials".into());
    }
    if parsed.fragment().is_some() || (channel != Channel::Webhook && parsed.query().is_some()) {
        return Err("notification URL must not contain a fragment or provider query".into());
    }

    Ok(endpoint.to_owned())
}

pub(super) fn validate_secret(channel: Channel, secret: &str) -> Result<(), String> {
    match channel {
        Channel::Webhook => Ok(()),
        Channel::Ntfy => {
            if secret.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'+' | b'/' | b'=')
            }) {
                Ok(())
            } else {
                Err("invalid ntfy access token".into())
            }
        }
        Channel::Bark => {
            if secret.trim().is_empty() {
                Err("Bark device key is required".into())
            } else {
                Ok(())
            }
        }
        Channel::ServerChan => server_chan_url(secret).map(|_| ()).map_err(str::to_owned),
    }
}

fn server_chan_url(send_key: &str) -> Result<String, &'static str> {
    if send_key.is_empty() {
        return Err("Server Chan SendKey is required");
    }
    if !send_key
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("invalid Server Chan SendKey");
    }
    if let Some(rest) = send_key.strip_prefix("sctp") {
        let (uid, suffix) = rest
            .split_once('t')
            .ok_or("invalid Server Chan 3 SendKey")?;
        if uid.is_empty()
            || uid.len() > 63
            || !uid.bytes().all(|byte| byte.is_ascii_digit())
            || suffix.is_empty()
        {
            return Err("invalid Server Chan 3 SendKey");
        }
        return Ok(format!("https://{uid}.push.ft07.com/send/{send_key}.send"));
    }
    Ok(format!("https://sctapi.ftqq.com/{send_key}.send"))
}

pub(super) struct Destination<'a> {
    pub channel: Channel,
    pub url: &'a str,
    pub topic: &'a str,
    pub secret: &'a str,
}

pub(super) struct Event<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub body: &'a str,
}

#[derive(Debug)]
pub(super) struct DeliveryResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Debug)]
pub(super) struct DeliveryError {
    pub status: Option<u16>,
    pub reason: String,
    pub retryable: bool,
    pub body: String,
}

impl DeliveryError {
    fn new(status: Option<u16>, reason: impl Into<String>, retryable: bool) -> Self {
        Self {
            status,
            reason: reason.into(),
            retryable,
            body: String::new(),
        }
    }
}

pub(super) async fn deliver(
    client: &Client,
    destination: Destination<'_>,
    event: Event<'_>,
) -> Result<DeliveryResponse, DeliveryError> {
    let request = build_request(client, &destination, &event)
        .map_err(|reason| DeliveryError::new(None, reason, false))?;
    let response = request.send().await.map_err(|error| {
        if error.is_builder() {
            DeliveryError::new(None, "invalid notification request", false)
        } else if error.is_timeout() {
            DeliveryError::new(None, "notification request timed out", true)
        } else {
            DeliveryError::new(None, "notification connection failed", true)
        }
    })?;
    let status = response.status();
    let status_code = status.as_u16();
    if destination.channel == Channel::Webhook {
        let (bytes, _) = read_body(response, WEBHOOK_RESPONSE_LIMIT)
            .await
            .unwrap_or_default();
        let mut body = String::from_utf8_lossy(&bytes).into_owned();
        body.truncate(body.floor_char_boundary(WEBHOOK_RESPONSE_LIMIT));
        return if status.is_success() {
            Ok(DeliveryResponse {
                status: status_code,
                body,
            })
        } else {
            let mut error = http_error(status);
            error.body = body;
            Err(error)
        };
    }
    if !status.is_success() {
        return Err(http_error(status));
    }

    let status = Some(status_code);
    let limit = if destination.channel == Channel::Ntfy {
        NTFY_RESPONSE_LIMIT
    } else {
        PROVIDER_RESPONSE_LIMIT
    };
    let (bytes, truncated) = read_body(response, limit).await.map_err(|_| {
        DeliveryError::new(
            status,
            "could not read notification provider response",
            true,
        )
    })?;
    if truncated {
        return Err(DeliveryError::new(
            status,
            "notification provider response is too large",
            false,
        ));
    }
    validate_response(destination.channel, destination.topic, &bytes)
        .map_err(|(reason, retryable)| DeliveryError::new(status, reason, retryable))?;
    Ok(DeliveryResponse {
        status: status_code,
        body: String::new(),
    })
}

fn build_request(
    client: &Client,
    destination: &Destination<'_>,
    event: &Event<'_>,
) -> Result<RequestBuilder, String> {
    let Destination {
        channel,
        url,
        topic,
        secret,
    } = *destination;
    if channel == Channel::Webhook {
        let mut request = client
            .post(url)
            .header("content-type", "application/json")
            .header("x-rustdesk-event", event.kind)
            .header("x-rustdesk-event-id", event.id);
        if !secret.is_empty() {
            request = request.header("x-rustdesk-signature", signature(secret, event.body));
        }
        return Ok(request.body(event.body.to_owned()));
    }

    let (title, message) = mobile_message(event)?;
    let request = match channel {
        Channel::Ntfy => {
            let request = client
                .post(url)
                .json(&json!({ "topic": topic, "title": title, "message": message }));
            if secret.is_empty() {
                request
            } else {
                request.bearer_auth(secret)
            }
        }
        Channel::Bark => {
            let push_url = format!("{}/push", url.trim_end_matches('/'));
            client
                .post(push_url)
                .json(&json!({ "device_key": secret, "title": title, "body": message }))
        }
        Channel::ServerChan => client
            .post(server_chan_url(secret)?)
            .json(&json!({ "title": title, "desp": message })),
        Channel::Webhook => unreachable!("webhook request was already built"),
    };
    Ok(request)
}

fn mobile_message(event: &Event<'_>) -> Result<(String, String), &'static str> {
    #[derive(Deserialize)]
    struct Device {
        name: String,
        id: String,
    }
    #[derive(Deserialize)]
    struct Notification {
        title: String,
        body: String,
    }
    #[derive(Deserialize)]
    struct Payload {
        resource: Option<Device>,
        notification: Option<Notification>,
    }

    let (title, state) = match event.kind {
        "device.online" => ("RustDesk device online", Some("Online")),
        "device.offline" => ("RustDesk device offline", Some("Offline")),
        "webhook.test" => ("RustDesk notification test", None),
        _ => return Err("unsupported notification event"),
    };
    let Payload {
        resource,
        notification,
    } = serde_json::from_str(event.body).map_err(|_| "invalid notification event payload")?;
    if let Some(Notification { title, body }) = notification {
        return Ok((title, body));
    }
    let Some(state) = state else {
        return Ok((
            title.into(),
            "This is a test notification from RustDesk Console.".into(),
        ));
    };

    let resource = resource.ok_or("device notification is missing its resource")?;
    let id = notification_field(&resource.id);
    let name = notification_field(&resource.name);
    let name = if name.is_empty() { &id } else { &name };
    Ok((
        title.into(),
        format!("Device: {name}\nRustDesk ID: {id}\nState: {state}"),
    ))
}

fn notification_field(value: &str) -> String {
    value
        .trim()
        .chars()
        .take(160)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn validate_response(
    channel: Channel,
    topic: &str,
    body: &[u8],
) -> Result<(), (&'static str, bool)> {
    #[derive(Deserialize)]
    struct NtfyResponse {
        id: String,
        topic: String,
    }
    #[derive(Deserialize)]
    struct CodedResponse {
        code: i64,
    }
    const MALFORMED: (&str, bool) = ("invalid notification provider response", false);
    const REJECTED: (&str, bool) = ("notification provider rejected the request", false);
    match channel {
        Channel::Ntfy => {
            let response: NtfyResponse = serde_json::from_slice(body).map_err(|_| MALFORMED)?;
            if response.id.is_empty() || response.topic != topic {
                return Err(MALFORMED);
            }
        }
        Channel::Bark | Channel::ServerChan => {
            let response: CodedResponse = serde_json::from_slice(body).map_err(|_| MALFORMED)?;
            let success_code = if channel == Channel::Bark { 200 } else { 0 };
            match response.code {
                code if code == success_code => (),
                429 | 500..=599 => {
                    return Err(("notification provider is temporarily unavailable", true));
                }
                _ => return Err(REJECTED),
            }
        }
        Channel::Webhook => unreachable!("webhook responses are handled by HTTP status"),
    }
    Ok(())
}

fn http_error(status: StatusCode) -> DeliveryError {
    DeliveryError::new(
        Some(status.as_u16()),
        format!("HTTP {}", status.as_u16()),
        status.is_server_error()
            || matches!(
                status,
                StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS
            ),
    )
}

async fn read_body(
    mut response: Response,
    limit: usize,
) -> Result<(Vec<u8>, bool), reqwest::Error> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let remaining = limit - bytes.len();
        if chunk.len() > remaining {
            bytes.extend_from_slice(&chunk[..remaining]);
            return Ok((bytes, true));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((bytes, false))
}

pub(super) fn signature(secret: &str, body: &str) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(body.as_bytes());
    format!("sha256={:x}", mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use axum::response::{IntoResponse, Redirect};
    use axum::routing::post;
    use axum::Router;
    use serde_json::Value;

    use super::*;

    const DEVICE_EVENT: Event<'_> = Event {
        id: "evt_fixture",
        kind: "device.online",
        body: r#"{"resource":{"name":"桌面\n电脑","id":"123456789","hostname":"private-host"},"device":{"last_online_ip":"192.0.2.8"}}"#,
    };
    const TEST_EVENT: Event<'_> = Event {
        id: "evt_test",
        kind: "webhook.test",
        body: r#"{"test":true}"#,
    };

    #[test]
    fn notification_configuration_validates_urls_and_write_only_credentials() {
        for channel in [
            Channel::Webhook,
            Channel::Ntfy,
            Channel::Bark,
            Channel::ServerChan,
        ] {
            assert_eq!(channel.as_str().parse::<Channel>().unwrap(), channel);
            assert_eq!(serde_json::to_value(channel).unwrap(), channel.as_str());
        }
        assert!("server_chan_3".parse::<Channel>().is_err());

        for url in [
            "ftp://example.test/",
            "https://user:secret@example.test/",
            "https://@example.test/",
            "https://example.test/#secret",
            "https://example.test/?token=secret",
        ] {
            assert!(normalize_endpoint(Channel::Ntfy, url, "my-topic").is_err());
        }
        for topic in ["", "name/subtopic", "中文", &"a".repeat(65)] {
            assert!(normalize_endpoint(Channel::Ntfy, "http://localhost", topic).is_err());
        }
        assert_eq!(
            normalize_endpoint(
                Channel::Ntfy,
                " http://localhost:7777/ntfy/ ",
                "my_topic-123"
            )
            .unwrap(),
            "http://localhost:7777/ntfy/"
        );
        assert_eq!(
            normalize_endpoint(Channel::Webhook, "https://example.test/?foo=bar", "").unwrap(),
            "https://example.test/?foo=bar"
        );
        assert!(normalize_endpoint(Channel::ServerChan, "https://example.test/", "").is_err());
        assert_eq!(normalize_endpoint(Channel::ServerChan, "", "").unwrap(), "");
        assert!(validate_secret(Channel::Ntfy, "").is_ok());
        assert!(validate_secret(Channel::Ntfy, "tk_x\r\n").is_err());
        assert!(validate_secret(Channel::Bark, " ").is_err());
        for secret in [
            "",
            "sctptABC",
            "sctp42t",
            "sctp4xtABC",
            "x/../y",
            "x?url=hello",
        ] {
            assert!(validate_secret(Channel::ServerChan, secret).is_err());
        }
    }

    #[test]
    fn providers_use_documented_requests_and_only_send_mobile_fields() {
        let client = Client::new();
        let mobile_message = "Device: 桌面 电脑\nRustDesk ID: 123456789\nState: Online";
        for (channel, url, secret, target, body) in [
            (
                Channel::Ntfy,
                "http://example.test/ntfy/",
                "tk_fake",
                "http://example.test/ntfy/",
                json!({ "topic": "my-topic", "title": "RustDesk device online", "message": mobile_message }),
            ),
            (
                Channel::Bark,
                "https://example.test/bark/",
                "device-key",
                "https://example.test/bark/push",
                json!({ "device_key": "device-key", "title": "RustDesk device online", "body": mobile_message }),
            ),
            (
                Channel::ServerChan,
                "",
                "SCTexample",
                "https://sctapi.ftqq.com/SCTexample.send",
                json!({ "title": "RustDesk device online", "desp": mobile_message }),
            ),
            (
                Channel::ServerChan,
                "",
                "sctp123tFake",
                "https://123.push.ft07.com/send/sctp123tFake.send",
                json!({ "title": "RustDesk device online", "desp": mobile_message }),
            ),
        ] {
            let destination = Destination {
                channel,
                url,
                topic: "my-topic",
                secret,
            };
            let request = build_request(&client, &destination, &DEVICE_EVENT)
                .unwrap()
                .build()
                .unwrap();
            assert_eq!(request.method(), reqwest::Method::POST);
            assert_eq!(request.url().as_str(), target);
            assert_eq!(request.headers()["content-type"], "application/json");
            if channel == Channel::Ntfy {
                assert_eq!(request.headers()["authorization"], "Bearer tk_fake");
                assert!(request.headers()["authorization"].is_sensitive());
            } else {
                assert!(!request.headers().contains_key("authorization"));
            }
            let actual: Value =
                serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
            assert_eq!(actual, body);
        }

        let offline = Event {
            kind: "device.offline",
            ..DEVICE_EVENT
        };
        assert_eq!(
            super::mobile_message(&offline).unwrap().0,
            "RustDesk device offline"
        );
        assert!(super::mobile_message(&offline)
            .unwrap()
            .1
            .ends_with("State: Offline"));
        assert_eq!(
            super::mobile_message(&TEST_EVENT).unwrap().0,
            "RustDesk notification test"
        );
    }

    #[test]
    fn providers_use_rendered_templates_for_device_events_and_manual_tests() {
        let client = Client::new();
        let title = "办公电脑上线 {{literal}}";
        let message = "已连接\n会话可用";
        let notification = json!({ "title": title, "body": message });
        let mut device_payload = serde_json::from_str::<Value>(DEVICE_EVENT.body).unwrap();
        device_payload["notification"] = notification.clone();
        let device_payload = device_payload.to_string();
        let test_payload = json!({ "test": true, "notification": notification }).to_string();
        let device = Event {
            body: &device_payload,
            ..DEVICE_EVENT
        };
        let test = Event {
            body: &test_payload,
            ..TEST_EVENT
        };

        for (channel, url, secret, body_key) in [
            (Channel::Ntfy, "https://example.test/", "", "message"),
            (Channel::Bark, "https://example.test/", "fake-key", "body"),
            (Channel::ServerChan, "", "SCTfake", "desp"),
            (Channel::ServerChan, "", "sctp123tFake", "desp"),
        ] {
            let destination = Destination {
                channel,
                url,
                topic: "my-topic",
                secret,
            };
            for event in [&device, &test] {
                let request = build_request(&client, &destination, event)
                    .unwrap()
                    .build()
                    .unwrap();
                let actual: Value =
                    serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
                assert_eq!(actual["title"], title);
                assert_eq!(actual[body_key], message);
                assert!(actual.get("notification").is_none());
                assert!(actual.get("resource").is_none());
            }
        }
    }

    #[test]
    fn webhook_preserves_event_headers_and_signature_over_original_bytes() {
        let client = Client::new();
        let destination = Destination {
            channel: Channel::Webhook,
            url: "https://example.test/?foo=bar",
            topic: "",
            secret: "fixture-hmac",
        };
        let event = Event {
            body: r#"{"x": 1}"#,
            ..DEVICE_EVENT
        };
        let request = build_request(&client, &destination, &event)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(request.headers()["content-type"], "application/json");
        assert_eq!(request.headers()["x-rustdesk-event-id"], event.id);
        assert_eq!(request.headers()["x-rustdesk-event"], event.kind);
        assert_eq!(
            request.headers()["x-rustdesk-signature"],
            "sha256=ad0a25258976f5dc5a8dc3dd07f981372a6f791d219b101ce3dc7192f867d541"
        );
        assert_eq!(
            request.body().unwrap().as_bytes().unwrap(),
            event.body.as_bytes()
        );

        let event = Event {
            body: r#"{"notification": {"title":"Example","body":"Original bytes"},"resource":{"id":"42"}}"#,
            ..DEVICE_EVENT
        };
        let request = build_request(&client, &destination, &event)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(
            request.body().unwrap().as_bytes().unwrap(),
            event.body.as_bytes()
        );
        assert_eq!(
            request.headers()["x-rustdesk-signature"],
            signature(destination.secret, event.body)
        );
    }

    #[test]
    fn coded_provider_responses_report_rejections_even_with_http_success() {
        assert!(validate_response(
            Channel::ServerChan,
            "",
            br#"{"code":0,"message":"success"}"#
        )
        .is_ok());
        assert!(
            validate_response(Channel::Bark, "", br#"{"code":200,"message":"success"}"#).is_ok()
        );
        assert!(
            !validate_response(Channel::Bark, "", br#"{"code":400,"message":"secret"}"#)
                .unwrap_err()
                .1
        );
        assert!(
            validate_response(Channel::Bark, "", br#"{"code":503,"message":"secret"}"#)
                .unwrap_err()
                .1
        );
        assert!(validate_response(
            Channel::Ntfy,
            "actual-topic",
            br#"{"id":"abc","topic":"other-topic"}"#
        )
        .is_err());
    }

    #[tokio::test]
    async fn local_delivery_bounds_webhook_responses_and_hides_provider_bodies() {
        let app = Router::new()
            .route(
                "/webhook",
                post(|| async { (StatusCode::BAD_REQUEST, "猫".repeat(2_000)) }),
            )
            .route(
                "/bark/push",
                post(|| async { r#"{"code":429,"message":"sensitive response"}"# }),
            )
            .route("/", post(|| async { r#"{"id":"abc","topic":"my-topic"}"# }))
            .route(
                "/echo",
                post(|| async {
                    axum::Json(
                        json!({ "id": "abc", "topic": "my-topic", "message": "猫".repeat(4_000) }),
                    )
                }),
            )
            .route(
                "/oversized",
                post(|| async { "x".repeat(NTFY_RESPONSE_LIMIT + 1) }),
            )
            .route(
                "/redirect/push",
                post(|| async { Redirect::temporary("/bark/push").into_response() }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap();
        let call = |channel, path| {
            let url = format!("{base}{path}");
            let client = &client;
            async move {
                deliver(
                    client,
                    Destination {
                        channel,
                        url: &url,
                        topic: "my-topic",
                        secret: "fixture-key",
                    },
                    TEST_EVENT,
                )
                .await
            }
        };

        let webhook = call(Channel::Webhook, "/webhook").await.unwrap_err();
        assert_eq!(webhook.status, Some(400));
        assert!(!webhook.retryable);
        assert!(webhook.body.starts_with("猫"));
        assert!(webhook.body.len() <= WEBHOOK_RESPONSE_LIMIT);

        let bark = call(Channel::Bark, "/bark").await.unwrap_err();
        assert_eq!(bark.status, Some(200));
        assert!(bark.retryable);
        assert!(bark.body.is_empty());
        assert!(!bark.reason.contains("sensitive response"));

        let ntfy = call(Channel::Ntfy, "/").await.unwrap();
        assert_eq!(ntfy.status, 200);
        assert!(ntfy.body.is_empty());
        let rendered_echo = call(Channel::Ntfy, "/echo").await.unwrap();
        assert!(rendered_echo.body.is_empty());
        let oversized = call(Channel::Ntfy, "/oversized").await.unwrap_err();
        assert!(oversized.body.is_empty());
        assert!(oversized.reason.contains("too large"));

        let redirect = call(Channel::Bark, "/redirect").await.unwrap_err();
        assert_eq!(redirect.status, Some(307));
        assert!(!redirect.retryable);
        assert!(redirect.body.is_empty());
        server.abort();
    }
}

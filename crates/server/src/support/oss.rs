//! Aliyun OSS post-policy token, ported from `lib/upload/oss.go` (`GetPolicyToken`).
//! Lets the browser upload directly to OSS with a server-signed policy.

use base64::Engine;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha1::Sha1;

use crate::config::Oss;

type HmacSha1 = Hmac<Sha1>;

fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Build the OSS policy token JSON the frontend uses for a direct upload.
pub fn get_policy_token(oss: &Oss, upload_dir: &str) -> String {
    let now = chrono::Utc::now().timestamp();
    let expire_end = now + oss.expire_time;
    let token_expire = chrono::DateTime::<chrono::Utc>::from_timestamp(expire_end, 0)
        .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default();

    let policy_doc = json!({
        "expiration": token_expire,
        "conditions": [
            ["starts-with", "$key", upload_dir],
            ["content-length-range", 0, oss.max_byte],
        ],
    });
    let policy_b64 = b64(&serde_json::to_vec(&policy_doc).unwrap_or_default());

    let signature = match HmacSha1::new_from_slice(oss.access_key_secret.as_bytes()) {
        Ok(mut mac) => {
            mac.update(policy_b64.as_bytes());
            b64(&mac.finalize().into_bytes())
        }
        Err(_) => String::new(),
    };

    let callback = json!({
        "callbackUrl": oss.callback_url,
        "callbackBody": "bucket=${bucket}&etag=${etag}&filename=${object}&size=${size}&mime_type=${mimeType}&height=${imageInfo.height}&width=${imageInfo.width}&format=${imageInfo.format}&origin_filename=${x:origin_filename}",
        "callbackBodyType": "application/x-www-form-urlencoded",
    });
    let callback_b64 = b64(&serde_json::to_vec(&callback).unwrap_or_default());

    let token = json!({
        "accessid": oss.access_key_id,
        "host": oss.host,
        "expire": expire_end,
        "signature": signature,
        "policy": policy_b64,
        "dir": upload_dir,
        "callback": callback_b64,
    });
    serde_json::to_string(&token).unwrap_or_default()
}

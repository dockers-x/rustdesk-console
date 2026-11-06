# Developer access

**English** · [中文](developer-access.zh-CN.md)

Open **Developer access → API keys** in Console. Create a named key, choose only
the required scopes and save the displayed value immediately. Console stores only
its hash and display prefix; the complete key cannot be retrieved again. Default
permissions are read-only and expiry is 90 days (maximum 365 days). Revoke unused
keys in the same page. This feature does not require a custom or self-signed client.

Use HTTPS outside a trusted local network. Store the key in a secret manager or
environment variable, never in URLs, committed source, browser applications or
diagnostic logs. API keys cannot log into RustDesk, manage keys or authenticate
existing Console session/deployment endpoints.

## SDKs and examples

The built-in **SDKs and examples** page provides runnable Rust, Go and Node.js
device-list examples. Set `RUSTDESK_CONSOLE_URL` to the external Console base URL
(without `/api/v1`) and `RUSTDESK_CONSOLE_API_KEY` to your key.

- Rust: `cargo add rustdesk-console-sdk`; source: https://github.com/lib-x/rustdesk-console-sdk-rs
- Go: `go get github.com/lib-x/rustdesk-console-sdk-go@v0.1.0`
- Node.js: `npm install github:lib-x/rustdesk-console-sdk-ts#v0.1.0`

Each repository contains typed methods for all 16 operations, complete examples,
English/Chinese documentation and tests. SDKs keep certificate verification enabled,
reject redirects and do not automatically retry mutations. TypeScript is initially
distributed from GitHub, not the npm registry.

## HTTP contract

Send `Authorization: Bearer <API_KEY>` to `/api/v1`. The public OpenAPI document is
available at `/api/v1/openapi.json`, without authentication; it includes schemas,
request bodies, errors, scopes and ownership boundaries. The built-in **OpenAPI**
page lists operations and offers the JSON download. Import the JSON into your
preferred OpenAPI documentation viewer or client generator.

Single resources return `{ "data": ... }`. Lists additionally include `total`,
`page` and `page_size`; pages start at 1, default size is 30, maximum 100. Errors
return a non-2xx HTTP status and `{ "error": { "code": "...", "message": "..." } }`.
Numeric path IDs refer to Console database rows, **not RustDesk connection IDs**.

Ordinary accounts can access their own devices and personal address book only.
User management, device changes, device groups and strategies require current
administrator privileges and their matching scopes. Permissions and account
status are checked again for every request: revocation, expiry, disabling an
account, mandatory password change and administrator demotion deny subsequent
requests immediately. The API never returns password hashes, TOTP secrets or
client signing keys. Existing RustDesk client APIs remain unchanged.

# RustDesk Console

![release](https://img.shields.io/badge/release-v0.2.9-blue) ![license](https://img.shields.io/badge/license-MIT-green)

A self-hosted **API server for [RustDesk](https://rustdesk.com)** — user & device
management, address books, audit logs and a built-in admin web UI, all shipped as a
**single self-contained binary**.

It is a from-scratch **Rust rewrite** of [`lejianwen/rustdesk-api`](https://github.com/lejianwen/rustdesk-api),
with its own brand-new admin web interface (not the original web project).

**English** · [中文](README.zh-CN.md)

---

## Features

- **Works with the RustDesk client out of the box** — login, heartbeat, device
  (peer) registration, address books, groups, and the web client config. JSON shapes,
  headers and tokens match the original server, so existing clients need no changes.
- **Admin web console** (served at `/_admin/`) to manage:
  - Users, groups, device groups, tags, devices
  - Address books, collections and share rules
  - OAuth providers, login logs, connection/file audit logs, share records, tokens
  - Light/dark theme, English / 中文
- **Sign-in options**: username/password (with captcha + rate limiting), third-party
  login (GitHub, Google, OIDC, LinuxDO), web SSO, and LDAP.
- **Personal area** for each user to manage their own devices, address book, tags,
  share records and login history.
- **Server commands**: send commands to your RustDesk ID/relay server from the console.
- **Single binary**: the web UI, translations and templates are embedded — nothing
  else to deploy. Works with **SQLite, MySQL or PostgreSQL**; tables are created
  automatically on first run.
- **Observability**: public `/health` and Prometheus-compatible `/metrics` endpoints
  for containers, load balancers and monitoring systems.
- **File upload**: local uploads out of the box, or direct-to-OSS with a signed policy.

## Quick start

### Docker Compose (recommended)

```bash
docker compose up -d --build
```

Then open `http://<host>:21114/_admin/`.

### Pre-built image

```bash
docker run -d --name rustdesk-console -p 21114:21114 \
  -e TZ=Asia/Shanghai \
  -v $PWD/data:/app/data -v $PWD/conf:/app/conf \
  ghcr.io/dockers-x/rustdesk-console:latest
```

Release tags build images for **GHCR** (`ghcr.io/dockers-x/rustdesk-console`).

### From source

```bash
# 1) build the admin web UI (outputs into resources/admin)
cd web && npm install && npx vite build && cd ..
# 2) build the server (embeds the UI)
cargo build --release
./target/release/rustdesk-console -c ./conf/config.yaml
```

### First login

There is no fixed default password. The server listens on `0.0.0.0:21114`; open
`/_admin/` after the first start.

If `admin.password` / `RUSTDESK_API_ADMIN_PASSWORD` is empty, the login page shows
an initial setup wizard. Create the first local administrator there.

If `admin.password` / `RUSTDESK_API_ADMIN_PASSWORD` is set before the first
database seed, the server creates the initial administrator automatically:

- Username: `admin` by default, or `RUSTDESK_API_ADMIN_USERNAME`
- Password: the configured `admin.password` / `RUSTDESK_API_ADMIN_PASSWORD`

If you need to recover access later, reset the password any time:

```bash
rustdesk-console reset-admin-pwd <newpassword>
```

These initial admin settings are applied only when the database is first
initialized; later restarts never overwrite an existing admin user. Set
`admin.force-change-password` / `RUSTDESK_API_ADMIN_FORCE_CHANGE_PASSWORD=true` to
require the environment-created initial admin user to change the password after
the first sign-in.

## Configuration

Settings live in `conf/config.yaml`. **Every** setting can also be provided via an
environment variable named `RUSTDESK_API_<PATH>` — uppercase the key path, join with
`_`, and replace `-` with `_`.

| Setting | Environment variable |
| --- | --- |
| `rustdesk.id-server` | `RUSTDESK_API_RUSTDESK_ID_SERVER` |
| `rustdesk.relay-server` | `RUSTDESK_API_RUSTDESK_RELAY_SERVER` |
| `rustdesk.api-server` | `RUSTDESK_API_RUSTDESK_API_SERVER` |
| `rustdesk.ws-host` | `RUSTDESK_API_RUSTDESK_WS_HOST` |
| `rustdesk.ws-id-host` | `RUSTDESK_API_RUSTDESK_WS_ID_HOST` |
| `rustdesk.ws-relay-host` | `RUSTDESK_API_RUSTDESK_WS_RELAY_HOST` |
| `rustdesk.key` | `RUSTDESK_API_RUSTDESK_KEY` |
| `admin.title` | `RUSTDESK_API_ADMIN_TITLE` |
| `admin.username` | `RUSTDESK_API_ADMIN_USERNAME` |
| `admin.password` | `RUSTDESK_API_ADMIN_PASSWORD` |
| `admin.force-change-password` | `RUSTDESK_API_ADMIN_FORCE_CHANGE_PASSWORD` |
| `db.type` | `RUSTDESK_API_DB_TYPE` (`sqlite` / `mysql` / `postgresql`; default `sqlite`) |
| `db.max-idle-conns` / `db.max-open-conns` | `RUSTDESK_API_DB_MAX_IDLE_CONNS` / `RUSTDESK_API_DB_MAX_OPEN_CONNS` |
| `mysql.addr` / `mysql.dbname` / … | `RUSTDESK_API_MYSQL_ADDR` / `RUSTDESK_API_MYSQL_DBNAME` / … |
| `jwt.key` | `RUSTDESK_API_JWT_KEY` |
| `app.register` | `RUSTDESK_API_APP_REGISTER` |
| `app.disable-pwd-login` | `RUSTDESK_API_APP_DISABLE_PWD_LOGIN` |
| `app.web-sso` | `RUSTDESK_API_APP_WEB_SSO` |
| `record-storage.type` | `RUSTDESK_API_RECORD_STORAGE_TYPE` (`local` / `s3` / `webdav`) |
| `record-storage.local-dir` / `record-storage.temp-dir` | `RUSTDESK_API_RECORD_STORAGE_LOCAL_DIR` / `RUSTDESK_API_RECORD_STORAGE_TEMP_DIR` |
| `record-storage.s3.endpoint` / `bucket` / `prefix` | `RUSTDESK_API_RECORD_STORAGE_S3_ENDPOINT` / `RUSTDESK_API_RECORD_STORAGE_S3_BUCKET` / `RUSTDESK_API_RECORD_STORAGE_S3_PREFIX` |
| `record-storage.s3.access-key-id` / `secret-access-key` | `RUSTDESK_API_RECORD_STORAGE_S3_ACCESS_KEY_ID` / `RUSTDESK_API_RECORD_STORAGE_S3_SECRET_ACCESS_KEY` |
| `record-storage.webdav.url` / `username` / `password` | `RUSTDESK_API_RECORD_STORAGE_WEBDAV_URL` / `RUSTDESK_API_RECORD_STORAGE_WEBDAV_USERNAME` / `RUSTDESK_API_RECORD_STORAGE_WEBDAV_PASSWORD` |
| `logger.path` / `logger.level` | `RUSTDESK_API_LOGGER_PATH` / `RUSTDESK_API_LOGGER_LEVEL` |

Database default is **SQLite** at `./data/rustdeskapi.db`, so `RUSTDESK_API_DB_TYPE`
is normally not needed. Logs go to `logger.path`
(default `./runtime/log.txt`) at `logger.level`, or to stdout if the path is empty.
Container images default to `TZ=Asia/Shanghai`, and `docker-compose.yaml` lets you
override it with the standard `TZ` environment variable. Set it to your IANA time
zone, for example `TZ=Europe/Berlin`, if you do not want Asia/Shanghai local time.
Business timestamps remain stored in UTC; `TZ` is used for server-local logs/start
time and for the admin UI's local-time display.

The embedded WebClient supports three WebSocket modes:

- Leave `rustdesk.ws-host`, `rustdesk.ws-id-host` and `rustdesk.ws-relay-host`
  empty to preserve the original behavior: derive `21118` from
  `rustdesk.id-server` and `21119` from `rustdesk.relay-server`.
- Set `rustdesk.ws-host` for path-based HTTPS/WSS reverse proxy deployments.
  WebClient will use `<ws-host>/ws/id` and `<ws-host>/ws/relay`.
- Set `rustdesk.ws-id-host` and `rustdesk.ws-relay-host` when the ID and relay
  WebSocket services are exposed as separate endpoints or ports, for example
  `wss://rd.example.com:21118` and `wss://rd.example.com:21119`. Explicit
  endpoints take precedence over `rustdesk.ws-host`.

To override the embedded WebClient, put a single `web.zip` at `./data/web.zip` and
restart the service. The server extracts it into a temporary directory at startup,
serves `/webclient/` and `/webclient2/` from that directory first, and falls back to
the embedded resources if the file is missing or invalid. The zip must contain a
root `index.html`; it is not unpacked into the persistent `data` directory.

## OAuth and OIDC

RustDesk clients discover third-party login entries from `GET /api/login-options`.
The server only exposes providers that are complete enough to start login:

- GitHub, Google and LinuxDO require both `client_id` and `client_secret`.
- Generic OIDC requires `client_id`, `client_secret` and `issuer`.
- Web SSO is exposed as `webauth` when `app.web-sso` is enabled.

The callback URL configured in the provider application must be:

```text
<rustdesk.api-server>/api/oidc/callback
```

For Google and generic OIDC, the server reads
`<issuer>/.well-known/openid-configuration`, validates the ID Token signature via
JWKS, and checks issuer, audience, subject and nonce before accepting the login.
Automatic binding to an existing local user by email only happens when the
provider returns a verified email address.

The admin OAuth page includes a provider test action. It checks saved settings,
discovery endpoints and the callback URL without starting a real user login.
The embedded and external WebClient can also start the same RustDesk OAuth flow
through the WebClient `account_auth` JavaScript bridge.

## Recording storage

RustDesk recording upload keeps the client protocol unchanged: `new`, random-offset
`part`, `tail`, and `remove` requests still go to `/api/record`. The server records
the storage backend and object key on each `record_files` row, so downloads and
deletes use the backend/key saved for that recording. Changing storage settings only
affects new recordings.

Supported backends:

- `local`: writes directly to `record-storage.local-dir`, or
  `<gin.resources-path>/record` when empty.
- `s3`: stages chunks in `record-storage.temp-dir` and uploads the completed file
  to `s3.prefix + filename` on `tail`.
- `webdav`: stages chunks in `record-storage.temp-dir` and uploads the completed
  file to `webdav.prefix + filename` on `tail`.

S3 and WebDAV credentials are server-side only. The admin UI never returns saved
secret values; leaving a secret field empty keeps the existing value.

## RustDesk deployment commands

The Client settings page can generate RustDesk deployment commands, including
optional client-side access settings. These preview-only options can add:

```bash
sudo rustdesk --password 'MyStrongPassword'
sudo rustdesk --option approve-mode 'password'
sudo rustdesk --option verification-method 'use-permanent-password'
```

Supported `approve-mode` values are `password`, `click`, and `password-click`.
Supported `verification-method` values are `use-temporary-password`,
`use-permanent-password`, and `use-both-passwords`.

The password and access-mode choices are not written to `conf/config.yaml`, the
database, localStorage, encoded config, or installer filename. Run the generated
commands on the client machine with administrator/root privileges.

## Observability

The server exposes machine-readable observability endpoints without admin login:

| Endpoint | Purpose |
| --- | --- |
| `GET /health` | Liveness/readiness JSON. Returns `200` when the API and database are available, or `503` when the database check fails. |
| `GET /metrics` | Prometheus text metrics with counts for users, peers, online peers, active connections, audit rows, login logs, uptime and database status. |

`rustdesk_console_peers_online` uses peers seen in the last 90 seconds, matching the
RustDesk heartbeat update cadence while avoiding short boundary flaps.

## Web admin UI

The admin console under `web/` is a **brand-new interface we wrote ourselves**
(it does not use the original project's web app). Its production build is embedded into
the binary and served at `/_admin/`.

For UI development:

```bash
cd web
npm install
npm run dev    # dev server; proxies /api to http://127.0.0.1:21114
```

## CLI

```bash
rustdesk-console reset-admin-pwd <newpassword>    # reset the admin (user 1) password
rustdesk-console reset-pwd <userId> <newpassword> # reset any user's password
rustdesk-console -c ./conf/config.yaml            # run the server (default)
```

## Tech stack

Server: Rust · axum · SeaORM (SQLite/MySQL/PostgreSQL) · tokio · JWT · bcrypt ·
rust-embed. Web: React + [Cloudflare kumo](https://github.com/cloudflare/kumo) ·
Vite · TanStack Query.

## Acknowledgements

This project is a Rust port of [`lejianwen/rustdesk-api`](https://github.com/lejianwen/rustdesk-api)
by 乐见文. The admin web UI is our own rewrite. Licensed under MIT.

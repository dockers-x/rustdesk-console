# RustDesk Console

![release](https://img.shields.io/badge/release-v0.2.4-blue) ![license](https://img.shields.io/badge/license-Apache--2.0-green)

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

Images are published to both **GHCR** (`ghcr.io/dockers-x/rustdesk-console`) and
**Docker Hub** on each release.

### From source

```bash
# 1) build the admin web UI (outputs into resources/admin)
cd web && npm install && npx vite build && cd ..
# 2) build the server (embeds the UI)
cargo build --release
./target/release/rustdesk-console -c ./conf/config.yaml
```

### First login

There is no fixed default password. On first start the server creates the default
administrator account:

- Username: `admin`
- Password: random, printed once in the server log
- The generated account can be customized before the first database seed with
  `RUSTDESK_API_ADMIN_USERNAME`, `RUSTDESK_API_ADMIN_PASSWORD`, and
  `RUSTDESK_API_ADMIN_FORCE_CHANGE_PASSWORD`

```text
INFO rustdesk-console: Admin Password Is: <random>
```

The server listens on `0.0.0.0:21114`. Sign in at `/_admin/`. If the initial log
line is no longer available, reset the password any time:

```bash
rustdesk-console reset-admin-pwd <newpassword>
```

When `admin.password` / `RUSTDESK_API_ADMIN_PASSWORD` is empty, the server generates
and logs a random initial password. When it is set, that configured password is used
but is not printed in logs. These initial admin settings are applied only when the
database is first initialized; later restarts never overwrite an existing admin user.
Set `admin.force-change-password` / `RUSTDESK_API_ADMIN_FORCE_CHANGE_PASSWORD=true`
to require that initial admin user to change the password after the first sign-in.

## Configuration

Settings live in `conf/config.yaml`. **Every** setting can also be provided via an
environment variable named `RUSTDESK_API_<PATH>` — uppercase the key path, join with
`_`, and replace `-` with `_`. This matches the original server exactly.

| Setting | Environment variable |
| --- | --- |
| `rustdesk.id-server` | `RUSTDESK_API_RUSTDESK_ID_SERVER` |
| `rustdesk.relay-server` | `RUSTDESK_API_RUSTDESK_RELAY_SERVER` |
| `rustdesk.api-server` | `RUSTDESK_API_RUSTDESK_API_SERVER` |
| `rustdesk.ws-host` | `RUSTDESK_API_RUSTDESK_WS_HOST` |
| `rustdesk.key` | `RUSTDESK_API_RUSTDESK_KEY` |
| `admin.title` | `RUSTDESK_API_ADMIN_TITLE` |
| `admin.username` | `RUSTDESK_API_ADMIN_USERNAME` |
| `admin.password` | `RUSTDESK_API_ADMIN_PASSWORD` |
| `admin.force-change-password` | `RUSTDESK_API_ADMIN_FORCE_CHANGE_PASSWORD` |
| `gorm.type` | `RUSTDESK_API_GORM_TYPE` (`sqlite` / `mysql` / `postgresql`) |
| `mysql.addr` / `mysql.dbname` / … | `RUSTDESK_API_MYSQL_ADDR` / `RUSTDESK_API_MYSQL_DBNAME` / … |
| `jwt.key` | `RUSTDESK_API_JWT_KEY` |
| `app.register` | `RUSTDESK_API_APP_REGISTER` |
| `app.disable-pwd-login` | `RUSTDESK_API_APP_DISABLE_PWD_LOGIN` |
| `logger.path` / `logger.level` | `RUSTDESK_API_LOGGER_PATH` / `RUSTDESK_API_LOGGER_LEVEL` |

Database default is **SQLite** at `./data/rustdeskapi.db`. Logs go to `logger.path`
(default `./runtime/log.txt`) at `logger.level`, or to stdout if the path is empty.
Container images default to `TZ=Asia/Shanghai`, and `docker-compose.yaml` lets you
override it with the standard `TZ` environment variable. Set it to your IANA time
zone, for example `TZ=Europe/Berlin`, if you do not want Asia/Shanghai local time.
Business timestamps remain stored in UTC; `TZ` is used for server-local logs/start
time and for the admin UI's local-time display.

The embedded WebClient derives `21118` from `rustdesk.id-server` and `21119`
from `rustdesk.relay-server` by default. When `rustdesk.ws-host` is set, it uses
`<ws-host>/ws/id` and `<ws-host>/ws/relay` instead, which fits Cloudflare Tunnel
or other HTTPS reverse proxy deployments. Leaving it empty preserves the old
behavior.

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
by 乐见文. The admin web UI is our own rewrite. Licensed under Apache-2.0.

# RustDesk Console (Rust)

A Rust rewrite of [`lejianwen/rustdesk-api`](https://github.com/lejianwen/rustdesk-api) — a
self-hosted API server for [RustDesk](https://rustdesk.com), built with **axum** + **SeaORM**.
The web frontend is **embedded into a single binary**, so deployment is one self-contained
executable.

> 中文说明见下方 [中文文档](#中文文档)。

![rust](https://img.shields.io/badge/rust-1.96-blue)
![axum](https://img.shields.io/badge/axum-0.7-blue)
![sea--orm](https://img.shields.io/badge/SeaORM-1.1-green)

---

## Highlights

- **Single binary** — the Flutter web client, i18n bundles and templates are embedded via
  `rust-embed`. No separate `resources/` directory to ship.
- **Wire-compatible** — JSON keys, headers, token semantics and the response envelope match the
  original Go server, so existing RustDesk clients and the admin web UI work unchanged.
- **Multi-database** — SQLite, MySQL and PostgreSQL from one codebase (SeaORM). Tables are created
  automatically on first run (the equivalent of GORM `AutoMigrate`), and a random admin password is
  generated and logged.
- **Auth** — bcrypt password hashing (with transparent upgrade from the legacy MD5 scheme), JWT,
  device-token auth for the PC client (`Authorization: Bearer`) and the admin panel (`api-token`),
  captcha + login rate-limiting.

## Project status

This is delivered in phases. **Phase 1 (current)** is a working, tested core:

| Area | Status |
| --- | --- |
| RustDesk PC-client API (login, heartbeat, sysinfo, address book, groups, peers, web client, audit) | ✅ implemented |
| Admin auth (login, captcha, logout, config) | ✅ implemented |
| Admin CRUD: user, group, device group, tag, peer; login-log read | ✅ implemented |
| Single-binary frontend embedding, Docker, multi-arch CI | ✅ implemented |
| OAuth / OIDC / LDAP login, web SSO | ⏳ later phase |
| Remaining admin CRUD (oauth, audit, share records, user tokens, address-book collections, server cmd, `my/*`) | ⏳ scaffolded — return a clear `NotImplemented` code until ported |

Endpoints that are not yet ported return `{"code":501,"message":"NotImplemented: <path>"}` and log a
warning, so nothing silently looks "done" when it isn't.

## Quick start

### From source

```bash
cargo build --release -p rustdesk-api-server
./target/release/rustdesk-api-server -c ./conf/config.yaml
```

On first run the admin password is printed to the log:

```
INFO rustdesk_api_server::bootstrap: Admin Password Is: <random>
```

The server listens on `0.0.0.0:21114` by default. The admin UI is served at `/_admin/`, the web
client at `/webclient/`.

### Docker

```bash
docker compose up -d --build
```

The Docker build clones and builds the admin UI from
[`lejianwen/rustdesk-api-web`](https://github.com/lejianwen/rustdesk-api-web) and embeds it into the
binary. Data (SQLite) and config are mounted from `./data` and `./conf`.

### CLI

```bash
rustdesk-api-server reset-admin-pwd <newpassword>     # reset the admin (user 1) password
rustdesk-api-server reset-pwd <userId> <newpassword>  # reset any user's password
```

## Configuration

Config is read from `conf/config.yaml`. Every key can be overridden with an environment variable
named `RUSTDESK_API_<PATH>`, where `<PATH>` is the upper-cased key path joined by `_` and hyphens
replaced by `_` (identical to the original server):

| Config key | Environment variable |
| --- | --- |
| `app.web-client` | `RUSTDESK_API_APP_WEB_CLIENT` |
| `rustdesk.id-server` | `RUSTDESK_API_RUSTDESK_ID_SERVER` |
| `gorm.type` | `RUSTDESK_API_GORM_TYPE` |
| `jwt.key` | `RUSTDESK_API_JWT_KEY` |

Supported `gorm.type` values: `sqlite` (default, file `./data/rustdeskapi.db`), `mysql`,
`postgresql`.

## Architecture

```
crates/
  entity/   SeaORM entities (17 models), ported 1:1 from the Go model/ structs
  server/   axum app, config, bootstrap (connect + migrate + seed), services, HTTP layer, CLI
resources/  Flutter web client + i18n + templates (embedded); admin/ is filled at build time
conf/       default config.yaml
```

Request flow: `router` → extractor-based auth middleware (`RustClientUser` / `BackendUser` /
`AdminUser`) → controller → service → SeaORM entity.

## Tech stack

axum · tokio · SeaORM (sqlx) · jsonwebtoken · bcrypt · rust-embed · clap · tracing · captcha.

## Acknowledgements

This project is a Rust port of [`lejianwen/rustdesk-api`](https://github.com/lejianwen/rustdesk-api);
the admin web UI comes from
[`lejianwen/rustdesk-api-web`](https://github.com/lejianwen/rustdesk-api-web). Licensed under
Apache-2.0.

---

## 中文文档

[RustDesk](https://rustdesk.com) 自建 API 服务的 **Rust 重写版**，基于 **axum + SeaORM**，前端被
**打包进单个二进制文件**，部署时只需一个可执行文件。

### 特性

- **单文件部署**：Flutter Web 客户端、i18n、模板通过 `rust-embed` 嵌入二进制，无需额外的
  `resources/` 目录。
- **协议兼容**：JSON 字段、请求头、令牌语义、响应结构与原 Go 版本一致，现有 RustDesk 客户端与后台
  Web UI 无需改动即可使用。
- **多数据库**：SQLite / MySQL / PostgreSQL 同一套代码（SeaORM）。首次启动自动建表（等价于 GORM
  `AutoMigrate`），并生成并打印一个随机管理员密码。
- **认证**：bcrypt 密码哈希（兼容旧 MD5 并自动升级）、JWT、PC 客户端设备令牌
  （`Authorization: Bearer`）、后台 `api-token`、验证码与登录限流。

### 当前进度（第一阶段）

已实现并通过测试的核心：RustDesk 客户端 API（登录、心跳、系统信息、地址簿、群组、设备、Web
客户端、审计）、后台登录/验证码/配置、用户/群组/设备组/标签/设备的增删改查、登录日志查询、单文件
前端嵌入、Docker 与多架构 CI。

暂未移植：OAuth/OIDC/LDAP 登录、Web SSO，以及部分后台增删改查（oauth、审计管理、分享记录、用户令牌、
地址簿集合、服务器命令、`my/*`）——这些接口会返回明确的 `501 NotImplemented` 并记录日志，留待后续
阶段补全。

### 快速开始

```bash
# 源码编译
cargo build --release -p rustdesk-api-server
./target/release/rustdesk-api-server -c ./conf/config.yaml

# 或使用 Docker（构建时会自动拉取并打包后台 Web UI）
docker compose up -d --build
```

首次启动会在日志中打印管理员密码。默认监听 `0.0.0.0:21114`，后台 UI 位于 `/_admin/`，Web 客户端位于
`/webclient/`。

### 配置

读取 `conf/config.yaml`；任意配置项均可用环境变量 `RUSTDESK_API_<键路径>` 覆盖（大写、`.`/`-` 均转为
`_`），例如 `rustdesk.id-server` 对应 `RUSTDESK_API_RUSTDESK_ID_SERVER`。

### 命令行

```bash
rustdesk-api-server reset-admin-pwd <新密码>     # 重置管理员（用户 1）密码
rustdesk-api-server reset-pwd <用户ID> <新密码>   # 重置指定用户密码
```

### 致谢

本项目是 [`lejianwen/rustdesk-api`](https://github.com/lejianwen/rustdesk-api) 的 Rust 移植版，后台 Web
UI 来自 [`lejianwen/rustdesk-api-web`](https://github.com/lejianwen/rustdesk-api-web)。以 Apache-2.0
协议开源。

# RustDesk Console

![release](https://img.shields.io/badge/release-v0.2.6-blue) ![license](https://img.shields.io/badge/license-Apache--2.0-green)

[RustDesk](https://rustdesk.com) 的**自建 API 服务**——用户与设备管理、地址簿、审计日志，
以及内置的管理后台，全部打包成**一个独立的二进制文件**。

本项目是 [`lejianwen/rustdesk-api`](https://github.com/lejianwen/rustdesk-api) 的
**Rust 完全重写版**，并配有一套**全新自研的管理后台**（不是原项目的 web）。

[English](README.md) · **中文**

---

## 功能

- **开箱即用对接 RustDesk 客户端**：登录、心跳、设备上报、地址簿、群组、web 客户端配置。
  JSON 字段、请求头、令牌都与原版一致，现有客户端无需改动。
- **管理后台**（位于 `/_admin/`）可管理：
  - 用户、群组、设备分组、标签、设备
  - 地址簿、地址簿集合、共享规则
  - OAuth 提供商、登录日志、连接/文件审计、分享记录、登录凭证
  - 明/暗主题，中文 / English
- **登录方式**：账号密码（含验证码 + 失败限流）、第三方登录（GitHub、Google、OIDC、
  LinuxDO）、Web SSO、LDAP。
- **个人中心**：每个用户管理自己的设备、地址簿、标签、分享记录和登录历史。
- **服务器命令**：从后台向 RustDesk ID/中继服务器下发命令。
- **单文件部署**：前端、翻译、模板都嵌入二进制，无需额外文件。支持
  **SQLite / MySQL / PostgreSQL**，首次启动自动建表。
- **可观测性**：提供公开的 `/health` 和 Prometheus 兼容 `/metrics`，方便容器、
  负载均衡和监控系统接入。
- **文件上传**：内置本地上传，也支持带签名策略的 OSS 直传。

## 快速开始

### Docker Compose（推荐）

```bash
docker compose up -d --build
```

然后访问 `http://<主机>:21114/_admin/`。

### 预构建镜像

```bash
docker run -d --name rustdesk-console -p 21114:21114 \
  -e TZ=Asia/Shanghai \
  -v $PWD/data:/app/data -v $PWD/conf:/app/conf \
  ghcr.io/dockers-x/rustdesk-console:latest
```

每次发布都会同时推送到 **GHCR**（`ghcr.io/dockers-x/rustdesk-console`）和 **Docker Hub**。

### 源码编译

```bash
# 1) 构建后台 web（产物输出到 resources/admin）
cd web && npm install && npx vite build && cd ..
# 2) 编译服务（会把前端嵌进去）
cargo build --release
./target/release/rustdesk-console -c ./conf/config.yaml
```

### 首次登录

Web 后台没有固定默认密码。首次启动时服务会创建默认管理员账号：

- 用户名：`admin`
- 密码：随机生成，只在服务启动日志里打印一次
- 首次建库前可通过 `RUSTDESK_API_ADMIN_USERNAME`、`RUSTDESK_API_ADMIN_PASSWORD`
  和 `RUSTDESK_API_ADMIN_FORCE_CHANGE_PASSWORD` 自定义初始化管理员

```text
INFO rustdesk-console: Admin Password Is: <随机密码>
```

服务默认监听 `0.0.0.0:21114`，在 `/_admin/` 登录。如果启动日志已经找不到，可以随时重置密码：

```bash
rustdesk-console reset-admin-pwd <新密码>
```

当 `admin.password` / `RUSTDESK_API_ADMIN_PASSWORD` 为空时，服务会生成随机初始密码并写入日志；
当它有值时，会使用该配置密码，但不会把密码打印到日志里。这些初始化管理员配置只在数据库首次
初始化时生效，后续重启不会覆盖已有管理员。设置
`admin.force-change-password` / `RUSTDESK_API_ADMIN_FORCE_CHANGE_PASSWORD=true`
后，初始化管理员首次登录后必须先修改密码。

## 配置

配置在 `conf/config.yaml`。**任意**配置项都可以用环境变量 `RUSTDESK_API_<键路径>` 覆盖：
键路径大写、用 `_` 连接、`-` 改成 `_`。与原版完全一致。

| 配置项 | 环境变量 |
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
| `gorm.type` | `RUSTDESK_API_GORM_TYPE`（`sqlite` / `mysql` / `postgresql`） |
| `mysql.addr` / `mysql.dbname` / … | `RUSTDESK_API_MYSQL_ADDR` / `RUSTDESK_API_MYSQL_DBNAME` / … |
| `jwt.key` | `RUSTDESK_API_JWT_KEY` |
| `app.register` | `RUSTDESK_API_APP_REGISTER` |
| `app.disable-pwd-login` | `RUSTDESK_API_APP_DISABLE_PWD_LOGIN` |
| `logger.path` / `logger.level` | `RUSTDESK_API_LOGGER_PATH` / `RUSTDESK_API_LOGGER_LEVEL` |

数据库默认是 **SQLite**，文件 `./data/rustdeskapi.db`。日志写入 `logger.path`
（默认 `./runtime/log.txt`），级别为 `logger.level`；路径为空则输出到控制台。
容器镜像默认设置 `TZ=Asia/Shanghai`，`docker-compose.yaml` 也支持用标准 `TZ`
环境变量覆盖。非中国时区部署时可改成对应 IANA 时区，例如 `TZ=Europe/Berlin`。
业务时间戳仍按 UTC 保存；`TZ` 用于服务端本地日志/启动时间，以及管理后台界面的本地时间显示。

内嵌 WebClient 支持三种 WebSocket 配置方式：

- `rustdesk.ws-host`、`rustdesk.ws-id-host`、`rustdesk.ws-relay-host` 都留空时，
  保持原有行为：从 `rustdesk.id-server` 推导 `21118`，从
  `rustdesk.relay-server` 推导 `21119`。
- 设置 `rustdesk.ws-host` 时，适合基于路径的 HTTPS/WSS 反向代理。WebClient 会使用
  `<ws-host>/ws/id` 和 `<ws-host>/ws/relay`。
- 当 ID 和 Relay WebSocket 通过不同端口或不同端点暴露时，设置
  `rustdesk.ws-id-host` 和 `rustdesk.ws-relay-host`，例如
  `wss://rd.example.com:21118` 和 `wss://rd.example.com:21119`。显式端点优先于
  `rustdesk.ws-host`。

## 可观测性

服务提供无需后台登录的机器可读接口：

| 接口 | 用途 |
| --- | --- |
| `GET /health` | 存活/就绪 JSON。API 和数据库可用时返回 `200`，数据库检查失败时返回 `503`。 |
| `GET /metrics` | Prometheus 文本指标，包含用户数、设备数、在线设备数、活动连接、审计记录、登录日志、运行时长和数据库状态。 |

`rustdesk_console_peers_online` 统计最近 90 秒内上报过心跳的设备，和 RustDesk
心跳更新节奏保持一致，同时避免边界时间导致监控短暂抖动。

## 管理后台

`web/` 下的后台是**我们自己重写的全新界面**（没有使用原项目的 web）。其生产构建会被嵌入
二进制并在 `/_admin/` 提供。

本地开发：

```bash
cd web
npm install
npm run dev    # 开发服务器；/api 反向代理到 http://127.0.0.1:21114
```

## 命令行

```bash
rustdesk-console reset-admin-pwd <新密码>      # 重置管理员（用户 1）密码
rustdesk-console reset-pwd <用户ID> <新密码>    # 重置任意用户密码
rustdesk-console -c ./conf/config.yaml         # 启动服务（默认）
```

## 技术栈

服务端：Rust · axum · SeaORM（SQLite/MySQL/PostgreSQL）· tokio · JWT · bcrypt ·
rust-embed。前端：React + [Cloudflare kumo](https://github.com/cloudflare/kumo) ·
Vite · TanStack Query。

## 致谢

本项目是 [`lejianwen/rustdesk-api`](https://github.com/lejianwen/rustdesk-api)（作者：乐见文）
的 Rust 移植版，管理后台为自研重写。以 Apache-2.0 协议开源。

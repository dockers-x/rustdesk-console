# RustDesk Console

![release](https://img.shields.io/badge/release-v0.2.44-blue) ![license](https://img.shields.io/badge/license-MIT-green)

[RustDesk](https://rustdesk.com) 的**自建 API 服务**——用户与设备管理、地址簿、审计日志，
以及内置的管理后台，全部打包成**一个独立的二进制文件**。

本项目是 [`lejianwen/rustdesk-api`](https://github.com/lejianwen/rustdesk-api) 的
**Rust 完全重写版**，并配有一套**全新自研的管理后台**（不是原项目的 web）。

[English](README.md) · **中文**

---

## 功能

- **开发者接入**：独立分类提供可撤销、有权限范围及有效期的 API 密钥、Rust/Go/Node.js SDK 示例，以及公开的 `/api/v1/openapi.json`；详见[接入指南](docs/developer-access.zh-CN.md)。

- **开箱即用对接 RustDesk 客户端**：登录、心跳、设备上报、地址簿、群组、web 客户端配置。
  JSON 字段、请求头、令牌都与原版一致，现有客户端无需改动。
- **管理后台**（位于 `/_admin/`）可管理：
  - 用户、群组、设备分组、标签、设备
  - 地址簿、地址簿集合、共享规则
  - OAuth 提供商、登录日志、连接/文件审计、分享记录、登录凭证
  - 明/暗主题，中文 / English / Русский
- **登录方式**：账号密码（含验证码 + 失败限流）、第三方登录（GitHub、Google、OIDC、
  LinuxDO）、Web SSO、LDAP。
- **个人中心**：每个用户管理自己的设备、地址簿、标签、分享记录和登录历史。
- **服务器命令**：从后台向 RustDesk ID/中继服务器下发命令。
- **Geo Relay 路由**：在现有管理后台中维护有序的国家、城市、子区域、ASN、运营商规则，
  并下载和管理可选 MMDB。
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
docker run -d --name rustdesk-console -p 127.0.0.1:21114:21114 \
  -e TZ=Asia/Shanghai \
  -v $PWD/data:/app/data -v $PWD/conf:/app/conf \
  ghcr.io/dockers-x/rustdesk-console:latest
```

发布 tag 会构建并推送 **GHCR** 镜像（`ghcr.io/dockers-x/rustdesk-console`）。

### 源码编译

```bash
# 1) 构建后台 web（产物输出到 resources/admin）
cd web && npx pnpm@10.34.0 install --frozen-lockfile && npx pnpm@10.34.0 run build && cd ..
# 2) 编译服务（会把前端嵌进去）
cargo build --release
./target/release/rustdesk-console -c ./conf/config.yaml
```

### 首次登录

Web 后台没有固定默认密码。服务默认监听 `0.0.0.0:21114`，首次启动后打开
`/_admin/`。

如果 `admin.password` / `RUSTDESK_API_ADMIN_PASSWORD` 为空，登录页会显示初始化向导，
在页面里创建第一个本地管理员。

如果首次建库前设置了 `admin.password` / `RUSTDESK_API_ADMIN_PASSWORD`，服务会自动创建
初始化管理员：

- 用户名：默认 `admin`，可用 `RUSTDESK_API_ADMIN_USERNAME` 覆盖
- 密码：配置的 `admin.password` / `RUSTDESK_API_ADMIN_PASSWORD`

后续如果需要恢复访问，可以随时重置密码：

```bash
rustdesk-console reset-admin-pwd --password-file /path/to/console-password
```

这些初始化管理员配置只在数据库首次初始化时生效，后续重启不会覆盖已有管理员。设置
`admin.force-change-password` / `RUSTDESK_API_ADMIN_FORCE_CHANGE_PASSWORD=true` 后，
通过环境变量创建的初始化管理员首次登录后必须先修改密码。

## 配置

配置在 `conf/config.yaml`。**任意**配置项都可以用环境变量 `RUSTDESK_API_<键路径>` 覆盖：
键路径大写、用 `_` 连接、`-` 改成 `_`。

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
| `db.type` | `RUSTDESK_API_DB_TYPE`（`sqlite` / `mysql` / `postgresql`；默认 `sqlite`） |
| `db.max-idle-conns` / `db.max-open-conns` | `RUSTDESK_API_DB_MAX_IDLE_CONNS` / `RUSTDESK_API_DB_MAX_OPEN_CONNS` |
| `mysql.addr` / `mysql.dbname` / … | `RUSTDESK_API_MYSQL_ADDR` / `RUSTDESK_API_MYSQL_DBNAME` / … |
| `jwt.key` | `RUSTDESK_API_JWT_KEY` |
| `app.register` | `RUSTDESK_API_APP_REGISTER` |
| `app.disable-pwd-login` | `RUSTDESK_API_APP_DISABLE_PWD_LOGIN` |
| `app.web-sso` | `RUSTDESK_API_APP_WEB_SSO` |
| `record-storage.type` | `RUSTDESK_API_RECORD_STORAGE_TYPE`（`local` / `s3` / `webdav`） |
| `record-storage.local-dir` / `record-storage.temp-dir` | `RUSTDESK_API_RECORD_STORAGE_LOCAL_DIR` / `RUSTDESK_API_RECORD_STORAGE_TEMP_DIR` |
| `record-storage.s3.endpoint` / `bucket` / `prefix` | `RUSTDESK_API_RECORD_STORAGE_S3_ENDPOINT` / `RUSTDESK_API_RECORD_STORAGE_S3_BUCKET` / `RUSTDESK_API_RECORD_STORAGE_S3_PREFIX` |
| `record-storage.s3.access-key-id` / `secret-access-key` | `RUSTDESK_API_RECORD_STORAGE_S3_ACCESS_KEY_ID` / `RUSTDESK_API_RECORD_STORAGE_S3_SECRET_ACCESS_KEY` |
| `record-storage.webdav.url` / `username` / `password` | `RUSTDESK_API_RECORD_STORAGE_WEBDAV_URL` / `RUSTDESK_API_RECORD_STORAGE_WEBDAV_USERNAME` / `RUSTDESK_API_RECORD_STORAGE_WEBDAV_PASSWORD` |
| `logger.path` / `logger.level` | `RUSTDESK_API_LOGGER_PATH` / `RUSTDESK_API_LOGGER_LEVEL` |

数据库默认是 **SQLite**，文件 `./data/rustdeskapi.db`，通常不需要设置
`RUSTDESK_API_DB_TYPE`。日志写入 `logger.path`
（默认 `./runtime/log.txt`），级别为 `logger.level`；路径为空则输出到控制台。
容器镜像默认设置 `TZ=Asia/Shanghai`，`docker-compose.yaml` 也支持用标准 `TZ`
环境变量覆盖。非中国时区部署时可改成对应 IANA 时区，例如 `TZ=Europe/Berlin`。
业务时间戳仍按 UTC 保存；`TZ` 用于服务端本地日志/启动时间，以及管理后台界面的本地时间显示。

### 设备上线通知

在「通知路由」配置 ntfy、Bark、Server 酱或 Webhook，支持多渠道、可编辑模板和测试发送。详见[设备通知说明](docs/notification-routing.zh-CN.md)。

### 认证与安全日志

升级后，管理员需要重新登录。尚未绑定 TOTP 的管理员须先在 Console 完成绑定。
验证器丢失时，可在有 Console 数据库访问权的主机上运行
`rustdesk-console -c ./conf/config.yaml reset-totp <用户 ID>`，再登录并重新绑定。

认证和敏感管理操作以 `security_audit` 目标输出 JSON 到 stdout，独立于
`logger.level` 和 `RUST_LOG`。请将事件归集到容器外。反代访问日志记录上游响应的
`X-Request-Id` 和不带查询串的路径。Compose 示例只向回环地址发布端口。反代和 Console
同在 Docker 网络时，可以取消端口发布，让反代访问服务名，并在 `gin.trust-proxy`
填入实际代理 IP 或 CIDR。详情见[安全升级说明](docs/security-upgrade.zh-CN.md)。


### Geo Relay 数据目录

Geo 规则和 MMDB 下载地址保存在 Console 数据库中。Console 约定使用
`rustdesk.key-file` 的父目录作为与 HBBS/HBBR 共享的数据目录，HBBS 会在该目录发现以下
可选文件：

- `GeoLite2-Country.mmdb`
- `GeoLite2-City.mmdb`
- `GeoLite2-ASN.mmdb`

如果 Console 与 HBBS 运行在不同容器中，需要把同一个宿主机目录挂载给两个容器，并让
`rustdesk.key-file` 指向该挂载目录中的公钥。两个容器内的挂载路径可以不同，但公钥和
MMDB 必须对应同一个宿主机目录。MMDB 缺失或损坏不会阻止 HBBS 启动，也不会影响原有
Relay 池。Console 仅在下发配置时创建短期 `.geo-config-*.json` 临时文件，收到 HBBS
响应后即删除。
自动更新默认关闭。启用后，Console 每 15 分钟检查一次，仅在配置的更新间隔到期后下载；
每次替换前仍会执行完整的 MMDB 完整性和数据库类型校验。

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

如需覆盖内置 WebClient，在 `./data/web.zip` 放一个 zip 文件并重启服务即可。
服务启动时会把它解压到临时目录，通过 `/webclient2/` 提供访问；`/webclient/` 仍使用
内置 v1 客户端。文件不存在或无效时，`/webclient2/` 不可用，管理界面回退到 v1 链接。
zip 支持两种结构：根目录直接包含 `index.html` 及其资源，或包含 `web/` 目录，里面放
`index.html` 及其资源。两者同时存在时优先使用根目录。两种结构都通过 `/webclient2/`
访问，URL 无需额外添加 `web/`。服务不会将其解压到持久化的 `data` 目录。

## LDAP 登录

打开 **系统设置 → LDAP** 配置目录地址、绑定凭据、TLS 信任、搜索范围和用户属性映射。
“测试连接”会校验表单并向目录发起绑定，无需保存设置或创建用户。数据库只保存覆盖项，
其余字段继续读取服务器配置文件和环境变量。更改目录地址、绑定 DN 或 TLS 信任设置时，
请重新输入绑定密码。

请使用带 StartTLS 的 LDAP 或启用证书验证的 LDAPS。若使用私有 CA，可在控制台填写 PEM
证书，或在 YAML 中设置 `ldap.tls-ca-file`。OpenLDAP 使用 `entryUUID`，Active Directory
使用 `objectGUID`。LDAP 初次登录创建普通用户，角色在控制台管理。若设置了允许组，
`ldap.user.admin-group` 的成员也可登录。

旧版本创建的 LDAP 账号没有记录目录身份。升级会将这些记录保留为本地账号，其角色及
TOTP 设置均保留。本地管理员可以为这类账号设置控制台密码。如果用户需要用原用户名通过
LDAP 登录，请先重命名旧账号，再单独核对目录新账号的权限。请保留一个本地管理员账号以便恢复。

## OAuth 与 OIDC

RustDesk 客户端会通过 `GET /api/login-options` 获取第三方登录入口。服务端只会暴露
配置完整、可以发起登录的提供商：

- GitHub、Google、LinuxDO 需要同时配置 `client_id` 和 `client_secret`。
- 通用 OIDC 需要配置 `client_id`、`client_secret` 和 `issuer`。
- 开启 `app.web-sso` 后，会额外暴露 `webauth` 入口。

在「个人中心 → 登录方式」绑定自己的第三方账号。完成提供商授权后会回到 Console，
绑定会沿用当前账号的角色和本地二次验证策略。也可以在同一处解绑，账号必须保留至少一种
可用的登录方式。邮箱仅用于显示或在明确允许建号时填写新账号资料。

默认只允许已绑定的身份登录。若管理员同时开启提供商的「自动注册」和站点注册，
站点已有管理员时才可新建普通用户。部署配置提供的 OIDC 始终要求先绑定。

Google 和通用 OIDC 通过发现文档和 JWKS 验证 ID Token 的签名、issuer、audience、
subject 和 nonce。默认回调为 `<rustdesk.api-server>/api/oidc/callback`。

容器可通过通用环境变量配置一个 OIDC 提供商，密钥留在部署配置中：

| 环境变量 | 用途 |
| --- | --- |
| `RUSTDESK_API_OIDC_CLIENT_ID` | OpenID Connect Client ID |
| `RUSTDESK_API_OIDC_CLIENT_SECRET` | OpenID Connect Client Secret |
| `RUSTDESK_API_OIDC_ISSUER` | 与发现文档中 issuer 完全一致的 URL |
| `RUSTDESK_API_OIDC_NAME` | 登录方式名称，默认为 `OIDC` |
| `RUSTDESK_API_OIDC_REDIRECT_URL` | 可选的完整回调 URL，需与提供商登记值一致 |
| `RUSTDESK_API_OIDC_SCOPES` | 可选的逗号分隔 scope，默认为 `openid,profile,email` |

在 Console 的网页地址与回调地址同源时发起绑定。若管理员启用了本地 2FA，
登录 Console 时依然执行二次验证。原版桌面客户端可通过 Webauth 由 Console 批准登录。

后台 OAuth 页面提供“测试”操作，可以检查已保存配置、发现端点和回调地址，不会发起真实
用户登录。内置 WebClient 和外部 `web.zip` WebClient 也支持通过 WebClient 的
`account_auth` JavaScript 桥接发起同一套 RustDesk OAuth 流程。

## 录像存储

RustDesk 录像上传协议保持不变：客户端仍通过 `/api/record` 发送 `new`、随机
offset 的 `part`、`tail` 和 `remove`。服务端会在每条 `record_files` 记录里保存
存储后端和对象 key，因此下载、删除时按该记录保存的类型和 key 读取；切换存储配置
只影响新的录像。

支持的后端：

- `local`：直接写入 `record-storage.local-dir`，留空则写入
  `<gin.resources-path>/record`。
- `s3`：分片上传期间先暂存到 `record-storage.temp-dir`，收到 `tail` 后上传到
  `s3.prefix + filename`。
- `webdav`：分片上传期间先暂存到 `record-storage.temp-dir`，收到 `tail` 后上传到
  `webdav.prefix + filename`。

S3 和 WebDAV 凭据只在服务端保存。管理后台不会回显已保存的密钥；密钥输入框留空表示
保留当前值。

## RustDesk 部署命令

客户端设置页面可以生成 RustDesk 部署命令，并可选生成客户端接入模式相关命令，例如：

```bash
sudo rustdesk --password 'MyStrongPassword'
sudo rustdesk --option approve-mode 'password'
sudo rustdesk --option verification-method 'use-permanent-password'
```

`approve-mode` 支持 `password`、`click`、`password-click`。
`verification-method` 支持 `use-temporary-password`、`use-permanent-password`、
`use-both-passwords`。

固定密码和接入模式选择只用于本次命令预览，不会写入 `conf/config.yaml`、数据库、
localStorage、导入配置串或安装包文件名。生成的命令需要在客户端本机以管理员/root
权限执行。

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
npx pnpm@10.34.0 install --frozen-lockfile
npx pnpm@10.34.0 run dev    # 开发服务器；/api 反向代理到 http://127.0.0.1:21114
```

## 命令行

```bash
rustdesk-console version --json
rustdesk-console config validate
rustdesk-console database status --json
rustdesk-console database migrate --json
rustdesk-console reset-admin-pwd --password-file /path/to/console-password
rustdesk-console reset-pwd 42 --password-stdin < /path/to/console-password
rustdesk-console -c ./conf/config.yaml
```

数据库状态、退出码和账号恢复流程见[本地运维命令](docs/local-maintenance.zh-CN.md)。

## 技术栈

服务端：Rust · axum · SeaORM（SQLite/MySQL/PostgreSQL）· tokio · JWT · bcrypt ·
rust-embed。前端：React + [Cloudflare kumo](https://github.com/cloudflare/kumo) ·
Vite · TanStack Query。

## 致谢

本项目是 [`lejianwen/rustdesk-api`](https://github.com/lejianwen/rustdesk-api)（作者：乐见文）
的 Rust 移植版，管理后台为自研重写。以 MIT 协议开源。

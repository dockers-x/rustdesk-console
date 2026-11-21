# Console 与 Server S6 改进计划

> 实现时可使用 `subagent-driven-development` 并行推进 Console 审计与 Server 运维，或使用 `executing-plans` 逐项执行。任务清单记录实现与验收进度。

**目标**　借鉴 SCTGDesk 的连接报表、服务就绪检查和备份流程，覆盖 Console、HBBS/HBBR 与联合部署的 S6 镜像。

**架构**　Console 继续使用 Axum、SeaORM 和 React/Kumo。Server 继续运行独立的 HBBS 和 HBBR，S6 通过本机接口汇总就绪状态。备份以本地 CLI 和离线恢复为入口。两个仓库独立发布，S6 固定经过验证的 Console 镜像摘要。

**比较基线**　SCTGDesk `7c265b1`，Console `f50d8e5`，Server `b7149ea`。本文件同时定义功能范围与验收条件。

**许可**　SCTGDesk 和当前 Server 使用 AGPL-3.0，Console 使用 MIT。Console 根据行为和接口独立实现。Server 如采纳 SCTGDesk 的源码，应保留相应许可证和著作权声明，并在发布时提供对应源码。

## 第一批

### 1. Console 连接历史与审计导出

**现状**　连接审计已经保存客户端提交的备注，当前列表主要按设备 ID 和来源设备筛选。SCTGDesk 提供日期和关键词过滤、会话耗时、备注展示与 CSV。这些功能可以利用当前客户端已经提交的记录。

**改动位置**　`crates/server/src/services/audit.rs`、`crates/server/src/http/admin.rs`、`crates/server/src/http/router.rs`、`web/src/resource/registry.tsx` 和审计页面组件。

- [x] 在现有连接审计列表支持可选的 `q`、`from`、`to`，保留原设备过滤参数。以 Console 站点时区解释日期边界，再转换为 UTC 查询。关键词覆盖现有设备标识、来源名称、IP 和备注。
- [x] 在列表突出设备、连接开始时间、耗时和备注摘要，详情展示完整字段。缺少结束时间且无近期在线快照的旧记录显示为「结束时间未知」。报告会话备注沿用已存储记录。客户端备注沿用原有上报，保持只读。
- [x] 为连接审计和文件审计增加 CSV 导出。导出沿用列表的筛选和权限，明确提示单次导出上限。服务端处理转义及电子表格公式前缀。文件审计继续标明缺失方向、缺失大小和仅覆盖部分文件的大小。
- [x] 验证列表与导出筛选一致，带日期边界、换行、引号、中文及公式前缀的记录能安全导出。在桌面和 390px 窄屏检查 Kumo 筛选控件、详情与操作按钮。

来源见 SCTGDesk 的 `crates/api-server/src/lib.rs` 中 `audit_conn_list` 和 `web/console/src/components/ConnectionsCard.vue`。

### 2. Server 就绪检查和结构化日志

**现状**　`rustdesk-utils doctor` 已提供端口检查，Console `/health` 已检查数据库，诊断页也有 ID 与 Relay 命令端口探测。S6 的 Docker healthcheck 目前只检查 HBBS/HBBR 的进程状态。

**改动位置**　Server 的 `src/main.rs`、`src/hbbr.rs`、`src/rendezvous_server.rs`、`src/relay_server.rs`、`docker/rootfs/usr/bin/healthcheck.sh`，以及 Console 的 `crates/server/src/services/diagnostics.rs` 和诊断页。

- [x] 为 HBBS/HBBR 增加可选的本机 HTTP `/livez` 和 `/readyz`。存活表示进程仍能响应。就绪依据已绑定的服务监听器与核心运行状态。未就绪返回 503。S6 内部启用，使用有界超时和并发。
- [x] S6 的容器健康检查汇总 HBBS、HBBR 与启用时的 Console `/health`。Console 单独部署时保留已有诊断路径。后台诊断显示具体失败组件。
- [x] HBBS/HBBR 提供可选的 `LOG_FORMAT=json`，保留文本默认格式。统一时间、级别、组件和事件字段，避免把认证凭据和报文内容写入结构化日志。
- [x] 用本地启动、正常运行、单组件停止和恢复场景验证 200/503 与容器健康状态。检查 S6 的默认安装与分开运行的部署方式。

来源见 SCTGDesk 的 `src/health.rs` 和 `src/json_log.rs`。

## 第二批

### 3. Console 与 S6 本地备份和恢复

**现状**　Console 已有本地运维 CLI。SCTGDesk 使用 SQLite 一致快照并生成归档清单。当前 S6 的 Console SQLite 与 HBBS SQLite 分开存放，还需覆盖配置和密钥。

- [x] 在 Console 运维 CLI 加入 `database backup` 的 SQLite 一致快照。输出版本、数据库 schema、文件校验和及创建时间。MySQL/PostgreSQL 使用各自的备份工具，并由状态输出明确区分后端。
- [x] 在 S6 增加本地备份入口和恢复说明。SQLite 全量恢复包在写入服务停止后采集两套数据库、配置和恢复所需的密钥。归档包含清单，以受限权限写入。录屏文件和外部对象存储按独立数据集列出。
- [x] 验证持续写入时的 Console SQLite 快照可打开。用隔离 S6 环境恢复整套备份，核对账号、地址簿、版本、密钥指纹和连接行为。第一版的入口放在本地运维命令。

- [x] 支持 Console 一键或按间隔创建快照，独立上传至多个 S3 和 WebDAV 目标，诊断页显示每个目标最新结果。S6 离线归档复用目标配置与上传命令。验证 S3 签名、WebDAV 认证、单目标 503 或 307、快照哈希及 SQLite 完整性。

来源见 SCTGDesk 的 `crates/api-server/src/backup.rs` 和 `crates/api-state/src/database.rs`。

## 独立验证项

- [ ] **Server TCP/WSS KX v1。** 当前 Server 已有加密握手。SCTGDesk 增加版本协商和双向独立密钥，可借鉴其协议测试。先用原版 RustDesk 1.5.1、1.5.3 和现用自签客户端检查 TCP、WSS、经 HBBR 转发及旧握手兼容，再决定是否加入协议选项。SCTGDesk 的浏览器工作流基于上游 1.4.0 的 Web Client v1，官方 Web Client v2 授权兼容不属于这项验证。
- [ ] **Console 私有客户端分发。** 可以给自定义客户端配置增加按平台和架构的下载页，并校验发布包的版本与摘要。客户端内的更新检查需要配套构建支持，因为 SCTGDesk 也通过客户端补丁改用私有 API。优先验证下载页和更新提示，再评估签名更新流程。
- [ ] **Server 安装包。** 有裸机安装需求时，可评估 SCTGDesk 的 nfpm、systemd 与 deb/rpm 迁移脚本。用临时系统验证安装、升级、卸载以及现有数据库和密钥的保留。

## 可增列的产品能力

### 4. 有方向的跨组连接授权

SCTGDesk 的组编辑页分别展示「能连接到哪些组」和「哪些组能连到本组」。当前 Console 已有组目录、地址簿共享和委派管理。启用强制登录（`MUST_LOGIN=Y`）后，配套 HBBS 向 Console 查询连接权限，目前设备管理员或所有者可以通过检查。可以在此基础上增加独立的显式授权规则。

- [x] 在 Console 提供「来源用户组 → 目标设备组」授权和生效预览，默认沿用现有权限。使用现有 Kumo 组选择组件，预览说明匹配的规则和实际限制。
- [x] 复用 `crates/server/src/http/security.rs` 的实时授权路径。配套 HBBS 在建立新的打洞和中继路由时执行检查。目录可见性和地址簿共享独立于连接授权。
- [x] 验证授权单向生效、撤销后新连接被拒绝、组变更后权限一致，以及禁用设备和账号仍被拒绝。文档说明 IP 直连和已建立会话的边界。

来源见 SCTGDesk 的 `web/console/src/components/GroupAccessFields.vue`。此候选可复用我们现有的协议和服务端授权路径，按需安排在审计改进之后。

### 5. 我的连接历史

SCTGDesk 使用记录中的账户归属，让普通用户只查询自己的连接历史。当前 Console 的 `audit_conns` 记录设备和会话，缺少可直接用于个人查询的账户归属字段。

- [x] 验证配套 Server 是否能将已认证的发起者及目标设备与后续会话记录可靠关联。保存发生时的账户归属，避免用设备当前所有者倒推历史。
- [ ] 归属可核实时，增加「我的连接历史」的只读入口，沿用日期、备注和详情界面。匿名上报和无法验证归属的历史记录继续遵守原审计权限。
- [ ] 验证两位用户连接同一设备、设备换绑、会话 ID 重用和客户端重试时，个人查询始终返回本人记录。

运行时审核确认，HBBS 授权与客户端审计上报之间没有可信的共同会话标识。
个人历史需要配套客户端回传认证后的会话关联标识，再持久化连接发生时的账号归属。

## 可提前并入的工程改进

- [x] **真实进程回归测试。** 借鉴 SCTGDesk 跑实际 `hbbs` 二进制的集成测试。用临时数据目录、独立端口启动 Console、HBBS 和 HBBR，覆盖自定义 API 端口、Console 不可用时及时拒绝新连接、重启后有效登录会话可用。保留原版客户端的独立验收。
- [ ] **可核验的发布与上游同步。** Console 和 Server 的二进制归档附上 SHA-256 清单，发布时校验实际上传的文件。把 Console 提交、Server 提交、内嵌镜像摘要写入发布记录。Server 上游更新按发布标签合并，用临时分支运行上述回归，再交付合并结果。SCTGDesk 的上游同步脚本、包校验和以及 GitHub 构建证明可作参考。
- [x] **可关闭的 Server 版本检查。** 当前 HBBS 启动后会向上游查询版本。给私有部署提供显式关闭选项，关闭后验证该任务不会产生外部请求。如需展示本项目的可更新版本，单独采用本项目的发布源和 S6 版本号。

## 联合验收

- [x] Console 运行 `cargo test --workspace --locked`，前端运行 `pnpm test` 和 `pnpm build`。验证 SQLite、MySQL/MariaDB 和 PostgreSQL 的审计筛选与导出。
- [ ] Server 运行 `cargo test --locked --all-features`。用两台测试客户端在同一套 S6 完成连接、断开、文件传输，确认 Console 的新报表与服务就绪状态符合实际。
- [ ] 实施并通过验收后，先发布 Console 的 amd64 和 arm64 镜像，再固定已验证的 manifest 摘要构建 S6。读回两个 S6 架构的发布清单，核对内置 Console，验证版本标签与 `latest`。

## v0.2.52 验收记录

- Console 327 项 Rust 测试通过，2 项依赖外部环境的既有测试忽略。前端通过 TypeScript 检查、11 项单测及生产构建。在桌面和 390px 使用浏览器检查登录减弱动效、审计导出、跨组授权和远程备份，覆盖中文、英文和俄文界面。
- SQLite、PostgreSQL 18 和 MariaDB 13 的隔离环境通过连接及文件审计筛选与导出验证。
- Server 测试与真实进程探针通过。验证 Console 重启前后同一会话的路由结果，以及 Console 暂停时拒绝新连接。Docker 运行态和两台原版客户端的完整连接、文件传输仍需独立实测。
- KX v1 的源码对照已完成。客户端实机矩阵、私有客户端更新链路和裸机包升级都需要独立验证。

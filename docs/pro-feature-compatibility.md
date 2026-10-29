# 现有客户端的管理能力与 Pro 功能边界

核对日期：2026-10-29。客户端基线及 HTTP 细节见
[`rustdesk-client-http-api-contract.md`](rustdesk-client-http-api-contract.md)。

本项目是独立实现，不是官方商业服务或授权的替代品。以下“已有”指代码链路
存在，不代表所有第三方身份源、网络拓扑或远程会话都已完成部署验收。
默认继续使用现有标准客户端支持的协议，以及 Console、hbbs/hbbr 能实现的
服务端能力，不要求修改客户端。另有默认关闭的“自有签名客户端”功能，允许
用户为自己的 fork 生成或导入独立密钥，签名配置并准备品牌构建资源；用户
自行替换源码验签公钥、构建和分发。它不提供在线客户端构建，也不让官方
二进制信任用户的签名。完整流程见 [`self-signed-client.zh-CN.md`](self-signed-client.zh-CN.md)。

参考官方公开功能清单与策略语义：

- https://rustdesk.com/docs/en/self-host/rustdesk-server-pro/
- https://rustdesk.com/docs/en/self-host/rustdesk-server-pro/strategy/
- https://rustdesk.com/pricing

## 能力矩阵

| 能力 | 当前状态 | 约束与实现位置 |
| --- | --- | --- |
| 账户、设备、用户组、设备组管理 | 已有 | `http/admin.rs`、`http/pro_api.rs`、`services/group.rs`。目录可见性不等于已知设备 ID 的连接权限。 |
| 个人/共享地址簿、标签、跨组共享 | 已有 | `services/address_book.rs`、`services/tag.rs`。按读/写/完全控制规则检查，标签引用事务同步。 |
| 浏览器共享链接、到期、凭据 | 已有 | `services/share_record.rs`、`WebClientActions.tsx`；实际连接仍依赖可用浏览器运行时和 WSS 部署。 |
| TOTP、邮件、登录设备验证 | 已有 | `services/login_security.rs`；此次加强禁用账户、封禁与密码重置挑战处理。 |
| OIDC/SSO、GitHub/Google/LinuxDO | 已有代码，需真实身份源验收 | `services/oauth.rs`；issuer、audience、nonce、JWKS 校验存在。 |
| LDAP | 已有代码，需真实目录验收 | `services/ldap.rs`；保留当前显式本地密码回退策略，不声称完全等价于任意企业目录部署。 |
| 登录/连接/文件/告警审计 | 已有 | `services/audit*.rs`；重试事务去重，空成功响应，告警可查询。客户端上报不是不可抵赖、无遗漏的网络侧审计。 |
| 录制上传与存储 | 服务端接口及存储已有 | `services/record_storage.rs` 支持本地、S3、WebDAV；当前客户端 `record_upload.rs` 的上传开关默认关闭，不能据此声称自动上传可用，也不能主动取回控制端工具栏录在本地的文件。 |
| 管理台操作审计 | 尚未实现 | `/api/audits/console` 仍是空结果兼容入口，不应和真实连接/文件/告警日志混为一谈。 |
| 集中策略与安全设置同步 | 已有 | `services/strategy.rs` 和客户端 heartbeat。增量合并，不自动回滚原值；不是不可修改的客户端锁定。 |
| 信任设备设置 | 已有设置分发 | `enable-trusted-devices` 使用客户端能力，与 Console 的登录设备验证不是同一信任体系。 |
| 强制登录后连接 | 已加固，仍有边界 | 此 hbbs fork 在 `MUST_LOGIN` 开启时检查普通及切换的打洞、中继协调请求：验证共享密钥签发的 JWT，或接受有效的目标绑定切换授权。JWT 密钥为空时拒绝 token 路径；离线验签仍不能实时查询撤销和目标权限，直连 IP 不经过此门禁。 |
| 强制退出、撤销 token | Console API 有效，连接侧不完整 | API 删除 token 后拒绝后续请求；hbbs 离线 JWT 在到期前仍可能有效，已建立会话也不会因此立即中断。 |
| 管理员断开连接 | 已有协作链路 | Console 排队 → heartbeat → 客户端断连；不是 HBBR 网络侧的即时强制切断。 |
| 目标设备/用户组连接 ACL | 可实现，尚未实现 | 可利用现有客户端 token 与目标 ID，由 hbbs 向 Console 查询当前有效身份和目标授权；需先确定允许规则及 API 不可用时拒绝策略。直连 IP 不经过 hbbs。 |
| Switch Sides 授权 | 新增配套实现 | Console 转发签名，hbbs 用登记公钥验证；10 秒目标绑定的可重试路由授权，客户端仍执行 UUID 一次性免密验证。见 HTTP 协议文档与 server `docs/switch-grants.md`。 |
| 多中继、地理路由 | 已有此 fork 的联动实现 | Console `services/geo_relay`、server `src/geo_relay`；按规则与候选中继回退，不声称复制官方全部选路算法。 |
| WebSocket / 自托管浏览器客户端 | 已有传输及资源托管 | server 提供 ID/Relay WS；Console 内嵌 v1，v2 取决于外部 `data/web.zip`。须配置 HTTPS/WSS 并实测会话；独立核对外部资源的来源和分发许可。 |
| 配置导入、CLI、移动端配置 URI | 已有，保留 | `support/deployment_config.rs`；配置现有客户端，不生成定制二进制。过长的编码文件名应改用导入/命令。 |
| 自有签名配置、应用名称与品牌构建资源 | 可选，默认关闭 | `features/self_signed_client`；管理员生成/导入独立 Ed25519 密钥并下载签名 `custom.txt`，私钥只在服务器受限文件中保存。用户自行 fork 替换验签公钥并编译；logo/平台图标是编译资源，不是签名 JSON 字段。关闭功能不撤销已分发配置。 |
| 官方商业授权、官方签名或在线客户端构建服务 | 不实现 | 不伪造官方签名，不绕过授权，不替用户构建、代码签名或公证二进制；自有配置签名不等于完整 Pro 功能或连接 ACL。 |

## 建议顺序

1. 发布本轮已实现的协议、安全修复与 UI 改进，保持准确的已知限制。
2. 单独设计连接授权：在已有强制登录验签和缺失密钥拒绝的基础上，再接实时
   token/账户检查和目标 ACL。需要双方服务集成测试，不能只增加一个“允许”接口。
3. 为已有能力补充跨仓库部署验收：策略、共享权限、登录/撤销、协作断连、
   审计重试、录制上传，以及实际浏览器远程会话。
4. 不把目录隔离宣传为连接隔离，不把配置同步宣传为客户端不可篡改，也不把
   HTML 可访问宣传为浏览器远程连接验收通过。

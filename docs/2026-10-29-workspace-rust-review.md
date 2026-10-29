# Console workspace Rust review

## Scope and method

本轮按 `rust-review`、`rust-best-practices` 审查整个 workspace 的 106 个 Rust 文件，而非仅自有签名 feature。服务端按应用代码评估；`rustdesk-client-config`、`rustdesk-server-control`、`rustdesk-uri` 按辅助库评估，entity 按持久化模型评估。Rust edition 为 2021，仓库未声明 MSRV。

使用静态工具、分模块人工审查与聚焦回归测试；不是完整渗透测试或所有外部身份源的端到端认证。优先修复已证实的授权、数据隔离、错误成功响应和外部输入导致的故障，不为风格警告重构正常路径。

下列位置为本轮修改后的文件和函数；规则来源为上述本地 skills 的对应章节。

## Must fix — 已修复

| 位置 | 问题与后果 | 最小修复与规则 |
| --- | --- | --- |
| `http/api.rs::apply_device_cli_assignment` | 普通用户 bearer 可以提交他人的用户名、设备组和策略，或利用同 UUID 改写其他属主设备。 | upsert 前限制普通用户仅 self、拒绝管理级指派并验证既有设备属主；管理员与显式 deployment token 保留原授权。`rust-best-practices` §4 Types and API design、§5 Errors and panics：边界输入必须验证，拒绝无效请求。 |
| `http/middleware.rs` 的 `RustClientUser` / `DeploymentAuth` | 强制改密状态只在后台 extractor 生效，临时账户仍能使用 bearer 写接口。 | 相同状态在这两个 extractor 同样拒绝；保留客户端资料查询和 logout。§4、§5：认证状态必须在各入口一致落实。 |
| `http/oauth.rs::auth_query_pre/admin_confirm/admin_bind_confirm` | action 或 IdP callback 完成状态未验证，可以消费错误流程状态、写入空身份或重复绑定。 | 检查登录 action、完成状态、未绑定状态，查询失败不继续插入。§4、§5：显式验证状态及传播失败。 |
| `http/oauth.rs::message` | 文案直接拼进 JavaScript 单引号字符串，撇号与换行可破坏脚本边界。 | 使用 JSON 字符串序列化，不再手写引号。§4：把外部文本当数据，不当代码。此接口返回外部 JavaScript，不是 HTML 内联 script。 |
| `http/oauth.rs` HTML renderer 与 `resources/templates/oauth_{fail,success}.html` | 错误文案直接替换到内联脚本，引号导致语法错误，HTML script 结束标记也未转义；没有据此声称已证明完整远程 XSS。 | renderer 输出 JSON 字面量并转义 `<`，模板不再添加引号，URL 参数单独编码。§4：编码必须符合实际 HTML/JavaScript 上下文。 |
| `services/oauth.rs` 的 token/userinfo 处理 | 上游错误响应未按 HTTP 状态拒绝；GitHub/LinuxDO 缺失 id 退化为同一个 `"0"` 身份。不同无效响应可能共享这个错误本地身份，不等于任意正常账户均可被接管。 | 拒绝 HTTP 4xx/5xx 错误状态和缺失/非正整数 id；有效身份流程保留。§5：外部失败必须显式返回错误。 |
| `support/mod.rs::persist_config_file` 与三个配置 store | 固定临时文件先以默认权限写入完整含密配置，存在短暂可读窗口及符号链接截断风险；行补丁器还可能把合法多行 YAML 写坏。 | YAML 校验先于写盘；随机同目录临时文件、create_new、Unix 创建时0600，再恢复原权限并 rename；失败保持原文件与运行中设置。§5、§7：失败安全、资源操作显式处理，保留原进程内锁。 |
| `services/deployment.rs`、`http/api.rs` 的部署 token 写入口 | 余额验证和后置计数分离，max_uses=1 的令牌可以并发完成多次设备写入。 | 在设备副作用前通过条件 UPDATE 原子领取已有额度，检查受影响行数；无新增计数状态或退款协调。§7：共享授权计数的检查和修改必须原子化。已领取后部署失败也消耗一次额度。 |

## Should fix — 已修复

| 位置 | 问题与后果 | 最小修复与规则 |
| --- | --- | --- |
| `services/tag.rs::update_tags_collection`、`http/api.rs::ab_update` | 旧地址簿标签同步按整个 owner 删除，可能删除其他集合标签；非零集合的新标签被写到集合0。 | 所有协调限制在当前 owner/collection，接口传入已验证 cid，传播写库失败。§4、§5：保持数据归属与错误语义。 |
| `http/api.rs::logout`、`http/admin.rs::logout` | token 撤销失败仍返回成功，客户端被误导；不能据此推断每种失败都保留 token。 | 传播实际服务错误，成功响应不变。§5：不要丢弃可恢复错误。 |
| `http/admin.rs` 两条 batch-create、`http/my.rs::address_book_batch_create_from_peers` | 查询、确保标签或插入失败仍可能宣告导入成功。 | 遇到读写错误返回原协议失败；重复项仍跳过。§5。未引入批事务，失败前已写入的项不回滚。 |
| `support/login_limiter.rs::require_captcha` | 未验证的验证码条目永不主动回收，匿名请求可持续增加内存。 | 生成时清理过期条目、最多4096个；生成前及插入前检查，不在锁内渲染图片。§7、§12：限制外部驱动资源增长，不增加计时器或新状态。 |
| `support/jwt.rs::try_new`、`bootstrap.rs::build_state` | 超大且可解析的 JWT duration 在登录生成 token 时 panic，release abort 会退出进程。 | 数据库初始化前验证 duration 转换与时间戳加法范围，非法启用配置拒绝启动。§5：外部配置错误不是程序不变量。 |
| `services/user.rs::login` | 登录审计写 user id 而非已插入的 token id，导致关联错误。 | 使用实际 token row id。§11 Tests：验证业务记录关联，而非实现顺序。 |
| `services/diagnostics.rs` 的端口推导 | `i32::MIN - 1` 在归一化前溢出。 | checked_sub 后再验证端口，非法输入正常失败。§5：无效配置不应触发算术 panic。 |

## Consider — 保留限制

- `cargo fmt --all --check` 仍会报告原有格式差异；未全仓格式化。普通 Clippy 与 pedantic 检查保留既有的参数数目、文档、命名和测试模块位置等建议，不将应用改造成泛化库来消除告警。
- YAML 多行块暂不由旧行补丁器重写；如果生成的 YAML 无效，保存明确失败，原配置不变。没有引入新的 YAML 重写框架。
- 验证码总量限制不等于完整的请求速率控制；现有登录失败/封禁规则不变。
- 配置落盘锁仍为单进程；配置目录必须可信。没有新增跨进程锁、缓存或生命周期机制。
- 部分既有只读列表接口仍把查询失败折叠为空列表；本轮没有扩大到所有响应封装或查询签名的重构。

## What's already good

- 自有签名 feature 默认关闭，前后端均有门禁；密钥与 hbbs 身份密钥、JWT secret 独立。
- 生成操作先安全落盘，私钥仅在本次生成响应返回且 no-store；后续设置、导入和签名均不回显私钥。
- 共享地址簿、部署 token、策略和管理员接口已有分层服务及可复用的 SQLite 测试方式；本轮补丁沿用这些机制，无新增依赖。
- Tokio 路径没有为本轮修复创建新 runtime；配置文件工作保持在已有 blocking 路径和锁边界内。

## Regression surface

- `http/api.rs`：普通 CLI 指派的授权、logout 错误、旧地址簿标签同步错误；成功输出协议保留。
- `http/middleware.rs`：必须改密账户的 bearer 写接口受限，其余账户原路径不变。
- `http/oauth.rs`、`services/oauth.rs`：非法/未完成 OAuth 流程和上游错误不再继续；有效认证流程保留。
- `resources/templates/oauth_{fail,success}.html`：仅消息脚本编码与 URL 参数编码改变。
- `services/deployment.rs`、`http/api.rs` 部署入口：额度领取前移至设备副作用之前；失败部署也可能消耗次数，以严格限制授权总量。
- `http/admin.rs`、`http/my.rs`：仅 logout 和指定批量写入失败不再伪装成功。
- `services/tag.rs`：同步当前集合，不再修改其他集合。
- `support/{mod,admin_config,webclient_config,record_storage_config}.rs`：设置持久化的临时文件和非法 YAML 处理；正常配置内容与缓存更新顺序不变。
- `support/jwt.rs`、`bootstrap.rs`：仅非法启用 JWT 配置提前拒绝启动。
- `support/login_limiter.rs`：验证码生成容量与过期回收；原验证、封禁逻辑不变。
- `services/user.rs`、`services/diagnostics.rs`：登录审计关联及非法诊断端口处理。
- Cargo workspace 版本与锁文件仅同步发布版本；客户端、共享子模块及 hbbs/hbbr 源码未改。

## Verification

- `cargo test --workspace --all-features --locked`：235 项测试通过（服务端219，三个辅助库7/5/4）。
- `cargo clippy --workspace --all-targets --all-features --locked`：通过，保留原有12项生产/13项测试警告；加 `-W clippy::pedantic` 的检查亦退出0，但仍有大量非阻断文档、命名、风格建议，不能称为零告警。
- `cargo build --workspace --all-features --locked` 与 `cargo doc --workspace --no-deps --locked`：通过。
- Web 按 CI 固定的 pnpm 10.34.0 执行 `test` 和 `run build`：7项测试通过，TypeScript/Vite 构建通过；保留既有大 chunk 提醒。
- `git diff --check`：通过。`cargo fmt --all --check` 仍因旧代码格式差异失败，新增修改已定点格式化，未改无关旧路径。
- 最后独立窄复核确认部署额度与 OAuth 模板修复无重要问题；并发 deploy/assign、无限额/失效额度、内联脚本文案三个专项测试再次通过。

外部身份源、真实跨设备远程会话及定制客户端编译不在此次单元测试验收范围内。

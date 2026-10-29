# Console 全量代码审查与本轮回归面

此文记录 v0.2.38 及此前的审查。v0.2.39 的后续全 workspace 审核、验证码容量修复和新验证结果见 `2026-10-29-workspace-rust-review.md`；下列已知边界为当时状态。

本轮按 `rust-review`、`rust-best-practices` 审阅所有 Rust 生产模块，
不是仅检查新增功能。范围：`crates/entity`、三个辅助库、server 全部 HTTP、
services（含 Geo 子模块）、support、入口、bootstrap、配置和资源模块。
大型旧测试模块按相关行为核对，未声称逐行重审所有旧测试。

## 修复与必要行为变化

| 原有路径/文件 | 变化及必要性 |
| --- | --- |
| `http/api.rs`、`services/audit*.rs`、新增审计实体、`bootstrap.rs` | 当前客户端要求空成功响应；落库失败不能确认；nonce 重试不能重复落库；告警必须持久化。旧无 nonce 请求保留原入口。设备相同 `conn_id` 的更新需按设备隔离。 |
| `services/tag.rs`、`http/api.rs` 标签操作 | 标签改名/删除与同属主同地址簿的设备引用事务同步，避免客户端重新同步出现旧标签。 |
| `services/strategy.rs`、前端策略元数据 | 补充已被当前客户端 Config 消费的三个选项；不重写已有策略优先级或 LocalConfig。 |
| `services/user.rs`、`http/oauth.rs` | 禁用账户不能继续使用有效 token 或完成 OAuth 换取 token；bcrypt 放入阻塞任务，避免拖住 heartbeat。 |
| `http/admin.rs`、`http/api.rs`、`support/login_limiter.rs` | 封禁先于密码/验证码分支；关闭验证码时仍可启用封禁；原生客户端不增加 CAPTCHA UI 协议。 |
| `services/login_security.rs`、重置密码 handler | 不再公开已有重置 secret；一次提交原子消费挑战，错误后重新申请，避免六位码无限猜测和并发复用；复用现有状态，无新挑战计数器。 |
| `http/file.rs`、`http/router.rs` | 只新建已识别的 PNG/JPEG/GIF/WebP 类型文件，生成文件名；历史上传也有独立 sandbox CSP/nosniff，避免同源可执行内容窃取 Console token。 |
| `http/api.rs`、`http/admin.rs` 录制删除 | 对象或暂存文件删除失败保留元数据，返回失败以供重试；不再丢掉位置并假报成功。 |
| `services/mod.rs`、HTTP 分页 | 极端外部页码不得溢出或回到第一页；正常默认值不变。 |
| `rustdesk-client-config`、`support/deployment_config.rs` | 编码配置文件名替代含端口、URL、密钥分隔符的 Windows 非法文件名。长配置建议使用导入/命令。 |
| `config.rs`、`bootstrap.rs` | 非法 duration 返回失败而不是 panic；数据库用户/密码按 URL 编码保真。 |
| 三个配置 store、`support/mod.rs` | 共用持久化锁保护完整读改写，防止不同设置页同时保存丢失另一节。仅进程内保证，不承诺多个进程并写同一配置。 |
| `rustdesk-server-control` | 完整有界读取到 EOF；超时是失败而非空成功；非法端口计算不溢出。 |
| `services/geo_relay/mmdb.rs` | 下载写入/fsync 与整库验证/下载后状态读取离开 async worker；不改变替换与恢复顺序。 |
| 新 `http/switch_grant.rs`、路由 | 有界转发已签名注册，只相信 hbbs 明确确认，不维护 Console 公钥信任副本或授权缓存。离线/旧 server 拒绝。 |

两个审计表在每次启动的缺表创建阶段加入；既有 schema 273 数据库也会创建，
不修改旧表列。未来改变这些实体的列仍需版本化迁移。

## UI 审查

| Before | After | Why |
| --- | --- | --- |
| 概览卡片、页面背景层次接近 | 中性工作区、独立白/暗面板、统一细边框阴影 | 增强数据层级，不堆叠装饰渐变 |
| 当前位置仅靠侧栏高亮 | 桌面顶栏显示当前分区和页面 | 导航收起时仍保留上下文 |
| 两个近期表在中等宽度挤压表头 | 宽屏并排，否则纵向；表头与数据不拆词，容器内部滚动 | 保持列可读，不让整个页面横向溢出 |
| 空/加载/失败只有文本条 | 有可识别图标和一致留白，保留 status/alert 语义 | 明确状态，不虚构数据 |
| 320px 窄屏高弹窗从顶部 32px 开始，底部超出视口 | 顶部与最大高度使用一致的视口边距，表单内部滚动 | 保存/取消区域始终可见，不新增状态或回调 |
| 动效可被误用为装饰 | 保留短按压反馈、抽屉空间连续性；不新增数据数字滚动或持续漂浮 | 高频操作不应等待，减少晕动；reduced-motion 保留静态状态 |

## 已知边界，不借审查扩张重构

- 旧 Axum handler 返回 `Response` 的大错误分支、少数多参数函数仍有 Clippy
  风格建议。不会为清警告而重写成熟调用链或统一加抑制属性。
- MMDB 概览与备份/恢复仍有同步 I/O；本轮未把带变更锁的恢复生命周期移到可
  失去调用方锁保护的后台任务。此项单独排期，不假报所有 I/O 已异步化。
- 验证码旧缓存回收、配置跨进程文件锁、超长 Windows 编码文件名不新增生命周期
  机制；后者已有配置导入替代路径。
- Pro 风格 API 不代表完整连接 ACL 或实时网络侧撤销；详见能力矩阵。
- 第三方身份源、S3/WebDAV、真实跨设备远程会话需实际部署验收；单测与浏览器
  管理页验收不能代替这些结果。

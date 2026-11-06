# 开发者接入

[English](developer-access.en.md) · **中文**

在 Console 打开 **开发者接入 → API 密钥**，填写名称、选择必要权限，然后立即保存
本次显示的完整密钥。Console 只存储哈希和展示前缀，之后无法重新查看完整值。
默认只读权限、90 天有效期，最长 365 天。无需使用的密钥请在同一页面撤销。
本功能不依赖定制客户端，也不依赖“自有签名客户端”开关。

可信本地网络之外请使用 HTTPS。通过密钥管理工具或环境变量提供密钥，不能放入 URL、
提交到源码、用于浏览器前端应用或写入排障日志。API 密钥不能登录 RustDesk、管理密钥，
也不能认证原有的后台会话或客户端部署接口。

## SDK 与示例

内置 **SDK 与示例** 页面提供 Rust、Go 和 Node.js 的设备查询示例。
`RUSTDESK_CONSOLE_URL` 填 Console 对外基址，不追加 `/api/v1`；
`RUSTDESK_CONSOLE_API_KEY` 通过环境变量传入密钥。

- Rust：`cargo add rustdesk-console-sdk`；源码：https://github.com/lib-x/rustdesk-console-sdk-rs
- Go：`go get github.com/lib-x/rustdesk-console-sdk-go@v0.1.0`
- Node.js：`npm install github:lib-x/rustdesk-console-sdk-ts#v0.1.0`

三个仓库都包含全部 16 个操作的类型化方法、完整示例、中英文文档和测试。
SDK 保留证书验证、拒绝重定向，不自动重试写操作。TypeScript 首版从 GitHub 安装，
不表示已发布到 npm registry。

## HTTP 接口约定

向 `/api/v1` 发送 `Authorization: Bearer <API_KEY>` 请求头。
公开的 `/api/v1/openapi.json` 无需鉴权，包含模型、请求体、错误、权限范围和归属限制；
内置 **OpenAPI 接口文档** 页面展示操作列表并提供 JSON 下载，可以导入自己的文档查看器或代码生成器。

单个资源返回 `{ "data": ... }`；列表另带 `total`、`page`、`page_size`。
页码从 1 开始，默认每页 30 条，最多 100 条。
错误使用真实的非 2xx HTTP 状态，正文为 `{ "error": { "code": "...", "message": "..." } }`。
路径中的数字 ID 是 Console 数据库行 ID，**不是 RustDesk 连接 ID**。

普通账户只能读取自己的设备和操作个人通讯录。用户管理、设备修改、设备组与策略操作
需要账户当前仍为管理员，同时具有对应权限范围。每次请求重新检查账户与授权：撤销、到期、
禁用账户、要求改密或撤销管理员身份均会影响后续请求。
API 不返回密码哈希、TOTP 秘密或客户端签名密钥；原有 RustDesk 客户端接口保持不变。

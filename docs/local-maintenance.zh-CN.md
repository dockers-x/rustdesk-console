# 本地运维命令

请在 Console 的安装目录运行这些命令。Docker 镜像的工作目录为 `/app`。
SQLite 使用相对于工作目录的 `./data/rustdeskapi.db`。

[English](local-maintenance.en.md)

```sh
rustdesk-console version --json
rustdesk-console config validate --config ./conf/config.yaml --json
rustdesk-console database status --config ./conf/config.yaml --json
rustdesk-console database migrate --config ./conf/config.yaml --json
```

| 命令 | 行为 |
| --- | --- |
| `version` | 显示程序版本、构建提交及支持的数据库架构版本，无需加载配置。 |
| `config validate` | 加载 YAML 和环境变量覆盖值，检查运行配置和本地文件引用。可选文件缺失时给出警告。该命令不会连接数据库或启动服务。 |
| `database status` | 只读检查配置指向的数据库。SQLite 文件不存在时返回 `empty`，也不会创建文件或父目录。 |
| `database migrate` | 创建缺失的表并执行现有迁移和数据回填。此命令和正常启动使用同一把迁移锁。默认用户组和管理员初始化由正常启动完成。 |

数据库架构状态有 `empty`（空）、`current`（当前）、`upgrade_required`
（待升级）、`newer_than_binary`（高于程序支持版本）和 `invalid`（无效）。
`safe_to_start` 表示记录的架构版本和程序支持的版本一致。正常启动仍会按需
执行兼容的迁移。如果数据库版本高于程序支持的版本，程序会在修改表或账号前
拒绝启动。可重复执行 `database migrate`，输出中的 `migrated` 表示数据库记录
的架构版本是否升级。

上述四个命令均接受 `--json`，并将一个包含 `schema_version: 1` 的 JSON 对象
输出到标准输出。退出码 `0` 表示成功，`1` 表示输出失败，`2` 表示命令用法错误，
`3` 表示配置无效，`4` 表示数据库连接或迁移失败，`5` 表示架构版本不兼容或
元数据无效。`database status` 正常完成检查时，即使状态为 `empty` 或
`upgrade_required`，也会返回退出码 `0`。

## 账号恢复

可从受保护的文件读取密码，或通过管道将密码交给标准输入。

```sh
rustdesk-console reset-admin-pwd --password-file /run/secrets/console-password
rustdesk-console reset-pwd 42 --password-stdin < /run/secrets/console-password
rustdesk-console reset-totp 42
```

`--password-file` 适用于 Unix，只接受未授予用户组及其他用户权限的普通文件。
两种安全输入方式都接受 4–72 字节的单行 UTF-8 密码，支持可选的末尾 LF 或 CRLF，
并保留空格。重置密码会撤销该用户的会话。原有的位置参数用法仍可使用，建议
使用安全输入选项，避免密码出现在进程参数或 shell 历史记录中。

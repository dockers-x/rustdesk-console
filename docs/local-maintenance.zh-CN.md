# 本地运维命令

请在 Console 的安装目录运行这些命令。Docker 镜像的工作目录为 `/app`。
SQLite 使用相对于工作目录的 `./data/rustdeskapi.db`。

[English](local-maintenance.en.md)

```sh
rustdesk-console version --json
rustdesk-console config validate --config ./conf/config.yaml --json
rustdesk-console database status --config ./conf/config.yaml --json
rustdesk-console database migrate --config ./conf/config.yaml --json
rustdesk-console database backup --config ./conf/config.yaml --output /backups/console-2026-11-21 --json
```

| 命令 | 行为 |
| --- | --- |
| `version` | 显示程序版本、构建提交及支持的数据库架构版本，无需加载配置。 |
| `config validate` | 加载 YAML 和环境变量覆盖值，检查运行配置和本地文件引用。可选文件缺失时给出警告。该命令不会连接数据库或启动服务。 |
| `database status` | 只读检查配置指向的数据库。SQLite 文件不存在时返回 `empty`，也不会创建文件或父目录。 |
| `database migrate` | 创建缺失的表并执行现有迁移和数据回填。此命令和正常启动使用同一把迁移锁。默认用户组和管理员初始化由正常启动完成。 |
| `database backup` | 将一致的 SQLite 快照和 JSON 清单写入新目录。 |

数据库架构状态有 `empty`（空）、`current`（当前）、`upgrade_required`
（待升级）、`newer_than_binary`（高于程序支持版本）和 `invalid`（无效）。
`safe_to_start` 表示记录的架构版本和程序支持的版本一致。正常启动仍会按需
执行兼容的迁移。如果数据库版本高于程序支持的版本，程序会在修改表或账号前
拒绝启动。可重复执行 `database migrate`，输出中的 `migrated` 表示数据库记录
的架构版本是否升级。

上述命令均接受 `--json`，并将一个包含 `schema_version: 1` 的 JSON 对象
输出到标准输出。退出码 `0` 表示成功，`1` 表示输出失败，`2` 表示命令用法错误，
`3` 表示配置无效，`4` 表示数据库连接、迁移或备份失败，`5` 表示架构版本不兼容或
元数据无效。`database status` 正常完成检查时，即使状态为 `empty` 或
`upgrade_required`，也会返回退出码 `0`。

## SQLite 备份与恢复

`database backup` 通过 SQLite 的 `VACUUM INTO` 创建一致快照，可在 Console 持续
写入时执行。`--output` 的父目录须已存在，输出目录由命令新建。目录包含
`rustdeskapi.db` 和 `manifest.json`。清单记录程序版本、快照中的数据库架构版本、
UTC 创建时间、文件大小和 SHA-256 校验值。Unix 下目录权限为 `0700`，文件权限为
`0600`，更严格的 umask 仍然有效。Windows 下应将备份放在仅授权运维人员可访问的目录中。

恢复前，比对快照和清单中的 SHA-256，并对快照执行
`sqlite3 -readonly rustdeskapi.db 'PRAGMA integrity_check;'`。停止 Console 和其他
写入进程，将现有数据库及对应的 `-wal`、`-shm` 和 `-journal` 文件移出原路径以备回滚。
复制已验证的快照到数据库原路径，设置服务运行账号的所有权和文件权限。用准备启动的
程序和配置执行 `database status`，重启后验证账号登录和地址簿。

`database status` 会显示配置使用的数据库。MySQL/MariaDB 请用 `mysqldump` 或
`mariadb-dump`，PostgreSQL 请用 `pg_dump`。Console 配置、部署密钥、签名密钥和
录屏须另行备份。联合 S6 部署可按 Server 镜像的离线备份指南同时备份两个数据库和服务密钥。

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

## SQLite 远程备份

在配置文件中添加命名目标，重启 Console。进入 **诊断 → SQLite 远程备份**，
点 **立即备份** 可在后台创建并上传快照。页面显示每个目标最近一次的结果。
如需上传 S6 离线归档，用 `backup-config-file` 指向单独挂载的 YAML
密钥文件，避免把各目标的凭据放进包含主配置的 S6 归档。密钥文件从
`interval-hours` 开始写，无需外层 `backup`。该文件优先于内联目标，
相对路径以主配置所在目录为准。

```yaml
backup:
  interval-hours: 24
  targets:
    - name: 异地 S3
      storage:
        type: s3
        s3:
          endpoint: https://s3.us-east-1.amazonaws.com
          region: us-east-1
          bucket: private-rustdesk-backups
          prefix: console/production
          access-key-id: REPLACE_ME
          secret-access-key: REPLACE_ME
    - name: WebDAV 归档
      storage:
        type: webdav
        webdav:
          url: https://dav.example.com/backups
          prefix: console/production
          username: REPLACE_ME
          password: REPLACE_ME
```

将 `interval-hours` 设为 0 可关闭定时备份。开启后首次自动运行随即执行，
之后按距离上一次尝试的时间计算。手动和定时任务共用同一把锁。
每次运行生成一份 ZIP，独立上传到各目标。请为保存敏感数据的存储桶或
WebDAV 目录设置访问控制，由目标端管理保留期和静态加密。建议使用 HTTPS。
私有 HTTP 服务可在该目标上显式设置 `allow-http: true`。
需要路径式访问的 S3 兼容服务可在 `s3` 下设置 `force-path-style: true`。

从成功的任一目标下载 ZIP，解压到私有目录，按前文使用
`manifest.json` 和 `rustdeskapi.db` 恢复。最近一次任务的状态保存在
`data/remote-backup-status.json`。

整套 S6 完成离线归档后，可以用相同配置执行
`rustdesk-console -c ./conf/config.yaml database upload-backup /backups/s6-2026-11-21.tar --json`。
可在宿主机停写备份完成后定时执行这一步。命令等待所有目标处理结束，
任一目标失败时返回退出码 4。Console 的定时任务仅备份自己的 SQLite，
日常运行无需停止 S6。

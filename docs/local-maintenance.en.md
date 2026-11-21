# Local maintenance CLI

Run these commands from the Console installation directory. The Docker image
uses `/app` as its working directory. SQLite uses the path
`./data/rustdeskapi.db` relative to that directory.

[简体中文](local-maintenance.zh-CN.md)

```sh
rustdesk-console version --json
rustdesk-console config validate --config ./conf/config.yaml --json
rustdesk-console database status --config ./conf/config.yaml --json
rustdesk-console database migrate --config ./conf/config.yaml --json
```

| Command | Behavior |
| --- | --- |
| `version` | Reports the binary version, build revision and supported database schema without loading configuration. |
| `config validate` | Loads YAML and environment overrides, checks runtime settings and local references, and reports optional missing files as warnings. It does not open the database or start the server. |
| `database status` | Uses a read-only connection to inspect the configured database. A missing SQLite file reports `empty` without creating the file or its parent directory. |
| `database migrate` | Creates missing tables and runs the existing migrations and their backfills. Startup and explicit migration share the same migration lock. Default groups and administrator setup run only on normal startup. |

The schema states are `empty`, `current`, `upgrade_required`,
`newer_than_binary` and `invalid`. `safe_to_start` means that the reported
schema version matches the binary. Normal startup still applies compatible
migrations when necessary. A newer schema is refused before any tables or
accounts are modified. Repeating `database migrate` is safe. Its `migrated`
field indicates that the recorded version advanced.

The four inspection and migration commands accept `--json` and print a single
object with `schema_version: 1` to stdout. Their exit codes are `0` for
success, `1` for output errors, `2` for command usage errors, `3` for invalid
configuration, `4` for database connection or migration errors, and `5` for
incompatible or invalid schema metadata. Status reports `empty` and
`upgrade_required` with exit code `0` because inspection completed.

## Account recovery

Use a protected password file or pipe the password from a secret store.

```sh
rustdesk-console reset-admin-pwd --password-file /run/secrets/console-password
rustdesk-console reset-pwd 42 --password-stdin < /run/secrets/console-password
rustdesk-console reset-totp 42
```

The password file option is available on Unix and requires a regular file with
all group and other permissions disabled. Secure input accepts one UTF-8
password of 4–72 bytes with an optional final LF or CRLF. Spaces are preserved.
Both password reset commands revoke the selected user's sessions. The existing
positional password forms remain available for compatibility. Use the secure
options so the password stays out of the process arguments and shell history.

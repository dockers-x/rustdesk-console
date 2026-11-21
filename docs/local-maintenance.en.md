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
rustdesk-console database backup --config ./conf/config.yaml --output /backups/console-2026-11-21 --json
```

| Command | Behavior |
| --- | --- |
| `version` | Reports the binary version, build revision and supported database schema without loading configuration. |
| `config validate` | Loads YAML and environment overrides, checks runtime settings and local references, and reports optional missing files as warnings. It does not open the database or start the server. |
| `database status` | Uses a read-only connection to inspect the configured database. A missing SQLite file reports `empty` without creating the file or its parent directory. |
| `database migrate` | Creates missing tables and runs the existing migrations and their backfills. Startup and explicit migration share the same migration lock. Default groups and administrator setup run only on normal startup. |
| `database backup` | Writes a consistent SQLite snapshot and a JSON manifest to a new directory. |

The schema states are `empty`, `current`, `upgrade_required`,
`newer_than_binary` and `invalid`. `safe_to_start` means that the reported
schema version matches the binary. Normal startup still applies compatible
migrations when necessary. A newer schema is refused before any tables or
accounts are modified. Repeating `database migrate` is safe. Its `migrated`
field indicates that the recorded version advanced.

These commands accept `--json` and print a single
object with `schema_version: 1` to stdout. Their exit codes are `0` for
success, `1` for output errors, `2` for command usage errors, `3` for invalid
configuration, `4` for database connection, migration or backup errors, and `5` for
incompatible or invalid schema metadata. Status reports `empty` and
`upgrade_required` with exit code `0` because inspection completed.

## SQLite backup and restore

`database backup` uses SQLite's `VACUUM INTO` and can run while Console is
accepting writes. The parent of `--output` must exist and the output directory
must be new. The directory contains `rustdeskapi.db` and `manifest.json`.
The manifest records the binary version, schema from the snapshot, UTC creation
time, file size and SHA-256 checksum. On Unix, the directory uses mode `0700`
and files use `0600`, subject to a stricter umask. On Windows, create the
backup under a directory accessible only to authorized operators.

To restore, compare the snapshot's SHA-256 with the manifest and run
`sqlite3 -readonly rustdeskapi.db 'PRAGMA integrity_check;'` against the
snapshot. Stop Console and every other writer. Move the current database and
any corresponding `-wal`, `-shm` and `-journal` files out of the active path
for rollback. Copy in the verified snapshot with the service account's
ownership and private permissions. Run `database status` with the intended
binary and config before restarting and checking login and address books.

`database status` identifies the configured backend. Use `mysqldump` or
`mariadb-dump` for MySQL/MariaDB and `pg_dump` for PostgreSQL. Save Console
configuration, deployment secrets, signing keys and recordings alongside the
database backup as separate datasets. For bundled S6 deployments, follow the
Server image's offline backup guide to capture both databases and the service
keys together.

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

## Remote SQLite backups

Create S3 or WebDAV profiles in **System ops → Storage configuration**.
In **System settings → Automatic backup**, select one or more profiles and set
the interval and the age public key. Console picks up the new settings within a minute. Recordings
can select one profile (local, S3 or WebDAV) in the Recordings page. New
recordings use the selected profile, and saved recordings keep the profile
used at upload. To change a destination, create a new profile. You can update
the name and credentials of existing profiles.

Profiles are stored in `data/storage-profiles.json` with restricted file
permissions. Set `storage-profiles-file` to use a different path. Keep a
separate private copy for disaster recovery, since the database backup and
S6 offline archive exclude it.

Existing deployments can continue using the following YAML settings.
To keep credentials out of S6 offline archives, point `backup-config-file` at a
separately mounted YAML secret file. That file contains the contents of the
`backup` section starting at `interval-hours`, without the enclosing key,
and takes precedence over inline targets. Relative paths are resolved from
the Console configuration file's directory.
**System settings → Automatic backup** shows the configured targets and each
target's latest result. **Back up now** starts a background run immediately.

```yaml
backup:
  age-recipient: age1...
  interval-hours: 24
  targets:
    - name: Offsite S3
      storage:
        type: s3
        s3:
          endpoint: https://s3.us-east-1.amazonaws.com
          region: us-east-1
          bucket: private-rustdesk-backups
          prefix: console/production
          access-key-id: REPLACE_ME
          secret-access-key: REPLACE_ME
    - name: WebDAV archive
      storage:
        type: webdav
        webdav:
          url: https://dav.example.com/backups
          prefix: console/production
          username: REPLACE_ME
          password: REPLACE_ME
```

An interval of 0 enables manual runs only. Automatic runs start when the
configured number of hours has passed since the last attempt. The first
scheduled run is due immediately. Manual and scheduled runs share a lock. Each
run encrypts one ZIP with age before uploading the same ciphertext to each
target independently. Use a private bucket or WebDAV collection with access
controls and manage remote retention on the storage side. Use HTTPS.
For private HTTP deployments, opting in requires `allow-http: true` on that
target. For S3-compatible path-style endpoints, set `force-path-style: true`
under `s3`.

Generate a recovery key with `age-keygen -o identity.txt`. Enter only the
`age1…` public key in Console or `age-recipient` in the existing YAML.
Keep the private identity separately from Console and remote storage.
Remote uploads require a valid recipient. To recover from a successful target:

```sh
age --decrypt --identity identity.txt --output console.zip console.zip.age
```

Extract the decrypted ZIP in a private directory and use its
`manifest.json` and `rustdeskapi.db` with the SQLite restore procedure.
The latest run's status is kept in `data/remote-backup-status.json`.

Console encrypts TOTP seeds using `data/totp.key`, or the file set by
`security.totp-key-file`. Console creates this key with restricted
permissions when it first needs it. Keep a separate private copy of the
original key and restore it to the same path before starting Console
against a database with encrypted seeds. Historical databases and
archives from before this migration may contain plaintext TOTP seeds.

For a completed S6 offline tar, the same destinations can be used by running
`rustdesk-console -c ./conf/config.yaml database upload-backup /backups/s6-2026-11-21.tar --json`.
Store remote credentials in the separate secret file before capturing an S6
tar. The S6 archive includes the main YAML file.
Run this as part of a host maintenance schedule after successful offline
capture if required. It encrypts the archive to a `.tar.age` object, waits for every destination and exits with status 4
if any delivery failed. The Console schedule captures only its own SQLite
database and runs without stopping S6.

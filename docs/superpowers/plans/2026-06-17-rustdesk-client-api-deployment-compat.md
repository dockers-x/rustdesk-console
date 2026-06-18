# RustDesk Client API And Deployment Compatibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make rustdesk-console compatible with the RustDesk client APIs that affect connection, recording upload, and custom deployment, then expose practical one-click deployment helpers in the admin UI.

**Architecture:** Keep client runtime endpoints under `/api/*` compatible with RustDesk's observed calls. Keep admin/operator workflows under `/api/admin/*`, and add compatibility shims only where official clients or official scripts call Pro-style endpoints directly. Store large recording blobs on disk under `resources/record`; store only metadata in the database.

**Tech Stack:** Rust, axum, SeaORM, tokio, React, Cloudflare Kumo, Vite, SQLite/MySQL/PostgreSQL-compatible entities.

---

## Source Evidence

RustDesk client and scripts checked:

- `/home/czyt/code/rust/rustdesk/src/hbbs_http/sync.rs`
- `/home/czyt/code/rust/rustdesk/src/hbbs_http/record_upload.rs`
- `/home/czyt/code/rust/rustdesk/src/ui_interface.rs`
- `/home/czyt/code/rust/rustdesk/src/core_main.rs`
- `/home/czyt/code/rust/rustdesk/src/custom_server.rs`
- `/home/czyt/code/rust/rustdesk/flutter/lib/common.dart`
- `/home/czyt/code/rust/rustdesk/res/devices.py`
- `/home/czyt/code/rust/rustdesk/res/device-groups.py`
- `/home/czyt/code/rust/rustdesk/res/strategies.py`
- `/home/czyt/code/rust/rustdesk/res/users.py`
- `/home/czyt/code/rust/rustdesk/res/user-groups.py`
- `/home/czyt/code/rust/rustdesk/res/ab.py`
- `/home/czyt/code/rust/rustdesk/res/audits.py`

Key findings:

- Runtime client calls that can block normal use: `/api/heartbeat`, `/api/sysinfo`, `/api/sysinfo_ver`, `/api/currentUser`, `/api/record`, `/api/audit/conn/active`, `/api/audit`, `/api/devices/deploy`, `/api/devices/cli`.
- RustDesk supports importable server configuration strings with JSON fields `host`, `relay`, `api`, `key`, encoded as reversed base64url.
- RustDesk Windows executable names can carry server settings directly: `rustdesk-host=<id-server>,key=<public-key>,api=<api-server>,relay=<relay-server>.exe`.
- RustDesk CLI supports local one-click configuration through `--option custom-rendezvous-server`, `--option relay-server`, `--option api-server`, `--option key`.
- RustDesk CLI supports device bootstrap through `--deploy --token <token> [--id <new-id>]` and post-bootstrap assignment through `--assign --token <token> ...`, calling `/api/devices/deploy` and `/api/devices/cli`.
- Official management scripts are useful as compatibility targets, but full `/api/users`, `/api/devices`, `/api/device-groups`, `/api/user-groups`, `/api/strategies`, `/api/audits/*`, and extended address-book APIs are larger than the immediate deployment problem.

## Scope

Build now:

- Finish runtime compatibility already started in the worktree: `/api/record`, heartbeat strategy shape, user payload fields, audit note update.
- Add backend-visible recording metadata and admin list/download/delete.
- Add custom deployment helpers for server address/public key/API/relay/WS settings.
- Add `/api/devices/deploy` and `/api/devices/cli` with token auth so official RustDesk `--deploy` and `--assign` work.
- Add a focused admin UI page that generates config strings, executable filename suffixes, and platform CLI commands.

Do not build in the first two phases:

- Full Pro strategy engine with every policy option.
- Email invitation and 2FA enforcement endpoints from `users.py`.
- Full console/audit compatibility for every script path.
- Client binary build/sign service from `res/job.py`.
- hbbs/hbbr changes, unless an API task proves impossible without them.

## Current Worktree Note

There are existing uncommitted partial changes in:

- `crates/server/src/http/api.rs`
- `crates/server/src/http/payloads.rs`
- `crates/server/src/http/router.rs`
- `crates/server/src/services/audit.rs`
- `crates/entity/src/record_file.rs`

These should be completed as Phase 1 and Phase 2 work, not committed in their partial state. In particular, `record_file.rs` currently needs to be wired into the entity module, bootstrap table creation, service layer, admin routes, and UI; otherwise remove it before committing Phase 1.

## Phase 1: Runtime Client Compatibility

This phase is independently mergeable. After it ships, current RustDesk clients should no longer fail on known runtime calls even if recording files are not yet listed in the admin UI.

### Task 1: Lock `/api/record` Upload Compatibility

**Files:**
- Modify: `crates/server/src/http/api.rs`
- Modify: `crates/server/src/http/router.rs`
- Test: `crates/server/src/http/api.rs`

- [ ] Keep `POST /api/record` with query parameters `type`, `file`, `offset`, and `length`.
- [ ] Support `type=new` by creating/truncating `resources/record/<file>`.
- [ ] Support `type=part` and `type=tail` by writing the binary body at the requested `offset`.
- [ ] Support `type=remove` idempotently.
- [ ] Keep the response body `{}` on success because the client only treats an `error` field as failure.
- [ ] Reject path separators, control characters, Windows-reserved filename characters, empty names, and overlong names.
- [ ] Keep `DefaultBodyLimit::max(64 * 1024 * 1024)` on this route.
- [ ] Run:

```bash
cargo test -p rustdesk-console-server record_tests
```

Expected: all record filename tests pass.

### Task 2: Finalize Heartbeat Strategy Shape

**Files:**
- Modify: `crates/server/src/http/api.rs`
- Test: `crates/server/src/http/api.rs`

- [ ] Return `strategy.config_options` as an object.
- [ ] Return `strategy.extra` as an object.
- [ ] Include `strategy.extra.translate_mode` with string value `"true"` or `"false"`.
- [ ] Preserve any older `translate_mode` field only as backward compatibility.
- [ ] Echo client `modified_at` when no server-side strategy changed, so clients do not see a strategy update on every heartbeat.
- [ ] Add a test that deserializes the heartbeat response shape expected by RustDesk's `StrategyOptions`.
- [ ] Run:

```bash
cargo test -p rustdesk-console-server heartbeat
```

Expected: heartbeat response tests pass.

### Task 3: Finish RustDesk User Payload Compatibility

**Files:**
- Modify: `crates/server/src/http/payloads.rs`
- Test: `crates/server/src/http/payloads.rs`

- [ ] Include `display_name`, `avatar`, and `verifier` in `UserPayload`.
- [ ] Map `display_name` to the current username unless a separate display-name field exists.
- [ ] Keep `avatar` and `verifier` as empty strings until dedicated data exists.
- [ ] Add a serialization test for the exact keys RustDesk Flutter reads.
- [ ] Run:

```bash
cargo test -p rustdesk-console-server user_payload
```

Expected: user payload compatibility test passes.

### Task 4: Finish Audit Note Compatibility

**Files:**
- Modify: `crates/server/src/http/api.rs`
- Modify: `crates/server/src/http/router.rs`
- Modify: `crates/server/src/services/audit.rs`
- Test: `crates/server/src/services/audit.rs`

- [ ] Keep `GET /api/audit/conn/active?id=<peer>&session_id=<sid>&conn_type=<n>`.
- [ ] Return an audit connection row id as a string guid when a matching active connection exists.
- [ ] Return an empty string when no match exists.
- [ ] Keep `PUT /api/audit` with body `{ "guid": "...", "note": "..." }`.
- [ ] Update only the note for the referenced connection audit row.
- [ ] Run:

```bash
cargo test -p rustdesk-console-server audit
```

Expected: audit active lookup and note update tests pass.

### Task 5: Add Legacy Address Book Alias If Needed

**Files:**
- Modify: `crates/server/src/http/router.rs`
- Test: manual client smoke test

- [ ] Add `POST /api/ab/get` as an alias to the existing legacy address-book getter if old Sciter client compatibility is still required.
- [ ] Do not remove existing `GET /api/ab` and `POST /api/ab`.
- [ ] Smoke test with authenticated client token:

```bash
curl -H "Authorization: Bearer <client-token>" -X POST http://127.0.0.1:21114/api/ab/get
```

Expected: response contains JSON with a `data` field.

## Phase 2: Recording Visibility In Backend

This phase is independently mergeable. After it ships, uploaded screen recordings appear in the admin console.

### Task 1: Complete Recording Entity

**Files:**
- Modify: `crates/entity/src/lib.rs`
- Modify: `crates/entity/src/record_file.rs`
- Modify: `crates/server/src/bootstrap.rs`

- [ ] Define table `record_files`.
- [ ] Include fields: `id`, `filename`, `path`, `size`, `status`, `remote_id`, `session_id`, `direction`, `created_at`, `updated_at`.
- [ ] Export `record_file` from `crates/entity/src/lib.rs`.
- [ ] Create the table in bootstrap with the existing `create!(...)` pattern.
- [ ] Use status values `uploading`, `complete`, and `removed`.

### Task 2: Add Recording Service

**Files:**
- Create: `crates/server/src/services/recording.rs`
- Modify: `crates/server/src/services/mod.rs`
- Test: `crates/server/src/services/recording.rs`

- [ ] Add service functions `mark_new`, `mark_part`, `mark_tail`, `mark_removed`, `list`, `info`, and `delete`.
- [ ] Update metadata size from filesystem metadata after each successful write.
- [ ] Treat `tail` as completion and set status `complete`.
- [ ] Treat `remove` as `removed` and remove the file idempotently.
- [ ] Run:

```bash
cargo test -p rustdesk-console-server recording
```

Expected: metadata lifecycle tests pass.

### Task 3: Wire `/api/record` To Metadata

**Files:**
- Modify: `crates/server/src/http/api.rs`
- Test: `crates/server/src/http/api.rs`

- [ ] Call `recording::mark_new` after successful `new`.
- [ ] Call `recording::mark_part` after successful `part`.
- [ ] Call `recording::mark_tail` after successful `tail`.
- [ ] Call `recording::mark_removed` after successful `remove`.
- [ ] Preserve current upload response shape `{}`.

### Task 4: Add Admin Recording API

**Files:**
- Modify: `crates/server/src/http/admin.rs`
- Modify: `crates/server/src/http/router.rs`

- [ ] Add `GET /api/admin/record_file/list` with pagination and filename/status filters.
- [ ] Add `GET /api/admin/record_file/download/:id` returning the file stream.
- [ ] Add `POST /api/admin/record_file/delete` accepting `{ "id": <number> }`.
- [ ] Add `POST /api/admin/record_file/batchDelete` accepting `{ "ids": [<number>] }`.
- [ ] Require `AdminUser` for all recording admin routes.
- [ ] Return the same admin response wrapper used by existing list/delete APIs.

### Task 5: Add Admin UI Recording Page

**Files:**
- Modify: `web/src/resource/registry.tsx`
- Create: `web/src/resource/RecordingsResource.tsx`
- Modify: `web/src/i18n.ts`

- [ ] Add a resource page under the existing resource/menu system.
- [ ] Show filename, size, status, created time, updated time, and actions.
- [ ] Add download and delete row actions.
- [ ] Add empty, loading, error, and filtered-empty states.
- [ ] Use existing Kumo semantic tokens only.
- [ ] Run:

```bash
pnpm -C web build
```

Expected: Vite build succeeds.

## Phase 3: Custom Deployment Helpers

This phase is independently mergeable. After it ships, an admin can copy a command or generated config string to configure clients with the current server settings.

### Task 1: Add Deployment Config API

**Files:**
- Modify: `crates/server/src/http/admin.rs`
- Modify: `crates/server/src/http/router.rs`
- Test: `crates/server/src/http/admin.rs`

- [ ] Add `GET /api/admin/deployment/config`.
- [ ] Return current `id_server`, `relay_server`, `api_server`, `ws_host`, and `key` from `WebClientConfigStore`.
- [ ] Return `encoded_config`, built from JSON fields `host`, `relay`, `api`, and `key`, base64url encoded and reversed, matching RustDesk `ServerConfig.encode()`.
- [ ] Return `filename_hint`, formatted as `rustdesk-host=<id>,key=<key>,api=<api>,relay=<relay>.exe`, omitting empty optional segments.
- [ ] Return CLI snippets:

```json
{
  "linux": [
    "sudo rustdesk --option custom-rendezvous-server '<id_server>'",
    "sudo rustdesk --option relay-server '<relay_server>'",
    "sudo rustdesk --option api-server '<api_server>'",
    "sudo rustdesk --option key '<key>'"
  ],
  "windows": [
    "rustdesk.exe --option custom-rendezvous-server \"<id_server>\"",
    "rustdesk.exe --option relay-server \"<relay_server>\"",
    "rustdesk.exe --option api-server \"<api_server>\"",
    "rustdesk.exe --option key \"<key>\""
  ]
}
```

- [ ] Escape shell values safely for generated commands.
- [ ] Run:

```bash
cargo test -p rustdesk-console-server deployment_config
```

Expected: encoded config and filename hint tests pass.

### Task 2: Add Deployment UI

**Files:**
- Create: `web/src/pages/DeploymentPage.tsx`
- Modify: `web/src/components/AppShell.tsx`
- Modify: `web/src/lib/api.ts`
- Modify: `web/src/i18n.ts`

- [ ] Add a menu entry named `部署`.
- [ ] Show current ID server, relay server, API server, WS host, and public key.
- [ ] Show the importable server configuration string with copy action.
- [ ] Show executable filename hint with copy action.
- [ ] Show Linux/macOS and Windows CLI command groups with copy buttons.
- [ ] Include a direct link to the existing WebClient settings page for editing server values.
- [ ] Keep layout consistent with existing admin pages and Kumo tokens.
- [ ] Run:

```bash
pnpm -C web build
```

Expected: Vite build succeeds.

### Task 3: Document One-Click Configuration

**Files:**
- Modify: `README.md`
- Modify: `README.zh-CN.md`

- [ ] Document the server config fields: ID server, relay server, API server, WS host, public key.
- [ ] Document the RustDesk import string behavior.
- [ ] Document executable filename format:

```text
rustdesk-host=<id-server>,key=<public-key>,api=<api-server>,relay=<relay-server>.exe
```

- [ ] Document CLI configuration commands for installed desktop clients:

```bash
sudo rustdesk --option custom-rendezvous-server '<id-server>'
sudo rustdesk --option relay-server '<relay-server>'
sudo rustdesk --option api-server '<api-server>'
sudo rustdesk --option key '<public-key>'
```

## Phase 4: Official Deploy And Assign API

This phase is independently mergeable. After it ships, RustDesk `--deploy` and `--assign` work against this API server.

### Task 1: Add Scoped API Token Model Or Reuse Existing User Tokens

**Files:**
- Modify: `crates/entity/src/user_token.rs`
- Modify: `crates/server/src/services/user.rs`
- Modify: `crates/server/src/http/middleware.rs`

- [ ] Prefer reusing existing `user_tokens` if it can represent long-lived admin API tokens safely.
- [ ] Require Bearer token auth for Pro-compatible script endpoints.
- [ ] Accept `Authorization: Bearer <token>` for `/api/devices/deploy` and `/api/devices/cli`.
- [ ] Require admin privileges for deploy and assign in the first implementation.
- [ ] Add middleware helper `ApiBearerAdmin`.

### Task 2: Implement `/api/devices/deploy`

**Files:**
- Modify: `crates/server/src/http/api.rs`
- Modify: `crates/server/src/http/router.rs`
- Modify: `crates/server/src/services/peer.rs`
- Test: `crates/server/src/services/peer.rs`

- [ ] Add `POST /api/devices/deploy`.
- [ ] Accept JSON body `{ "id": "...", "uuid": "...", "pk": "..." }`.
- [ ] Validate non-empty id and uuid.
- [ ] Return `{ "result": "INVALID_INPUT" }` on invalid body.
- [ ] Return `{ "result": "ID_TAKEN" }` if the requested id belongs to another uuid.
- [ ] Create or update the peer with id, uuid, public key, and default status.
- [ ] Return `{ "result": "OK" }` on success.
- [ ] If explicit deployment is disabled by config, return `{ "result": "NOT_ENABLED" }`.
- [ ] Run:

```bash
cargo test -p rustdesk-console-server deploy_device
```

Expected: OK, invalid input, id taken, and disabled cases pass.

### Task 3: Implement `/api/devices/cli`

**Files:**
- Modify: `crates/server/src/http/api.rs`
- Modify: `crates/server/src/http/router.rs`
- Modify: `crates/server/src/services/peer.rs`
- Modify: `crates/server/src/services/address_book.rs`
- Test: `crates/server/src/services/peer.rs`

- [ ] Add `POST /api/devices/cli`.
- [ ] Accept JSON body with `id`, `uuid`, and optional `user_name`, `strategy_name`, `address_book_name`, `address_book_tag`, `address_book_alias`, `address_book_password`, `address_book_note`, `device_group_name`, `note`, `device_username`, `device_name`.
- [ ] Update peer owner when `user_name` exists.
- [ ] Update peer device group when `device_group_name` exists.
- [ ] Update peer note, device username, and device name when provided.
- [ ] Add/update address-book peer when `address_book_name` exists.
- [ ] Store `strategy_name` as a no-op accepted field in Phase 4 if the strategy engine is not yet implemented, and return a clear error only if strict mode is later added.
- [ ] Return empty body or success text so the RustDesk CLI prints `Done!`.
- [ ] Run:

```bash
cargo test -p rustdesk-console-server devices_cli
```

Expected: assignment updates peer fields and address book data.

### Task 4: Add Admin UI Token And Command Generator

**Files:**
- Modify: `web/src/pages/DeploymentPage.tsx`
- Modify: `web/src/lib/api.ts`
- Modify: `web/src/i18n.ts`

- [ ] Let admin choose or paste an API token for generated commands.
- [ ] Generate deploy command:

```bash
sudo rustdesk --deploy --token '<token>'
```

- [ ] Generate deploy-with-id command:

```bash
sudo rustdesk --deploy --token '<token>' --id '<new-id>'
```

- [ ] Generate assignment command:

```bash
sudo rustdesk --assign --token '<token>' --user_name '<user>' --device_group_name '<group>' --note '<note>'
```

- [ ] Do not render token values into page URLs.
- [ ] Do not store pasted tokens in localStorage.

## Phase 5: Script Compatibility Backlog

This phase is optional and should be split into smaller plans before implementation.

Useful scripts to borrow:

- `devices.py`: list/filter/disable/enable/delete/assign devices. High value after Phase 4 because it maps to fleet operations.
- `device-groups.py`: create/update/delete groups and add/remove devices. High value for deployment segmentation.
- `strategies.py`: list/enable/disable/assign strategies. Medium value; requires a real strategy model and heartbeat application.
- `user-groups.py`: user groups and access rules. Medium value; current app already has simpler groups, but rules need a model decision.
- `ab.py`: extended shared address book CRUD and rules. Medium value; many underlying concepts already exist.
- `audits.py`: `/api/audits/{conn,file,alarm,console}` compatibility. Low-to-medium value; current admin audit APIs already cover connection/file logs.
- `users.py`: user CRUD, invite, 2FA enforcement, force logout. Low value for custom deployment; useful for full Pro API compatibility later.
- `job.py`: signing/build task server. Out of scope for this API server unless the project explicitly becomes a build/distribution platform.

Recommended backlog order:

1. `/api/devices` list/filter and device enable/disable/delete/assign compatibility.
2. `/api/device-groups` list/create/update/delete/add-devices/remove-devices compatibility.
3. Minimal `/api/strategies` CRUD and assignment, then heartbeat strategy application.
4. `/api/user-groups` compatibility if access rules need to mirror Pro behavior.
5. Extended `/api/ab/shared/*` and `/api/ab/rules` compatibility.
6. `/api/audits/*` compatibility aliases.
7. User invite/2FA/force logout compatibility.

## Verification Before Commit

Run targeted checks after each phase:

```bash
cargo test -p rustdesk-console-server
cargo test -p entity
pnpm -C web build
```

Run formatting checks for touched Rust files:

```bash
rustfmt --edition 2021 --check \
  crates/server/src/http/api.rs \
  crates/server/src/http/router.rs \
  crates/server/src/http/payloads.rs \
  crates/server/src/services/audit.rs \
  crates/server/src/services/recording.rs \
  crates/server/src/services/peer.rs \
  crates/entity/src/lib.rs \
  crates/entity/src/record_file.rs
```

Manual acceptance checks:

- Log in to `/_admin/`.
- Open WebClient settings and verify ID server, relay server, API server, WS host, and public key persist after refresh.
- Open deployment page and copy the encoded config, filename hint, and CLI commands.
- Use a RustDesk client to import the generated config string.
- Use an installed RustDesk client to run `--option` commands and verify settings changed.
- Use RustDesk CLI `--deploy --token <token>` and verify the peer appears in admin devices.
- Start a screen recording upload from a RustDesk client and verify file exists under `resources/record`.
- After Phase 2, verify the same recording appears in admin recording list and can be downloaded/deleted.

## Rollback

- Phase 1 rollback: remove only the added compatibility routes and tests; no schema changes are required.
- Phase 2 rollback: keep uploaded files on disk, remove recording admin routes, and leave `record_files` table unused. Do not delete user recording files during rollback.
- Phase 3 rollback: remove deployment helper API/UI; no persisted client state changes occur unless the admin copied and ran commands manually.
- Phase 4 rollback: disable `/api/devices/deploy` and `/api/devices/cli` routes first. Existing peer rows remain valid and do not require migration rollback.

# RustDesk Client HTTP API Contract

This document records the RustDesk client HTTP requests that `rustdesk-console`
tracks for compatibility. It is derived from the upstream client source, not from
server-side assumptions.

## Source Baseline

- RustDesk repository: `https://github.com/rustdesk/rustdesk`
- RustDesk source: `/home/czyt/code/rust/rustdesk`
- RustDesk git commit: `bf206dc309fda20097ef0335c3563812296a5553`
- RustDesk git describe: `1.4.7-9-gbf206dc30`
- RustDesk Cargo version: `1.4.7`
- RustDesk Flutter version: `1.4.7+65`
- RustDesk branch at inspection time: `master`
- Inspected date: `2026-06-18`

Primary client files inspected:

- `src/hbbs_http/sync.rs`
- `src/hbbs_http/account.rs`
- `src/hbbs_http/record_upload.rs`
- `src/common.rs`
- `src/server/connection.rs`
- `src/core_main.rs`
- `src/ui_interface.rs`
- `src/ui/index.tis`
- `src/ui/ab.tis`
- `flutter/lib/models/user_model.dart`
- `flutter/lib/models/group_model.dart`
- `flutter/lib/models/ab_model.dart`
- `flutter/lib/models/model.dart`
- `flutter/lib/common/hbbs/hbbs.dart`
- `flutter/lib/common/widgets/dialog.dart`

When updating RustDesk client support, first update the source baseline above,
then re-run the endpoint matrix below against the new client source.

## Endpoint Matrix

### Account And Login

| Method | Endpoint | Client request fields | Client expects |
| --- | --- | --- | --- |
| `GET` | `/api/login-options` | none | JSON array of strings. OAuth entries are `common-oidc/<json-array>` and/or `oidc/<name>`. Used also as a TLS warm-up URL. |
| `POST` | `/api/login` | Normal login: `username`, `password`, `id`, `uuid`, `autoLogin`, `type`, `deviceInfo.{name,os,type}`. Verification login: `username`, `id`, `uuid`, `autoLogin`, `type`, `verificationCode`, `tfaCode`, `secret`, `deviceInfo`. | Token response: `type=access_token`, `access_token`, `user`. Challenge response: `type=email_check`, `secret`, `tfa_type=email_check|tfa_check`, `user`. |
| `POST` | `/api/logout` | `id`, `uuid` with bearer token | `null`/empty success body. |
| `POST` | `/api/currentUser` | `id`, `uuid` with bearer token | User payload fields: `name`, `display_name`, `avatar`, `email`, `note`, `status`, `is_admin`, optional `verifier`, `info`. |
| `POST` | `/api/oidc/auth` | `op`, `id`, `uuid`, `deviceInfo.{name,os,type}` | Data payload with `code` and `url`. |
| `GET` | `/api/oidc/auth-query` | Query: `code`, `id`, `uuid` | Waiting response with `error`, or the same login token response as `/api/login`. |

### Heartbeat, Strategy, And Sysinfo

| Method | Endpoint | Client request fields | Client expects |
| --- | --- | --- | --- |
| `POST` | `/api/heartbeat` | `id`, `uuid`, numeric `ver`, optional `conns`, `modified_at` | JSON object with `modified_at`, optional `sysinfo`, optional `disconnect`, optional `strategy.config_options`, optional `strategy.extra`. |
| `POST` | `/api/sysinfo` | `id`, `uuid`, `version`, `cpu`, `hostname`, `memory`, `os`, `username`; preset fields listed below. | Plain text `SYSINFO_UPDATED` on success; `ID_NOT_FOUND` may be used by older servers. |
| `POST` | `/api/sysinfo_ver` | empty body | Plain text version marker used to skip unchanged sysinfo uploads. |

Preset fields sent by the client when built-in options are present:

- `preset-user-name`
- `preset-strategy-name`
- `preset-device-group-name`
- `preset-address-book-name`
- `preset-address-book-tag`
- `preset-address-book-alias`
- `preset-address-book-password`
- `preset-address-book-note`
- `preset-device-username`
- `preset-device-name`
- `preset-note`

Heartbeat strategy keys consumed through `strategy.config_options` include normal
permission/access keys and network/server keys. Network/server keys are powerful:
bad values can make clients unreachable, so console should keep them visibly
separate from ordinary permission policies.

### Groups And Peers

| Method | Endpoint | Client request fields | Client expects |
| --- | --- | --- | --- |
| `GET` | `/api/device-group/accessible` | Query: `current`, `pageSize` | Paginated object with `total`, `data[]`; each item needs at least `name`. |
| `GET` | `/api/users` | Query: `current`, `pageSize`, `accessible`, `status` | Paginated object with `total`, `data[]`; user items parse the same user payload fields used by login. |
| `GET` | `/api/peers` | Query: `current`, `pageSize`, `accessible`, `status` | Paginated object with `total`, `data[]`; peer item fields: `id`, `info.device_name`, `info.os`, `info.username`, `status`, `user`, `user_name`, `device_group_name`, `note`. |

### Address Book

| Method | Endpoint | Client request fields | Client expects |
| --- | --- | --- | --- |
| `POST` | `/api/ab/settings` | empty JSON body | `max_peer_one_ab`. |
| `POST` | `/api/ab/personal` | empty JSON body | `guid`, `name`, `rule`. `guid` identifies the synthetic personal address book. |
| `POST` | `/api/ab/shared/profiles` | Query: `current`, `pageSize`, optional `name` | Paginated object with `total`, `data[]`; profile fields: `guid`, `name`, `owner`, `note`, `rule`, optional `info`. |
| `GET` / `POST` | `/api/ab` and `/api/ab/get` | Legacy sync: POST body `{ "data": "<json string>" }`. Inner JSON has `peers`, `tags`, `tag_colors`. | Legacy response object with `data` as a JSON string containing `peers`, `tags`, `tag_colors`; may include `licensed_devices`. |
| `POST` | `/api/ab/peers` | Query: `current`, `pageSize`, `ab`, optional `id`, optional `alias` | Paginated object with `total`, `data[]`, `licensed_devices`. Peer fields match RustDesk `Peer.fromJson`. |
| `POST` | `/api/ab/tags/:guid` | empty JSON body | Array of `{ name, color }`. |
| `POST` | `/api/ab/peer/add/:guid` | `id`, `alias`, `tags`, optional `password`, optional `note`; peer metadata may include `username`, `hostname`, `platform`, `hash`, `forceAlwaysRelay`, `rdpPort`, `rdpUsername`, `loginName`, `sameServer`. | Empty string on success or JSON error. Existing same `id` in the same collection should update/merge instead of creating duplicates. |
| `PUT` | `/api/ab/peer/update/:guid` | Always includes `id`; update fields can be `tags`, `alias`, `note`, `hash`, `password`, `username`, `hostname`, `platform`, `forceAlwaysRelay`, `rdpPort`, `rdpUsername`, `loginName`, `sameServer`/`same_server`. | Empty string on success or JSON error. |
| `DELETE` | `/api/ab/peer/:guid` | JSON array of peer IDs | Empty string on success or JSON error. |
| `POST` | `/api/ab/tag/add/:guid` | `name`, `color` | Empty string on success or JSON error. |
| `PUT` | `/api/ab/tag/rename/:guid` | `old`, `new` | Empty string on success or JSON error. |
| `PUT` | `/api/ab/tag/update/:guid` | `name`, `color` | Empty string on success or JSON error. |
| `DELETE` | `/api/ab/tag/:guid` | JSON array of tag names | Empty string on success or JSON error. |

RustDesk `Peer.fromJson` fields currently include:

- `id`
- `hash`
- `password`
- `username`
- `hostname`
- `platform`
- `alias`
- `tags`
- `forceAlwaysRelay`
- `rdpPort`
- `rdpUsername`
- `loginName`
- `device_group_name`
- `note`
- `same_server`

### Audit

| Method | Endpoint | Client request fields | Client expects |
| --- | --- | --- | --- |
| `POST` | `/api/audit/conn` | Lifecycle events use `id`, `uuid`, `conn_id`, `session_id`, `action=new|close`, `ip`. Peer update event uses `peer=[id,name]`, `type`. Note update can send `note`, `session_id`. | `null` success body. Server stores one lifecycle row per `conn_id`, keeps large numeric `session_id` as a string. |
| `GET` | `/api/audit/conn/active` | Query: `id`, `session_id`, `conn_type` | JSON string audit row id/guid, or empty string. |
| `PUT` | `/api/audit` | `guid`, `note` | `null` success body. |
| `POST` | `/api/audit/file` | `id`, `uuid`, `peer_id`, `type`, `path`, `is_file`, `info`. `info` is a JSON string containing at least `ip`, `name`, `num`, and `files`. | `null` success body. Compatibility mapping follows the Go API: stored `peer_id` is request `id`; stored `from_peer` is request `peer_id`; `from_name`, `ip`, and `num` are parsed from `info` unless explicit top-level values are present. |
| `POST` | `/api/audit/alarm` | `id`, `uuid`, `typ`, `info` | `null` success body. |

### Deployment And Recording

| Method | Endpoint | Client request fields | Client expects |
| --- | --- | --- | --- |
| `POST` | `/api/devices/deploy` | `id`, `uuid`, `pk` with deployment bearer token | JSON object with `result=OK|ID_TAKEN|INVALID_INPUT`, or `error`. |
| `POST` | `/api/devices/cli` | `id`, `uuid`, optional `user_name`, `strategy_name`, `address_book_name`, `address_book_tag`, `address_book_alias`, `address_book_password`, `address_book_note`, `device_group_name`, `note`, `device_username`, `device_name`. | Empty body on success; plain text error on failure. |
| `POST` | `/api/record` | Query `type=new|part|tail|remove`, `file`, and for chunks `offset`, `length`; request body contains bytes for `part`/`tail`. | Empty JSON object or JSON error. |

## Current Console Alignment Notes

- `/api/ab/peer/update/:guid` accepts all fields the Flutter client can send from
  address book edits and recent-peer sync. This includes `username`, `hostname`,
  and `platform`, which are sent by `syncFromRecent`.
- `/api/audit/file` stores fields according to the original Go API mapping and
  parses `info` for `ip`, `name`, and `num`.
- User payloads returned by RustDesk client login/current-user endpoints make
  local `/upload/...` avatars absolute from the public request host. This keeps
  Cloudflare/reverse-proxy deployments compatible with clients and peer avatar
  display while still storing relative paths in the database.
- `/api/sysinfo` consumes all preset fields listed in this document and applies
  first-report user, device-group, strategy, and address-book assignment.
- `/api/heartbeat` supports strategy delivery through `strategy.config_options`;
  `extra` is sent for metadata but standard clients only apply `config_options`.
- `/api/record` is only used when the RustDesk client sends recording upload
  events. The controller toolbar's manual "Start session recording" path records
  locally on the controller and does not call `/api/record` in the inspected
  client source.

## Recheck Procedure

1. Update the source baseline at the top of this document.
2. Search the RustDesk client source for `/api/`, `post_request`,
   `http_request`, `http.get`, `http.post`, `http.put`, and `http.delete`.
3. For every changed call site, compare request fields, query fields, and response
   parsing against `crates/server/src/http/router.rs`,
   `crates/server/src/http/api.rs`, `crates/server/src/http/oauth.rs`,
   `crates/server/src/http/pro_api.rs`, and the relevant service/entity modules.
4. Add fields additively. Do not remove or change existing response field types
   unless the client source proves the old shape cannot work.
5. Add regression tests for any field mapping, persistence, or duplicate-handling
   behavior fixed during the recheck.

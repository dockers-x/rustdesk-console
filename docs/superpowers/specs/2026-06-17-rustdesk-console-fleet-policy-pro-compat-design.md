# RustDesk Console Fleet, Policy, And Pro Compatibility Design

## Goal

Implement the RustDesk client and Pro-compatible management surface in four independently mergeable stages:

1. Runtime client compatibility: heartbeat strategy delivery, pagination compatibility, and contract tests for audit, record, and address book APIs.
2. Deployment lifecycle: deployment tokens, `/api/devices/deploy`, `/api/devices/cli`, and persisted device `pk`/`uuid` state with an hbbs integration decision.
3. Strategy management UI: strategy CRUD, assignment to devices, users, and device groups, and heartbeat application.
4. Pro compatibility facade: `/api/devices`, `/api/strategies`, `/api/device-groups`, `/api/user-groups`, `/api/audits/*`, plus compatibility with RustDesk `res/*.py` scripts.

The admin frontend is part of the scope. UI must stay consistent with the existing React + Cloudflare Kumo console: dense admin tables, restrained forms, semantic tokens, accessible controls, and no new design system.

## Current State

The repository already contains a meaningful subset of the target:

- Runtime client routes exist in `crates/server/src/http/router.rs`: heartbeat, login, sysinfo, group peers/users, audit, device deploy/cli, record upload, OIDC, address book, and WebClient config.
- Recording support exists: `entity::record_file`, `services::record_file`, `/api/record`, admin record listing/download/delete, and `web/src/components/RecordFileActions.tsx`.
- Deployment config helper support exists in `crates/server/src/support/deployment_config.rs` and `web/src/pages/WebClientSettingsPage.tsx`.
- Device deployment routes exist, but they use ordinary client login tokens, ignore `pk`, and treat `strategy_name` as a no-op.
- Heartbeat currently returns the correct strategy shape, but `strategy.config_options` is always empty.
- Pro script routes such as `/api/devices`, `/api/strategies`, `/api/device-groups`, `/api/user-groups`, and `/api/audits/*` do not exist.

## Design Thesis

The internal product model is not "RustDesk Pro route handlers." The internal model is Fleet plus Policy:

- Fleet owns devices, device groups, deployment tokens, deployment events, and device registration material.
- Policy owns strategies, strategy options, assignments, and effective policy resolution.
- Compatibility owns RustDesk client and Pro script route shapes as facades over Fleet, Policy, Workspace, and Evidence.
- Workspace owns address books, shared profiles, tags, and address-book rules.
- Evidence owns audits, active connections, recording files, and operational logs.

Compatibility endpoints may preserve official RustDesk response quirks, but they must not define the internal database model.

## Module Boundaries

### Fleet

Backend files:

- Entities: `deployment_token`, `deployment_event`, peer registration fields added to `peer`.
- Services: `services::deployment`, `services::peer`, `services::group`.
- Admin HTTP: `/api/admin/deployment_token/*`, existing `/api/admin/peer/*`, existing `/api/admin/device_group/*`.
- Client HTTP: `/api/devices/deploy`, `/api/devices/cli`.
- Pro facade: `/api/devices`, `/api/device-groups`.

Frontend:

- Resource entry for deployment tokens.
- Existing devices and device groups remain resource-table pages.
- WebClient settings page remains the place for install/config command generation.

### Policy

Backend files:

- Entities: `strategy`, `strategy_assignment`.
- Services: `services::strategy`.
- Admin HTTP: `/api/admin/strategy/*`, `/api/admin/strategy_assignment/*` or focused assignment actions under `/api/admin/strategy`.
- Client HTTP: heartbeat strategy resolution in `/api/heartbeat`.
- Pro facade: `/api/strategies`, `/api/strategies/{guid}`, `/api/strategies/{guid}/status`, `/api/strategies/assign`.

Frontend:

- A first-class Strategies page.
- Strategy editor supports option key/value entries from a curated allowlist.
- Assignment UI supports devices, users, and device groups with visible precedence.

### Compatibility

Backend files:

- Keep public RustDesk client handlers in `http/api.rs` or split into `http/client_api.rs` if the file becomes too large.
- Add Pro facade handlers in a separate `http/pro_api.rs` when endpoint count grows beyond a small shim.
- Do not put admin UI-only behavior in Pro facade routes.

Frontend:

- No separate Compatibility page in the first implementation.
- Add a diagnostics checklist later only if script compatibility needs operator visibility.

### Workspace

Backend files:

- Reuse `address_book`, `address_book_collection`, `address_book_collection_rule`, and `tag`.
- Add Pro facade aliases for shared address book and rules only after strategy and deployment are stable.

Frontend:

- Keep existing address book resource pages.
- Avoid duplicating address book management inside deployment or strategy pages.

### Evidence

Backend files:

- Reuse `audit_conn`, `audit_file`, `record_file`, `active_connection`.
- Add Pro audit facade routes over existing audit services.

Frontend:

- Keep resource pages for audits and recordings.
- Add filters only where needed for script and operator parity.

## Data Model

### `peers` additions

Add the following optional columns to `peer`:

- `pk`: text, default empty string. Stores the client public key from `/api/devices/deploy`.
- `guid`: text, default empty string. Stable external identifier for Pro-compatible device APIs.
- `disabled`: bool or integer status, default enabled. Used by `/api/devices/{guid}/disable` and `/enable`.

If the project prefers no boolean migrations, use `status: i32` with `1 = enabled`, `2 = disabled`, matching user status conventions.

### `deployment_tokens`

Fields:

- `id`: primary key.
- `token_hash`: text, unique. Store only a hash of the token.
- `name`: text.
- `scopes`: text JSON array. Initial scopes: `deploy`, `assign`, `strategy_assign`, `address_book_assign`.
- `default_user_id`: integer, default 0.
- `default_device_group_id`: integer, default 0.
- `default_strategy_id`: integer, default 0.
- `expires_at`: integer timestamp, default 0 means no expiry.
- `max_uses`: integer, default 0 means unlimited.
- `used_count`: integer, default 0.
- `revoked_at`: integer timestamp, default 0.
- `created_by`: integer user id.
- `created_at`, `updated_at`.

Plain tokens are shown only once at creation time. Existing token rows show metadata but never the raw token.

### `deployment_events`

Fields:

- `id`: primary key.
- `token_id`: integer.
- `peer_id`: text RustDesk id.
- `uuid`: text.
- `action`: text, values `deploy` and `assign`.
- `result`: text, values such as `OK`, `INVALID_INPUT`, `ID_TAKEN`, `UNAUTHORIZED`, `ERROR`.
- `message`: text.
- `ip`: text.
- `created_at`.

### `strategies`

Fields:

- `id`: primary key.
- `guid`: text, unique.
- `name`: text, unique.
- `note`: text.
- `enabled`: integer, `1` enabled and `2` disabled.
- `config_options`: text JSON object mapping RustDesk option keys to string values.
- `extra`: text JSON object for client capability metadata.
- `modified_at`: integer timestamp. Update on every option/status change.
- `created_at`, `updated_at`.

### `strategy_assignments`

Fields:

- `id`: primary key.
- `strategy_id`: integer. `0` means explicit unassign when needed.
- `target_type`: text enum: `peer`, `user`, `device_group`.
- `target_id`: text. For peer use peer guid when available, else RustDesk id; for user/group use their stable id or guid.
- `priority`: integer. Lower number wins when target specificity is equal.
- `created_at`, `updated_at`.

Effective strategy precedence:

1. Peer assignment.
2. User assignment.
3. Device group assignment.
4. No strategy.

Within the same level, use lowest priority and then newest assignment as tie-breaker. Disabled strategies are ignored.

## Runtime Client Compatibility

### Heartbeat

`POST /api/heartbeat` must continue accepting body fields `id`, `uuid`, `version`, `ver`, `conns`, and `modified_at`.

Response must include:

```json
{
  "modified_at": 123,
  "strategy": {
    "config_options": {
      "enable-file-transfer": "N"
    },
    "extra": {
      "translate_mode": "true"
    },
    "translate_mode": true
  }
}
```

Rules:

- If no effective strategy exists, echo the client's `modified_at` and return empty `config_options`.
- If an effective strategy exists and its `modified_at` differs from client `modified_at`, return the server `modified_at` and the strategy options.
- Keep `extra.translate_mode` as a string.
- Keep root `strategy.translate_mode` only for backward compatibility.
- Continue active connection sync and pending disconnect behavior.

### Pagination Compatibility

Client-facing list endpoints must accept both `page` and `current` as page number aliases:

- `/api/users`
- `/api/peers`
- `/api/device-group/accessible`
- `/api/ab/peers`
- Pro facade list endpoints

`pageSize` remains the page-size parameter.

### Contract Tests

Add focused tests for:

- Heartbeat shape deserializes into the RustDesk client's `StrategyOptions` shape.
- `current` pagination alias maps to the expected page.
- `/api/record` filename validation and success response shape.
- `/api/audit/conn/active` empty and found cases at service level.
- Legacy `/api/ab/get` route remains present.

## Deployment Lifecycle

### Authentication

`/api/devices/deploy` and `/api/devices/cli` must accept either:

- Existing user access token, for backward compatibility.
- Deployment token with the required scope.

Deployment token is preferred in generated commands.

### `/api/devices/deploy`

Input:

```json
{
  "id": "123456789",
  "uuid": "base64-uuid",
  "pk": "base64-public-key"
}
```

Response:

- `{ "result": "OK" }`
- `{ "result": "NOT_ENABLED" }` if explicit deployment is disabled by config.
- `{ "result": "INVALID_INPUT" }`
- `{ "result": "ID_TAKEN" }`
- `{ "error": "..." }` for unexpected failures.

Behavior:

- Validate id and uuid are non-empty.
- Store uuid and pk on peer.
- Reject id reuse when an existing peer has a different non-empty uuid.
- Assign default user/group/strategy from deployment token when configured.
- Record a deployment event.

### `/api/devices/cli`

Accept existing RustDesk fields:

- `id`, `uuid`
- `user_name`
- `strategy_name`
- `address_book_name`, `address_book_tag`, `address_book_alias`, `address_book_password`, `address_book_note`
- `device_group_name`
- `note`, `device_username`, `device_name`

Behavior:

- Apply user assignment.
- Apply device group assignment.
- Apply peer note, hostname, and username updates.
- Add/update address-book entry when address-book fields are provided.
- Resolve `strategy_name` and create a peer-level strategy assignment.
- Return empty body on success because the RustDesk CLI prints `Done!` for empty success text.

### hbbs Integration Decision

Stage 2 must inspect the local rustdesk-server deployment mode before final completion:

- If console and hbbs share the same peer database, store compatible `uuid` and `pk` fields in the shared schema.
- If hbbs uses its own database, add a documented sync path or mark deployment as API-side only with a clear admin warning.

The stage is not complete until one of those two states is implemented and documented.

## Strategy Management UI

Add a Strategies page that follows the existing resource-table style:

- Table columns: name, enabled status, option count, assignment count, modified time, note.
- Row actions: edit, enable/disable, assign, delete.
- Editor fields: name, note, enabled, config options.
- Option editor: key selector, value input/select, remove action.
- Assignment drawer/modal: target type segmented control, searchable devices/users/device groups, assigned targets table.
- Empty states: no strategies, no assignments, no search results.
- Error states near the failing form field or table action.

Initial option allowlist:

- `enable-keyboard`
- `enable-clipboard`
- `enable-file-transfer`
- `enable-camera`
- `enable-terminal`
- `enable-audio`
- `enable-tunnel`
- `enable-remote-restart`
- `enable-record-session`
- `enable-block-input`
- `enable-privacy-mode`
- `approve-mode`
- `verification-method`
- `allow-auto-disconnect`
- `auto-disconnect-timeout`
- `whitelist`
- `allow-remote-config-modification`

The UI can store string values directly. For boolean-like RustDesk options, present enabled/disabled controls but serialize to the string values expected by the client.

## Pro Compatibility Facade

The facade must use `Authorization: Bearer <token>` like the RustDesk scripts.

### Devices

Routes:

- `GET /api/devices`
- `POST /api/devices/{guid}/disable`
- `POST /api/devices/{guid}/enable`
- `DELETE /api/devices/{guid}`
- `POST /api/devices/{guid}/assign`

Response list items must include at least:

- `guid`
- `id`
- `device_name`
- `user_name`
- `group_name`
- `device_group_name`
- `last_online`
- `status`

### Device Groups

Routes:

- `GET /api/device-groups`
- `POST /api/device-groups`
- `PATCH /api/device-groups/{guid}`
- `DELETE /api/device-groups/{guid}`
- `POST /api/device-groups/{guid}`
- `DELETE /api/device-groups/{guid}/devices`

### Strategies

Routes:

- `GET /api/strategies`
- `GET /api/strategies/{guid}`
- `PUT /api/strategies/{guid}/status`
- `POST /api/strategies/assign`

### User Groups

Routes:

- `GET /api/user-groups`
- `POST /api/user-groups`
- `PATCH /api/user-groups/{guid}`
- `DELETE /api/user-groups/{guid}`
- `POST /api/user-groups/{guid}`

### Audits

Routes:

- `GET /api/audits/conn`
- `GET /api/audits/file`
- `GET /api/audits/alarm`
- `GET /api/audits/console`

`console` may return an empty compatible list until console audit events exist.

## UI/UX Rules

Use the existing console design system:

- Keep Kumo components and existing semantic tokens.
- Keep admin density: tables first, drawers/dialogs for edit flows, compact filters above tables.
- Preserve accessible labels, visible focus, 44px minimum hit targets where actions are clickable.
- Use icons only when the existing console already uses them or Kumo provides them.
- Do not introduce decorative gradients, marketing sections, hero layouts, nested cards, or a new color theme.
- Do not add a separate navigation concept for compatibility endpoints.
- Long identifiers use monospace truncation with `title` tooltip, matching existing `monoCol`.

## Verification

Backend:

```bash
cargo test -p rustdesk-console-server heartbeat
cargo test -p rustdesk-console-server record
cargo test -p rustdesk-console-server audit
cargo test -p rustdesk-console-server strategy
cargo test -p rustdesk-console-server deployment
```

Frontend:

```bash
pnpm -C web build
```

Script compatibility:

```bash
python /home/czyt/code/rust/rustdesk/res/devices.py --url http://127.0.0.1:21114 --token <token> view
python /home/czyt/code/rust/rustdesk/res/device-groups.py --url http://127.0.0.1:21114 --token <token> list
python /home/czyt/code/rust/rustdesk/res/strategies.py --url http://127.0.0.1:21114 --token <token> list
python /home/czyt/code/rust/rustdesk/res/user-groups.py --url http://127.0.0.1:21114 --token <token> list
python /home/czyt/code/rust/rustdesk/res/audits.py --url http://127.0.0.1:21114 --token <token> conn
```

Manual client checks:

- Start a RustDesk client configured with this API server.
- Confirm `/api/sysinfo` creates or updates the peer.
- Assign a strategy and confirm heartbeat updates local `strategy_timestamp`.
- Run `rustdesk --deploy --token <deployment-token>`.
- Run `rustdesk --assign --token <deployment-token> --strategy_name <name>`.

## Rollback

- Strategy tables can remain unused if heartbeat strategy resolution is disabled.
- Deployment token routes can be disabled without deleting peer rows.
- Pro facade routes can be removed without affecting admin UI routes.
- Recording files and metadata must not be deleted during rollback.

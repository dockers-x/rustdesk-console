# Developer access contract

## Scope

Independent developer navigation: API keys, SDK examples, and public OpenAPI documentation. This is a standard Console capability, not part of the optional self-signed-client feature. Existing RustDesk client and admin-session protocols must remain unchanged.

Delivery: Console tests/build and release Docker verification first; then a server release pins that exact Console digest. SDK repositories: `lib-x/rustdesk-console-sdk-rs`, `lib-x/rustdesk-console-sdk-go`, `lib-x/rustdesk-console-sdk-ts`. Publish the Rust crate as `rustdesk-console-sdk` if available; do not claim npm publication unless actually published.

## Authentication and keys

- Dedicated API keys, not login tokens or deployment tokens. `Authorization: Bearer rdc_<random>` is accepted only on `/api/v1` management endpoints.
- At least 32 cryptographically random bytes; only SHA256 hash and a non-secret display prefix persist. Return plaintext once on creation with `Cache-Control: no-store`; never put it in URLs, logs, browser persistence, query/mutation caches, examples or OpenAPI defaults.
- Keys belong to the creating account. Re-read owner status and role on every request; deletion, disabling, must-change-password and demotion take effect on subsequent requests. Requested scopes cannot exceed the owner's current permissions.
- Default read-only scopes and expiry90days; expiry must be future and at most365days. Owners list/revoke their keys using the existing backend session only. API keys cannot create/revoke keys or access ordinary admin/client routes.
- No new cross-process cache or delayed revocation state. Database failure denies access instead of treating it as an anonymous or empty successful request.

## Key management (backend session, existing Console response envelope)

- `GET /api/admin/developer/scopes`: `{code:0,data:[{id,label,admin_only}]}`.
- `GET /api/admin/developer/keys`: `{code:0,data:[KeyMetadata]}` for current owner.
- `POST /api/admin/developer/keys`: body `{name,scopes:[string],expires_at:unix_seconds}`; returns `{code:0,data:{key:string,metadata:KeyMetadata}}`, no-store.
- `POST /api/admin/developer/keys/{id}/revoke`: owner-only, idempotent; `{code:0,data:null}`.
- Metadata: `{id,name,prefix,scopes:[string],expires_at,revoked_at,created_at}`. Times unix seconds, revoked_at=0 means not revoked. No hash or plaintext fields.

## Public management API

Base: `/api/v1`. Explicit bounded pagination: `page` starts at1, `page_size` defaults30, maximum100. Numeric IDs below are Console row IDs, not RustDesk connection IDs. `Device.id` is the separate RustDesk connection ID string.

Success: single `{data:T}`, list `{data:T[],total:number,page:number,page_size:number}`. Delete success `{data:null}`. Failures use actual HTTP status and `{error:{code:string,message:string}}`; SDKs must preserve status/code/message without including credentials. Unknown fields may be added without breaking clients.

| Method/path | Scope | Visibility/body |
| --- | --- | --- |
| GET `/me` | `profile:read` | Current account User |
| GET `/devices` | `devices:read` | Admin all, ordinary user own; optional `id` filter |
| GET `/devices/{id}` | `devices:read` | Same visibility |
| PATCH `/devices/{id}` | `devices:write` (admin only) | `{alias?,user_id?,group_id?}` |
| DELETE `/devices/{id}` | `devices:write` (admin only) | Existing cleanup semantics |
| GET `/users` | `users:read` (admin only) | User page |
| GET `/users/{id}` | `users:read` (admin only) | User |
| POST `/users` | `users:write` (admin only) | `{username,password,email?,nickname?}`; no role escalation parameter |
| PATCH `/users/{id}` | `users:write` (admin only) | `{email?,nickname?,status?}`; status1 enabled/2 disabled |
| DELETE `/users/{id}` | `users:write` (admin only) | No deleting self; existing cleanup semantics |
| GET `/device-groups` | `device-groups:read` (admin only) | DeviceGroup page |
| POST `/device-groups` | `device-groups:write` (admin only) | `{name,note?}` |
| GET `/strategies` | `strategies:read` (admin only) | Strategy page |
| GET `/address-books` | `address-books:read` | Own entries only |
| POST `/address-books` | `address-books:write` | `{id,alias?,hostname?,platform?,note?}`; creates own legacy collection0 entry |
| DELETE `/address-books/{id}` | `address-books:write` | Own entry only |

Stable response fields:

- User: `{id:number,username:string,email:string,nickname:string,is_admin:boolean,status:number}` (no password/TOTP).
- Device: `{row_id:number,id:string,alias:string,hostname:string,username:string,os:string,user_id:number,group_id:number,status:number,last_online_time:number}` (no peer pk/uuid).
- DeviceGroup: `{id:number,name:string,note:string}`.
- Strategy: `{id:number,name:string}` (initial discovery only, no write claim).
- AddressBook: `{row_id:number,id:string,alias:string,hostname:string,platform:string,note:string}` (no password/hash).

## OpenAPI and SDKs

- Public unauthenticated `GET /api/v1/openapi.json`: OpenAPI3.0.3, all management operations, request/response schemas, scopes/admin-only/owner restrictions, error statuses and bearer security scheme. No internal hostname or key examples. No global server URL hardcoded to official RustDesk.
- UI includes downloadable spec, endpoint/scopes reference, repository links and runnable Node/TypeScript, Rust, Go examples using environment variables `RUSTDESK_CONSOLE_URL` and `RUSTDESK_CONSOLE_API_KEY`.
- All three SDKs expose typed models and named methods for every management operation above, pagination, configurable base URL/timeout, structured errors. No auto retry of mutations. Disable redirects or reject cross-origin redirects to prevent forwarding keys. No API-key Debug/Display/log exposure. SDKs never bypass TLS certificate verification.
- Rust: async reqwest, public crate docs, example, unit tests with a real local mock HTTP server, release/package verification and crates.io publication.
- Go: standard net/http with caller context, module `github.com/lib-x/rustdesk-console-sdk-go`, tests with httptest, example, tagged release.
- TypeScript: Node18+ fetch, AbortSignal/timeout, dependency-minimal typed package, tests using local server/fetch mock, example, tagged release and install-from-GitHub/build instructions.

## Acceptance

- Key material is not returned by list; no-store creation; expired/revoked/wrong-scope/disabled-owner/demoted-owner requests fail; API keys cannot access session admin/client endpoints.
- Ordinary keys cannot read another user's device/address book or acquire admin scopes; write APIs produce persistent changes and report failures.
- OpenAPI parses and route/SDK coverage matches the table. SDK contract tests cover success, auth header, query/body, structured error, redirects and absence of credential logging.
- Console workspace Rust tests/Clippy, Web tests/typecheck/build, responsive browser smoke and real API lifecycle smoke pass. Preserve old client behavior when API keys are unused.

# Self-signed client feature contract

All routes use the existing `api-token` backend authentication. Responses use
`{code:0,message:"success",data:...}` except validation/disabled errors, which
return an HTTP error with `{error:"..."}`. Request and exported content limits
are 1 MiB. This feature defaults to disabled and never changes ordinary client
API behavior or builds clients.

## Routes

- `GET /api/admin/features`: authenticated backend users; data is
  `{self_signed_client:{enabled:boolean}}` for navigation.
- `GET /api/admin/features/self-signed-client`: admin, even while disabled;
  data is `{enabled:boolean,public_key:string,key_configured:boolean,fingerprint:string}`.
- `PUT /api/admin/features/self-signed-client`: admin; toggle-only updates are
  permitted while disabled. A request with `enabled:false` and either key
  field present (including an empty value) returns HTTP 403. A request with
  `enabled:true` may configure/import keys and enable the feature atomically.
  body `{enabled:boolean,public_key?:string,private_key?:string}`. Omitted keys
  preserve the existing pair. Supplying both empty strings clears the pair.
  Private key accepts standard base64 Ed25519 seed32 or libsodium secret64;
  public key, when supplied, must match the derived public32. Returns the
  redacted settings shape, never the secret.
- `POST /api/admin/features/self-signed-client/keys/generate`: admin, enabled
  only; generates and persists a new keypair. Only this generation response
  returns `{enabled,public_key,key_configured,fingerprint,private_key}`, where
  `private_key` is standard base64 libsodium secret64 for the newly generated
  pair. The response has `Cache-Control: no-store`. The UI downloads the pair
  immediately without putting its secret into query/mutation caches. Subsequent
  settings reads and key imports never return the private key; this is not an
  endpoint for exporting an existing key.
- `POST /api/admin/features/self-signed-client/sign`: admin, enabled and keyed;
  body `{app_name:string,branding:Record<string,string>,default_settings:Record<string,string>,override_settings:Record<string,string>}`.
  Each map defaults to `{}`. Unknown request fields and unsupported branding
  keys are rejected. Advanced setting map keys are exported verbatim and are
  not a claim that arbitrary keys have a client consumer. Data is
  `{filename:"custom.txt",content:string,public_key:string,fingerprint:string,source_hint:string}`;
  `content` is standard base64(signature64 || compact client JSON). The UI
  downloads `content` as `custom.txt`; no secret is included.
- `GET /api/admin/features/self-signed-client/catalog`: admin, enabled only;
  returns `{branding:string[],config:string[],local:string[],display:string[],builtin:string[],expert_note:string}`.
  The catalog snapshots all four key arrays in client `bd9262397` and shared
  `5d52530b6`, plus six source-verified HARD_SETTINGS keys. It is not a claim
  that arbitrary extra keys have consumers or every platform supports each key.
- `GET /api/admin/features/self-signed-client/profile`: admin, enabled only;
  data is the `sign` request shape. Default draft is app_name `RustDesk` and
  three empty maps.
- `PUT /api/admin/features/self-signed-client/profile`: admin, enabled only;
  body is the `sign` request shape, validated and persisted atomically with the
  feature settings. Returns the saved draft. Does not store signed artifacts.

`app_name` maps to `app-name`; default/override maps map to `default-settings`
and `override-settings`; allowlisted branding keys become top-level HARD_SETTINGS.
The stock client verifies a hard-coded vendor public key: users must replace
`read_custom_client`'s `KEY` in their own client source and build it themselves.
This service never signs as RustDesk or produces an official client binary.

Storage is `<config-parent>/features/self-signed-client.json` (directory 0700,
file 0600 on Unix). Enabled/public/private are persisted in one atomic file.
Newly generated private keys are returned only in the generation response for
an immediate local backup. Back up the full feature file using server-side
administrative tooling to preserve its profile and enabled flag as well.
Signing requires enabled; enabling does not require a key.

## Acceptance

- Disabled signing, generation and catalog return HTTP 403.
- Disabled key import/clearing returns HTTP 403; toggle-only updates remain
  available, and importing keys while explicitly enabling is supported.
- Generated signatures verify as Ed25519 signed-message blobs; payload matches
  `src/common.rs::read_custom_client`'s JSON structure.
- Wrong public/private pair and malformed secret64 are rejected without
  changing persisted state. Settings get/put and sign never expose private keys.
  Generation returns only its new private key with `Cache-Control: no-store`;
  its base64 secret64 and public32 must form a valid pair.
- Persistence survives reload, with private directory/file modes on Unix.

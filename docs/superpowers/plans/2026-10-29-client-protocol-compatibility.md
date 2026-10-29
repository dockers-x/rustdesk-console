# Current Client Protocol Compatibility Implementation Plan

**Goal:** Correct Console behavior against RustDesk client `bd9262397`, without changing the client; add the explicitly approved paired hbbs switch-grant support.

**Architecture:** Keep existing HTTP shapes and legacy paths. Add narrowly scoped service functions and additive tables only where current client audit messages require persistence. Use existing SeaORM transactions, pagination, and authorization.

**Tech Stack:** Rust, Axum, SeaORM; existing SQLite/MySQL/PostgreSQL support.

**Spec:** `docs/rustdesk-client-http-api-contract.md`; the current client's `src/hbbs_http/sync.rs`, `src/server/connection.rs`, and `flutter/lib/models/ab_model.dart` override outdated contract notes.

## Constraints

- No client customization or submodule changes. User explicitly approved paired hbbs switch-grant implementation, server commits/push, and a ten-second retryable routing capability with endpoint-enforced single-use login.
- Follow-up authorization: complete the workspace review/refactoring, bump Console, commit/push and publish a Console release; verify successful Docker publication before the subsequent RustDesk release (repository clarification pending).
- Preserve old requests without audit nonce and all existing authorization checks.
- Do not invent LocalConfig distribution: current heartbeat consumes only Config.
- Do not return successful switch-grant registration without a usable trusted-key and hbbs grant-consumption contract. Record this cross-service limitation explicitly.
- Do not implement automatic strategy rollback: the current client applies options incrementally and has no original-value restoration protocol. Document explicit empty-value reset semantics instead.

## Tasks

- [x] Fix tag rename/delete so peers in the same owner and address book change atomically; preserve other owners/books. Add focused database regression tests.
- [x] Persist current client's alarm fields and expose real admin-only paginated `/api/audits/alarm`; preserve the unrelated console-audit endpoint. Add focused persistence/query tests.
- [x] Deduplicate nonce-bearing connection/file audit retries transactionally and retain their raw association metadata. Keep nonce-less requests on the existing implementation path. Add focused retry/rollback tests.
- [x] Return an empty 2xx audit success body, not JSON null; return a retryable 5xx on storage failure, as required by the current client's `post_audit_async`.
- [x] Allow current Config keys `id-whitelist`, `allow-scope-violation-alarm`, and `allow-scope-violation-close`; expose them using existing strategy option metadata and translations. Add whitelist tests.
- [x] Refresh the HTTP contract baseline and document LocalConfig, strategy reset, switch-grant, and registration trust/consumer limits.
- [x] Run focused tests, then `cargo test --workspace --locked`; check frontend with its existing typecheck/build command if frontend metadata changes.
- [x] Review final diff for isolation, migration compatibility, and unchanged legacy behavior.
- [x] Per the user's explicit clarification, apply `rust-review` and `rust-best-practices` to the entire Console Rust workspace, not only the new diff; run existing frontend type/build checks too. Fix confirmed issues without mechanical rewrites and report the full review surface and remaining limitations.

## Verification Baseline

The initial offline run was blocked by uncached `bcrypt`; the locked online run
subsequently passed all 186 tests after building the embedded admin UI. Frontend
baseline: six tests passed and production build succeeded. Final validation is
rerun after the full-workspace review fixes and UI changes.

Final Console candidate: 206 Rust workspace tests pass after the version bump,
six frontend tests pass, and the production frontend builds. Ordinary workspace
Clippy completes with existing style warnings; `-D warnings` is not clean and
is not represented as passing. Browser checks cover the specified six widths,
Chinese/dark and English/light pages, mobile focus/inert/Escape behavior,
reduced motion, wide table containment, and dialog bounds at 320x844/720x400.
Automated accessibility checks report no violations on overview/settings,
with contrast and empty-table checks requiring manual review.

## Approved Scope Additions

- Whole-workspace production-code review: HTTP, all services, all entities,
  auxiliary crates, bootstrap/config/support. Fix confirmed security, correctness
  and blocking async-work issues, not mechanical mass rewrites.
- UI polish preserves Kumo, branding, real operational data and responsive
  navigation. Apply the requested UI/motion skills; verify light/dark,
  Chinese/English, narrow viewports and reduced motion.
- Evaluate existing-client Pro-like capabilities; record partial and unsupported
  boundaries in `docs/pro-feature-compatibility.md`. No custom client builds.
- Console commits should separate protocol functionality, safety/correctness,
  UI and version/release changes. Server changes receive their own commit and
  push, without a submodule bump.

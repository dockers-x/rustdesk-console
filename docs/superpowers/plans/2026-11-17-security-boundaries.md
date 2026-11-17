# Security boundary fixes implementation plan

> Execute inline; the user authorized implementation, commits and pushes. Use Rust best practices and verification-before-completion, then a fresh code review.

**Goal:** Prevent device-report privilege changes and enforce current per-device connection authorization in the paired hbbs.

**Architecture:** Console owns current account/device permissions. hbbs queries a bounded, fail-closed HTTP authorization endpoint when mandatory login is enabled. Device reports cannot assign ownership or policies. Anonymous reports require an explicit legacy compatibility opt-in.

**Constraints:** No client repository changes; no deployment or release; preserve unrelated `.nightshift/` state; no production probing. Remote endpoint passwords and approval remain required. IP-direct connections bypass hbbs and require network controls.

## Tasks

Implementation and local boundary checks complete; release delivery is tracked separately below.

- [x] Console report guard: default reject anonymous heartbeat/sysinfo/audit writes; authenticate and bind reports to enabled owned devices; legacy opt-in never restores preset writes. Remove login/logout ownership changes.
- [x] Login protection: trust forwarded IPs only from configured proxies, disable ID-only trusted-device verification exemptions, issue unpredictable session tokens.
- [x] Connection ACL: permit enabled registered targets for admins, owners; reject revoked/expired/disabled sessions, unowned targets and self-added address-book entries.
- [x] hbbs: revalidate token and target via Console for punch/relay, bound timeout/concurrency/response, refuse missing configuration or lookup failure; preserve target-bound signed switch grants.
- [x] Add boundary regression tests and upgrade documentation, run available checks, review, commit and push both repositories.

## Review focus

Anonymous preset fields, fabricated device UUIDs, forged forwarding headers, revoked tokens, manually added address-book targets, missing/offline authorization endpoint, and unchanged legitimate deployment and shared-device workflows.

## Rulings during implementation

- Address-book entries can be inserted by shared-collection writers. They are not trustworthy device connection grants. The first release allows only canonical device owners and administrators; shared directories alone never confer routing access.
- Stock telemetry has no device proof. Strict mode rejects it, and the documented opt-in retains only untrusted reporting, never management presets. Stock recording uploads are disabled in the current client source; recording mutations require an administrator.
- User requested a release after verification: Console v0.2.42, paired Server v0.2.44. S6 must pin the verified new Console image digest before its release.
- User reports WebClient entry, including latest native clients. Source review has not established the incident root cause; tests and release notes must not claim otherwise.
- Additional review found audit identity/notes and MFA guessing paths. Canonical local audit IDs, scoped notes, single-attempt challenges and a persistent five-challenge account quota close those source paths.

## Release delivery

Publish Console v0.2.42 after CI; verify its image digest before pinning the S6 base and publishing Server v0.2.44. Verify final release assets and image manifests.

# Machine-local development setup

Scope: development environment migration. No app unit or release acceptance.
Original app integration batch base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Provenance

- Original checkout start: `783efd28a882fa200dc1f472dae90d66bc0f03e1`.
- Infrastructure session: `s-20261109T152245Z-42d01b37`.
- Storage instructions and ignore rules: `4b325bcc`, on master.
- Existing prepared sync patches and review reports remain under ignored `build/`.
- Original VM retained on T9. Active runtime is `.local/meguri-vm/`.

## Transfer and privacy

The source VM was stopped before copying. Only its Tart application, dedicated
SSH directory, disk, configuration and NVRAM were copied. Runtime sockets and
old download caches were excluded. Shared OrbStack storage was not migrated.
The current Docker data path and checkout build paths resolve to internal storage.

The disk copy preserved zero regions and compared the full source and destination
SHA-256 values before publication. Configuration, NVRAM and the Tart executable
also matched. Source file bindings and timestamps remained unchanged. The local
root is private, and the SSH key retains restricted permissions. The startup
guard remained in place until validation completed. The runtime wrappers resolve
their own directory and contain no T9 dependency.

`.local/` was ignored before copying. No runtime files are tracked. Docker image
contexts use existing allowlists; the filtered implementation view excludes this
root. Private transfer details and checksums remain in `.local/`.

## Independent review

Reviewer: `machine_migration_review`, read-only infrastructure review.

- Storage documentation, ignore rules and wrapper templates: no P0–P2 findings.
- One-off copy helper: review caught a write inside an assertion. All safety
  assertions became explicit checks, with unconditional writes, before execution.
  Final review found no P0–P2 findings.
- Account diagnostic: final review found no P0–P2 findings. A negative legacy
  preference lookup remains WARN and required, with account status unverified.
  Positive detection and actual signing/profile checks remain unchanged. No
  signing receipt automatically makes doctor pass. The current milestone manifest
  has no account setup validator; do not infer future acceptance from unverified.

## Verification

- Docker Rust canary: `20261109T151545Z-12609`, passed 1 test.
- Host runner: `20261109T152014Z-16904`, passed 29 tests.
- Isolated implementation image and boundary probes: passed.
- Normal Mac build: `20261109T152602Z-63136`, passed.
- iOS simulator build: `20261109T152918Z-67499`, passed.
- Swift smoke tests: `20261109T153143Z-70923`, passed 7 tests.
- Team-signed Mac build after owner sign-in: `20261109T153733Z-78347`, passed.
- Local VM boot, SSH and pool status: passed through MG; runner is VM.
- One leased iPhone simulator booted through MG and was released successfully.
- Account diagnostic tests: `20261109T155353Z-86665`, passed 2 tests, including
  warning/read-only/schema checks and the original positive path.
- Privacy checks on the migration instructions and diagnostic changes: passed.

Xcode 27.2 (27B52) is installed on both host and guest. The owner completed account
sign-in and global Xcode selection. The initial team-signing failure and refused
source-only check are retained. After those owner actions, signing and the
source-only Xcode check passed. Doctor still reports the legacy account probe as
unverified; the separate successful signed build is the current signing evidence.

App acceptance stays open. The original batch gate and required app, device
and trial checks still need their records. This setup accepts none of U113,
U114 or U151.

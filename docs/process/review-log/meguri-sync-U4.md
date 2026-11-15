# Account-service substrate review

Unit start: `938e72db8210f880e0c88eb681acf676110571bb`.
Original coherent-batch base: `38235917e6a025daa57625c582e90d253aae62e3`.
Focused verification is complete; acceptance awaits the combined foundation gate.

## Delivered behavior

A standalone MIT Rust service owns one locked data root, shared account/session
metadata and isolated per-account storage reservations. It supplies operator-
controlled first setup, invitation and password-reset capabilities, browser and
native credentials, authorization, revocation, administrator recovery, and
bounded request/hash/temporary-payload resources. Browser routes enforce the
configured origin and trusted-peer policy. Collection reservations still hold
substrate markers; they are not synchronized collection databases.

Production authors and fixers ran in fresh filtered clean-room contexts. All
nineteen account-service session records are retained as individual commits.
Their original session-only commits are inventoried at
`/private/tmp/meguri-sync-U4-session-commits.txt`; the ledger carries their roles
and original bases. No Anki implementation or private collection was supplied.

## Review and repair evidence

The original 39-test snapshot passed before independent review found material
storage, authority, ingress and verification defects. Repairs added durable
initialization intent, bound filesystem access, validated stored semantics,
rechecked credential/administrator authority, retained publication ownership
through cancellation, and bounded browser body/JSON admission. Strict MG result
parsing and scope handling have thirteen synthetic wrapper checks.

A broader operational test-author request was refused at
`/private/tmp/meguri-sync-U4/build/mg/cleanroom/20261115T142854Z-77427/result.json`.
It yielded no accepted patch. Work continued only on defensive fixes and bounded
functional fixtures. Do not claim pre-fix operational reproduction from that run.

The final source review identified parent-directory durability, earliest partial
initialization repair and staging-budget defects, plus four finite proof gaps.
The final repair adds twelve tests, retaining all original 83 test attributes.
A separate typed row decoder preserves every stored-value and authority check.

The final combined independent review is clean:
`/private/tmp/meguri-sync-U4/build/mg/cleanroom/20261115T165639Z-99905/codex.out`.
It checked the nine-file known-fix delta and all five simplification rubrics;
no actionable correctness or worthwhile simplification finding remained.
All tests and proof helpers were protected; cosmetic churn was skipped.
The reviewer verified source hashes but could not access external runtime receipts.
The integrator separately inspected those receipts and ran the canonical tests.

## Frozen source and checks

All 39 owned files match source SHA-256
`5b1a1a8aff98f95f768280383fb945708781e7cac05b0c179e1e20495a34bf8d`.
The algorithm hashes sorted path bytes, NUL, file SHA-256 hex, NUL.
Manifest: `/private/tmp/meguri-sync-U4-source12.json`.

- Canonical tests: `build/mg/sync-server-test/20261115T165926Z-1744/result.json`,
  95 passed, zero skipped.
- Isolated checkout tests:
  `/private/tmp/meguri-sync-U4-verify/build/mg/sync-server-test/20261115T165338Z-93642/result.json`,
  95 passed, zero skipped; lint at `sync-server-lint/20261115T165503Z-95121/result.json` passed.
- Static checks: `build/mg/static-checks/20261115T165608Z-96887/result.json`, three passed.
- Similarity: `build/mg/similarity/20261115T165613Z-98940/result.json`, all 27 Rust
  files and the SQL schema passed; zero flags or allowlist entries.
  The earlier failed receipt at `20261115T155956Z-59034` remains retained.
- Dependency purposes, narrow features, locked 133-package graph, maintained
  password-hashing compatibility and scoped license/advisory checks are recorded
  in the frozen `server/DEPENDENCIES.md`; no dependency changed in the final repairs.
- Shared tier integration reuses the independently reviewed two-file export,
  with real registry/dry-run evidence at
  `/private/tmp/meguri-sync-gate-wiring-verify/build/mg/selftest/20261115T151824Z-32059/result.json`.

## Privacy admission and limits

The candidate privacy scan failed on sixteen files at
`build/mg/private/20261115T165608Z-96886/result.json`.
An independent complete-content/hash review found no obvious private material:
`/private/tmp/meguri-sync-U4-privacy-review.md`.
Under delegated maintenance authority, exact path/full-byte entries admit those
reviewed contents. Eight entries cover production Rust/SQL; the others cover
synthetic tests and technical documentation. Edited bytes lose admission.
Scanner logic, index-byte checks and synthetic mutation tests remain unchanged.
The final staged privacy receipt is recorded in the source commit message.

Process-interruption and injected-boundary tests do not establish power-loss
behavior. The temporary budget measures active staging payload, not all retained
disk usage or filesystem overhead. These substrate checks establish no Desktop,
Meguri or Mobile interoperability, compiled web interface, backup or release
acceptance. Those remain later units and the required integration gates.

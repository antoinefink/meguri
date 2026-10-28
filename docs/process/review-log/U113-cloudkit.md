# U113 — CloudKit implementation in progress

Status: incomplete; no Unit trailer or acceptance.

Starting commit, recorded before authoring:
`1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Original pending integration base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
The starting record is `build/unit-intake/U113-cloudkit/start.json`.
Prepared-work audit found spec/parser work, with no existing CloudKit provider
or tracking implementation to import. The approved R2066 contract is in force.

## Store foundation

A separate versioned `meguri-cloud.db` stores typed build/binding facts and
classified inbox entries with opaque SDK state. Server facts commit atomically
and durably. Reads and mutations are scoped by container/environment; SQLite
enforces one active binding per build. Stored binding selection makes no
network-readiness claim. No collection SQL, network, FFI, UI or provider wiring
is added. The manual-transfer implementation remains unchanged and frozen.

The private WAL-aware probe preserves damaged/newer originals, including a
newer schema present only in WAL. Missing tracking is empty and unbound; no
acknowledgement is inferred. Damaged-file salvage remains the next dependency.
The caller must serialize probing/opening against all tracking writes and
checkpoints; production composition still has to establish that ownership.

Filtered implementation: `cleanroom/20261028T000217Z-23700`.
Foundation review 1 found P1 `CLOUD-TRACKING-APPLE-DURABILITY`: FULL alone did
not enable Apple's device-cache flush. The filtered repair
`cleanroom/20261028T001526Z-28363` enables fullfsync and checkpoint_fullfsync
before writes. Two private unit tests check the actual store connection after
creation and reopening. Foundation review 2 is clear. No pre-change runtime
failure is claimed; the issue was established by source and the C9 contract.

The disjoint native-proof export, `cleanroom/20261028T001839Z-30753`, extends
the existing native durability job with the two CloudKit integration targets
and only the cloud namespace of the library target. It preserves the original
three suites, isolated cache, build-before-lease order, serial measurement and
strict executable/result checks. New fixtures reject wrong/missing/ambiguous
artifacts, incomplete Cargo streams, empty results and nonzero exits. Its
independent source review is clear. These are component reviews; the complete
U113 acceptance review remains outstanding.

## Focused receipts

- `core-format/20261028T002312Z-33247`: pass.
- `core-test/20261028T002432Z-34042`: 41 passed, zero skipped. Includes the
  twelve new store/connection tests and existing library regressions.
- `selftest/20261028T002749Z-35451`: all twelve native-runner checks passed.
- `native-durability/20261028T003010Z-36411`: 37 passed, zero skipped:
  the existing 25 plus four cloud-record, six cloud-recovery and two actual
  Apple connection-setting tests. No retry or limit change.
- `core-lint/20261028T005127Z-46907`: pass on the final source.
- `similarity/20261028T003700Z-39920`: pass for the new supported source;
  `similarity/20261028T005127Z-46955`: both final changed files scanned, clear.
- `milestone-gate/20261028T003700Z-39651 --check`: pass.
- `static-checks/20261028T005126Z-46691`: pass.
- `private/20261028T004949Z-45942`: pass, 14 files.

The earlier private scan `20261028T003700Z-40291` failed on generic diagnostic
labels and a comment. Filtered repair `cleanroom/20261028T004025Z-41992`
replaced four already value-free Debug labels with type names and removed the
comment. Independent delta review is clear: no API, serialization, SQL,
durability setting or test operation changed. The native and focused runtime
receipts above retain their coverage. No privacy exception or guard change.

## Required continuation

Complete V8.1 salvage and its crash boundaries. Before account-dependent
tracking/reconciliation, add the exact P24/Q11 dropped-mark, crash, offline
identity failure, clean-close and next-online-scan regression. No P24/Q11 item
is closed by this store primitive. Semantic record handling, instance/account
guards, local capture, the dedicated reader, WAL coverage, J6 media facts,
merge/rekey/hold-back, structural generations, recovery, CloudAdopt and policy
remain required, followed by U114 and shared U117/U118 evidence.

Keep the unfinished UI units, retained frozen/signed limitations, U127
escalation and incomplete integration gates in
[the maintenance integration log](maintenance-integration-2026-10-27.md).
The independent store slice does not accept those units or any release gate.

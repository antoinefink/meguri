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

## J6 media feed dependency — work in progress

Starting commit: `3c0b5114c084e2456ed5a39d9596f6691e54813b`;
recorded before work in `build/unit-intake/U113-cloudkit/media-feed-start.json`.
This is additive U31 work for U113, with no new Unit acceptance.

The journal adds version 3 file-fact snapshots, a persistent feed identity,
registration completeness, removal causes and floor-based retention. Separate
facts preserve unsyncable files without changing manual Awaiting rows. Normal
transfer bookkeeping remains unchanged. U113 remains incomplete.

Filtered author `20261028T005705Z-49821`; repair
`20261028T011628Z-58337`. Independent review 1 found a move-before-observation
crash gap, direct OS randomness and missing facts for unsyncable Awaiting files.
The repair adds a durable origin receipt, verifies the moved file before replay,
retains the injected RandomSource and stores independent physical facts.
Review 2 closed those findings; it found a test build error and a child status
check to fix. Repair: `20261028T013151Z-66458`. Review 3: clear.
Wording repairs used no guard change; `20261028T013728Z-70497`
changes only one test identifier. No runtime operation or assertion changed.

Receipts:

- `core-test/20261028T012647Z-64056`: retained test compile failure.
- `core-test/20261028T013516Z-69418`: 130 passed, zero skipped, including
  media storage, import, staging, reset and actual process-death recovery.
- `similarity/20261028T013908Z-71834`: 33 supported source files clear;
  includes this core slice and independent scanner infrastructure.
- `private/20261028T013910Z-72261`: media source clear; separate scanner
  fixtures still need wording repairs.
- `core-lint/20261028T013909Z-72070`: rustfmt, layering and host scan pass;
  two collapsible-if diagnostics in receipt enumeration remain to fix.

This WIP is not accepted. Resolve lint, retain final checks and finish the
CloudKit dependencies above. The original pending integration base is unchanged.

Lint follow-up: filtered `20261028T014817Z-78054` and
`20261028T015143Z-81194` keep the same guard order and boolean test condition.
Both small changes are independently clear. `core-lint/20261028T015244Z-82045`
passes all four checks. `similarity/20261028T015041Z-80553` clears the final
production file. No runtime operation changed; the 130-test receipt is retained.
The complete U113, integration and release checks remain open.

## V8 tracking recovery dependency

Starting commit: `bac351fd`; recorded before work in
`build/unit-intake/U113-cloudkit/salvage-start.json`.
The reviewed spec correction is `e8ef12d0`.

Filtered author `20261028T015847Z-84709` adds version 2 migration, conflict
storage and explicit recovery. A durable move journal precedes either file
move. Recovery checks main/WAL identity, repeats sync barriers, commits copied
rows before source finalization and keeps unreadable sources. Copies retain
their ids and known old identity fields; separate salvage owners never resume.
No acknowledgement, engine cursor or pending work is copied.

Source review 1 is clear. The first focused run,
`core-test/20261028T022018Z-93770`, passed 65 of 66 cases. Its corruption fixture
used a 17-byte id for a 16-byte key and changed zero rows. Filtered repair
`20261028T022543Z-96657` uses a bound id, checks one affected row and reads back
the bad content. The expected recovery and retention checks remain unchanged.
Review 2 is clear. A flagged comment was removed; no guard exception.

- `core-test/20261028T022815Z-98533`: 66 passed, zero skipped. Includes actual
  SIGABRT at all five V8 boundaries with a real WAL.
- `private/20261028T022817Z-98793`: pass.
- `core-lint/20261028T022905Z-99626`: pass.
- `similarity/20261028T022903Z-99379`: pass.

Native verification is shared with the pending WAL-reader slice. Exhaustive
F3 sync-failure/power-loss runs and U117/U250 evidence remain open; file-layout
simulations do not supply that proof. Production exclusive ownership across
all builds remains unwired. Conflict-list and bulk-recovery memory checks also
remain part of full integration. No Unit or release acceptance is claimed.

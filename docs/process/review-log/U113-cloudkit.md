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

## Cloud reader and WAL coverage dependency

Starting commit: `bac351fd`; recorded before work in
`build/unit-intake/U113-cloudkit/wal-reader-start.json`.

The Cloud reader is lazy, independent and query-only. Snapshots carry native
file identity and collection generation. A bounded off-writer wait returns
coverage only after actual WAL sync or a successful close checkpoint. It
neither starts sync nor calls providers under engine locks. Reopen and replace
cannot transfer an old receipt. The owned snapshot handoff remains unfinished.

Filtered author `20261028T020809Z-88840`. Source review 1 was clear.
`core-test/20261028T023645Z-3573` stopped at a private test helper call.
Filtered repair `20261028T024018Z-5363` uses the public error conversion.
`core-test/20261028T024430Z-7573`: 34 passed, two failed.

The close fixture held an external reader but expected close success. The
corrected fixture requires the exact checkpoint refusal, wakes the waiter as
unavailable, releases that reader, then closes and reopens successfully.
The two-case diagnostic run `20261028T030046Z-14926` proved this repair and
retained the no-op failure. Temporary diagnostics were removed afterward.

P1 `WAL-NOOP-TOKEN-COVERAGE`: SQLite's own-commit counter changed after a
successful operation with no changed rows. Filtered repair
`20261028T031229Z-19193` rebases that counter only against healthy, clean prior
coverage with matching pre-commit counter, file identity and generation.
Prior dirty or unsequenced writes remain uncovered. Two deterministic tests
pause automatic flushes through a collection-scoped test hook; explicit sync
remains real. Both verify those prior writes cannot be covered by a no-op.
Independent review 3 is clear.

Final mechanical repairs remove a flagged generic test comment and move
`validate_identity` verbatim before its test module
(`cleanroom/20261028T033329Z-30387`). Independent delta review is clear.
No assertions, limits, privacy exceptions or runtime operations changed.

- `core-test/20261028T033139Z-28457`: 38 passed, zero skipped.
- `core-lint/20261028T033222Z-29376`: retained declaration-order failure.
- `core-lint/20261028T033442Z-31854`: all four checks pass.
- `similarity/20261028T033139Z-28604`: twelve supported files clear;
  `20261028T033442Z-31646`: both final files clear.
- `private/20261028T033139Z-28274`: retained comment finding;
  `20261028T033443Z-32053`: final two files clear; the other eleven were
  clear in the earlier scan.

`native-durability/20261028T033543Z-32800`: 76 passed, zero skipped.
This single native run covers the WAL slice and V8 recovery above, including
actual Apple connection settings and process-death boundaries. It does not
supply exhaustive power-loss evidence or accept U113, P24/Q11, integration
gates or the release.

## Pure entity wire codecs

Starting commit: `f7a83f2b`; recorded before work in
`build/unit-intake/U113-cloudkit/entity-codec-start.json`.
The reviewed format clarification is `d2eef6ac`.
Filtered author: `20261028T033809Z-35437`.

The codec covers K4 entity naming, the format-1 Entity envelope, all nine
K6.1 payload messages, canonical bytes, storage-class presence, digests and
inline/asset classification. Inputs include the caller's canonical key and
computed history/identity fields. No row reads, tracking policy, provider,
asset I/O, account decisions or P24 behavior are added.

Review 1: P1 `CODEC-DEFERRED-ORD-VALIDATION`. Filtered repair
`20261028T035116Z-44417` checks `ord` before `push`. Four tests cover
`FieldValue::read_field`. Independent review 2 is clear.

The first focused build (`20261028T034824Z-42028`) stopped at an ambiguous
empty-vector assertion. The next run (`20261028T035322Z-45953`) passed 20 of
21 selected tests, including those four parser checks; 42 unrelated library
tests were unselected. A malformed fixture's one-byte format prefix consumed
the next field tag as its value, producing supported decoding of an
unsupported format rather than truncation. Repair `20261028T035617Z-48060`
uses the standalone truncated byte and preserves every other refusal and
newer-format check. Its pair-iteration lint edit is equivalent. Independent
review confirms both changes.

- `core-test/20261028T035759Z-49411`: 29 passed, zero skipped: all seventeen
  codec integration tests plus twelve separate fake-service foundation tests.
  The four parser unit passes above remain valid; production code did not
  change after that run.
- `core-deny/20261028T034825Z-42245`: all four dependency checks pass.
- `private/20261028T035323Z-46197`: all ten codec inputs clear; the final
  test file is also clear in `20261028T035800Z-49640`, whose failures are
  confined to the disjoint fake-service files.
- `similarity/20261028T035800Z-49642`: eleven supported files clear,
  including the final codec and the separate fake foundation.

The lockfile adds HMAC and its required transitive packages; no existing
version changes. The initial new-package update could not select an absent
lock entry (`20261028T034731Z-40761`). Resolving with the existing base32
package fixed at its current version succeeded (`20261028T034802Z-41397`).
No broad dependency update or guard exception.

`receive` is wire/integrity validation. Payload-to-envelope identity, model
row readability and live/tombstone rules still require semantic validation;
`Received` is not permission to apply a row. Other control envelopes, the
actual image builder, P24, shared fake coverage and complete U113 remain open.

`core-lint/20261028T035900Z-50556` passes formatting, layering and the host
scan. Its only two Clippy findings are documentation indentation in the
separate fake service; the codec has no lint finding. That repair and the
shared tree's final lint remain pending. No full integration gate is claimed.

Standalone codec closure on clean `a71284f7`:
`core-lint/20261028T040241Z-52902` passes all four checks;
`static-checks/20261028T040240Z-52668` passes. The unfinished fake source was
preserved outside the checkout for its filtered repair during these checks.
No codec source changed. U113 and the original integration gate remain open.

## Same-commit generic row access

Starting commit: `2c3c32e2`; recorded before work in
`build/unit-intake/U113-cloudkit/commit-row-access/start.json`.
Filtered author `20261028T040800Z-55382` adds image-free key enumeration,
keyed row access and direct lazy delta iteration to the owned journal handle.

Review 1 found P1 `NOTE-ROW-DIGEST-ACCESS`: asking the journal for a full note
after-image returned Mode and hid both its before-image and digest. Filtered
repair `20261028T041723Z-60473` exposes `RowAfter::{Absent,Row,NoteDigest}`.
Notes keep the existing journal digest and changed columns; other rows keep
fallible image access. It stores no new note image and makes no CloudKit hash
claim. Independent review 2 is clear.

`core-test/20261028T042143Z-65248`: nine passed, one fixture failed. Its alias
lookup expected the changed tag spelling as the journal key. The journal
retains the original key; independent review confirmed the test correction.
The first small fixer (`20261028T042359Z-67134`) did not start: main changed
during source preparation. No code or test ran in that attempt. The combined
filtered repair `20261028T042650Z-69659` corrects only that expectation and
removes the flagged generic test comment. Independent equivalence review is
clear. Source preparation is now kept stable through the boundary probes.

- `core-test/20261028T042812Z-71131`: ten observer tests and seventeen
  separate fake-service tests passed; zero skipped. Includes real note
  add/edit/delete, raw before-images/digests, composite keys, aliases,
  failures and retained reads after later commits and close.
- `private/20261028T042813Z-71353`: all nine inputs clear.
- `similarity/20261028T042813Z-71357`: supported source clear.
- `core-lint/20261028T042253Z-66417`: all four checks pass for the accessor.
  The final shared run `20261028T042914Z-72246` has only three fake-service
  lint findings; formatting, layering and host scan pass.
- `static-checks/20261028T042915Z-72485`: pass.

Writer fallback for a note's CloudKit digest, the tracking observer/failure
hook, CloudIncoming/Rekey, structure reports and the 150k consumer allocation
proof remain open. This accessor does not accept full S2.4 or U113.

The shared final `core-lint/20261028T043423Z-75307` passes all four checks with
the committed row accessor unchanged. Its focused and guard receipts above
remain valid. No full U113 or integration acceptance is claimed.

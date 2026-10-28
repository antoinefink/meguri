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

## P24 account and root regressions — enabled, failing build

Starting commit: `5755bbc0`; recorded before test authoring in
`build/unit-intake/U113-cloudkit/account-workflow/start.json`.
No production account/capture/reconcile/send implementation preceded these
regression definitions. The approved focused-fixture interpretation is in
`build/cloudkit-intake/account-test-first-packet.json`: arrange prior bound
state only; the edit, lifecycle, scan, guarded builder and Service SAVE must
be real. Full Q4/Q5 and signed requirements remain unchanged.

Filtered test author `20261028T044616Z-80826` adds three enabled tests in
`cloud_recovery.rs`: lost capture through crash/offline clean close; edits
before a different identity; finite current-instance root-confirmation cases.
Review 1 required the loss to precede editing and cover the whole capture
transaction, separate counter domains, keep durable drained false while open,
use the fixed fake-clock origin and require an exact revision increment.
Repair `20261028T050226Z-88228` implements those corrections.
Review 2 found a row-versus-payload digest assertion; repair
`20261028T050947Z-92666` separates the hashes and requires a nonzero baseline
layout generation. Review 3 is clear. Final equivalent explicit Save
assertions (`20261028T051342Z-95682`) preserve the conditional tag; independent
review 4 is clear. Existing recovery tests are unchanged.

- `core-test/20261028T051531Z-97100`: **failed build**, only the missing
  `meguri_testkit::cloud::{devices,coordinator}` interfaces. Zero runtime tests
  ran. This is the recorded test-first failing state, not a pass or skip.
- `private/20261028T051533Z-97360`: pass.
- `similarity/20261028T051533Z-97399`: pass.

Earlier build failures `20261028T050726Z-91208` and
`20261028T051210Z-94257` also included a concrete mutation-reference error,
now removed. The initial generic fixture/comment privacy findings
`20261028T045958Z-86778` were rewritten without a guard exception.

Next: implement the supported production composition, early-open evidence,
observer capture, lifecycle, reconciliation, account/root guards, conditional
send and shared process harness. Keep these tests enabled. P24 items, U113,
U117, native/full integration gates and release acceptance stay open. The
full native durability target now also awaits the missing test interfaces;
its earlier receipt is not a current whole-target pass.

## Reconciliation read primitives

Starting commit: `16f12a9b`; recorded before work in
`account-workflow/storage-start.json` under the unit intake directory.
Filtered author `20261028T052145Z-772` adds a checked stored-row cursor,
raw notetype part pages and the unfiltered maximum revlog id for one card.
The cursor preserves raw text keys, the part pages follow signed ordinals
without assuming contiguous values, and reads retain their snapshot. The
existing generic pager and manual-transfer behavior are unchanged.

Independent review 1 is clear, including clean-room conformance.
`core-test/20261028T052704Z-3919`: four passed, zero skipped.
`private/20261028T052705Z-4156` and `similarity/20261028T052704Z-3693` pass.
Whole-target lint remains blocked by the separately recorded missing account
harness; it is not reported as passing for this commit.

The page limit is 250 rows, not a byte-budget or 150k allocation proof.
The revlog helper describes live snapshot rows; a held image must compute
its own image-aware history. Actual reconciliation, sender bounds, P24 and
U113 acceptance remain open.

## Early-open evidence and observer read/failure ports

Starting commit: `16f12a9b`; recorded before authoring in
`account-workflow/engine-start.json` under the unit intake directory.
Filtered author `20261028T052143Z-374` exposes immutable pre-write opening
facts and a borrowed writer-row read capability for commit observers.
Initial open, resume, successful replacement and failed-swap reopening
refresh the facts. Observer errors and panics notify that observer's failure
hook without undoing the committed operation or preventing other delivery.
The ordinary row journal and note digest representation are unchanged.

Independent review 1 found live recovery-copy reads in two test fixtures.
Repair `20261028T054016Z-10328` uses owned Query snapshots instead and fixes
the test RowError conversion. Review 2 is clear. The first runtime pass
then found three timestamp fixture failures: incoming operations preserve
the stamp the assertions expected to change. Repair
`20261028T054813Z-15301` uses actual local non-undoable writes, retaining
all assertions and clock advances. Independent review 3 is clear.

- `core-test/20261028T053319Z-7505`: failed build, test error conversion.
- `core-test/20261028T054721Z-14500`: 18 passed, three fixture failures.
- `core-test/20261028T054952Z-17090`: all 21 passed, zero skipped.
- `private/20261028T053317Z-7074`: all 12 initial inputs clear; final
  fixture `private/20261028T055005Z-17500` is clear.
- `similarity/20261028T053319Z-7328`: combined initial source clear; final
  fixture `similarity/20261028T055016Z-17809` is clear.
- `core-format/20261028T055025Z-18163`: pass.

The two native identifiers omit the creation instant that V8.2 also requires. The scoped reader capability cannot be retained in owned commit data;
its compile-fail documentation example has not been executed separately.
Account capture, scan, conditional sending, the enabled test-first harness,
full S2.4 and 150k allocation checks remain open. Whole-target lint and native
checks still await the intentionally missing account harness. No U113,
batch gate or release acceptance is claimed.

## Existing-Note capture and reconciliation component

Starting commit: `46f39d6d`; recorded before work in
`account-workflow/runtime-start.json` under the unit intake directory.
The enabled account/root regressions at `16f12a9b` preceded this work.
Filtered author `cleanroom/20261028T055446Z-20632` adds an optional
production runtime through normal CoreHost composition. It captures
existing acknowledged Notes before live identity verification, isolates
changed accounts without deleting pending facts and reconciles those
Notes from a held CloudReader snapshot with a matching WAL receipt.
The runtime extension has its own version inside outer tracking V2.
All four runtime ports register before engine open; default configuration
keeps the component disabled.

Independent review 1 required streaming capture instead of retaining all
changed bodies, reading scan pages outside writer-shared locks, scoping
ordinary payload validation to this build and fixing account-reason
precedence. Repair `cleanroom/20261028T061413Z-31932` closes those four
findings and adds ten tests using real CoreHost operations. Tracking
capture is atomic; newer observer captures survive scan page merges.
Review 2 found test barriers that could hang on an early failure.
Repair `cleanroom/20261028T062858Z-52822` bounds reached/completion waits
and releases paused workers before scoped joins on error. An unreached
barrier cannot certify completion. Review 3 is clear, including the
unchanged production behavior and clean-room scope.

- `core-test/20261028T061054Z-28117`: initial component compiled; three
  existing host composition checks passed, one existing milestone-owner
  check remained ignored. This did not execute the new runtime.
- `core-test/20261028T064020Z-64392`: all ten new host runtime tests and
  60 separate format checks passed, zero skipped. Feature:
  `meguri-host/test-hooks`, target `cloud_runtime`.
- Tests cover disabled/enabled composition, capture with identity
  unavailable, rollback of a failed tracking capture while retaining the
  collection commit, concurrent edits during paused reconciliation, stale
  account/incarnation guards, build scope, isolation precedence and
  lifecycle refusal of drained/sending status.
- `private/20261028T061124Z-28699`: new comments and identifier spellings
  flagged; rephrased/renamed without a guard exception. Final
  `private/20261028T064024Z-64694` passes for all ten inputs.
- `similarity/20261028T061126Z-28962`: initial source clear. Final
  `similarity/20261028T064024Z-64699`: pass.
- `core-format/20261028T064000Z-63930`: pass.

This is a component, not whole-binding reconciliation. New/deleted Notes,
other entity kinds, structural layouts, intent replay, root confirmation,
transport, conditional sending/acknowledgements and the shared process
harness remain open. Scan-required stays set; whole-binding drained and
sending remain disabled, and the Sync service is pending. Pages are
limited by row count; the full byte-budget/150k allocation proof remains
open. The three enabled end-to-end account/root regressions still await
the missing testkit helpers. Whole-target lint/native durability and
full U113/U117 acceptance remain open. The integration base is unchanged.

## Saved/action ordering and real review contracts — test first

Starting commit: `712d84f3`; recorded before actual authoring in
`review-workflow/start.json` under the unit intake directory. The earlier
`cleanroom/20261028T065914Z-78171` admission refused dirty K7 preparation;
no author ran in that attempt.

Filtered test author `cleanroom/20261028T070611Z-84346` adds six enabled
Saved/pending-action contracts and one real review round-trip contract.
They cover a local revision 3 in flight followed by revision 4; held edits
waiting for SessionEnded; deferred additions/removals and blob deletion
ordering; automatic sync off with 400 selected records, the first 250
failing limitExceeded, completion within eight calls; and complete action
emission order. The separate review test requires actual CoreHost
start/show-answer/Good/end-and-drain, canonical Card/Revlog service saves
and local persistence after reopening. Millisecond review identity is
compared without dividing it by 1,000.

Q2/Q11 permit Rust coverage of U114's core side first. The stand-in must
remove Saved names at handler return and defer the whole UpdatePending,
removals included. Actual CoreHost, production actions and Service results
are required; helper-inserted outcomes are not evidence. Swift fake-engine,
real SDK and signed A23 proof remain separate requirements. The P24 item
is not removed by this partial test phase.

Independent review 1 is clear. Mechanical repair
`cleanroom/20261028T073018Z-8442` renames new helper methods to
`drive_ready`/`drive_until_response`, rephrases two comments and makes
owned name collection explicit. Independent equivalence review 2 is clear;
all seven tests and assertions remain enabled. No production/helper code
was added before these tests.

- `core-test/20261028T072216Z-5243`: failed build on missing shared
  adapter/device APIs, with associated inference diagnostics. Zero tests ran.
- `private/20261028T072217Z-5693`: new helper spellings/comments flagged;
  repaired without exceptions.
- `similarity/20261028T072216Z-5437`: initial source clear.
- `core-test/20261028T085409Z-34670`: still failed build, missing
  `cloud::{adapter,devices}` and cascading unsized-name diagnostics despite
  the explicit owned collection type. Zero runtime tests ran; this is the
  enabled test-first state, not a pass or skip.
- `private/20261028T085410Z-34894` and
  `similarity/20261028T085411Z-35042`: final source passes.
- `core-format/20261028T085332Z-33958`: pass.

Next implement one coherent stable-layout review workflow: common entity
capture/reconciliation, current-instance root guard, conditional save/ack
and pending actions, shared SDK stand-in and actual CoreHost child driver.
It must pass the three earlier account/root tests and these seven tests.
No U113/U114/U117, full integration or release acceptance is claimed.


## Stable prebound workflow — source handoff, not accepted

Starting main commit: `d96d9a71`; recorded in `review-workflow/production-start.json`.
Implementer source pass for the ten enabled account/root, adapter and real-review
contracts already defined test-first. No forbidden source, history or network
was read. No build, formatter, test, review or gate ran in this pass.

Production source now includes raw `row_payload` projections, additive generic
entity tracking with an authoritative Note adapter, whole-kind/parts/empty-J6
coverage, current-instance root proof persistence, conditional batches, Saved
acknowledgements, ordered pending lists and bounded Sync Now exchanges. Real
ReviewService start hooks close sending; actual end/drain precedes the full
pending update. Lifecycle suspension, after admission closes, records durable
drain; ordinary background flush does not. File guards include creation facts.

The shared testkit exports `cloud::{devices,adapter,coordinator,child_driver}`.
`PreboundDevice::arrange` captures raw canonical baselines and a matching real
WAL receipt, acknowledges them through Service and seeds only prior healthy
tracking. SDKRig and Coordinator use one current-executable CoreHost child via
bounded Unix IPC; the parent owns the persistent Service. Three guarded
`cloud_child_entry` functions are infrastructure additions; existing product
assertions, limits and enablement remain unchanged. SIGKILL uses Child::kill,
checks signal 9 and reaps. No cargo-built child executable is assumed.

Source consistency corrections included checked signed SQLite boundaries,
raw REAL bit persistence, snapshot dereference types, RowError conversion
through StorageError, a guard before child reaping, preservation of deferred
removals, completion of send calls after handler/actions return and keeping
snapshot projection/decoding outside shared writer guards. This is source
inspection only, with no compilation or passing-test claim.

Remaining: integrator formatting/compilation, the ten requested contracts and
all ten existing host runtime tests, independent review/fixes and every
provenance/privacy/similarity/native/full gate. Production support is limited
to the stable prebound subset: no publication/adoption/manual-transfer change,
K19 structure claim/join/rollback, incoming merge/re-key/absence purge,
nonempty media/assets/retries/cleanup/reset, new non-Revlog entities or
acknowledged deletions. Concurrent boundary changes beyond the captured
snapshot are refused. Swift fake-engine, real SDK, signed A23, Q4/Q5, 150k
allocation/byte-budget evidence and all remaining plan clauses stay open.
U113/U114/U117, P24/Q11, milestone and final acceptance remain unaccepted.

Integrator WIP evidence: `core-format/20261028T094612Z-65075` passes.
`core-test/20261028T094714Z-65746` fails before runtime on two shared-helper
imports: ReaderRole is in storage, and StatvfsSpace is in storage::space.
Production sync/host libraries compile; no workflow tests ran.
`private/20261028T094808Z-66316` and
`similarity/20261028T094810Z-66586` pass for the 26-file export.
Independent whole-workflow review is in progress. This commit preserves
reviewable WIP, not a completed unit or accepted workflow.

## Stable workflow R1 repairs — verification in progress

Starting main commit: `540ae56e`. Independent review found three P1 issues:
stale callbacks could mutate current tracking, a scan could clear a newer
capture failure, and Card history missed raw untracked or negative IDs. It
also found P2 collection-wide body decoding inside ordinary writer callbacks.
The shared device fixture placed backups inside the collection directory;
all ten new workflow tests failed that valid production overlap guard before
reaching their workflow assertions (`core-test/20261028T095807Z-70968`,
32 pass and 10 fail).

Filtered fixer `cleanroom/20261028T100440Z-73238` exports instance-bound
mutation APIs, scan invalidation guards, the borrowed writer history query,
indexed dirty-name discovery and owned sibling backup fixtures. Five new
behavioral regressions and one engine observer regression remain enabled.
The earlier account, adapter, review and host assertions are preserved.
Independent review 2 is in progress; these repairs are not yet accepted.

- `core-format/20261028T102338Z-80150`: pass.
- `private/20261028T102416Z-81033`: pass for the ten source paths.
- `similarity/20261028T102418Z-81283`: pass for those paths.
- `core-test/20261028T102400Z-80713`: build fails because the new test
  observer omits the required `observe` method. No runtime pass is claimed.

The source is retained as WIP. A narrow fixture compilation repair follows;
all workflow checks and review findings remain open until verified.

R2 is clear of semantic P0–P2 findings in the stable prebound scope. The
mechanical repair `cleanroom/20261028T102509Z-81980` adds the test observer's
required fail-loud fallback and removes a synthetic source-base comment.
Independent delta review is clear; assertions and production behavior are
unchanged.

- `core-test/20261028T102517Z-82336`: all 48 selected Cloud tests pass,
  zero skipped, including the actual review/save/reopen contract and all
  enabled R1 safety regressions. Guarded child entry tests are included in
  this count; it is not 48 independent product workflows.
- `core-test/20261028T102618Z-83002`: all 13 engine observer tests pass.
- `private/20261028T102638Z-83699` and
  `similarity/20261028T102641Z-83946`: final two-file delta passes.
- `core-lint/20261028T102621Z-83263`: fails ten Clippy style diagnostics;
  formatting, layering and documentation checks pass. A mechanical repair
  follows; no lint suppression or assertion change is authorized.

Full CloudKit SDK, media, signed-device and milestone verification remains
open. No Unit trailer or acceptance is added.

Filtered lint repair `cleanroom/20261028T102726Z-84562` resolves all ten
Cloud diagnostics using equivalent let-chains and `is_empty`. Independent
review confirms evaluation and mutation order are unchanged. The final
five-file delta passes `private/20261028T102900Z-85987` and
`similarity/20261028T102902Z-86222`.
`core-lint/20261028T102846Z-85593` gets past Sync and reports one remaining
let-chain diagnostic in the earlier scenario prerequisite work
(`meguri-testkit/src/scenario/model.rs`). That mechanical repair remains;
this is not a passing full-lint receipt.

### Verified stable-prebound checkpoint

All required semantic R1 fixes are verified in their scoped tests, and R2 is
clear. Mechanical deltas have separate clear equivalence reviews. Remaining
runner lint repairs are recorded with the oracle intake work; they preserve
all fixtures and assertions.

- `core-test/20261028T102517Z-82336`: 48 selected Cloud tests pass, including
  the review/save/reopen workflow, prior host contracts and new safety tests.
- `core-test/20261028T102618Z-83002`: 13 engine observer tests pass.
- `core-test/20261028T103318Z-90400`: 38 affected schema/runner tests pass.
- `core-format/20261028T103348Z-91076`: pass.
- `core-lint/20261028T103402Z-91405`: all four checks pass.

These are focused receipts, not a full gate or a live Apple SDK result.
The earlier full T0 receipt predates this workflow. Keep the original pending
integration base `1416a520542a8c5daa38a23bfe398da704c3d6c9` and all existing
integration failures. No Unit trailer, P24 item removal, milestone or release
acceptance follows from this checkpoint. Publication/adoption, incoming
reconciliation, K19 structure, nonempty media, Swift adapter and signed
cross-device verification remain open.

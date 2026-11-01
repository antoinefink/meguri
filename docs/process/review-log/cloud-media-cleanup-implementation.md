# Media cleanup source delivery

Base: `3f241c19b77db754b71c0d8c433f25fd674903ae`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Seed author: implementer `s-20261025T134357Z-235af9a2`.
Repair author: fixer `s-20261025T162405Z-9dc656e4`.
Authority: K9.5, P10.3, P24 and Q11. No contract changes.

This delivery retains the complete 48-path input and its existing assertions.
It is source for integration, with no acceptance result. The runner performs
no builds, test execution, network access or repository-history operations.
The integrator owns formatting, Rust/native compilation, generated bindings,
focused execution, independent review and the required gates.

The implementation records reference age from live local and acknowledged
versions. ACK and apply writes preserve the earlier materialized payload while
updating tracking. Cleanup computes within one IMMEDIATE transaction; committed
unlink evidence is replayed afterward. A9b remains disabled. Media deletion uses
the original engine batch, delivery admission and callback categories. Durable
Deleted state requires the original deletion result or accepted UnknownItem.
No own-operation delete or substitute pending/ACK result is provided.

R1 repairs:

1. Eligibility reads unresolved media copies across ordinary and recovered
   owners. Each copy uses its owner's stored record key; missing or unreadable
   authority withholds deletion. Foreign owners remain outside ordinary binding
   selection and writes. Resolved copies retain their existing expiry rules.
   The same eligibility function governs computation, batch selection and final
   queue admission.
2. The Driver retains the accepted drain receipt with the exact issued command.
   Runtime consumes that original SDK, backend guard and timestamp once. Clock,
   file and account observations happen before Runtime/Driver locks in this port.
   Delayed or backward observations cannot renew the one-hour window. Failure
   does not advance daily success, and inventory listing does not refresh marks.
   The direct publication helper also propagates clock errors and rejects stale
   receipts. Native RunCleanup remains explicitly unavailable until retained
   inventory integration is implemented in the next checkpoint.
3. Swift forwards the whole original sent receipt to Core before classifying
   continuation. A validated UnknownItem delete remains diagnostic and permits
   remaining media work. Save failures and other delete failures still stop it.
4. Swift computes the reported server delay from both original failure arrays.
   Per-record code, zone, name and delay remain available in its diagnostic.
   A subsequent normal absence callback cannot clear an earlier blocking error.

The existing testkit module import was corrected to use `meguri_sync::cloud`.
A second seed mismatch was repaired: Rust reads failed-record zone metadata,
so its FFI record and the Swift raw mapping now carry zone, native code and
retry delay. The existing Rust result literals were updated consistently.
The affected custom helper identifier and complete comment/prose regions were
rewritten. No privacy search, guard alteration or allowlist exception was used.
These edits do not establish a privacy or compilation pass.

New regression source:

- `cloud/media/cleanup.rs`: ordinary isolated, other-build and actual recovery
  owners; missing decoding authority; eligibility, due clearing and
  resolved-copy behavior. Copy inputs are typed storage fixtures, not UI proof.
- `meguri-ffi/src/cloud_media_tests.rs`: fixture callbacks forward test Service
  records to Core and obtain Driver-issued commands. The source calls
  `Driver::compute_qualified_local_cleanup` for delayed-command, invalidation
  and foreign-copy cases, then checks batch selection and queue admission.
  These assertions have not run here and establish no positive Driver or
  native deletion result.
- `meguri-sync/tests/cloud_media_cleanup.rs`: the source asserts that an
  unresolved copy in another build prevents deletion, then updates the stored
  copy and expects deletion through the test SDK. Its existing helpers issue
  Driver commands but do not call `Driver::compute_qualified_local_cleanup`;
  this source therefore does not establish positive coverage of that port.
- `CoreBridgeTests/CloudMediaTests.swift`: accepted-receipt continuation/error
  classification, deletion-only and mixed delays, real policy delay facts, and
  the raw metadata mapping, and an original fake SDK callback rejected by real Core when it reports a delete
  for a handed save. Successful native deletion continuation still requires an
  integrated positive callback proof; the Rust Service UnknownItem test alone
  does not supply that Swift evidence.

The owner/storage, Driver and Swift regression groups were written before their
corresponding behavior edits. The additional cross-owner Service and FFI stage
cases were added afterward. None has execution evidence in this runner.

Known results remain: the original tests-first Service deletion assertion failed
in core-test070033; the quota cases stopped earlier in drive_budget with
RuntimeIncompleteRootConfirmationAndSending. Root's
`core-test/20261101T073720Z-69112` stopped during compilation on the testkit import.
No result is replaced by the source corrections here. The enabled application
review/media proof remains red; full reviews and mediaCloudKit are unaccepted.
Required next work includes native retained inventory, positive Swift deletion
continuation, full application Service evidence and milestone/final gates.

| Check | Result |
| --- | --- |
| cleanroom/20261101T075956Z-76519 | Source exported |
| core-test/20261101T075807Z-75267 | Compile failed |
| core-format/20261101T080210Z-77185 | Pass |
| core-test/20261101T080235Z-77497 | 69 pass, 11 fail; FFI filter matched none |
| core-test/20261101T080537Z-79006 | 61 selected, 59 pass, 2 fail; actual FFI `sync::tests::media_port` |
| swift-test/20261101T080938Z-80368 | Native/bridge compiled; all 25 focused CloudMediaTests pass |
| private/20261101T080500Z-78633 | Source and notes pass after notes repair; source scan 080239 had only notes flagged |
| similarity/20261101T075815Z-75899 | Pass before final source edits |

The final source pass adds cleanup_drain: None to STOP commands and changes
test names and comments. Test assertions stay enabled. Native cleanup and the
full app sync tests remain open. No Unit trailer is added.

## Request action repair

Repair base: 6363dda2226652919d5705cbdf87fd7072c888a5.
Fixer session: s-20261025T162405Z-9dc656e4.
Source: cleanroom/20261101T081230Z-82608. Review round 3: clear of required fixes.

Save and Delete stay attached to the original media batch through admission
and results. The existing mismatch assertion passes. The removal test checks
the original SDK request and unchanged ACK. Its later checks stay enabled.

| Check | Result |
| --- | --- |
| core-format/20261101T082139Z-84939 | Pass |
| core-test/20261101T082233Z-85372 | 1 pass, 4 fail |
| similarity/20261101T082808Z-86749 | Pass |

The read-only output shows no drain receipt before or after cleanup. The gate
is open, media is not paused, and the old blob has an age. No cleanup ran.
The test harness must forward actual fetch callbacks. MediaReply and another
input still return InvalidData. Their causes remain open.

Native cleanup, Driver execution and the full app tests still need proof.
No unit is accepted. Lint and the full batch gate remain required.

## R4 source handoff

Session: s-20261025T162405Z-9dc656e4.
Repair base: ecb521a0f4c089b9e9b0dc894e79926ee3d8abf4.
Unit start: 3f241c19b77db754b71c0d8c433f25fd674903ae.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.

The seven-file seed is retained. SDKRig forwards its original fetch callbacks,
including held completion, to Core. Service I/O, SDK ownership and lifecycle
errors remain in place. Media request/reply stage diagnostics remain enabled
under test-hooks. The seed's source review R4 was CLEAR: no P0, P1 or new P2.
That result predates this repair.

Reported execution: 83 tests, 79 passed and 4 failed. The separate existing
review/media/incoming selection passed 56/56. Cleanup qualification actually
ran; the old mark and removal-age cases passed. Seven new FFI cleanup tests
passed. The four failures were the empty replacement, the zone retry fixture,
and two cases stopping in adoption before their GC assertions. These results
remain recorded; this source does not replace them with passing evidence.

This repair excludes unsyncable names from download candidates, checks retained
download admission and preserves those files when a shared blob is applied for
another name. The existing empty-replacement test keeps every prior assertion
and adds local-file checks. A focused FFI test covers candidate selection and
rejection of an already issued download before enqueue, with unchanged ACKs.

The zone fixture now executes its issued RunCleanup via begin_cleanup_pass.
Its retry test observes that original command and the resulting cleanup stamp.
A policy refresh reads the due-zone facts after that computation.
The retained-zone and retry assertions remain enabled. Driver policy, daily
conditions, drain qualification and GC eligibility are unchanged.

Adoption diagnostics add static task and stage labels. Page output contains
static record kinds and validation booleans. Original error returns are kept.
No adoption cause is asserted without execution. Labels: operation, owner,
claim, authority, active_validation, epoch_shape, epoch_validation, epoch_write,
media_root_shape, media_root_validation, media_root_write, media_root_epoch,
media_root_stage_create, media_root_space, page_validation,
page_record_validation, payload_decode, payload_validation, payload_read,
payload_space, payload_stage, received, received_validation, received_stage,
stage_row_write, page_record_space, page_space and page_stage_write.

The upload/queue condition is collapsed with its short circuit and body intact.
The flagged adapter comment is removed; this handoff replaces the appended
prose. No privacy guard or exception is changed.

The filtered source check completed. Root owns the pending MG checks and
integration evidence. Source review R5 found no P0-P1 issue and one P2: retiring
a blocked download must also reset its persisted Running state, so another
name can later fetch the same blob. The repair and two-name test remain open.
No unit is accepted.

R5 integration evidence on a915 with this source applied:
- core-format/20261101T090055Z-14744: PASS.
- core-test/20261101T090129Z-15337: 101/104 PASS. Empty replacement and the
  actual Driver cleanup/retry case now pass. Two cases stop at adoption's
  received_validation for a MediaName tombstone. The new FFI test tries to
  decode an empty ACK envelope; its candidate/admission checks ran first.
- core-lint/20261101T090133Z-15586: FAIL, collapsible_if at adapter.rs:491.
- similarity/20261101T090305Z-16348: PASS.
- private/20261101T090059Z-15002: source files passed; this log passed after
  rewrite in private/20261101T090203Z-15877.
- Independent R5 review: no P0-P1 finding. The Running transfer-state finding
  above remains open, together with the adoption and focused-check failures.

## R6 source handoff

Session: s-20261025T162405Z-9dc656e4.
Source: cleanroom/20261101T090450Z-17162, base 8a52b651.
Unit start: 3f241c19b77db754b71c0d8c433f25fd674903ae.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.

R6 adds validated MediaName tombstones to adoption and keeps their original
ACK envelope and system fields. Local files remain withheld after reopen.
MediaName has no collection row; tombstones cannot satisfy live references.
Other unsupported tombstone kinds remain refused.

Retiring an exact blocked download resets Running in the ledger transaction
before releasing its pending slot. A second name can then download that blob.
The first file and its ACK remain intact. The FFI test checks its actual target;
all ACK equality checks remain. The adapter lint branch is collapsed.

Focused tests cover Service tombstone adoption and reopen, matching and differing
local files, invalid records, original system fields, payload form and the
second-name download. The two original GC scenarios remain unchanged.
Source checks completed; MG execution and independent R6 review are pending.
No new pass, unit acceptance or gate acceptance is claimed.

R6 integration evidence on fa945 with this source applied:
- Independent R6 review: no P0-P1 finding or new P2 finding.
- core-format/20261101T091246Z-22090: PASS.
- core-test/20261101T091323Z-22902: 105/107 PASS. Both original fetched-name
  GC cases now pass. Service tombstone adoption preserves matching and differing
  files through reopen. Original-field and payload-form validation also passes.
  Two new tests fail during setup: the FFI case finds no target ACK, and the
  invalid-record case receives a failed Service mutation before adoption.
  Their remaining assertions have not run and remain required.
- core-lint/20261101T091328Z-23150: FAIL, type_complexity in the separate
  manual-media policy test at cloud_policy.rs:379. Adapter lint is clear.
- similarity/20261101T091346Z-23492: PASS.
- private/20261101T091250Z-22338: source files passed. The log passed after
  rewrite in private/20261101T091318Z-22658.
Focused setup repairs, the shared lint issue and native reopen remain open.
The batch gate and unit acceptance remain pending.

## R6 verification setup repair

Session: s-20261025T162405Z-9dc656e4.
Repair base: 59e385ca37f95d67edd23bc59ea4384832745bd7.
Unit start: 3f241c19b77db754b71c0d8c433f25fd674903ae.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.

The FFI test starts from Service publication with media off. External file
removal is registered before the policy enables media. The original target ACK,
download checks, queue rejection and second-file checks remain required.

The invalid-record fixture uses the original ACK's system fields when replacing
that same Service record. Changed record names still use creation. All nine
malformed cases, Service mutation checks, adoption refusal and prior collection
and file preservation checks remain. The codec can encode these variants, so no
codec bypass is needed. The policy test uses a local type alias; its eighteen
gate cases and loop assertions are unchanged.

Only three test files and this log changed. Production behavior, Service checks,
assertions, retry limits and budgets are unchanged. Independent R6 source review
was clear of P0-P1 and new P2 findings before this setup repair. Native reopen
diagnostics remain separately owned.

Existing receipts remain core-test/20261101T091323Z-22902 (105/107 pass),
core-lint/20261101T091328Z-23150 (type complexity failure),
similarity/20261101T091346Z-23492 (pass) and
private/20261101T091507Z-24077 (pass). One source consistency check completed in
the filtered fixer session. Root owns MG execution and fresh receipts. No new
test pass, unit acceptance or batch gate acceptance is claimed.

Verification setup integration:
- Independent review: no P0-P2 finding.
- core-format/20261101T092053Z-26156: PASS.
- core-test/20261101T092132Z-26723: 21/22 PASS. All nine malformed Service
  records now reach refusal and preservation checks. The remaining FFI case
  reaches its metadata read but expects the single-record request form; the
  real enabled policy may issue its batched read round first. This remains open.
- core-lint/20261101T092139Z-26991: PASS.
- similarity/20261101T092316Z-27792: PASS.
- private/20261101T092057Z-26406: source files passed; the log passed after
  rewrite in private/20261101T092156Z-27311.

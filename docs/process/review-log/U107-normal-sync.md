# U107 normal sync — in progress

> **2026-10-26: Anki server work is frozen indefinitely.** Earlier next steps
> and acceptance requirements below are retained history. Do not resume them
> without Antoine's explicit request. See [the delivery scope](../review-first-delivery.md#anki-server-freeze--2026-10-26).

Starting commit: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Batch base: the same commit. Planned batch: U107, U110, U111.
The start was recorded before spec intake in `build/unit-starts/U107-20261025.json`.
Previous full gate `20261025T110905Z-28599` passed on this base.

## Prepared contract intake

Reuse the neutral U107 packet at `spec/U107`, commit `8da0e61d`.
No existing U107 production implementation was found in the recorded audit.
Spec session: `s-20261025T132312Z-c047b13c`.
Golden intake session: `s-20261025T132746Z-35ca6910`.
The 18 prepared golden assets initially matched their source blobs exactly.
Their original recording commits are `ed4f2b52` and `e407410c`.

Keep Anki reconciliation behind explicit Settings transfers; automatic sync remains
CloudKit. Retain the custom-server wait exception and U29 real notetype-upload
regression. No actual connection values or private fixture data are used.

## Contract review

Independent pre-implementation review found one P2 at plan Approach 6: an extra
missing-note skip contradicted R1527, normal-sync N6.2 and hold-back H10.2.
The sentence now preserves the card's own unsent-grave skip, merges an otherwise
eligible card even when its note is absent, and never recreates the note.
Re-review found no remaining P0–P2 finding in this clause. This is contract intake, not implementation acceptance.

## Open verification

Golden check `20261025T132817Z-84583` failed: five hold-back scenarios lack
`review_session` parser support; schema validation has ten such step errors and
three observations using the two missing sync-error values. Thirteen parseable
prepared scenarios are fresh. The remaining five are uncertified.

Privacy `20261025T132907Z-86068` failed on documents and two generated test rows.
Intake scan `20261025T133335Z-90915` passes after text changes and new numeric fields.
Two oracle records still need observations from those updated inputs. Grammar support
is required first. U107 remains open.

Complete-intake privacy check `20261025T133607Z-94310` rejected only the two old
golden files for the changed test inputs. They are retained under ignored build
storage and will not be published. Both scenarios remain enabled and require
fresh records. The other 16 prepared goldens remain in the intake.

## Grammar support

Filtered implementer session `s-20261025T134357Z-235af9a2` exported support in
`cleanroom/20261025T134422Z-4951`; only standard MG formatting followed.
Rust grammar check `135224Z-17688` passed all 47 cases. Schema test `135212Z-17253`
failed because a marker result was accepted despite its no-output contract.
Independent review confirmed this P2; its earlier clear receipt was superseded.

Filtered fixer `s-20261025T135952Z-cc91c1ec`, run `140017Z-26669`, added one explicit
false property. It preserves draft keys and the original assertion. Schema run
`140155Z-28699` now passes all five inner tests. Re-review has no P0–P2 findings.
Similarity `140154Z-28468` passes. Rust source is unchanged from the 47-case pass.

Prepared oracle support is in `89de0e5c`: all 25 focused tests and privacy passed,
and independent review is clear. The two changed inputs still need oracle records.
The normal executor and its acceptance checks remain outstanding.

## Intake metadata

The early intake commits placed role data in their message bodies. Those pushed
commits stay intact. This table records the source sessions; new code commits use
one contiguous trailer block.

| Source commit | Role | Session |
| --- | --- | --- |
| `b74696b2` | spec-writer | `s-20261025T132312Z-c047b13c` |
| `8e201054`, `89de0e5c` | oracle-maintainer | `s-20261025T132746Z-35ca6910` |
| `1aac94de` | implementer | `s-20261025T134357Z-235af9a2` |

The session ledger and intake packets retain the recorded roles and source hashes.
U107 is not accepted.

## Fixture validation after support

Oracle run `140646Z-33304` recorded only the two changed cases, both successfully.
Goldens check `140717Z-34200`, based on the unit start, reports all 697 scenarios
fresh and valid schemas, including the 18 U107 cases. Asset privacy `140725Z-34489`
passes. The oracle pin and locks are unchanged; no unchanged case was recorded.
This proves the test inputs and expected data, not normal-sync execution. The
production job and its acceptance checks remain outstanding.

## Temporary local macOS feedback

Antoine requested Xcode MCP on this Mac for the current iteration on 2026-10-25.
The working baseline at `3ea7d5dd` built for My Mac in 89.282 seconds with no errors;
launch completed in 12.139 seconds. One local app instance was verified. VM and
mobile iteration checks are deferred, with required integration/final evidence
still outstanding. This baseline does not include the unverified production export.
The local bridge keeps workspace opening and its action in one connection; saved
workspace handles expired between separate calls in this setup. Mac-only catalog
extraction was archived separately and restored for later cross-platform review.

## Bounded prerequisite contracts, 2026-10-25

Recovered the four already-authored contract/provenance files from the saved work; every SHA matches the spec-writer packet. The independent review of neutral rename F1–F5 and ChangeSet S1.1 found no P0–P2 findings. Publication privacy passed. Source session remains s-20261025T132312Z-c047b13c. This is the rename-only prerequisite inside U107; full U34 stays in M1 without a Unit trailer. U107 remains incomplete. The batch base remains 1416a520542a8c5daa38a23bfe398da704c3d6c9.

## Session routing and retained payload contracts

Independent review of the published S8.4 per-session routing contract is clear.
The existing native live runner needs a separately owned loopback route for each
session; implementation and the 18 live sync cases remain outstanding.

The storage journal J1–J4 and change-stream S2.1–S2.3 contracts cap retained
before/after row bytes at 8 MiB. They require
immutable same-commit reads, ownership through queued consumers, pre-COMMIT
compact facts and explicit read-failure handling. Independent contract review
found no P0–P2 issues. Privacy `170557Z-96547` and static checks `170631Z-96994`
pass. Earlier rejected prose scans remain recorded in the contract packet.
Implementation, allocation measurements and device limits are not yet verified.

## Production checkpoint and focused diagnosis

Integrated the production export from implementer session
`s-20261025T134357Z-235af9a2`, then the filtered repairs from fixer session
`s-20261025T162405Z-9dc656e4`. Independent review round 1 found a schema race
between metadata and snapshot capture, plus missing undo targets for removed
configuration keys. Both repairs have focused regression cases.

Focused runs `162809Z-61925` and `164931Z-78983` failed during compilation;
neither executed tests. The next run compiled the workspace and is still active
at this checkpoint: 18 cases passed before its first 150k-card case. This is
partial evidence only. Round 2 review, the remaining prerequisites, full focused
results, privacy/similarity checks and the batch gate are pending. No Unit
trailer is added.

Focused run `170327Z-92729` finished: 35/38 pass, including both 150k cases.
Failures remain enabled: post-snapshot `curDeck` pending status, held-deck
reconciliation and the sanity-failure test's admission path. Round 2 source
review confirms the earlier schema/config-undo repairs and finds no new P0–P2
issue; the failed focused checks still require diagnosis. Static checks
`172050Z-9413` pass for the original batch base.

The six-file rename prerequisite has a clear independent source review and is
integrated as WIP. Its first focused run `172233Z-12378` failed compilation:
the new storage regression compares a note type without `PartialEq`. No rename
tests ran. The correction must preserve complete aggregate equality checks.
The initial formatting attempt could not reach Docker under sandbox limits;
the authorized Docker run formatted successfully. Full U34 remains outside
this prerequisite, and neither U34 nor U107 is accepted.

Filtered fixer `172405Z-14357` repaired the storage test comparison and two
invalid server fixtures: a server deck carried client-pending USN -1, and the
sanity test supplied equal metadata that correctly bypassed the round. Their
replacement assertions preserve and extend the original behavior checks.
Independent source review confirms these diagnoses. The same-instant config
pending bug remains open; counting all config USNs would incorrectly include
day-clock bookkeeping and received positive USNs.

The real rename/upload/fresh-peer regression is included from implementer
session `s-20261025T172143Z-1e9723c8`, with a separate filtered repair
`172954Z-18122` for its mistaken template-stamp expectation. Re-review is clear;
template stamps remain captured data. The test exercises complete wire objects
and unknown metadata, with actual normal jobs and unchanged schema stamps.

Focused compilation `173407Z-21279` exposed two more unsupported equality
assertions in the engine rename tests, after the storage compile repair.
No tests executed in that run. Both require complete-data comparisons; the
Use existing stored fields and opaque bytes for these test assertions.
Similarity `172439Z-14974` passed the prior checkpoint (58 files, zero flags).
Later deltas still need their checks. U107 and the batch gate remain open.

Filtered test repair `173619Z-22958` corrected the remaining unsupported
equalities using complete stored scalars and protobuf bytes. It also replaced
text rejected by the private scan; the guard is unchanged. Focused run
`173937Z-25330` compiled and executed: 79 pass, two fail. The actual rename
upload/fresh-peer regression passes, as do corrected sanity and held-deck cases.
The remaining failures are the known same-instant config pending bug and a
rename test that assumes card-ID order equals template-ordinal order
(actual ordinals 5, 0, 1 versus expected 0, 1, 5). Preserve exact membership
and duplicates when correcting this test-only ordering assumption.
The two large sync cases passed in the earlier full focused run; they were not
repeated by this small-case filter. U107 remains unaccepted.

## Commit payload and settings repair

The bounded payload export used clean input `d50af3a2`, with 22 owned paths.
Filtered repair `174954Z-32451` added the local modification sequence fact and
fixed the test clock. Review is clear. Final stamping supplies the fact before
COMMIT; the writer records it only after success. Local and KeepMtime writes
count even when the stored timestamp stays equal. Raw day-clock work does not.
The normal merge uses this fact above S. Config set/removal, empty writes,
rollback and the following clean round have tests.

New observer comparisons needed full stored card rows instead of unsupported
typed equality. Filtered repair `180321Z-45065` preserves all columns and the
None/Some checks. Rejected comments were removed without changing assertions
or privacy rules. The source reviews and original failed runs remain retained.

Focused combined run `180714Z-48860` passes all 199 executed cases across
journal, savepoint, observer, queue, precondition, normal-sync and media checks.
It includes the settings repair and media report error/resume cases. Allocation
and original held-run memory runs are separate and pending at this checkpoint.
The marker change belongs to U110 and is kept in its own source commit.
The live-session cleanup repair, typed skipped IDs and batch gate remain open.

The allocation run `181049Z-51995` passes the 150k-card width test. Original
held-run memory check `181135Z-51994` also passes. These are Rust heap/payload
proofs; device and process limits remain separate requirements.

## Whole-route lifetime correction

Worker exit can delete its proxy and stop its backend. Parent-only cleanup
cannot keep the old route safe. The corrected S8.4/S10.4 contract assigns the
proxy and worker namespace to the caller before begin. A container-local holder
keeps the namespace separate after driver exit; cleanup removes edge, proxy,
then namespace. Failed cleanup retains the remaining resources.
The expected worker comes from trusted launch/checkout state, independently of
the begin result. Re-review of this binding has no P0–P2 findings.
Implementation requires a small oracle-wrapper handoff as well as MG/helper
work; the earlier no-driver-change claim was wrong. Existing partial exports
remain saved. Actual live cases, cleanup tests and the batch gate are pending.

## Typed skipped-row diagnostics

The 15-file filtered delta is byte-exact to its runtime export and has a clear
independent source review. Actual own-grave decisions produce sorted typed ID
lists through results, events, FFI summaries and the Swift bridge. Empty
constructors, no-op publication and rollback behavior remain unchanged.
Filtered repair `183724Z-79187` uses full stored card rows in its rollback
assertion and records that test convention. The prepared ordinal-order test
correction preserves exact card membership and duplicates.

Run `184521Z-83695` executes 61 cases: 60 pass, one fails. All 14 rename tests
pass. The new repeated-key normal-sync case exposes a scratch bulk insert of
duplicate fresh IDs; its regression remains enabled. Fix the staging behavior,
not the fixture. FFI library run `185302Z-89044` passes all 69 cases, including
nonempty conversion, summary and event payloads. Swift bridge run
`181902Z-59842` regenerates bindings and passes both selected tests.
The source delta remains incomplete until repeated-key behavior, actual live
transport and the required integration gate are verified. No Unit trailer.

## Repeated incoming IDs

The repeated-key failure needs a staging repair, not a weaker test. N6.5/W7.1
now state ordered processing for repeated IDs, arbitrary strict i32 USNs and
first-existing review rows. Card/note values use fixed metadata. The stage also
stores each note's numeric validation record. Apply reads these records in order
and checks the live row before commit.
Own pending graves mask their keys. The pending/mtime test still applies after S.
The first contract review found a lost conditional note error; the repaired
contract has no P0–P2 findings. Source provenance stays separate. Implementation
and focused regressions for this correction remain pending.

## Whole-route integration checks

Route source has 24 files. Its hashes are checked. The worker
metadata schema has a passing focused harness (`193423Z-16409`, eight
internal cases). Rust formatting passes (`193537Z-17393`). Focused test run
`193555Z-17762` executes 60 cases: 58 pass; two retain old assertions that
Meguri cannot play a sync actor. Preserve the no-execution-in-T0 assertions
while correcting these capability expectations. Self-test `193620Z-18166`
passes nine cases; the quarantine fixture fails before producing its nested
gate receipt. Its fixture does not list the newly registered transport job.

Independent review also found an allocation/cleanup race: a helper can publish
an edge after the owning CLI exits and its route has been released. Fix the
allocation and sweep ordering, retain a closing fence and prove the paused
allocation case. Actual transport and live sync checks remain pending.

## Follow-up repair checkpoint

Route run `193953Z-22375` repairs the allocation/sweep race with shared and
exclusive admission and a persistent closing fence. Review round two confirms
the P1 repair, but finds a P2 in cancellation cleanup of extra fixture owners.
Their contexts must be registered before allocation. Focused Rust run
`194943Z-29510` passes 60/60; self-test `194948Z-29776` passes 10/10. Real
transport and live scenarios are still unrun.

Repeated-row run `192926Z-13058` preserves the original duplicate fixture and
adds six mechanism cases. Independent review finds a P2: validation replay
work is absent from the declared write budget. Focused run `194405Z-25706`
compiles and records 96 passing cases and one test setup write-mode error,
then stalls on a nested reader acquisition in a new test. The integrator
cancels that run via MG; all unfinished cases remain unverified. The budget
and fixture repairs are in filtered run `194914Z-28664`.

This is a WIP checkpoint only. Both findings, actual route execution, the
original-base batch gate and U107 acceptance remain open.

## Ordered-row repair verified

Review round two clears the write-budget and fixture repairs. Focused run
`195439Z-32853` executes 139 cases: 138 pass, including the original repeated-ID
regression, all other normal-sync cases selected and the FFI library checks.
The single new WAL assertion samples its evidence before the recorder stores it.
Test-only run `195732Z-35011` preserves the one-second automatic flush deadline
and captures the applied commit key. Independent review is clear; focused run
`200039Z-37229` passes the corrected case. The earlier failed run remains saved.

Route self-test `200344Z-39313` passes all 11 selected cases. Privacy check
`200342Z-39122` passes across all 40 pending files, including new files. Actual
transport, fresh large-merge coverage and the batch gate remain pending.

## Route follow-up

Round three review has no P0–P2 findings. Owner contexts are saved before
allocation. Parent cleanup reads all contexts. Self-test `200344Z-39313`
passes 11 checks. The transport run is active; its result remains pending.

Core lint `200411Z-39826` fails: apply takes nine arguments and three labels
use conversions. Fix these before the batch gate.

## App delivery priority and open checks

Antoine asked to stop extended Anki sync investigation and return to app
delivery. The sync batch is parked, not accepted. The original integration
base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The private merge-input refactor has a clear independent review. Lint run
`201709Z-48187` clears those four sync diagnostics but fails on three testkit
checks: the actors expect after is_some, items after the storage test module,
and a guard held across await in the sync adapter. These remain open.

Real transport run `200526Z-40823` ends with error and zero transport passes.
The before-token cancellation cleanup cannot prove proxy ownership. Source
triage does not establish the cause. The failed receipt and owner registry
remain in that run directory; reservations remain held. Inspect through MG
and prove exact-owned cleanup before reusing them. No force cleanup or fault
service restart was performed. Actual live cases and the batch gate remain
pending. Return to the shared app and review workflow; resume these checks
before sync or release acceptance.

# First Mac CloudKit smoke

Status: WIP; no signed execution or Unit acceptance.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Client integration start: 74ea4958.
Contract: conformance/cloudkit-agent J2, J3.1; cloudkit-acceptance Q5.

Filtered client export cleanroom/20261102T133207Z-6004 adds the ten-action
program and fixed storage inspection. The recorded author is fixer
s-20261025T162405Z-9dc656e4; the handoff prose names a different session in
error. The export receipt supplies the actual provenance.

Independent review cloud_smoke_review is in progress. Parent coordination is
still being authored separately. All new focused checks and builds are unrun.
No SDK work may start before both halves are reviewed together. Generated
checks and compilation are not signed execution evidence.

Integration removes one redundant comment after a privacy finding; executable
code is unchanged. Staged privacy scan private/20261102T135313Z-24442 passes.

Host integration start: 394edd34. Filtered export
cleanroom/20261102T133203Z-5702 adds the tools-only parent and generated tests.
Its completion validator remains implementation_missing. Review also found
that coreOpened's actual count field is rejected by the parent's current fact
parser. Both need one shared serialization repair before execution. Integration
renames internal tooling names and removes redundant comments after the privacy
scan; no command, assertion or process behavior changes.

The first focused Rust invocation lacked the required test-hooks feature and
did not compile tests. The corrected invocation then found an unsupported
rusqlite u64 conversion. Filtered compiler cleanup
cleanroom/20261102T135550Z-27512 reads i64 and checks conversion; independent
review found no required finding. After that fix,
core-test/20261102T135755Z-29493 passed all three selected storage/FFI tests,
with no skips. This is local generated-data proof, not CloudKit execution.

Independent reviewer cloud_smoke_review reported P1, the
coreOpened shape mismatch above; P2, missing actual SDK/state-save ordering
and outgoing review hold-back evidence. The explicit completion Missing fence
also remains open. A filtered shared repair is in progress; no SDK run starts
before independent re-review.

On clean b139641a, focused checks pass:
selftest/20261102T140138Z-34631 (3/3 host wrappers),
swift-test/20261102T140139Z-34902 (1/1 real bridge inspection),
swift-test/20261102T140336Z-38293 (41/41 protocol/product tests), and
build-app/20261102T140512Z-40625 (Mac Debug CloudAgent compile only).
No test skips are reported in these focused results. The Mac build does not
launch the test host, prove signing or exercise CloudKit.

The combined repair cleanroom/20261102T140128Z-34085 was cancelled and joined
after 20 minutes with no source changes. Its error result is retained. Split
host repair cleanroom/20261102T142316Z-52904 fixes the exact launch fact and
validates current step encodings and retained observations. Independent review
found no required finding. Command 3 explicitly remains Missing for SDK order;
the app/core evidence repair runs separately. No signed pass is claimed.
Integration removes one redundant docstring after the privacy finding.

Filtered evidence export cleanroom/20261102T142707Z-55686 adds the bounded
SDK, own-operation and held-image observations. Fixer session:
s-20261102T142603Z-4626b865. Source start: 5e58e13a; integration start:5999cc58.
Independent review and all new focused checks remain pending. The host keeps
its explicit Missing boundary until it validates the exact added facts.
Integration renames one generated-test local binding after the privacy finding;
the assertion and executable checks are unchanged. Privacy68716 passes.

Evidence review found one P2: valid existing media-root conflicts were rejected
although production publication accepts them. Filtered repair
cleanroom/20261102T150241Z-72116 retains the typed conflict for saveMediaRoot
only, and requires the original Core commit plus completed publication.
Independent re-review found no required client issue. Encoding is unchanged.
New native/generated checks remain pending.

Core-test/20261102T150603Z-73358 passed 17/17 selected real builder, publication,
review, storage and FFI regressions, no skips, after the shared Rust refactor.
This uses generated local data and supplies no actual CloudKit result.

Host evidence export cleanroom/20261102T150002Z-70976 integrates the fixed
SDK/own-operation/held-image/ACK/join encoding. Independent review found one
remaining P2: it requires a positive held-period batch instead of accepting
recorded zero batches plus the separate deterministic eligibility proof.
The correction remains pending. All other required wire/order checks cleared
source review. Integration renames one generated test method for privacy;
its body and assertions are unchanged. No actual smoke pass is claimed.

Filtered zero-batch repair cleanroom/20261102T151119Z-75535 removes only the
positive exercise requirement. Exact zero counts, deterministic eligibility,
all observed-batch checks and SDK/own/ACK/join checks remain. Generated cases
cover zero batches, no target-card handoff and inconsistent evidence. Review
and focused execution after this correction remain pending.

Swift-test/20261102T151316Z-75981 passed 25/25 selected native bridge tests,
no skips: Core inbox-before-state order, held edits, exact revision retention,
inspection and captured native own operations. These use generated data and
captured operations, without adding them to a live CloudKit database.

Independent re-review cleared the zero-batch correction. On clean57ffce27,
selftest/20261102T151720Z-78500 passed all 3 selected host wrappers and
swift-test/20261102T151719Z-78108 passed 60/60 coordinator, presentation,
protocol and running-product tests, no skips. App-hosted evidence tests and
actual signed execution remain unrun.

On cleanf6e0b036, all11 CloudKitSmokeObservationTests execute as separate
focused app-hosted VM checks with one pass and zero skips/failures each:
4281,33328,34734,36373,37770,39151,40557,41947,43408,44804,46282
(mac-hosted-test/20261102T16*). They cover exact current ACK revisions and
blob proof, generated media blocks, original operation phases, typed media-root
conflicts, actual callback ordering checks, zero batch admission, held images
and ACK/batch/returned-state linkage. These tests supply generated observations;
none is a live SDK publication or real account proof.

Static-checks/20261102T162332Z-5554 passes3/3 at the original batch base.
Similarity/20261102T162335Z-6474 scans708 files with zero flags and zero
allowlisted matches. The original failed full gate remains open; no Unit
trailer, milestone acceptance or signed-smoke pass is added.

## Durable callback observations

Starting commit `921d0bb9`. The signed test runtime now uses the agent's
durable journal writer during the awaited callback and batch handoff. The
ledger releases its mutex before the writer appends. A callback's commit and
return retain the command selected at delivery, including across a successor
start. A batch without a known causal command uses the latest started command.
Ordinary traces already buffered are written in their original order. An
append failure uses the agent's existing journal fence and join path.

The host reads callback traces in journal order up to each terminal record.
It accepts late callback phases under the original command before close, and
still refuses any trace appended after the joined close. The account check
remains the first ordinary fact for a command. Trace observations can precede
it while the account check awaits a reply.

`swift-test/20261110T230521Z-98754` passes all 37 protocol tests. The new test
uses two started commands and confirms durable append under both exact hashes,
with the original command retained after the successor starts. The first
build `97363` refused conversion of the writer to a sendable closure. The
writer now has an explicit main-actor boundary. `selftest/20261110T230630Z-2033`
passes five wrappers, including the new retained-journal order case.

`mac-hosted-test/20261110T230704Z-2399` executes the new callback and batch
handoff test in the VM, with one pass and no failures or skips. The first
build `269` refused a mutable test capture. The fixture now installs two
writers with fixed command values and retains the original phase assertions.

Direct review checked callback attribution, append failure, late trace order
and the close boundary. This change supplies the durable append path for J4.
Distinct engine labels after re-creation, per-record and pending-list evidence
for J4.1, and original exchange reconstruction for J5.1 remain open. No
signed CloudKit run, full gate or Unit completion is claimed.

## Engine identity across replacement

Starting commit `bacd5f5f`. The signed test runtime gives each admitted engine
its own label. A restored runtime reserves the original journal's labels.
Callback phases and batch handoffs retain the engine label. An upload
acknowledgement needs a completed state callback from the same engine. The
phone's fetch proof also keeps its phases within one engine.

Direct review checked alias reuse, callback attribution, batch identity and
close across engine replacement. The host verifier requires each engine's own
admission and refuses a later engine's saved state as proof of an earlier
engine's upload.
`selftest/20261110T231747Z-11764` passes all five wrappers.
`mac-hosted-test/20261110T231745Z-11229` verifies that a restored admission
leaves the original callback ledger empty and uses a fresh engine label.
`mac-hosted-test/20261110T231804Z-11228` verifies that a replacement engine
can reuse native ordinals and batch IDs while the original upload still
needs its own completed state callback.

J4.1 still needs per-record results and pending-list evidence. J5.1 still
needs original exchange reconstruction and a signed recovery run. The
full gate and signed round trip remain open.

## Original sends and saved records

Starting commit `a089494a`. The selected test observer links each built batch
to the operation in its native send context. The journal uses run-local
labels for operations and records. The manual
return follows the original SDK call, and a scheduled end event does not
close a batch. The existing runtime path does not install this observer
outside the selected test.

The Swift and host checks require a saved record to match one open batch of
its own engine. The Mac VM test `84770` shows that a rebuild stays ambiguous
until the earlier manual call returns. Host run `84514` passes five wrappers
and refuses an acknowledgement for an entity absent from the saved batch.
Swift run `86934` passes 71 existing sync and native media tests.
Direct review checked native context binding, label reuse, result order and
ambiguity refusal.

This records successful saved outcomes. Failed results, adapter pending-list
changes, and state saves naming the batches they reflect remain needed before
J4.1 and J5.1 can be checked in full. The live signed run still awaits the
approved test account.

## Pending records and saved state

Starting commit `5050a1a1`. The selected native test records the pending list
from the engine while its original state callback is active. It records each
adapter update with the prior presence of removed records and the additions
the adapter applied. Both paths use run-local aliases and durable journal
groups. The host derives the batches named by each state save from those
facts and checks the earliest matching save in journal order.

Direct review checked callback authority, journal grouping, pending changes
and the order of the original result and state save. It found that the Swift
check used the batch key to find the state callback. The corrected test
requires the exact callback to return and refuses a state save that preceded
the result even if a later save would pass.

Mac VM run `97087` passes the focused callback test. Swift run `94761` passes
71 sync and native media tests. Host run `94802` passes five test wrappers.

This covers saved records. Failed and deleted record results, the exemptions
for batches without a result, full exchange replay and the signed round trip
remain open. This change carries no Unit completion or full-gate claim.

## Failed saves and deletes in the original batch

Starting commit `ecaec5a8`. The selected test records delete members alongside
save members and records the actual SDK outcomes before the callback returns.
It records only labels and numeric CloudKit error codes. The host checks
membership, operation type and the durable callback group. A result can name
only one still-open batch. An all-error batch with only the specified pending
errors needs no state pair. Other results still need their original pair.

Direct review checked scope, original callback grouping, error classification,
and the boundary between failed results and upload acknowledgements. The
native engine method now explicitly implements the asynchronous pending-list
requirement so the selected observer uses its native read.

Mac VM run `2060` passes the focused save and delete cases. Host run `99922`
passes five wrappers, including refusal of a save outcome for a delete member
and of close without the required state pair. The VM answered `doctor` run
`3738`. Batches without a result and original exchange replay still need
coverage. No signed round trip or full-gate claim is made here.

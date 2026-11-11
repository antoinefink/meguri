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

## Original exchange capture

Starting commit `b1913578`. A test-only Core read returns the live capture
for the original issued policy command and its exact Runtime exchange. The
selected app test writes the collection sequence and aliases of the selected
entities in one group before running the exchange. The same aliases identify
entities in the SDK batch journal. The host and the relaunch parser require
the capture between issuance and result, from the same process, and compare
its selection size to the original result.

Direct review checked command ownership, the capture lifetime, alias reuse
and the observation order. Rust run `5578` compares the test export to the
actual capture and refuses a different Runtime exchange. Mac VM run `5807`
passes the alias test. Host run `7339` passes five wrappers, including the
missing capture and missing member cases. Swift run `9614` passes 13
selected protocol and restoration tests. Swift run `12111` passes the seven
instrumentation registry tests. The capture supplies one input
for replay. Image sequences, held flags and the full outcome computation
still need coverage before Mac exchange recovery can pass.

## Collection images accepted by CloudKit

Starting commit `2143b730`. A successful collection save uses the image
sequence of its own native request when deciding whether it covers the Sync
Now capture. Identical bytes handed later do not change that request's
sequence. A selected entity that has become clean by completion counts as
done even if its last save predates the capture.

The selected test reads the image sequence and held flag from the original
Core request. It checks the envelope before appending those facts. The host
requires the image in the same journal record as its matching result.

Direct review checked the receipt authority, the clean-state check and the
matching journal group. Rust run `20603` passes all 23 policy-driver tests.
The new case verifies two identical handoffs across the capture and both a
clean and a pending outcome. Host run `14523` passes five wrappers including
missing and wrong-member image evidence. Mac VM run `21548` verifies both
values of the held flag at the original result. The historical conflict and
fetched own-save paths still need exact image accounting and journal evidence.
The signed round trip and full gate remain open.

## Lost replies in Sync Now accounting

Starting commit `8ae6b5ba`. A conflict can acknowledge its matching retained
flight after Core records the result. A fetch can do the same after the
incoming settle commits. Each path credits the exchange only when Core can
link the flight to a handoff in the current opening and its image meets the
capture sequence. An older result still updates tracking and leaves a newer
local change pending.

The exchange keeps the held flag from a qualifying acknowledgement. An
unheld acknowledgement satisfies that entity without a held flag. The
reported held count also includes pending held names, once per name.

Direct review checked the binding, the final batch guard, durable receipt
order and compatibility with older exchange rows. Rust run `29406` passes
29 tests, including both discovery paths on either side of the capture.
Core lint run `30752` passes. The signed journal still
needs the original conflict and fetch acknowledgement facts for replay.

## Original evidence for lost saves

Starting commit `96dfaf5b`. The selected test reads Core's conflict
acknowledgement only while its sent handler still owns the exact batch. The
app checks the original reply before writing the image sequence and held
flag in the callback's record. A fetched save is recorded only after its
incoming settle. The app matches it to a completed fetch of the same
envelope, then appends it before the original exchange result.

Direct review checked original batch ownership, fetch member identity and
journal order. The host refuses a conflict image attached to a successful
save, an applied acknowledgement for the wrong fetch member, and one
written after the exchange result. Rust run `32033` passes both focused
checks. Mac VM run `32306`, host run `33874` and core lint run `35377` pass.
Swift instrumentation run `36196` passes seven tests.
Replay and the signed round trip remain open.

## Batches without record results

Starting commit `c14a077b`. The observer and host now check unreported
batches at every later state save. Each member needs an original removal
after its build, while the engine held it, with no later add before that
state. A later removal cannot repair evidence missing from an earlier state.
An empty sent callback now closes its own batch before another batch can
receive a result for the same record.

Direct review removed an observer tick comparison and added the host's
complete-membership check. The observer follows actual adapter transitions
for each batch. The host checks the journal's record order. Missing members
cannot satisfy the removal rule.

Mac VM runs `52513`, `54371`, `56082` and `52512` pass one test each,
with zero skips or failures. They cover unreported batches, empty returns,
live state reads and original send returns. Host run `59652` passes five
wrappers. Similarity run `59943` passes. Run `52228` rejected a suite-only
argument before execution.

Signed selected run `38952` built and installed both development products.
It ended `awaiting_user` because the approved account source is absent.
Its `signed_execution` is false. Exchange replay, the signed round trip and
the full integration gate remain open.

## Incoming records in the original exchange result

Starting commit `336f56e1`. Runtime now classifies the incoming rows when
it ends an exchange. Generation, reference and structure waits permit
success once selected work is satisfied. Ready rows still require application. Format, asset
and apply failures prevent success. The completed Runtime row retains
these counts, and policy uses them when it records that result. Later
incoming work cannot replace them. Older rows decode with no retained
counts and use the existing fallback.

Direct review checked classification precedence, the guarded tracking read,
the durable commit and legacy decoding. Rust run `61215` passes the new
regression across all eight reason variants. Run `61945` passes 28 Host
policy and policy-store checks. The regression changes each inbox reason
after completion and checks the original result through policy persistence.

Review also found that capture used sendability filters. It now reads all
dirty entity names before those filters. The original revision checks and
capture gate still apply. A local edit waiting behind incoming generation,
reference or structure work stays selected and outstanding.

Run `65126` passes 28 checks and fails the new test at its early cached-list
read. The same assertion now follows capture's actual pending update.
Run `65853` passes that corrected test across the three waiting reasons,
checking preserved local data, selection and incomplete-result accounting.

Swift run `66499` passes both production publication, exchange and reopen
tests against the final source. Core lint run `67958` and similarity run
`67677` pass.

Blocked-entity classification still needs work. Held, newer-pending and
media counts also need original evidence for interrupted-exchange replay.
The signed round trip and full integration gate remain open.

## Stored blockers and pending references

Starting commit `3a949b0b`. The original Runtime result now retains blocked
selected names separately from outstanding names. Format-waiting records,
refused records and stored media blockers propagate through pending
references. A clean reference stops that walk. Actual acknowledgements and
clean selected images take precedence over blockers.

The sender and accounting reader share the existing structural and media
reference rules. Plain notes avoid the media-ledger read. Media names with
`file_missing`, `withheld` or `superseded` leave the pending selection under
K12.1. `unsyncable` and `removal_held` stay pending and blocked. Older result
rows decode an absent blocked-name set as empty.

Direct review checked binding scope, dependency traversal, precedence,
legacy decoding and the original durable commit. Run `69873` passes the
Host case and finds two fixture errors. Runs `70551` and `71227` settle the
fixtures with the proper layout seed path, current ledger revision and
required acknowledgement metadata. Run `71566` passes all 58 affected Rust
checks. After removing unnecessary reads, run `71890` passes 13 sender and
accounting checks, including the 400-record send. Lint `74246` and similarity
`73739` pass.

Swift run `72588` passes both publication, exchange and reopen checks.
Swift run `72587` passes the large-note conflict exchange through the real
Core and adapter with the test transport.

This covers the stored blockers above. Payload-size and unreadable-row
classification and generation-claim integration still need work. Original
exchange replay, the signed round trip and the full gate remain open.

## Original completion work

Starting commit `8a9cb2fb`. Runtime now stores held and unheld pending names
and media facts with the original completed exchange. Policy recording uses
that snapshot. Later edits cannot change those reported values. Older rows
without the optional snapshot retain their existing decoding and fallback.

Direct review checked lock ownership, provider re-entry, original completion
timing, set membership and optional media values. No independent agent review
ran, following Antoine's instruction to work in this session. The first build
found an ambiguous State type. The explicit Runtime type fixes that error.
Focused run `77626` passes the before/after-edit regression. Run `78363`
passes all 31 affected Rust checks. Swift run `78698` passes both publication,
exchange and reopen checks. Lint `79816` and similarity `80360` pass.

The snapshot is product accounting, not original callback proof. Original
journal recomputation, the signed round trip and the full gate remain open.

## Original-command work reader

Starting commit `e8c3f312`. A test-only reader exposes the original completion's
held-pending names, held-acknowledged names, unheld pending names and media
facts. It requires the original issued RecordExchange handle, its retained
capture and matching completion time. Missing legacy snapshots fail the read.
It reads no current status and grants no recovery or SDK acknowledgement.

Direct review checked original-owner and command guards, issuance equality,
capture lookup and the absence of foreign calls under the driver lock. Rust
run `81628` passes the frozen-work regression, changed-command refusal and
closed-owner refusal. Swift run `82336` passes both publication and reopen
checks, including the FFI read and refusal of old handles after reopening.
Run `85494` passes all seven interval-registry checks. Lint `83456` and
similarity `83783` pass. Doctor `84300` reports working Docker and VM.

This reader does not yet append original work to the signed journal. The
replay implementation, signed round trip and full gate remain open. No
independent review or completed-unit claim is made.

## Original work in the signed journal

Starting commit `3a786582`. The signed test backend reads work through the
issued result command before recording it. After that record commits, it
appends the original name sets and optional media facts before the terminal
result. Only run-local entity labels leave the ledger. Repeated work groups,
duplicate names, inconsistent held sets and held acknowledgements outside
the original selection fail the producer.

The host requires one original work group after capture and before the
terminal result, in the original process. It computes held counts from the
set union and newer-pending counts from the unheld set, then compares all
observed media fields. Unknown values remain absent.

Direct review checked command correlation, original snapshot timing, label
mapping, group shape and ordering, set membership and optional-field equality.
The first selftest filter selected nothing and supplies no test evidence.
Run `88106` finds one old media fixture missing its new work observation.
After adding that observation, run `90462` passes all four selected harness
tests. VM-hosted run `88405` executes the producer test once with no skips or
failures. Static checks `90799` and similarity `91131` pass.

This is original pending-work and media evidence only. Incoming-entry and
other selected-entity outcomes, full journal recomputation, the signed round
trip and the full gate remain open. The approved account source is still
absent on this host. No account value was read, and setup was not rerun.

## Original outcomes and deletion before upload

Starting commit `8d557ca6`. The completion snapshot now retains original
incoming-entry reasons and selected-name outcomes for the signed test path.
Each incoming entry keeps its ordinal, so two entries for one record remain
two entries. Release builds do not collect the extra named accounting. The
test-only reader requires the same issued command and original completion;
older rows without this accounting refuse the read.

The regression exposed a product defect: a note and card created, selected,
then deleted before handoff left the exchange incomplete. Tracking had
correctly dropped both unsent rows, but the active exchange did not learn
that they were done. The observer now reports those drops only after its
tracking transaction commits. They satisfy only the captured revisions and
never create an acknowledgement. Undo restores newer pending work without
undoing that original completion. Run `93560` reproduces the incomplete
result; run `94247` passes the deletion, undo, clean-selection and incoming
checks after the fix.

The journal producer checks an exact, disjoint selected-name partition and
emits only opaque entity labels and closed reason values. The host checks
that partition, the original incoming counts, qualifying original ACK images,
their held flags and original state-save ordering. It rejects a claimed ACK
without its original result, and a clean classification hiding such an ACK.
Terminal-absence claims require the original unknown-item result. This is
additional evidence checking, not the full interrupted-command replay.

Broad run `95035` found an older tombstone test expecting unchanged observer
stamps after a verified reopen scan. Baseline run `96892`, at the starting
commit, reproduces the same failure. The assertion now checks the real scan's
certified stamps while preserving exact equality of every other tracking
field. Its pre-scan equality assertions are unchanged. The first baseline
build (`96477`) lacked spec data and supplies no test evidence.

The comparison build reused the main Cargo cache and replaced its executable
files. Some Cargo metadata still appeared fresh, so run `97257` does not prove
the current source. After removing the finished copy and advancing source
timestamps, a fresh build in run `98121` passes all 43 selected Rust tests,
including both new tests by name. Source-copy checks need a separate target
directory; tools/AGENTS.md records this.

Swift run `98819` passes both real publication and reopen checks. Core lint
`99932`, verifier selftest `3098` (four harness cases), static checks `3583`
and similarity `4002` (implementation) and `6485` (tools) pass. VM-hosted run `4641` executes the
producer test once with zero skips and failures. Selftest `98521` identified
two fixture/parser issues: a moved state-save fixture shifted non-tick facts,
and a missing work marker received the wrong refusal category. Both are
corrected with their assertions retained. Restricted selftest `1975` reached
no Docker tests; restricted hosted run `1495` was stopped in preflight and
ran no tests. Their host-access runs above provide the actual evidence.

Direct review covered transaction ordering, captured revisions, outcome
precedence, legacy decoding, build-feature scope, original command ownership,
opaque labels and matching SDK evidence. No independent agent review ran,
following Antoine's instruction to work in this session. The full recovery
implementation, signed round trip and full batch gate remain open. No Unit
trailer or current signed acceptance is claimed.

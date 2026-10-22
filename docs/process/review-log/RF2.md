# RF2: safe local collection entry

Starting commit: `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.
Authority: Antoine's review-first ordering, recorded in
`docs/process/review-first-delivery.md`. RF1's full gate and catch-up passed.

This delivery adds supported local collection entry to the real review app.
Start with latest-format, collection-only `.colpkg`. Preserve the source,
inspect a staging copy, verify it and activate it through the planned replace
primitive. Existing local data requires a real safety backup before replacement.
Unsupported formats and media payloads must receive an explicit refusal.
No Apple account or network connection is required for this local path.

Reuse the prepared U124a package and replacement specifications and their
original provenance. Reconcile the obsolete replacement recovery section with
the current persistent generation/account publication fence before implementation.
Reuse the relevant prepared backup specifications for the mandatory safety path.
Keep the full U124a, U126, U127 and U166 acceptance requirements open until met.

One integrated delivery item: package and replacement foundations, real safety
backup, job/FFI service, security-scoped picker and persistent imported reviews.
Component commits retain focused checks and clean-room provenance. The final
Swift/FFI integration gate uses the starting commit above. The spec writer,
filtered implementer, independent reviewer and fixer sequence remains mandatory.
Review covers correctness, plan conformance, clean-room boundaries and UI quality.
No Unit trailer or completed implementation is claimed at this starting record.

## Specification intake

Prepared work was integrated serially on master, preserving original authors,
Role/Spec/Session trailers and coauthors, with Prepared-Commit and Codex attribution.
No new branches or worktrees were created. No production implementation was changed.

| Prepared commit | Master commit | Content |
|---|---|---|
| 0fe35ed7 | dd16edae | U124a original spec session |
| c2878318 | 004b56c5 | Container, inspect and verified-replace contract |
| 615937e9 | 0b1c12d1 | Original correction session and incidental earlier oracle session metadata |
| c862a4bf | 0533767e | U32 staging, atomic-file, sidecar and reopen corrections |
| 6c32c73e | 92d5df89 | U124a final spec session |
| dc285e06 | d9d08175 | Inspection-order clarification |
| 59b8bc62 | 3b12b788 | U126 initial spec session |
| 6eb8fdc3 | b615474c | Real backup writer, names, retention and listing contracts |
| 5da94d48 | c8e833f3 | U126 final spec session |
| caa01955 | 08f47084 | Prepared backup evidence status |
| d8b6902e | 0b77efb3 | U127 initial spec session |
| 6b85adf7 | c15b8e44 | Safety backup and trigger contracts |
| 8c3f45ed | 923d5dd5 | U127 final spec session |
| e74b98ab | f74d796c | Prepared trigger evidence clarification |

The original oracle session s-20261017T190507Z-c6475cb1 is already represented
through 0b1c12d1. Keep the ledger append-only; do not duplicate that record.
Other oracle sessions, implementation, fixtures and goldens remain deferred.
Merge commits were not replayed; conflicts retained current interfaces and harvest obligations.

The package area still has `active: []`. Prepared scenarios added: collection-package
10, backups 13, backup-triggers 6. No golden is yet integrated on master.
Full retention and periodic backup implementations are outside RF2's critical path.

Privacy-hook conflicts required complete neutral prose replacements. A synthetic
backup case now has id/path `package/backups/period-boundaries`; a static comparison
proved only its id changed, with all values and assertions retained.
`package/backups/name-variants` has an equivalent new description.
The serial oracle phase must regenerate those two cases through tools/mg;
never hand-edit golden metadata. Preserve the other prepared fixture/golden bytes.
Automatic approval initially rejected committing the id change without its evidence.
The explicitly authorized retry included static scope proof and deferred oracle
regeneration; approval then permitted it. No hook or gate was bypassed.

## RF2 neutral contract

New session: s-20261022T144230Z-5dc66097 (record commit 6b53010a).
`engine/collection-replace#R6.1–R6.4` defines persistent publication identity,
account binding, file-identity recovery and a durable activation boundary.
Missing or ambiguous recovery state keeps publication paused, with local review allowed.
Legacy manual-transfer bookkeeping cannot clear the cloud publication decision.
`docs/process/review-first-entry-contract.md` defines RF2.1–RF2.5:
supported local input, security-scoped copy, inspect, real safety backup, guarded
activation, job/UI integration and acceptance. R7 stamps are applied once for U166.

No builds, tests or gates ran in this spec-writer phase. No full unit is complete.
Remaining verification: oracle integration,
required independent review/fix sequence, focused checks and full RF2 integration gate.
U124a/U126/U127/U166 requirements outside this local path remain open.

## Oracle intake and focused verification

Oracle integration base: `d57529365c2b22612dc2cb24d0b84bb51a0f9740`.
New session: `s-20261022T150435Z-fc55a4bd`, recorded in `5c86f599`.
The first session job ended with a session-lock error because the sandbox could
not write Git metadata; the same authorized command passed with filesystem access.

| Prepared commit | Master commit | Content |
|---|---|---|
| 67978b5c | 60bac8dd | Original U124a oracle session |
| 40b9768f | 91cf1f43 | Package oracle support, fixtures and ten goldens |
| be55016e | 1ce9e55f | Original U126 oracle session |
| 1991f850 | a1d9e862 | Backup oracle support and thirteen goldens |
| c5aa0c27 | 59dfbba1 | Original U127 oracle session |
| 75d938ad | eb170105 | Trigger oracle support and six goldens |

Original authors, sessions, specification trailers and coauthors are retained,
with Prepared-Commit and Codex attribution. All four original oracle session
records occur exactly once. No old branch tip was merged. Registration and
AGENTS conflicts retain current conventions and the prepared additions.

The 14 package fixtures and 27 unchanged goldens retain the
prepared bytes. Only `package/backups/period-boundaries` and
`package/backups/name-variants` were recorded again through `tools/mg oracle record`.
Their observations equal the prepared observations. Their metadata was not
edited by hand. The byte/hash comparison is in
`build/rf2-oracle-intake/preservation.json`.

The unchanged privacy hook rejected new oracle-local identifiers and prose.
Owned lines were replaced with neutral equivalent local names or prose; no
scenario inputs, assertions or runtime rules changed. Automatic approval rejected
one commit attempt until the unchanged scanner and static origin comparison
provided concrete evidence. One evidenced retry then passed. Hooks and their
matching rules were never changed or bypassed. No private collection was read.

Focused receipts (all paths below are under `build/mg/`):

| Job | Result | Receipt |
|---|---|---|
| `oracle test package` | pass, 18/18 | `oracle/20261022T150634Z-33640/result.json` |
| Two safe-case golden recordings | pass, 2/2 | `oracle/20261022T150735Z-34832/result.json` |
| `oracle test backups` | pass, 20/20 | `oracle/20261022T150840Z-36438/result.json` |
| `oracle test backup` on final driver | pass, 28/28 | `oracle/20261022T150935Z-37942/result.json` |
| `oracle compare package` on final driver | pass, 29/29 | `oracle/20261022T150944Z-38617/result.json` |
| Package privacy scan | pass, 39 files | `private/20261022T150627Z-33365/result.json` |
| Backup privacy scan after neutral replacements | pass, 18 files | `private/20261022T150833Z-36149/result.json` |
| Trigger privacy scan | pass, 10 files | `private/20261022T150920Z-37581/result.json` |

The package area remains `active: []`. The oracle replay proves the reference
evidence, not a working Meguri importer or backup provider. No core, app or runner
code changed. No full gate ran in this phase.

Remaining shared testkit work includes the typed collection-package forms,
`package_listing`, backup operations and observations, and their schema/model
agreement. Freshness validation covers inactive scenarios too, so valid parsing,
fresh goldens and oracle replay remain required. Missing executor capabilities
may stay pending while inactive; adding those contracts does not require RF2 to
implement full retention or periodic scheduling. Keep those full parent-unit
requirements open and implement RF2's actual writer and pre-import safety path.

## Writer red checkpoint

Implementer session: `s-20261022T151201Z-48f16d8b` (record `4ac5cd29`).
Filtered run `20261022T151243Z-41150` exported a small standalone writer API
and one independent archive-layout test. Its source snapshot was clean master.
Docker run `20261022T151638Z-44478` compiled and ran exactly that test:
zero passes, one expected failure, zero skips. It reached the production writer
with the valid public fixture and failed because the writer returns `Unavailable`.
Both source-byte preservation assertions passed before that failure.
Archive-layout assertions have not yet executed; they await the real writer.
Cargo updated only the package's dependency edges in the existing lockfile.
This is an intentional test-first WIP checkpoint, not passing implementation
evidence or Unit acceptance. The next continuation implements the writer and
storage-owned helpers, then must make this same test pass without weakening it.

## Standalone writer and schema-model components

Filtered writer run `20261022T151953Z-47043`, session
`s-20261022T151201Z-48f16d8b`, exported the real standalone writer and narrow
storage snapshot/placeholder APIs. The original proof assertions remain.
Filtered parsing run `20261022T152651Z-53627`, session
`s-20261022T152628Z-f2bde0e5`, exported only testkit scenario models and tests.
Both passed boundary probes; the integrator inspected and applied their
disjoint patches. Neither run claims compilation or runtime verification.

Docker format run `20261022T154220Z-60172` passed. Focused Docker test run
`20261022T154246Z-60574` passed all 53 selected tests, with zero skips.
This includes the original four-entry archive proof, source preservation,
destination collision, cancellation, atomic fault paths, storage snapshots,
placeholder contents and prepared-scenario parsing/hash checks.

Bounded-verifier run `20261022T154332Z-61486` passed all seven tests, zero skips.
Independent backend review by `/root/rf2_backend_review` found no confirmed
P0–P2 finding in this component. It covered correctness, plan conformance,
source separation, bounded verification, storage-owned SQL and truthful
unsupported executor requirements. An unconfirmed ZIP64 precedence question
is retained for the general inspection reader; it is not a review finding.

The unchanged privacy hook rejected owned comments, diagnostics and identifiers.
Fixer session `s-20261022T154449Z-17e43054` received the complete preserved patch
through the filtered seed mechanism. Runs `20261022T154542Z-63118` and
`20261022T154959Z-67568` replaced complete owned lines and consistent local
references without changing inputs, expected values, branches or assertions.
No private token was read or inferred. No hook or scanner rule changed.
The first unchanged scan still rejected one test name; the second correction
changes only that identifier. Focused run `20261022T154906Z-66543` passed all
16 writer/storage/verifier tests after the first corrections, zero skips.

Parser commit: `0c22e574`. Unchanged goldens-check run
`20261022T154556Z-63776` passed all 679 freshness checks and validated 1,477
schema files with zero errors. No golden changed.

Final unchanged writer privacy scan `20261022T155111Z-68348` passed.
The final identifier-only correction executed its named archive test in
`20261022T155141Z-68708`: one selected test passed (six others filtered out).
Writer component commit: `6dea5b4d`. Static checks
`20261022T155551Z-71281` and similarity `20261022T155552Z-71471` passed
against the RF2 base. These focused receipts do not replace the final full gate.

Next component starts: verified replacement uses source commit `146c87b7`,
session `s-20261022T154626Z-96381939`, filtered run `20261022T154710Z-65004`.
Package inspection uses source commit `2c6db0e3`, session
`s-20261022T155441Z-633703c8`, filtered run `20261022T155517Z-70627`.
Their source ownership is disjoint, apart from additive exports/dependency
edges that integration must reconcile. Neither is accepted before focused
checks and independent review.

## Live safety-backup snapshot seam

Starting source commit: `89a10998`. Implementer session
`s-20261022T160015Z-02e9d4b0`; filtered run `20261022T160036Z-74758`.
`Reader::snapshot_live` uses only the Maintenance reader outside a transaction,
claims a new file in caller-owned U32 staging, runs VACUUM on that live reader
and finalises the snapshot to WAL format without adjacent SQLite state files.
It never closes or checkpoints the live collection. Snapshot finalisation is
shared with the standalone writer and now checks checkpoint busy status.

Format `20261022T160349Z-76643` passed. Focused Docker run
`20261022T160359Z-76943` passed all 14 tests, zero skips. The new tests prove
that committed WAL answers enter the snapshot while a sync reader retains
its earlier view; the live source, WAL, sidecar and writer remain usable.
They also cover wrong roles, an existing transaction, output collisions and links.
Independent review by `/root/rf2_backend_review` found no confirmed P0–P2 defect.

The unchanged privacy scan rejected new owned prose/identifiers and an arbitrary
test sentinel. Fixer session `s-20261022T160600Z-967041be` used preserved seed
patches in `20261022T160616Z-78553` and `20261022T160958Z-80280`.
Complete comments/identifiers were replaced. The arbitrary sentinel's matching
write and exact-read assertion both use the same three synthetic bytes.
All paths, loops, error expectations, preservation checks and behavior remain.
Independent delta review confirmed no weakened property or new P0–P2 finding.
No private token was read or inferred, and no hook/scanner rule changed.
Final unchanged privacy scan `20261022T161137Z-81265` passed.
Final focused Docker run `20261022T161141Z-81505` passed all 14 tests, zero skips.

This is the storage seam only. Real package backup composition and the import
consumer remain required; no unit or RF2 acceptance is claimed.

The package area stays inactive; parsing does not advertise executor support.
Live-source backup, inspection, guarded replacement, FFI and picker integration
remain pending within RF2. No full gate or unit acceptance is claimed here.


## Replacement, live package writer and inspection checkpoint

Replacement starts at `146c87b7`, session `s-20261022T154626Z-96381939`.
Filtered run `20261022T154710Z-65004` implements private verified staging,
R7 stamps and persistent R6 identity/intent/activation recovery. Sidecar writes
share a strict durable read-modify-write seam; unreadable metadata is retained.
Focused run `20261022T162507Z-90978` passed 28 tests, zero skips. Existing
lifecycle, exclusive, rollover, session recovery, suspension, WAL durability,
crash consistency and atomic-file checks passed all 91 tests in
`20261022T162649Z-91985`, zero skips.

Live package writing starts at `6817c975`, session
`s-20261022T161330Z-07cac90c`, export `20261022T161356Z-82888`.
It publishes one bounded package from the live Maintenance-reader snapshot,
including committed WAL data. Snapshot modification time comes from that copy.
Focused run `20261022T162925Z-93735` passed all 27 selected tests, zero skips.
Independent review found no P0–P2 defect in this writer. Full periodic backup
scheduling, retention and service composition are not claimed by this component.

Inspection starts at `2c6db0e3`, session `s-20261022T155441Z-633703c8`,
export `20261022T155517Z-70627`. It validates raw ZIP directory records before
index allocation, bounds decompression and media metadata, explicitly rejects
unsupported media, and gates/upgrades only an owned extracted copy.
Initial focused run `20261022T161720Z-85248` failed compilation on a count
conversion; no test ran. Fixer `s-20261022T162153Z-ed6f7a95`, export
`20261022T162350Z-89167`, uses checked nonnegative i64 counts. It also closes
review findings for malformed-column error classification and scheduler-marker
allocation before rejection. Spec-writer guidance fixes P8's probe-value cap at
4 MiB; commit `89d4563d` records it. Real pager tests cover oversized TEXT/BLOB.
Combined focused run `20261022T163419Z-97415` passed 90 tests, with 17 filtered.

Independent reviewer `/root/rf2_backend_review` covered correctness, contract
conformance and source separation. Round 2 closes malformed-column handling,
checked counts and scheduler-value bounds. Two related P2 fixes remain for the
next filtered round: reject noninteger `col.ver` without loading its payload,
and reuse that bounded probe before generic replacement opens a candidate.
No acceptance or full-gate claim is made before those fixes and their checks.

Unchanged privacy checks rejected complete owned comments/diagnostics/test names.
Fixer `s-20261022T163040Z-0d038a2c`, export `20261022T163107Z-95290`, preserves
all replacement/writer behavior and changes five complete prose/name lines.
Independent delta review confirmed unchanged predicates and assertions.
The combined scan still flags two own lines in schema/version.rs; the next
filtered fix replaces those complete comments without reading private matches.

The VM returned to its normal desktop after the authorized dedicated-VM restart.
Antoine supplied a screenshot; no password entry was needed. The VM is running
headless again. Doctor `20261022T162442Z-90258` passed SSH/runtime checks with
91 GiB available. Catch-up `20261022T161447Z-83478` passed on clean master.
Later catch-up `20261022T162653Z-92250` refused the dirty integration tree and
ran no tests; rerun it after this checkpoint is committed.


Round 3 starts at `a2f19705`, fixer `s-20261022T164000Z-b4c89cdc`.
Filtered run `20261022T164036Z-2346` reuses the complete reviewed backend seed.
It rejects noninteger version cells from SQLite record metadata, before loading
the value, and calls the same bounded candidate probe before generic replacement.
Real 64-MiB TEXT/BLOB regressions verify bounded pager reads, caller non-invocation
and unchanged candidate/live bytes. Two complete own comments were rewritten.
Independent round-3 review closes both prior P2 findings and finds no new P0/P1.
Format `20261022T164254Z-3635` passed. Focused run `20261022T164322Z-4159`
passed all 86 selected tests (185 excluded by selection). Unchanged privacy
`20261022T164339Z-4719` and similarity `20261022T164342Z-4949` passed.

Lint `20261022T164451Z-5603` fails two new engine style checks: unnecessary
unwrap after an error test and a complex closure field type. Focused compilation
also notes one unused test import. These remain explicit cleanup for the next
filtered provider pass. This is a WIP source checkpoint, with no Unit trailer,
no unit acceptance and no claim that RF2's full gate has passed.


## Forced safety-backup service integration

Provider component starts at `d70c214f`; source session
`s-20261022T164558Z-d8420230`, clean baseline `6ca02e97`.
Filtered run `20261022T164613Z-7405` implements the real package-owned forced
backup provider and production host binding. It uses the existing single live
snapshot writer, no-clobber backup names, capacity checks before directory/file
writes, and strict last-backup sidecar updates. Receipt/error types retain
publication state. The caller passes exclusive Working's privileged collection.
Published files are retained; periodic scheduling, retention and T7 remain open.
Unopened existing imports are refused until their required safety copy exists.

Format `20261022T165836Z-12154` passed. Initial focused selection
`20261022T165904Z-12690` rejected a nonexistent test binary and ran no tests.
Corrected selection `20261022T165924Z-13282` failed compilation: a new host test
uses RawRows.len rather than RawRows.rows.len. No tests ran. Lint
`20261022T170025Z-13951` confirms that test error and one needless borrow;
the two earlier engine style errors are corrected without suppression.
Unchanged privacy scan flags only one complete own comment in backup.rs.

Independent round-1 provider review by `/root/rf2_backend_review` finds one P1:
the initial-import path checked destination absence before ownership, leaving
a window for a new file to appear without a safety backup. The fix must check
under CollectionLock before metadata/candidate mutation and prove preservation
at that boundary. No other confirmed P0–P2 finding in this component.
Fixer `s-20261022T170117Z-7921357e`, starting `c7063683`, receives the complete
preserved 22-file seed plus exactly that finding, the test/lint corrections and
the whole-comment privacy correction. No unit or full-gate acceptance is claimed.

Catch-up `20261022T164600Z-7066` passed after the backend checkpoint was clean.
Doctor `20261022T165111Z-9502` passed: VM SSH/runtime remain available and
83 GiB is free. The dedicated VM stays headless.


Provider round-2 filtered run `20261022T170129Z-14846` moves the existence
check under retained CollectionLock, before reconciliation or candidate writes.
A deterministic pre-ownership callback creates a file or dangling link under
brief cooperating ownership. Both cases refuse import with unchanged live,
sidecar and candidate bytes, no caller mutation, and unchanged generation/state.
It also corrects the exact row-count expression, needless borrow and complete
own comment. No test assertion or scanner rule was weakened.
Independent delta review closes the P1 and finds no new P0–P2 finding.

Format `20261022T170326Z-16121` passed. Focused Docker run
`20261022T170350Z-16729` passed all 89 selected tests. The one ignored check is
`meguri-host::ports::milestone_provider_owners`, intentionally run by
its dedicated milestone job. This component receipt grants no milestone acceptance.
The passing tests prove a recoverable committed-WAL-answer backup and release
of its reader before replacement, forced empty/unchanged publications, all
61 occupied names, capacity/cancellation preservation, durable-file retention
when metadata fails, and R6/unknown metadata preservation.
Unchanged privacy `20261022T170354Z-16985`, full Rust lint
`20261022T170509Z-17673` and similarity `20261022T170512Z-17905` passed.
This closes the earlier two engine style failures and unused import as well.

RF2 next: expose owned source-copy/inspection and activation jobs, connect the
native coordinated picker/preview, prove imported reviews/background/restart on
all three platforms, then run the RF2 full gate against `81c20757`.
U126/U127 and other parent units remain open; full scheduling, retention, T7 and
prepared conformance executors are not accepted by this forced-service component.


## Owned inspection and activation jobs

Entry component starts at `55258d83`; implementer
`s-20261022T170635Z-d68a2c78`, clean baseline `2bfd30f4`.
Filtered run `20261022T170751Z-20216` exports package-owned entry orchestration,
bounded coordinated-source copying, aggregate space preflight, single-use owned
inspection handles, committed-work/generation confirmation guards, real safety
backups and typed activation results. Host composition reuses the process core
for first import and carries the host with postactivation failures. Recovery
ownership persists when reopening fails. FFI uses that same registry/dispatcher
and retains the host before reporting the activation outcome.

Format `20261022T172632Z-27187` passed. Focused Docker run
`20261022T172716Z-27831` ran 198 tests: 196 passed and two failed; the one
ignored provider-owner check remains a dedicated milestone check. The failures
are a legacy FFI test still expecting unavailable backup behavior and a new
test setup uses a different file than the media service opens. Use the
production path is storage-owned `meguri-media.db`; strict postactivation,
lock/fence retention, close/reopen and actual-answer assertions must remain.

Independent round-1 entry review by `/root/rf2_backend_review` confirms one P2:
both flows disable cancellation before repeated validation/finalisation rather
than at the actual swap boundary. Check both tokens after that work, preserve a
published backup receipt on cancellation, and finish recovery after activation.
The review lists no further P0–P2 issue.

Lint `20261022T172924Z-28976` fails the large EntryError return-size check in
four entry functions. Reduce that enum's size while keeping its variants.
The unchanged privacy scanner flags own FFI module prose and own host-test
prose/local bindings. Fixer `s-20261022T173106Z-3d22b442`, starting `c1017d27`,
receives the complete 21-file seed and these findings in one filtered pass.
It must keep the original FFI refusal proof using a real backup failure, make
the new setup exercise an actual provider failure, and add deterministic
cancellation tests before and after the commit boundary. No Unit acceptance.

The prior provider checkpoint and session record were pushed to master.
Catch-up `20261022T170643Z-19574` passed. Approved Meguri-only cleanup ran with
build jobs idle, removed incremental cache only, and restored 69 to 79 GiB free.
No other project's Docker data was touched.

Entry round-2 filtered run `20261022T173127Z-30279` preserves the reviewed
component and adds a compatible precommit hook after candidate finalisation.
Both cancellation tokens are sampled there; only then is Committing published.
Eight deterministic early-cancellation cases preserve destination/generation,
and two late-cancellation cases finish activation and recovery. Callbacks can
reenter read APIs without a lifecycle mutex or reader lease held.
Independent delta review closes the P2 and reports no new P0–P2 finding.

Format `20261022T173731Z-33255` and unchanged privacy scan
`20261022T173832Z-34342` passed. Focused `20261022T173803Z-33729` ran 200 tests:
Results: 199 pass, one failure. Provider ownership is checked at its milestone.
All ten cancellation checks and the host recovery test returned PASS.
The remaining legacy FFI assertion expects a Rust variant name in a Display
message; the real error formatter uses its descriptive text. Correct that
expectation while retaining the capacity/publication/no-mutation assertions.
Lint reports only `.err().expect()` in host initial-entry handling; replace
with the direct Result method without changing behavior. Both failures remain
open until their focused checks pass. No Unit acceptance or integration gate.

Round-3 fixer `s-20261022T174235Z-5429897a`, baseline `94fa5892`, exports
`20261022T174246Z-36820`: exactly the Display prefix assertion and direct
expect_err correction, with the complete reviewed seed preserved. Independent
narrow review confirms both and finds no P0/P1. All capacity, publication and
exact preservation assertions remain enabled. Focused Docker
`20261022T174358Z-38103` passes all 200 selected tests; the separate milestone
ownership check remains outside this run. Similarity against RF2 base
`81c20757` passes (`20261022T174401Z-38370`). Privacy initially flags only own
review prose; that full sentence is rewritten without looking up the token.
Unchanged scan `20261022T174453Z-39444` passes all 24 staged files.
Doctor `20261022T174258Z-37251` passes: VM SSH and runtime ready, 68 GiB free.
Rust lint `20261022T174523Z-39956` passes with the direct Result fix. This checkpoint
integrates owned inspection/activation primitives only; native entry and the
full RF2 integration gate remain required. Parent unit requirements stay open.

## Native local entry

Native component starts `4e7ad102`; implementer `s-20261022T174625Z-b54035c3`,
clean baseline `44080ce7`. Filtered run `20261022T174640Z-41515` exports 33 files:
typed bridge outcomes, dedicated coordinated-source lifetime, entry coordinator,
shared native sheet/picker, initial-host startup integration, platform backup
paths, public source-only test setup and actual picker/review/restart journeys.
Its reported `5dab13ee` is the filtered synthetic Git baseline, not master.
Source fixtures are copies of the existing public package. No unit acceptance.

Core XCFramework `20261022T174729Z-42413` passes for Debug Mac and simulator.
Swift bridge `20261022T180426Z-49102` compiles and runs 59 tests: 58 pass;
legacy MutationServiceTests still expects backup-unavailable refusal. Preserve
its refusal and unchanged-content proof using actual scoped low capacity, then
restore capacity for the rest of that persistence test. New entry tests pass.
Unchanged privacy `20261022T180442Z-50150` flags own prose/declarations in four
new Swift files; rewrite full own regions without inspecting private tokens.
No scanner, source archive, requirement or assertion exemptions are permitted.

Independent native round-1 review by `/root/rf2_backend_review` reports no
P0–P2 findings beyond those known check corrections. It checks source scope
through terminal delivery, single-owned preview cleanup, typed active outcomes,
retained-host refresh ordering, replacement cleanup, lock-free callbacks and
lifecycle/picker/persistence test intent. This is static evidence only.
Actual VM picker journeys and full RF2 gate remain required. The new import
sheet still needs its registered visual states, a11y runs and rendered critic.
MeguriKit `20261022T180501Z-50640` compiles without diagnostics, then the new
missing-directory import test does not complete. Its AppModelTests child is
stopped after several minutes without progress; the parent exits and retains a
failed receipt with no complete test summary. This is not passing evidence.
Other bundles progressed, including the unchanged R554 limit check, but no
acceptance or R554 repair is inferred from this partial run. Trace the new test
await before further checks; preserve actual import/review/persistence coverage.
P2: the test calls startStudy without hosting its pages. It waits on
CardPagePair.prepare, whose unpresented runtime awaits two animation frames
before emitting settled. This model test provides no window. Add a display-only
surface factory for the test, with unchanged production ReviewPages default and
real core sessions, rendering, answers, records, backup and restart assertions.
Actual rendered reviews remain mandatory in the three VM picker journeys.
Native round-2 filtered run `20261022T181304Z-56370`, fixer
`s-20261022T181252Z-325b97bc`, baseline `75f40a0e`, preserves the full seed.
Only the new model test substitutes display handling; it still calls actual
Core.renderSessionHead and checks the matching card and both rendered sides.
Production defaults to ReviewPages. Independent review closes that P2 and finds
no further P0–P2. The legacy deletion test now proves a real capacity refusal
and exact content preservation before its existing persistence continuation.
Focused Swift `20261022T181615Z-57802` passes 26 tests across both bundles.
Kit `20261022T181649Z-59133` runs six tests: five pass, one fails. First import,
actual answer, background, restart and a seven-review safety backup pass.
The session-refusal setup starts its real session after inspection, so that
session's writes make the preview stale first. Inspect after session startup
to bind those writes, then keep the exact SessionActive refusal assertion.
Privacy `20261022T181634Z-58830` flags only own review prose, rewritten in full;
all product-source regions pass. No passing acceptance yet.
Doctor `20261022T181143Z-54780` passes with VM SSH/runtime ready and 68 GiB free.
Round-3 fixer `s-20261022T181802Z-207061ab`, baseline `e42ead0d`, exports
`20261022T181817Z-60838`. Only the second preview moves after real session
startup, with an added non-nil preview requirement. Exact SessionActive,
generation, session-end and close checks remain. Independent review finds
no P0/P1. Focused Kit `20261022T181938Z-61744` passes all six tests, including
real first import, rendered payload/answer, background/restart and backup proof.
Bridge/instrumentation `20261022T181615Z-57802` passes 26 tests.
Privacy `20261022T181957Z-63202` and similarity `20261022T181954Z-62930` pass.
This is a native-source checkpoint, not RF2 acceptance. VM picker journeys,
registered visual/a11y evidence and full gate against `81c20757` follow.

## Actual picker checks

Native checkpoint `84c9365b` is pushed to origin/master.
Catch-up receipt: `20261022T182037Z-63859`, PASS. The first phone run
`20261022T182044Z-64184` builds, then both cases fail at the test's local-storage
navigation step. They reach the actual system picker. The captured hierarchy
shows Recents and the system Browse tab (`DOC.browsingModeTabBar`), while the
test used immediate existence checks before the picker finished loading.
Wait for that observed system control, navigate once, then keep the same actual
file/preview/import/review/background/restart assertions. Preserve this red run.
Picker readiness fixer `s-20261022T182522Z-3b76f2fb`, filtered
`20261022T182551Z-68350`, adds a bounded wait for the observed Browse tab.
Independent review closes that source finding. Second iPhone run
`20261022T182833Z-69792` reaches Browse, local storage and Meguri, then both
cases tap the folder caption without entering it. The hierarchy retains the
parent and shows `RF2 Import Tests, Folder` with a visible image child. Target
that cell/image, keep the actual file selection and preview assertions, and
preserve the failed run. No import or persistence success is inferred yet.
The user selected a self-hosted Anki-compatible server for the expanded first
usable build; `review-first-delivery.md` records manual server import/export
and file transfers next after the local-review checkpoint. Full goal unchanged.
Picker cell fixer `s-20261022T183201Z-0e8ee227`, filtered
`20261022T183218Z-74140`, targets native folder/file images once. Privacy
`20261022T183329Z-75836` passes. Independent review closes that targeting issue.
Third iPhone run `20261022T183324Z-75019` selects the real source and shows its
verified six-note, eleven-card, six-review preview. Both tests then fail because
SwiftUI propagates the parent preview identifier onto every child, including
the controls below it. Add a native accessibility container for this group.
Actual AX frames also prove undersized Home/entry actions; move target sizing
into their actionable labels and verify in the following VM/a11y checks.
The phone result remains FAIL until both complete journeys pass.
### Preview controls and real activation

Fixer: `s-20261022T183714Z-73879246`; base: `34e1b6cc`.
Filtered output: `20261022T183732Z-79309`.
The two Swift files preserve distinct child IDs and place minimum sizing
inside button labels. Independent review reports zero P0–P2 findings.
Privacy receipt: `20261022T183906Z-81262`, PASS.
Phone receipt: `20261022T183902Z-80351`, FAIL, two cases.
The picker opens the package; facts and confirmation checks pass; activation
loads real decks. The test then clicks an offscreen duplicate after its layout
probe. Reviewer diagnosis: restore the layout, require sheet dismissal and
choose an actionable deck. Reuse that deck ID after relaunch. Keep all answer
and durability checks. This checkpoint does not accept RF2.

### Further picker verification

Base `4244d099`.
Fixer sessions: `s-20261022T184525Z-61b624a8`,
`s-20261022T184942Z-6696df7e`, `s-20261022T185243Z-019a2140`.
Exports: `20261022T184607Z-85989`, `20261022T184954Z-89384`,
`20261022T185256Z-91455`.
Review requested a generic AX query, applied-trait acknowledgments and an
exact deck-page identifier. Those changes are present; round 3 reports no
P0/P1. The test now chooses public deck `1788249600212` for normal study.
A newly added comment failed privacy scanning and was removed in full.
Product-source scan then passes. No test assertion was removed.
`ui-test/20261022T185400Z-92699`: FAIL during compilation, before VM execution.
Three notification status assertions compare UInt32 to Int32. Correct the
constant types and run the same first-import case. No acceptance claim.

### RF2 visual registration checkpoint

- Base: `3de1daab`.
- Implementer: `s-20261022T185502Z-7544e0ac`; export `20261022T185517Z-95157`.
- Review round 1: no P0–P2. Shared live/fixture presentation and Home actions.
- Design docs: `20261022T190233Z-98278` FAIL; `20261022T190311Z-98855` PASS after U166 unit metadata correction.
- Similarity: `20261022T190405Z-2280` PASS.
- Phone: `20261022T190327Z-99228` FAIL after activation, before Study.
- Actual AX: FSRS parent identifier overrides the Enable button identifier.
- Fixer: `s-20261022T190619Z-c229fb33`; export `20261022T190635Z-5342`.
- Review round 2: P2, admission uses firstMatch; use all matching hittable rows.
- Phone: `20261022T190902Z-7787` PASS, 1 test, 0 skipped. Other platforms and visual gates pending.

- Phone replacement: `20261022T191317Z-13430` PASS, 1 test, 0 skipped.
- Query fix session: `s-20261022T191237Z-c6b9deb4`; export `20261022T191304Z-13012`.
- Round 3: readiness P2 closed; no new P0/P1. Both predicates use all matching rows and retain policy dismissal checks.
- iPad, Mac, snapshots, audits and integrated gate remain pending.

### Platform checks and accessibility correction

- iPad `20261022T191544Z-16449`: FAIL, 2 cases, picker targeting.
- Probe `drive/20261022T191901Z-19587`: FAIL; picker screenshot is retained. Exported hierarchy omits provider elements; app identifiers also fail format checks.
- Mac `20261022T192232Z-23557`: FAIL at build; incorrect scroll argument label.
- Picker fixer: `s-20261022T192221Z-1e3b2410`, base `eb1538f1`, export `20261022T192241Z-24451`.
- UI fixer: `s-20261022T192618Z-51eb35e6`, base `de9b1a95`, export `20261022T192631Z-26918`.
- Review: no P0–P2 findings; production actions and test assertions remain intact.
- Actual iPad Cancel was 36pt in the toolbar. Footer measurement and current platform runs remain pending.
- Source privacy PASS; checkpoint `81cd44ab`. No unit acceptance.
- iPad first-import `20261022T192905Z-28896`: FAIL before selection; remote AX query interrupted inside the short folder wait.
- Doctor `20261022T193004Z-30943`: VM ready; 59 GiB free.

### Native text and picker readiness

- Fixers: `s-20261022T193127Z-d5555937`, `s-20261022T193359Z-a956e22c`.
- Checkpoint `d8b3dd8a`: scoped text checks accept native label or value; folder readiness uses the existing 20s bound. Independent review: no P0–P2.
- iPad first import `ui-test/20261022T194801Z-55067`: PASS, 1 test, no skips.
- Mac `ui-test/20261022T193041Z-31689` and `20261022T194503Z-52454`: FAIL at the intro check. The latter hierarchy proves the NavigationStack root identifier is absent.
- Root fix base: `06a8efca701321ce9ab76ab16781a0aca664d366`.
- Fixer `s-20261022T194939Z-a90de3f4`; export `cleanroom/20261022T195156Z-58960`.
- Independent review: no P0–P2 findings. No test assertions removed.
- iPad replacement `ui-test/20261022T195209Z-59405`: PASS, 1 test, no skips.
- Snapshot recording `20261022T193457Z-36824`: PASS; 203 selected baselines committed in `06a8efca`. Comparison and audits remain pending. This is recording evidence only.

### Mac picker and resume checks

- `ui-test/20261022T195518Z-62512`: FAIL after path entry. The content identifier and intro check pass. The native GoToWindow remains open; the test clicks a disabled Open button.
- Fixer `s-20261022T195738Z-d420578e`, base `2107c882`, export `20261022T195758Z-65945`; checkpoint `588e0623`. Independent review: no P0–P2.
- `ui-test/20261022T195930Z-67219`: FAIL after a real imported answer. Picker selection, preview, activation and review pass. The test hides the app, then reads a window before activating it.
- Fixer `s-20261022T200143Z-4d88d210`, base `588e0623`, export `20261022T200209Z-70307`; checkpoint `55996c55`. Independent review: no P0–P2. No test assertions removed.
- Doctor `20261022T195606Z-64405`: PASS; VM answers; 63 GiB free.
- Mac first import `ui-test/20261022T200319Z-71476`: PASS, 1 test, no skips. Native picker, real answer, hide/resume and restart checks pass.
- Mac replacement `ui-test/20261022T200459Z-73519`: PASS, 1 test, no skips. Both picker journeys now pass on all three platforms.
- Catch-up `20261022T200513Z-74897`: PASS.

### Contrast and large-text audit

- Visual comparison `snapshots/20261022T200637Z-75874`: PASS, 225 selected cases, 0 failures, 490 deferred entries outside the filter. Input digest `947cc48f6457f3fb77c860f006cdf3cfcd7c52dafa2b1e26f88fd26950355f6a`.
- iPhone `a11y/20261022T201355Z-88814`: FAIL, 2 tests. Import/Home button contrast fails; two fixed swipes do not reach overflow confirmation at the largest text size. Preserve the failed receipt. Error/loading variants remain unverified in this run.
- Fixer `s-20261022T201813Z-fdfd892f`, base `2a424c33`, export `20261022T201837Z-92495`, checkpoint `64a205b1`.
- Review round 2: no P0–P2 findings. Use existing neutral button tokens; keep confirmation emphasis, shortcuts, actions and guards. Bound navigation to 24 gestures and retain explicit reachability, audit and touch-target assertions. No suppressions added.
- Current audits and new visual baselines remain pending after this paint change.

### Progress accessibility frame and scroll evidence

- iPhone `a11y/20261022T202045Z-94081`: FAIL, 2 tests. All states ran. Import button colours are corrected; remaining findings are recorded below.
- Confirm: overflow/light/largest, frame `(24,2722,283,139)`. Image `2205FCC8-1691-435C-8093-308BC609F68E.png` shows the top of the sheet, with Confirm outside the window.
- Loading text: light/dark/largest, frame `(24,225,354,869)`. Images `6DE463B8-2ADF-4B69-89A1-B39F5F5ED809.png` and `2EB4E29C-596C-4B83-BE19-EE494888D014.png` show black/white text extending below the viewport and footer.
- Home name: typical/dark/largest, frame `(107,144,173,125)`. Image `E2C96D73-1490-4ADA-BB52-E7BCC20D0A93.png` shows white text on the dark surface.
- Loading progress: light/dark/default, native frame `(24,350,354,4)`, hitRegion finding. This needs a real accessible frame; no hitRegion exception is added.
- Fixer `s-20261022T202844Z-7450bf7e`, base `1a686015`; exports `20261022T202909Z-98991`, `20261022T203339Z-1758`, `20261022T203544Z-3152`. The last two change only two new reason strings after privacy scanning.
- Checkpoint `1885005e`. Round 3: no P0/P1 or confirmed required-check defect. Full audits remain enabled; three exact contrast exceptions follow R1977 and the existing scroll-edge case. Frame, value, stage, full control visibility and token checks remain required.
- Actual audits, token checks and visual evidence remain pending after this fix.
- Token checks `swift-test/20261022T203737Z-4448`: PASS, 13 tests in ContrastTests and TokenCoverageTests.
- iPhone `a11y/20261022T203812Z-5807`: FAIL, 1 passed and 1 failed. Home passes. Import fails because progress still exposes a 4pt frame; the new explicit frame assertion catches this.
- Fixer `s-20261022T204324Z-076f2a06`, base `d55891f8`, export `20261022T204339Z-10171`, checkpoint `a23e2791`. Independent review: no new P0/P1. Exactly one metadata line sets the accessibility shape on the existing 44pt wrapper.
- API: https://developer.apple.com/documentation/swiftui/contentshapekinds/accessibility

### Native progress and import sheet audit

- Input `97dc60529b232a17e8bc1f63f07f3d64547920340577a401c30a18d2ab5b85e1`: iPhone `a11y/20261022T204506Z-11473` PASS, 2 tests, no skips. All five import states and scoped Home empty/typical checks pass.
- iPad `a11y/20261022T205058Z-15571`: FAIL. Home passes. Import fails. Progress frame and value checks pass. The failed receipt retains the findings.
- Mac `a11y/20261022T205633Z-18860`: FAIL, 1 passed and 1 failed. The wrapper exposes an Other element with an empty value. The required value assertion catches it. Backdrop text also fails contrast. Home passes.
- Base `4ec4a77f`; fixer `s-20261022T210101Z-3842fce5`; exports `20261022T210123Z-21883` and `20261022T210558Z-24055`; checkpoint `0a6adb6b`.
- Independent review caught a Route/String type mismatch in the first export. The second export changes only the switch to `route?.path`. Review confirms the defect is closed, with no further P0/P1 or confirmed required-check defects.
- Native ProgressView replaces the ignored wrapper. Frame, native role, label, exact value and separate stage assertions stay enabled. Five exact sheet fixtures use a plain backdrop through TestHooks.Root; production entry and real picker journeys retain their existing root.
- Seven exact contrast reasons record the iPad offscreen cases. Existing token pair checks cover those colours. No nil finding, role or hit-target exception is added. The full audits and bounded control visibility checks remain enabled.
- Current audits and visual comparison remain pending. No acceptance claim.
- Mac `a11y/20261022T210712Z-25128`: FAIL, 1 passed and 1 failed. Progress role, frame and label pass. The String cast fails. `ui-tree/20261022T210920Z-27250` PASS records the native progress value as 0.42.
- Fixer `s-20261022T211009Z-98befd29`, base `3945a99d`, export `20261022T211026Z-29283`, checkpoint `d2945846`. Independent review: no findings. Mac requires a number equal to 0.42 within 0.000001; other platforms retain exact text. All other assertions remain enabled.
- Doctor `20261022T211036Z-29620`: PASS. VM running, SSH ready, 62 GiB free.
- Mac `a11y/20261022T211131Z-30870`: FAIL. Native progress role, frame, label and numeric value pass. The separate stage has an empty label; the captured native hierarchy records its text in value.
- Export `20261022T211328Z-32720`, same fixer session, checkpoint `7d3ec912`: Mac checks the exact stage String value; other platforms check the exact label. Independent review: no findings. No product change.
- Mac `a11y/20261022T211442Z-33905`: PASS, 2 tests, no skips. Five import states and Home empty/typical pass, with no unsuppressed findings or touch-target violations. Input `e303130bb922cbc5217b32125f3113197d22dfd3e169a125bf0bf17979a0eee7`.
- iPad `a11y/20261022T211640Z-35753`: PASS, 2 tests, no skips. Five import states at both sizes and appearances, plus scoped Home empty/typical checks. No unsuppressed findings or touch-target violations.
- iPhone `a11y/20261022T212147Z-38907`: PASS, 2 tests, no skips. The same state coverage passes, with no unsuppressed findings or touch-target violations.
- All three audits use input `e303130bb922cbc5217b32125f3113197d22dfd3e169a125bf0bf17979a0eee7`. Mac uses native text sizing; phone and iPad include the largest accessibility size. Home coverage here concerns the import control in empty/typical states; complete U152 acceptance remains open.

### Visual baseline review

- Recording `snapshots/20261022T212653Z-42308`: PASS, 225 selected cases, 490 outside the filter. Input matches the three passing audits. This is recording evidence, not comparison acceptance.
- Independent visual review finds a required artifact defect in the Mac import images: the scroll body is transparent while the footer is opaque. The real native sheet remains legible. Home is opaque; phone and iPad frames show no new P0/P1.
- Preserve this recording as WIP. Add the existing primary-background token to the shared import scroll body. Re-record affected Mac cells, verify legibility, then obtain fresh audit and visual receipts. No palette or layout redesign.
- Surface fix: starting master `168ed621`, fixer `s-20261022T213712Z-9aa429e6`, export `20261022T213739Z-58291`, checkpoint `9edc5a33`. Independent review: no new P0/P1 or required-check defect. One existing background-token modifier; no layout or action changes.
- Mac recording `snapshots/20261022T213851Z-59408`: PASS, 30 selected import cases, 195 outside the filter. Root inspected the light loading image: body is solid white and text is legible. Input `05559d8728298676125a12690ce31cb2f977c8961cca4d9f6ada6b6788e17a53`. Fresh audits and comparison remain pending.
- Independent review: no findings. The Mac images pass.
- Final source input `05559d8728298676125a12690ce31cb2f977c8961cca4d9f6ada6b6788e17a53`: Mac `a11y/20261022T214006Z-61863`, iPad `a11y/20261022T214125Z-64004`, iPhone `a11y/20261022T214624Z-67053` all PASS, each 2 tests and no skips. Five import states and scoped Home empty/typical checks pass. No unsuppressed findings or touch-target violations.
- Final comparison `snapshots/20261022T215132Z-70068`: PASS, 225 selected cases, 0 failures, 490 outside the filter. Input matches the three final passing audits. Design review and full integration gate remain pending.

### Critic result

- Critic `20261022T215823Z-82548`, session `s-20261022T215826Z-fa23f249`: WARN. Report retains 24 high, 16 medium and 2 low issues. Both screens remain unverified. The ledger retains the findings; no visual acceptance is claimed.
- Independent triage: twenty high entries concern native import-title truncation at AX4/AX5. Keep the full title and use the native inline title on iOS, then inspect those cells and obtain a fresh critic check. Full accessibility labels do not waive visible truncation. This remains open in U166.
- Four Home high entries concern absent sidebar/title/toolbar in standalone leaf previews. Production supplies the existing shell; native journeys exercise it. Record that context or render the existing shell in future evidence. Do not duplicate a sidebar inside Home content. Complete U152 remains open.
- Medium findings remain tasks for the full Home/import UI work. Keep the actual report, the passing focused checks and matching source receipts. R1985 keeps screens out of verified; the full integration gate may run without claiming complete Home or visual acceptance.
- The local review path is functional. File export and explicit server transfers remain the next delivery priority. No Unit acceptance is claimed for these partial UI units.

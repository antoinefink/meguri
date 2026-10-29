# U113 incoming runtime

Status: tests first; ordinary live application remains unimplemented.
Unit start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Phase start: `9c983bea6396099912eae574dd30e1bebe103501`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Producer `cleanroom/20261029T125003Z-59987` adds one enabled test target.
It reuses actual publication/adoption and CoreHost/Service inputs. Existing
review and note tests, and the four Q11 media tests, are unchanged.
The filtered author's header is corrected to the recorded canonical start.

Independent R1 requires two fixture repairs. Three lost-save cases try to undo
an idle edit after a barrier would have marked it UploadPending. They must use
genuinely held work, through automatic sending: P12.1 refuses Sync Now during
an active review. A negative WAL receipt assertion also needs an actual pause
of automatic flushing before the commit, followed by the worker's real flush.

Fixer `cleanroom/20261029T130758Z-66799` uses a real session answer and two
sessionless Browse edits more than 60 seconds apart, sending an eligible older
image in the background. Undo removes only the newest held edit. The remaining
row differs from the actual saved image, retaining the no-overwrite assertion.
It forwards the existing automatic-WAL pause for the durability case, retaining
all seq, native file, generation and receipt checks. It adds an ordinary-intent
restart case. The old binding must finish its tracking update before the scan,
with actual row, backup, published Service and reopened-state checks.

Independent R2 is clear of P0-P2 findings. Its narrow final check confirms that
formatting and removed comments leave executable tokens, control flow, literal
inputs and all assertions unchanged. Privacy cleanup also renames one test.

- `core-test/20261029T130226Z-64319`: original expected compile RED, 127
  missing-type/method/Point errors, zero executed cases.
- `core-format/20261029T131616Z-69741`: repaired formatting passes.
- `core-test/20261029T131630Z-70078`: repaired expected compile RED, 138
  errors, all E0432/E0599 for required future types, methods and Point values.
  Zero tests execute; this is not runtime evidence for any safety property.
- Privacy scans identify comments and one custom test name. No exception or
  guard change is made. Final `private/20261029T131841Z-72116` passes.
- Final similarity receipt is recorded below.

Required forwards expose actual Store/Runtime/Engine facts: typed intents,
inbox/refused rows, operation results, saved cursor and matching WAL receipts;
real incoming delivery/drain, interruption hooks, the cloud held run and the
existing WAL pause. Latches must observe real child work, without owning policy.
No fake baseline, tracking ACK, apply result, merge or power simulation is used.

Real power-loss infrastructure and runs remain required. The ordinary-intent
replacement case supplements the exact Q11 media/move ordering cases and the
cross-build matrix; it does not replace them. Those cases, structural/media
behavior, native integration, signed exchanges and the full batch gate remain
open. No Unit, P24 closure or working-sync acceptance is claimed.

Final similarity: `similarity/20261029T131939Z-73046`: pass.

## Shared incoming implementation WIP

Producer `cleanroom/20261029T132315Z-74623` adds shared Runtime/CoreHost
receive and apply code. It uses the engine writer, persists typed intents,
records actual operation results and waits for matching WAL coverage before
updating tracking. Child transport forwards real core operations and queries.
The source explicitly leaves major workflows incomplete.

Full production R1 requires two P1 repairs: the new persistent tables need an
atomic versioned migration that older readers reject; transitions must settle
or preserve outstanding old work before removing inbox/base facts, and include
the new tables in correctly scoped cleanup. Automatic active-review sending
still lacks the Cloud barrier, held before-images and ACK ownership wiring.
Deletion and reference handling, restart with mixed row state or an isolated
binding, test latches and complete state queries still need implementation.
The review remains open.

- `core-test/20261029T135027Z-83874`: library compilation fails with E0596
  in Runtime Drop after state changes from Mutex to Arc<Mutex>; zero executed.
- Fixer `cleanroom/20261029T135658Z-86638` repairs Drop with state-before-
  tracking locking, renames only two custom digest locals, and removes one new
  AGENTS paragraph flagged by privacy. Independent narrow review confirms all
  other seeded files are unchanged; hash inputs/order and copy fields stay.
- `core-test/20261029T140024Z-88458`: all dependencies build; three cases run.
  The child helper and full incoming note-edit/reopen test pass. The Good review
  case reaches matching actual Card/Revlog/full rows and preserves them across
  reopen, then fails at its second sync (line 110). This case remains failed.
- `core-test/20261029T140527Z-90810`: the complete incoming-safety target still
  fails compilation with 84 missing API/type/Point diagnostics; zero execute.
  Real source-process holds, faults and complete observations must be wired.
- `private/20261029T140530Z-91073`: all 18 final WIP paths pass.
- `similarity/20261029T140533Z-91370`: 18 paths, no findings or exemptions.

The review resync failure currently arrives through a generic child error.
Diagnose it with safe type-only context; never expose record bodies, private
connection values or credentials. Keep all tests enabled and repair the actual
path. Native app lifecycle/capability integration, media, signed exchanges,
power-loss and remaining Q11 matrices, and the full batch gate remain open.
This checkpoint is WIP, without Unit, P24 or delivery acceptance.

WIP formatting: `core-format/20261029T140011Z-88120`: pass.

## Safe diagnostic context

Fixer `cleanroom/20261029T141021Z-93224` changes only the test transport to
carry a finite catalog of error categories without payloads. Independent review
confirms no source error text, paths, keys or arbitrary Debug/Display values
cross IPC. Boot, normal and nested replies agree; success handling is unchanged.

- `core-format/20261029T141306Z-94409`: pass.
- `private/20261029T141327Z-95134`: pass.
- `similarity/20261029T141329Z-95382`: pass, no findings or exemptions.
- `core-test/20261029T141324Z-94874`: the single failed review case remains
  failed at line 110, now with `RuntimeIncompleteWholeBindingReconciliation`.

The bounded source trace locates registration before startup reconciliation:
`sync_now_and_drain` calls `ensure_sdk` before the Idle scan path. Reopen correctly
requires a new scan, so `register_sdk` refuses it. No fetch has begun; no token
or seen-set change is justified. Drive the actual scan first; keep the guard.

## Startup reconciliation, root confirmation and held explicit requests

Phase base `63f8528618482ab51a1211946a1b086a9c09ba07`; original U113 start
and batch base remain unchanged. Fixer exports
`cleanroom/20261029T142153Z-98510` and
`cleanroom/20261029T142910Z-3093` change only the publication driver and its
additive regression. Existing review and note-edit tests remain unchanged.

The first repair reaches the next real prerequisite: after reconciliation,
explicit sync still precedes root confirmation. R1 also finds a P2: a request
paused before scanning is lost when released through `drive_ready` with
automatic scheduling disabled. The final repair shares actual reconciliation,
registration and root reading with Idle, retaining one explicit request until
the real Sync Now accepts it. Resumed driving preserves the shared step budget
and collection-drain/cleanup tail. No product guard or readiness flag changes.
Independent R2 is clear of P0-P2 findings for this bounded change.

- `core-format/20261029T142718Z-912`: environment error, Docker socket not
  reachable inside the sandbox. Host-authorized MG access resolves placement;
  `doctor/20261029T142736Z-1445` confirms OrbStack and the dedicated VM answer.
- `core-format/20261029T142744Z-1954`: first repair formatting passes.
- `core-test/20261029T142756Z-2274`: four pass; review resync fails with
  `RuntimeIncompleteRootConfirmationAndSending`.
- `core-test/20261029T143218Z-4701`: the new hold/release regression fails
  against the first repair because no collection fetch follows release.
- `core-format/20261029T143254Z-5135`: final repair formatting passes.
- `private/20261029T143323Z-5916`: both final paths pass.
- `similarity/20261029T143322Z-5905`: both final paths pass.
- `core-test/20261029T143313Z-5614`: broader compilation remains blocked
  by the existing missing `fail_held_io` driver method; zero tests execute.
- `core-test/20261029T143347Z-6577`: 19 run, 18 pass, one fails. All four
  incoming-review target cases pass, including actual Good review Card/Revlog
  rows, entropy-free reopen, duplicate delivery, note edits and held explicit
  sync. Publication-driver, cleanup and empty-media adoption checks pass.
  Nonempty-media publication/adoption fails with `RuntimePublication`.
- `core-test/20261029T143423Z-7016`: restoring the pre-fix driver reproduces
  that same media failure at the same assertion. The final driver is restored
  byte-for-byte afterward. This establishes an existing gap, not a passing
  media check or an exemption.

The bounded startup findings are resolved. All other R1 findings, remaining
Q11 cases, media handling, app wiring, signed device evidence and the full
batch gate still await work. No Unit or P24 acceptance is claimed.

## Versioned tracking migration: tests first

Phase start `3644983a`; U113 and the original batch base remain unchanged.
Implementer export `cleanroom/20261029T143126Z-4023` was prepared from
`63f85286`; its two owned paths had no intervening changes. Tests only.
Historical fixtures use the actual V1/V2 table definitions without today's
opener, preserving binding, inbox, engine serialization and conflict facts.
The prior V1 check moves to this stronger fixture. Current-format expectations
are version 3; existing future-format tests advance to 4, keeping their
original-byte and sidecar checks.

Independent R1 is clear of P0-P2 findings. Atomic migration is checked through
a real second-table DDL failure. Four current-format damage cases require
private-copy probing to preserve every original main/WAL/SHM byte.

- `core-format/20261029T143632Z-8118`: pass.
- `core-test/20261029T143700Z-8517`: 76 execute, 67 pass and nine fail as
  expected before implementation. Three find persisted version 2 instead of 3;
  one detects leaked DDL after migration fails; four receive NewerSchema rather
  than Damaged for the explicit version-3 corrupt fixtures; one finds version 2
  still in the main file before the future-version WAL probe.
- `private/20261029T143739Z-8970`: both owned paths pass.
- `similarity/20261029T143741Z-9214`: both owned paths pass.

Repair remains required. No acceptance is claimed.

## Tracking format 3 repair

Phase start `f50cd7a9`. Fixer export
`cleanroom/20261029T143942Z-10277` changes three production files and updates
the runtime-store comments. All tests from the preceding commit stay unchanged.
Creation and V1/V2 migration now install the incoming tables, set version 3 and
validate in one transaction. Current-format tables and typed rows are checked
in the private WAL-aware probe before the original opens. Existing valid WIP
rows remain; new intents store their assigned row id in the same transaction.
Pending legacy id-zero rows remain readable. Independent R1 is clear of P0-P2.

- `core-format/20261029T144350Z-12585`: pass.
- `core-test/20261029T144358Z-12890`: 82 pass, zero skipped. Covers the full
  76-case schema/recovery selection, incoming reviews, held startup and actual
  empty-media adoption. All nine pre-implementation failures are repaired.
- `private/20261029T144417Z-13224`: four owned files pass.
- `similarity/20261029T144419Z-13473`: four owned files pass.
- `core-test/20261029T144208Z-11617`: the actual prior reader from commit
  `63f85286`, copied under `build/compat/cloud-tracking-v2-63f85286` without Git
  metadata, passes its existing format-3 WAL refusal test. One selected test
  executes; 22 are outside the filter. This proves that reader's version guard,
  not the complete cross-build exchange matrix or the signed CloudKit lane.

The optional follow-up from review is a dedicated migration case with valid
pending legacy WIP rows. No source defect is reported for that branch.
Other R1 findings, media, native wiring, signed runs and the batch gate remain
open. No Unit or P24 acceptance is claimed.

## Recovery-transition regressions before implementation

Phase start `a5081a8b`. Test export `cleanroom/20261029T144103Z-11001`
uses actual CoreHost and Service operations to leave a real ordinary intent
after COMMIT, with its seq and original inbox entry. The same-file/account
binding is stopped, then a new publication must complete with exact old ACK,
WAL, no-repeat-apply, cleanup and reopen checks.

The first assertion was too early: V17.6 requires finishing old work before the
new scan, while V2.4 step 8 authorizes an isolated old owner once a recovery row
names it. `core-test/20261029T144658Z-14978` reaches that earlier assertion but
does not prove the required timing. Corrected export
`cleanroom/20261029T144959Z-16546` retains all data assertions after actual
reconciliation and before sending. Independent R2 is clear for that correction.
`core-test/20261029T145229Z-17951` then fails because the old intent still exists
after actual reconciliation. The full existing BeforeScan latch case remains
required to establish exact internal ordering, beyond this return boundary.

`private/20261029T145241Z-18279` flags two comments and a diagnostic.
Fixer export `cleanroom/20261029T145403Z-19308` removes those comments and
changes only the diagnostic text, while extracting the same Service-save code.
It adds a second regression: V15.2 step 1 excludes queued, unapplied downloads
from the replacement. Confirmation, publication and reopen must succeed with
the original row, no incoming apply, no old-zone send and final-only cleanup.
The Preview API has no queued-work count, so this case cannot prove that UI/API
requirement; it checks the actual inbox alongside the actual preview.

Independent R3 reports no P0/P1. One P2 fixture mismatch remains in the second
case's final tail: after proving the old binding was deleted, its query APIs
return `Tracking(BindingNotFound)`, rather than empty/None. The next fixer must
assert that exact existing error, keeping all pre-deletion evidence checks.
Production query semantics must stay unchanged.

- `core-format/20261029T145813Z-20966`: pass.
- `core-test/20261029T145820Z-21270`: 17 execute, 15 pass and the two new
  cases fail before that tail: an old intent remains after the scan, and a
  queued download changes the local row during confirmation.
- `private/20261029T145831Z-21593`: pass.
- `similarity/20261029T145833Z-21841`: pass.

The next source repair must finish existing intents under the recorded owner
without applying queued old downloads or refusing the required publication.
Retained-copy, adoption, account/file and full latch cases remain required.
No acceptance is claimed.

## Corrected noncrash Stop test

Phase start `856623d0`. Root catches a fault-mode mistake in the prior fixture:
F3 permits error modes only at tracking-commit points, so IoError at
`cloud.apply.after_commit` is not valid fault-mode evidence. The earlier review
approval was wrong on that point. Retain those receipts as history; they prove
no F3/F5 mode pass.

Fixer `cleanroom/20261029T150926Z-26005` changes only the stopped-binding test.
The existing Callback handshake pauses the real worker after COMMIT, outside
writer/state/tracking locks. The parent calls actual Stop, then releases and
joins the worker before asserting `StaleIncarnation`. Both waits are bounded;
the local guard disarms/releases on normal exit and unwind. All later row,
intent, inbox, ACK, WAL, publication, cleanup and reopen assertions remain.
Independent review is clear of P0-P2 for this narrow correction.

- `core-format/20261029T151208Z-27221`: pass.
- `core-test/20261029T151226Z-27569`: one selected case runs and fails at
  the actual missing old-intent recovery after the scan. The real Stop and
  stale-authority checks pass first. Sixteen cases are outside this filter.
- `private/20261029T151237Z-27893`: pass.
- `similarity/20261029T151238Z-28134`: pass.

This is noncrash concurrency evidence only. Required crash/power modes, exact
BeforeScan ordering and all other pending sync checks remain open.

## Scoped old-owner recovery and publication exclusions

Implementation starts at `856623d0`; integration follows the corrected test
commit `4ff99be1`. Original U113 and batch bases remain unchanged. Fixer export
`cleanroom/20261029T150052Z-22856` changes six production files and only the
five reviewed post-deletion test expectations to `Tracking(BindingNotFound)`.
No production query semantics or other test assertions change.

Confirmation waits for running work without applying queued downloads. An
existing same-scope recovery row grants the old owner permission to finish its
intents, without selecting it or making it usable. Account, file, publication,
open and incarnation checks remain. Actual snapshot WAL coverage precedes ACK
and intent removal, outside the writer and other guards. Cleanup refuses live
intents and removes refused rows in the final transaction; conflict owners stay.
Independent R1 is clear of P0-P2 for the bounded live-transition repair.

- `core-test/20261029T151517Z-29599`: 117 pass, zero skipped. Includes
  both transition cases, host publication/adoption/backup/SDK guards, complete
  selected schema recovery, incoming reviews and empty-media adoption.
- `private/20261029T151529Z-30158`: one diagnostic string is flagged.
- `similarity/20261029T151527Z-29896`: all seven paths pass.
- Fixer `cleanroom/20261029T151732Z-31037` changes only that Busy diagnostic
  to existing text. Independent comparison confirms the other six files and
  all operands, conditions and tests are identical.
- `core-format/20261029T151945Z-32499`: pass.
- `core-test/20261029T152005Z-32871`: final 30 host cases pass, zero skipped,
  after the diagnostic-only change. The 117-case result above remains the
  broader pre-wording-change evidence, not a new full run on identical bytes.
- `private/20261029T152014Z-33170`: all seven final paths pass.
- `similarity/20261029T152016Z-33444`: the one changed file passes; the
  other six retain their preceding passing result.

Still required: exact BeforeScan/restart ordering with old intents after the
new decision, retained conflict owners, all adoption/account/file variants,
the full incoming safety target, media, native app integration, signed runs and
the original batch gate. This is no Unit or P24 acceptance.

## Retained conflict owner: required coverage

Phase start `e01d8fca`. Implementer export
`cleanroom/20261029T152825Z-38754` adds one host case, with all prior tests and
helpers unchanged. A native local edit and a conditional other-writer Service
save produce an actual field conflict. Strict K16 checks require the copy-time
names and deck, then publication cleanup must retain that copy and its isolated
owner through clean reopen while removing only work rows.

Independent R1 is clear of P0-P2 findings. Generated note times are backdated,
so the native edit's newer timestamp is a valid, deterministic precondition.
Direct Store reads occur only after Host/Collection close and drop, inside the
tracking gate. No conflict row, ACK, baseline or outcome is seeded.

- `core-test/20261029T153508Z-44380`: the one selected case compiles and
  reaches the actual field-name assertion: None instead of the generated note
  type's two names. Native edit, conditional save, merge, actual losing copy,
  digests, envelope and clean-reopen inspection run before this failure.
  Cleanup and subsequent reopen assertions remain unexecuted.
- `core-format/20261029T153458Z-44051`: pass.
- `private/20261029T153522Z-44744`: pass.
- `similarity/20261029T153523Z-44993`: pass.

Keep the assertions. The next source change must add the missing metadata.
No acceptance is claimed.

## Retained conflict metadata repair

Repair start: `78dbd2c7`. Fixer export
`cleanroom/20261029T154945Z-53451` captures names and deck placement through
typed reads in the current writer transaction, before replacing rows.
The first card by ID supplies the deck. Existing copies, IDs, payloads,
digests, winners and deduplication remain unchanged. Independent R1 found
no P0-P2 issue in this scope.

- `core-format/20261029T155305Z-55564`: pass.
- `core-test/20261029T155317Z-55914`: retained-conflict case passes, including
  cleanup and both clean reopens; 17 unrelated cases excluded.
- `core-test/20261029T155432Z-57727`: all 18 publication cases pass,
  zero skipped.
- `private/20261029T155401Z-56572`: pass.
- `similarity/20261029T155401Z-56761`: pass.

This closes the observed missing-metadata failure. Incoming safety, media,
native integration and the original full batch gate remain open.

## Incoming worker evidence and reply-retention regression

Harness integration start: `3395fa4c`; the original unit and batch bases
remain unchanged. Source exports `cleanroom/20261029T152542Z-37511` and
`cleanroom/20261029T154847Z-52428` add actual worker pauses, typed
observations, fixed tracking faults and native writer interruptions.
No existing incoming test is changed. Independent review confirmed the
final mechanical corrections and three real reply-retention regressions.

- `core-format/20261029T155459Z-58332`: pass.
- `private/20261029T155526Z-58983`: pass for all 14 source paths.
- `core-test/20261029T155506Z-58640`: eight tests execute; five pass and
  three fail. All review-sync cases pass. The new count, Cancel and Error
  cases reach the intended failure: a later automatic Build overwrites the
  first operation's unconsumed reply. Subsequent calls return Completed(0).
- `similarity/20261029T155543Z-59349`: all 14 source paths pass.
- `core-test/20261029T155552Z-59744`: all 15 cases compile and execute;
  the child entry passes and all 14 safety cases fail. Failures include
  identity recovery, held review publication, tracking validation, writer
  interruption and observation during pauses. These are recorded failures,
  not accepted coverage. Diagnose with focused cases before another full run.

The single reply slot remains deliberately unfixed in this test-first
checkpoint. The next repair must retain each actual reply until its caller
consumes it once, without rerunning the completed operation. No incoming
safety, crash-mode or full-sync acceptance is claimed here.

## Retain every completed worker reply

Repair start: `80f47ea2`. Fixer export
`cleanroom/20261029T155704Z-60629` replaces the single completed-reply slot
with retained actual replies. Lookup consumes only the oldest matching
input once, including its original error. Independent R1 found no P0-P2
issue. Existing tests remain unchanged.

- `core-format/20261029T155835Z-62449`: pass.
- `core-test/20261029T155843Z-62756`: all eight cases pass, zero skipped.
  The three preceding count, Cancel and Error failures now pass, alongside
  review persistence and explicit-sync tests.
- `private/20261029T155849Z-63042`: pass.
- `similarity/20261029T155850Z-63272`: pass.

The broader receive-safety failures remain open. This repair proves reply
retention in the test driver, not native app integration or full sync.

## Track new ordinary notes and cards

Integration start: `713498fd`; source export
`cleanroom/20261029T155814Z-61837` was prepared from `80f47ea2` and applies
without overlapping the reply repair. Commit capture accepts actual new
Note/Card insertions. Reconcile can recover those additions with a random
origin, pending revision and no fabricated acknowledgement or base.
Both paths share initialization. Incoming protection and structural gates
remain. Independent R1 found no P0-P2 source issue.

- `core-format/20261029T160116Z-65801`: pass.
- `private/20261029T160127Z-66453`: pass.
- `similarity/20261029T160131Z-66698`: pass.
- `core-test/20261029T160124Z-66147`: six cases execute; five pass.
  Parent/child delivery now passes, with all four review cases. The 502-entry
  case passes creation, send, bounded apply and once-only entry checks, then
  fails its final whole-collection row comparison. That mismatch remains open.

This is a WIP repair, not acceptance of the full batching case or unit.

## Wait for identity after a failed initial account read

Phase start: `713498fd`; final integration follows `b2bd47cc` without
overlapping its files. Exports `cleanroom/20261029T155737Z-61172` and
`cleanroom/20261029T160109Z-65462` distinguish an initial failed account read
from a confirmed mismatch. A completed adoption keeps its existing decision
and waits for actual identity verification. Unfinished adoption and known
account changes keep their existing protections. Fresh replacement account
input resets the old read-failure flag. Independent R2 has no P0-P2 finding.

- `core-test/20261029T160030Z-65007`: 14 pass, one fails. All adoption
  guards and rollover pass; fetch-crash progresses past its identity failure
  to the separate missing BeforeSave pause.
- `core-format/20261029T160238Z-68934`: pass.
- `core-test/20261029T160250Z-69403`: final 16 selected tests pass; 53 are
  outside the filter. Includes native-proof and account-change safeguards.
- `private/20261029T160255Z-69898`: pass.
- `similarity/20261029T160253Z-69667`: pass.

No existing Pending decision is cleared and no recorded account is substituted
for a live read. Remaining receive, media and native integration work is open.

## Flush incoming work and record confirmation before transition

Phase start: `713498fd`; final integration follows `4153c2fd`. Exports
`cleanroom/20261029T155918Z-63837` and `cleanroom/20261029T160339Z-70974`
request a real generation-bound writer flush before checking incoming WAL
coverage. The snapshot and caller locks are released first. Flush and
coverage share the deadline. Confirmation records authorization without
waiting for the paused worker; Bind still waits and settles old intents.
Independent R2 has no P0-P2 finding.

- `core-format/20261029T160446Z-72245`: pass.
- `private/20261029T160512Z-73191`: pass.
- `similarity/20261029T160511Z-72960`: pass.
- `core-test/20261029T160458Z-72625`: 27 pass, one fails; 13 excluded.
  Actual WAL coverage, all 18 publication cases and all eight reply/review
  cases pass. The restarted-publication case passes its former blocked
  confirmation and reaches the later intent-ID assertion: two instead of
  one. The run-count assertion itself passes. Preserve the failure and
  investigate before accepting that case.

This WIP commit does not claim complete incoming-safety verification.

## Complete the batch fixture's actual input

Start: `106e6a50`. Independent diagnosis traced the whole-collection mismatch
to Config `nextPos`: native note creation changes it, but the fixture
delivered only Note/Card records. Export `cleanroom/20261029T160338Z-70753`
preserves every 502-entry, exact-once and per-operation limit assertion.
It first proves exact non-Config equality, then delivers the actual differing
Config records from the existing Service save. Final unfiltered equality and
clean close remain. Independent R1 found no P0-P2 issue.

- `core-format/20261029T160625Z-74560`: pass.
- `core-test/20261029T160634Z-74914`: the full corrected batch case passes;
  14 other cases are outside this filter.
- `private/20261029T160637Z-75251`: pass.
- `similarity/20261029T160638Z-75474`: pass.

This closes the batch-fixture mismatch without changing product counters or
weakening the collection comparison. Other incoming failures remain open.

### Recovery diagnostic correction

`core-test/20261029T160829Z-77879` confirms one apply run. It fails only
because replay uses intent ID 2 while the test expects original ID 1.
The earlier interpretation as two apply operations was incorrect.
The temporary data-free diagnostic from `cleanroom/20261029T160718Z-76209`
is preserved under build intake and removed from source. No safety assertion
was weakened. Confirm the required intent identity before making a repair.

## Match recovered input rather than an internal attempt ID

Start: `72051dbc`. Independent review of K13.2 and V5 confirms an
all-before replay creates a new apply attempt and can atomically replace
its SQL intent ID. The required invariant is its original input set.
Export `cleanroom/20261029T160918Z-78586` replaces only that assertion with
exact inbox-ID equality. One-run, changed-key, WAL, ACK, backup, publication
and reopen assertions remain. All-after checks remain unchanged. R1 is clear.

- `core-format/20261029T161004Z-79320`: pass.
- `core-test/20261029T161012Z-79631`: full selected recovery case passes;
  14 other cases excluded.
- `private/20261029T161017Z-79901`: pass.
- `similarity/20261029T161017Z-80018`: pass.

This closes that recovery-case failure, not the remaining incoming suite.

## Integrated incoming checkpoint

At `88fa0a75`, `core-test/20261029T161031Z-80591` runs all three incoming
targets: 23 execute, 15 pass, eight fail, zero skipped. This includes the
three child entry helpers. Recovery, WAL coverage, parent/child delivery,
the large batch, ordinary review persistence and retained replies pass.

Open failures remain enabled: interruptions/retry, failed inbox commit,
revision ordering under an overlapping apply, own-landed save handling,
deletion side-effect base capture, two held-review send cases and the
remaining fetch-crash case. The original full batch base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`; no Unit or P24 acceptance is claimed.

## Receive while paused and finish terminal callbacks

Start: `c938c195`. Export `cleanroom/20261029T161146Z-81591` adds separate
receive-only pause ownership while preserving the held apply and its reply.
Only an actual WorkPaused retains the SDK page. Terminal callback outcomes
reach Runtime; original asynchronous errors remain available once through
the driver. Automatic driving respects the real stopped state.

Independent R1 found no new P0/P1. P2 remains: Runtime maps Failed to
Cancelled when SDK authority is invalidated. Fault coverage remains open.
Tests and limits are unchanged.

- `core-format/20261029T162309Z-87130`: pass.
- `private/20261029T162329Z-88172`: pass.
- `similarity/20261029T162327Z-87931`: pass.
- `core-test/20261029T162324Z-87663`: 10 pass, one fails; 12 excluded.
  Revision ordering, all interruption cases and all reply/review cases pass.
  Failed-inbox now passes stop, cursor and no-timer-retry assertions, but its
  post-restart note comparison fails with automatic sync disabled. The
  required initial-fetch behavior needs diagnosis before acceptance.

This is a WIP checkpoint; native delivery and full incoming safety remain open.

## Use explicit fetches in manual-mode restart fixtures

Start: `a6dd7205`. P10.1 and V9.1 do not fetch on identity verification
when automatic sync is false. Exports `cleanroom/20261029T162452Z-89260`
and `cleanroom/20261029T162601Z-90183` add explicit SyncNow calls in three
restart cases. Shared helpers, limits and every existing assertion remain.
Durable-Ready gate-opening cases are unchanged. Independent R1 is clear.

- `core-format/20261029T162701Z-90935`: pass.
- `private/20261029T162721Z-91578`: pass.
- `similarity/20261029T162723Z-91819`: pass.
- `core-test/20261029T162719Z-91320`: nine pass, two fail; 12 excluded.
  Failed-inbox recovery and all reply/review cases pass. Own-landed-save
  passes its first action and reaches the existing held-review send gap in
  the undo action. Fetch-crash reaches RuntimeTracking(InvalidData) during
  later identity/recovery work. Both remain enabled and unaccepted.

The correction adds required user intent to the fixtures; it grants no
automatic fetch permission to the product.

## Cloud review capture: resumed implementation

Phase start: `2cafb4360ed6ef8bff5cda14f5c55b694388fab8`; original unit and
batch bases remain unchanged. Run `cleanroom/20261029T162917Z-93071` stopped
on Antoine's pause request. Its permitted source was preserved and recovered.
Resume export `cleanroom/20261029T164715Z-1120` adds a separate Cloud reader
barrier, lazy held-image projection, per-image acknowledgement evidence and
detached-session retention. Explicit SyncNow remains blocked during review.
Mechanical FFI and test callers pass Collection to acknowledgement handling.
Local reconciliation now retains queued Ready inbox entries for real apply
before sending; old-owner publication exclusion remains.

`core-test/20261029T165533Z-4895` found one E0502 borrow error. Fixer export
`cleanroom/20261029T165848Z-7335` corrects only that borrow order, removes
two flagged comment lines and adds the two real authority regressions below.
Existing assertions remain unchanged. Independent comparison confirms the
other 13 seed files are identical.

## Review findings and real authority failures

R1 found two P1 cases: a binding change and a rejected capture leave the
same step UploadPending after real ACKs. The Engine must retain the original
row-key authority for both. Tests use native edits, Stop, publication and
Service acknowledgements.

Both tests initially stopped at an earlier publication regression. Existing
publication also failed in `core-test/20261029T170606Z-12177`; Cloud reader
capture passed. A transient parent-ACK wait was stored as a persistent held
deferral. `cleanroom/20261029T170831Z-13467` separates those states. Independent
review confirms only the builder changes; held protection and tests remain.

`core-test/20261029T171129Z-15581`: publication and reader checks pass. Both
new authority cases reach the intended final failure: the same native step
remains UploadPending instead of AlreadyUploaded after real new-binding ACKs.
This is the required RED evidence before the authority repair.

## Current Cloud review verification

- `core-format/20261029T171119Z-15242`: pass.
- `core-test/20261029T171219Z-16080`: 29 execute, 26 pass, three fail;
  31 excluded. All Cloud undo tests, partial acknowledgement, detached-session,
  inbox restart and reply/review cases pass. Remaining workflow failures:
  another-writer identity equality, own-landed stale incarnation, and a
  `remote_built_on_a_lost_save`. These cases remain red.
- `core-test/20261029T171434Z-17921`: all 78 FFI library tests pass.
- `private/20261029T171250Z-16496`: all 17 paths pass.
- `similarity/20261029T171307Z-16987`: all 17 paths pass.

This is a compiled WIP checkpoint with the two P1 regressions enabled.
No Unit, P24, milestone or full-batch acceptance is claimed. Implementation
session is `s-20261025T134357Z-235af9a2`; source corrections and additive
regressions use fixer session `s-20261025T162405Z-9dc656e4`.

## Distinct device streams and manual fetch diagnostics

Start: `52302641`. Export `cleanroom/20261029T172014Z-22892` gives the
two fixture devices distinct random streams and asserts distinct bindings.
It adds the required explicit fetch after a manual-mode restart and fixed
phase labels to existing errors. Every original merge assertion remains.
Independent review is clear for this fixture-only correction.

- `core-format/20261029T172330Z-25133`: pass.
- `private/20261029T172410Z-25971`: pass for all three source paths.
- `similarity/20261029T172412Z-26227`: pass for all three source paths.
- `core-test/20261029T172408Z-25711`: 13 execute, ten pass, three fail.
  The distinct-binding assertion passes. Another-writer handling still fails
  at its apply interruption. Own-landed and lost-save cases report stale
  incarnation; own-landed identifies `submit_explicit_sync` after undo.

This preserves the failed checks for the next product repair. The authority
fix exported by `cleanroom/20261029T172016Z-23144` is separate and awaits
independent review and integration. No acceptance is claimed.

## Retain pending row authority across binding changes

Start: `536414e3`. Export `cleanroom/20261029T172016Z-23144` repairs both
Pending cases below. Original unit and batch bases remain unchanged.

Independent R2 is clear: both P1 cases are repaired, including the rejected
capture handoff. Composite notetype rows, folded tags, partial ACKs and newer
tracked edits retain their checks. Existing tests are unchanged.

- `core-format/20261029T173036Z-28996`: pass.
- `core-test/20261029T173055Z-29877`: all 20 selected tests pass, including
  both previously red native-step regressions, partial ACK, initial
  publication, all 15 Engine Cloud undo cases and the Cloud reader check.
- `similarity/20261029T173059Z-30143`: all six paths pass.
- `private/20261029T173100Z-30371`: flags only a new nonessential comment.
  Export `cleanroom/20261029T173202Z-31298` deletes that entire comment.
  Exact patch comparison confirms every other source byte is unchanged.

The three incoming workflow failures remain open; full U113/U114 acceptance,
P24 closure and the original batch gate are still pending.

## Scope sequence ordering to the collection opening

Start: `6b136035`. Export `cleanroom/20261029T173457Z-33915` checks the
actual opening before comparing persisted observation sequences with the
current snapshot. It preserves caller, SDK, generation and sequence checks.
Another-writer's manual-mode fixture now explicitly fetches after reopening.
Independent review is clear. Existing workflow assertions are unchanged.

Two new real host tests cover old-opening selection after close/reopen and
a same-opening edit after Cloud capture. The first retains actual observation
stamps through reconciliation. The second defers the old batch, then sends
the new payload. Deterministic same-opening Sync Now rejection after pinning
remains a coverage limit; no new hook or fabricated tracking fact was added.

- `core-format/20261029T173932Z-36381`: pass.
- `private/20261029T173954Z-37007`: all three paths pass.
- `similarity/20261029T173955Z-37241`: all three paths pass.
- `core-test/20261029T173951Z-36740`: 30 execute, 28 pass, two fail.
  Both new host tests, partial ACK, republication and Stop-gap cases pass.
  Lost-save merging and another-writer apply now pass. Own-landed progresses
  beyond stale-incarnation failure but fails an existing intent-key count.
  Deleted-card side-effect handling still fails reconciliation.

Keep both failures enabled. No Unit, P24 or full-batch acceptance is claimed.

## Identify the extra own-save intent entry

Start: `73733d7a`. Diagnostic export `cleanroom/20261029T174309Z-38940`
keeps the exact assertion and adds fixed metadata only. Independent review
is clear: no names, payloads, digests or system fields are printed.

`core-test/20261029T174504Z-40307` fails at the same count in action `undo`.
Both entries are actual successful saves from the submitted batch: Config
and the target Note. Both are Entry keys, with unchanged before/after digests
and own-flight evidence. The preceding row, mtime, undo and event checks pass.
The fixture assumed a single record after submitting the complete batch.

- `private/20261029T174508Z-40797`: pass.
- `similarity/20261029T174507Z-40570`: pass.

Next check the exact successful batch membership and each no-op entry, keeping
the target Note's exact digest assertions. No production grouping change or
acceptance follows from this diagnostic run.

## Check every actual own-save batch member

Start: `8647b7f4`. Export `cleanroom/20261029T174616Z-41625` checks exact
successful-save names and cardinality, each Entry's unchanged digest and
own-flight proof, and the target Note's exact digests. Independent review is
clear. All surrounding assertions and production behavior remain unchanged.

`core-test/20261029T174936Z-45064` passes the real FFI open/provider test.
Own-save now passes its batch checks in the first two actions and reaches
`WorkPaused` in the edit action's explicit second build, before reopen.
That controlled-build handling still needs diagnosis; the case stays red.

- `core-format/20261029T174840Z-44115`: pass.
- `private`: `/tmp/meguri-cloud-noop-final-private.log` retains the receipt.
- `similarity/20261029T174939Z-45325`: pass.

No Unit or integration acceptance is claimed.

## Resume the original build before starting another

Start: `3945833c`. Export `cleanroom/20261029T175424Z-49549` releases
only the child build latch, consumes its original keyed reply once without
transport, and compares its envelope with the actual landed save. A fresh
build then proves the current payload and a newer persisted flight before
the child dies. Independent review is clear. Production is unchanged.

- `core-format/20261029T175636Z-50707`: pass.
- `private/20261029T175656Z-51306`: both paths pass.
- `similarity/20261029T175657Z-51506`: both paths pass.
- `core-test/20261029T175653Z-51044`: 30 execute, 29 pass, one fails.
  All three own-save actions now pass with exact batch membership and every
  prior no-op, flight, row, undo and WAL assertion. Restart and review tests
  pass. Deleted-card side-effect reconciliation remains red and enabled.

No Unit, P24 or original batch acceptance is claimed.

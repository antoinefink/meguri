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

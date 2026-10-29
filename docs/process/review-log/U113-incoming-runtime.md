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

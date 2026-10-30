# Native iCloud integration checkpoint

Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
U114 continuation base: `babb20ab`. Fixer session:
`s-20261025T162405Z-9dc656e4`. Status: WIP, no Unit closure.

Prepared native execution, media and settings patches were retained and
integrated from filtered source runs. Their source reviews covered plan
conformance, correctness and the clean-room boundary. UI review also covered
native controls. Source review does not establish runtime or visual acceptance.
Transient handoffs remain under `build/unit-intake/U114-native-final-api/handoffs/`
with a SHA-256 manifest; this log records integration evidence.

## Repairs and review

- Original SDK work owns the public asynchronous CloudKit call and its drain.
  Task creation alone is not operation completion or acceptance.
- Local metadata recovery retains binding and lifecycle checks without applying
  the network media switch to local recovery.
- Resume holds callbacks until Core validates the current account response.
  The policy update uses that response without another provider call.
- A zero-progress apply preserves waiting media and evidence, then waits for
  an external trigger. Positive progress keeps its continuation.
- Settings retain unavailable platform configuration across collection
  replacement. Status rows share one native group. Search identifiers attach
  to native controls without changing their appearance.
- Independent source reviews found no remaining P0/P1 in these repairs.
  The missing canonical unknown-account string was registered in `93028713`.

## Verification

All entries below are MG run IDs. Earlier failures remain retained.

- `core-test/20261029T231617Z-58539`: six no-collection authority checks pass.
- `swift-test/20261030T003414Z-3565`: the native partial-error callback check
  passes with its exact delay assertion.
- `core-test/20261030T004846Z-14620`: all four media regressions pass, including
  the two cases that previously stalled and the existing-file variant.
- `core-test/20261030T005821Z-28582`: all 171 FFI/Host checks pass. The explicit
  apply fixtures use real guarded inbox writes; the normal FFI path still
  applies fetched records. Timer cancellation runs through native admission.
- `core-lint/20261030T005944Z-29471`: pass, including all-target compilation.
  Operation facts use one FFI record with unchanged values and validation.
- `swift-test/20261030T005312Z-20105`: the original startup check passes with
  Connected established before Host open and exactly one policy-issued Start.
  Production LocalOnly behavior and pure adapter defaults are preserved.
- `build-app/20261030T005632Z-24992`: standalone Mac Debug build passes.
- `swift-test/20261030T010112Z-31426`: 18 pass, seven fail. Six checks assume
  exchange IDs start at one; the actual ID is five. The suspended callback
  check adds a Deck while reading the account. New Deck capture is unsupported:
  tracking stops, invalidating the original read. U113 remains open.
- `core-test/20261030T010022Z-30691`: 23 pass, three fail. The server comparison
  omits MediaName from its expected set. Two file-replacement reopen cases
  return Engine(Other); the exact cause still needs a typed diagnosis.
- `similarity/20261030T005703Z-26740`: pass for changes since `babb20ab`.
  `similarity/20261030T010217Z-33424`: pass for the final five changed files.
- `drive/20261030T010345Z-34191`: both minimum and default settings captures
  were inspected by the integrator and an independent reviewer. No further
  visible layout findings. Hierarchy still fails the Search child button ID;
  the search field and Form IDs pass. No guard exception is permitted.

Final source follow-ups `005449Z-22900`, `005623Z-24589` and `005809Z-28152`
were independently reviewed: no remaining source P0–P2 findings. The runtime
failures and UI identifier above remain open. No check is treated as passing
because source review passed.

## Follow-up from c716d9dd

Starting commit: `c716d9dd505dac49de59e5f566a7cba43068c7ca`.
The original batch base is unchanged. Filtered fixes used session
`s-20261025T162405Z-9dc656e4`. Independent source reviews found no P0–P2
in the Deck capture, read-only exchange observations or recovery repairs.

- `f1caa6b6`: new Decks and same-op ancestors use real observer rows and the
  existing tracking transaction. Five Rust checks pass, including rollback,
  invalidation and real Service save/ACK/base capture.
- `52037c95`: SQL-only collection observations remain unchanged. Separate
  journal/file media observations make the exact server comparison complete.
  Recovery fixtures use actual cloud-disabled exclusive replacement and prove
  unchanged cloud authority through close, with identical bytes on a new inode.
- `04d989b3`: native exchange checks use actual capture mappings and stored
  results. Test observations never allocate, complete or reset an exchange.
- `1abd7ac6`: the exhaustive instrumentation expectation retains all 259 names
  in the existing registry order; the production registry did not change.

Evidence:

- `core-test/20261030T012732Z-51769`: all five new Deck checks pass.
- `swift-test/20261030T012607Z-49913`: the unchanged suspended-review test passes
  with actual Deck creation, review and endSession during the held account read.
- `swift-test/20261030T011958Z-44606`: all six exchange-ID checks pass.
- `core-test/20261030T013028Z-54607`: both replacement recovery checks pass.
- `core-test/20261030T013337Z-57045`: all 263 selected FFI, Host policy,
  publication and adoption checks pass; none skipped.
- `core-lint/20261030T013242Z-56173`: pass. Later Rust edits removed comments only.
- `swift-test/20261030T013544Z-58373`: 257 of 258 pass, including every one of
  the 242 CoreBridge checks. The remaining instrumentation order failure is
  repaired by `1abd7ac6`; `swift-test/20261030T014543Z-65005` passes all seven
  focused interval-name checks. Preserve the original full-run failure.
- `similarity/20261030T013542Z-58000`: pass for these follow-ups.
- `drive/20261030T014330Z-62503`: both actual VM captures were inspected by the
  integrator and independent reviewer. No further visible layout findings.
  Search's native child still lacks its identifier. Preserve this P2 and failed
  hierarchy result. Three render rounds are complete; no further local UI fixes.

The focused app-model checks first exposed two test macro compile errors.
Their lifecycle reads now occur in typed locals outside the assertions, within
the existing throwing callbacks. Assertions and error propagation are unchanged.
The focused checks now compile and pass as recorded below.

## Collection owner event lifetime

Start: `1abd7ac67aa385eb4123f2cf8015ce0018020ed9`. Filtered source runs:
`cleanroom/20261030T022937Z-90801`, `20261030T024444Z-99078` and
`20261030T024620Z-1127`. Fixer session unchanged. Independent code review:
no P0–P2 findings. Existing S3, L3/L10 and U151 contracts cover this repair.

Closing a collection cancelled the sole app stream consumer. Later owners
could still complete Core jobs but lost review durability and sync updates.
The app now retains one process consumer and forwards ordered events to weak
registered owners. A successful Core close detaches its owner. Failed drain
or Core close retains the registration. CoreBridge and JobRegistry are unchanged.

Evidence:

- `swift-test/20261030T020140Z-77407` and the later isolated five-check run
  were cancelled after stalling. They remain incomplete; the isolated run also
  recorded the missing durable update. Fresh-process passes did not settle it.
- `swift-test/20261030T024355Z-97679`: two new test comment expressions failed
  compilation. Explicit interpolation repaired their type without changing
  either requirement or its error text.
- `swift-test/20261030T024635Z-1589`: all six real collection checks pass in
  one process. The five existing preview/review/lifecycle cases and the new
  real close/reopen regression preserve their assertions. The latter observes
  actual durable answers, local refresh, increasing sequences, new generation,
  failed-close retention, detached-owner silence and real job completion.
- `swift-test/20261030T023136Z-91775`: all 13 model-only coordinator and
  presentation checks pass separately.
- `doctor/20261030T024454Z-99442`: Docker and the test VM are available, with
  53 GiB free. The earlier sandboxed Docker failure was a permission boundary.

The final source-only change removes a redundant test comment flagged by the
privacy scan. No runtime statements changed after the six-check pass.
`private/20261030T024741Z-3113` and
`similarity/20261030T024742Z-3364` pass for the final source repair.

No Unit, P24/F5, UI or release closure. The original-base gate remains required.

## Open acceptance work

- Original-base full gate, focused checks, privacy and similarity.
- Initial collection adoption and native collection assets.
- Deletion handling and conflict navigation.

## First adoption into an absent collection

Continuation start: `8edfe8913e3ffb71c20a9955ae4aa95b3aa5154d`.
U113's original start and the integration batch base above remain unchanged.
Status: backend WIP; no Unit, P24/F5, first-install UI or release acceptance.

The retained Host now drives ordinary inline-record adoption without opening
a dummy collection. Prior file absence stays explicit. Valid prior sidecar
state is retained. Actual R6 operation, candidate and native-file evidence
guard activation and restart; a foreign file that appears during account
preparation is refused before it can be opened or changed. An absent local
file takes no safety backup; existing-file adoption retains its real backup.

Source runs:

- `cleanroom/20261030T025119Z-5778` ended at model capacity. Its prepared
  regular files were retained, validated with the existing source-view rules,
  and recovered against the exact starting commit. The failed run remains
  failed; its draft was not treated as verified or accepted.
- Fixes: `cleanroom/20261030T031233Z-13075`, `20261030T032710Z-18929` and
  `20261030T060150Z-64996`, session `s-20261025T162405Z-9dc656e4`.
- Independent reviews covered conformance, correctness and the clean-room
  boundary. R1 found an unsafe native preview activation and insufficient
  recovery account authority; R2 found an unwitnessed file-arrival path.
  The unsafe native activation was removed, restoring read-only preview.
  R3/R4 found no remaining P0/P1 in the bounded own-build backend slice.

Evidence:

- `core-test/20261030T030743Z-11147`: seven compile errors, retained in the
  failed receipt. The declaration was corrected before the next run.
- `core-test/20261030T032257Z-17153`: production libraries compile; one new
  test used Ready as a unit variant. The struct-variant pattern was corrected
  with the exact generation assertion retained.
- `core-test/20261030T055701Z-62998`: 332 of 334 pass. A rejected adoption
  consumed a read-only preview; absent-file rollback cleanup used the current
  account binding instead of the selected prior publication. One pure mapping
  test also reported LEAK; that evidence is retained without a harness change.
- `core-test/20261030T060502Z-66627`: both failing tests pass unchanged after
  the targeted preview and local-cleanup repairs.
- `core-test/20261030T060613Z-67335`: all 334 selected tests pass, none skipped,
  with no LEAK reported. This is a changed-source regression run, not a claim
  that a separate harness defect was diagnosed or repaired.
- `core-lint/20261030T060652Z-68647`: pass, including all-target checks,
  formatting, layering and binary boundary checks.
- `private/20261030T055733Z-63417` and
  `private/20261030T060616Z-67605`: pass for the final backend source.
- `swift-test/20261030T060755Z-69249`: all eight focused connection,
  ownership and cancellation tests pass. Debug native bindings were rebuilt;
  the parameterized account check covers all five statuses.
- `similarity/20261030T060758Z-69679`: pass against the continuation start.

Required next work:

- The native AppModel must own initial adoption through actual replacement,
  drain and new-generation attachment, enable sync only after acceptance, and
  avoid returning a consumed no-file preview. Its real app regression is open.
- Collection payload assets and structural generation completion remain open.
- Shared R6 recovery across builds still needs its P24/Q11 proof; a missing
  scoped operation currently refuses recovery. Do not count own-build tests
  as evidence for that case.
- Documented P2s: bounded release/join cleanup for the new held-provider tests;
  a CloudUnreadable preview with retained active/epoch facts can still be
  consumed before NotOffered. Preserve these findings for follow-up.

The original-base full gate, native Search identifier P2, mobile and signed
CloudKit evidence remain outstanding. No required verification is waived.

## Initial adoption owned by the app

Continuation start: `318740f20eee8a37c1ffb70d97ad3175bfb7bae3`.
The original unit starts and integration batch base remain unchanged.
Status: in progress; no Unit, P24/F5, visual or release acceptance.

The app retains its original NoCollection Host through discovery, adoption and
callback drain, then attaches the replacement generation. Enabled is written
only at acceptance and restored to its prior value after a terminal failure or
cancellation without a usable binding. A transient waiting operation retains
Enabled. Initial work closes its native enqueue admission on pause/background
and waits for the original callback before a fresh account proof can resume it.

Independent R1 review found missing preparation before policy could start the
SDK, incomplete pause drain, and a replaceable activation callback. R2 uses the
guarded Runtime preparation after actual account verification, closes admission
before draining, and always executes the real Core activation. R2 source review
found no P0–P2 remaining in this bounded handoff. Compiler-only follow-ups
retained that review coverage; runtime verification below remains authoritative.

Source runs: `cleanroom/20261030T061905Z-77932`,
`20261030T065420Z-99717`, `20261030T082916Z-32523`,
`20261030T083433Z-36374`, `20261030T083649Z-39878`;
fixer session `s-20261025T162405Z-9dc656e4`.

The app regression consumes encrypted records exported by a real Rust
publication. The MG Swift job now creates and validates that run-owned fixture
before compiling MeguriKit. It rejects a failed, dry-run, skipped or missing
producer result and nonregular output. No manual environment preparation or
skipped test stands in for the fixture. The fake SDK does not save records;
these checks cannot establish successful upload or full sync.

Evidence so far:

- `selftest/20261030T065757Z-1658`: all five fixture orchestration checks pass.
  Independent infrastructure review is clear after the regular-file guard.
- `swift-test/20261030T064936Z-95979`,
  `20261030T071025Z-5558`, `20261030T083339Z-34632` and
  `20261030T083551Z-37609`: failed compilation receipts retained. The optional
  readiness guard and the test harness's lazy initializer were repaired.
- `swift-test/20261030T083803Z-40677`: production and test code compile. The
  13 selected tests report four issues: successful adoption, pause/background
  resume and transient-error resume time out. Staging cancellation, corrupt
  page and acceptance failure cases pass, as do the six existing collection
  lifecycle/review checks. This is a failed run, not workflow acceptance.
- `core-lint/20261030T083525Z-37142`: pass for the new Rust fixture producer.
- `private/20261030T083824Z-42266` and
  `similarity/20261030T083824Z-42169`: pass for the source at that point.
- `doctor/20261030T083608Z-39181`: VM running and reachable; 53 GiB free.
  Reserved account setup remains pending for final acceptance.

Focused diagnosis:

- `cleanroom/20261030T084142Z-44038` adds named bounded waits and synthetic
  backend observations without changing Core calls or deadlines. Independent
  review is clear; concurrent commands can overwrite `lastExecute`, so that
  field is context rather than an execution history.
- `swift-test/20261030T084347Z-45084`: the timeout is `factory-fetch`.
- `cleanroom/20261030T084513Z-47012` requires the actual successful StartEngine
  result before waiting on that dependent fetch; the original checks remain.
- `swift-test/20261030T084622Z-47901`: StartEngine succeeds, but no fetch is
  requested with automatic sync off. The test expectation was incorrect.
  Independent contract review confirms P6, P10.1 and P11.1: adoption starts the
  cursorless instance but does not override the manual setting. V14.2 step 9
  describes its first fetch; explicit Sync Now supplies that fetch under P12.3.

Next: verify both automatic modes and rerun the affected cases together. A real
native download-state fixture is still needed for VM visual evidence; the
existing setup-needed route cannot render that state.

`cleanroom/20261030T084905Z-50063` corrects the test's scheduling expectation:
the manual case first proves no automatic fetch, then invokes real Sync Now;
the automatic case checks its actual startup fetch separately. Independent
review found no P0–P2 in that correction. No production gate was changed.

`swift-test/20261030T085224Z-51818`: 14 tests run, with two issues remaining.
Pause/background resume, transient-error resume, original-callback account/close
drain, the cancellation/failure pairs and the existing six collection checks
pass together. Manual Sync Now returns before the expected SDK fetch callback.
Automatic startup fetches the actual records, persists the root and leaves no
inbox entries, but `absencePassCompleted` remains false. Keep both failures
open; neither the full fetch workflow nor post-adoption review is accepted.

The next repair is limited to actual native exchange and absence processing.
No test deadline, success condition or assertion is relaxed.

Independent tracing establishes two concrete missing paths. Manual capture
uses the send gate, which requires root confirmation before the captured worker
can fetch that root. Capture must retain its real boundary and selected local
revisions while leaving physical send admission guarded. Separately,
`AbsenceLedger` has a snapshot getter but no production pass creation or
completion path. A completed flag alone cannot supply K17's seen accounting or
deletion safety. Both remain open until their actual regressions pass.

`private/20261030T085616Z-55766`: 8 diagnostic expressions blocked. Their
replacement must pass the same check before commit. No private values were read.

### Bounded Saved-receipt repair of recovered preparation

Role: fixer, following implementer instructions. Resume prepared source
`20261030T085800Z-57176`; starting integration commit remains
`318740f20eee8a37c1ffb70d97ad3175bfb7bae3`, original integration batch base
`1416a520542a8c5daa38a23bfe398da704c3d6c9` unchanged. Full recovered source,
Host/FFI capture tests, private diagnostic rephrasing, tools and docs retained.

Independent review P1: the added full send gate in `Runtime::sent_results`
rejects an already-landed Saved receipt while unrelated real reconciliation or
incoming apply temporarily blocks new sends. Removed only that gate, preserving
the original caller, current binding, native-file, handler/request and persisted
flight checks. New sends, batch construction and capture keep their respective
existing gates and real selection boundary. No additional root proof is created.

Added Host regression
`saved_flight_during_real_reconcile_scan_preserves_newer_edit_and_capture_accounting`:
real capture/root fetch, prepared batch and registered send request; newer deck
edit; real `reconcile_binding` held by the existing `BeforeSnapshot` scan seam;
wrong-request and foreign-writer refusal without tracking/accounting changes;
original SDK/request Saved accepted while scan admission remains closed. It
asserts flight acknowledgement fields, removal of the flight, captured boundary
and selected/acked/outstanding accounting, retained newer pending revision and
unchanged actual newer collection row after scan completion. No production
boolean or fabricated root/flight proof was introduced.

Adjusted
`first_manual_capture_before_root_selects_pending_revisions_without_admitting_sends`
to reject an actual baseline record with no matching flight and assert unchanged
facts, selection and absent root proof, replacing the empty zero-count receipt
expectation. Its no-send-before-root checks remain enabled. The recovered FFI
`cursorless_auto_off_ffi_capture_maps_before_first_root_fetch` is preserved.

One source consistency pass completed (scoped diff/whitespace inspection and
source API/type inspection). No builds, tests, formatter, commits, network or
forbidden-source reads performed. Integrator must run focused Rust Host/FFI and
Swift checks, independent review, and provenance/privacy/similarity checks after
export. K17 absence remains genuinely unwired; enabled app assertions and failed
evidence remain. No protocol-server or reference implementation work. No unit,
milestone, workflow or final acceptance claimed.

## Resumed manual first-fetch repair

After Antoine resumed the goal and reconnected the drive, the interrupted
`cleanroom/20261030T085800Z-57176` source was recovered against its exact seed
and `318740f20eee8a37c1ffb70d97ad3175bfb7bae3`. Permitted regular files passed
`source_view.validate`; the interrupted run stays error. No old implementation
was recreated. The original integration batch base remains unchanged.

The final repair separates pre-root exchange capture from physical send
admission, preserving the actual boundary and selected local revisions.
Saved replies retain their flight authority while a scan blocks new
sends. Successful scan completion rebuilds pending work after clearing its
running flag under the final guarded state lock; stale or failed completion
keeps admission closed. No root proof, SDK identity or completion is fabricated.

Review found the extra ACK gate in the recovered draft.
The held-scan regression then exposed the unreachable pending rebuild. Both
repairs are clear of P0–P2 in their bounded source reviews. Test-only corrections
keep the observation helper absent from ordinary release builds, use the
contract's permitted later image, and register the FFI SDK through a real issued
Start capability after account/settings setup. All original safety assertions
remain, including wrong-request/foreign-writer refusal and newer-edit retention.

Source fixes: `cleanroom/20261030T122908Z-83877`,
`20261030T123504Z-88361`, `20261030T123837Z-91698` and
`20261030T124502Z-96547`, session `s-20261025T162405Z-9dc656e4`.

Evidence:

- Failed receipts remain: `core-test/20261030T122932Z-84587` (default test
  helper visibility), `20261030T123238Z-86480` (incorrect frozen-image
  expectation), `20261030T123534Z-89008` (scan completion), and
  `20261030T123927Z-92469` (FFI setup order).
- `core-test/20261030T124349Z-95702`: all 18 Host policy-driver tests pass.
- `core-test/20261030T124722Z-99460`: the corrected default-build FFI
  cursorless manual-capture regression passes.
- `core-test/20261030T124857Z-1073`: all 353 affected Rust checks pass,
  none skipped, covering core libraries, policy-driver execution, adoption and
  collection replacement.
- `private/20261030T124724Z-99720`: pass for the final changed runtime and
  test files. `similarity/20261030T125010Z-2068`: pass against this continuation
  start. The new fixture orchestration files are byte-identical to their source
  after the five passing `selftest/20261030T065757Z-1658` cases.
- `doctor/20261030T124530Z-97219`: the dedicated VM is running, SSH is
  available and 67 GiB is free after rebuilding Meguri's tooling caches.
  Reserved account setup remains pending.

Final checks: `core-lint/20261030T125609Z-7244` passes all-target formatting,
Clippy, layering and feature-free binary checks. The earlier
`125008Z-1822` lint receipt reports two borrowed-slice changes;
`125404Z-5512` reports their formatting. Both were corrected without suppressions
in `cleanroom/20261030T125302Z-4171` and MG formatting. Review is clear for
those two mechanical test edits; production and all assertions are unchanged
from the 353-check run. `similarity/20261030T125610Z-7485` passes for final
source. `private/20261030T125407Z-5789` passes for the app handoff, fixture
tooling and rephrased review log. Guard refusals remain in their old receipts.

`swift-test/20261030T125729Z-8279`: all six existing real review, preview and
lifecycle checks pass against the rebuilt native core. The job also regenerates
the run-owned adoption fixture through the actual Rust producer and validates
its passing result before compiling. The new absence checks were not selected;
their known failures remain open and are not counted as passes.

K17's actual absence-pass processing, collection payload assets, shared-build
recovery, native download-state visual evidence and the original-base full gate
remain open. The enabled app absence assertions are not passing evidence. No
Unit, P24/F5, first-delivery or release acceptance is claimed.

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

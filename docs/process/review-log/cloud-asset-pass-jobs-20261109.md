# Asset pass creation on retained jobs

Component start: `03abad1a`. Batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
U113, U114 and U151 remain unaccepted.

Two disjoint filtered workers implement retry and first-conflict pass creation
against one fixed FFI contract. Both starts return a retained typed pass result;
an empty completed pass remains distinct from a pending result. Runtime work
checks original authority after State and tracking access, before selection
cleanup, accounting or member selection. Activity-guard failure cancels only
the pass just created. Synchronous interfaces and legacy errors remain.

Rust export `73511` receives a clear independent source review. `91344` only
renames one test-local variable and its two uses for privacy; behavior is exact.
Swift `73807` receives one P2 finding: post-reopen nonnil result alone does not
prove original authority. `91722` adds the original-work cancellation check,
which requires the same Rust owner across wrapper copies. Independent review
closes it. Existing assertions, cleanup and limits remain.

The future completion contract is separately recorded in `6732af21`; no
completion-mode implementation is part of these creation ports.

## Compiler evidence

Core `97868` and Native `98187` fail before tests. UniFFI rejects a qualified
self type in the new workflow export. Export `3753` uses the plain type already imported by the parent.
Independent review confirms that is its sole change to the saved component. The compiler rule is recorded in the FFI instructions. No test is
counted as passing. Focused checks, native build and original batch gate remain.

## Phone setup observation

Signed run `80070` passes. User-confirmed awake drive `83162` still times out
enabling automation before any UI step or status capture. No new media result
is inferred. The user has been asked to restart the phone for one fresh attempt.
No collection, credentials or connection values are included here.

Signed preparation `2921` passes.
Drive `7390` still times out enabling automation before inspecting the app.
The next setup check is the phone UI Automation setting; there is no new
sync or media observation.

Core `12246` does not reach tests: the new conflict fixture stores a boxed
record in an unboxed field. A test-only type correction is pending. The
original compilation failure is retained; it is not a passing check.

## Exact legacy error parity

Core `16651` runs 217 checks: 216 pass. One new assertion incorrectly expects
CloudPolicy Closed from both resource ports. Independent source review confirms
the first workflow preserves its existing Unavailable error, while retry keeps
CloudPolicy Closed. Export `19781` checks each exact error and adds parity with
the same retained synchronous port after close. Other assertions, cleanup and
deadlines remain. Production source is unchanged by this repair.

## Physical automation

Phone `23389` completes all five drive steps after authorization. Its overall
result still fails identifier checks. Sync status shows media work pending;
it is not evidence of a complete sync. A normal Sync Now observation follows.

Core `28434` passes the corrected test. The other 216 checks passed in `16651`
on the same production source; the only later Rust edits are the reviewed new
test assertion and fixture type repairs. The failed receipt remains retained.
Similarity `29914` passes. Native focused verification and lint are pending.

Phone `26433` completes four normal Sync Now drive steps, with identifier
findings still failing its overall result. The status diagnostic identifies a
media/read failure with code 0. It is not a root-cause trace or sync acceptance.

Lint `30174` passes. Native `28884` builds and runs 25 checks: 24 pass.
The new retry result test has four wrong stale-error expectations across its
two parameter cases. Existing Runtime errors remain Unavailable, whereas the
new assertions expect CloudPolicy StaleCommand. Independent diagnosis and an
exact legacy-parity test correction are pending. The failed native receipt is
retained; no unit or batch acceptance is added.

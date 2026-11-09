# Driver committed native validation repair

Status: bounded partial WIP; U113/U114/U151 remain open. Not unit acceptance,
J2.1.1 closure, or full final-enqueue closure.
Start: `f2203b79` (orchestrator input).
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9` (unchanged).
Role: fixer; filtered implementation runner. Authority: engine/jobs J2.1.1
and the supplied independently reviewed committed-view repair scope.

The actual-Owned guard wraps the existing Driver mutex and publishes only
State.guard() and effective State.automatic() in Drop before unlocking Driver.
All actual-Owned production acquisition sites use lock_owned or lock_for_work;
the latter retains its positive try_lock contention observation. Prepared
accepted-identity clones never publish; assignment into actual Owned does.
Success, early returns and partial-effect errors share this publication path.
The compact mutex covers only fixed-size reads/publication, with no Runtime,
projection, callback, media or database work under it.

Final native validation checks the compact policy predicate, retains exact
original Runtime owner/backend/generation/binding/quiet-qualification checks
and the pinned SDK check, then performs one compact policy revalidation.
A changed predicate retains StaleCommand precedence over Runtime failure;
an unchanged predicate retains the original Runtime failure. Native validation
rejects quiet completion with existing UnqualifiedCompletion; Core validation
retains its full qualified-proof contract. FFI retains Host/generation/command
ownership checks and registration/original SDK selection, including missing-SDK
error ordering, without a second full Driver validation. Registration and
stateful native operations retain Driver authority.

Added deterministic real Settings commits between Runtime checks and final
compact validation for both StartEngine and ConfigureAutomatic, checking that
both guards stay equal. Cover both successful Runtime checks and stale SDK
errors. Added actual accepted/superseded identity projection publication checks,
partial-effect failure publication assertions in the existing durable-evidence
fixture, effective automatic publication from Zones, and native rejection of a
real quiet candidate while full Core validation remains valid.

Preserve the original held-projection RED and nextest reservation byte-for-byte,
as well as lifecycle/replacement/no-SDK tests and existing assertions/timeouts.
The supplied RED actually uses a 5-second completion timeout; the request
mentions 1 second. This repair preserves the supplied test without changing
that discrepancy.

One source consistency check only; no builds/tests run in the filtered runner.
Root owns compilation, focused verification, independent review and
provenance/privacy/similarity checks. Runtime lock responsiveness and synchronous
Started reporting/full native enqueue remain separate unresolved obligations.
No Started, scheduling budget, retry, timeout, admission queue or approval gate
change is included.

## Integration evidence

Independent review of export `40884` finds no P0–P2 issue in this partial
repair. Core `43955` passes all 62 selected policy and validation tests,
including the unchanged held-projection test. Similarity `44240` and privacy
pass. No assertion, bound or reservation changed.

This closes that test's Driver contention failure only. Runtime projection
still holds its state lock across tracking scans, and Started reporting still
performs ordered projection before SDK entry. Those remain known technical
gaps; do not infer a passing full enqueue boundary or unit acceptance.

Correction: Core lint `44852` fails clippy on the new test's immediately
invoked closure. Formatting and layering pass. The earlier pass claim was
recorded before checking the complete receipt. The syntax repair and lint
remain pending; no full gate is run over the known enqueue gaps.

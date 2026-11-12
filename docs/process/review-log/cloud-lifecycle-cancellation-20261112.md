# Native cancellation ownership — 2026-11-12

Role: fixer. Base2298db85. Contracts: sync/policy P4, P11.2, P15;
engine/jobs J2.1.1. Source author18431.
Native evidence and first original-task intent must both match.
Original five-case fixture, limits and assertions remain unchanged.
Helper closed follows failed reopen and cleanup; no helper repair.

Review30345: P2, internal receipt constructor permits test construction;
Core30906: FAIL, 7/9 groups pass. New media fixture did not issue its action;
new cleanup fixture used a generation change that does not fail discard.
App36896: FAIL, 10/12 groups pass. Five-case fetch errors are gone, including
replacement4. Expiry3 retains ResumeMedia cancellation, then failed reopen.
New app test counts successor ACKs and assumes nonempty outputs; both need
original-command scope and actual-output-count checks. Keep all original tests.
Private36357: FAIL on two source comments and prose. No guard change.
Required batch gate remains pending from1416a520; no acceptance claim.

Fixer follow-up (filtered start034c550d; integration base2298db85):
- P2: receipt constructor fileprivate; adapter-file-only edge evidence feeds
  the central initializer with no caller overrides. Exact matching retained.
- Media fixture uses automatic:true for real ResumeMedia in both rows.
- Cleanup row uses original-task DEBUG fault after joined real discard;
  asserts invocation, retained injected error, and absent receipt. No fence claim.
- App receipt/result/ACK checks bind captured A/B before Account input;
  each command requires one result and ACK, with actual output-count accounting.
- Added preparation-plus-intent negative rows, including unknown cancellation;
  absent receipt and sticky failure checked before and after close.
- Reworded/removed only the two flagged source comments; privacy guard untouched.
- Existing task-local first-error recorder now covers ResumeMedia preparation,
  SDK lookup, both quiescing checks, explicit cancellation check and worker catches.
  Media behavior unchanged; helper and original five-case fixture untouched.
Native compilation/tests/review: UNRUN here; integrator owns these checks.
Run unchanged five-case once to identify the actual media site before repair.
Source consistency: git diff --check (single check); no native execution.

R2 source review50339: no P0–P2 findings in the corrections.
Core51445: PASS, 9 groups/39 cases, no compiler warning.
App53799: compile failed in a new test: capturingForeground changes after
a Sendable closure captures it. Replace that local flag with synchronized storage.
Private55410: one source comment and two prose lines require text cleanup.
No assertions, limits or production behavior change for this compiler repair.

Compiler-only correction56419 uses Mutex at the same flag toggle points;
reviewed as exact substitutions. No production predicate changed.
Private58804 and similarity59157: PASS.
App57374: FAIL, 12/13 groups pass. New native original A/B tests pass for
expiry and replacement. Generic error tests remain passing. Original five-case
expiry3 reports ResumeMedia firstSite=mediaPreNetwork, firstCategory=taskCancelled.
That edge constructs its native receipt. The remaining intent record holds only
the active command; later pre-issued actions in the same cancelled list do not
match. Preserve remaining queued command identities at the original cancel.
No reset, helper repair or passing batch-gate claim.

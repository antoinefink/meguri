# SDK lifecycle jobs

Status: component in review; U113/U114/U151 unaccepted.
Component start: `e45a0dc5`. Integration base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Session: `s-20261103T133750Z-f276f58e` (fixer).
Contract: engine/jobs J2.1.1, sync/policy P4, cloudkit-recovery V3.4.

## Scope and evidence strategy

The two SDK lifecycle calls use jobs on their original Host. Final invocation reporting and validation stay unchanged.
The existing pool, ordering, typed values and cancellation/drain contract apply.
Cancellation remains effective through lock waits until the first real effect.

Existing registration-once, successor protection and policy tests supply the
baseline. New start APIs have no old implementation to test; a compile-only
failure would not prove the required behavior. Add held-lock admission and
cancellation proofs, then run them with the existing policy tests. Source
preparation does not count as verification.

## Round 1

Filtered export `89668` passes its boundary probes. Format `95099` passes.
Core `95605` stops at compilation: a shared test guard is private to its
parent module. No candidate test ran. Privacy `95887` flags two own-source
files; rewrite the flagged regions without changing the guard.

Independent review finds three P2 issues:

- After registration commits, listener setup failure can drain the job and
  discard its SDK identity. Preserve the actual resource result or clean up
  that exact SDK before returning an error.
- The concurrency proof needs a positive still-held witness at review
  completion, in addition to its existing release and join cleanup.
- Give the existing shared test guard appropriate test-only visibility.

The filtered repair retains all existing assertions and limits. No full gate
or unit acceptance is claimed.

## Signed phone observation

On the prior validated Runtime component, normal signed preparation `88332`
passes. Status `90160` completes five steps; the hierarchy audit fails.
Drive `91280` cannot tap the tab beneath the open sheet; no sync starts.
Corrected drive `92238` uses the open popover and completes all five steps,
including one Sync Now request. Read-only status `93372` completes three
steps. Both drives retain hierarchy failures and are not passing receipts.

The displayed last-sync time advances; the media queue is 2,885 files,
compared with the earlier 2,919. Another 2,397 files remain reported missing
on this device and in iCloud. The retained taskCancelled diagnostic does not
prove the cause of the current media gap. No extra answer or replacement is
performed. This is partial device evidence, not a complete media round trip.

## Round 2

Export `96823` closes the three round-1 findings. Core `99490` passes all
116 focused checks. Privacy `99143` still flags comments and a test fixture
name; those regions need another nonbehavioral rewrite.

Independent review finds one further P2: after Driver checks ownership,
a job can wait for Runtime while a real exclusive replacement advances
the engine generation. These job names are outside replacement's two named
sync drains. The old Runtime guard can still be visible before rebind.
Retain the original engine-generation fence through the pre-effect checks,
without invoking Host validation under Runtime. Preserve the exact sampled
GenerationChanged failure and every original post-effect result.

The next repair adds a real replacement/contention regression. Its test-only
form must first fail against the prior candidate, then pass with the repair.
Existing queued-generation coverage and all timing limits remain enabled.

## Round 3 and focused verification

Export `691` adds the retained generation fence and its sampled refusal flag.
The original owner error keeps precedence; no later owner lookup replaces an
actual result. Independent review has no remaining P0–P2 finding.

Test-only Core `2361` reproduces the race on the previous candidate: the real
replacement advances generation inside the positively held window, but the
job succeeds. All hold, release and teardown checks preceding that assertion
complete. The same authored test is retained with the fix. Core `3759` then
passes all 117 focused checks, including both held-lock validation regressions.

Integration omits one unnecessary added comment still flagged by privacy.
No executable line, assertion or limit changes. Final privacy `4712`, lint
`5003` and similarity `5293` pass. Earlier failed receipts remain retained.
Native bridge build/tests, full invocation-boundary work, the remaining native
admission calls, media evidence and the original batch gate remain pending.
No Unit trailer or acceptance is added.

## Native completion of this component

Swift `6229` builds all three Debug Rust slices and matching bindings, then
stops at a Swift compiler error: the shared helper's tracking closure needs
an escaping annotation. No tests run in that failed receipt.

Filtered export `8550` adds only that annotation. Independent review finds
no P0–P2 issue; privacy passes. Swift `9299` reuses the built Rust products
and passes all 15 selected checks, including the real installed-identity
listener-failure proof, old listener/order/drain checks, instrumentation,
held-factory readiness and six stale final-admission cases.

This closes the reviewed two-call component. The unfinished unit, media,
full invocation-boundary and batch-gate requirements above remain open.

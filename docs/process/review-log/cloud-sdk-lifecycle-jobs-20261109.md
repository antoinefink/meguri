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

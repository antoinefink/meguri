# Review lifecycle failure observation — 2026-11-16

Starting commit: `8082b2ff2f0a8d6b38fa5f35a84ca27ef3dd6bc6` (clean master).
Diagnostic source: `d637ae0b4`.
Keep integration base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The retained Mac Unit result in `xcode-check/20261116T023911Z-29156` fails six
ReviewCoordinator lifecycle methods. Most report the shared three-second
deadline. The visual-preparation cases also report assertions after teardown
starts. The standalone host package run passes this suite, which does not
establish the cause of the VM failures.

Read-only diagnosis `cleanroom/20261116T100540Z-79508` found that the fixture's
`requested()` can return when its stream ends without a real request. Deadline
cleanup cancels the scenario and releases gates, so subsequent assertions can
describe teardown rather than the original wait. This diagnostic defect does
not prove that production lost an answer. The original stalled step is unknown.

Author `cleanroom/20261116T101711Z-56321` added fixed fixture checkpoints,
gate-entry counters and a value snapshot captured at the original deadline,
before cancellation or gate release. Independent source review
`20261116T102540Z-95050` found no P0–P2 findings; its export matched the seed.
Only one test file changed. The snapshot contains fixed fake facts. No product
code, assertion, timing limit, concurrency setting, task or cleanup order changed.
Checkpoints record the last reached location across callbacks, not the identity
of a proven stalled await.

Privacy `private/20261116T102856Z-30499` passes for the source.
Similarity `similarity/20261116T102912Z-32846` passes: one scanned, zero flags
and zero allowlisted files. Only the filtered implementer report was read.
Static checks `static-checks/20261116T102912Z-32755` pass all three checks.

The complete Mac Unit Debug workload ran in the dedicated VM as
`xcode-check/20261116T103014Z-33872`, on clean `d637ae0b4`. It fails. MG reports
1,548 passes, six failures and four skips. Five failed methods belong to these
lifecycle scenarios; the sixth is native replay.
The formerly failed `resumeAfterCloseJoinedWorkCannotRestartPages` passes in
this attempt. That change of outcome does not establish a repair or its cause.
The four skips are media tests in the VM; none of the lifecycle cases was skipped.

Thirty timeout snapshots are retained locally. They show different reached
stages: active visual holds before physical suspension, entered physical holds,
caller-result joins after cancelled preparation, and persistence or restore
joins. They do not establish one shared ownership cycle or prove that test
limits are wrong. The next step is tracing these observed waits against source.
No further native attempt or production repair has been made from a guess.

The run's result bundle and log were copied into its own `retained/` directory
before another job can replace the shared Xcode paths. The original failure
receipt and exported original test tree remain. The diagnostic adds evidence;
it is not a lifecycle repair, full-gate pass, Unit acceptance or real-device
review-convergence claim.

Read-only follow-up `cleanroom/20261116T105806Z-16457` traced all 30 snapshots
and exported no source change. It establishes no production ownership cycle or
admission defect. Several cases already joined the cancelled preparation and
entered physical suspension; the close variant of one accepted-answer case
even reached the end callback after pause persistence returned. The Redo cases
timed out during preparatory Undo, before the Redo exercise began.

The gate's `cancelled` flag is latched after its wait returns. An active wait
with that flag false does not prove that cancellation was never requested.
The caller-cancellation case has no remaining fixture-held dependency after its
gate exits, but the snapshot does not show whether the return chain is runnable
or suspended. Scheduling delay remains unproved.
`theStoreWorksOffTheMainActor` completed in 0.033 seconds in this attempt;
that duration alone does not explain these deadlines.

Next observation: retain the synthetic caller/preparation/resume task states
at the deadline in a required VM workload, without locals or unrelated task
data. That should distinguish delayed execution from an unexpected dependency.
Another unchanged full workload would not establish that fact. Other required
failure repairs may proceed meanwhile; this group remains failed.

Follow-up: [current suite-isolation evidence](review-lifecycle-suite-isolation-20261116.md).
The current Mac Unit workload passes; the earlier failures remain retained.

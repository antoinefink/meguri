# Review lifecycle suite isolation — 2026-11-16

Starting commit: `ccd8ecf` (clean master). Experiment source: `85b53e8f`.
Integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Change and review

Read-only source review found no dependency between separate test cases.
Each ReviewCoordinator fixture owns its model, client, surface and UUID store.
Intentional start/resume/pause, cancellation and two-coordinator races run
inside each case. Adding `.serialized` to this suite preserves those races.
Other suites can still overlap it.

Author `cleanroom/20261116T125342Z-55923` exported exactly that one-line change.
Independent review `20261116T125522Z-58527` was clear and preserved the seed.
Every argument, assertion, diagnostic, cleanup and original 3 s deadline remains.
Privacy `private/20261116T125712Z-64779`, static
`static-checks/20261116T125712Z-64976`, and similarity
`similarity/20261116T125712Z-64670` pass. The filtered similarity report has one
file scanned and no flags or exceptions; no reference content was read.

## Original VM workload

`xcode-check/20261116T125837Z-67484` ran the complete Mac Unit Debug plan on
clean `85b53e8f` in the dedicated VM. It passes: 1,553 tests passed, zero failed,
four skipped. The structured tree records Passed for all six original methods:

- secondPauseCancelsAndJoinsResumedStartup
- callerCancellationKeepsResumePausedUntilNewHeadIsPresented
- resumedStartupLoadPreservesCancellationOwnership
- resumeAfterCloseJoinedWorkCannotRestartPages
- acceptedAnswerPreparationIsCancelledWithoutCancellingCommit
- visualPreparationIsCancelledAndJoinedBeforeLifecycleStop

The four skips are the existing VM media cases: MP3 range playback, silent-file
advance/seek, controller queue playback, and autoplay held until presentation.
None belongs to the review lifecycle suite. The rendering stress workload also
retains its existing known web-content memory issue; no assertion was removed.
The full bundle and log are retained in the run's own `retained/` directory.

The earlier failed workload ran at `d637ae0b4`. Since then, the Mac native AX
proof moved to its mandatory app-hosted Snapshot VM lane and fixture lifecycle
helpers changed. This run verifies the current combined source. It does not
isolate suite serialization as the sole cause or prove a production repair.
All original failed receipts and 30 pre-cleanup snapshots remain available.
The full integration gate remains failed.

## Separate native accessibility observation

`ccd8ecf` adds only existing-read context to the native AX failure: child-index
path, depth and already-read role/identifier. Author `cleanroom/20261116T124827Z-41745`
and independent review `20261116T125045Z-44553` preserve every read, assertion,
ordering and timing bound. Privacy 47777, static 48113 and similarity 48541 pass.

`mac-hosted-test/20261116T125318Z-53598` still fails: one executed test, zero
passes/skips. AXChildren fails at owned path root/0/0, depth 2, with error -25204,
nil role/identifier/value, and process trust false. This locates the failure;
it does not establish a permission cause. No further AX repair is inferred.

## Next independent repairs

The two original Mac audit crash stacks both stop in XCUIElement.screenshot,
before image conversion or the appearance algorithm. A read-only source audit
rejected ImageIO decoding as a fix for that earlier crash. The Mac-only public
NSApplication initialization candidate is prepared as
`cleanroom/20261116T130145Z-70608`, independently clear in
`20261116T130308Z-72142`. It remains outside master and unverified.
The exact native capture, audit assertions and thresholds must remain intact.

Independent evidence review `cleanroom/20261116T133215Z-4158` is clear.
The exported document content matches the seed.

## Credential path follow-up

The unchanged `deviceSlotIsNonsynchronizingAndNeverContainsPassword` method
passes in `mac-hosted-test/20261116T133036Z-251` on clean `85b53e8f`: one
executed pass, zero failures/skips, full_suite false. This uses the existing
team-signed VM path. The original ordinary snapshot app was ad-hoc signed and
returned a missing-entitlement error. Its complete signed grants were not
retained. No store code, permission or profile scope changed. The ordinary
snapshot job still needs valid signed authority and its own verification.
An independent read-only check of both run summaries confirms this scope.

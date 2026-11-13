# Retained publication proof order

Start: `ca265ffe`. Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The paused test held the original zones reply, then joined a repeated account
read. Production correctly fenced that read behind the held original work.
The test could not reach its own release call.

Diagnostic export `cleanroom/20261113T094551Z-90866` added only synthetic test
markers. In `swift-test/20261113T094852Z-94399`, fresh unpause proof completed
and the original zones request was admitted. The repeat read then reached the
waiting marker with zones=1, save=0 and active=1. It never reached the release.
Only that run's verified helper group was stopped. This incomplete run and the
earlier `20261113T085431Z-26888` remain failures, not passes. The diagnostic
patch was reversed exactly; its export and trace are retained in build output.

Repair export `cleanroom/20261113T095008Z-95946` changes only the existing test.
It observes bounded waiting, verifies the repeat proof has not completed,
releases the original transport and joins its work, then checks proof success.
All five cases, existing assertions, replies and signal limits remain.
The observer is cleared at teardown. Production is unchanged.

Independent read-only review `cleanroom/20261113T095138Z-96478` found no P0,
P1 or P2, and confirmed contract and coverage preservation.

Focused `swift-test/20261113T095153Z-96892` passed all five parameter cases.
Privacy `20261113T095228Z-99825` passed. Similarity
`20261113T095229Z-99823` passed one file with zero exemptions.
Static checks `20261113T095259Z-1209` passed 3/3. An earlier invocation used
an unknown job name and executed no static checks; its receipt is retained.

The full batch gate and signed cross-device verification remain due. This
correction supplies no unit, milestone or release acceptance.

## Wider selection remains red

`swift-test/20261113T095217Z-98420` progressed past the repaired test. It then
reported failures in these existing methods:

- `selectedAdoptionRefusalFailedPageAndLateCancellationJoinWithoutReplacement`
- `serviceAppOriginalProofProjectsActualProviderOutcomesWithoutLosingPendingWork`
- `serviceAppParkedFlushRevocationACKPrecedesNativeAndCoreCancellation`
- `serviceAppPredecessorCorePhaseRetainsRealResultACKBeforeAccountProof`
- `q11ServiceAppSignInAfterRootFetchReadsDeletedActiveWithoutDrain`

It later stopped emitting output in the account case of
`initialAdoptionOriginalCallbackDrainsAcrossLifecycle`. Its verified original
helper group alone was stopped after over 60 seconds without output. The run
is failed and incomplete; later tests are unrun. Raw output and the stop record
are retained. These observations do not establish a baseline comparison or
prove their causes. Focused diagnosis remains required before the full gate.

# Review terminal authority

Implementation start: `8593a2080f06daa027507773514fc5a62a744722`.
The open integration batch still starts at
`81c207573c20c337c8834e4ae8b4ee8f29603dbf`. No unit acceptance is claimed.

Native AX5 run `ui-test/20261025T050355Z-66903` reached Check Now and tapped
it, then failed because waiting changed to finished. The old recipe put its
10-minute relearning step inside 20-minute learn-ahead. Easy could exhaust
that session. Separately, the coordinator combined an accepted session head
with committed deck facts, which could still describe the earlier learning
card. The exact native commit timing was not captured.

Spec session `s-20261025T051506Z-1148ea52` added `sched/review-session#C17.1`
and the F9 recipe setting in `8593a208`. Independent review found no P0–P2.
The head carries its own active, waiting or finished state from the same
projection as its items and counts. This adds no persistence or sync format.
Only the local-review recipe sets learn-ahead to zero; other defaults and
preset steps remain unchanged. Generator version is 5.

Fixer session `s-20261024T174115Z-511e3ad6` exported
`cleanroom/20261025T051852Z-90361`. Review round 1 found two P2 issues:
optional summary loading still blocked terminal presentation, and the waiting
event branch had lost its session-identity check. Cumulative round 2 export
`20261025T053001Z-94091` closed both. Independent review found no P0–P2.
Summary work cannot hold terminal controls or replace newer state after
cancellation, pause or close. Both event branches validate the session.
The ended-session assertions do not claim to execute the exact projection/end
race; no new hook was added for it.

Round 3 export `20261025T053857Z-3648` changed only two test-only regions
flagged by the privacy scan. Independent delta review confirmed the same two
terminal values, eight summary scenarios and four-card assertion. The privacy
guard and native AX5 assertions were not changed.

Focused results:

| Check | Run | Result |
|---|---|---|
| Session faults | `core-test/20261025T053524Z-96498` | 22 passed |
| Rust bridge | `core-test/20261025T053615Z-99775` | 5 passed |
| Swift bridge | `swift-test/20261025T053527Z-96747` | 8 passed |
| Coordinator | `swift-test/20261025T053722Z-1291` | 13 passed |
| Recipe validity | `core-test/20261025T053809Z-2844` | 7 passed |
| Rewritten test | `swift-test/20261025T054252Z-6166` | 1 passed, same eight scenarios |
| Rust lint | `core-lint/20261025T054250Z-5921` | passed |
| Privacy | `private/20261025T054247Z-5680` | passed |

Filtered-out tests are not counted as passes. The broader focused checks used
the reviewed round 2 behavior; round 3 changed test text only. Native iPhone,
iPad and Mac evidence, rendered-image review and the full gate from the
original batch base remain pending.


## U186 Continue — 2026-11-14

Continuation starts at `17a3a93a7f330926fd037bf2fb85712fb3c9cdca`.
Original start `8593a208` and its three review rounds remain recorded above.
The active integration base is `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
U113/U114 are unchanged. This work has no Unit trailer.

Automatic head reads update queue facts while the waiting/completion content
stays visible. Continue reads the head again before showing its card. Check Now
keeps its existing refresh role. The same native terminal component supplies the
live view and four fixed preview states; controls and corner layout stay native.
The new label uses `review.congrats.continue`, English “Continue” (S8).

Author `cleanroom/20261114T052002Z-38555` starts from 17a3a93a.
Baseline `swift-test/20261114T052000Z-37473` passes 35 tests; the first candidate
`swift-test/20261114T053050Z-40204` passes 43. Reviewer session
`s-20261109T210639Z-47d06d5b`, R4 `cleanroom/20261114T053035Z-39770`, finds:

- P1: a failure received during an older head read can lose its fresh read.
- P2: pause can interrupt Undo or failure recovery before the card is shown.

Fixer session `s-20261110T214025Z-20fa15fd`, export
`cleanroom/20261114T053822Z-44794`, captures pending flags before reads.
One restoration identity survives interrupted preparation. An old result cannot
finish a newer recovery. Successful question presentation or its own empty head
settles that restoration; close and collection invalidation discard it.
Continue stays bound to its one action, including cancelled preparation.
The display timer stays hidden during terminal presentation. The ready-work
fixture now has nonzero remaining work and progress below full.

`swift-test/20261114T054534Z-46368` passes all 49 selected tests, with no failure
or skip. It includes the exact failure-during-read order, interrupted Undo/Redo
and failed-answer recovery, empty results followed by new work, later failures
during earlier reads/presentation, cancelled Continue and timer visibility.
Tests for existing media references, saved answers and visual cancellation run
too. Cases and timing bounds remain enabled.

Render run `snapshots/20261114T053242Z-42105` produces no images because the
app does not compile. The route source lacks `import AppModel`.
`cleanroom/20261114T053400Z-44151` fixes it and replaces the reported prose.
`20261114T054424Z-45443` uses the S8 label key; `20261114T054627Z-48277`
changes one comment. Checks retain their original settings. Earlier rejected
scans are `private/20261114T053051Z-40693` and `20261114T054536Z-46853`.

Independent R5 `cleanroom/20261114T054428Z-45789` reports no P0–P2 findings.
Five rounds are counted for this continuation and its prior work; R6 is unused.
The later catalog/comment changes preserve that logic. Root checks those small
patch differences; neither changes an assertion, timing bound or behavior.

Final focused checks:

- `unit-sim/20261114T055325Z-52326`: iPhone, 53 passes, zero failures/skips.
- `unit-sim/20261114T055715Z-54654`: iPad, 53 passes, zero failures/skips.
- Both mobile logs include the failure/read ordering, all nine restoration
  interruption cases, newer recovery events, cancelled Continue and real
  WebKit review-media owner cases. The Mac 49-test result is recorded above.
- `similarity/20261114T055002Z-51564`: the permitted implementer report shows
  twelve files checked, zero flags, zero exceptions and none out of scope.
- `private/20261114T054913Z-48824` finds only one log paragraph; all twelve
  source paths are clear. The paragraph is rewritten, then the log passes
  `private/20261114T054937Z-50957`. No scanner settings change.

`snapshots/20261114T054922Z-49101` records exactly four new references under
`review.terminalContinuation`; no older reference is changed. Root inspects
waiting, ready, finished and finished-with-new-work: text wraps without clipping,
buttons align, and the ready states show Continue. These are native content
renders on the light standard Mac cell, with system controls. Window toolbars,
live Core/WebKit transitions, other appearances, accessibility audits and user
design agreement are not established by those images. The references are new
and unapproved; four recorded renders are not a baseline-comparison pass.

The snapshot job fails overall on eight retained checks: the cloud callback
fact, soak without its dedicated context, settings search, credential access
under ad-hoc signing, two cloud-scope probes without their run context, and two
route-list expectations missing the older replay routes. None is suppressed or
counted as passing. The four new route kind cases execute successfully. This
bounded work does not repair those older failures or count as a full gate.

No live Core transition, hardware, full-unit, milestone, design or release pass
is asserted. Full congratulations details, unbury, summary hand-off and remaining
integration evidence stay open. U113/U114 repairs remain held for the owner's
bounded-round decision. The earlier history and all failed receipts are retained.

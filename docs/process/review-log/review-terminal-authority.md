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

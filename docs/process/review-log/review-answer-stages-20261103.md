# Review answer stages

Status: diagnostics; no unit acceptance.
Start:9a0d22e5; source:1111d522.
Batch:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer:s-20261103T121513Z-929039f7.

The active-sync Reveal wait has no proven dependency on MaintenanceLane.
Two files add DEBUG-only static OSLog messages, category ReviewAnswerStage,
subsystem Meguri. Each existing await has begin/end messages. No card values,
IDs, paths, typed input or errors enter these messages. No await or guard changes.

R1 independent review is clear of P0–P2 findings. Swift59034 passes17 existing
coordinator tests. Private58979 and similarity58978 pass both files, with no
exceptions. UI lint and compilation pass within the Swift job.

complete denotes successful showAnswer return. reveal.complete is terminal
after success or a handled error. Neither an end marker from an optional
helper nor reveal.complete alone proves a visible answer.

Signed build and live trace await verification. C6 hook ordering and the native
cloud/Resume repair cap remain intact. No new tests mirror diagnostic literals.

Build62232 passes from clean147381e4. Executable SHA256:
c8dce791007ebf778b3c536752c35f58ceb32cd12b059cedf07aa3f2a9f2acd3.
The first live Reveal completes all stages in23ms during Syncing Media.
After returning to decks and entering another review, the question has a
disabled Show Answer control and no second surfaceReveal.begin. This points
before renderer admission; it is not evidence of a shown-hook stall.
The fixed-category log stream stopped and joined normally.

An independent source audit identifies a startup activity race: beginPause
can run before start supplies sessionID, while recovered activity tasks call
resume too early. Startup completion does not resume that paused model.
Its exact live occurrence is unproved. A held-start inactive/active regression
and same-model activity reconciliation are the next bounded work.

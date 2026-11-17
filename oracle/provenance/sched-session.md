# Review session queue progression (U43 spec writer)

Spec-writer notes for part P of `specs/sched/review-session.md` and for
`specs/conformance/session-ops.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Part C (the C5 contract) is Meguri
design from the plan and the Product Contract and has no Anki source.

| Spec section | Anki files read | Read for |
|---|---|---|
| P1, P2 | `rslib/src/scheduler/queue/mod.rs`, `learning.rs`, `main.rs`, `entry.rs`, `builder/mod.rs` | what a built queue keeps, the cutoff and learn-ahead parts, the presentation order, the learning key |
| P3 | `rslib/src/scheduler/queue/mod.rs` | the cutoff refresh when every count is 0, before the queued cards are returned |
| P4, P5 | `rslib/src/scheduler/queue/mod.rs`, `learning.rs`, `main.rs`, `rslib/src/scheduler/answering/mod.rs` | removal from the learning list at any position or from the main list's front, the not-at-top refusal, the requeue condition, the order of requeue and cutoff update, the move to one second after the first learning entry while the main list is empty, the saturating counts |
| P6 | `rslib/src/scheduler/queue/main.rs`, `learning.rs` | count changes per entry kind |
| P7 | `rslib/src/scheduler/answering/mod.rs`, `rslib/src/scheduler/bury_and_suspend.rs`, `rslib/src/scheduler/queue/mod.rs` | siblings buried by an answer are not removed from the held queue; the mtime check when queued cards are read |
| P8 | `rslib/src/scheduler/queue/mod.rs`, `rslib/src/ops.rs` | answers never clear the queue; other changes clear it and the next read builds again |
| P9 | `rslib/src/scheduler/queue/undo.rs`, `learning.rs`, `main.rs` | what an answer's undo record keeps (entry, requeued entry, build time, count and cutoff snapshot), undo pushes to the front, redo restores the snapshot, a changed build time clears the queue |
| P10 | `rslib/src/scheduler/queue/mod.rs`, `rslib/src/scheduler/bury_and_suspend.rs`, `rslib/src/scheduler/service/mod.rs` | the day check at the next read (queue and undo dropped, buried cards restored); the timing call's unbury writes outside any operation, so it keeps the held queue |
| P11, C2 | `rslib/src/scheduler/answering/mod.rs` (state comparison), `rslib/src/scheduler/reviews.rs` (Grade Now clears the queue), `rslib/src/scheduler/queue/builder/mod.rs` (the load balancer is built only when enabled), `qt/aqt/reviewer.py` (the reviewer refreshes to the new head after a queue change) | the differences table |

Checked on the oracle (`tools/mg oracle record sched/session/`): every
scenario under `conformance/scenarios/sched/session/` was compared, step by
step, with a model of P1 to P9 written from the spec text; all queue reads
agreed. The drain scenario `sched/session/drain/mixed-40` was grown one answer
at a time, each answer naming the card the oracle's queue presented first.

## Portable pin manifest

Pinned release: `26.09.3`; commit: `29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba`. The section citations above resolve against this commit, independent of the prepared Meguri branch. The hashes below identify the cited pinned files; no source is copied here.

| Pinned path | Git blob | SHA-256 |
|---|---|---|
| `rslib/src/scheduler/queue/mod.rs` | `6a8357142df04cc80e2b9a0a0e422402958b76ec` | `ecdb6d90c70a1a1c74d91f82655d0da84c4ece71dc103a66ec40fc6de1c6f5d7` |
| `rslib/src/scheduler/queue/learning.rs` | `169b65ae47e7643e5ce7ecda544760ec5860f82c` | `65e7543e3cf0a124013c2ca8bb67ca11a40242093c1acf167fb4c2773af44464` |
| `rslib/src/scheduler/queue/main.rs` | `39129fe7ef5419473f58c7a8fc26c3a66405237d` | `3891fcb68958fb5d8e8376eb13343db30b2f5130e57dbd96bc96b1502d0d5023` |
| `rslib/src/scheduler/queue/entry.rs` | `1ca9de47899ad01b80c597ffefe245c88c8fd653` | `70f10d3800a8da49319df7b91aa4a4a3464a5a1ea20fca6f2c9ea54e7b829e5c` |
| `rslib/src/scheduler/queue/builder/mod.rs` | `1f12450b1970cdaac6f93aba781f67f7bda891e8` | `66c08d5f19a874c2c27a42391f3e34c550f283d06e9c6b89d1daeee4aa730633` |
| `rslib/src/scheduler/queue/undo.rs` | `02046eebcff233e096bc92e353ce0786b7da49db` | `3e2e978b12f04db4c27a6c509d897794f76bc4ccfe4fcda5f81a0ade216a7b02` |
| `rslib/src/scheduler/answering/mod.rs` | `ff81d86fa67f8cf24efce1c27d199f7e80522856` | `8b4be53a9f812699186922da8f898103457fe83e150eb3cd666210a64ddbde47` |
| `rslib/src/scheduler/bury_and_suspend.rs` | `8511995086d210afe1a43803d5dfe9209c3fe278` | `6e155f0368de174f8e57ac773e8ddd3846605982b7e2b3cf0c89a69ee15dbeb9` |
| `rslib/src/scheduler/service/mod.rs` | `073706ce81982afb217547c5759b6d8706d92db3` | `de65847d35d79aba01143053f55a3498ad2e007228edfad9ad9910e606569f3e` |
| `rslib/src/scheduler/reviews.rs` | `1bf3e309ddeb006932a17dd8607b2df103d44197` | `5af11598fcac28a1251f027dc09928c9be4339124f8fdd877361def750771860` |
| `rslib/src/ops.rs` | `be27b649a6e0a0810529aa9af61b2b5fbaa7e25b` | `1e1ad13156fbc7ab83db173d52fd8312a66630772714f59f21b036f2617423a2` |
| `qt/aqt/reviewer.py` | `59c829c475bfe4400b890358fe84cbe42cd5466d` | `7be45f1c0da888a1bb530ea37f824f8f55a7e943ab2e35c5d8ff47e1ac33d363` |

## U43 resume boundary

The prepared section mapping is preserved. Current Part C admission, token, observer ownership, optimistic cache accounting and journal/recovery amendments are Meguri requirements, derived from the accepted neutral contracts and Meguri APIs. They add no new reference-source claim. Part P remains the behavioral basis of the eight unchanged recorded pairs. Historical focused comparisons do not imply a successful whole-corpus check or current product activation.

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| Held-queue session scenarios: sched/session/progress/new-note-again, learn-ahead-finish, answer-counts and sched/session/requeue/lapsed-review-due (specs sched/review-session P2-P6, P8; sched/queue-building B6, B9) | `pylib/tests/test_schedv3.py` (tests behind h1b8a7b774a45, h6e88f0974790, h85d4fffafbab, h69f8e4b9fca9) | Which behaviors the harvested tests check: the new count after adding a note, and Again moving it into learning; learn-ahead showing future learning cards once other work is done, then finishing; counts across a run of answers; a failed review waiting behind due reviews until it is due. Expected values come only from the oracle goldens. |
| sched/session/head/counts-follow-answers; existing sched/session/requeue/after-next-learning (review-session P2, P4, P5, P6) | `pylib/tests/test_schedv3.py` | Which behaviors to prove: fetching the head does not decrement counts, and answering moves the card's count category; in collapsed learning, the card just failed is not presented again at once when another learning card is available. |
| sched/session/deck-flow/parent-then-children, child-review-limit, new-within-review-limit | `pylib/tests/test_schedv3.py` (test_deckFlow, test_review_limits), `rslib/src/scheduler/answering/mod.rs` (new_limited_by_reviews) | Which behaviors rows hdf5598f4cd0f, he3a4a3e5014c and h0cc7ca135634 oblige: parent deck study order across children, a child answer lowering the parent's remaining reviews in tree and queue, and new cards limited by the review limit with counts through learning graduation. |
| sched/session/undo/learning-counts, sched/session/undo/redo-after-deck-step; proposed P9 redo-after-rebuild text | `rslib/src/scheduler/queue/undo.rs`; `rslib/src/scheduler/queue/mod.rs`; `rslib/src/ops.rs` | undo.rs tests: which behaviors the harvest rows require (counts across add/answer undo/redo, redo of an answer after queue invalidation). undo.rs queue-change handling and mod.rs queue invalidation by build time: that a redo after a rebuild only drops the queue. ops.rs: which op changes force a study-queue rebuild, used to pick Add Deck as a portable invalidating step. Expected values come from oracle goldens only. |

# Answering a card and learning-step fuzz (U25 spec writer, for U44)

Spec-writer notes for `specs/sched/answering.md` A1-A11 and `specs/sched/fuzz.md`
Z7, and for switching `undo/revlog-taken-id` to FSRS. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Every rule was checked against
goldens recorded from the oracle (`sched/answer/new-card-ratings`,
`state-kinds`, `deck-counters`, and the existing `smoke/fsrs-new-card`,
`smoke/sm2-new-card`, `storage/stamps/answer`); the Apple oracle lane gives the
same values for the three new scenarios and the re-recorded undo scenario.

| Spec section | Anki files read | Read for |
|---|---|---|
| answering A1, A2 | `rslib/src/scheduler/answering/mod.rs`, `pylib/anki/scheduler/v3.py`, `rslib/src/collection/transact.rs` | the answer transaction, the time cap from the home preset, the deck lookup failing with not found, `col.mod` |
| answering A3 | `rslib/src/scheduler/answering/learning.rs`, `review.rs`, `relearning.rs`, `rslib/src/scheduler/states/interval_kind.rs`, `rslib/src/card/mod.rs` | the columns each state kind writes, the conversion of a delay to days at the next day boundary, stamping |
| answering A4 | `rslib/src/scheduler/answering/mod.rs` (the card state updater), `rslib/src/storage/card/data.rs`, `rslib/src/scheduler/fsrs/memory_state.rs`, `rslib/src/decks/mod.rs`, `rslib/src/deckconfig/mod.rs` | `pos` on leaving New, `lrt`, desired retention and decay with FSRS on, the parameter list chosen |
| answering A5 | `rslib/src/scheduler/answering/revlog.rs`, `rslib/src/revlog/undo.rs`, `rslib/src/storage/revlog/add.sql`, `rslib/src/scheduler/states/*.rs` (revlog kinds and intervals) | the row's columns, the shifted difficulty, the id bump |
| answering A6 | `rslib/src/decks/stats.rs`, `rslib/src/decks/undo.rs`, `rslib/src/storage/deck/mod.rs` (parent walk, update), `rslib/src/decks/addupdate.rs`, `proto/anki/decks.proto` | the counters, the day reset, the parent walk stopping at a missing parent, no name normalization on this write |
| answering A9 | `rslib/src/scheduler/answering/*.rs` | what differs with FSRS off |
| fuzz Z7 | `rslib/src/scheduler/answering/learning.rs`, `current.rs`, `mod.rs` (fuzz seed) | the learning fuzz range and draw, the previous seed used to rebuild the current step |

Notes for U44's spec writer:

- The oracle's `answer` op asks for the next states and answers outside the
  study queues. Anki's load balancer lives in the queue state, so it applies
  only once something has built the queues (no driver op or observation does
  today), even with `loadBalancerEnabled` on. Load-balancer scenarios need a
  driver op that builds the queues or answers from them; check this before
  recording them.
- Answering stamps the card's decks even when their counters do not change
  (an intraday learning answer of 0 ms on the same day); Meguri's write rule
  skips unchanged rows (`storage/ids-usn-mtime` S4), so such a scenario needs
  a deviation or a decision.
- The deck counter write does not normalize the deck's stored name; U28's
  `decks/decks` D1 says every deck write does (finding for spec/U28).

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| Step progression through real answers: sched/steps/answered-progress, relearn-none-answer, interday-answers (specs sched/steps L1-L3, L5-L7, L9; sched/answering A3.2, A5; sched/review-session P9, P10) | `pylib/tests/test_schedv3.py` (tests behind h07a0e8e761e3, h5566e8764422, h9ee8c470c9b4) | Which behaviors to prove: fractional learning steps, with remaining counts and logged delays up to graduation; Again with no relearning steps returning the card to review; day-length learning steps moving between the intraday and interday queues; undo of an answer; a 1-day relearning step. Step lists, deck contents and expected values are Meguri's own or come from the oracle. |
| sched/steps/custom-steps-chain; sched/answer/lapse-then-easy; existing sched/answer/new-card-ratings (steps L2-L6, L9; answering A3, A5; fsrs-transitions T4, T9) | `pylib/tests/test_schedv3.py` | Which behaviors to prove: previews match answers through custom learning and relearning steps (SM-2 multipliers excluded); first-step Hard gives the averaged delay with a negative-seconds review-log interval; Again sends a review card into relearning and Easy graduates it due today plus its interval. |
| sched/answer/review-ratings, sched/answer/previewed-states | `pylib/tests/test_schedv3.py` (test_reviews), `rslib/src/scheduler/answering/mod.rs` (state_application) | Which behaviors the harvest rows hf44d25b35e71 and hb46e7a2d004c oblige: per-rating review answers with counters, due, ease writes and leech suspend; and that an applied state equals the previewed state through learning, graduation, lapse, relearning and re-graduation. Expectations come only from the oracle goldens. |
| sched/leech/undo-with-buried-sibling (answering A7 atomic undo of leech, sibling burial, revlog and deck counters) | `rslib/src/scheduler/queue/undo.rs` | undo test: the set of rows one answer undo/redo must restore together (sibling queue, leech tag and registry, revlog row, deck studied counters, queue counts). Expected values come from the oracle golden. |
| Needs-op note for the leech status of a scheduling state (answer result leech / leech_states observation) | `rslib/src/scheduler/service/mod.rs` (state_is_leech service entry and its test); `pylib/anki/scheduler/v3.py` and `qt/aqt/reviewer.py` (grep only: where the reviewer asks for the leech status after an answer) | That the leech status is a separate read on the chosen state, used for the reviewer notice, and not part of the answer op's outputs. |
| Harvest rows habc95d948b8b, hb643b497ade7, hb8dcf303d0e0: existing scenarios sched/answer/positions-normal, sched/answer/new-card-ratings, sched/data/positions, sched/steps/hard-delays, sched/steps/new-card-buttons mapped as coverage | `rslib/src/scheduler/service/mod.rs` | Which answer behaviors the service tests require: Good twice from new to review, Easy on a new card graduates to review with an interval, and Hard vs Again on the first of two steps. Expectations come from existing oracle goldens. |

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

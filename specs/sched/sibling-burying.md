# Burying siblings when a card is answered

Spec: sched/sibling-burying
Requirements: R631, R632, R633, R721
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/bury/

Siblings are the other cards of the answered card's note. An answer can bury some of them until the next day, so that two cards of one note are not studied on the same day. This file says which ones and what is written. Skipping siblings while the queue is gathered is `sched/queue-building#B5` (U41): a sibling skipped there is written only here, when a card of its note is answered (R631).

Terms: the answered card's queue is its stored `queue` before the answer, whatever the session's own view of the card. Its home preset is the preset of its home deck (`odid` when it is not 0, else `did`), read with `sched/presets#V1`.

## B1. The options

Three booleans of the home preset decide (U42, `sched/presets`):

| Preset field | Schema-11 key | Buries siblings that are |
|---|---|---|
| `bury_new` | `new.bury` | new: `queue` 0 |
| `bury_reviews` | `rev.bury` | review: `queue` 2, whatever their `due` (also due in the future) |
| `bury_interday_learning` | `buryInterdayLearning` | in interday learning: `queue` 3 |

When all three are off, an answer buries nothing. The siblings' own presets play no part.

## B2. Which kinds an answer may bury

The answered card's queue removes kinds that the queue gathers before it (gathering order: intraday learning, interday learning, review, new; `sched/queue-building#B4`):

| Answered card's queue | New siblings | Review siblings | Interday learning siblings |
|---|---|---|---|
| 1 (intraday learning) or 4 (preview) | if `bury_new` | if `bury_reviews` | if `bury_interday_learning` |
| 3 (interday learning) | if `bury_new` | if `bury_reviews` | if `bury_interday_learning` |
| 2 (review) | if `bury_new` | if `bury_reviews` | never |
| 0 (new) | if `bury_new` | never | never |
| −1, −2, −3 (suspended or buried; Grade Now can answer them) | if `bury_new` | never | never |

So a review answer never buries an interday learning sibling and a new card's answer buries only new siblings (R632).

## B3. Which siblings

Every card of the same note other than the answered one whose current `queue` is one of the kinds B2 allows. In particular:

- siblings in any deck are buried, also in a deck of another preset and in a filtered deck (R633);
- a sibling in intraday learning (`queue` 1) or preview (`queue` 4) is never buried;
- a suspended sibling (`queue` −1) stays suspended, and a sibling already buried (−2) or buried by the user (−3) is left as it is (R633).

## B4. What is written

Each buried sibling gets `queue` −2, `mod` = the answer's `now_s` and `usn` −1. Its other columns (`type`, `due`, `ivl`, `odid`, `odue`, `data`, …) stay. Before writing any sibling to queue −2, the same answer transaction establishes today’s `lastUnburied` marker under `sched/day-clock#D8`. This native preservation rule prevents a later automatic check or same-day reopen from restoring newly buried cards. It writes no marker when no sibling is buried. The marker and siblings roll back together and share the answer’s undo/redo journal (`sched/answering#A7`, `#A14`). It does not create or repair `creationOffset`. Siblings are written in the answer's transaction (`sched/answering#A2`); no review-log row is written for them (R683).

## B5. Undo, redo and unburying

- The answer's undo step covers the siblings: undo writes each back exactly as it was, `queue`, `mod` and `usn` included; redo writes them as the undo found them (`engine/undo#A9`, R721).
- Buried siblings come back on the next day (`sched/day-clock`, the unbury at rollover) or through Unbury (U48), which restore the queue from `type`.

## B6. The answer's result

The answer reports the ids of the siblings it buried (`sched/answering#A13`), so that the session drops them from its queue (U43).

## B7. Examples

One note with cards c1 to c5 in a preset with all three options on; c1 is answered, and the other columns give each sibling's `queue` afterwards:

| c1's queue before | c2 new | c3 review due in 5 days | c4 interday learning | c5 intraday learning |
|---|---|---|---|---|
| 2 (review) | −2 | −2 | 3 | 1 |
| 0 (new) | −2 | 2 | 3 | 1 |
| 3 (interday learning) | −2 | −2 | −2 | 1 |

AE22: cards A (review, due today) and B (new) of one note, Bury new and Bury review on. Answering A Good writes B with `queue` −2; undoing the answer puts B back to `queue` 0 with its `mod` and `usn`, deletes A's review-log row and restores the deck counters (golden `sched/bury/ae22-undo`).

## B8. Scenarios

| Scenario | Covers |
|---|---|
| `sched/bury/ae22-undo` | AE22, B4, B5: the sibling's row, the answered card, its review-log row and the deck counters after the answer, after the undo and after the redo |
| `sched/bury/answer-kinds` | B2: answers from the review, new, interday learning, intraday learning and suspended queues, with all options on |
| `sched/bury/sibling-states` | B3: siblings in another deck of another preset, suspended, buried by the user, in intraday learning and due in the future; options off buries nothing |

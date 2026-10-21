# Answering a card

Spec: sched/answering
Requirements: R587, R589, R591, R632, R633, R663, R666, R668, R672, R673, R674, R675, R676, R677, R678, R681, R683, R684, R719, R721, R764, R1890
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/answer/, conformance/scenarios/sched/bury/, conformance/scenarios/sched/leech/, conformance/scenarios/sched/revlog/, conformance/scenarios/sched/loadbalancer/

What one answer writes to the collection: the card row, one review-log row, the studied-today counters of the card's decks, buried siblings, the `leech` tag, `col.mod` and one undo step. The scenario op `answer` (`specs/conformance/dsl.md` D7.2) is this operation.

The states an answer chooses between (the four next states of the card, and its current state) are computed by the next-state engine (U45: `sched/steps`, `sched/fsrs-transitions`, with the load balancer of `sched/load-balancer` as its fuzzed-day chooser) and reported by the `next_states` observation (`specs/conformance/observations.md` O5). This file takes the chosen state as given and says how it is written; A10 lists the files that cover the other parts of an answer.

Terms: `now_ms` is the clock's instant in epoch milliseconds and `now_s` = `now_ms` / 1000 rounded down (`storage/ids-usn-mtime`). `today` and `next_day_at` are the day clock's day number and next day boundary at `now_s` (`sched/day-clock`, U40; the `timing` observation). `r` = `next_day_at` − `now_s`, the seconds left until the next day starts (at least 1).

## A1. Scope and inputs

- **Inputs**: the card, the rating (Again 1, Hard 2, Good 3, Easy 4) and the time taken `m` in milliseconds (a non-negative integer; the scenario's `time_ms`, 0 when missing). The answer instant is `now_ms`; it is the review time the answer records.
- **Preset**: the card's home preset, read with the bounds of `sched/presets#V3` (`sched/presets#V1`; the home deck is `did`, since a card in a filtered deck is refused, A12). When no preset has the deck's preset id, the answer uses the built-in defaults, not the default preset that `decks/deck-presets` P3 falls back to for a deck (golden `sched/data/missing-preset`). This file reads its answer time cap (`cap_answer_time_to_secs`, schema-11 `maxTaken`: 1 to 9999 s, any other stored value reads as the default 60 s) and its desired retention and FSRS parameters (`sched/fsrs-transitions` T2).
- **FSRS.** Meguri always answers with FSRS on (Goal Capsule; R702 turns the `fsrs` key on). Anki 26.09.3 writes a few values differently when `fsrs` is off (A9); scenarios that answer cards set `fsrs: true` so that Meguri can match them.
- **Covered**: a card in a normal deck (`odid` 0) whose current state is new, learning, review or relearning, in any queue (a suspended or buried card can be answered through Grade Now, U48). Burying siblings, leeches and the load balancer are the files A10 names. A card in a filtered deck is refused (A12).
- A card whose deck does not exist cannot be answered: the operation fails with `NotFound` and writes nothing.

## A2. What one answer writes

One undoable operation, named "Answer Card", in one transaction:

1. the card row (A3, A4; `queue` −1 when the leech action suspends it, `sched/leeches#L3`);
2. one new review-log row (A5);
3. the studied-today counters of the card's deck and of its ancestors (A6);
4. the siblings the answer buries (`sched/sibling-burying`);
5. on a leech answer, the `leech` tag on the card's note and its registry row (`sched/leeches#L2`);
6. `col.mod` = `now_ms` (`storage/ids-usn-mtime` S5); `col.usn` is not touched;
7. one undo step (A7).

Nothing else changes: no unrelated note, card or tag. When at least one sibling is actually buried, D8 requires today’s `lastUnburied` marker in this same transaction before the buried queue writes. It is the only answer-owned config exception. A same-value marker need not be rewritten; an answer that buries no sibling adds no marker. See A13 for maintenance and A7 for undo. Outside the collection, a load-balancer cache that is present is updated after the commit (`sched/load-balancer#W8`). The collection commits these writes atomically; the native bury marker must be established before storing buried sibling queues (A13).

## A3. The card row

### A3.1 Every answer

| Column | Value |
|---|---|
| `reps` | stored value + 1 |
| `mod` | `now_s` |
| `usn` | -1 |
| `data` | A4 |
| `id`, `nid`, `did`, `ord`, `odue`, `odid`, `flags` | unchanged |

The card is written even when no other column changed (reps always does). Use the current U45 transition values and conversions; do not implement a second transition calculation here.

### A3.2 By the kind of the chosen state

The chosen state is one of `next_states`' `again`, `hard`, `good` or `easy` (O5): a learning state (`remaining_steps`, `scheduled_secs`, `memory_state`), a review state (`scheduled_days`, `ease_factor`, `lapses`, `memory_state`) or a relearning state (a learning part and a review part).

**Learning or relearning delay.** The delay `t` is the learning state's `scheduled_secs` (for a relearning state, its learning part's). When `t < r` the delay is intraday; otherwise it is `d = floor((t − r) / 86400) + 1` days. So a delay that reaches the next day boundary counts as at least one day, and a delay of `r + 86400` seconds or more as two.

| Column | Learning state | Review state | Relearning state |
|---|---|---|---|
| `type` | 1 | 2 | 3 |
| `queue` | 1 when intraday, 3 when in days | 2 | 1 when intraday, 3 when in days |
| `due` | intraday: `now_s` + the delay with learning fuzz (`sched/fuzz` Z7); in days: `today` + `d` | `today` + `scheduled_days` | as for a learning state |
| `ivl` | unchanged | `scheduled_days` | the review part's `scheduled_days` |
| `factor` | unchanged | `ease_factor` × 1000 | the review part's `ease_factor` × 1000 |
| `lapses` | unchanged | `lapses` | the review part's `lapses` |
| `left` | `remaining_steps` | 0 | the learning part's `remaining_steps` |

- `ease_factor × 1000` is computed in 32-bit float arithmetic and rounded to the nearest integer, halves away from zero (2.5 gives 2500, 2.35 gives 2350), kept within 0 to 65,535 (`sched/fsrs-transitions#T11`, which also gives the ease each rating chooses, R587).
- `left` holds the number of steps left as a plain integer. (Older clients stored a day count times 1000 on top; the next-state engine reads only the remainder modulo 1000, U45.)
- Learning fuzz applies only to the intraday `due`; the review-log row records the delay without it (A5).

## A4. Card data (R589, R591, R676)

The card's `data` text is written again per `model/card-data-json` C4 from these values:

| Key | Value |
|---|---|
| `pos` | when the current state is new: the card's new-card position, which is its `due` before the answer (0 if that is negative). Otherwise unchanged. |
| `s`, `d` | the chosen state's memory state (for a relearning state, its learning part's): stability and difficulty. C4 rounds them when written (4 and 3 decimals). |
| `dr` | the card's desired retention (`sched/fsrs-transitions` T2.2: the deck's own value, else the preset's). C4 rounds it to 2 decimals. |
| `decay` | from the preset's parameter list (`sched/fsrs-transitions` T2.1): 0.1542 for the empty list, 0.5 for 17 or 19 values, the 21st value as stored (not clamped) for 21 values. C4 rounds it to 3 decimals (0.1542 is written 0.154). |
| `lrt` | `now_s` |
| `cd`, unknown keys | unchanged (R591; C3 rule 7) |

So a new card answered with the default preset gets `{"pos":1,"s":2.3065,"d":2.118,"dr":0.9,"decay":0.154,"lrt":…}` when it was first in the new queue.

## A5. The review-log row (R681)

| Column | Value |
|---|---|
| `id` | `now_ms`, or the table's largest id plus one when `now_ms` is taken (`storage/ids-usn-mtime` I2, I3) |
| `cid` | the card's id |
| `usn` | -1 |
| `ease` | the rating: 1, 2, 3 or 4 |
| `ivl` | the chosen state's interval (below) |
| `lastIvl` | the current state's interval (below) |
| `factor` | the chosen state's difficulty, shifted (below) |
| `time` | `min(m, cap × 1000)`, with `cap` the preset's answer time cap in seconds (R678) |
| `type` | from the current state: 0 (learning) for new and learning, 1 (review) for review, 2 (relearning) for relearning |

**Intervals.** A state's interval for the review log is:

- new: 0;
- learning or relearning: its delay `t` (for relearning, the learning part's `scheduled_secs`), as A3.2 classifies it: intraday gives `−t` (negative seconds, the delay without learning fuzz); in days gives `+d`;
- review: `+scheduled_days`.

For the current state, `t` is the current step's delay that `next_states`' `current` reports, and the classification uses the same `r` as the chosen state's.

The exact stored `ivl` and `lastIvl` conversions are the current `sched/fsrs-transitions#T15` review-log preview. Negative seconds saturate at −2,147,483,647; day intervals retain their low 32 bits as a signed integer, including imported current intervals. This table describes units, not a replacement conversion rule.

**Factor.** With the memory state (stability `S`, difficulty `D`) of the chosen state (for a relearning state, its learning part's): `((D − 1) / 9 + 0.1) × 1000`, each operation in 32-bit float arithmetic in that order, rounded to the nearest integer with halves away from zero. So difficulty 1 gives 100 and difficulty 10 gives 1100. Example: `D` = 2.118104 gives 224.

A review-log row is only ever inserted by an answer; Meguri never rewrites one (`model/row-records` R5). Its undo deletes it (A7, R684).

## A6. Studied-today counters (R677)

The decks: the card's deck (`did`), then its parent, then the parent's parent, up to the top level (`decks/decks` D3). The walk stops at the first parent name that names no deck: for `A::B::C` with no `A::B`, only `A::B::C` is counted, even when `A` exists.

For each of these decks, in its `common` blob (`model/protobuf-records` P6.4):

1. When `last_day_studied` is not `today`: `new_studied`, `review_studied`, `learning_studied` and `milliseconds_studied` become 0 and `last_day_studied` becomes `today`.
2. By the card's `queue` before the answer: 0 (new) adds 1 to `new_studied`; 2 (review) or 3 (interday learning) adds 1 to `review_studied`; any other queue adds to neither. An intraday learning answer uses no daily limit.
3. `milliseconds_studied` += the review-log row's `time` (A5, already capped).

The deck row gets `mtime_secs` = `now_s` and `usn` -1, and its `common` blob is encoded again (`model/protobuf-records` P3, P5) with every other field as read: the collapse flags, `other` and unknown fields (P4). Its `name` and `kind` are written as stored: answering does not apply `decks/decks` D1's stored-name normalization or D2's unique-name rule.

Each of these decks is written with `mtime_secs` = `now_s` and `usn` −1 even when no counter changed (an intraday learning answer of 0 ms on a day the deck was already studied), as Anki does (`sched/presets#D4` item 4). The answer hands the row over already stamped, so `storage/ids-usn-mtime#S4` skips the write only when the stamped row equals the stored one, which no observation can tell from Anki's write (golden `sched/answer/unchanged-counters`).

Example: a new card of `Lang::Spanish::Verbs` answered in 95 s with the default cap, on a day when none of the three decks was studied yet: each of `Lang::Spanish::Verbs`, `Lang::Spanish` and `Lang` gets `last_day_studied` = today, `new_studied` 1, `review_studied` 0, `milliseconds_studied` 60000. In the schema-11 deck object (`model/schema11-json` S7.1) that is `newToday` `[today, 1]` and `timeToday` `[today, 60000]`.

## A7. `col.mod` and the undo step (R719, R684)

- `col.mod` becomes `now_ms` when the operation commits.
- The step is named "Answer Card" (`specs/conformance/undo-ops.md` V4 id `answer_card`), kind `Answer` (`engine/undo` M1). It covers the rows of A3 to A6, the buried siblings, the note and tag registry rows of a leech answer, and `col`. Like every operation, the answer empties the redo history (`engine/undo` A2).
- **Undo** restores any answer-owned `lastUnburied` change with its original bytes and stamps, or removes the key if this answer inserted it. It does not undo day-maintenance work committed before the answer. The marker is an ordinary journal row of this answer, not an undo-free exception. Undo also writes the card row, the deck rows, the siblings and the note back exactly as they were, `mod`, `mtime_secs` and `usn` included, deletes the review-log row and a tag row the answer registered, and sets `col.mod` back (`engine/undo` A7, A9; R721). So a buried sibling is back in its queue, a suspension by the leech action is gone and the deck counters are as before (AE22).
- **Redo** writes the rows back as the undo found them, inserting the review-log row again under its id (`engine/undo` A8 when another writer took that id).
- The answer's restorer (`engine/undo#M10`) handles what lies outside the collection: on undo it removes the card from the load-balancer cache (`sched/load-balancer#W9`) and gives U43 the answer metadata of A13 to restore its queue and learning cutoff; on redo it does nothing to the cache.

## A8. Examples (oracle)

A new card answered Good with the default preset (learning steps 1 m and 10 m), FSRS on, default parameters, load balancer off. Its chosen state (`next_states` `good`) is learning with `remaining_steps` 1, `scheduled_secs` 600 and memory state (2.3065, 2.118104).

From `conformance/goldens/smoke/fsrs-new-card.json`: card id 1894698000000, answered at 1894698000 (2030-01-15 10:00 Europe/Paris) with 4000 ms. `r` is 64800, so the delay is intraday.

| Card column | Value |
|---|---|
| `type`, `queue` | 1, 1 |
| `due` | 1894698696: 600 s plus 96 s of learning fuzz |
| `ivl`, `factor`, `lapses` | 0, 0, 0 (unchanged) |
| `reps` | 1 |
| `left` | 1 |
| `mod`, `usn` | 1894698000, -1 |
| `data` | `{"pos":1,"s":2.3065,"d":2.118,"dr":0.9,"decay":0.154,"lrt":1894698000}` |

| Review-log column | Value |
|---|---|
| `id` | 1894698000000 |
| `ease`, `ivl`, `lastIvl` | 3, -600, 0 |
| `factor` | 224 |
| `time`, `type`, `usn` | 4000, 0, -1 |

In `sched/answer/new-card-ratings` four new cards of the deck `Inbox` are answered at 2030-01-15 11:00 UTC (1894705200), one rating each. The Good answer of `#c3` (card id 1894701600002, 3500 ms) writes the same `type`, `queue`, `ivl`, `factor`, `lapses`, `reps`, `left` and memory values as above, with `due` 1894705870 (600 s plus 70 s of learning fuzz), `pos` 3, `lrt` and `mod` 1894705200, and a review-log row with id 1894705200002 (the two answers before it took the instant and the next millisecond), `ease` 3, `ivl` -600, `lastIvl` 0, `factor` 224 and `time` 3500. Afterwards `col.mod` is 1894705200000, and `Inbox` has `mtime_secs` 1894705200, `usn` -1, `newToday` `[0, 4]` and `timeToday` `[0, 12000]`. `undo/revlog-taken-id` answers card 1894701600000 Good at the same instant (its fuzz is `sched/fuzz` Z7's first example) and then undoes and redoes the answer (A7).

## A9. Anki with FSRS off

Meguri never answers with FSRS off. For reading goldens recorded with `fsrs: false` (`smoke/sm2-new-card`, `storage/stamps/answer` and the other storage scenarios that answer cards): Anki then has no memory state, so `s`, `d` and `dr` are removed from the card data and `decay` stays as it was; the review-log `factor` is 0 for a learning state and the review part's `ease_factor` × 1000 for review and relearning states; and the states themselves come from the SM-2 rules (out of Meguri's scope). A scenario whose observations show these values cannot pass on Meguri until it sets `fsrs: true`.

## A10. The other parts of an answer (U44)

| Part | File |
|---|---|
| burying siblings when the preset's bury options are on (R632, R633) | `sched/sibling-burying` |
| a leeched chosen state: the `leech` tag, the suspend action and the result flags (R672, R673) | `sched/leeches` |
| the load balancer's choice of day and its cache (R666 to R669) | `sched/load-balancer` |
| which review-log rows Meguri writes and keeps (R683, R684) | `sched/revlog` |
| cards in filtered decks | A12: refused |
| decks stamped when no counter changes | A6: stamped, as Anki does |

## A11. Scenarios

| Scenario | Covers |
|---|---|
| `sched/answer/new-card-ratings` | A2 to A7 for new cards: one rating each at one instant (review-log ids bumped), the card rows, the review-log rows, the deck counters and stamps, `col.mod` |
| `sched/answer/state-kinds` | A3.2, A5: learning delays intraday and in days, a learning card graduating, a review card lapsing into relearning and one answered Hard, `lastIvl` and `type` by current state |
| `sched/answer/deck-counters` | A6: a nested deck and its ancestors, a missing parent, the time cap, the queue before the answer (new, review, interday and intraday learning), the reset on a later day |
| `sched/answer/unchanged-counters` | A6: a deck stamped by an answer that changes none of its counters |
| `sched/answer/custom-data` | A4, A15: `cd` kept byte for byte after the rewritten keys, on a new and a review card, unchecked even beyond the custom-data limits |
| `smoke/fsrs-new-card` | A8's example |
| `undo/revlog-taken-id` | A7: undo and redo of an answer |
| `sched/bury/*`, `sched/leech/*`, `sched/revlog/*`, `sched/loadbalancer/*` | the files of A10 |

## A12. Cards in filtered decks

Meguri cannot study filtered decks (R53, R706). A card whose `odid` is not 0 has no next states in Meguri: the next-state engine refuses it with `InFilteredDeck` (`sched/fsrs-transitions#T14`), and the answer operation refuses it with the same error and writes nothing. Review-log rows of type 3 are therefore never written (`sched/revlog#E1`). Anki answers such cards with its filtered-deck states (rescheduling and preview), which no Meguri scenario uses.

## A13. The answer operation (engine)

- **Op.** Source `UserUndoable` (`engine/op-model#O1`), step name "Answer Card", kind `Answer`.
- **Input.** The card id; the chosen state (one of the four next states the engine computed for the card, with the load balancer when one is present); the answer instant `answered_at` in epoch milliseconds (`now_ms`); the time taken in milliseconds (before the cap of A5); the session's queue kind the card was taken from (new, learning or review), or none outside a session (Grade Now). The queue kind is only passed through to the undo metadata: counters (A6) and burying (`sched/sibling-burying#B2`) use the card's stored `queue` before the answer.
- **Maintenance and current reads.** Before submitting the answer operation, the caller completes `sched/day-clock#D8` automatic maintenance and passes the resulting day and boundary. The writer must not submit another maintenance operation and wait. Answering never adds or changes `creationOffset`; a legacy collection keeps its legacy day calculation. Read the current card, home deck and bounded home preset inside the write transaction; a missing preset uses built-in defaults (A1). U45/U46 remain the single transition/memory providers. U43 owns refresh of predictions and session tokens after preset or generation changes.
- **Check.** The card row must be the one the states were computed from: the session passes its `stateToken`, a `RowDigest` precondition on the card row (`engine/op-model#O7`; R727, U43). When the row changed, the op is `Stale` and writes nothing. Siblings, deck rows, the note and the tag registry are read inside the write transaction (re-derived, O7).
- **Writes.** In one transaction, in A2's order: the card row (A3, A4), the review-log row (A5), the deck rows (A6), today’s `lastUnburied` marker before any newly buried sibling queues (`sched/day-clock#D8`), the buried siblings (`sched/sibling-burying`), the note and tag row of a leech answer (`sched/leeches#L2`), `col.mod`. Any failure rolls all of them back (A14).
- **Result.** The change set (`engine/change-stream`); `leech` and `suspended` (`sched/leeches#L5`); the ids of the buried siblings (`sched/sibling-burying#B6`); the review-log id written.
- **Undo metadata.** Attached to the step for the restorer (A7): the card id, the session queue kind, whether a load-balancer cache was present and which build it was (`sched/load-balancer#W9`), and the session's learning cutoff before the answer (U43).
- **After the commit.** The load-balancer cache, when present, gets the card (`sched/load-balancer#W8`).

## A14. One transaction

If any write of A13 fails, nothing of the answer persists: not the card, the review-log row, a deck, a sibling, the note's tags, a tag row, the answer-owned bury marker or `col.mod`, and the load-balancer cache is not updated. The required fault test uses U24’s hook to fail the review-log insert and checks every affected row and cache state (`core/crates/meguri-engine/tests/answer.rs`). This is required evidence, not a claim that the test already exists. Own tests must also cover marker insertion/update/no-op, rollback, undo/redo, a same-day reopen and the no-sibling case. Keep the existing legacy-timing and taken-review-log-id cases, including their three established undo deviations.

## A15. Custom scheduling and custom data (R591, R764)

- The custom scheduling JavaScript (config key `cardStateCustomizer`) is never run or read when answering: a collection that sets it is scheduled as if it did not (R764, deviation from Anki desktop, whose reviewer runs it). The key itself is kept and synced (R1484).
- The answer never writes the card's custom data `cd` (R591, A4): it stays byte for byte, also when another client's custom scheduler set it, and its content is not checked (`model/card-data-json` C4: the limits apply only when a custom scheduler sets it). Example (golden `sched/answer/custom-data`): a new card whose data is `{"cd":"{\"src\":\"other client\", \"n\":1}"}`, answered Good, is written `{"pos":1,"s":2.3065,"d":2.118,"dr":0.9,"decay":0.154,"lrt":1894705200,"cd":"{\"src\":\"other client\", \"n\":1}"}`.

# Answering a card

Spec: sched/answering
Requirements: R587, R589, R591, R663, R675, R676, R677, R678, R681, R684, R719, R1890
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/answer/

What one answer writes to the collection: the card row, one review-log row, the studied-today counters of the card's decks, `col.mod` and one undo step. The scenario op `answer` (`specs/conformance/dsl.md` D7.2) is this operation.

The states an answer chooses between (the four next states of the card, and its current state) are computed by the next-state engine (U45: `sched/steps`, `sched/fsrs-transitions`) and reported by the `next_states` observation (`specs/conformance/observations.md` O5). This file takes the chosen state as given and says how it is written. U44 extends it (A10).

Terms: `now_ms` is the clock's instant in epoch milliseconds and `now_s` = `now_ms` / 1000 rounded down (`storage/ids-usn-mtime`). `today` and `next_day_at` are the day clock's day number and next day boundary at `now_s` (`sched/day-clock`, U40; the `timing` observation). `r` = `next_day_at` − `now_s`, the seconds left until the next day starts (at least 1).

## A1. Scope and inputs

- **Inputs**: the card, the rating (Again 1, Hard 2, Good 3, Easy 4) and the time taken `m` in milliseconds (a non-negative integer; the scenario's `time_ms`, 0 when missing). The answer instant is `now_ms`; it is the review time the answer records.
- **Preset**: the preset of the card's deck (`decks/deck-presets` P3). This file reads its answer time cap (`cap_answer_time_to_secs`, schema-11 `maxTaken`, default 60 s) and its desired retention and FSRS parameters (`sched/fsrs-transitions` T2).
- **FSRS.** Meguri always answers with FSRS on (Goal Capsule; R702 turns the `fsrs` key on). Anki 26.09.3 writes a few values differently when `fsrs` is off (A9); scenarios that answer cards set `fsrs: true` so that Meguri can match them.
- **Covered here**: a card in a normal deck (`odid` 0) whose current state is new, learning, review or relearning, answered with the load balancer off, with the preset's three bury options off and a chosen state that is not leeched. A10 lists what U44 adds.
- A card whose deck does not exist cannot be answered: the operation fails with `NotFound` and writes nothing.

## A2. What one answer writes

One undoable operation, named "Answer Card", in one transaction:

1. the card row (A3, A4);
2. one new review-log row (A5);
3. the studied-today counters of the card's deck and of its ancestors (A6);
4. `col.mod` = `now_ms` (`storage/ids-usn-mtime` S5); `col.usn` is not touched;
5. one undo step (A7).

Within A1's scope nothing else changes: no note row, no tag, no config key, no other card.

## A3. The card row

### A3.1 Every answer

| Column | Value |
|---|---|
| `reps` | stored value + 1 |
| `mod` | `now_s` |
| `usn` | -1 |
| `data` | A4 |
| `id`, `nid`, `did`, `ord`, `odue`, `odid`, `flags` | unchanged |

The card is written even when no other column changed (reps always does).

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

- `ease_factor × 1000` is computed in 32-bit float arithmetic and rounded to the nearest integer, halves away from zero (2.5 gives 2500, 2.35 gives 2350).
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

**Factor.** With the memory state (stability `S`, difficulty `D`) of the chosen state (for a relearning state, its learning part's): `((D − 1) / 9 + 0.1) × 1000`, each operation in 32-bit float arithmetic in that order, rounded to the nearest integer with halves away from zero. So difficulty 1 gives 100 and difficulty 10 gives 1100. Example: `D` = 2.118104 gives 224.

A review-log row is only ever inserted by an answer; Meguri never rewrites one (`model/row-records` R5). Its undo deletes it (A7, R684).

## A6. Studied-today counters (R677)

The decks: the card's deck (`did`), then its parent, then the parent's parent, up to the top level (`decks/decks` D3). The walk stops at the first parent name that names no deck: for `A::B::C` with no `A::B`, only `A::B::C` is counted, even when `A` exists.

For each of these decks, in its `common` blob (`model/protobuf-records` P6.4):

1. When `last_day_studied` is not `today`: `new_studied`, `review_studied`, `learning_studied` and `milliseconds_studied` become 0 and `last_day_studied` becomes `today`.
2. By the card's `queue` before the answer: 0 (new) adds 1 to `new_studied`; 2 (review) or 3 (interday learning) adds 1 to `review_studied`; any other queue adds to neither. An intraday learning answer uses no daily limit.
3. `milliseconds_studied` += the review-log row's `time` (A5, already capped).

The deck row gets `mtime_secs` = `now_s` and `usn` -1, and its `common` blob is encoded again (`model/protobuf-records` P3, P5) with every other field as read: the collapse flags, `other` and unknown fields (P4). Its `name` and `kind` are written as stored: answering does not apply `decks/decks` D1's stored-name normalization or D2's unique-name rule.

Example: a new card of `Lang::Spanish::Verbs` answered in 95 s with the default cap, on a day when none of the three decks was studied yet: each of `Lang::Spanish::Verbs`, `Lang::Spanish` and `Lang` gets `last_day_studied` = today, `new_studied` 1, `review_studied` 0, `milliseconds_studied` 60000. In the schema-11 deck object (`model/schema11-json` S7.1) that is `newToday` `[today, 1]` and `timeToday` `[today, 60000]`.

## A7. `col.mod` and the undo step (R719, R684)

- `col.mod` becomes `now_ms` when the operation commits.
- The step is named "Answer Card" (`specs/conformance/undo-ops.md` V4 id `answer_card`), kind `Answer` (`engine/undo` M1). It covers the rows of A3 to A6 and `col`. Like every operation, the answer empties the redo history (`engine/undo` A2).
- **Undo** writes the card row and the deck rows back exactly as they were, `mod`, `mtime_secs` and `usn` included, deletes the review-log row and sets `col.mod` back (`engine/undo` A7, A9).
- **Redo** writes the rows back as the undo found them, inserting the review-log row again under its id (`engine/undo` A8 when another writer took that id).

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

## A10. Not covered yet (U44)

U44 adds, as further sections of this file (or `sched/revlog`, `sched/sibling-burying`, `sched/leeches`, `sched/load-balancer`):

- cards in filtered decks (the filtered deck's counters, rescheduling and preview states, review-log type 3);
- burying siblings when the preset's bury options are on (R632, R633);
- a leeched chosen state: the `leech` note tag and the suspend action (R672);
- the load balancer's choice of day for review states and its cache (R666);
- whether an answer that changes a deck's counters by nothing (an intraday learning answer of 0 ms on the same day) still stamps the deck: Anki stamps it; `storage/ids-usn-mtime` S4 skips it, which a scenario would show as a deviation.

## A11. Scenarios

| Scenario | Covers |
|---|---|
| `sched/answer/new-card-ratings` | A2 to A7 for new cards: one rating each at one instant (review-log ids bumped), the card rows, the review-log rows, the deck counters and stamps, `col.mod` |
| `sched/answer/state-kinds` | A3.2, A5: learning delays intraday and in days, a learning card graduating, a review card lapsing into relearning and one answered Hard, `lastIvl` and `type` by current state |
| `sched/answer/deck-counters` | A6: a nested deck and its ancestors, a missing parent, the time cap, the queue before the answer (new, review, interday and intraday learning), the reset on a later day |
| `smoke/fsrs-new-card` | A8's example |
| `undo/revlog-taken-id` | A7: undo and redo of an answer |

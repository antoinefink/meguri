# Card scheduling fields and card data

Spec: sched/card-data
Requirements: R584, R585, R586, R588, R589, R590, R591, R647, R648
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/data/

What the scheduling columns of a card row and the keys of its `data` mean to the scheduler, which operations set or clear each one, and two helpers other scheduling units build on: restoring a card's queue from its type (K3) and saving a new card's position (K6). Column types and read tolerances are `model/row-records#R4`; the `data` text format (key order, rounding, number text, unknown keys) is `model/card-data-json` and `model/json-numbers`. This file never changes those formats. "Today" and day numbers are `sched/day-clock`'s; the preset, its parameters and the desired retention are `sched/presets`.

## K1. Type and queue (R584)

| `type` | Card type |
|---|---|
| 0 | new |
| 1 | learning (never graduated) |
| 2 | review |
| 3 | relearning (lapsed, back in steps) |

| `queue` | Queue | Count it adds to (U41) |
|---|---|---|
| 0 | new | new |
| 1 | intraday learning: the current step ends before the next day boundary (U44 decides) | learning |
| 2 | review | review |
| 3 | interday learning: the current step ends on a later day | review |
| 4 | preview repeat: a card a filtered deck without rescheduling showed again after Again | learning |
| -1 | suspended | none |
| -2 | buried by the scheduler (sibling burying) | none |
| -3 | buried by the user | none |

- Meguri's card model holds `type` and `queue` as two enums with exactly these numbers and writes them back as these numbers. A stored value outside the lists makes the row unreadable (`model/row-records#R4`); the scheduler never sees one.
- The scheduler does not check that `type` and `queue` go together. Usual pairs: type 0 with queue 0, types 1 and 3 with queue 1 or 3 (or 4 in a preview deck), type 2 with queue 2, and any type with -1, -2 or -3. A row holding another pair is kept as stored and handled by its queue for counting and by its type for scheduling.

## K2. `due` and the filtered-deck columns (R585)

| `queue` | `due` holds |
|---|---|
| 0 | the new-card position (K7) |
| 1, 4 | the epoch second at which the step ends |
| 2, 3 | the day number on which the card is due |
| -1, -2, -3 | the `due` of the queue K3 gives back; suspending or burying never changes `due` |

- **Timestamp or day.** Where a `due` has to be read without its queue (K3, and orderings that mix queues: the browser's due column, filtered-deck gathering, set due date on a learning card), a value greater than 1,000,000,000 is an epoch second and any other value a day number. 1,000,000,000 itself is a day number; 1,000,000,001 is a timestamp. No day number reaches that size (day 1,000,000,000 is about 2.7 million years after creation), and every epoch second since 2001-09-09 is above it.
- **Filtered decks.** A card that a filtered deck holds has `did` = the filtered deck, `odid` = its home deck and `odue` = the `due` it had in its home deck (0 when it had none to keep). Outside filtered decks `odid` and `odue` are 0. The card's home deck is `odid` when it is not 0, otherwise `did`; the home deck gives the preset (`sched/presets#V1`).
- A new card's `due` in a filtered deck is its place in the filtered deck; its position stays in `odue`.

## K3. Restoring a queue from the type

Unsuspend, unbury (by the user, and at the start of a day, `sched/day-clock#D7`) and moving a card out of a filtered deck put the card back in the queue its type and `due` call for:

| `type` | `due` | `queue` becomes |
|---|---|---|
| 0 | any | 0 |
| 2 | any | 2 |
| 1 or 3 | above 1,000,000,000 | 1 |
| 1 or 3 | 1,000,000,000 or less | 3 |

- The helper changes `queue` only. Whether the card is stamped (`mod`, `usn`) is the caller's rule: the unbury at the start of a day does not stamp (R599); unsuspend and manual unbury do.
- Unsuspend and unbury apply it only to a card whose queue is -1, -2 or -3 (which of these an operation selects is its own rule); a card in any other queue is left as it is and counts as unchanged.
- **Leaving a filtered deck** (emptying or deleting the filtered deck, Change Deck, U48): `did` becomes `odid`; when `odue` is not 0, `due` becomes `odue`; when `queue` is 0 or above, K3's table then sets it from the type and the restored `due`; a negative queue (suspended or buried in the filtered deck) stays; `odid` and `odue` become 0.

| Card | After restore |
|---|---|
| type 1, `due` 1,700,000,000 | queue 1 |
| type 1, `due` 450 | queue 3 |
| type 3, `due` 1,000,000,000 | queue 3 |
| type 3, `due` 1,000,000,001 | queue 1 |
| type 0, `due` 12 | queue 0 |
| type 2, `due` 30 | queue 2 |

The day-clock scenario `sched/unbury-on-rollover/both-kinds` covers every row through the unbury at the start of a day (golden recorded by U40).

## K4. The other columns (R586)

- `ivl`: the review interval in days of a review or relearning card; 0 for new cards and for learning cards that never graduated.
- `factor`: the SM-2 ease in permille (2500 = 250 %); 0 for a new card. Meguri keeps it moving as R587 says (U45); it never schedules with it.
- `reps`, `lapses`: answers and lapses counted by the answer (U44).
- `left`: the (re)learning steps still to do. Older clients packed a second number into it; only `left` modulo 1000 counts (2002 means 2 steps). Meguri writes the plain count: the number of steps left for a learning or relearning card, 0 for new and review cards.
- `odue`, `odid`: K2. Meguri never creates filtered decks but keeps both columns on every card it rewrites (R1483).
- `flags`: the low 3 bits are the user flag (U30); the other bits are kept as stored.
- No scheduling operation repurposes a column for something else (R586).

## K5. Card data keys (R588, R591)

The scheduler's meaning of each key of `cards.data` (format: `model/card-data-json` C2 to C4), and which operations change it. A key an operation does not list keeps its value; the format rewrites the whole text every time a card row is written (rounding, key order, unknown keys at the end: `model/card-data-json` C4).

| Key | Meaning | Set by | Removed by |
|---|---|---|---|
| `pos` | the card's new-card position, saved when it left the new state | K6 | Forget (K6) |
| `s`, `d` | FSRS memory state: stability in days, difficulty from 1 to 10 | every answer (the memory state of the new state, U45); memory-state computation (U46) | Forget; Change Deck (U46 then computes it again in an FSRS deck); U46's recompute (`sched/memory-state#M4.1`) of a processed card whose review-log rows give no memory item, which still writes `dr` and `decay` (goldens `sched/memory/recompute` c4, `sched/compat/enable-fsrs` hotel) |
| `dr` | the desired retention the card was last scheduled with | every answer that schedules the card (not a preview answer, K5.1): the effective desired retention of `sched/presets#V7`; U46 | Change Deck |
| `decay` | the FSRS decay the card was last scheduled with | every answer, preview answers included: the decay of the home preset's parameters (`sched/presets#V6`); U46 | Change Deck |
| `lrt` | the time of the last answer, in whole epoch seconds | every answer except a preview answer: the answer's instant; Check Database's repair (`sched/memory-state#M6`, U128): the second of the latest graded row | Check Database's repair only (`sched/memory-state#M6`), for a card that is not new and has review-log rows: when none of its rows is graded, or when a Reset marker without a button follows its latest graded row (golden `sched/memory/lrt-repair`) |
| `cd` | data of another client's custom scheduler: a string holding JSON | never by Meguri (R591) | never by Meguri |
| other keys | another client's data | never | never (Meguri keeps them, `model/card-data-json` C3 rule 7; Anki drops them) |

- The values come from the card's home deck and its preset, also while the card sits in a filtered deck.
- `decay` is the parameter value as stored, before the FSRS crate clamps it to its own range: a preset whose 21st parameter is 0.05 writes `"decay":0.05` although the model computes with 0.1 (`sched/presets#V6`).
- Forget (U48) removes `s`, `d` and `pos` and keeps `dr`, `decay` and `lrt`.
- Change Deck removes `s`, `d`, `dr` and `decay` (and returns a card from a filtered deck, K3); with FSRS on, U46 then computes the memory state of the moved cards that have none.
- 2026-11-17: an earlier table said nothing removes `lrt` and listed only Forget and Change Deck as removing `s` and `d`, against `sched/memory-state` M4.1 and M6. Those sections decide; implemented code must be checked: any data-model test or invariant that `lrt` is never removed, or that a recompute keeps `s` and `d`.
- Meguri always schedules with FSRS (R702). Another client answering with FSRS off removes `s`, `d` and `dr` and keeps `decay`; Meguri reads such a card as one without a memory state (U46 computes one).

### K5.1 Preview answers

A filtered deck without rescheduling shows cards in preview: the answer moves the card within the filtered deck and leaves its scheduling alone. Such an answer sets `decay` (the state is computed with the home preset), and sets neither `dr`, `lrt`, `pos` nor the memory state.

### K5.2 Examples

From the goldens (FSRS on, answered Good on 2030-01-15 10:00 UTC, epoch second 1894701600):

| Card before | Home preset | `data` after |
|---|---|---|
| new, position 1 | 21 parameters ending in 0.3, desired retention 0.85 | `{"pos":1,"s":2.3065,"d":2.118,"dr":0.85,"decay":0.3,"lrt":1894701600}` |
| new, position 3 | FSRS-5 list only (19 values) | `{"pos":3,"s":3.173,"d":5.282,"dr":0.9,"decay":0.5,"lrt":1894701600}` |
| new, position 7 | no parameters | `{"pos":7,"s":2.3065,"d":2.118,"dr":0.9,"decay":0.154,"lrt":1894701600}` |
| review, `{"s":3.5,"d":6.25,"cd":"{\"k\":true,\"n\":[1,2]}","lrt":1894442400,"dr":0.8,"decay":0.2,"pos":4}` | defaults | `{"pos":4,"s":9.9987,"d":6.239,"dr":0.9,"decay":0.154,"lrt":1894701600,"cd":"{\"k\":true,\"n\":[1,2]}"}` |
| intraday learning, `{"pos":9,…,"cd":"{\"step\":1}"}` | defaults | `{"pos":9,"s":2.3065,"d":2.111,"dr":0.9,"decay":0.154,"lrt":1894701600,"cd":"{\"step\":1}"}` |
| review with unknown keys `"xyz":[1,2]` and `"zz":"x"` | defaults | Anki drops both; Meguri keeps them after `cd` (deviation `card-data-unknown-keys-answer`) |

The values of `s` and `d` are U45's; they appear here only as the goldens hold them.

## K6. The new position, `pos` (R589)

A card leaves the new state when an operation gives a card of type 0 another type. Each time it does, its position is saved in `pos`; the key is removed again when the card is made new (Forget). The rules by operation:

1. **Answer** (U44). When the card's type is 0 and the answer gives it a learning, review or relearning state, `pos` = its position, which is:
   - in a normal deck: `due`;
   - in a filtered deck (the card's `did` is a filtered deck): `odue` when it is not 0, otherwise `due`;
   - and 0 when that value is negative.
   The filtered/preview branches describe reference behavior and stored-data interoperability. Meguri does not answer a card in filtered membership: `sched/answering#A12` returns `InFilteredDeck` without writes. This rule does not remove the stored original-position facts used by other operations. A preview answer (K5.1) saves nothing; an answer that leaves the card new saves nothing.
2. **Set due date** (U48). When the card's type is 0 (whatever its queue: suspended and buried new cards too), `pos` = `odue` when `odid` is not 0, otherwise `due`, read as an unsigned 32-bit number: a negative value wraps (-1 gives 4294967295), which is the position K7 stores as that negative `due`. For any other type `pos` keeps its value, present or not.
3. **Forget** (U48). `pos` is removed. With "Restore original position", the card's new `due` is its last position when it has one: for a card of type 0 its current position (rule 2's value), otherwise `pos` when present; else the position U48 assigns.
4. Nothing else writes or removes `pos`: an answer of a card that is not new keeps it, and so do Change Deck, suspend, bury, reposition and every edit.

| Card | Operation | `pos` after |
|---|---|---|
| new, `due` 41 | answer Good | 41 |
| new, `due` -1 (position 4294967295) | answer Good | 0 |
| new in filtered membership, `odue` 7 | reference answer Good; Meguri refuses (A12) | reference `pos` 7; Meguri unchanged |
| learning, `pos` 41 | answer Good (graduates) | 41 |
| new, `due` -1 | set due date | 4294967295 |
| review, no `pos` | set due date | absent |
| review, `pos` 12 | Forget | absent; `due` 12 with "Restore original position" |

The first four reference rows are recorded in `sched/data/positions`. That full mixed-scope case requires `answer.filtered` and stays product-pending. `sched/answer/positions-normal` covers the supported normal-deck rows separately. Set Due Date and Forget retain U48’s real-operation evidence obligations.

## K7. New positions and the counter (R590)

- The cards of one note created together share one position, and the collection key `nextPos` gives it: read as an unsigned 32-bit integer (anything else reads as 0), then written back as that value plus one, wrapping from 4294967295 to 0 (`sched/config-keys#G2`). A note that already has a new card gives its new cards that card's position without touching the counter. The full rule, with random insertion order, is `notes/deck-placement-and-position` P1 to P4, which U27's card generation already implements. Meguri has one implementation of P1's read and write of the counter: the scheduling model and any later operation that draws a position from `nextPos` (U48) use that one rather than a second copy.
- A position is stored in `due` (or `odue`) as a signed 32-bit value: positions from 2147483648 up are stored as the position minus 4294967296 (`notes/deck-placement-and-position` P3).

| `nextPos` before | Cards of the added note | `nextPos` after |
|---|---|---|
| 41 | both at 41 | 42 |
| 4294967295 | both at `due` -1 | 0 |
| `"x"` (a string) | both at 0 | 1 |

(Golden of `sched/data/positions`.)

## K8. Scenarios

| Scenario | Contract | Evidence |
|---|---|---|
| `sched/data/answer-data` | K5; `sched/presets#V6`, V7 | recorded |
| `sched/data/answer-keeps-cd` | K5; retained `cd`/`pos`; unknown keys | recorded; deviation `card-data-unknown-keys-answer` |
| `sched/data/positions` | K6, K7 | `conformance/answer-ops#H6` |
| `sched/answer/positions-normal` | K6 rules 1 and 4; K7 | `conformance/answer-ops#H6` |
| `sched/unbury-on-rollover/both-kinds` (U40) | K3 | recorded |

Answer-case acceptance requires the real answer adapter and H6 evidence. H6 controls the two position cases; A12 controls refusal; D8.1 controls capability preflight.

Existing recorded model-test inputs:

| Case | `data` observations |
|---|---|
| `sched/data/answer-data` | step 12 |
| `sched/data/answer-keeps-cd` | step 6 |
| `sched/data/positions` | steps 15, 18 |

Use each golden's corresponding card values; its memory states are U45 results.

Harvest row `h151bd9c1b80b` remains deferred for the existing U48/U246 current/original/saved-position follow-up, including real Reset/Set Due Date operations. Normal-answer proof does not close that whole obligation.

Meguri's own tests (`core/crates/meguri-sched/tests/card_data_roundtrip.rs`) add what no golden shows: a property test that an answer's rewrite of `s`, `d`, `dr`, `decay` and `lrt` leaves every other key, `cd` and unknown keys included, byte for byte the same over 10,000 random inputs (R588); K3's table on the helper itself; K6 rules 2 and 3 until U48's scenarios exist.

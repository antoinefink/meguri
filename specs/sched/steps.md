# Learning and relearning steps

Spec: sched/steps
Requirements: R601, R636, R638, R639, R640, R641, R642, R643, R655, R663
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/steps/

How a card moves through the learning steps of its home preset (and a lapsed card through the relearning steps): the current step, the delay each rating gives, when a delay becomes whole days, and the elapsed time the current state reports. What happens when a rating leaves the steps is FSRS's (`sched/fsrs-transitions` T8 to T10). The states and their fields are those of `specs/conformance/observations.md` O5: a learning state has `remaining_steps`, `scheduled_secs`, `elapsed_secs` and `memory_state`; a relearning state has a learning part and a review part.

## L1. Steps as stored (R636)

- A preset's learning steps (`learn_steps`, schema-11 `new.delays`) and relearning steps (`relearn_steps`, `lapse.delays`) are lists of minutes as `f32` (`sched/presets#V5`). They are used as stored: no sorting, no bounds, and an empty list means no steps.
- The delay of a step in seconds is its minutes times 60, multiplied in `f32`, then converted to an unsigned 32-bit integer: truncated toward zero, NaN and negative values give 0, values above 4,294,967,295 give 4,294,967,295.

| Step (minutes, as `f32`) | Seconds |
|---|---|
| 1.0 | 60 |
| 0.75 | 45 |
| 0.5 | 30 |
| 19.983334 (written as 19m59s) | 1199 |
| 1440.0 | 86400 |
| 0.001 | 0 |
| -5.0 | 0 |

- `n` below is the number of steps of the list in use: learning steps for a new or learning card, relearning steps for a relearning card or a review card's Again.

## L2. The current step (R638)

- The remaining count `r` is the card's `left` modulo 1000 (older clients packed a second number into the thousands; `sched/card-data#K4`). It is the state's `remaining_steps`.
- The current step's index is `n − r`, with 0 used when `r > n`, then lowered to `n − 1` when it is higher (and 0 when `n` is 0).

| `n` | `left` | `r` | Index |
|---|---|---|---|
| 3 | 3 | 3 | 0 |
| 3 | 2 | 2 | 1 |
| 3 | 1 | 1 | 2 |
| 3 | 1002 | 2 | 1 |
| 3 | 2003 | 3 | 0 |
| 3 | 0 | 0 | 2 (clamped to the last step) |
| 3 | 5 | 5 | 0 |

(Golden `sched/steps/learning-progress`, cards c1 to c7.)

## L3. Again (R639)

With steps, Again goes back to the first step: delay = the first step's seconds, `remaining_steps` = `n`. Without steps, Again leaves the steps (`sched/fsrs-transitions#T8`, `#T9`, `#T10`).

## L4. Hard (R640)

With steps, Hard stays on the current step and keeps `remaining_steps` = `r` exactly as read (so `left` 5 with 3 steps gives 5). Its delay:

- on a step other than the first (index above 0): that step's seconds, as they are, even above one day;
- on the first step (index 0), with at least two steps: `floor((s0 + s1) / 2)`, the sum of the first two steps' seconds in unsigned 32-bit arithmetic that stops at 4,294,967,295;
- on the first step of a single-step list: multiply `s0` by 3 with saturation at 4,294,967,295, then divide that result by 2, discarding the remainder; take the smaller of this value and `s0 + 86,400` with saturation at the same maximum. Saturation precedes division, as for the two-step sum; widened multiplication followed by final saturation gives a different result and is not this rule;
- then, for the two first-step forms only: a delay above 86,400 s (exactly 86,400 is not above) becomes whole days: `round(delay / 86,400)` with the division in `f32` and halves away from zero, times 86,400.

Without steps, Hard leaves the steps (T8 to T10).

| Steps | Index | Hard delay (s) | Why |
|---|---|---|---|
| 10m | 0 | 900 | 1.5 × 600 |
| 30s 2m | 0 | 75 | (30 + 120) / 2 |
| 1m 10m | 0 | 330 | (60 + 600) / 2 |
| 10m 1d | 0 | 43500 | 12 h 5 min, not above a day |
| 16h | 0 | 86400 | 1.5 × 57,600 is exactly a day: not rounded |
| 23h | 0 | 86400 | 124,200 s is 1.44 days, rounded to 1 |
| 1d | 0 | 172800 | 129,600 s is 1.5 days, rounded half up to 2 |
| 25h | 0 | 172800 | min(135,000, 176,400) is 1.56 days, rounded to 2 |
| 2d | 0 | 259200 | min(259,200, 259,200) is 3 days |
| 1d 3d | 0 | 172800 | (86,400 + 259,200) / 2 is exactly 2 days |
| 20h 49h | 0 | 86400 | 124,200 s, rounded to 1 day |
| 1d23h | 0 | 259200 | min(253,800, 255,600) is 2.94 days, rounded to 3 |
| 1m 10m 1h | 1 | 600 | the current step |
| 1m 10m 1h | 2 | 3600 | the current step |

(Goldens `sched/steps/hard-delays` and `sched/labels/units`, through the `scheduled_secs` of the Hard states.)

At the imported-data boundary, one step whose seconds saturate to 4,294,967,295 gives 2,147,483,647 seconds before day rounding, then 2,147,472,000 seconds (24,855 days). This multiplication boundary is separate from Z7's explicit safe rule for an unrepresentable fuzz range.

## L5. Good (R641)

With a next step (index + 1 < `n`), Good moves to it: delay = the next step's seconds, `remaining_steps` = `n − (index + 1)`. Otherwise (the last step, or no steps) Good leaves the steps (T8 to T10). So `left` 5 with three steps gives a Good state with `remaining_steps` 2 (golden `sched/steps/learning-progress` c6).

## L6. Easy and new cards (R641, R642)

- Easy always leaves the steps (T8, T9), on any step.
- A new card is answered as a learning card at its first step that has not been seen yet: `remaining_steps` = `n` (learning steps), `scheduled_secs` 0, `elapsed_secs` 0 and no memory state. Its Again, Hard and Good states are therefore those of a learning card at the first step, and its Easy is a graduation (T8). Only the current state stays `new` (`sched/fsrs-transitions#T7`).
- The memory states still differ from a learning card's: a new card has none, so FSRS gives the initial states.

Golden `sched/steps/new-card-buttons`: the new cards c1 and c2 and the learning card c3 on the first step of `1m 10m` all show Again 60 s with 2 remaining, Hard 330 s with 2, Good 600 s with 1.

## L7. Seconds or days (R601, R643)

A learning or relearning state keeps its delay in seconds. Wherever that delay is used as a due time (the answer, U44), shown (`sched/answer-labels#A1`) or logged (the review-log preview, `sched/fsrs-transitions#T14`), it is first compared with `T`, the number of seconds from now to `next_day_at` (`sched/day-clock`; Meguri's day clock keeps it above 0):

- a delay `d` with `d ≥ T` counts as `k = floor((d − T) / 86,400) + 1` whole days: the card goes to the interday learning queue with `due` = today + `k`;
- a shorter delay stays `d` seconds: the intraday learning queue with `due` = now + `d` plus the learning-step fuzz (`sched/fuzz#Z7`).

| Now (next day at 04:00) | `T` | Delay | Result |
|---|---|---|---|
| 23:55 | 14,700 | 43,500 (Hard on `10m 1d`, AE30) | 1 day: due tomorrow, not at 11:55 |
| 23:55 | 14,700 | 86,400 (a `1d` step) | 1 day |
| 23:55 | 14,700 | 600 | 600 s |
| 03:55 | 300 | 600 | 1 day: tomorrow starts five minutes later |
| 10:00 | 64,800 | 259,200 (a `3d` step) | 3 days |
| 10:00 | 64,800 | 169,200 (a `1d23h` step) | 2 days |

(Goldens `sched/steps/rollover-answer`, which applies the answers, and `sched/labels/rollover`, `sched/labels/units`.)

Steps of a day or more therefore make interday learning cards, which the queue builder counts against the review limit (R643, U41).

## L8. Elapsed time of the current state (R663)

The current state of a learning card, and the learning part of a relearning card, report `scheduled_secs` = the current step's seconds (L2; 0 without steps) and an `elapsed_secs` that depends on the card's queue:

- **Queue 1 (intraday learning):** the instant the card was last answered is taken as `due − F`, where `F` is the current step's seconds after learning-step fuzz (`sched/fuzz#Z7`) with the seed card id + (reps − 1). The count minus one is taken in unsigned 32-bit arithmetic, so 0 reviews give 4,294,967,295 (not 0, unlike `sched/fuzz#Z1`'s fuzz-delta rule). Then `elapsed_secs` = now − (`due` − `F`), computed in signed 64-bit arithmetic and kept as its low 32 bits: a card whose due time is further away than `F` gives a negative difference that wraps.
- **Queue 3 (interday learning):** `k` = today − `due` + the larger of 1 and `floor(step seconds / 86,400)`, then `elapsed_secs` = `k` × 86,400; both in signed 32-bit arithmetic (the product wrapping), the result read as unsigned 32-bit.
- **Any other queue** (suspended, buried): 0.

`elapsed_secs` does not affect any interval (FSRS uses whole days since the last review, `sched/fsrs-transitions#T2.3`); it is part of the state that the answer checks and that conformance observes.

| Card | Step | Card id, reviews | `due` | `elapsed_secs` |
|---|---|---|---|---|
| queue 1 | 3s | 1894701600000, 1 | now | 3 (no room for fuzz) |
| queue 1 | 30s | 1894701600002, 1 | now | 33 |
| queue 1 | 30s | 1894701600003, 0 | now | 31 (seed id + 4,294,967,295) |
| queue 1 | 30s | 1894701600009, 1 | now + 100 | 4294967229 (33 − 100, wrapped) |
| queue 1 | 2h | 1894701600007, 3 | now | 7360 |
| queue −2 | 10m | any | now | 0 |
| queue 3 | 1d | | today | 86400 |
| queue 3 | 1d | | today − 2 | 259200 |
| queue 3 | 1d | | today + 2 | 4294880896 (−86,400 wrapped) |
| queue 3 | 3d | | today | 259200 |

(Goldens `sched/steps/learning-fuzz` and `sched/steps/interday`.)

## L9. Relearning steps (R655)

A relearning card moves through its home preset's relearning steps with L2 to L5: Again goes back to the first relearning step, Hard repeats the step (with the first-step forms of L4), Good moves to the next step. The learning part of the new state carries the delay and `remaining_steps`; what the review part holds and what happens when a rating leaves the steps is `sched/fsrs-transitions#T9`.

## L10. The learning part of each new state

| Rating | Delay, `remaining_steps` | `elapsed_secs` | `memory_state` |
|---|---|---|---|
| Again | L3 | 0 | FSRS's Again state |
| Hard, learning card or new card | L4 | 0 | FSRS's Hard state |
| Hard, relearning card | L4 | kept from the current state | FSRS's Hard state |
| Good | L5 | 0 | FSRS's Good state |

So a relearning card on its first step of `10m 1h`, 680 s after its due-time reconstruction, shows a Hard state of 2100 s with `elapsed_secs` 680 (golden `sched/steps/relearning` c2).

## L11. Scenarios

| Scenario | Covers |
|---|---|
| `sched/steps/new-card-buttons` | L6; T7 and T8 for new cards |
| `sched/steps/hard-delays` | L1, L4 |
| `sched/steps/learning-progress` | L2, L3, L4, L5, L8 |
| `sched/steps/interday` | L7 (states only), L8 queue 3 |
| `sched/steps/learning-fuzz` | L8, `sched/fuzz#Z7` |
| `sched/steps/relearning` | L9, L10, `sched/fsrs-transitions#T9` |
| `sched/steps/rollover-answer` | L7 and the learning-step fuzz through answers (AE30; needs the `answer` op, U44) |

All goldens are recorded. Meguri's unit tests add L1's conversion table and L4's arithmetic on lists no scenario has (a sum above 4,294,967,295, NaN steps).

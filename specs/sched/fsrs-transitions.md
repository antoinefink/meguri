# FSRS transitions

Spec: sched/fsrs-transitions
Requirements: R587, R601, R646, R647, R648, R649, R651, R652, R654, R655, R657, R658, R670, R671, R704, R764
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/fsrs/, conformance/scenarios/sched/steps/, conformance/scenarios/fp-probe/

How the current state and the four answer states of a card are computed. Meguri always schedules with FSRS (R702), so every state below has a memory state. T1 to T6 were written for the floating-point probe (U14, `conformance/float-determinism`) and cover review cards; T7 to T14 add new, learning and relearning cards, short-term scheduling, the ease factor, leeches, parameters Meguri cannot use and the engine's interface (U45). The steps themselves are `sched/steps`, fuzz is `sched/fuzz` and the button labels are `sched/answer-labels`; the load balancer (U44) plugs into `sched/fuzz#Z8`.

## T1. The FSRS model

- Meguri evaluates FSRS only with the crate `fsrs` at exactly 6.6.2 (R646): `FSRS::new(&params)` with the parameter list of T2.1, unchanged, then `next_states(memory_state, desired_retention, days_elapsed)`, where `memory_state` is `Some(MemoryState { stability, difficulty })` for a card that has one and `None` for a new card (the crate then gives the initial states).
- The crate fills the shorter parameter forms (0, 17 or 19 values) and clips every parameter to its range itself. Meguri never fills, converts or clips parameters, and never re-implements a formula of the crate. A list of any other length, or one holding NaN or an infinity, is refused by the crate; Meguri reports it as `UnsupportedParams` (T13).
- `next_states` returns, for each of again, hard, good and easy, a memory state (stability `S_r`, difficulty `D_r`) and an unrounded interval `u_r` in days (`f32`). All of it is 32-bit float arithmetic with the platform's math library, as in Anki on the same platform (KTD24, `conformance/float-determinism`).
- An error of the crate for any other reason (a memory state it cannot use) is a typed error; nothing is guessed.

## T2. Inputs

### T2.1 Parameters (R647)

The card's home preset is the preset of its home deck: the original deck (`odid`) while the card is in a filtered deck, otherwise its deck (`did`). Its parameter list is the first non-empty one of `fsrsParams6` (21 values), `fsrsParams5` (19), `fsrsWeights` (17), or the empty list when all three are empty (the crate's defaults). These are the schema-11 preset keys; in the preset protobuf they are `fsrs_params_6`, `fsrs_params_5` and `fsrs_params_4`. Values are `f32`.

The decay the model then uses (conformance tooling needs it, `conformance/float-determinism#F8`; the scheduler never computes it): 0.1542 for the empty list; 0.5 for 17 or 19 values; for 21 values the 21st value clamped to [0.1, 0.8].

### T2.2 Desired retention (R648)

The home deck's own desired retention when the deck sets one, otherwise the home preset's `desiredRetention` (`f32`, default 0.9). In the deck protobuf the deck's value is an optional `f32`; in the schema-11 deck object it is `desiredRetention`, a whole percent, which reads as that integer divided by 100 in `f32` arithmetic.

### T2.3 Elapsed days (R649)

`days_elapsed = floor(max(0, next_day_at − lrt) / 86400)` in whole seconds, where `next_day_at` is the next day boundary of the day clock and `lrt` the last review time in the card data (`model/card-data-json` C2). A card reviewed earlier the same day gets 0. When `lrt` is missing, the time of the card's last review in the review log stands in for it (U46); with neither, 0.

The boundary counts: with the next day starting at 2030-01-16 04:00 UTC, a review at 2030-01-15 04:00:00 (exactly one day before) gives 1, at 04:00:01 gives 0, at 03:59:59 gives 1; a review time at or after the boundary gives 0; exactly three days before it gives 3 and one second more still 3 (golden `sched/fsrs/elapsed-days`).

### T2.4 Memory state

The card data's `s` and `d` (`model/card-data-json` C3). A card that is not new and has none gets one from its review history first (U46, R650). A new card has none.

### T2.5 Preset values

From the home preset, read with the bounds of `sched/presets#V3`: the maximum interval `M_raw` (schema-11 `rev.maxIvl`, 1–36,500, default 36,500) and `M = max(1, M_raw)`; the learning steps and relearning steps (`sched/steps#L1`); the starting ease (`new.initialFactor` / 1000, default 2.5); the leech threshold (`lapse.leechFails`, 1–9999, default 8); whether the parameter list allows short-term scheduling (`sched/presets#V6`). The minimum lapse interval (`lapse.minInt`) and the other SM-2 options play no part (R653; T9).

## T3. A review card's current state

The current state of a review card in a normal deck is `review` with `scheduled_days` = `cards.ivl`, `elapsed_days` = `max(0, ivl − (due' − today))` where `due' = min(cards.due, today)`, `ease_factor` = `cards.factor` / 1000 (`f32`), `lapses` = `cards.lapses`, `leeched` = false and `memory_state` = T2.4. Only `scheduled_days` and `ease_factor` feed the states below; the FSRS call takes T2.3's elapsed days.

## T4. Again on a review card (R654, R671, R587)

- The review part: `scheduled_days = max(1, round(u_again))`, rounding halves away from zero, as an unsigned integer. No fuzz, no minimum and no maximum interval apply here; they apply when the card leaves relearning (T9). `elapsed_days` 0; `ease_factor` = max(1.3, ease − 0.2) in `f32`; `lapses` + 1; `leeched` when the leech threshold `t` is above 0, the new lapse count `l` is at least `t`, and `l − t` is a multiple of `ceil(t / 2)` (at least 1) (T12); `memory_state` = (`S_again`, `D_again`).
- With relearning steps the state is `relearning`: `learning` = {`remaining_steps`: the number of relearning steps, `scheduled_secs`: the first step in seconds (minutes × 60 in `f32`, truncated), `elapsed_secs`: 0, `memory_state`: as the review part}, and `review` = the review part.
- Without relearning steps: when short-term scheduling applies (T10), a `relearning` state with `learning` = {`remaining_steps` 0, `scheduled_secs` = `u_again` × 86,400 truncated (T10), `elapsed_secs` 0, `memory_state` as the review part} and `review` = the review part; otherwise the review part alone as a `review` state.
- So without relearning steps a lapse goes straight back to review with FSRS's Again interval, rounded but neither fuzzed nor raised to the minimum lapse interval nor capped by the maximum interval: with a maximum of 2 days, a card whose Again interval is 3.75 days gets 4 days (golden `sched/fsrs/review-again` c13).

## T5. Hard, Good and Easy on a review card (R652, R670, R587)

For `r` = hard, then good, then easy:

1. `z_r` = the no-regression minimum of `sched/fuzz#Z5` for (`u_r`, the current `scheduled_days`, `M_raw`).
2. The lower bound: hard: `max(z_r, 1)`; good: `max(z_r, hard days + 1)`; easy: `max(z_r, good days + 1)`.
3. The bound clamped to [1, `M`].
4. `days_r` = the fuzzed interval of `sched/fuzz#Z4` for (`u_r`, that bound, `M`), with the factor of the seed card id + `reps` before the answer (`sched/fuzz#Z1`, `Z2`). A fuzzed-day chooser (the load balancer, `sched/fuzz#Z8`) may replace the pick.

Each result is a `review` state: `scheduled_days` = `days_r`, `elapsed_days` 0, `ease_factor` = max(1.3, ease − 0.15) for hard, ease for good, ease + 0.15 for easy (`f32`), `lapses` and `leeched` as in the current state, `memory_state` = (`S_r`, `D_r`).

So Good lands above Hard and Easy above Good, unless the maximum interval holds them all at `M`.

Examples (golden `sched/fsrs/review-buttons`, default parameters, desired retention 0.9 unless noted):

| Card | `u_hard`, `u_good`, `u_easy` | Hard, Good, Easy days |
|---|---|---|
| c1 (AE23): s 10, interval 10, reviewed 9 days ago | 22.27, 30.41, 48.23 | 25, 34, 53 |
| c2: s 15, interval 22, reviewed 2 days ago | 18.37, 20.61, 25.51 | 21, 25, 29 (Good's minimum is 22: `u_good` rounds to 21, not above 22, and 22 lies inside its fuzz range) |
| c3: s 10, interval 100, reviewed 5 days ago | 17.78, 22.93, 34.21 | 20, 25, 37 (no minimum: 100 is beyond every fuzz range) |
| c9: s 30, interval 30, reviewed an hour ago (0 days) | 30.0, 30.0, 43.35 | 31, 33, 41 |
| c11: as c4 with desired retention 0.99 | | 2, 3, 4 |
| c15: s 3, interval 2, reviewed 3 days ago | 10.29, 15.11, 25.69 | 8, 14, 23 (each grew: minimum 3) |

## T6. Example

Oracle (`conformance/goldens/smoke/fsrs-new-card.json`, step 6): a review card with `s` 2.3065, `d` 2.111, interval 2, 3 days after its last review, default parameters, desired retention 0.9, maximum 36,500, relearning step 10 m, card id 1894698000000 with 2 reviews. Again: relearning (10 min), review part 1 day, memory (0.6369821, 7.3921676). Hard 10 days (9.240417, 4.7481427), Good 15 days (13.836124, 2.1041174), Easy 26 days (23.900333, 1.0). With desired retention 0.9 the unrounded intervals equal the new stabilities, so fuzz moved Hard from 9.24 to 10, Good from 13.84 to 15 and Easy from 23.90 to 26.

## T7. The current state of the other cards

Of a card in a normal deck (`odid` 0), by its type (`sched/card-data#K1`):

| Type | Current state |
|---|---|
| 0 new | `new` with `position` = `due`, or 0 when `due` is negative |
| 1 learning | `learning`: `remaining_steps` = `left` modulo 1000; `scheduled_secs` = the current learning step's seconds (`sched/steps#L2`; 0 without learning steps); `elapsed_secs` per `sched/steps#L8`; `memory_state` = T2.4 |
| 2 review | T3 |
| 3 relearning | `relearning`: the learning part as for type 1 but with the relearning steps; the review part has `scheduled_days` = `ivl`, `elapsed_days` = `ivl`, `ease_factor` = `factor` / 1000, `lapses` = `lapses`, `leeched` false, `memory_state` = T2.4 |

- A card in a filtered deck (`odid` not 0) is out of scope (filtered decks are not studied in Meguri, R612): the engine refuses it with `InFilteredDeck` (T14) instead of computing the states of a normal deck.
- Only the review part of a relearning card is carried into its next states (T9); its `elapsed_days` is the interval, not a count of days.

## T8. Leaving the learning steps (R651, R642)

A new or learning card leaves its learning steps on Easy, and on Again, Hard or Good when `sched/steps#L3` to `#L5` give no delay (no steps, or Good on the last step). Unless short-term scheduling keeps it (T10), the state is `review` with:

- **Again, Hard, Good:** `scheduled_days` = the fuzzed interval (`sched/fuzz#Z4`) of `x` = `round(u_r)`, at least 1, with minimum 1 and maximum `M`;
- **Easy:** first `g` = the fuzzed interval of the unrounded `u_good` with minimum 1 and maximum `M`; then `scheduled_days` = the fuzzed interval of `x` = `round(u_easy)`, at least 1, with minimum `g + 1` and maximum `M` (`sched/fuzz#Z4` step 1 lowers that minimum to `M` when it is above);
- `ease_factor` = the starting ease of the home preset; `elapsed_days` 0; `lapses` 0 and `leeched` false, whatever the card had; `memory_state` = (`S_r`, `D_r`).

All fuzz of one answer uses the same factor (seed card id + `reps`). Examples: new cards with default parameters (`u_good` 2.3065, `u_easy` 8.2956): Good after the last step 2 days, Easy 8 to 10 days by seed (goldens `sched/steps/new-card-buttons`, `sched/steps/hard-delays`); a learning card answered again the same day (`u_good` 0.2467, `u_easy` 0.4244): Good's fuzzed interval is 1, so Easy is 2 days (`sched/steps/new-card-buttons` c3).

## T9. Relearning (R655, R654)

The review part of every new state starts from the current review part (T7) and keeps its `ease_factor`, `lapses` and `leeched` (Again in relearning is not another lapse). With relearning steps (`sched/steps#L9`, `#L10`):

| Rating | With a step delay | Leaving the steps (no delay) |
|---|---|---|
| Again | `relearning`: learning part per L3; review part with `scheduled_days` = `max(1, round(u_again))` (not fuzzed), `elapsed_days` 0 | (only without relearning steps) review part R with `scheduled_days` = fuzzed `round(u_again)` (at least 1; minimum 1, maximum `M`) and `elapsed_days` kept |
| Hard | `relearning`: learning part per L4; review part with `elapsed_days` 0, `scheduled_days` kept | (only without relearning steps) as Again with `u_hard` |
| Good | `relearning`: learning part per L5; review part with `elapsed_days` 0, `scheduled_days` kept | as Again with `u_good` |
| Easy | (always leaves) | `review`: `scheduled_days` = fuzzed `round(u_easy)` (at least 1) with minimum `g + 1` and maximum `M`, `g` = fuzzed unrounded `u_good` (as T8); `elapsed_days` 0 |

- "Leaving the steps" gives the review part R as a `review` state, unless short-term scheduling applies (T10): then a `relearning` state whose review part is R and whose learning part has `scheduled_secs` = `u_r` × 86,400 truncated, `memory_state` = (`S_r`, `D_r`), and `remaining_steps` and `elapsed_secs` as follows: Again: `remaining_steps` = the number of relearning steps, `elapsed_secs` 0; Hard: both kept from the current learning part; Good: `remaining_steps` per L5, `elapsed_secs` kept.
- In every new state `memory_state` = (`S_r`, `D_r`), in both parts.
- `elapsed_days` "kept" is the current review part's, which T7 sets to the interval: a relearning card with interval 1 that leaves relearning on Good shows `elapsed_days` 1 (golden `sched/steps/relearning` c3).
- The preset's minimum lapse interval does not apply anywhere under FSRS: a card leaving relearning gets FSRS's interval even below it (golden `sched/steps/relearning`: minimum 3, intervals of 1 day for c3, c4 and c6).

## T10. Short-term scheduling (R657, R658)

FSRS may keep a card in (re)learning for less than half a day instead of letting it leave the steps.

- **Allowed** when the parameter list allows it (`sched/presets#V6`: the empty list; a list of 19 or more values whose 18th and 19th values are both above 0, compared as stored; never a 17-value list) and, for the step list in question, that list is empty or the collection key `fsrsShortTermWithStepsEnabled` is true (`sched/config-keys#G2`; Meguri never writes it).
- **Applies** to a rating that leaves the steps (T4 without relearning steps, T8 for Again, Hard and Good, T9) when `u_r < 0.5` (`f32`). Easy never stays.
- **Learning card or new card:** a `learning` state with `scheduled_secs` = `u_r` × 86,400 (`f32` product, truncated as in `sched/steps#L1`), `elapsed_secs` 0, `memory_state` = (`S_r`, `D_r`), and `remaining_steps` = the number of learning steps for Again, the current count for Hard and Good (`n` for a new card).
- **Review and relearning cards:** T4 and T9.
- With steps, Again and Hard always have a delay, so the key changes only Good on the last step (learning or relearning).

| Preset | Card | Again | Hard | Good | Easy |
|---|---|---|---|---|---|
| no steps, no parameters, retention 0.9 | new | learning 18,316 s | review 1 day | review 2 days | review |
| no steps, retention 0.99 | new | learning 1,258 s | learning 7,675 s | learning 13,690 s | review 2 days |
| no steps, retention 0.99 | learning, answered 20 min ago | 494 s | 1,258 s | 1,464 s | review 2 days |
| no steps, retention 0.9 | relearning (s 0.3, d 9) | relearning 9,961 s | 25,920 s | 29,480 s | review 2 days |
| no steps, retention 0.9 | review with s 0.4, 1 day | relearning 13,896 s | review | review | review |
| steps 10m, retention 0.99, key off | new | learning 600 s | learning 900 s | review 1 day | review 2 days |
| the same, key on | new | learning 600 s | learning 900 s | learning 13,690 s (1 remaining) | review 2 days |
| the same, key on | relearning on its last step | relearning 600 s | 900 s | relearning 2,025 s, 0 remaining | review 2 days |
| no steps, 21 parameters with the 18th 0 | new | review 1 day | review 1 day | review 1 day | review 2 days |
| no steps, 19 parameters (FSRS-5 defaults) | new | learning 3,010 s | 8,853 s | 23,730 s | review 2 days |
| no steps, 17 parameters | new | review 1 day | review 1 day | review 1 day | review 2 days |

(Goldens `sched/fsrs/short-term`, `sched/fsrs/review-again` c14.)

## T11. The ease factor (R587)

Meguri keeps the SM-2 ease moving although FSRS never uses it.

- Again on a review card: max(1.3, ease − 0.2). Hard on a review card: max(1.3, ease − 0.15). Good: unchanged. Easy: ease + 0.15, with no upper bound. All in `f32` with the constants as the nearest `f32`: 1.3 + 0.15 is 1.4499999 (stored 1450), 5.0 + 0.15 is 5.15.
- Answers in relearning keep the ease; graduation from learning sets the home preset's starting ease (T8).
- The stored `factor` is `round(ease × 1000)`, halves away from zero, in `f32`, kept within 0 to 65,535. Learning states do not change `factor` (a new card keeps 0 until it graduates).

| Current ease | Again | Hard | Good | Easy |
|---|---|---|---|---|
| 2.5 | 2.3 | 2.35 | 2.5 | 2.65 |
| 1.4 | 1.3 | 1.3 | 1.4 | 1.55 |
| 1.3 | 1.3 | 1.3 | 1.3 | 1.4499999 |
| 5.0 | 4.8 | 4.85 | 5.0 | 5.15 |

(Golden `sched/fsrs/review-buttons` c12 to c14.)

## T12. Leeches (R671)

- A review card's Again state is `leeched` when its new lapse count `l` and the home preset's leech threshold `t` satisfy: `t` above 0, `l ≥ t`, and `l − t` a multiple of `h`, where `h` = `t / 2` rounded up (computed in `f32`), at least 1.
- The threshold is read with `sched/presets#V3`: a stored 0 (or anything outside 1 to 9999) reads as 8. So through a preset the threshold is never 0, and a stored 0 does not turn leech detection off: a card reaching 8 lapses becomes a leech (golden `sched/fsrs/review-again` c11). The predicate's `t` = 0 case (never a leech) is reachable only by calling it directly.
- Only Again on a review card sets the flag; every other state keeps the current `leeched` (false). What a leech does (tag, suspension, notice) is U44's (R672, R673).

| Threshold | Lapse counts that are leeches |
|---|---|
| 8 | 8, 12, 16, 20, … (not 7, 10) |
| 3 | 3, 5, 7, … (not 4) |
| 1 | every lapse |
| 0 (predicate only) | none |

## T13. Parameters Meguri cannot use (R704)

The engine refuses to compute states, with `UnsupportedParams`, for a card whose home preset has any of:

1. a parameter list (T2.1) whose length is not 0, 17, 19 or 21, for example 34 (the parameter count of later FSRS versions);
2. a parameter list holding NaN, +∞ or −∞;
3. a later FSRS parameter list than `fsrs_params_6`, which a later Anki release may write:
   - in the deck config blob, an unknown field with number 7 or 8 (numbers Anki reserves for later parameter lists, `model/protobuf-records#P6.6`) other than a length-delimited field of length 0;
   - in `other` (`model/protobuf-records#P7`), a top-level member whose key is `fsrsParams` followed by a decimal number of at least 7 (`fsrsParams7`, `fsrsParams8`, …) and whose value is a JSON array with at least one element.

- Nothing is written and no other card is affected; the study screen shows R704's message, and memory-state computation and optimization skip such presets (U46, U47, U50 use the same check).
- Anki 26.09.3 has no later list: it ignores such a key or field and schedules with `fsrsParams6`. The oracle at the pin cannot produce such a preset, so this is a forward-compatibility rule, asserted by Meguri's unit tests (a 34-value list; a valid `fsrsParams6` plus a non-empty `fsrsParams7` member; a valid list plus an unknown field 7 holding a packed float), not a conformance scenario.

## T14. The engine (U45)

A pure function of its inputs: it reads nothing else and writes nothing.

**Inputs:** the card's scheduling columns (`id`, `type`, `queue`, `due`, `ivl`, `factor`, `reps`, `lapses`, `left`, `odid`); its memory state or none (T2.4); its last review time or none (T2.3, with U46's review-log fallback injected); the home preset read with `sched/presets#V3` (T2.5, T2.1, and the markers of T13); the desired retention in force (T2.2); today, `next_day_at` and now (`sched/day-clock`); the keys `fsrsShortTermWithStepsEnabled` and `collapseTime` (`sched/config-keys#G2`); and optionally a fuzzed-day chooser (`sched/fuzz#Z8`; none means plain fuzz). The custom scheduling JavaScript (`cardStateCustomizer`) and the card's `cd` are never consulted (R764).

**Outputs:** the current state and the Again, Hard, Good and Easy states in the shape of `specs/conformance/observations.md` O5, and for each of the four:

- the delay: seconds or whole days by `sched/steps#L7` for a learning or relearning state, `scheduled_days` for a review state;
- `left`: `remaining_steps` of the (learning part of the) state, 0 for a review state;
- the `factor` the answer stores (T11; unchanged for a learning state);
- the review-log preview of the row the answer writes (R681, applied by U44): `ivl` = the state's delay as days (positive) or seconds (negative, `−seconds`, with seconds above 2,147,483,647 written as −2,147,483,647); `lastIvl` = the current state's delay the same way (a new card's is 0; a learning card's is its current step, a review card's its interval); `type` from the current state: 0 for new and learning cards, 1 for review, 2 for relearning; `factor` = `round(1000 × ((D − 1) / 9 + 0.1))` in `f32` from the state's difficulty `D` (543 for 4.990228, golden `sched/fsrs/ae23-answer`);
- the label (`sched/answer-labels`), computed from the same state, so a label always shows what answering applies (R656).

Again on a review card carries the leech flag (T12).

**Errors:** `UnsupportedParams` (T1, T13), `InFilteredDeck` (T7), and the crate's other errors (T1). None of them changes anything.

**FSRS instances:** the engine may keep `FSRS` instances made from a parameter list for reuse, keyed by the exact `f32` values of the list; a reused instance gives the same results as a new one. U46, U47 and U50 use the same instances and the same T13 check.

## T15. Scenarios

| Scenario | Covers |
|---|---|
| `sched/fsrs/review-buttons` | T3, T5 (no-regression minimum inside and outside the fuzz range, on time, early, late by 1, 10 and 100 days, same day, desired retention 0.7 and 0.99), T11; AE23's states |
| `sched/fsrs/ae23-answer` | AE23 through an answer: the stored interval and the review-log row equal the Good state (needs the `answer` op, U44) |
| `sched/fsrs/review-again` | T4, T12, T10 for a lapse without relearning steps, the uncapped Again interval |
| `sched/fsrs/short-term` | T10 with and without `fsrsShortTermWithStepsEnabled`, parameter forms of 17, 19 and 21 values |
| `sched/fsrs/elapsed-days` | T2.3 at the day boundary |
| `sched/fsrs/max-interval` | T5, T8 and T9 under a maximum of 1, 5 and 30 days, an interval above the maximum |
| `sched/steps/*` | T7, T8, T9 with `sched/steps` |
| `fp-probe/*` | T1 to T5 on many inputs (`conformance/float-determinism`) |

All goldens are recorded. T13 and the `InFilteredDeck` refusal are unit tests only.

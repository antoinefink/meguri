# FSRS transitions

Spec: sched/fsrs-transitions
Requirements: R587, R646, R647, R648, R649, R652, R654, R670, R671
Anki: 26.09.3
Scenarios: conformance/scenarios/fp-probe/

How the four answer states of a card are computed when FSRS is on. This file was written for the floating-point probe (U14, `conformance/float-determinism`) and so far covers review cards (cards whose current state is `review`, card type 2) with the load balancer off. U45 adds new, learning and relearning cards, short-term scheduling (R657, R658), the memory-state fallback and the labels as further sections; U44 adds the load balancer.

## T1. The FSRS model

- Meguri evaluates FSRS only with the crate `fsrs` at exactly 6.6.2 (R646): `FSRS::new(&params)` with the parameter list of T2.1, unchanged, then `next_states(Some(MemoryState { stability, difficulty }), desired_retention, days_elapsed)`.
- The crate fills the shorter parameter forms (0, 17 or 19 values) and clips every parameter to its range itself. Meguri never fills, converts or clips parameters, and never re-implements a formula of the crate. A list of any other length is refused by the crate (U45 turns that into `UnsupportedParams`, R704).
- `next_states` returns, for each of again, hard, good and easy, a memory state (stability `S_r`, difficulty `D_r`) and an unrounded interval `u_r` in days (`f32`). All of it is 32-bit float arithmetic with the platform's math library, as in Anki on the same platform (KTD24, `conformance/float-determinism`).

## T2. Inputs

### T2.1 Parameters (R647)

The card's home preset is the preset of its home deck: the original deck (`odid`) while the card is in a filtered deck, otherwise its deck (`did`). Its parameter list is the first non-empty one of `fsrsParams6` (21 values), `fsrsParams5` (19), `fsrsWeights` (17), or the empty list when all three are empty (the crate's defaults). These are the schema-11 preset keys; in the preset protobuf they are `fsrs_params_6`, `fsrs_params_5` and `fsrs_params_4`. Values are `f32`.

The decay the model then uses (conformance tooling needs it, `conformance/float-determinism#F8`; the scheduler never computes it): 0.1542 for the empty list; 0.5 for 17 or 19 values; for 21 values the 21st value clamped to [0.1, 0.8].

### T2.2 Desired retention (R648)

The home deck's own desired retention when the deck sets one, otherwise the home preset's `desiredRetention` (`f32`, default 0.9). In the deck protobuf the deck's value is an optional `f32`; in the schema-11 deck object it is `desiredRetention`, a whole percent, which reads as that integer divided by 100 in `f32` arithmetic.

### T2.3 Elapsed days (R649)

`days_elapsed = floor(max(0, next_day_at − lrt) / 86400)` in whole seconds, where `next_day_at` is the next day boundary of the day clock and `lrt` the last review time in the card data (`model/card-data-json` C2). A card reviewed earlier the same day gets 0. When `lrt` is missing, the time of the card's last review in the review log stands in for it (U46).

### T2.4 Memory state

The card data's `s` and `d` (`model/card-data-json` C3). A card that is not new and has none gets one from its review history first (U46, R650).

### T2.5 Preset values

The home preset's maximum interval `M_raw` (schema-11 `rev.maxIvl`, 1–36,500, default 36,500) and `M = max(1, M_raw)`; its relearning steps (schema-11 `lapse.delays`, minutes); its leech threshold (`lapse.leechFails`).

## T3. A review card's current state

The current state of a review card in a normal deck is `review` with `scheduled_days` = `cards.ivl`, `elapsed_days` = `max(0, ivl − (due' − today))` where `due' = min(cards.due, today)`, `ease_factor` = `cards.factor` / 1000 (`f32`), `lapses` = `cards.lapses`, `leeched` = false and `memory_state` = T2.4. Only `scheduled_days` and `ease_factor` feed the states below; the FSRS call takes T2.3's elapsed days.

## T4. Again on a review card (R654, R671, R587)

- The review part: `scheduled_days = max(1, round(u_again))`, rounding halves away from zero, as an unsigned integer. No fuzz, no minimum and no maximum interval apply here; they apply when the card leaves relearning (U45). `elapsed_days` 0; `ease_factor` = max(1.3, ease − 0.2) in `f32`; `lapses` + 1; `leeched` when the leech threshold `t` is above 0, the new lapse count `l` is at least `t`, and `l − t` is a multiple of `ceil(t / 2)` (at least 1); `memory_state` = (`S_again`, `D_again`).
- With relearning steps the state is `relearning`: `learning` = {`remaining_steps`: the number of relearning steps, `scheduled_secs`: the first step in seconds (minutes × 60 in `f32`, truncated), `elapsed_secs`: 0, `memory_state`: as the review part}, and `review` = the review part.
- Without relearning steps: U45 (short-term rule, R657), otherwise the review part alone as a `review` state.

## T5. Hard, Good and Easy on a review card (R652, R670, R587)

For `r` = hard, then good, then easy:

1. `z_r` = the no-regression minimum of `sched/fuzz#Z5` for (`u_r`, the current `scheduled_days`, `M_raw`).
2. The lower bound: hard: `max(z_r, 1)`; good: `max(z_r, hard days + 1)`; easy: `max(z_r, good days + 1)`.
3. The bound clamped to [1, `M`].
4. `days_r` = the fuzzed interval of `sched/fuzz#Z4` for (`u_r`, that bound, `M`), with the factor of the seed card id + `reps` before the answer (`sched/fuzz#Z1`, `Z2`). With the load balancer on, U44's choice replaces the pick.

Each result is a `review` state: `scheduled_days` = `days_r`, `elapsed_days` 0, `ease_factor` = max(1.3, ease − 0.15) for hard, ease for good, ease + 0.15 for easy (`f32`), `lapses` and `leeched` as in the current state, `memory_state` = (`S_r`, `D_r`).

So Good lands above Hard and Easy above Good, unless the maximum interval holds them all at `M`.

## T6. Example

Oracle (`conformance/goldens/smoke/fsrs-new-card.json`, step 6): a review card with `s` 2.3065, `d` 2.111, interval 2, 3 days after its last review, default parameters, desired retention 0.9, maximum 36,500, relearning step 10 m, card id 1894698000000 with 2 reviews. Again: relearning (10 min), review part 1 day, memory (0.6369821, 7.3921676). Hard 10 days (9.240417, 4.7481427), Good 15 days (13.836124, 2.1041174), Easy 26 days (23.900333, 1.0). With desired retention 0.9 the unrounded intervals equal the new stabilities, so fuzz moved Hard from 9.24 to 10, Good from 13.84 to 15 and Easy from 23.90 to 26.

# Interval fuzz

Spec: sched/fuzz
Requirements: R652, R659, R660, R661, R662, R663, R664, R1892
Anki: 26.09.3
Scenarios: conformance/scenarios/fp-probe/, conformance/scenarios/sched/answer/

How Anki spreads a review interval over a range of nearby days, so that cards answered together do not stay together. Fuzz cannot be turned off (R664): Anki has no setting for it, and every interval below goes through it.

This file was written for the floating-point probe (U14, `conformance/float-determinism`). It covers review-interval fuzz, the no-regression minimum and the fuzz-delta observation. Z7, learning-step fuzz (R663), was added for `sched/answering`, which applies it; U44 adds the load balancer, which replaces the pick of Z4 when it is on (R666).

## Z1. Seed

- The seed of an answer is the card id plus the card's review count before the answer (`cards.reps`), both read as unsigned 64-bit integers and added with wrap-around (R659). Card ids are positive and below 2^63, and `reps` is below 2^32, so the sum never wraps for stored cards.
- Recomputing a fuzzed interval that an earlier answer already gave (the fuzz-delta observation, Z6) uses the review count minus one instead, floored at 0.

## Z2. Fuzz factor

- The factor `f` is the first value that the `rand` crate, version 0.9.4 exactly, returns for `Rng::random_range(0.0f32..1.0f32)` on a generator made by `StdRng::seed_from_u64(seed)` (R660). It lies in [0, 1).
- Meguri depends on `rand =0.9.4` for this under a renamed workspace dependency, separate from whatever version fsrs pulls in. A change of that pin changes fuzz results; the 10,000-pair fuzz table (Z6, `conformance/float-determinism#F4`) must fail before any other test when it does (R1892, AE82).
- The draw uses only integer operations and exact float scaling, so it gives the same `f` on every platform.

## Z3. Fuzz delta

`Δ(x)`, the amount of fuzz in each direction around an interval of `x` days (`x` a 32-bit float), is computed in 32-bit float arithmetic, every operation rounded to `f32`, in exactly this order:

1. If `x < 2.5`, `Δ = 0`.
2. Otherwise start with `Δ = 1.0`, then add three terms in this order, each `factor × max(0, min(x, end) − start)` (first `min`, then the subtraction, then `max` with 0, then the product with the factor, then the sum with `Δ`):

   | Term | start | end | factor |
   |---|---|---|---|
   | 1 | 2.5 | 7.0 | 0.15 |
   | 2 | 7.0 | 20.0 | 0.1 |
   | 3 | 20.0 | the largest finite `f32` | 0.05 |

   The factors are the `f32` values nearest to 0.15, 0.1 and 0.05.

Examples (in exact arithmetic; the `f32` results differ only in the last bits): `Δ(2.5) = 1`, `Δ(7) = 1.675`, `Δ(20) = 2.975`, `Δ(100) = 6.975`.

## Z4. Fuzz range and pick

Given an unrounded interval `x` (`f32` days), a minimum `m` and a maximum `M` (unsigned integers), the fuzzed interval is:

1. `m' = min(m, M)`.
2. `x' = x` clamped to `[m', M]` (as `f32`).
3. `lower = round(x' − Δ(x'))` and `upper = round(x' + Δ(x'))`, each subtraction or sum done in `f32`, rounding halves away from zero, then converted to an unsigned integer.
4. `lower` and `upper` are each clamped to `[m', M]`.
5. When `lower = upper` and `upper > 2` and `upper < M`, `upper` becomes `lower + 1`.
6. The result is `floor(L + f × N)` converted to an unsigned integer, where `L` is `lower` as `f32`, `N` is `1 + upper − lower` (an integer) as `f32`, `f` is the factor of Z2, and the product and the sum are each rounded to `f32`. Since `f < 1`, the result lies in `[lower, upper]`.

So intervals below 2.5 days are only rounded and clamped (R661), and `x` itself is never rounded on the way: only `x' − Δ` and `x' + Δ` are.

No oracle observation can fix the factor (it always comes from a seed), so these examples, worked from the steps above for chosen factors, are for unit tests only:

| x | m | M | lower, upper | f = 0 | f = 0.5 | f = 0.99 |
|---|---|---|---|---|---|---|
| 2.4 | 1 | 36500 | 2, 2 | 2 | 2 | 2 |
| 3 | 1 | 36500 | 2, 4 | 2 | 3 | 4 |
| 10 | 1 | 36500 | 8, 12 | 8 | 10 | 12 |
| 25 | 1 | 36500 | 22, 28 | 22 | 25 | 28 |
| 4 | 5 | 36500 | 5, 6 (x' = 5; the minimum lifts `lower`) | 5 | 6 | 6 |
| 200 | 1 | 150 | 141, 150 (x' = 150) | 141 | 146 | 150 |
| 1 | 4 | 4 | 4, 4 (no widening at the maximum) | 4 | 4 | 4 |

## Z5. No-regression minimum

When a review card is answered Hard, Good or Easy, its new interval must not fall below what it had, unless FSRS clearly shortened it (R652). Given the unrounded new interval `x` (`f32`), the card's current interval `p` (`cards.ivl`) and the preset's maximum interval `M`, the minimum is:

1. `r = round(x)` (halves away from zero) as an unsigned integer, and `upper` from Z4 steps 1–5 for `(x, 1, M)`.
2. If `r > p`: `p + 1`.
3. Otherwise, if `p ≤ upper`: `p` (the interval did not grow, but fuzz may not shrink it).
4. Otherwise: 0 (the interval shrank beyond the fuzz range, for example after new parameters or a lower desired retention: no minimum).

Examples with `M = 36500` and `p = 9`: `x = 9.6` gives 10 (it grew); `x = 8.2` gives 9 (`upper` of 8.2 is 10); `x = 5` gives 0 (`upper` of 5 is 6).

`sched/fsrs-transitions#T5` combines this minimum with the order of the buttons.

## Z6. Fuzz delta of a stored card (`fuzz_table`)

The `fuzz_table` observation (`specs/conformance/observations.md` O4) reports, for a card id `c`, a review count `n` (the reviews before the answer) and a whole interval `i`, the change fuzz makes to `i`:

- `delta = Z4(x = i, m = 1, M = the maximum interval of the card's home preset) − i`, as a signed integer, with the factor of Z2 for the seed `c + n` (Z1).
- So a delta is 0 for `i ≤ 2` when the maximum is at least `i`, and the table is the same on every platform (Z2, Z3 and Z4 use no math library function).

Examples (oracle, `conformance/goldens/smoke/fuzz-table-small.json`, maximum interval 36,500):

| card id | reps | i = 1 | 2 | 3 | 7 | 20 | 100 | 1000 |
|---|---|---|---|---|---|---|---|---|
| 1 | 0 | 0 | 0 | 1 | 2 | 2 | 5 | 34 |
| 1 | 1 | 0 | 0 | −1 | −2 | −3 | −6 | −44 |
| 1894680000001 | 2 | 0 | 0 | 0 | 1 | 1 | 2 | 13 |

## Z7. Learning-step fuzz (R663)

When an answer puts a card into an intraday learning or relearning step (`sched/answering` A3.2), its `due` is the answer second plus the step's delay with a small random extra, so that cards answered together are not shown again together. The extra is never shown on the answer buttons and never recorded in the review log.

Given the delay `s` in whole seconds (an unsigned 32-bit integer) and the seed of Z1 (card id plus `reps` before the answer):

1. `w` = `floor(min(f(s) × 0.25, 300.0))` in 32-bit float arithmetic, where `f(s)` is `s` converted to `f32`, then converted to an unsigned integer. So `w` is a quarter of the delay, at most 300 seconds (5 minutes).
2. When `w` is 0 (delays under 4 seconds), the fuzzed delay is `s`.
3. Otherwise the fuzzed delay is the first value that the `rand` crate, version 0.9.4 exactly (as Z2), returns for `Rng::random_range(s..s + w)`, a half-open range of `u32`, on a generator made by `StdRng::seed_from_u64(seed)`. It lies in `[s, s + w − 1]`.

The draw is integer-only, so it is the same on every platform. U45's current state of a learning card reconstructs the instant of its last answer from its `due` with the fuzzed delay that the seed of the previous answer gives (Z1's review count minus one).

Examples (oracle):

| Card id | `reps` before | `s` | `w` | Fuzzed delay | Golden |
|---|---|---|---|---|---|
| 1894701600000 | 0 | 600 | 150 | 739 | `storage/stamps/answer`, step 4 |
| 1894698000000 | 0 | 600 | 150 | 696 | `smoke/fsrs-new-card`, step 3 |
| 1894698000000 | 2 | 600 | 150 | 715 | `smoke/fsrs-new-card`, step 8 |
| 1894719600000 | 3 | 600 | 150 | 705 | `smoke/sm2-new-card`, step 9 |

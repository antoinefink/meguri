# Load balancer

Spec: sched/load-balancer
Requirements: R665, R666, R667, R668, R669, R670, R721, R1893
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/loadbalancer/

The load balancer picks the day of a short review interval among the days that fuzz allows, preferring days with few cards due in the same preset. It is the fuzzed-day chooser of `sched/fuzz#Z8`: at each point where the next-state engine fuzzes a review interval, it is offered the same `(x, m, M)` that `sched/fuzz#Z4` would get, and it either returns a day or declines. A declined offer, and every offer when no load balancer is present, gets Z4's plain pick with the same factor.

The load balancer works on a cache of card counts per preset and day (W2). The cache belongs to whoever builds the study queue: the review session builds it with each queue build (U43, R668), Grade Now builds one for its own use (U48), and the conformance executors hold one as `specs/conformance/answer-ops.md` H2 says. W8 and W9 say how an answer and its undo change it.

Terms: `today` and `next_day_at` are the day clock's values (`sched/day-clock`) at the instant the cache is built. A card's home deck is `odid` when it is not 0, else `did`; its home preset is the preset id stored in the home deck (`conf`), whether or not a preset with that id exists. `f32` means IEEE 754 single precision with every operation rounded to `f32`.

## W1. The switch (R665)

- The collection config key `loadBalancerEnabled` (a boolean; missing reads as true, `sched/config-keys`) decides whether a cache is built. It is read when the cache is built; changing it takes effect at the next build.
- With the key off there is no cache, so every offer gets plain fuzz (`sched/fuzz` alone, AE23).
- Meguri shows the key as the toggle "Load balancer" (U49); Anki 26.09.3 has no visible toggle. Writing the key from the toggle is a change that rebuilds the queue (`sched/queue-building#B10`).

## W2. Building the cache (R668, R669)

The cache is built from the collection as it is at that instant:

1. **Days.** The cache has 99 days for each preset: day 0 is `today`, day 98 is `today + 98`.
2. **Counted rows.** Every card row whose `due` is at least `today` and less than `today + 99` is counted on day `due − today`, whatever its `type` and `queue`. So review and interday-learning cards due in the window count, and so do new cards whose position (`due`) falls in the window, and suspended and buried cards. Ordinary intraday learning timestamps lie outside this day-number window. The query adds no type/queue exclusion: an imported row with a due value inside the window still counts.
3. **Preset of a row.** A counted row goes to the list of the preset that the card's current deck (`did`) names. A card whose `did` is a filtered deck, or names no deck, is not counted. The deck's preset need not exist.
4. **Entries.** Each day of a preset's list holds entries (card id, note id), one per counted row. A preset with no counted row has no list at all.
5. **Easy Days.** For every preset row of the collection, its `easy_days_percentages` (schema-11 `easyDaysPercentages`, a list of `f32`; R669) gives one class per weekday, Monday first:

   | Stored list | Classes |
   |---|---|
   | empty | Normal for all seven days |
   | 7 values | per value: exactly 1.0 → Normal, exactly 0.0 → Minimum, any other value → Reduced |
   | any other length | none: building the cache fails with an invalid-input error |

   New presets store seven times 1.0 (`sched/presets`). The last row is an open point for U43: Anki's queue build fails as a whole when any preset of the collection has such a list, so its user cannot study until the preset is fixed. Meguri reproduces the failure here and U43 decides what the session shows.
6. **Day boundary.** The cache keeps the `next_day_at` it was built with; W5 uses it for weekdays.

The requirements' wording "review-due counts for today to today + 99" is narrower than this: Anki counts every row in the window, new and suspended cards included, and the window ends at `today + 98`. Counts decide the chosen day, so Meguri counts exactly as above.

## W3. Answering an offer (R666, R667)

The engine offers `(x, m, M)`: the unrounded interval `x` (`f32` days), the minimum `m` and the maximum `M` (unsigned integers), exactly as `sched/fuzz#Z8` passes them. With the offer come three values of the card being scheduled:

- its home preset (as defined above);
- its note id, but only when its home preset exists and has `bury_reviews` (schema-11 `rev.bury`) on; otherwise no note id, and W6 leaves every modifier at 1;
- the fuzz seed of `sched/fuzz#Z1` (card id plus `reps` before the answer).

The load balancer declines when:

1. the integer part of `x` (its value rounded toward zero) is above 90, or `m` is above 90 (so `x` = 90.9 is balanced and `x` = 91.0 is not; R666's "90 days or less" refers to these two values);
2. the home preset has no list in the cache (W2 step 4: no card of that preset was counted);
3. the home preset is not a preset row of the collection (it has no Easy Days classes).

Otherwise:

4. **Candidates.** `lower` and `upper` are the bounds of `sched/fuzz#Z4` steps 1 to 5 for `(x, m, M)`. The candidate days are `lower`, `lower + 1`, …, `upper`. Since every point that offers passes `m ≥ 1`, and `x < 91` and `m ≤ 90` here, the candidates lie within days 1 to 98.
5. For each candidate day `d`, with `n_d` the number of entries on day `d` of the home preset's list: the Easy Days factor `e_d` (W5), the sibling factor `s_d` (W6) and the weight `w_d` (W7).
6. **Draw.** The chosen day is drawn with the weights (W7). If the draw cannot be made, the load balancer declines.

The result is the chosen day itself; it lies in `[lower, upper]` and so within the minimum and the maximum interval (R670).

## W4. What the weights mean

W5–W7 define the calculation. The following points explain its effect.

Each empty candidate has weight 1. A nonempty candidate has weight `n^−2.15 × d^−3 × s_d × e_d`, with W7’s operation order. At day 30, one counted card gives a base weight of 1/27,000. Empty candidates therefore dominate the draw in ordinary cases, but other positive weights remain in the draw.

For nonempty days, apply W5’s weekday factor, W6’s sibling factor and W7’s exact weight/draw rule.

## W5. Easy Days factor (R667, R669)

- **Weekday of a day.** The weekday of candidate day `d` is the local weekday (Monday to Sunday) of the instant `next_day_at + (d − 1) × 86,400` seconds, `next_day_at` being the cache's (W2 step 6). Local means the collection's time zone at that instant (`sched/day-clock`), so around a DST change with a rollover hour of 0 the instant can fall on the previous evening and give the previous weekday; Meguri computes it exactly so.
- **Loads.** Normal 1.0, Reduced 0.5, Minimum 0.0001 (`f32`).
- With `N` = the sum of `n_d` over all candidates and `P` = the `f32` sum, in candidate order, of the loads of the candidates' weekday classes:
  - a Normal day has `e_d` = 1.0;
  - a Minimum day has `e_d` = 0.0001;
  - a Reduced day is compared with the other days: `t` = (`N − n_d` as `f32`) / (`P − 0.5`), and `e_d` = 0.0001 when `n_d / 0.5` (as `f32`) is greater than `t`, else 1.0. When `t` is not a number (0 / 0, a single candidate), the comparison is false and `e_d` = 1.0.

Example: seven candidates on Monday to Sunday with 4, 4, 4, 4, 4, 3 and 1 cards, Saturday and Sunday Reduced (stored 0.5 and 0.7), the rest Normal: `N` = 24 and `P` = 6.0. Saturday: `t` = 21 / 5.5 ≈ 3.82 and 3 / 0.5 = 6 is greater, so 0.0001. Sunday: `t` = 23 / 5.5 ≈ 4.18 and 1 / 0.5 = 2 is not, so 1.0.

## W6. Sibling factor (R667)

Only when the offer carries a note id (W3):

- The sibling days are the days, out of 0 to 98, on which the list of any preset holds an entry with that note id. The card being scheduled counts too: a card counted on its current due day makes that day a sibling day.
- Each candidate starts with `s_d` = 1.0. For each sibling day `y` and each `k` from −5 to 5, the candidate `y + k` (when it is a candidate) has its `s_d` multiplied by the factor for `|k|`:

  | `|k|` | 0 | 1 | 2 | 3 | 4 | 5 |
  |---|---|---|---|---|---|---|
  | factor | 0.000001 | 0.2 | 0.4 | 0.6 | 0.8 | 1.0 |

- Meguri applies sibling days in increasing order and rounds each multiplication to `f32`. The prepared observations do not establish the reference order for every multi-day product. A different multiplication order can affect the last bit and a draw at a boundary. This grants no tolerance or waiver: a different chosen interval fails parity and needs concrete source-owner evidence and a decision. Do not create an overlay or deviation merely from this possibility.

Example: candidates 25 to 31 and sibling days 27 and 33. Day 27 gives 0.4, 0.2, 0.000001, 0.2, 0.4, 0.6, 0.8 to days 25 to 31; day 33 gives 1.0, 0.8, 0.6, 0.4 to days 28 to 31. So `s` = 0.4, 0.2, 0.000001, 0.2, 0.32, 0.36, 0.32.

## W7. Weight and draw (R667)

- **Weight.** `w_d` = 1.0 when `n_d` = 0. Otherwise `w_d` = `a × b × s_d × e_d`, multiplied left to right in `f32`, with `a` = (1 / `n_d`)^2.15, computed as `powf` of the `f32` quotient and the `f32` nearest to 2.15, and `b` = (1 / `d`)^3, computed as `q × (q × q)` with `q` the `f32` quotient. `powf` is the platform's single-precision power function; on macOS and iOS it may differ from glibc's in the last bit (`conformance/float-determinism`, the `fp-libm-boundary` class).

  | `n_d` | `a` | | `d` | `b` |
  |---|---|---|---|---|
  | 1 | 1.0 | | 25 | 6.4e−5 |
  | 2 | 0.2253126 | | 30 | 3.7037e−5 |
  | 3 | 0.09423 | | 34 | 2.5443e−5 |
  | 10 | 0.0070795 | | | |

- **Draw.** The weights, in candidate order, go to `rand::distr::weighted::WeightedIndex::new` of the `rand` crate, version 0.9.4 exactly (the pin of `sched/fuzz#Z2`), as `f32` weights. One `sample` on a fresh generator `StdRng::seed_from_u64(seed)`, `seed` from W3, gives the index of the chosen candidate. When `new` returns an error (no positive weight, a weight that is not finite or not a number), the load balancer declines.
- Each offer uses a fresh generator: the two offers of an Easy answer from learning (`sched/fuzz#Z8`) both start from the same seed.

## W8. After an answer (R668)

When an answer (U44, `sched/answering`) commits with a cache present and the card row it wrote has `queue` 2 (a review state that the leech action did not suspend, `sched/leeches#L3`):

- an entry (card id, note id) is added to day `ivl` (the interval just written) of the list of the card's home preset, when that preset has a list and `ivl` is at most 98; otherwise nothing is added;
- nothing is removed: the card keeps the entry of the day it was counted on at the build, if any. Entries are not deduplicated by card id. This update occurs only after a successful commit; a failed answer changes no cache.

Learning, relearning and suspended results add nothing. The answer does this whether or not the load balancer chose its day.

## W9. Undo and redo (R721)

- Undoing an answer that a review session made while it held a cache removes, from every day of every preset's list, one entry of that card (the first found), when the cache is still the one present at the answer. A day keeps the note id among its sibling notes while another entry of that note remains on it. So the undo also removes the entry the build gave the card, and an answer that added nothing still removes the build's entry.
- Undoing a Grade Now answer touches no cache (Anki records no queue change for it; Meguri's Grade Now cache is gone by then).
- When the queue was rebuilt between the answer and its undo (or its redo), the undo or redo drops that queue and its cache and removes no entry. Until the next build no cache is held, so next states use plain fuzz (W10). The next build makes a new cache from the collection after the undo or redo has committed (W2). A review session builds again at once (`sched/review-session` P9 "Build changed", C15), so its cache is then a fresh W2 build of that collection.
- Redoing the answer adds nothing back. These are entry-removal rules, not restoration of a whole prior cache snapshot. Tests must retain the cache-build identity and check multiple entries, answers that appended no entry, and an undo after a rebuild: the cache held afterwards must equal a fresh W2 build of the collection after the undo, not the rebuilt cache with the entry left in or taken out.
- Changed 2026-11-17: an earlier text kept the rebuilt cache unchanged on such an undo. Implemented code must be checked against the rule above: the U43 session's answer restorer and cache handling, and the conformance executor's held queue (`specs/conformance/answer-ops.md` H2, H3).
- Meguri does this through the answer's restorer (`engine/undo#M10`, `sched/answering#A13`).

## W10. Lifetime

- A review session builds the cache with every queue build of its deck and keeps it until the next build (U43, `sched/queue-building#B10`); a new day rebuilds. The cache is collection-wide: it counts every preset, whatever deck is studied.
- Grade Now (U48) builds one when it runs and drops it afterwards. Anki uses for Grade Now the study queue's load balancer when its queue happens to be built, else none.
- Outside a session and Grade Now, nothing holds a cache, and next states use plain fuzz (labels shown outside a session, for example in Card Info, are not U44's).

## W11. Examples (oracle)

Values from the goldens of W12 (the Linux and Apple oracles agree). Cards with stability 10 last reviewed 9 days before the next day starts have unrounded Hard 22.27, Good 30.41 and Easy 48.23 days (`sched/fsrs/review-buttons` c1), so candidates 19 to 25, 27 to 34 and 44 to 53.

| Scenario, card | Plain fuzz (Hard, Good, Easy) | With the load balancer | Why |
|---|---|---|---|
| `plain-over-90` `#c1` (stability 100) | 199, 261, 396 | 199, 261, 396 | every target above 90 (W3 step 1) |
| `plain-over-90` `#c2` | 19, 27, 44 | 24, 28, 50 | the only empty candidate days (W4) |
| `counted-rows` `#c1` | 25, 34, 53 | 24, 30, 53 | new, suspended, buried and interday learning rows of the preset count; another preset's do not (W2) |
| `crafted-days` `#c1` (AE82) | 23, 31, 49 | 22, 28, 49 | every candidate day holds cards: fewest cards, earlier days, away from the sibling on day 30 (W6, W7) |

`cache-update` (Hard of the five cards `#c2` to `#c6` with candidates 19 to 25, days 19 and 22 empty):

| Moment | Hard of `#c2` to `#c6` |
|---|---|
| queue built | 19, 22, 22, 22, 22 |
| after `#c1` is answered Good onto day 22 | 19, 19, 19, 19, 19 (day 22 now holds `#c1`, W8) |
| after the undo | 19, 22, 22, 22, 22 (W9) |
| after the redo | 19, 22, 22, 22, 22 (the redo adds nothing, W9) |

`easy-days`: cards `#c1`–`#c6`; values `(Hard, Good, Easy)`; the same candidate ranges as above. Apply W5 and W7.

| Weekday | Day offsets |
|---|---|
| Monday | 20, 27, 34, 48 |
| Tuesday | 21, 28, 49 |
| Sunday | 19, 47 |

| Moment | `#c1` | `#c2` | `#c3` | `#c4` | `#c5` | `#c6` |
|---|---|---|---|---|---|---|
| plain fuzz (no queue held, and again with the key off) | 25, 34, 53 | 19, 27, 44 | 24, 33, 52 | 22, 31, 49 | 24, 33, 51 | 25, 34, 52 |
| every day Normal | 23, 34, 52 | 19, 27, 44 | 22, 33, 52 | 19, 30, 48 | 20, 32, 51 | 22, 33, 52 |
| Monday and Tuesday Minimum, Sunday Reduced | 23, 32, 53 | 19, 29, 44 | 22, 32, 52 | 19, 30, 50 | 19, 31, 52 | 23, 32, 53 |

In the last row no Monday or Tuesday is chosen. Sunday is Reduced: day 19 (one card, below its share) keeps the factor 1.0 and is still chosen, day 47 (three cards) gets 0.0001 (W5).

`siblings` (every candidate day holds exactly one card; each card's sibling days in brackets). The sibling factor lowers the weight of the days near a sibling; it does not exclude them:

| Card | `bury_reviews` off | on |
|---|---|---|
| `#c1` (28) | 25, 34, 52 | 24, 34, 52 |
| `#c3` (31) | 24, 33, 52 | 24, 33, 52 |
| `#c5` (22) | 23, 32, 51 | 24, 32, 51 |
| `#c7` (47) | 19, 27, 45 | 19, 27, 45 |
| `#c9` (60) | 22, 31, 49 | 22, 31, 49 |
| `#c11` (27, 33) | 23, 32, 50 | 22, 31, 50 |

## W12. Scenarios

| Scenario | Covers |
|---|---|
| `sched/loadbalancer/plain-over-90` | AE23's load-balancer part: with the cache present, targets far above 90 days keep plain fuzz (W3 step 1); a 30-day target is balanced |
| `sched/loadbalancer/counted-rows` | W2 steps 2 and 3: the rows counted whatever their queue, another preset's rows not counted |
| `sched/loadbalancer/crafted-days` | AE82: a card with 3 earlier reviews, every candidate day of its Hard, Good and Easy ranges holding one to four cards of its preset, its sibling on day 30 with Bury review siblings on, another preset's cards ignored |
| `sched/loadbalancer/siblings` | W6: siblings near the targets with `bury_reviews` off, then on |
| `sched/loadbalancer/easy-days` | W5: a synced Easy Days list (Minimum and Reduced days) changes the choice; with the key off it has no effect |
| `sched/loadbalancer/cache-update` | W8, W9: an answer fills an empty day for other cards of the preset; its undo removes the entry, its redo adds none; W3 step 2 for a preset with no list |

Apply `conformance/answer-ops#H2`–`#H5` to every scenario id in this table: cache setup, execution constraints, fresh recordings and product evidence.

Own tests are also required for the exact offer boundaries (`x` 90.99 and 91.0; `m` 90 and 91), the factor examples, weight arithmetic, failed draws, invalid Easy Days list lengths and one-entry removal on undo. These requirements are not claims of completed tests. Compare exact selected intervals on Linux and macOS; no new tolerance is granted.

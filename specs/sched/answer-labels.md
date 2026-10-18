# Answer button labels

Spec: sched/answer-labels
Requirements: R656, R601
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/labels/

The short text above each answer button ("<10m", "1d", "2.3mo"): which delay it shows, how the unit and the number are chosen, and the exact strings. The engine computes each label from the same state that answering applies (`sched/fsrs-transitions#T14`), so a label never disagrees with the interval it announces (R656).

## A1. The delay a label shows

For each of Again, Hard, Good and Easy, take the state of that rating (`sched/fsrs-transitions`) and its delay in seconds, an unsigned 32-bit number `s`:

- a `learning` state: `scheduled_secs`;
- a `relearning` state: the `scheduled_secs` of its learning part;
- a `review` state: `scheduled_days` × 86,400, stopping at 4,294,967,295.

For a learning or relearning state, the day rule of `sched/steps#L7` applies first, with `T` = `next_day_at` − now (0 if negative): when `s ≥ T`, `s` becomes `k` × 86,400 (stopping at 4,294,967,295) with `k = floor((s − T) / 86,400) + 1`.

- The learning-step fuzz is not shown (`sched/fuzz#Z7`): the label of a 10-minute step says 10 minutes although the card comes back a little later.
- An interval chosen by a fuzzed-day chooser (the load balancer) is shown, since it is the state's `scheduled_days`.
- The current state has no label.

## A2. The unit

`t` is `s` converted to `f32` (round to nearest, ties to even). The unit is the first that fits:

| Condition | Unit | Unit length (s) |
|---|---|---|
| `t` < 60 | seconds | 1 |
| `t` < 3,600 | minutes | 60 |
| `t` < 86,400 | hours | 3,600 |
| `t` < 2,628,000 | days | 86,400 |
| `t` < 31,536,000 | months | 2,628,000 |
| otherwise | years | 31,536,000 |

A year is 365 days and a month a twelfth of it (30.42 days). All the constants are exact in `f32`.

## A3. The amount

`q` = `t` / unit length, divided in `f32`.

- Seconds, minutes and days: the amount is `round(q)`, halves away from zero.
- Hours, months and years: the amount is `round(q × 10) / 10`, the product, the rounding (halves away from zero) and the division in `f32`.

The amount is written in decimal digits with a full stop as decimal separator: as a whole number when it is one, otherwise with exactly one digit after the point (the amount of an hours, months or years label is a multiple of 0.1, so `k` = `round(amount × 10)` gives the text `k / 10`, for example 23 gives `2.3` and 10 gives `1`). No grouping separators, no leading zeros, no sign.

Rounding happens after the unit is chosen, so a delay just under a unit boundary can show the next unit's size in the smaller unit: 3,570 s is 59.5 minutes and shows `60m`; 86,399 s shows `24h`; 31,449,600 s (364 days) shows `12mo`.

## A4. The text

The label is the amount followed directly by the unit's suffix, with no space:

| Unit | Suffix | Example |
|---|---|---|
| seconds | `s` | `45s` |
| minutes | `m` | `10m` |
| hours | `h` | `1.5h` |
| days | `d` | `1d` |
| months | `mo` | `2.3mo` |
| years | `y` | `1.1y` |

## A5. The `<` prefix and the end label

With `L` = the learn-ahead limit (`collapseTime`, seconds, read as `sched/config-keys#G2` says: default 1,200):

- when `s` is 0, the label is `(end)`, with no prefix;
- otherwise, when `s < L`, the label is `<` followed by the A4 text;
- otherwise it is the A4 text.

The comparison uses `s` from A1, not the rounded amount: with the limit at 1,200 s, a step of 19m59s (1,199 s) shows `<20m`, a step of 20 minutes shows `20m`, and with the limit at two days a 2-day review interval shows `2d` while a 20-minute step shows `<20m`.

## A6. Exact strings

Meguri takes these strings only from this table (role: `docs/process/roles/spec-writer.md`):

| Use | String |
|---|---|
| prefix under the learn-ahead limit | `<` |
| seconds, minutes, hours, days, months, years | `s`, `m`, `h`, `d`, `mo`, `y` |
| a delay of 0 | `(end)` |

- Anki's label wraps the amount in the invisible Unicode isolation marks U+2068 and U+2069. Meguri's labels carry no isolation marks; the visible text is the same, and the `answer_labels` observation compares structured parts, so neither side's marks are compared (R1887).
- Labels are English only, as Meguri's interface is.

## A7. Examples

Values from the oracle (the `sched/labels` goldens, A8). Day conversions at 10:00 UTC with the next day at 04:00 (`T` = 64,800 s) unless noted.

| Delay or state | `s` | Learn-ahead limit | Label |
|---|---|---|---|
| step 45s | 45 | 0 | `45s` |
| step 59s | 59 | 0 | `59s` |
| Hard on 45s (67 s) | 67 | 0 | `1m` |
| step 90s | 90 | 0 | `2m` (1.5 rounds up) |
| step 150s | 150 | 0 | `3m` |
| Hard on 150s (225 s) | 225 | 0 | `4m` |
| step 10m | 600 | 0 | `10m` |
| step 90m | 5,400 | 0 | `1.5h` |
| Hard on 90m (8,100 s) | 8,100 | 0 | `2.3h` (2.25 rounds up) |
| step 119m30s | 7,170 | 0 | `2h` |
| step 1d | 86,400 | 0 | `1d` |
| step 1d23h | 1 day + 23 h → 2 days | 0 | `2d` |
| step 30d | 30 days | 0 | `30d` |
| step 31d | 31 days | 0 | `1mo` |
| step 70d | 70 days | 0 | `2.3mo` |
| step 364d | 364 days | 0 | `12mo` |
| step 365d | 365 days | 0 | `1y` |
| step 400d | 400 days | 0 | `1.1y` |
| step 0s | 0 | 0 | `(end)` |
| step 5m | 300 | 1,200 | `<5m` |
| Hard on 5m 20m (750 s) | 750 | 1,200 | `<13m` |
| step 20m | 1,200 | 1,200 | `20m` |
| step 10m | 600 | 172,800 | `<10m` |
| review, 2 days | 172,800 | 172,800 | `2d` |
| review, 26 days | | 1,200 | `26d` |
| review, 36 days | | 1,200 | `1.2mo` |
| review, 814 days | | 1,200 | `2.2y` |
| review, 1,077 days | | 1,200 | `3y` |
| relearning step 1h | 3,600 | 1,200 | `1h` |
| Hard on relearning 10m 1h (2,100 s) | 2,100 | 1,200 | `35m` |
| 23:55, next day at 04:00 (`T` 14,700): Hard on 10m 1d (43,500 s) | 1 day | 1,200 | `1d` (AE30) |
| 23:55: Again on 10m 1d (600 s) | 600 | 1,200 | `<10m` |
| 03:55 (`T` 300): Again on 10m 1d (600 s) | 1 day | 1,200 | `1d` |

The plan's examples of a 45-second delay as `<1m` and of a 10-minute delay as `10m` do not match Anki: a 45-second delay shows `45s` (`<45s` under the default limit), and a 10-minute delay shows `<10m` under the default limit and `10m` only with a limit of 600 s or less.

## A8. Observation and scenarios

The `answer_labels` observation (`specs/conformance/observations.md` O10) reports each label as its parts (`less_than`, `amount`, `unit`), not as text.

| Scenario | Covers |
|---|---|
| `sched/labels/units` | A2 to A4, the end label, A1's day rule at 10:00 |
| `sched/labels/less-than` | A5 with limits 1,200 s, two days and 0 |
| `sched/labels/rollover` | A1 near the start of the next day (AE30) |
| `sched/labels/review` | review and relearning states, months and years |

All goldens are recorded. U45 registers the typed `answer_labels` kind (O10) and its real adapter; all four label scenarios are active. Meguri's unit tests add the boundaries no scenario reaches: 3,570 s (`60m`), 86,399 s (`24h`), and `s` stopping at 4,294,967,295 (`136.2y`).

# FSRS items from the review log

Spec: sched/fsrs-items
Requirements: R649, R650, R685, R689, R690, R694, R1852
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/memory/

How a card's review-log rows become input for the `fsrs` crate (6.6.2, `sched/fsrs-transitions#T1`). One builder serves two purposes (R685):

- the **memory form**: one item that replays a card's history up to now, with an optional starting state, from which the crate computes the card's current memory state (`sched/memory-state` M2, M4);
- the **training form**: the items the optimizer trains on, one per qualifying review (U47).

Both forms use the same row classes (I2), the same rule for where a history starts (I3, with one difference) and the same elapsed days (I4).

## I1. Inputs

### I1.1 A card's rows

- The rows of `revlog` whose `cid` is the card's id, read as `model/row-records#R5` says (a `type` outside 0 to 5 reads as 0), ordered by `id` ascending. `id` is the review instant in epoch milliseconds.
- Bulk paths read the rows of many cards in one query ordered by `cid`, then `id`, and cut the stream into one group per card. They never query card by card.

### I1.2 The day boundary

`next_day_at`: the instant the next scheduler day starts, epoch seconds, from the day clock (`sched/day-clock`) at the moment of the computation.

### I1.3 The cutoff date ("Ignore cards reviewed before", R690)

- Stored per preset: protobuf field `ignore_revlogs_before_date`, schema-11 key `ignoreRevlogsBeforeDate`; text, `""` by default (`sched/presets#V2`).
- The cutoff `T` is an epoch-millisecond value: 0 for the empty text; otherwise the text read as a date, and `T` = 00:00:00 UTC of that date (proleptic Gregorian calendar). `T` can be 0 (1970-01-01) or negative (1969-12-31 gives -86,400,000).
- A row is **after the cutoff** when its `id` > `T`, and **before the cutoff** when its `id` < `T`. A row whose `id` equals `T` is neither.
- Anki reads the date leniently. The text is accepted when it is `<year>-<month>-<day>` where:
  - before each of the three numbers, any white space (Unicode White_Space) is skipped;
  - the year is 1 to 4 ASCII digits, or a `+` or `-` sign followed by one or more digits (`-` makes the year negative);
  - the month and the day are 1 or 2 ASCII digits each;
  - the two separators are exactly `-`, and nothing follows the day, not even white space;
  - the date exists (month 1 to 12, a day that month has; year 0 is a leap year) and its year lies from -262,143 to 262,142.

  | Text | Read as |
  |---|---|
  | `2026-01-03`, `2026-1-3`, ` 2026-01-03`, `2026- 1- 3`, `+2026-01-03` | 2026-01-03 |
  | `26-01-03` | year 26 |
  | `-2026-01-03` | year -2026 |
  | `2026-01-03 ` (trailing space), `02026-01-03`, `2026-02-30`, `2026-13-01`, `2026/01/03`, `2026-01-03T00:00`, `garbage` | refused |

  (Golden `sched/memory/ignore-before-count` gives the counts for each of these texts.)
- Anki's deck options screen only writes `""` or a canonical `YYYY-MM-DD`. A stored text that is refused makes every operation that needs the cutoff fail with an invalid-input error, and nothing changes: next states of a card without a memory state (`sched/memory-state` M2), a recompute (M4), the count (M5) and training (I7). Meguri fails the same operations with a typed error naming the preset.
- Writing the date is U49's (R690): when Anki saves a preset whose date text sorts after the local date of (`next_day_at` − 86,400 s) as plain strings, it stores that local date instead.

### I1.4 Historical retention (R689)

The home preset's `historical_retention` (schema-11 `sm2Retention`), an `f32` read with the bounds of `sched/presets#V3` (0.7 to 0.97; default 0.9). It is the retention assumed for history from before FSRS (I5).

## I2. Row classes

| Class | Rule |
|---|---|
| has a button | `ease` ≥ 1 |
| cram row | `type` 3 and `factor` 0: a review in a filtered deck that did not reschedule |
| graded | has a button and is not a cram row |
| learning row | graded and `type` 0 |
| Reset marker | `type` 4 and `factor` 0 (Forget writes one, R682) |
| interday | `ivl` ≥ 1, or `ivl` ≤ -86,400 (a step of at least a day stored in seconds) |

- The classes go by `ease`, `type` and `factor` only. Manual rows from set due date (`type` 4, `factor` above 0) and rescheduled rows (`type` 5) are not graded because their `ease` is 0. A filtered-deck row with a `factor` above 0 (an early review that did reschedule) is a graded review.
- Rows no Anki client writes follow the same table (golden `sched/memory/crafted-rows`): a row of an unknown type reads as type 0 and is a learning row when it has a button; a `type` 4 row with a button and a `factor` above 0 is a graded review; a Reset marker that has a button is both a Reset marker and graded; a `type` 0 row without a button is not a learning row.
- `ease` is the rating the crate receives (I4). Clients write 1 to 4.

## I3. Where a card's history starts

Cram rows play no part in this section: they neither belong to a run nor end one.

- The **tail** is the card's rows from its last Reset marker on, the marker included; all its rows when it has no marker.
- A **learning run** is a maximal sequence of learning rows of the tail with no other row between them. The **learning start** is the first row of the tail's last learning run.
- The **first qualifying row** is the earliest graded row that is interday and after the cutoff, among the rows from the learning start to the end, or among the whole tail when the tail has no learning run. (A Reset marker is never graded in data clients write, so the history begins after it.)

**Memory form.**

1. When the tail has a learning run, and its learning start is not before the cutoff or is the card's very last row (no row of any class, cram rows included, comes after it), the history is **complete** and starts at the learning start.
2. Otherwise, when a first qualifying row exists, the history is **incomplete** and starts at it.
3. Otherwise the card gives no item.

**Training form.**

1. When the tail has a learning run whose learning start is not before the cutoff, the history is complete and starts at the learning start.
2. Otherwise the card gives no training item.

The **kept rows** are the graded rows from the start to the end, in order. Cram rows, manual and rescheduled rows, and every other row without a button are dropped.

| History (oldest first; L learning, R review, X Reset marker, M set due date, C cram) | Cutoff | Memory form | Training form |
|---|---|---|---|
| L L R R | none | complete from the first L | complete from the first L |
| R R R | none | incomplete from the first interday R | none |
| L R X L L R | none | complete from the L after X | complete from the L after X |
| L R X R R | none | incomplete from the first interday R after X | none |
| L R X, or L R X M | none | none | none |
| L L R R L R (no marker before the second run) | none | complete from the last run's L | complete from the last run's L |
| L M L R | none | complete from the second L | complete from the second L |
| L C L R | none | complete from the first L | complete from the first L |
| L L R ∣ R R (cutoff between the R's) | between | incomplete from the first interday graded row after the cutoff | none |
| L L R R ∣ (all before) | after all | none | none |
| L ∣ (a single L, before) | after it | complete from that L | none |
| L L ∣ (both learning rows before) | after both | none | none |
| L ∣ L (cutoff between the two learning rows, nothing after) | between | incomplete from the second L when that row is interday, otherwise none | none |
| L ∣ L R (cutoff inside the learning run) | between the L's | incomplete from the first interday graded row after the cutoff | none: the card gives no training item although rows follow the cutoff |
| L at exactly the cutoff, then R R | equal to the first L's id | complete from that L | complete from that L |
| R at exactly the cutoff, then R R | equal to the first R's id | incomplete from the second R | none |

Goldens: `sched/memory/reset`, `skipped-rows`, `crafted-rows`, `ignore-before`, `ignore-before-boundary`, `sm2-history`.

## I4. Reviews and elapsed days (R649)

- The **day** of a row is `floor(max(0, next_day_at − s) / 86,400)`, where `s` is the row's `id` divided by 1000, rounded toward zero. Days are counted back from `next_day_at` in steps of exactly 86,400 s, whatever the time zone does; a row at or after `next_day_at` is on day 0.
- The kept rows `k0, k1, …, kn` give the reviews `(rating, delta_t)`: the rating is the row's `ease`; `delta_t` is 0 for `k0` and `day(k(i−1)) − day(ki)` for the others. Rows on the same day give 0.
- Imported timestamp boundaries use `sched/fsrs-transitions#T2.3`'s signed wrapping subtraction and low-32-bit day conversion, after converting each row's milliseconds to seconds toward zero. Each `delta_t` likewise retains its low 32 bits; it never panics on subtraction. Ordinary chronological histories have the nonnegative day differences above.
- Example: `next_day_at` 2026-03-03 04:00 UTC and the rows of H in `sched/memory/on-the-fly` (2025-10-01 09:00 and 09:10, 2025-10-02 10:00, 10-05, 10-14 10:00 and 10:10, 10-16, 10-22, 11-11 10:00) are on days 152, 152, 151, 148, 139, 139, 137, 131 and 111; with the ratings 3, 3, 3, 3, 1, 3, 3, 4, 2, the reviews are (3, 0), (3, 0), (3, 1), (3, 3), (1, 9), (3, 0), (3, 2), (4, 6), (2, 20).

## I5. The memory item

The memory form gives one item, the crate's `FSRSItem` with one `FSRSReview` (`rating`, `delta_t`) per review of I4, and a starting state:

- **Complete history:** no starting state. The crate starts from the first review's rating.
- **Incomplete history:** the first kept row is converted with the SM-2 conversion and removed from the item; the remaining reviews keep their `delta_t`:
  - ease = the row's `factor` / 1000 in `f32`, with a `factor` of 0 read as 2500;
  - interval = the larger of the row's `ivl` and 1, as `f32` (a negative `ivl` gives 1);
  - starting state = `FSRS::memory_state_from_sm2(ease, interval, historical retention)` on the home preset's model (`sched/fsrs-transitions#T2.1`);
  - when the ease is at most 1.1 (compared as `f32`), the row's `factor` is a difficulty that FSRS wrote (R681), and the starting difficulty becomes `(ease − 0.1) × 9 + 1`, computed in `f32` in that order. The stability stays.
- The memory state is `FSRS::memory_state(item, starting state)` on the home preset's model. An item left with no review (an incomplete history of one kept row) gives the starting state itself.
- `FSRS::memory_state_batch` gives each card the same values as one call per card; Meguri may use either.
- A crate error (the crate refuses non-finite results) fails the operation that needed the state.

Examples (goldens, default parameters, historical retention 0.9 unless said):

| Golden, card | History | Memory state (stability, difficulty) |
|---|---|---|
| `on-the-fly` c1 | H, complete | 39.451553, 7.6402645 |
| `sm2-history` c1 | H without its learning rows, incomplete from the 3-day row | 17.275585, 9.251785; with retention 0.8: 8.982975, 9.840615 |
| `sm2-history` c2 | one review row, `ivl` 100, `factor` 2500 | 100.0, 5.003576 |
| `sm2-history` c5 | one review row, `ivl` 100, `factor` 820 | 100.0, 7.4799995 |
| `sm2-history` c8 | first graded row `ivl` -172,800 (interval 1), then one review | 3.3629165, 8.202865 |
| `reset` c1 | H, Reset marker, then a new learning run | 23.205364, 2.0974553 (as with the new run alone) |
| `ignore-before` c1 | H then two 2026 reviews, cutoff 2026-01-01 | 92.9232, 6.5275307 (as with the two reviews alone) |

These values compare with the 1e-5 tolerance of `conformance/observations#O7` inside `next_states`; values stored in card data are rounded (`model/card-data-json#C4`) and compare exactly.

## I6. Cards that give no item

The builder only reports that a card gives no item. What happens then depends on the path: the card's own values on the fly (`sched/memory-state` M2), no memory state in a recompute (M4), no training item (I7).

## I7. Training items (R694; U47)

- **Card set.** One preset at a time (R1852): the cards of `sched/optimizer#O2`'s default set (by preset id, not suspended), or the cards U80 finds for the preset's `param_search` (schema-11 `weightSearch`) when it is not empty after trimming white space (R694). The search itself is U80's. As with the count (`sched/memory-state#M5`, `sched/memory-state#M10`), the default search text Anki builds for a preset whose name holds `\` or `"` can miss the preset's cards; Meguri selects them by preset id (the table of `sched/optimizer#O2`).
- **Items.** For each card, the training form of I3 and the reviews of I4. Each review `i` ≥ 1 whose `delta_t` is above 0 gives one item: the reviews 0 to `i`, tagged with the `id` of row `ki` and the card's id. Reviews on the same day as the one before give no item of their own but stay in later items.
- **Order.** All items of the preset sorted by their tag `id`, ascending; the card ids in the same order. They are the crate's `ComputeParametersInput` `train_set` and `card_ids`.
- **Counts.** The item count is the number Anki reports as the preset's FSRS items. The review count is the number of kept rows of the cards that gave at least one item (Anki's progress shows it; the diagnostics view's "review count used", R701).
- **Memory.** The items of one preset are dropped before the next preset's are built. The builder reports the item count and owned bytes, including review buffers and aligned card-id/tag buffers, with the accounting scope stated (R1852, U47). Serialized JSON size does not stand in for these owned allocations or for the separately measured process peak. A caller must not retain earlier presets' items in its diagnostics or results.
- Example (from the rules; U47's goldens check the training form): H of `sched/memory/on-the-fly` with no cutoff gives 6 items, ending at its rows 3, 4, 5, 7, 8 and 9 (reviews 0..2, 0..3, 0..4, 0..6, 0..7 and 0..8; review 5 is on the same day as review 4), and a review count of 9. With a cutoff of 2025-10-02 it gives none (its learning start is before the cutoff); with 2025-10-01 it gives the same 6 items.

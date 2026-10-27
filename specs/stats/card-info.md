# Card Info and Previous Card Info

Spec: stats/card-info
Requirements: R1248, R1249, R1250, R1251, R1252, R1253, R1254, R1255, R1256, R1257, R1259, R1260, R1261, R1262, R1263, R1264, R1423, R1424, R1425, R1426, R1427, R1428, R1429, R1430, R1448, R1841
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/card-info/

The read-only snapshot of one card that the Card Info view shows (U92 builds it, U222 shows it, U103 exports it): the stats rows, the review history, the forgetting curve and the debug dump, plus which card "Previous Card Info" names. Every text here is English and is shown exactly as the core returns it; the UI takes only labels from its String Catalogs (C12; the plan's Open Questions keep these strings verbatim in every locale, R1448).

Anki builds the raw values in its core and writes every text in its web page. The oracle returns the raw values only, so the raw values are compared with oracle goldens (C13) and the texts are tested from the tables here (C14.2), as `stats/studied-today` T6.4 does for the Today lines.

Related specs: card columns `model/row-records#R4`, review-log columns and read rules `model/row-records#R5`, card data keys `model/card-data-json` and `sched/card-data`, home deck and preset `sched/presets#V1`, decay `#V6`, desired retention `#V7`, memory items `sched/fsrs-items`, memory states `sched/memory-state`, retrievability `search/card-extractors#X5`, number writing `stats/studied-today#T5.2`.

## C1. The snapshot

- **Input.** A card id and the clock's current instant `now` (epoch seconds) with the day clock's `days_elapsed` and `next_day_at` (`sched/day-clock`). Card Info takes nothing else: Previous Card Info only chooses the id (C9).
- **Reads.** The card row, its note's note type id, the note type (name, kind, template names), the card's current deck (`did`), its home deck (`sched/presets#V1`), the home deck's preset, the card's review-log rows (by `cid`, ascending `id`), and the `rollover` config value for the debug dump. One indexed read per table on a reader connection; 500 review-log rows must still fit R1841's 100 ms budget.
- **No writes (R1252, R1425).** Building the snapshot never writes: not the card row (`data`, `mod`, `usn`), not `col.mod`, not the WAL. A missing memory state and a missing `lrt` are computed in memory (C3.4, C3.5) and dropped with the snapshot. When it builds its card stats and the data has no `lrt`, Anki rewrites the card's data with one (C13.3); Meguri never does.
- **Contents.** The raw values (C3), the stats rows (C4), the history entries (C6), the curve (C7), and on request the debug dump (C8).
- **Errors (C11).** When a record the snapshot needs is missing, the snapshot fails with a typed error and the view shows its unavailable state.

## C2. Terms

- **Instant**: epoch seconds. A review-log row's instant is its `id` divided by 1,000, truncated toward zero; the card's added instant is the card `id` divided the same way.
- **Local date and time of an instant**: the calendar date and the hour and minute of the instant under the UTC offset in effect at that instant in the device's current zone (the clock seam's offset at that instant, as `stats/day-buckets` B4 uses). The oracle runs in the scenario's zone (`conformance/dsl` D5).
- **Rated row**: a review-log row whose `ease` is 1 or more (any `type`, Filtered and Manual rows included).
- **Kind** of a row: its `type` as `model/row-records#R5` reads it (low 8 bits, unknown values read as 0): 0 Learn, 1 Review, 2 Relearn, 3 Filtered, 4 Manual, 5 Rescheduled.
- **Cram row**, **Reset marker**, **graded row**, **kept rows**, **memory item**: `sched/fsrs-items` I2, I3 and I5, built with the home preset's model, cutoff date and historical retention, and `next_day_at` now.
- **f32 value**: an IEEE 754 binary32 value. Every f32 that a text is written from is first widened exactly to binary64; the text is written from that binary64 value, never from the f32's own shortest decimal (`0.9` as f32 is written from 0.89999997615814208984375).

## C3. Raw values

Each value below is part of the snapshot whether or not its row shows. The column "Oracle" names the `CardStatsResponse` field that C13 compares it with.

### C3.1 Card, note and decks

| Value | Rule | Oracle |
|---|---|---|
| card id, note id | the card's `id` and `nid` | `card_id`, `note_id` |
| added | the card id's instant (C2) | `added` |
| card type name | the name of the note type's template at the card's `ord`; for a cloze note type always the first template's name, whatever `ord` is | `card_type` |
| note type name | the note type's name | `notetype` |
| deck name | the current deck's (`did`) full name, levels joined with `::` | `deck` |
| home deck name | when `odid` is not 0 and names another deck than `did`: the home deck's full name; otherwise absent | `original_deck` |
| preset name | the name of the home deck's preset | `preset` |
| interval | the card's `ivl` | `interval` |
| ease | the card's `factor` (kept for the dump and the oracle; no row shows it, C4) | `ease` |
| reviews, lapses | the card's `reps` and `lapses` | `reviews`, `lapses` |
| custom data | the card data's `cd` string exactly as stored, read by `model/card-data-json` C3 except its rule 5: a stored `{}` gives `{}` (the Custom Data row then shows with an empty value, C5.5); `""` only when `cd` is absent, empty or unreadable | `custom_data` |
| desired retention | the card data's `dr` (f32), absent when the data has none | `desired_retention` |
| parameters | the home preset's parameter list (`sched/presets#V6`) | `fsrs_params` |

### C3.2 First and latest review

- **first review**: the instant of the rated row with the smallest `id`; absent when the card has no rated row.
- **latest review**: the instant of the rated row with the largest `id`; absent likewise.
- Filtered (cram) and rated Manual rows count; unrated rows never do. The card data's `lrt` plays no part here, so a cram answer after the last graded review moves the latest review but not `lrt`.
- Oracle: `first_review`, `latest_review`.

### C3.3 Due and position

**Due instant** (`due_date`), by the card's `type`:

- `type` 0 (new): absent.
- any other type: let `x` be `odue` when `odue` is not 0, otherwise `due` (whatever `odid` says).
  - `x` above 1,000,000,000: `x` itself is the due instant (an intraday learning card's stored due time, R1248's "real time").
  - otherwise `x` is a day number: the due instant is `now + (x − days_elapsed) × 86,400`. It keeps `now`'s time of day; it is not the start of a day.
- Queue plays no part: suspended and buried cards have a due instant like the others.

**Position** (`due_position`):

- when the card data has `pos`: `pos`, whatever the card's type, read as a signed 32-bit integer (a `pos` above 2,147,483,647 wraps to a negative value);
- otherwise, for `type` 0: the card's `due` as stored (for a new card in a filtered deck, its place there, not `odue`);
- otherwise absent.

Every card that left the new state by an answer or Set Due Date keeps `pos` (`sched/card-data#K6`), so a review card usually has both a due instant and a position. Decided 2026-10-27: Meguri follows Anki here (R1261); R1248 and R1423 describe the common new-card case, not a narrower rule.

### C3.4 Memory state

- When the card data has a memory state (`s` and `d`, `model/card-data-json` C3 rule 4): that state, whatever the card's type.
- Otherwise, when the card is not new: the memory state of its memory item (`sched/fsrs-items#I5`), computed in memory and never stored (R1251). A card that gives no item has none. Unlike `sched/memory-state#M2`, there is no SM-2 conversion of the card's own `ivl` and `factor`.
- Otherwise none.
- The snapshot records which of the two sources the state came from (`stored` or `computed`).
- Oracle: `memory_state` (`stability`, `difficulty`), compared within `conformance/observations#O7`'s tolerance.

### C3.5 Retrievability

Present exactly when C3.4 gives a memory state:

1. The last review instant `L`: the card data's `lrt` when present; otherwise the instant of the card's latest row whose `ease` is 1 to 4 and that is not a cram row, whatever its kind (the lookup of `sched/memory-state#M3`); otherwise 0.
2. The elapsed seconds `e = now − L` as a 64-bit integer, then kept to its low 32 bits as an unsigned number: an `L` after `now` wraps to a very large value (`L` 100 s ahead gives 4,294,967,196 s and a retrievability close to 0).
3. `t = e / 86,400` in f32 (`e` rounded to f32 first).
4. The decay: for a stored state, the card data's `decay`, or 0.5 when absent (as `search/card-extractors#X5` step 4); for a computed state, the home preset's decay (`sched/presets#V6`), the decay that state was computed with.
5. R = the pinned crate's current retrievability for the state, `t` and the decay (`search/card-extractors#X5` step 5).

Oracle: `fsrs_retrievability`, compared within O7's tolerance.

Decided 2026-10-27 (AE46, R1251): Meguri gives a retrievability for a computed memory state too. Anki computes it only from a stored state, so for a card without one its `fsrs_retrievability` is absent while Meguri's is present (deviation `card-info-computed-retrievability`, C13.4).

### C3.6 Average and total time

- `total`: the sum, in f32 and in ascending `id` order starting from 0, of `time ÷ 1,000` for every review-log row of the card, rated or not; each term is the row's `time` (read by `model/row-records#R5`) converted to f32 and divided by 1,000 in f32.
- `n`: the number of rated rows.
- When `n` is 0 or `total` is 0, both values are 0. Otherwise `average = total ÷ n` in f32.
- Oracle: `average_secs`, `total_secs`, compared exactly (f32).

## C4. Stats rows

### C4.1 Order and applicability

The snapshot lists the applicable rows in this order. Each row has a key (the UI's `cardInfo.row.<key>` identifier), an English label (C12) and a value text (C5).

| # | Key | Label | Shown when | Value text |
|---|---|---|---|---|
| 1 | `added` | Added | always | local date (C5.1) of the added instant |
| 2 | `first_review` | First Review | first review present | local date |
| 3 | `latest_review` | Latest Review | latest review present | local date |
| 4 | `due` | Due | due instant present | local date of the due instant |
| 5 | `position` | Position | position present | the position as decimal digits, `-` before a negative value |
| 6 | `interval` | Interval | `ivl` is not 0 | time span (C5.2) of `ivl × 86,400` seconds |
| 7 | `stability` | Stability | memory state present | C4.2 |
| 8 | `difficulty` | Difficulty | memory state present | whole percent (C5.4) of `(d − 1) ÷ 9 × 100` |
| 9 | `retrievability` | Retrievability | memory state present and R neither 0 nor NaN | whole percent of `R × 100` |
| 10 | `reviews` | Reviews | always | `reps` as decimal digits |
| 11 | `lapses` | Lapses | always | `lapses` as decimal digits |
| 12 | `average_time` | Average Time | `total` is not 0 | time span of `average` seconds |
| 13 | `total_time` | Total Time | `total` is not 0 | time span of `total` seconds |
| 14 | `card_type` | Card Type | always | card type name |
| 15 | `note_type` | Note Type | always | note type name |
| 16 | `deck` | Deck | always | deck name; `<deck> (<home deck>)` when the home deck name is present |
| 17 | `preset` | Preset | always | preset name |
| 18 | `card_id` | Card ID | always | card id as decimal digits |
| 19 | `note_id` | Note ID | always | note id as decimal digits |
| 20 | `custom_data` | Custom Data | the custom data text is not `""` | C5.5 |

- Decimal digits are never grouped (`1234`, a card id `1894701600000`).
- No row shows the card's ease: Meguri always runs FSRS (R1250, R1424). Anki shows an Ease row (`factor ÷ 10` and `%`) for a card with no memory state and no `dr`; Meguri never does. No observation carries row text, so this needs no deviation entry; U92's unit tests assert it.
- Difficulty is computed in binary64 from the widened f32 `d`: subtract 1, divide by 9, multiply by 100, in that order.

### C4.2 Stability text

With `s` the stability (widened):

1. `A` = the rounded time span (C5.2, rounded form) of `s × 86,400` seconds.
2. When `s > 31`: `A (B)`, where `B` is the rounded time span of the same seconds with days as the largest unit (`1,000 days`).
3. Otherwise `A`.

| `s` | Text |
|---|---|
| 0.01 | `14 minutes` |
| 0.5 | `12 hours` |
| 1.4 | `1 day` |
| 1.5 | `2 days` |
| 2.3065 | `2 days` |
| 30.4 | `30 days` |
| 30.5 | `1 month` |
| 31 | `1.02 months` |
| 31.2 | `1.03 months (31 days)` |
| 39.451553 | `1.3 months (39 days)` |
| 45 | `1.48 months (45 days)` |
| 92.9232 | `3.06 months (93 days)` |
| 365 | `1 year (365 days)` |
| 1000 | `2.74 years (1,000 days)` |

### C4.3 Example rows

| Input | Row texts |
|---|---|
| `ivl` 1, 15, 30, 31, 45, 365, 400, 1000 | `1 day`, `15 days`, `30 days`, `1.02 months`, `1.48 months`, `1 year`, `1.1 years`, `2.74 years` |
| `d` 1, 2.118, 5.5, 7.6402645, 10 | `0%`, `12%`, `50%`, `74%`, `100%` |
| R 0.9, 0.905, 0.9403443, 0.995, 0.004 (f32) | `90%`, `90%`, `94%`, `100%`, `0%` (R is not 0, so the row shows) |
| `total` 0 | no Average Time and no Total Time rows |
| 2 rated rows of 3,000 ms and 6,000 ms and one Manual row of 0 ms | `4.5 seconds`, `9 seconds` |
| 1 rated row of 70,000 ms | `1.17 minutes`, `1.17 minutes` |
| current deck `Filtered::Today`, home deck `Lang::Verbs` | `Filtered::Today (Lang::Verbs)` |

## C5. Writing values

### C5.1 Dates

- **Date**: `YYYY-MM-DD` of the local date (C2): the year as plain decimal digits (no padding: year 970 is `970`), month and day with two digits.
- **Date and time**: `<date> @ HH:MM`, the local hour (00 to 23) and minute with two digits each, seconds dropped (not rounded).
- Examples, instant 1894748400: UTC `2030-01-15 @ 23:00`; Europe/Paris `2030-01-16 @ 00:00`; America/Los_Angeles `2030-01-15 @ 15:00`.
- The due instant of a day-number due keeps `now`'s time of day (C3.3), so between midnight and the rollover hour the Due date is one calendar day later than the scheduler day it stands for. With the rollover hour 4, at 2030-01-16 02:00 local (still the scheduler day that began 2030-01-15 04:00), a card due on the next scheduler day (which begins 2030-01-16 04:00) shows `2030-01-17`. Meguri reproduces this (R1261).

### C5.2 Time spans

A time span writes a number of seconds `x` (binary64) with one unit:

1. The unit, by `|x|`: below 60 seconds; below 3,600 minutes; below 86,400 hours; below 2,628,000 (a twelfth of 365 days) days; below 31,536,000 (365 days) months; otherwise years. A largest unit, when given, caps it (C4.2 step 2 caps at days).
2. The amount: `x` divided by the unit's seconds (1, 60, 3,600, 86,400, 2,628,000 or 31,536,000) in binary64.
3. The rounded form (Stability only) rounds the amount to an integer, halves toward positive infinity, when the unit is seconds, minutes, hours or days; months and years keep their fraction.
4. The amount is written by `stats/studied-today#T5.2` (shortest decimal, two fraction digits with halves away from zero, trailing zeros dropped, `,` grouping).
5. **Long form**: `<amount> <unit>`, with the unit singular exactly when the amount is written `1` (`second`, `minute`, `hour`, `day`, `month`, `year`), plural otherwise (`seconds` … `years`). **Short form** (history Time column): `<amount><suffix>` with no space; suffixes `s`, `m`, `h`, `d`, `mo`, `y`.

| Seconds | Long | Short |
|---|---|---|
| 0 | `0 seconds` | `0s` |
| 0.004 | `0 seconds` | `0s` |
| 0.005 | `0.01 seconds` | `0.01s` |
| 1 | `1 second` | `1s` |
| 1.004 | `1 second` | `1s` |
| 1.005 | `1.01 seconds` | `1.01s` |
| 4.5 | `4.5 seconds` | `4.5s` |
| 59.995 | `60 seconds` | `60s` |
| 60 | `1 minute` | `1m` |
| 70 | `1.17 minutes` | `1.17m` |
| 600 | `10 minutes` | `10m` |
| 3,599 | `59.98 minutes` | `59.98m` |
| 5,400 | `1.5 hours` | `1.5h` |
| 86,399 | `24 hours` | `24h` |
| 129,600 | `1.5 days` | `1.5d` |

### C5.3 Integers

Card id, note id, position, reviews, lapses and ratings are written as plain decimal digits with a leading `-` for a negative value, never grouped. Amounts inside time spans are grouped (C5.2 step 4).

### C5.4 Whole and two-decimal percents

A percent writes a binary64 value `x` from its exact binary value, not its shortest decimal:

- **Whole**: the integer nearest `|x|`, halves away from zero; `-` in front when `x < 0` (so −0.4 is `-0`, while −0.0 is `0`); then `%`.
- **Two decimals** (curve tooltip, C7.6): the same with two fraction digits always written (`90.12%`, `100.00%`).

Because the exact binary value is used, 8.855 gives `8.86` but 1.005 gives `1.00` (its binary value is just below 1.005), and 99.995 gives `100.00`. This differs from C5.2's amounts on purpose: Anki writes these values with a different number routine.

### C5.5 Custom data

The `cd` text, when not `""`, is written as follows:

1. It is read as JSON (RFC 8259). Text that is not valid JSON, or is the JSON `null`, is written unchanged.
2. A JSON object: one `<key>=<value>` part per member, joined by single spaces. Order: members whose key is an array index (a canonical decimal integer from 0 to 4,294,967,294) first in ascending numeric order, then the other members in the order they first appear. A repeated key keeps its first place and its last value.
3. A JSON array: the same with the element indexes as keys (`[1,2]` gives `0=1 1=2`). A JSON string: one part per UTF-16 code unit (`"ab"` gives `0=a 1=b`). A JSON number, `true` or `false`: the empty text (the row still shows, with an empty value).
4. A value is written as: a string unchanged; a number in JavaScript's number-to-string form (shortest round trip; `1.5` for `1.50`, `0` for `-0`, `1e+21`, `1e-7`); `true`, `false`, `null`; an array as its elements written by this rule (`null` elements as the empty text) joined by `,` (nested arrays flatten: `[[1,2],3]` is `1,2,3`, `[]` is empty); an object as `[object Object]`.

| `cd` | Text |
|---|---|
| `{"a":1,"b":"x"}` | `a=1 b=x` |
| `{"2":1,"1":2,"z":3}` | `1=2 2=1 z=3` |
| `{"v":null,"t":true,"f":1.50}` | `v=null t=true f=1.5` |
| `{"n":[1,2],"o":{"p":1}}` | `n=1,2 o=[object Object]` |
| `{"a":1,"b":2,"a":3}` | `a=3 b=2` |
| `{"a":1e21}` | `a=1e+21` |
| `{}`, `5`, `true` | `` (empty; the row shows, since the custom data text is not `""`) |
| `xyz`, `null` | `xyz`, `null` |

## C6. Review history

### C6.1 Entries

One entry per review-log row of the card, newest first (descending `id`). Each entry holds the raw values below (oracle: the `revlog` entries of `CardStatsResponse`) and the texts of C6.2.

| Raw value | Rule | Oracle |
|---|---|---|
| instant | C2 | `time` |
| kind | C2, 0 to 5 | `review_kind` |
| rating | the row's `ease` | `button_chosen` |
| interval seconds | `ivl` above 0: `ivl × 86,400`; otherwise `−ivl`; both saturating at 2,147,483,647 | `interval` |
| last interval seconds | the same from `lastIvl` | `last_interval` |
| factor | the row's `factor` | `ease` |
| time taken | the row's `time` (R5 read) as f32 divided by 1,000 in f32 | `taken_secs` |
| memory state | C6.3, optional | `memory_state` |

### C6.2 Texts

| Column | Label | Text |
|---|---|---|
| Date | Date | the local date; the date-and-time form (C5.1) is also returned for wide layouts (R1253, R1426) |
| Type | Type | `Learn`, `Review`, `Relearn`, `Filtered`, `Manual` or `Rescheduled` by kind |
| Rating | Rating | the rating as decimal digits (`0` for unrated rows) |
| Interval | Interval | long time span of the interval seconds (`0 seconds` for a Reset marker's 0) |
| Ease | Ease | C6.4 |
| Time | Time | short time span of the time taken (`3.5s`, `0.13s` for 125 ms, `1.02m` for 61,000 ms) |
| Stability | Stability | long time span of `stability × 86,400` from the entry's memory state; `""` when the entry has none or its stability is 0 |

- The Stability column is Meguri's (R1254); Anki computes the values but hides the column.
- Colouring is U222's (R1426): Learn uses the New state colour, Review the Review state colour, Relearn the Learn state colour, Filtered, Manual and Rescheduled no colour; a rating of 1 is drawn in the Learn state colour.

### C6.3 Per-entry memory states

When the card gives a memory item (`sched/fsrs-items#I5`, built from all its rows), replaying it gives one memory state per kept row, oldest first: for an incomplete history the first kept row gets the starting state (the SM-2 conversion) and each later kept row the state after its review; for a complete history each kept row gets the state after its review. Every row of the card then gets, in ascending `id` order:

| Row | Memory state |
|---|---|
| a kept row | its own state |
| a row before the first kept row | none |
| a row after the last kept row | the last kept row's state |
| any other row (between two kept rows: a cram, Manual, Rescheduled or unrated row) | the state of the **next** kept row |

The last rule is Anki's actual output (a row between two kept rows shows the state after the following review, not the preceding one); Meguri reproduces it (R1261). When the card gives no item, no entry has a state. States compare within O7's tolerance.

### C6.4 The Ease column

With `f` the row's `factor` and `p = f ÷ 10` in binary64:

- `f` is 0: the empty text.
- `p ≤ 110` (that is `f` ≤ 1,100): an FSRS difficulty (`sched/answering#A5` writes 100 to 1,100): `D:` followed by the whole percent of `p − 10` (C5.4), for example `D:12%`.
- otherwise an SM-2 ease: the whole percent of `p`, for example `250%`.

| `factor` | Text | `factor` | Text |
|---|---|---|---|
| 0 | `` | 1,100 | `D:100%` |
| 1 | `D:-10%` | 1,101 | `110%` |
| 95 | `D:-1%` | 1,105 | `111%` |
| 96 | `D:-0%` | 1,300 | `130%` |
| 100 | `D:0%` | 2,350 | `235%` |
| 224 | `D:12%` | 2,500 | `250%` |
| 1,005 | `D:91%` | 2,505 | `251%` |
| 1,055 | `D:96%` | 4,000 | `400%` |

## C7. Forgetting curve

### C7.1 When there is a curve

The snapshot has a curve when the card has a memory state (C3.4) and at least one curve review (C7.2). Otherwise it has none and the view shows no chart (R1427). Decided 2026-10-27: Meguri hides the chart when there is no curve review; Anki draws an empty chart with a no-data overlay whenever the card has a memory state. UI only, no deviation entry.

### C7.2 Curve reviews

Walk the history entries from newest to oldest and stop before the first entry that is a Reset marker (kind 4 with `factor` 0) or has no memory state (C6.3). Of the entries passed, the curve reviews are those whose kind is not Manual, not Rescheduled, and not a Filtered entry with `factor` 0 (R1257: manual, rescheduled and cram entries are skipped; the curve starts after the latest reset).

Write `r0` for the oldest curve review and `rN` for the newest, `t(r)` for an instant and `S(r)` for the stability of a review's memory state.

### C7.3 Ranges

- `first` = `(now − t(r0)) ÷ 86,400`, `since` = `(now − t(rN)) ÷ 86,400`, `last` = `rN`'s interval seconds ÷ 86,400, all binary64.
- `preview` = max(`last × 1.5 − since`, `last × 0.5`).
- `M` = `first + preview`.
- The ranges: First Week (limit 7 days), First Month (30), First Year (365), All Time (no limit). A range's **span** is min(`M`, its limit).
- Offered: First Week always; First Month when `M > 7`; First Year when `M > 30`; All Time when `M > 365`. The default is the last one offered. When only First Week is offered the view shows no range picker.

| History (`now` fixed) | `M` | Offered (default last) |
|---|---|---|
| one learning answer 1 h ago, interval 600 s | 0.0451 | Week |
| first answer 3 days ago, latest 1 day ago, interval 2 days | 5 | Week |
| first answer 4 days ago, latest 2 h ago, interval 3 days | 8.4167 | Week, Month |
| first and only answer 10 days ago, interval 1 day | 10.5 | Week, Month |
| first 20 days ago, latest 5 days ago, interval 15 days | 37.5 | Week, Month, Year |
| first 400 days ago, latest 100 days ago, interval 90 days | 445 | all four |

Decided 2026-10-27: the plan's U92 example ("a 10-day history offers only First Week, and it is the default") does not hold in Anki, where such a card is offered First Week and First Month with First Month selected. U92 and U222 test this table instead.

### C7.4 The curve

- Retrievability at instant `τ`, given a stability `S` and the instant `t` of the review it follows: with `x = (τ − t) ÷ 86,400`, `c = 0.9^(1 ÷ −decay) − 1`, `R = (x ÷ S × c + 1)^(−decay)`, all in binary64, times 100 for percent. The decay is the home preset's (`sched/presets#V6`), the same rule for a stored or a computed state.
- Between `t(r_i)` and `t(r_{i+1})` the curve follows `S(r_i)` from `t(r_i)`; at each later review it returns to 100 % (R1255, R1427). After `t(rN)` it follows `S(rN)`, through `now` and into the future.
- A range covers the instants from `t(r0)` to `t(r0) + span × 86,400`. Instants up to `now` are the past (drawn solid), instants from `now` on the future (drawn dashed); the point at `now` belongs to both.
- The desired retention line (R1255): the retention in force for the card's home deck (`sched/presets#V7`: deck override, else preset), whatever the card's `dr` says. Decided 2026-10-27: Anki draws the card's stored `dr`, or 0.9 when the card has none; Meguri follows R1255's effective retention, because a stored `dr` is out of date once the deck or preset retention changes. Only the drawn line and the tooltip's retention (C7.6) differ; no observation carries them, and the raw `desired_retention` value (C3.1) stays the card's `dr`.

### C7.5 Points (Meguri design)

The core returns, per offered range, a list of points so that the UI only draws (U92). Anki samples its curve in its web page, which no oracle observes; Meguri's rule is its own and is tested from this section:

- `step` = min(`span ÷ 1,000`, 1) days.
- Points: each curve review instant; inside each segment (between reviews, and from `t(rN)` to the range end), the instants `t + k × step` days for k = 1, 2, … strictly before the segment's end; the instant `now` when inside the range; the range end.
- At each review after `r0` two points share the instant: first the end value of the previous segment, then 100 %, so the line drops vertically.
- Each point holds: instant (binary64 epoch seconds), days since `t(r0)`, days since the review it follows, retrievability percent, the stability it follows, and past or future.
- Per range the core also returns `y_min = max(0, 100 − 1.2 × (100 − the smallest retrievability of its points))`; the view draws the desired-retention line only when that retention × 100 is above `y_min`.

### C7.6 Tooltip

For a point the core writes (R1256, R1428): elapsed time, the long time span of `days since the review × 86,400`; retrievability, the two-decimal percent of the point's value; stability, the long time span (not rounded) of `stability × 86,400`; and near the line, the desired retention as a whole percent of `retention × 100`. The point's date and time are locale formatting that U222 does itself, as Anki does (a date for spans of 365 days or more, a date and time otherwise). Labels: C12.

## C8. Debug dump (R1259, R1430)

`card_info_debug_json(card_id)` returns UTF-8 JSON, two-space indent, members in this order:

| Member | Content |
|---|---|
| `stats` | the raw values of C3 only, under C13.1's names, without the names `deck`, `original_deck`, `notetype`, `card_type` and `preset`; no `revlog` (the history is the `revlog` member below); `card_id` and `note_id` as JSON integers; an absent optional value as `null` (every member is always present) |
| `card` | the card row: every column of `model/row-records#R4` in table order, integers as JSON integers, `data` as its stored text |
| `revlog` | newest first, one object per row: `row` (every column of R5 in table order, as stored) and `info` (C6.1's raw values for that row) |
| `deck` | the current deck as its schema-11 object (`model/schema11-json`) without `name`, `desc` and `usn` |
| `homeDeck` | only for a card in a filtered deck: the home deck likewise |
| `preset` | the home preset as its schema-11 object without `name` |
| `rollover` | the `rollover` config value as stored, `null` when absent |

- f32 values are written as the shortest decimal that reads back as the same f32 (`model/json-numbers`).
- No name or text a user typed is in the dump apart from custom data (`stats.custom_data` and `card.data`) and a filtered deck's search terms (`terms` in its deck object), so the dump can be shared in a bug report. Anki drops the card type, note type and preset names and the deck object's name but keeps the home deck name; Meguri drops that too.
- Decided 2026-10-27: the shape is Meguri's. Anki's own dump follows its internal message layout, which no client reads; R1430 asks for the same information, not the same bytes.

## C9. Previous Card Info (R1262, R1263, R1264)

- **Meguri.** In a review session, the previous card is the card of the last answer still in the session record (U175): undone answers are dropped from the record, so after undoing an answer it is the card answered before the restored one (R1264). Before the session's first answer, and after undoing every answer, there is none and the view's Previous switch is disabled (R1262). The previous card can be the current card itself, when the card just answered is shown again at once.
- Bury, suspend, delete and other moves to another card without an answer leave the previous card unchanged. A previous card deleted since its answer gives C11's not-found error.
- The id is all the core receives (C1); the snapshot is the same as for Card Info.
- **Anki.** Its previous card is the card shown just before the current one, whatever moved the reviewer on (an answer, bury, suspend, delete or undo); after an undo it is the card that was showing before the undo. Decided 2026-10-27: Meguri follows the plan (R1264, U222). No observation covers the session, so this needs no deviation entry; U222 and U183 test it.

## C10. Freshness (R1260, R1429)

- The snapshot also returns what it depends on: the card id, note id, note type id, current deck id, home deck id, home preset id and the review-log ids it read.
- `card_info_affected(dependencies, change_set)` (`engine/change-stream#S1`) is true when the change set lists the card, the note, the note type, either deck or the preset; removes one of the review-log ids; adds a review-log row whose `cid` is the card (an indexed read of the added ids); changes the config key `rollover`; or reports `collection_row`. Otherwise false. U103 derives `CardStatsChanged` from it.
- A new scheduler day changes the due date of a day-number due and the retrievability; U103 also refreshes on the day clock's day change. Retrievability is not refreshed on a timer.

## C11. Errors and damaged records

The snapshot fails with a typed error naming the missing record when: the card does not exist (`not_found`, the view shows "(No card to display.)"); its note, note type, the template its `ord` names (normal note types), its current deck or its home deck does not exist; its home deck is a filtered deck; its home preset does not exist; the home preset's cutoff date is refused (`sched/fsrs-items#I1.3`); or the crate fails while replaying the history. Anki's card stats fail in each of these cases. Check Database repairs the damaged references (U128).

## C12. Strings

Labels are the UI's (String Catalog, English values here); value texts are the core's.

| Where | Text |
|---|---|
| row labels | `Added`, `First Review`, `Latest Review`, `Due`, `Position`, `Interval`, `Stability`, `Difficulty`, `Retrievability`, `Reviews`, `Lapses`, `Average Time`, `Total Time`, `Card Type`, `Note Type`, `Deck`, `Preset`, `Card ID`, `Note ID`, `Custom Data` |
| history column labels | `Date`, `Type`, `Rating`, `Interval`, `Ease`, `Time`, `Stability` |
| kinds (core) | `Learn`, `Review`, `Relearn`, `Filtered`, `Manual`, `Rescheduled` |
| curve | `Forgetting Curve`, `First Week`, `First Month`, `First Year`, `All Time`, `Desired Retention`, `Elapsed Time`, `Retrievability`, `Stability`; tooltip date label `Date` (spans of 365 days or more) or `Date Time` (shorter spans) |
| actions and states | `Card Info`, `Previous Card Info`, `Copy Debug Info`, `Copied to clipboard`, `(No card to display.)` |
| time span units (core) | `second`/`seconds`, `minute`/`minutes`, `hour`/`hours`, `day`/`days`, `month`/`months`, `year`/`years`; short `s`, `m`, `h`, `d`, `mo`, `y` |
| value patterns (core) | `<date> @ <HH:MM>`, `<deck> (<home deck>)`, `D:<n>%`, `<n>%`, `<a> (<b> days)`, `<key>=<value>` |

No text contains U+2068 or U+2069. Anki wraps each number in that pair of invisible direction marks; Meguri leaves them out (as `stats/studied-today`).

## C13. Oracle observation `card_stats` (for the oracle-maintainer)

### C13.1 What the driver must return

- **Request.** `{card_stats: [#cN, ...]}`: a non-empty list of card references.
- **Value.** `{"#cN": <stats>}`. `<stats>` holds the fields of the `CardStatsResponse` message for that card, as the backend returns them at the clock's current instant:
  - `card_id` as `#cN`, `note_id` as `#nN`;
  - `deck`, `original_deck` (`null` when not set), `card_type`, `notetype`, `preset`, `custom_data`: strings;
  - `added`, `first_review`, `latest_review`, `due_date` (epoch seconds), `due_position`: integers, `null` when the optional field is not set;
  - `interval`, `ease`, `reviews`, `lapses`: integers;
  - `average_secs`, `total_secs`, `desired_retention` (`null` when not set), `fsrs_retrievability` (`null` when not set): f32 values written as their shortest f32 decimal;
  - `memory_state`: `{stability, difficulty}` (f32) or `null`;
  - `fsrs_params`: the list of f32 values;
  - `revlog`: the entries in the message's order (newest first), each `{time, review_kind, button_chosen, interval, ease, taken_secs, memory_state, last_interval}`, `review_kind` as its number 0 to 5 and `memory_state` as above or `null`.
- **Oracle.** The backend's card-stats call for the card's id. Scenarios set the config key `fsrs` to true in `setup.config` (Anki computes a missing memory state only with FSRS on, and Meguri always runs it). The frozen clock fixes `now`, including the instant the due estimate adds days to.
- **Meguri.** The raw values of the core's snapshot (C3, C6.1) under the same names.
- **Comparison.** Exact, except `memory_state` (top level and per entry) and `fsrs_retrievability`, within a relative 1e-5 under `conformance/observations#O7`'s rule (the row C13.2 adds there).
- **Errors.** A card stats failure is a run error (`anki_error` in the oracle; the matching typed error in Meguri, C11).
- **No text.** The pylib backend returns no display text (R1887). If U226's headless graphs-page image lands, a later `card_info_text` kind may record the card-info page's texts; until then C4 to C7 are tested from this spec.

### C13.2 Where it is defined

When the driver support lands, the oracle-maintainer adds this kind to `specs/conformance/stats-ops.md` (as H9), to `observations.md` O4, the O7 tolerance row (1e-5 for `stability` and `difficulty` in any `memory_state` and for `fsrs_retrievability` inside `card_stats`), and its shape to `conformance/schema/`; this section then points there. The driver op belongs in `oracle/driver/ops/stats.py`.

### C13.3 Anki's write

When the card's data has no `lrt`, Anki's card stats write the card row back. `mod`, `usn` and the other columns keep the values Anki read. `data` is rebuilt whole in `model/card-data-json` C4 form from the keys C3 reads, plus `lrt` = the instant C3.5 step 1 finds (0 when there is no such row). Besides adding `lrt`, the rebuild puts the keys in C4 order, rounds `s`, `d`, `dr` and `decay` (C4 rule 2), drops unknown keys and drops a `cd` of `""` or `{}`. Data already in C4 form changes only by the added `lrt`. When the data has `lrt`, Anki writes nothing.

Later Anki steps on that card (another `card_stats`, an answer, a search) read the rebuilt data. Meguri writes nothing (R1252, R1425) and keeps the original data, so after the observation the two sides can start from different inputs.

### C13.4 Deviation entries the scenarios need

| Entry | Requirement | Difference | Transform |
|---|---|---|---|
| `card-info-no-lrt-backfill` | R1252, R1425 | after a `card_stats` observation Anki's card data holds an `lrt` it did not hold before | drop `lrt` from the observed card's `data` in Anki's later `card` and `db_dump` observations when it was absent before |
| `card-info-computed-retrievability` | R1251, AE46 | for a card whose data has no memory state, Meguri returns `fsrs_retrievability`, Anki `null` | ignore `fsrs_retrievability` when Anki's is `null` and the card's data has no `s`/`d` |

The `card-info-no-lrt-backfill` transform covers only the added `lrt`, so card-info scenarios keep C13.3's rebuild from changing anything else:

- A card observed with `card_stats` whose data has no `lrt` stores its data in C4 form: C4 key order, `s`, `d`, `dr` and `decay` already rounded to their C4 places, no unknown keys, no `cd` of `{}`. Data written with `db_exec` follows this too.
- A card whose data cannot be in C4 form (the `{}` value of `custom-data`) has an `lrt` in its data, so Anki writes nothing.
- A scenario observes `card_stats` after the ops whose effect it checks on that card, not before them.

## C14. Scenarios and tests

### C14.1 Scenarios for the oracle-maintainer

None is in the tree: the driver has no `card_stats` op and the runner no adapter (C13.2). Each lands with its golden recorded at the current oracle pin, under `conformance/scenarios/stats/card-info/` and `conformance/goldens/stats/card-info/`. All set the config key `fsrs` to true and build history with `answer` or with `db_exec` revlog inserts (`specs/conformance/stats-ops.md` H3). All follow C13.4's data rules.

| Scenario | Pins |
|---|---|
| `new-card` | C3.3 position from `due`, no due, no memory state, empty history, `total` 0; rows 1, 5, 10, 11, 14 to 19 |
| `review-card` | answered L L R R with stored state: due from a day number, interval, memory state, retrievability from `lrt`, first/latest review, per-entry states for a complete history, `D:xx%` factors |
| `ae46-no-state` | review card with history, data without `s`, `d`, `lrt`: computed memory state, retrievability (deviation `card-info-computed-retrievability`), `card` before and after (deviation `card-info-no-lrt-backfill`), `col.mod` unchanged on Meguri |
| `no-lrt-elapsed` | stored state, no `lrt`: elapsed from the latest rated non-cram row, with a later cram row and a rated Manual row |
| `filtered-card` | card moved into a filtered deck with `db_exec` (`did`, `odid`, `odue`): due from `odue`, `original_deck`, home preset, and a new card there (position from its filtered `due`) |
| `intraday-learning` | learning card with an epoch-seconds due: `due_date` equals it |
| `due-estimate-night` | rollover 4, observed at 02:00 local: due instant `now + days × 86,400` (C5.1) |
| `reset-position` | answered card with `pos`, then Forget with and without restore: position, the Reset marker, per-entry states after it |
| `mixed-history` | SM-2-era rows (`factor` 2500, incomplete history with starting state), then FSRS rows, a Set Due Date row, a Rescheduled row, a cram row, an unrated Review row between kept rows: kinds, factors, C6.3's mapping including the next-state rule |
| `times` | rated and unrated rows with times, one of 8,855 ms: `average_secs`, `total_secs`; a card whose rows all have time 0 |
| `custom-data` | `cd` values of C5.5 as stored text (each card's data with an `lrt`, C13.4), `{}` giving `custom_data` `{}` |
| `cloze-type` | a cloze card with `ord` 2: `card_type` is the first template's name |
| `damaged` | a card whose deck was removed with `db_exec`: run error on both sides |

### C14.2 Unit tests

`core/crates/meguri-stats/tests/card_info.rs` asserts every table of C4 to C7 (row order and applicability, C4.2, C4.3, C5.1 to C5.5, C6.4, C7.3 and the point rules of C7.5) and C1's no-write property (card row bytes, `col.mod` and the WAL frame count unchanged).

## C15. Not covered

- Collections with FSRS off: Meguri always runs FSRS (R1250); Anki would then report no computed memory state and an Ease row.
- Review-log values stored as REAL, TEXT or BLOB beyond `model/row-records#R5`'s read rules.
- Day-number dues far enough from today to overflow 32-bit arithmetic.

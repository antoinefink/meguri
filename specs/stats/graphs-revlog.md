# Stats review-log graphs

Spec: stats/graphs-revlog
Requirements: R1271, R1277, R1367, R1379, R1380, R1381, R1382, R1383, R1384, R1385, R1386, R1387, R1388, R1389, R1405, R1406, R1407, R1408, R1410, R1411, R1412, R1413, R1414, R1446
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/graphs/calendar/, conformance/scenarios/stats/graphs/reviews/, conformance/scenarios/stats/graphs/retention/, conformance/scenarios/stats/graphs/hourly/, conformance/scenarios/stats/graphs/buttons/

The data of the five review-log graphs (U99): Calendar, Reviews, Retention (the True Retention table), Hourly Breakdown and Answer Buttons. They run on the machinery of `stats/graphs-common`: scope G2, history G3, row reading G4, graph order G5, aggregate shape G6, requests G8, caches G9. This file says, per graph, which review-log rows count, the keys and values of its record, and its extras. Ranges, bins, the "0–4" first-bin shift, table values, percentages, "N/A" cells, tooltip and day text, and the colour scale are `stats/graph-presentation` (U226, `stats/graphs-common` G12.1). The Calendar cells' item queries are `stats/browse-queries` Q4.2; the other four graphs have no browse queries.

Every value here equals Anki's graph data for the same collection, scope, history and clock (R1446). The Calendar dates (V3.2) are not part of Anki's graph data; they follow Anki's page except where V3.3 says.

## V1. Rows each graph counts

The rows are those G2 and G3 select, read as G4 says: `type` as its kind (so 260 reads as Manual, −1 and 6 as Learn), `time` as 0 outside `u32`, `ivl` and `lastIvl` as integers. `lastIvl` is the interval before the answer: days when positive, seconds when negative (`model/row-records` R5). A deleted card's row counts in every graph of the Collection scope and in no other scope (G2, `stats/counting-rules` N1). A row counts once per answer: nothing is de-duplicated per card or per day (R1278, R1408).

By kind:

| `type` | Reviews, Calendar (V2, V3) | Hourly (V5) | Answer Buttons (V6) | Retention (V4) |
|---|---|---|---|---|
| 0 Learn | learn | yes | Learning | only with a long last interval |
| 1 Review | young (`lastIvl` < 21) or mature (≥ 21) | yes | Young or Mature, same split | yes |
| 2 Relearn | relearn | yes | Learning | only with a long last interval |
| 3 Filtered | filtered | no | Learning | not with `factor` 0; else only with a long last interval |
| 4 Manual | no | no | no | only with a long last interval |
| 5 Rescheduled | no | no | no | only with a long last interval |

A long last interval is `lastIvl` ≥ 1 (a day or more) or `lastIvl` ≤ −86,400 (86,400 seconds or more). Retention's column gives the kind condition only; a row also needs a rating there (next table). Every `lastIvl` below 21, negative values included, is young, in every graph.

By `ease`:

| `ease` | Reviews, Calendar | Hourly | Answer Buttons | Retention |
|---|---|---|---|---|
| 0 | counted | in the total, not correct | left out | left out |
| 1 | counted | in the total, not correct | button 1 | fail |
| 2 to 4 | counted | correct | buttons 2 to 4 | pass |
| 5 to 255 | counted | correct | left out | pass |

- So an unrated Learn or Review row (other clients) counts in Reviews, Calendar and Hourly. Set Due Date, Forget and bulk reschedule rows (Manual and Rescheduled with `ease` 0) count nowhere. A rated Manual row (other clients) counts only in Retention, when its last interval is long.
- These rules are Anki's, graph by graph. They differ from each other, and from the Today section's (`stats/studied-today` T3.1) and the heatmap's (`stats/counting-rules` N2), on purpose: each matches its oracle.

## V2. Reviews

### V2.1 Values

Each counted row (V1) adds 1 to its kind's count and its `time` to its kind's time, under its day index d (`stats/day-buckets` B3: `floor(id / 1,000) − N`, divided by 86,400 with rounding toward zero). The five kinds are learn, relearn, young, mature and filtered (R1385).

- Times are the stored values as G4 reads them, in milliseconds, summed with no cap and no recomputation. Meguri sums them in 64 bits (V9).
- Days are fixed 86,400 s spans counted back from `N`, also across DST changes (R1269). Day 0 holds rows with `N − 86,400 < t < N + 86,400`, where t is the row's whole second; rows dated after `N` land on day 0 or later.
- With `last12Months`, the window starts at `t = N − 366 × 86,400` (G3), so the oldest key is −366, which holds only rows of that exact second; −365 is the oldest full day. With `allHistory`, the keys go back to the oldest row with `id ≥ 0`.

| `id` (`N` = 1791856800) | d |
|---|---|
| 1760234399999 | not read with `last12Months`; −366 with `allHistory` |
| 1760234400000 (window start) | −366 |
| 1760234401000 | −365 |
| 1791770400000 (`N` − 86,400 s) | −1 |
| 1791770401000 | 0 |
| 1791856800000 (`N`) | 0 |
| 1791943199999 | 0 |
| 1791943200000 (`N` + 86,400 s) | 1 |

### V2.2 Record

`count` and `time`: two sparse maps day index → `{learn, relearn, young, mature, filtered}`. A key is present in both maps exactly when at least one row counted on that day; a kind with nothing is 0. Nothing else is in the record (G6).

- Size: with all history and study on every day since 2006 (about 7,400 keys) the record holds about 74,000 numbers, most of them small. G6's 256 KB limit holds only with an encoding of U103's that averages under 3.5 bytes per number.

What U226 needs from it: the two maps, `today`, `N`, `R` and the offset of G1 for day labels, and the history (G3) to offer the "all" range only with `allHistory` (R1367, R1387). Days studied, totals, averages and answer speed (R1389) are computed there from the maps.

## V3. Calendar

### V3.1 Counts

The count of day index d is the sum of the five kinds of V2's `count` for d (R1379): every row Reviews counts, of every kind, under the same day index. The Calendar has no rule of its own for rows, scope or history.

### V3.2 The date of a day index

Decided 2026-10-27: the calendar date of day index d is the date D₀ + d days, where D₀ is the local date of the instant `now − R × 3,600 s`, read in the device zone (`stats/day-buckets` B5) with the UTC offset in force at that moved instant (R1384). `now`, the zone and `R` are G1's. `N` plays no part in the date.

- D₀ is today's local date from the rollover hour on, and yesterday's before it: Paris, `R` 4, at 03:30 day 0 is yesterday's date, at 04:00 today's.
- Rows logged within an hour of a DST change can therefore get a Calendar date different from their heatmap day (`stats/day-buckets` B4.1, B4.4). Both are right (R1271, AE58).

| now (Europe/Paris, `R` 4) | d | Date |
|---|---|---|
| 2026-03-29 12:00 +02:00 | −1, 0, 1 | 2026-03-28, 03-29, 03-30 |
| 2026-06-10 03:30 +02:00 | −1, 0, 1 | 2026-06-08, 06-09, 06-10 |
| 2026-06-10 12:00 +02:00 | −74, −73, −72 | 2026-03-28, 03-29, 03-30 |
| 2026-06-10 04:15 +02:00 | −74, −73, −72 | 2026-03-28, 03-29, 03-30 |
| 2026-11-10 12:00 +01:00 | −17, −16, −15 | 2026-10-24, 10-25, 10-26 |
| 2026-11-10 03:30 +01:00 | −17, −16, −15 | 2026-10-23, 10-24, 10-25 |
| 2026-03-29 04:30 +02:00 | −2, −1, 0, 1 | 2026-03-26, 03-27, 03-28, 03-29 |
| 2026-10-25 03:30 +01:00 | −2, −1, 0, 1 | 2026-10-23, 10-24, 10-25, 10-26 |

### V3.3 Where Anki's page differs

Anki's page moves now's local date and time of day by d calendar days first, then moves that instant back `R` hours (a fixed span) and takes its local date. Call a date "crossed" when moving its wall-clock time of now back `R` hours lands before that date's local midnight; with no offset change between midnight and that time of day, that is exactly when the time of day is before `R`:00. Anki's date for d is the target date (now's date + d) when the target date is not crossed and the day before when it is; V3.2's is D₀ + d, which is now's date + d when now's date is not crossed and the day before when it is. So the two differ exactly when one of now's date and the target date is crossed and the other is not. That needs an offset change between local midnight and now's time of day on one of the two dates, near the rollover hour:

- **(a) The target date has the change.** Two neighbouring day indices get the same date and the date next to it gets none. In V3.2's table: the fourth row (Anki: −74 and −73 on 2026-03-28, nothing on 03-29) and the sixth (Anki: −16 and −15 on 2026-10-25, nothing on 10-24). Every other day index keeps V3.2's date.
- **(b) Now's date has the change** (now is just after the rollover hour on a spring-forward date, or just before it on a fall-back date). Every d other than 0 moves by one date (except a target date that is crossed as now's is), so day 0 shares its date with a neighbour and the date on its other side gets none. In row 7 Anki gives −2 → 03-27, −1 and 0 → 03-28, 1 → 03-30 (nothing on 03-29); every d < 0 is one date later than V3.2's. In row 8 Anki gives −2 → 10-22, −1 → 10-23, 0 and 1 → 10-25 (nothing on 10-24); every d < 0 is one date earlier. During that hour of the change date, every past day of Anki's calendar is one date off.

Where two day indices share a date, the page draws only one of the two counts (which one depends on the order of the map's keys). Meguri keeps every day index on its own date (V3.2), so no count is hidden and no date is shifted. Pending `conformance/deviations.yaml` entry `stats-calendar-dst-date`, added with the Meguri-only test of V10 that asserts the table.

### V3.4 Record

| Member | Value |
|---|---|
| `day0_date` | D₀ (V3.2), `YYYY-MM-DD` (`stats/day-buckets` B4.5) |
| `today_date` | the local date of now, unmoved |
| cells | day index d → its count (V3.1) and, when d ≤ 0, its item text (`stats/browse-queries` Q4.2); only days with a count above 0 |
| `max_count` | the largest cell count in the record, all years and dates after today included (R1382) |

- A cell's date is `day0_date` + d. Cells dated after `today_date` (rows after now, clock skew) stay in the record and in `max_count`, as in Anki's data; whether they are drawn is U226's (R1380).
- What U226 needs besides: `firstDayOfWeek` (G7, R1381; its menu writes through G7), and the history for year navigation and the cells it fills in (R1380). The cells U226 fills with no reviews, the year bounds and the colour scale are `stats/graph-presentation`'s.
- The cells hold every day of the Reviews `count` map, so all years are on hand and year navigation never calls the core (R1290, R1355).

## V4. Retention

### V4.1 Rows

A row counts for retention when all of these hold (R1405):

- its `ease` is 1 or above (it has a rating);
- it is not a cram row: `type` Filtered with `factor` 0 (an answer in a filtered deck that does not reschedule);
- its `type` is Review, or its last interval is long (V1).

Of a counted row: young when `lastIvl` < 21, mature otherwise; fail when `ease` is 1, pass otherwise (R1405). Every counted row counts, however many a card has that day (R1408).

### V4.2 Periods

With t the row's whole second (`floor(id / 1,000)`), a row is in a period when `start ≤ t < end`:

| Period | start | end |
|---|---|---|
| `today` | `N` − 86,400 | `N` |
| `yesterday` | `N` − 2 × 86,400 | `N` − 86,400 |
| `week` | `N` − 7 × 86,400 | `N` |
| `month` | `N` − 30 × 86,400 | `N` |
| `year` | `N` − 365 × 86,400 | `N` |
| `all_time` | 0 | `N` |

- The bounds are fixed spans from `N` (R1406), whatever the zone's offset changes. A row stamped in the second `N` − 86,400 is in `today` here but on day −1 in Reviews (V7).
- A row with `t ≥ N` (clock skew) is in no period, `all_time` included.
- `all_time` is in the record only with `allHistory` (G3, R1367, AE59). With `last12Months` the rows of the 366th day back are read but fall in no period that is returned.

### V4.3 Record

One entry per period: `{young_passed, young_failed, mature_passed, mature_failed}`, the four counts of V4.1. Nothing else (G6). The Young, Mature and All tables, the percentages with one decimal and the "N/A" cells (R1407) are U226's, from these counts.

## V5. Hourly Breakdown

### V5.1 Hour

The hour of a counted row (V1: Learn, Review and Relearn) is the hour of its local time at the zone's current offset: `floor((t + offset) / 3,600)` taken modulo 24 as a value from 0 to 23, with t the row's whole second and offset G1's (the offset at now, seconds east). Every row uses that one offset, summer and winter rows alike (R1411). A row with `t + offset < 0` (west of UTC, in the first hours after the epoch, read only with `allHistory`) is in hour 0 (V9).

- Paris, now in summer (+02:00): a winter review at 09:30 +01:00 (08:30 UTC) is in hour 10; a summer review at 09:30 is in hour 9.
- Asia/Kathmandu (+05:45): 09:59:59.999 local is hour 9, 10:00:00.000 hour 10, 23:59:59.999 hour 23, 00:00 hour 0. Half- and quarter-hour offsets give the local wall-clock hour.

### V5.2 Ranges

| Range | Rows |
|---|---|
| `one_month` | `t ≥ N − 30 × 86,400` |
| `three_months` | `t ≥ N − 90 × 86,400` |
| `one_year` | `t ≥ N − 365 × 86,400` |
| `all_time` | every row the request reads (G3) |

- No range has an upper end: rows dated after `N` count in all four.
- `all_time` is in the record only with `allHistory` (G3, R1367). With `allHistory` the other three hold the same rows as with `last12Months`.

### V5.3 Record

For each range, 24 entries, hour 0 to 23: `{total, correct}`, where `total` counts the range's rows in that hour and `correct` those with `ease` above 1 (R1410). The range picker, its default, percentages and text (R1412) are U226's.

## V6. Answer Buttons

### V6.1 Groups

A row counts when its kind gives a group and its `ease` is 1 to 4 (R1413):

| Kind | Group |
|---|---|
| Learn, Relearn, Filtered (cram rows included) | Learning |
| Review, `lastIvl` < 21 | Young |
| Review, `lastIvl` ≥ 21 | Mature |
| Manual, Rescheduled | none: left out |

Rows with `ease` 0 or above 4 are left out.

### V6.2 Record

For each range of V5.2 (same bounds, `all_time` only with `allHistory`): `{learning, young, mature}`, each four counts for buttons 1 to 4. Each group's total presses and correct percentage, and the share of each button (R1414), are U226's, from these counts.

## V7. Edges compared

With t a row's whole second:

| t | Reviews, Calendar day | Retention | Hourly, Answer Buttons `one_month` |
|---|---|---|---|
| `N` − 30 × 86,400 | −30 | `month` | yes |
| `N` − 30 × 86,400 − 1 | −30 | not `month` | no |
| `N` − 86,400 − 1 | −1 | `yesterday` | yes |
| `N` − 86,400 | −1 | `today` | yes |
| `N` − 86,400 + 1 | 0 | `today` | yes |
| `N` | 0 | none | yes |
| `N` + 86,400 | 1 | none | yes |

The Today section (`stats/studied-today` T3.1) and the Home line (`stats/day-buckets` B2) have their own edges.

## V8. Observation

U99's parts of the `stats` kind (`stats/graphs-common` G14), in G14's form (field names as Anki's protobuf file declares them, map keys as decimal strings, members at their default omitted), plus V8's own additions, which G14 does not state and which narrow its default rule for U99's parts: only scalar members at 0 are omitted; every message member and every list is written, empty or not; an `hours` entry with both counts 0 is written `{}`. Both sides serialise U99's parts this way:

- `reviews` as G14 defines it; it is also the Calendar's observation (V3.1).
- `true_retention`: `{today, yesterday, week, month, year, all_time}`, each `{young_passed, young_failed, mature_passed, mature_failed}`.
- `hours`: `{one_month, three_months, one_year, all_time}`, each a list of 24 `{total, correct}`.
- `buttons`: `{one_month, three_months, one_year, all_time}`, each `{learning, young, mature}`, each a list of 4 counts.
- With history `year`, the `all_time` members of these three parts are left out on both sides: Anki computes them and hides them, Meguri has none (G3, G13).

The Calendar's dates, `max_count` and item texts (V3.4) are not in Anki's graph data and are not observed. Meguri tests them from V3.2's table and V3.4; the item texts are Q4.2's, and the scenario `calendar-cells` (`stats/browse-queries` Q8) checks what they find.

## V9. Differences from Anki

- **Calendar dates at DST changes.** Anki's page gives a day index a date one off from V3.2's whenever the `R`-hour move back crosses local midnight on only one of now's date and the target date (V3.3): (a) a target date with an offset change merges two day indices and leaves a date empty; (b) for one hour on a change date, every day index but 0 is one date off and day 0 merges with a neighbour. Meguri keeps V3.2's dates in both cases. Pending entry `stats-calendar-dst-date`.
- **Review time sums.** Anki adds a day's times in 32 bits, so a day whose time of one kind reaches 2³² ms (about 49.7 days) wraps around. Meguri sums in 64 bits and never wraps. Only rows with stored times near the `u32` maximum reach it. Decided 2026-10-27. Pending entry `stats-review-time-no-wrap`, added with the Meguri-only test of V10.
- **Hours of rows just after the epoch.** With a negative offset (west of UTC), a row in the first hours after the epoch (`0 ≤ t < −offset`, read only with `allHistory`) has `t + offset < 0`. Anki puts it in hour 0 when `−3,600 < t + offset < 0`, and fails its whole graph request when `t + offset ≤ −3,600`. Meguri puts every such row in hour 0 (V5.1) and returns data. Decided 2026-10-27: equal to Anki wherever Anki returns data, and one rule for the rows where it fails, so that one imported row of 1970 never stops the Stats screen. The difference is only those failing rows. Pending entry `stats-hourly-epoch-rows`, added with the Meguri-only test of V10.
- Parts that need all history are left out with `last12Months` (G13).

## V10. Scenarios

For the oracle-maintainer, under `conformance/scenarios/stats/graphs/`, with the `stats` kind (V8) once `oracle/driver/ops/stats.py` exists (none does yet, `stats/graphs-common` G15). Review-log rows come from `db_exec` (`conformance/stats-ops` H3), on a deleted card's id unless a scope is named. All in Europe/Paris, rollover 4, now 2026-10-12 12:00 +02:00 (`N` = 1791856800), Collection scope and `year`, unless named. "Today at hh:mm" is local time on 2026-10-12; a row given only as "today" is at 12:00, and a row given as "k days ago" at 12:00 on that date. A row is type 1, `lastIvl` 30, `ease` 3, `factor` 2500, `time` 0 unless named. Values are this file's; the goldens are the authority once recorded.

| Scenario | Pins | Expected |
|---|---|---|
| `reviews/kinds-split` | V1, V2.1, R1385, R1386 | ten rows today at 12:00: type 0 (time 1,000); type 2 `ease` 1 (2,000); type 1 `lastIvl` 20 (3,000), 21 (4,000) and −600 (5,000); type 3 `factor` 0 (6,000); type 3 `ease` 0 (7,000); type 4 `ease` 0 (8,000); type 5 `ease` 0 (500); type 1 `ease` 0 (9,000): `reviews` count {0: learn 1, relearn 1, young 2, mature 2, filtered 2}, time {0: learn 1,000, relearn 2,000, young 8,000, mature 13,000, filtered 13,000}; `hours` hour 12 {total 6, correct 4} in every range |
| `reviews/day-edges` | V2.1 | rows with the ids 1791770399999, 1791770400000, 1791770400500, 1791770401000, 1791856800000, 1791943199999, 1791943200000: count {−1: 3, 0: 3, 1: 1}, all mature |
| `reviews/window-start` | V2.1, G3 | ids 1760234399999, 1760234400000, 1760234401000: `year` {−366: 1, −365: 1}; `all` {−366: 2, −365: 1} |
| `calendar/ae58-paris-spring-forward` (AE58) | V3.2, R1271 | now 2026-03-29 12:00 +02:00 (`N` = 1774836000); rows at 03:30 +02:00 (id 1774747800000) and 04:30 +02:00 (1774751400000) that day: `reviews` count {−1: 1, 0: 1} (Anki oracle); `heatmap_totals` home {2026-03-28: 2} (add-on oracle). Meguri's Calendar dates are 2026-03-28 and 2026-03-29 (Meguri test) |
| `calendar/history-years` | V3.1, V3.4, R1380 | rows on 2019-05-01, 2025-12-31, twice on 2026-01-02 and today, each at 10:00: `year` count keys {−285: 1, −283: 2, 0: 1}; `all` adds {−2721: 1}. Meguri test: dates 2019-05-01 to 2026-10-12, `max_count` 2 |
| `retention/row-filter` | V4.1, R1405, R1408 | eleven rows today: type 1 `lastIvl` 10 `ease` 1; type 1 `lastIvl` 30; type 0 `lastIvl` 1; type 0 `lastIvl` −600; type 2 `lastIvl` −86,400 `ease` 1; type 2 `lastIvl` −86,399 `ease` 1; type 3 `factor` 0 `lastIvl` 10; type 3 `lastIvl` 10; type 1 `lastIvl` 10 `ease` 0; type 4 `lastIvl` 15; type 1 `lastIvl` 25 `ease` 5: `today`, `week`, `month`, `year` and (with `all`) `all_time` each {young_passed 3, young_failed 2, mature_passed 2}, `yesterday` {} |
| `retention/periods` | V4.2, R1406 | rows at ids `N`×1,000 − 1, `N`×1,000, then for k = 1, 2, 7, 30, 365 at (`N` − k × 86,400)×1,000 and that − 1: with `all`, mature_passed `today` 2, `yesterday` 2, `week` 6, `month` 8, `year` 10, `all_time` 11; the row at `N` in none |
| `retention/ae59-history-parts` (AE59) | G3, V4.2, V5.2, V6.2 | rows today and 400 days ago at 12:00: `all`: `true_retention` `all_time` mature_passed 2, `year` 1; `hours` `all_time` hour 12 {total 2, correct 2}; `buttons` `all_time` mature [0, 0, 2, 0]; `year`: the same without `all_time`. Meguri test: the `last12Months` records have no all-time member |
| `hourly/row-kinds` | V1, V5.3, R1410 | rows today at 10:15: type 0; type 1 `ease` 1; type 2 `ease` 2; type 3; type 4 `ease` 0; type 5 `ease` 0; type 1 `ease` 0; type 1 `ease` 5: hour 10 {total 5, correct 3} |
| `hourly/current-offset` | V5.1, R1411 | now 2026-06-10 12:00 +02:00 (`N` = 1781143200), `all`: a Good row at 2026-01-15 09:30 +01:00 (id 1768465800000), an Again row at 2026-06-01 09:30 +02:00 (1780299000000): `one_month`, `three_months` hour 9 {total 1}; `one_year`, `all_time` hour 9 {total 1}, hour 10 {total 1, correct 1} |
| `hourly/kathmandu-edges` | V5.1 | Asia/Kathmandu, now 2026-10-12 12:00 +05:45: ids 1791778499999, 1791778500000, 1791742499999, 1791742500000: hours 9, 10, 23 and 0, one each, in every range |
| `hourly/near-epoch` | V5.1, V9 | America/New_York, now 2026-10-12 12:00 −04:00, `all`: a row at id 12600000 (`t + offset` = −1,800): `hours` `all_time` hour 0 {total 1, correct 1}; every hour of the other three ranges `{}` |
| `hourly/range-edges` | V5.2, V6.2, V4.2 | `all`; rows at (`N` − k × 86,400)×1,000 and that − 1 for k = 30, 90, 365, and an Again row at (`N` + 3,600)×1,000: `hours` `one_month` {4: 1/1, 5: 1/0}, `three_months` {3: 1/1, 4: 2/2, 5: 1/0}, `one_year` {3: 2/2, 4: 3/3, 5: 1/0}, `all_time` {3: 3/3, 4: 3/3, 5: 1/0} (total/correct); `buttons` mature [1, 0, 1, 0], [1, 0, 3, 0], [1, 0, 5, 0], [1, 0, 6, 0]; `true_retention` `month` 1, `year` 5, `all_time` 6 mature_passed |
| `buttons/groups` | V6.1, R1413 | thirteen rows today: type 0 `ease` 1, 2, 3, 4; type 2 `ease` 1; type 3 `factor` 0; type 1 `lastIvl` 20 `ease` 2; type 1 `lastIvl` −600; type 1 `lastIvl` 21 `ease` 4; type 1 `ease` 0; type 1 `ease` 5; type 4; type 5 `ease` 1: every range learning [2, 1, 2, 1], young [0, 1, 1, 0], mature [0, 0, 0, 1] |
| `buttons/scope-parts` | G2, R1446 | a card in deck `Lang` with a Good and an Again row today, and a deleted card's Good row: Collection `buttons` mature [1, 0, 2, 0], `true_retention` `today` {mature_passed 2, mature_failed 1}, `hours` hour 12 {total 3, correct 2}; Deck `Lang` and Search `deck:Lang`: `buttons` mature [1, 0, 1, 0], `today` {mature_passed 1, mature_failed 1}, hour 12 {total 2, correct 1} |
| `buttons/tz-matrix` | R1446 | the rows of `retention/row-filter`, `buttons/groups` and `hourly/range-edges` together, observed with all four parts and both histories in America/St_Johns, Asia/Kathmandu, Australia/Lord_Howe on a DST change day, and with a collection created in another zone (`stats/day-buckets` scenario `other-creation-zone`): equal to the oracle's graph data |

Meguri-only tests (no oracle field): V3.2's date table, rows 4 and 6 (case a) and 7 and 8 (case b) included (deviation `stats-calendar-dst-date`); `day0_date`, `today_date` and `max_count` with a cell after today (V3.4); the item text only for d ≤ 0 (V3.4); the records of `last12Months` without all-time members (AE59); a day whose young times sum past 2³² ms (V9); a row at t = 0 in America/New_York with offset −04:00, which goes to hour 0 where Anki fails (V9).

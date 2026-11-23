# Stats review-log graphs

Spec: stats/graphs-revlog
Requirements: R1271, R1277, R1367, R1379, R1380, R1381, R1382, R1383, R1384, R1385, R1386, R1387, R1388, R1389, R1405, R1406, R1407, R1408, R1410, R1411, R1412, R1413, R1414, R1446
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/graphs/calendar/, conformance/scenarios/stats/graphs/reviews/, conformance/scenarios/stats/graphs/retention/, conformance/scenarios/stats/graphs/hourly/, conformance/scenarios/stats/graphs/buttons/
Status: Approved 2026-11-22 (R1357) for the lines that begin "Changed 2026-11-22 (round 11)". Approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/statistics-round11-20261122.md`); Antoine approved the round 11 design in chat on 2026-11-22 (`docs/design/decisions.md`, rows `stats.overview` of 2026-11-22). The lines that begin "Changed 2026-11-23" were decided by Claude (coordinator) under the same permission, after independent review (`docs/design/decisions.md`, row `stats.overview` of 2026-11-23, "Statistics specs follow the built core"). Every other section is in force and unchanged.

The data of the five review-log graphs (U99): Calendar, Reviews, Retention (the True Retention table), Hourly Breakdown and Answer Buttons. They run on the machinery of `stats/graphs-common`: scope G2, history G3, row reading G4, graph order G5, aggregate shape G6, requests G8, caches G9. This file says, per graph, which review-log rows count, the keys and values of its record, and its extras. Ranges, bins, the "0–4" first-bin shift, table values, percentages, "N/A" cells, tooltip and day text, and the colour scale are `stats/graph-presentation` (U226, `stats/graphs-common` G12, item 1). The Calendar cells' item queries are `stats/browse-queries` Q4.2; the other four graphs have no browse queries.

This file governs U99's split, decided 2026-11-10. The plan's U99 Approach and Test scenario lines that give U99 the Calendar's year-navigation bounds, the Retention Young, Mature and All variants and "N/A" cells, the Hourly percentages, the Answer Buttons percentages and totals, "Days studied 29 of 30" and Learning-group percentages are superseded: those are U226's (`stats/graph-presentation`), and the oracle has no percentages. The plan test "years back to 2019" is superseded by R1380's bound of 2000 with all history (`stats/graph-presentation` P7). The plan's owner updates those lines.

Every value here equals Anki's graph data for the same collection, scope, history and clock (R1446). The Calendar dates (V3.2) are not part of Anki's graph data; they are the dates of Anki's page, and only the count of a date that two day indices share differs (V3.3).

## V1. Rows each graph counts

The rows are those G2 and G3 select, read as G4 says: `type` as its kind (so 260 reads as Manual, −1 and 6 as Learn), `time` as 0 outside `u32`, `ivl` and `lastIvl` as integers. `lastIvl` is the interval before the answer: days when positive, seconds when negative (`model/row-records` R5). A deleted card's row counts in every graph of the Collection scope and in no other scope (G2, `stats/counting-rules` N1). A row counts once per answer: nothing is de-duplicated per card or per day (R1278, R1408). A row counts whatever its card's state is now: suspended, buried by the user or by a sibling, new again after Forget, or in a filtered deck. No graph here reads the card's queue or type.

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

- The key is 32 bits. d is then cut to its low 32 bits, read as a signed integer (two's complement), as in Anki's graph data, whose day keys are `int32`. Only a row dated about 5.9 million years after `N` reaches this: with `N` = 1791856800, an `id` of 185,544,379,044,000,000 or more, such as an id written in nanoseconds. A cut key can be negative and can be a day that other rows use; the row then counts on that day.

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
| 1791856800000000 (an id in microseconds) | 20,718,344 |
| 1791856800000000000 (an id in nanoseconds) | −735,773,886 (20,739,062,594 cut to 32 bits) |
| 9223372036854775807 | −622,211,972 |

### V2.2 Record

Changed 2026-11-22 (round 11): the record also carries the Meguri extras `count_by_answer`, `time_by_answer` and `unrated` of `stats/history-reads` H6. They are computed in the same pass, are not observed (V8) and change none of the values below (G6's list of extras).

Changed 2026-11-23: the record no longer carries them. By Answer is the part `by_answer` of its own (`stats/history-reads` H6), because with its members the record of G6's size test is over G6's 262,144-byte cap. The record holds Anki's two maps only, as below.

`count` and `time`: two sparse maps day index → `{learn, relearn, young, mature, filtered}`. A key is present in both maps exactly when at least one row counted on that day; a kind with nothing is 0. Nothing else is in the record (G6).

- Size: with all history and study on every day since 2006 (about 7,400 keys) the record holds about 74,000 numbers, most of them small. G6's 256 KB limit holds only with an encoding of U103's that averages under 3.5 bytes per number.

What U226 needs from it: the two maps, `today`, `N`, `R` and the offset of G1 for day labels, and the history (G3) to offer the "all" range only with `allHistory` (R1367, R1387). Days studied, totals, averages and answer speed (R1389) are computed there from the maps.

## V3. Calendar

### V3.1 Counts

The count of day index d is the sum of the five kinds of V2's `count` for d (R1379): every row Reviews counts, of every kind, under the same day index. The Calendar has no rule of its own for rows, scope or history.

### V3.2 The date of a day index

Decided 2026-10-27: a day index gets the date Anki's page gives it (R1384, AE58). For day index d:

1. Take now's local date and wall-clock time.
2. Move the date by d calendar days and keep the wall-clock time. When that time does not exist on the target date (a spring-forward gap), it is read with the offset in force before the change, so the instant lands later by the gap. When it occurs twice (a fall-back hour), the first occurrence is taken. This holds for d = 0 too: when now is in the second occurrence of a repeated hour, day 0's instant is the first occurrence, earlier than now by the size of the change (row 9 below). When the target date does not exist (a skipped calendar day), the wall-clock time on it is read with the offset in force before the change, which puts it on the next date (row 11).
3. Move that instant back `R` × 3,600 s, a fixed span.
4. The date is the local date of the result, read with the UTC offset in force at that instant, written `YYYY-MM-DD` (`stats/day-buckets` B4.5).

`now` and `R` are G1's; the zone is the device zone of `stats/day-buckets` B5. `N` plays no part in the date. This rule is the Calendar's own: it is neither Anki's day (B3) nor the heatmap day (B4).

Anki's page reads its own clock at each render, so a re-render with no new graph request (year navigation, a `firstDayOfWeek` change) after midnight or the rollover hour dates the same day indices again against a later now, one day off from their counts. Meguri dates cells once, from the request's now and zone (G1, `stats/graphs-common` G8.3), so a re-render never moves a cell (V9).

- Away from offset changes, the date of d is today's date + d from the rollover hour on, and yesterday's + d before it: Paris, `R` 4, at 03:30 day 0 is yesterday's date, at 04:00 today's.
- The zone, decided 2026-10-27: steps 2 and 4 need the zone's rules at other instants than now, so the zone id is part of the request context with G1's offset. The first request of a refresh reads it with the clock, and the context token (`stats/graphs-common` G8.3) carries it to the second request, so both requests date cells in one zone. This file adds to `stats/graphs-common` a G1 row `zone`, the device zone's IANA id, taken from the clock with now (`stats/day-buckets` B5); a member `zone` in G8.3's list of token members; and the zone in the context that G8.2's Data result carries. Until `stats/graphs-common` lists it in those three places, this bullet is the rule, and it binds U98's request and token as well as U99.
- A row's Calendar date can differ from its heatmap day (`stats/day-buckets` B4) on any date whose UTC offset differs from now's, not only near a DST change. B3's spans put that date's day boundary at `R`:00 under now's offset, which is `R`:00 local moved by the difference between that date's offset and now's (`(R − 1)`:00 for a date one hour behind, `R`:00 − 0:30 on Lord Howe Island), while the heatmap day changes at `R`:00 under the row's own offset. A row logged between those two times gets a neighbouring date. Paris, `R` 4, now in summer: a review at 03:30 +01:00 on 2026-01-15 has the Calendar date 01-15 and the heatmap day 01-14. The shared dates of V3.3 add more differences. Both dates are right, each by its oracle (R1271, AE58).
- This is wider than the Product Contract's wording: R1384 says the cells "may differ from the Home heatmap only at DST edges", R1271 says "within one hour of a DST change", and plan U99's Approach repeats it. The dates differ on every date whose offset differs from now's (every winter date seen from summer, and the reverse), for reviews between the two boundaries above. V3.2's rule stands (decided 2026-10-27). Raised 2026-11-10: the wording of R1271, R1384 and U99's Approach needs a Product Contract correction, which Antoine approves; until then this bullet is the rule.

Anki's page's dates:

| Row | now (Europe/Paris unless named) | `R` | d | Date |
|---|---|---|---|---|
| 1 | 2026-03-29 12:00 +02:00 | 4 | −1, 0, 1 | 2026-03-28, 03-29, 03-30 |
| 2 | 2026-06-10 03:30 +02:00 | 4 | −1, 0, 1 | 2026-06-08, 06-09, 06-10 |
| 3 | 2026-06-10 12:00 +02:00 | 4 | −74, −73, −72 | 2026-03-28, 03-29, 03-30 |
| 4 | 2026-06-10 04:15 +02:00 | 4 | −74, −73, −72 | 2026-03-28, 03-28, 03-30 |
| 5 | 2026-11-10 12:00 +01:00 | 4 | −17, −16, −15 | 2026-10-24, 10-25, 10-26 |
| 6 | 2026-11-10 03:30 +01:00 | 4 | −17, −16, −15 | 2026-10-23, 10-25, 10-25 |
| 7 | 2026-03-29 04:30 +02:00 | 4 | −2, −1, 0, 1 | 2026-03-27, 03-28, 03-28, 03-30 |
| 8 | 2026-10-25 03:30 +01:00 | 4 | −2, −1, 0, 1 | 2026-10-22, 10-23, 10-25, 10-25 |
| 9 | 2026-10-25 02:30 +01:00 (second occurrence) | 3 | −1, 0, 1 | 2026-10-23, 10-24, 10-25 (day 0 from now's instant itself would be 10-25) |
| 10 | Australia/Lord_Howe, 2026-10-04 04:15 +11:00 | 4 | −2, −1, 0, 1 | 2026-10-02, 10-03, 10-03, 10-05 |
| 11 | Pacific/Apia, 2012-01-05 12:00 +14:00 | 4 | −7, −6, −5 | 2011-12-29, 12-31, 12-31 |

### V3.3 Dates that two day indices share

Call a date "crossed" when now's wall-clock time on that date (read as V3.2 step 2 reads it), moved back `R` hours (step 3), lands before the first instant of that local date. The first instant is midnight, or the end of the gap on a date whose clock change skips midnight. With no offset change on the date before that time of day, a date is crossed exactly when now's time of day is before `R`:00. By V3.2, the date of d is the target date (now's date + d) when the target date is not crossed, and the date before when it is. So neighbouring day indices get neighbouring dates, except when only one of their two target dates is crossed, or when a target date does not exist.

V3.2's four steps are the rule in every case. The cases below describe one offset change of size Δ on a date: one hour in Paris, 30 minutes on Lord Howe Island. Any other offset change can also make two day indices share a date by those steps.

- **(a) A target date has the change** between its first instant and now's time of day, near the rollover hour. Its day index shares a date with a neighbour, and the date next to them gets no day index. Row 4: −74 and −73 on 2026-03-28, none on 03-29. Row 6: −16 and −15 on 2026-10-25, none on 10-24.
- **(b) Now's date has the change.** Day 0 shares a date with a neighbour when now's wall-clock time is in [`R`:00, `R`:00 + Δ) after a forward change (−1 and 0 share), or in [`R`:00 − Δ, `R`:00) after a backward change (0 and 1 share), and that span is wholly after the change's skipped or repeated wall-clock times. A time in a repeated span never qualifies, because step 2 reads it as its first occurrence. The date of day 0's own study day then gets no day index. Row 7: −1 and 0 on 03-28, none on 03-29. Row 8: 0 and 1 on 10-25, none on 10-24. Row 10 (Δ 30 minutes, now 04:15 with `R` 4): −1 and 0 on 10-03, none on 10-04. Row 9: with `R` 3, the span [02:00, 03:00) +01:00 is the repeated hour itself, so nothing is shared. Every other day index keeps its usual date.
- **(c) A target date does not exist** (a skipped calendar day). Its day index gets the next date, which its neighbour also has. Row 11: −6 and −5 on 2011-12-31.

On a shared date, Anki's page keeps the count of one of the two day indices, whichever its map's key order puts last, and hides the other. Decided 2026-10-27: Meguri draws the date once with the sum of the two counts, so no review is hidden. Its item text is that of the later of its day indices that is 0 or below and has a count above 0; when neither index qualifies, the cell has no query (`stats/browse-queries` Q4.2). So with rows only on −74 of the pair (−74, −73), the text is that of −74; with rows only on 1 of the pair (0, 1), there is no query. The dates stay Anki's, the empty date included. Register entry `stats-calendar-shared-date-sum` (V9).

### V3.4 Record

| Member | Value |
|---|---|
| `today_date` | the local date of now, unmoved |
| cells | one per date that a day index with a count above 0 has (V3.2): the date, its count (V3.1; on a shared date the sum, V3.3) and the item text of `stats/browse-queries` Q4.2 (on a shared date, V3.3's rule) |
| `max_count` | the largest cell count in the record, all years, shared dates and dates after today included (R1382), and the count of each day index whose date gives no cell (below) |

- Changed 2026-10-27: each cell carries its own date, so the record has no `day0_date`, and a cell's date is not d days after day 0's.
- Cells dated after `today_date` (rows after now, clock skew) stay in the record and in `max_count`, as in Anki's data; whether they are drawn is U226's (R1380).
- A day index whose V3.2 date is before 0001-01-01 or after 9999-12-31 gives no cell, because `YYYY-MM-DD` (`stats/day-buckets` B4.5) cannot write it. Its count still counts in `max_count`, as on Anki's page, which counts every day index in its maximum and never draws such a date (no year it offers holds it). Only V2.1's far-future ids reach such dates: an id in microseconds gives d 20,718,344, a date in about the year 58,750. Meguri computes these dates without overflow and never fails on them.
- Encoding, decided 2026-11-10 (U103, `stats/graphs-common` G6): a date is written as its day number, the days from 1970-01-01 in the proleptic Gregorian calendar, negative before it. The cells are a sparse map keyed by that day number in ascending order, written as G6's runs with two members: the count, and the item text (length 0 for no query). Consecutive dates make one run; a date with no cell ends it, so a shared date is one key and the empty date next to it a gap. `today_date` is a day number and `max_count` an integer. These three are the Calendar's extras of G6.
- What U226 needs besides: `firstDayOfWeek` (G7, R1381; its menu writes through G7), and the history for year navigation and the cells it fills in (R1380). The cells U226 fills with no reviews, the year bounds and the colour scale are `stats/graph-presentation`'s.
- The cells cover every day of the Reviews `count` map, so all years are on hand and year navigation never calls the core (R1290, R1355).

## V4. Retention

### V4.1 Rows

A row counts for retention when all of these hold (R1405):

- its `ease` is 1 or above (it has a rating);
- it is not a cram row: `type` Filtered with `factor` 0 (an answer in a filtered deck that does not reschedule);
- its `type` is Review, or its last interval is long (V1).

Of a counted row: young when `lastIvl` < 21, mature otherwise; fail when `ease` is 1, pass otherwise (R1405). Pass is every `ease` other than 1, 5 to 255 included, as in Anki's graph data (R1446); R1405's "pass is buttons 2–4" is read as "not button 1". Every counted row counts, however many a card has that day (R1408).

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

Changed 2026-11-22 (round 11): the Statistics page also reads the Meguri part `retention_by_day` (`stats/history-reads` H4), the same row rule (V4.1) per day; this record is unchanged and is the By Period table.

One entry per period: `{young_passed, young_failed, mature_passed, mature_failed}`, the four counts of V4.1. Nothing else (G6). The Young, Mature and All tables, the percentages with one decimal and the "N/A" cells (R1407) are U226's, from these counts.

## V5. Hourly Breakdown

### V5.1 Hour

The hour of a counted row (V1: Learn, Review and Relearn) is the hour of its local time at the zone's current offset, with t the row's whole second and offset G1's (the offset at now, seconds east). For `t + offset ≥ 0`, the hour is `floor((t + offset) / 3,600)` taken modulo 24, a value from 0 to 23. That formula does not apply below 0. Every row uses that one offset, summer and winter rows alike (R1411). A row with `t + offset < 0` (west of UTC, in the first hours after the epoch, read only with `allHistory`) is in hour 0 (V9).

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

Changed 2026-11-22 (round 11): the record also carries the Meguri extras of `stats/history-reads` H7: a `six_months` range and `retention_by_hour`. Not observed (V8); Anki's four ranges are unchanged.

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

Changed 2026-11-22 (round 11): the record also carries H7's `six_months` range. Not observed (V8).

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

U99's parts of the `stats` kind (`stats/graphs-common` G14), in G14's form (field names as Anki's protobuf file declares them, map keys as decimal strings, members at their default omitted), plus V8's own additions, which G14 does not state and which narrow its default rule for U99's parts: only scalar members at 0 are omitted; every message member and every list is written, empty or not; an `hours` entry with both counts 0 is written `{}`. A map member with no key is omitted, as G14's default rule says: `reviews` with no counted row is `{}`, not `{count: {}, time: {}}`. Both sides serialise U99's parts this way:

- `reviews` as G14 defines it; it is also the Calendar's observation (V3.1).
- `true_retention`: `{today, yesterday, week, month, year, all_time}`, each `{young_passed, young_failed, mature_passed, mature_failed}`.
- `hours`: `{one_month, three_months, one_year, all_time}`, each a list of 24 `{total, correct}`.
- `buttons`: `{one_month, three_months, one_year, all_time}`, each `{learning, young, mature}`, each a list of 4 counts.
- With history `year`, the `all_time` members of these three parts are left out on both sides: Anki computes them and hides them, Meguri has none (G3, G13).

With no counted row in the scope, the parts are: `reviews` `{}`; each `true_retention` period `{}`; each `hours` range 24 entries `{}`; each `buttons` range `{learning: [0, 0, 0, 0], young: [0, 0, 0, 0], mature: [0, 0, 0, 0]}`.

Meguri's side of these four parts is U104's, as the Statistics plan assigns it (`docs/plans/2026-11-22-1200-feat-statistics-page-implementation-plan.md` §4: U104 owns the graph observation of G14, V8 and `stats/graphs-cards` C12). Changed 2026-11-23: built on 2026-11-22 (PR #133). The testkit's `stats` executor (`core/crates/meguri-testkit/src/exec/stats.rs`, `conformance/stats-ops` H10) runs the request, and its part serialiser (`core/crates/meguri-testkit/src/observe/stats.rs`) writes these parts as above from U99's records; Meguri's extras are not written. Every V10 scenario is active; `reviews/time-wrap` passes under the deviation `stats-review-time-no-wrap` (V9). Before, this paragraph said that until U104 no V10 scenario could pass on Meguri; on 2026-11-22 it had already corrected that U99 would add them, since U99 (PR #126) built the records and no observation.

The Calendar's dates, `max_count` and item texts (V3.4) are not in Anki's graph data and are not observed. Meguri tests them from V3.2's table and V3.4; the item texts are Q4.2's, and the scenario `calendar-cells` (`stats/browse-queries` Q8) checks what they find.

## V9. Differences from Anki

V9 is this file's register of differences (R1889, `conformance/dsl` D13). An entry that no scenario can observe is a register entry, as in `sched/review-session` P11: it authorizes no transform, and its proof is the named Meguri test (`core/crates/meguri-stats/tests/graphs_revlog.rs`). An entry that a scenario can observe gets a `conformance/deviations.yaml` transform entry with that scenario. The `conformance/deviations.yaml` header links this register (to be added by the owner of that file).

- **Calendar counts on a shared date.** Register entry `stats-calendar-shared-date-sum`, test `calendar_shared_date_sum`. Where two day indices get one date (V3.3), Anki's page draws the count of one of them and hides the other. Meguri draws their sum, with V3.3's item text, and counts the sum in `max_count`. The dates are Anki's in every case. Decided 2026-10-27. No scenario observes it: the dates are not in Anki's graph data (V8).
- **Calendar dates and the page clock.** Register entry `stats-calendar-request-clock`, test `calendar_dates_from_request_clock`. Anki's page dates cells from its own clock at each render (V3.2), so a page left open across midnight or the rollover hour moves its cells by one day when it re-renders without a new request. Meguri dates them from the request's now and zone. Decided 2026-11-10. No scenario observes the page.
- **Review time sums.** Pending `conformance/deviations.yaml` entry `stats-review-time-no-wrap`, test `review_time_sums_no_wrap`, scenario `reviews/time-wrap` (V10), with a `replace` at its `reviews` `time` day 0 (to be added by the owner of that file). Anki keeps each day's time of one kind in 32 unsigned bits, so when the times of one kind on one day sum to 2³² ms (4,294,967,296, about 49.7 days) or more, the sum wraps around: two Learn rows of 2,200,000,000 ms give 105,032,704, two mature rows of 3,000,000,000 ms give 1,705,032,704. Meguri sums in 64 bits and never wraps: 4,400,000,000 and 6,000,000,000. Any rows of one kind on one day whose times reach that sum do it; two rows another client wrote are enough, since one row's `time` may be up to 4,294,967,295 (G4). Decided 2026-10-27.
- **The Today sum is not this one.** The Today section takes the opposite rule for the same kind of 32-bit sum (`stats/studied-today` T3.3, also decided 2026-10-27): it reproduces the wrap, because its sum is written into the first Today line and observed as is, and no deviation is declared for it. The Reviews sums do not wrap, under the entry above, because they feed the time chart, totals and averages of `stats/graph-presentation` over many days, where one wrapped day would lose 2³² ms (about 49.7 days) of study time. So U97's and U99's time sums are two rules: code shared between them takes the wrap as an explicit choice per caller, never a default.
- **Hours of rows just after the epoch.** Register entry `stats-hourly-epoch-rows`, test `hourly_epoch_rows`. With a negative offset (west of UTC), a row in the first hours after the epoch (`0 ≤ t < −offset`, read only with `allHistory`) has `t + offset < 0`. Anki puts it in hour 0 when `−3,600 < t + offset < 0`. When `t + offset ≤ −3,600`, Anki's graph request panics while it holds the collection: every later call to that Anki backend then fails too, closing the collection included, until Anki restarts. Meguri puts every such row in hour 0 (V5.1) and returns data. Decided 2026-10-27: equal to Anki wherever Anki returns data, and one rule for the rows where it fails, so that one imported row of 1970 never stops the Stats screen. The difference is only those failing rows. No scenario can observe it: the oracle cannot run past the panic (V10).
- Parts that need all history are left out with `last12Months` (G13).

## V10. Scenarios

For the oracle-maintainer, under `conformance/scenarios/stats/graphs/`, with the `stats` kind (V8) of `oracle/driver/ops/stats.py`; as of 2026-10-27 every scenario below is in the tree with its golden (`buttons/tz-matrix` as `buttons/tz-matrix/<zone>`, one scenario per zone, and AE58's heatmap side held with the add-on image, `conformance/stats-ops` H4); `reviews/time-wrap` and the `buttons/tz-matrix/<zone>-scopes` scenarios were added with their goldens on 2026-11-17. Review-log rows come from `db_exec` (`conformance/stats-ops` H3), on a deleted card's id unless a scope is named. All in Europe/Paris, rollover 4, now 2026-10-12 12:00 +02:00 (`N` = 1791856800), Collection scope and `year`, unless named. "Today at hh:mm" is local time on 2026-10-12; a row given only as "today" is at 12:00, and a row given as "k days ago" at 12:00 on that date. A row is type 1, `lastIvl` 30, `ease` 3, `factor` 2500, `time` 0 unless named. Values are this file's; the goldens are the authority once recorded.

No oracle scenario may store a row that a graph request reads with `t + offset ≤ −3,600` (V9): Anki's request panics, and every later step of that run fails with it.

| Scenario | Pins | Expected |
|---|---|---|
| `reviews/kinds-split` | V1, V2.1, R1385, R1386 | ten rows today at 12:00: type 0 (time 1,000); type 2 `ease` 1 (2,000); type 1 `lastIvl` 20 (3,000), 21 (4,000) and −600 (5,000); type 3 `factor` 0 (6,000); type 3 `ease` 0 (7,000); type 4 `ease` 0 (8,000); type 5 `ease` 0 (500); type 1 `ease` 0 (9,000): `reviews` count {0: learn 1, relearn 1, young 2, mature 2, filtered 2}, time {0: learn 1,000, relearn 2,000, young 8,000, mature 13,000, filtered 13,000}; `hours` hour 12 {total 6, correct 4} in every range |
| `reviews/day-edges` | V2.1 | rows with the ids 1791770399999, 1791770400000, 1791770400500, 1791770401000, 1791856800000, 1791943199999, 1791943200000: count {−1: 3, 0: 3, 1: 1}, all mature |
| `reviews/window-start` | V2.1, G3 | ids 1760234399999, 1760234400000, 1760234401000: `year` {−366: 1, −365: 1}; `all` {−366: 2, −365: 1} |
| `reviews/time-wrap` | V2.1, V9 | today: two type 0 rows with `time` 2,200,000,000, one type 1 row with `lastIvl` 10 and `time` 3,000,000,000, and two type 1 rows with `time` 3,000,000,000; yesterday one type 1 row with `time` 3,000,000,000: count {−1: mature 1, 0: learn 2, young 1, mature 2}; time {−1: mature 3,000,000,000, 0: learn 105,032,704, young 3,000,000,000, mature 1,705,032,704} (Anki, wrapped). Meguri: day 0 learn 4,400,000,000 and mature 6,000,000,000, under the pending deviation `stats-review-time-no-wrap` (V9) |
| `calendar/ae58-paris-spring-forward` (AE58) | V3.2, R1271 | now 2026-03-29 12:00 +02:00 (`N` = 1774836000); rows at 03:30 +02:00 (id 1774747800000) and 04:30 +02:00 (1774751400000) that day: `reviews` count {−1: 1, 0: 1} (Anki oracle, the only part this scenario observes). The heatmap half, `heatmap_totals` home {2026-03-28: 2}, is not in this scenario: it is held for the add-on image (`conformance/stats-ops` H4) and belongs to the `stats/day-buckets` scenario `paris-spring-forward` (B7), which has no file in the tree as of 2026-11-17. Meguri's Calendar dates are 2026-03-28 and 2026-03-29, as on Anki's page (Meguri test). Only the 04:30 row's dates differ (Calendar 2026-03-29, heatmap 2026-03-28); the 03:30 row is on 2026-03-28 in both. The oracle evidence is the day index; the date is a Meguri test of V3.2. AE58's and plan U99's "03:30 CEST review" is read as this pair, the differing review being the 04:30 one |
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
| `buttons/tz-matrix` | R1446 | the rows of `retention/row-filter`, `buttons/groups` and `hourly/range-edges` together, observed with all four parts and both histories in America/St_Johns, Asia/Kathmandu, Australia/Lord_Howe on a DST change day, and with a collection created in another zone (`stats/day-buckets` scenario `other-creation-zone`): equal to the oracle's graph data. Collection scope only: every row is on a deleted card's id |
| `buttons/tz-matrix/<zone>-scopes` | G2, R1446 | per zone of `buttons/tz-matrix`, the same rows, with every second row of today (the second, fourth, …) and every `hourly/range-edges` row on a card in deck `Lang`, the rest on a deleted card's id; all four parts, both histories, in the Collection scope, Deck `Lang` and Search `deck:Lang`: Collection equal to the zone's `buttons/tz-matrix` scenario; Deck and Search equal to each other, for example `buttons` `one_month` learning [3, 0, 3, 0], young [0, 1, 0, 0], mature [1, 0, 2, 1] against Collection's [4, 1, 6, 1], [1, 1, 1, 0], [1, 0, 2, 1]. With `buttons/scope-parts`, these hold the plan's "deck, collection and search scope across the TZ matrix" |

Meguri-only tests (no oracle field), in `core/crates/meguri-stats/tests/graphs_revlog.rs`: V3.2's date table, every row, rows 4 and 6 (case a), 7, 8 and 10 (case b) and 11 (case c) each with rows on both day indices of the shared date: one cell with the sum, V3.3's item text, and `max_count` counting the sum (`calendar_shared_date_sum`); row 9, where day 0 is dated from the first occurrence of now's wall-clock time; a shared date with rows only on −74 of (−74, −73), whose text is that of −74, and one with rows only on 1 of (0, 1), with no query (V3.3); a re-render after the rollover hour with no new request, whose dates do not move (`calendar_dates_from_request_clock`); a winter row at 03:30 +01:00 seen in summer, whose Calendar date and heatmap day differ (V3.2); `today_date` and `max_count` with a cell after today (V3.4); a row at id 1791856800000000, which has no cell and counts in `max_count` (V3.4); the item text only for d ≤ 0 (V3.4); the Calendar record's encoding by day number (V3.4); the records of `last12Months` without all-time members (AE59); a day whose young times sum past 2³² ms (`review_time_sums_no_wrap`, V9); a row at t = 0 in America/New_York with offset −04:00, which goes to hour 0 where Anki fails (`hourly_epoch_rows`, V9).

# Day buckets

Spec: stats/day-buckets
Requirements: R1269, R1270, R1271, R1272, R1273, R1274, R1280, R292, R1296, R1447
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/day-buckets/

Statistics put each review-log row on a day in two different ways.

- **Anki's day** (B2, B3) is the scheduler's day: fixed 86,400-second spans counted back from the next day boundary. The Home "studied today" line, the Stats screen and day-based searches use it, so they agree with the scheduler on what "today" is (R1269, R1272).
- **The heatmap day** (B4) is the day the Review Heatmap add-on 1.0.1 puts a review on. The Home and deck-page heatmaps, the streak stats and the widget use it, so the user's heatmap looks as it did on desktop Anki (R1270, R292).

The two can disagree for a review logged near the rollover hour when the UTC offset changed in between (B4.4, R1271). That is expected. Both mappings are pure functions of the inputs in B1. This crate never computes the day boundary itself.

## B1. Inputs

| Input | Meaning | Source |
|---|---|---|
| row instant | `revlog.id`: the review time in epoch milliseconds | the row |
| `N` | `next_day_at`: the start of the next scheduler day, epoch seconds | the day clock (`specs/sched/day-clock.md`, U40): the value the scheduler uses at this instant, never recomputed here (R1272) |
| `R` | the rollover hour, 0 to 23 | the day clock's effective value of the `rollover` config key (4 when the key is missing) |
| zone | an IANA zone id | the clock (U21): the device's current zone |
| now | the current instant, epoch milliseconds | the clock |

- Stats never read the system clock or the system time zone, so Docker, the simulator and devices agree.
- The collection's creation time and `creationOffset` change the scheduler's day number (`days_elapsed`), but not B2, B3 or B4. Scenario `other-creation-zone` pins this.

## B2. Anki's day 0: the studied-today window

- Day 0 starts at `S = (N − 86,400) × 1,000` (epoch milliseconds).
- A row is in the studied-today window when its `id` is strictly greater than `S`. A row stamped exactly `S` is not in it.
- The window has no upper end. Rows dated after now, and after `N`, are in it (clock skew on another device, scenario `future-row`). Decided 2026-10-27: no upper end, as Anki. R1269's "to `next_day_at`" bounds day 0 for day indices (B3), not the studied-today window, which R1296 defines as every row since the start of day 0. Never cap the window at `N`.
- When `N` changes (rollover, a change of `rollover`, an offset change; U40 says when), `S` changes with it and the window holds exactly the rows after the new `S` (B6).

| `id` relative to `S` | In the window |
|---|---|
| `S − 1` | no |
| `S` | no |
| `S + 1` | yes |
| `N × 1,000 + 3,600,000` (one hour after the next day starts) | yes |

Note for other units: the Stats screen's Today section counts rows with `id ≥ S`, so it includes the row stamped exactly `S` (`stats/studied-today`, U97). Day-based searches (`rated:`, `prop:rated`) have their own boundaries in the search specs.

## B3. Anki's day index of a row

The Reviews and Calendar graphs (U98, U99) group rows by a day index relative to today:

- `t = floor(id / 1,000)` (whole seconds);
- `d = (t − N) / 86,400`, divided with rounding toward zero.

So day 0 holds rows with `N − 86,400 < t < N + 86,400`. A past day `−k` (k ≥ 1) holds `N − (k + 1) × 86,400 < t ≤ N − k × 86,400`. A future day `+k` holds `N + k × 86,400 ≤ t < N + (k + 1) × 86,400`. Days are fixed spans of 86,400 seconds counted back from `N`, also across DST changes (R1269). The calendar date a day index stands for belongs to the graphs (U99).

B2 and B3 do not match exactly at the edges. A row with `S < id < S + 1,000` is in the studied-today window but has day index −1. A row with `t ≥ N + 86,400` is in the window but has index 1 or more.

| `id` (N = 1791856800, 2026-10-13 04:00 in Europe/Paris) | `d` | In the B2 window |
|---|---|---|
| 1791770399999 | −1 | no |
| 1791770400000 (`S`) | −1 | no |
| 1791770400500 | −1 | yes |
| 1791770401000 | 0 | yes |
| 1791856800000 (`N`) | 0 | yes |
| 1791943200000 (`N` + 1 day) | 1 | yes |

## B4. The heatmap day

### B4.1 Rule

The heatmap day of a row is a calendar date: take the instant `floor(id / 1,000) − R × 3,600` seconds (the review time moved back by `R` hours of elapsed time), and read its local date in the device zone **with the UTC offset the zone has at that moved instant**.

The offset is the one at the moved instant, not at the review instant. When the zone's offset changes during the `R` hours before a review, the result differs from "the local clock time of the review minus `R` hours". The table shows such cases (rollover 4). The last column is the rejected reading, for contrast.

| Zone | Review (local, offset) | Moved instant (UTC) | Offset at moved instant | Heatmap day | Wall clock − 4 h |
|---|---|---|---|---|---|
| Europe/Paris | 2026-03-29 03:30 +02:00 | 03-28 21:30 | +01:00 | 2026-03-28 | 2026-03-28 |
| Europe/Paris | 2026-03-29 04:30 +02:00 | 03-28 22:30 | +01:00 | **2026-03-28** | 2026-03-29 |
| Europe/Paris | 2026-03-29 05:30 +02:00 | 03-28 23:30 | +01:00 | 2026-03-29 | 2026-03-29 |
| Europe/Paris | 2026-10-25 02:30 +02:00 | 10-24 20:30 | +02:00 | 2026-10-24 | 2026-10-24 |
| Europe/Paris | 2026-10-25 03:30 +01:00 | 10-24 22:30 | +02:00 | **2026-10-25** | 2026-10-24 |
| Europe/Paris | 2026-10-25 04:30 +01:00 | 10-24 23:30 | +02:00 | 2026-10-25 | 2026-10-25 |
| Australia/Lord_Howe | 2026-10-04 04:15 +11:00 | 10-03 13:15 | +10:30 | **2026-10-03** | 2026-10-04 |
| Australia/Lord_Howe | 2026-10-04 04:45 +11:00 | 10-03 13:45 | +10:30 | 2026-10-04 | 2026-10-04 |
| Australia/Lord_Howe | 2026-04-05 03:45 +10:30 | 04-04 13:15 | +11:00 | **2026-04-05** | 2026-04-04 |
| Australia/Lord_Howe | 2026-04-05 04:15 +10:30 | 04-04 13:45 | +11:00 | 2026-04-05 | 2026-04-05 |
| America/St_Johns | 2026-03-08 04:30 −02:30 | 03-08 03:00 | −03:30 | **2026-03-07** | 2026-03-08 |
| America/St_Johns | 2026-03-08 05:30 −02:30 | 03-08 04:00 | −03:30 | 2026-03-08 | 2026-03-08 |

The scenarios of B7 that cross an offset change tell this reading apart from the review-instant reading. The add-on oracle's recordings of these scenarios confirm it; their goldens of 2026-11-10 hold it (`specs/conformance/stats-ops.md` H4).

Without an offset change in the window, the heatmap day is the local date of the review time minus `R` hours. With `R` = 4, a review at 03:59:59 falls on the previous date and one at 04:00:00 on the same date. With `R` = 0 the heatmap day is the plain local date.

Sub-second parts never change the result: local day boundaries fall on whole seconds, so `floor(id / 1,000)` and `id` give the same date (scenario `kathmandu-day-start`).

### B4.2 The heatmap's today

The heatmap's today `T` is the same rule applied to now: the local date of `now − R × 3,600` seconds, read with the offset at that moved instant (R1270).

### B4.3 Rows after today

A row whose heatmap day is after `T` counts on `T` (R1280). This is a deviation from the add-on, which draws such a row on its own future date: `conformance/deviations.yaml` entry `heatmap-future-rows-today`, scenario `future-row` (both added with the oracle support, `specs/conformance/stats-ops.md` H4). It covers clock skew from another device. A row that is after now but still has heatmap day `T` counts on `T` both in Meguri and in the add-on.

Keeping each row's count under its own heatmap day, and adding the days after `T` into `T` when a query runs, means nothing has to move when `T` advances.

### B4.4 Where Anki's day and the heatmap day disagree

B2 and B3 use fixed spans of 86,400 s counted back from `N`. `N` is computed with the offset in force when the day clock last computed it. B4 uses the offset at each moved instant. So a review within the rollover hour's reach of an offset change can land on different dates. In scenario `paris-spring-forward`, the 04:30 CEST review on 2026-03-29 is in Anki's day 0 (2026-03-29, studied today) but on heatmap day 2026-03-28. In `lord-howe-dst-end`, the 03:45 review is outside Anki's day 0 but on heatmap day 2026-04-05, which is today. Both results are correct. Each matches its oracle (AE58, R1271).

### B4.5 Format

A heatmap day is a date of the proleptic Gregorian calendar, written `YYYY-MM-DD` wherever it is exchanged (observations, the query API, the widget snapshot).

### B4.6 Where the add-on's calendar draws a day

The add-on computes its counts per B4.1 date. Its calendar can then draw one date's count on the date before:

- The add-on hands each date to its calendar as the instant 00:00 UTC of that date.
- The calendar moves that instant back by the zone's UTC offset at that instant, and draws the count on the local date of the result.
- When the zone's offset at the result is smaller than its offset at 00:00 UTC, the result is before local midnight. The count is then drawn on the previous date, added to that date's count, and the date itself is drawn empty. Otherwise the count is drawn on its own date.

This needs an offset change between local midnight and 00:00 UTC of the same date: a DST start east of UTC early in the local night, or a change at local midnight. Some 2026 dates, by this rule:

| Zone | Date that has the count (B4.1) | Date the add-on draws it on |
|---|---|---|
| Australia/Lord_Howe | 2026-10-04 | 2026-10-03 |
| Australia/Sydney | 2026-10-04 | 2026-10-03 |
| Pacific/Auckland | 2026-09-27 | 2026-09-26 |
| Asia/Beirut | 2026-03-29 | 2026-03-28 |

Europe/Paris, America/New_York, America/St_Johns and Asia/Kathmandu have no such date. Only the Lord Howe row has scenarios: `lord-howe-dst-start` here (oracle value 2026-10-03: 2) and `stats/streaks/lord-howe-dst-start`; their goldens of 2026-11-10 show the shift in the counts and history, and that scenario's streaks are on B4.1 dates (`specs/conformance/stats-ops.md` H4). The calendar runs only in the add-on's web view, which the oracle does not run, so the headless oracle applies this placement itself to the dates the add-on's computation keys its counts by (`specs/conformance/stats-ops.md` H1, drawn date). The rule above is the placement the oracle-maintainer saw in that web view; if a later reading of it shows no shift, this section, H1's rule and the `heatmap-drawn-day-shift` entry change together.

Meguri does not reproduce this. Every count stays on its B4.1 date, in every heatmap, streak and widget value (R1270: a review belongs to the local date of the review time minus the rollover hours). This is the deviation `heatmap-drawn-day-shift` (its `conformance/deviations.yaml` entry is added with scenario `lord-howe-dst-start`, `specs/conformance/stats-ops.md` H4). The add-on's own counts agree with Meguri. Only the cell the add-on draws them in differs.

## B5. Time zone data

- Offsets come from the IANA time zone database bundled with Meguri, for the zone id the clock reports. There is no system lookup. Every instant uses the rules in force at that instant, past instants included (R292).
- All history uses the device's current zone. When the zone changes (travel), every heatmap day is computed again in the new zone. History is not frozen in the zone where it was recorded (R1273). Scenarios `travel-paris` and `travel-new-york` hold the same instants: Meguri moving from the first zone to the second must give the second scenario's values.
- A zone id the bundled database does not know: every instant uses the clock's current UTC offset, as a zone without transitions. The core logs the unknown id once per open.
- For instants before 1971 or after 2037, the add-on's date arithmetic borrows the DST rules of an equivalent year. Meguri applies the zone's rules for the actual year. No real review falls there, and no scenario covers it.
- The bundled database and the oracle's can differ for rules changed recently. Scenarios use zones whose 2026 rules are settled.

## B6. When days change

- Anki's day 0 moves whenever `N` moves. From then on, studied-today values cover exactly the rows of the new window, with no user action (R1274, scenario `rollover-while-open`: at 03:59:59 the 22:00 review is still studied today; at 04:00:00 it no longer is).
- The heatmap's today `T` changes when `now − R` hours crosses a local midnight. On a day whose UTC offset changes, this can happen at another instant than the move of `N`, by up to the size of the change (B4.4).
- `T` next changes at `at(T + 1)`: the first instant of local date `T + 1` in the zone (local midnight, or the first instant after a gap when midnight falls in one), plus `R` × 3,600 seconds of elapsed time. This is the first instant whose B4.2 date is after `T`, and the same instant as `stats/widget-snapshot` W4's `at(D)`. Example: Europe/Paris, `R` 4, `T` 2026-03-28: `at(2026-03-29)` is 2026-03-28 23:00Z + 4 h = 2026-03-29 03:00Z (05:00 CEST), while `N` moves at 02:00Z (04:00 CEST). No day-clock event marks `at(T + 1)` when it differs from the move of `N`, so the queries return it (`stats/counting-rules` N7) and hosts query again then (`stats/studied-today` T7).
- `T` and the folding of later days into `T` (B4.3) are evaluated at each query. No value computed for an earlier `T` or `N` is served after either has changed.

## B7. Scenarios

All scenarios in `conformance/scenarios/stats/day-buckets/` write review-log rows with `db_exec`. The rows belong to a card id no card has (counted as a deleted card's reviews, `stats/counting-rules` N1), so the scenarios need no notes. The values below are Meguri's, from B2 and B4. As of 2026-11-10 every scenario below is in the tree, inactive, with a golden recorded on the add-on image on 2026-11-10 (`specs/conformance/stats-ops.md` H4); the goldens are authoritative. Each golden equals the table except where a deviation is named: `future-row` (B4.3) and `lord-howe-dst-start` (B4.6).

| Scenario | Pins | `heatmap_totals.home` | `studied_today_raw` |
|---|---|---|---|
| `rollover-shift` (AE5) | 02:30 and 05:00 with rollover 4 | 10-11: 1, 10-12: 1 | 1 row, 6000 ms |
| `ease-zero-row` (AE52) | a Set Due Date row with `limresched` true, then false, then true | 10-11: 1; then 10-11: 1, 10-12: 1; then 10-11: 1 | 0 rows, 0 ms (three times) |
| `paris-spring-forward` (AE58) | B4.1, B4.4; also `timing` (the Anki-day half of AE58): `days_elapsed` 2, `next_day_at` 2026-03-30 04:00 +02:00 (1,774,836,000), `utc_offset_secs` 7200 | 03-28: 2, 03-29: 1 | 2 rows, 5000 ms |
| `paris-fall-back` | B4.1 | 10-24: 1, 10-25: 2 | 1 row, 3000 ms |
| `lord-howe-dst-start` | 30-minute change, B4.6, deviation | 10-03: 1, 10-04: 1 (oracle: 10-03: 2) | 2 rows, 3000 ms |
| `lord-howe-dst-end` | 30-minute change, B4.4 | 04-05: 2 | 1 row, 2000 ms |
| `st-johns-dst-start` | half-hour zone with a change | 03-07: 1, 03-08: 1 | 2 rows, 3000 ms |
| `st-johns-rollover-edge` | half-hour zone, 03:59:59 / 04:00:01 | 06-09: 1, 06-10: 1 | 1 row, 2000 ms |
| `kathmandu-day-start` | B2's strict start, quarter-hour zone, milliseconds | 10-11: 2, 10-12: 2 | 1 row, 4000 ms |
| `future-row` | B4.3, deviation | 10-12: 2 (oracle: 10-12: 1, 10-13: 1), then 10-12: 1, 10-13: 1 | 2 rows, 6000 ms; then 1 row, 5000 ms |
| `travel-paris`, `travel-new-york` | B5 | 10-11: 1, 10-12: 3 / 10-11: 2, 10-12: 2 | 3 rows, 9000 ms / 2 rows, 7000 ms |
| `other-creation-zone` | B1 (creation zone has no effect) | 10-11: 1, 10-12: 1 | 1 row, 2000 ms |
| `rollover-while-open` | B6 | 10-12: 1 (three times), then 10-12: 1, 10-13: 1 | 1 row, 5000 ms; then 1 row, 5000 ms; then 0 rows, 0 ms; then 1 row, 7000 ms |
| `rollover-zero` | `R` = 0 | 10-11: 1, 10-12: 1 | 1 row, 2000 ms |
| `rollover-23` | `R` = 23, the latest hour (B2, B4.1, B4.2) | 10-11: 1, 10-12: 1 | 1 row, 2000 ms |
| `rollover-change` | B4.1, B6: a change of `rollover` from 4 to 2 applies at once | 10-11: 1, then 10-12: 1 | not observed (the oracle keeps the scheduler day it computed before a config write, `sched/day-clock` D14.2) |

(Dates are in 2026. `n rows, m ms` is `{cards: n, time_ms: m}`. `then` separates the successive observations of one scenario, `(three times)` marks one value observed at three steps, and `/` separates the two scenarios of the `travel-*` row. Each column has one value per observation step.)

B4.2 and B6's move of T at another instant than Anki's day is pinned by `stats/heatmap/today-dst-shift` (`stats/heatmap-addon` A16), which observes `heatmap_report` and `studied_today_raw`.

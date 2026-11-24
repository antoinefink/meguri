# Activity heatmap

Spec: stats/activity-heatmap
Requirements: R1272, R1275, R1276, R1278, R1280, R1281, R1284, R1291, R1295
Anki: 26.09.3 (the review-log rows of `stats/counting-rules` N1 and the scheduler day of `stats/day-buckets` B3 and `sched/day-clock`, which it reuses)
Scenarios: none. It is a Meguri query and presentation with no Anki or add-on behaviour to compare; core unit tests check it against the tables below.
Status: Approved 2026-11-22 (R1357) for the lines that begin "Changed 2026-11-22 (round 11)". Approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/statistics-round11-20261122.md`); Antoine approved the round 11 design in chat on 2026-11-22 (`docs/design/decisions.md`, rows `stats.overview` of 2026-11-22). On 2026-11-24 Antoine requested that the macOS Home heatmap use the full row width. AH1 records that sizing decision, and AH9 applies it to month labels.

## AH1. What this heatmap is

Changed 2026-11-22 (round 11): the Statistics page's Activity (`stats/statistics-page` S7.5) is this grid extended to every year, with streaks beside it (under it since 2026-11-23), from the part `activity_days` (`stats/history-reads` H11): AH2's rows and AH3's days, the page's deck rule (G2.1) instead of AH4's home deck, and AH7's levels per year block (changed 2026-11-23, the owner's Option E: one continuous grid for all of history, wrapped onto lines, with AH7's levels over every day it shows). This heatmap on Today, the deck list and deck pages is unchanged and still has no streaks (round 10).

Changed 2026-11-24 (owner decision): Mac Home (Today and the shared deck surfaces) selects whole weeks at the existing 10 pt baseline square and 2 pt gap, at most 53, then sizes square cells to fill the row with that fixed gap, including at very wide widths. The side is (W − (weeks − 1) × gap) ÷ weeks. A positive width below one baseline cell shows one current-week column with smaller day squares; unmeasured or invalid widths show no weeks and reserve the baseline height. Labels use the resulting pitch and width; seven rows, placeholders and hover regions use the resulting side. Width changes update layout without a query. Study stays primary. Touch, Statistics and widgets keep their existing sizing. This supersedes earlier fixed-size wording for Mac Home only (contract N14.11, N24.6).

The minimal heatmap on Home's Today page, the touch deck list and deck pages (`docs/design/native-interface-contract.md` N14.11). It is a Meguri design:

- One query per scope when the view asks. No resident aggregates: `stats/counting-rules` N7 does not apply, and neither do R1286 to R1288.
- No `heatmap` config key: `stats/heatmap-config` is not read, and the result is the one its defaults would give.
- No forecast, streaks or paging through history. `stats/heatmap-addon` and `stats/heatmap-presentation` describe the planned full heatmap, not this one.
- It uses Anki's scheduler day (`stats/day-buckets` B3), not the heatmap day (B4), so that its today is the day the due counts use (R1272). R1270's add-on day is for the full heatmap. The two days can disagree only for a review within the rollover hours of a UTC offset change (B4.4). That is accepted for this view.

## AH2. Counted rows

One review-log row is one review. Every counted row adds 1, also when a card has several rows (R1278).

- Counted: rows with `ease` ≥ 1, whatever their `type`: learning, review, relearning and filtered or cram answers, and a rated Manual row that another client wrote (R1275). This is `stats/counting-rules` N2 with `limresched` at its default (true).
- Not counted: rows with `ease` 0, whatever their `type`. These are the Set Due Date and Forget or Reset rows (type 4), the Rescheduled rows (type 5) and any other unrated row (R1276).

| `type` | `ease` | Counted |
|---|---|---|
| 0 | 3 | yes |
| 1 | 1 | yes |
| 2 | 1 | yes |
| 3 | 3 | yes |
| 4 | 2 | yes |
| 3 | 0 | no |
| 4 | 0 | no |
| 5 | 0 | no |
| 1 | 0 | no |

## AH3. Days

`N` is the `next_day_at` the day clock returns when the query runs (`sched/day-clock` D5, D8). It is the value the due counts use, and this query never computes it again. A row's day is its day index `d` of `stats/day-buckets` B3 relative to `N`. Day 0 is today, and the row is `k = −d` days ago. A row with `d` ≥ 1 (stamped after the next boundary, for example by clock skew on another device) counts on day 0 (R1280).

With `t = floor(id / 1,000)`, a row in the window (AH5) is on day

`k = max(0, trunc((N − t) / 86,400))`, where trunc rounds toward zero.

| `id` (N = 1791856800, 2026-10-13 04:00 Europe/Paris) | Instant | `k` |
|---|---|---|
| 1791856800000 | `N` | 0 |
| 1791856799000 | `N` − 1 s | 0 |
| 1791770400000 | `N` − 86,400 s | 1 |
| 1791770399000 | `N` − 86,401 s | 1 |
| 1791860400000 | `N` + 3,600 s | 0 |
| 1791946800000 | `N` + 90,000 s | 0 |

**`T0`, the date of day 0**, used to place weekdays and to label days: take the instant `N − 86,400 − h × 3,600 + 43,200`, with `h` the effective rollover hour (D2), and read its local date under the device's current offset (`sched/day-clock` D1). This is the date on which day 0 began at hour `h`, read twelve hours after that start, so a change of offset after `N` was computed (D3.3) never shifts it by a day. Day `k` falls on `T0` minus `k` calendar days. Weekday numbers run from 0 (Sunday) to 6 (Saturday). Example with the table's `N` and `h` = 4: the instant is 2026-10-12 12:00 +02:00, so `T0` = 2026-10-12, weekday 1, a Monday.

## AH4. Scopes

- **Collection scope**: every counted row, deleted cards' rows included (`stats/counting-rules` N3.1 defaults, R1281).
- **Deck scope** for deck `D`: the counted rows whose card exists now and whose home deck is `D` or a descendant of `D` (`stats/counting-rules` N4, R1284). The home deck is `odid` when it is not 0, else `did`. Descendants follow `decks/decks` D3: a deck whose stored name starts with `D`'s stored name followed by the separator, compared as deck names are compared. Cards and names are read when the query runs, so moving a card moves its rows between deck scopes at once, and a rename or move of a deck takes effect at the next query.
- A deck id that names no deck gives an empty result (all counts 0), not an error.

## AH5. Window and query

- The query covers 371 days, `k` = 0 to 370: 53 weeks, which is 52 full weeks plus the current partial week, the most the grid can show.
- It reads only rows with `id` ≥ (N − 371 × 86,400 + 1) × 1,000, so that `k` ≤ 370, and every later row. With the AH3 example's `N` the bound is 1759802401000.
- One statement per scope, which groups and counts the rows by `k` inside SQLite and returns (`k`, count) pairs. It is bounded by the review log's primary key (`id` is the rowid). It never scans the whole review log and never runs one query per day.
- The deck scope finds its cards through the existing indexes: `ix_cards_sched` (its `did` prefix, for cards with `odid` 0) and `idx_cards_odid` (partial, `odid` != 0). It finds their rows through `ix_revlog_cid` or the `id` range. It adds no index (`storage/schema-18` S3; the S6 guard).
- It runs on a read-only reader, the query role of `storage/open-and-connections` C6.
- It never writes the collection, as `stats/counting-rules` N8 says (R1295): no row, `usn`, `mod` or config key changes. The day query that gives `N` may write as `sched/day-clock` D5 and D8 say; those writes belong to the day clock, not to this query.

## AH6. Result

- The scheduler day number (today) and the `N` it was computed with.
- `T0`, as a date and its weekday (AH3).
- The collection generation (`engine/lifecycle`) the query read.
- The 371 counts, indexed by `k`, zeros included.

The host derives everything it shows from these values. Its only day arithmetic is `T0 − k` for labels.

## AH7. Shading levels

Changed 2026-11-21 (round 10; `docs/design/native-interface-contract.md` N24.6; Antoine's answer 4B). In force: approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/round10-today-20261121.md`). It replaces the rule that divided by the busiest day.

There are five levels, from the days the view shows (not all 371). Let `D` be the counts above 0 among those days, in ascending order, `n` their number and `D[0]` to `D[n − 1]` its members. The three thresholds are `t1 = D[(n − 1) div 4]`, `t2 = D[(2 × (n − 1)) div 4]` and `t3 = D[(3 × (n − 1)) div 4]`, in integer arithmetic (multiply, then divide); `M = D[n − 1]` is the busiest day. A day with count `c` has:

- level 0 when `c` is 0;
- level 4 when `c` is `M`;
- otherwise level 1 when `c ≤ t1`, 2 when `c ≤ t2`, 3 when `c ≤ t3`, and 4 when `c > t3`.

So each level holds about a quarter of the days studied, ties stay together, a day of the same count always has the same level, and the busiest day shown is always level 4. One catch-up day of 1,400 reviews no longer pushes ordinary days into level 1: it only takes level 4 with the heaviest quarter.

| Days studied (`D`) | `t1`, `t2`, `t3` | Day `c` → level |
|---|---|---|
| 10, 20, 30, 40, 50, 60, 70, 80 | 20, 40, 60 | 10 → 1, 20 → 1, 30 → 2, 40 → 2, 50 → 3, 60 → 3, 70 → 4, 80 → 4 |
| 150, 150, 160, 170, 180, 1,400 | 150, 160, 170 | 150 → 1, 160 → 2, 170 → 3, 180 → 4, 1,400 → 4 |
| 1, 1, 1, 100 | 1, 1, 1 | 1 → 1, 100 → 4 |
| 7 (one day) | 7, 7, 7 | 7 → 4 |
| 5, 5, 5 | 5, 5, 5 | 5 → 4 (`M`) |
| 3, 9 | 3, 3, 3 | 3 → 1, 9 → 4 |

The levels are computed where the view knows which days it shows (Swift, HeatmapKit's `ActivityGrid`), after a width change too, so a rotation or fold can change a day's level; the query (AH5) is unchanged. `ActivityGridTests` checks this table, and the core's unit tests no longer check levels.

## AH8. Summary numbers

- The total is the sum of the counts of the days shown.
- The week count is the number of week columns shown.
- When that total is 0, the view hides the heatmap completely (contract N14.11).

## AH9. Month labels (round 10)

Changed 2026-11-21 (round 10; `docs/design/native-interface-contract.md` N24.6). In force as AH7 says.

- A week column's days are dated `T0 − k` (AH3, AH6). A column's month is the month of its top drawn day (the oldest drawn day of that week).
- A column is labelled when its drawn days include the first day of a month (a 1st after today, in the current week, is not drawn and labels nothing): that month's short standalone name in the app's locale, leading-aligned with the column. No year.
- The first column shown is also labelled with its month, even with no 1st in it, but only when at least three columns of that month are shown.
- Labels are placed from the newest column back. A label that would run past the grid's trailing edge ends at that edge instead; a label that would overlap one already placed is left out. Under right-to-left layout the labels mirror with the columns.
- The labels update with the days shown and the grid geometry (a new day, a width change, including Mac Home resizes that keep the same week count), never with a new query.
- `ActivityGridTests` checks: a 1st in each of a few columns; a partial current week whose 1st is after today; the first-column rule with two and with three columns of its month; and an overlap.

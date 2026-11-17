# Stats graph presentation

Spec: stats/graph-presentation
Requirements: R1290, R1293, R1362, R1363, R1364, R1365, R1366, R1367, R1368, R1376, R1377, R1378, R1380, R1382, R1383, R1386, R1387, R1388, R1389, R1392, R1395, R1396, R1398, R1399, R1400, R1401, R1403, R1404, R1406, R1407, R1409, R1412, R1414, R1415, R1416, R1417, R1418, R1420, R1439, R1440, R1443, R1444
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/presentation/

How the Stats screen turns each graph's record into bars, labels, tooltips, tables and Browser queries (U226), and how charts behave and read with VoiceOver. It is the only place that bins, ranges, quantiles and presentation text are defined (`stats/graphs-common` G12, decision 1). The records come from `stats/graphs-common` G6, `stats/graphs-cards` (Future Due, Card Counts, Intervals, Stability, Difficulty, Retrievability, Added) and `stats/graphs-revlog` (Reviews, Calendar, Retention, Hourly Breakdown, Answer Buttons). The Today section is `stats/studied-today`. The scope text and the join of a query are `stats/browse-queries` Q2 and Q3; this file gives only the item text of histogram bars.

The data rules here are platform-neutral pure functions: a record plus the control state gives bins, text and queries, with no core call and no clock (R1290; the context of `stats/graphs-common` G1 travels with the record). Drawing follows `docs/design/native-interface-contract.md`: Swift Charts, platform text styles and system colours.

Every rule reproduces Anki's graphs page at the pinned version unless P21 names a difference. Bins, values, tooltip text, table text and queries must equal the page's for the same record and controls (R1362).

## P1. Inputs

| Graph (`stats/graphs-common` G5) | Key in this file | Record | Owner |
|---|---|---|---|
| Today | `today` | the Today payload | `stats/studied-today` T6.2 (not presented here) |
| Future Due | `future_due` | `future_due`, have backlog, daily load, due tomorrow | `stats/graphs-cards` C3.6 |
| Calendar | `calendar` | cells (each its date, its count and, when one of its day indices is 0 or below, its item text), `today_date`, `max_count` | `stats/graphs-revlog` V3.4 |
| Reviews | `reviews` | `count` and `time`, day index → the five kinds | `stats/graphs-revlog` V2.2 |
| Card Counts | `card_counts` | both count sets, notes, label texts | `stats/graphs-cards` C4.4 |
| Intervals | `intervals` | interval → cards | `stats/graphs-cards` C5 |
| Stability | `stability` | rounded stability → cards, left out | `stats/graphs-cards` C7, C6.2 |
| Difficulty | `difficulty` | 1% key → cards, the median, left out | `stats/graphs-cards` C8 |
| Retrievability | `retrievability` | 1% key → cards, average, sums, left out | `stats/graphs-cards` C9 |
| Retention | `retention` | per period: young and mature pass and fail counts | `stats/graphs-revlog` V4.3 |
| Hourly Breakdown | `hours` | per range, 24 hours of `{total, correct}` | `stats/graphs-revlog` V5.3 |
| Answer Buttons | `buttons` | per range, three groups of four button counts | `stats/graphs-revlog` V6.2 |
| Added | `added` | creation day → cards | `stats/graphs-cards` C10 |

- A map is read as a set of (key, count) entries in ascending key order. An entry's key is its value on the axis; its count is how much it adds to a bin.
- `stats/graphs-common` G7 gives `firstDayOfWeek`, `cardCountsSeparateInactive` and `futureDueShowBacklog`, and the history value of the request (G3).

## P2. Writing numbers and times

All text is English (en-US). Anki's page wraps numbers in U+2068/U+2069 and puts U+200E before one percentage; Meguri writes none of these marks (`stats/studied-today` T6.3).

### P2.1 Writers

| Writer | Rule | Examples |
|---|---|---|
| F | `stats/studied-today` T5.2: the shortest decimal of the binary64 value, rounded to two fraction digits halves away from zero, trailing zeros and point removed, integer part grouped with `,`. +∞ is written `∞`. | 1.005 → `1.01`; 2.5 → `2.5`; 333,333.333… → `333,333.33` |
| L(p) | r = round(n × 10^p) ÷ 10^p in binary64 (multiply, round to an integer with halves toward +∞, divide), then written as F writes r. p is 2 unless named. | L(2) of 1.005 → `1`; L(2) of 0.125 → `0.13`; L(1) of 0.125 → `0.1`; L(0) of 1,234.5 → `1,235` |
| W | the nearest integer to the exact binary64 value, halves toward +∞, written by F | 2.5 → `3`; 0.5 → `1`; 1,234.4 → `1,234` |
| PC | the binary64 quotient × 100 as a percentage with exactly one fraction digit: its shortest decimal, moved two places, rounded halves away from zero, then `%`. | 6/7 → `85.7%`; 1/1 → `100.0%`; 0/5 → `0.0%`; 1/800 → `0.1%` |

- F and L differ on purpose: F is the page's text arguments, L its directly formatted numbers. 1.005 is `1.01` by F and `1` by L(2).
- Singular and plural follow `stats/studied-today` T5.4: a text takes its singular form exactly when its number is written `1`.
- Integers inside queries (P4.7) are never grouped.

### P2.2 Time spans

A span of s seconds (binary64) is written in one unit:

1. The natural unit by |s|: seconds below 60, minutes below 3,600, hours below 86,400, days below 2,628,000 (a month, 365 × 86,400 ÷ 12), months below 31,536,000 (a year), else years. A cap, when named, replaces any larger unit.
2. amount = s ÷ the unit's seconds (minute 60, hour 3,600, day 86,400, month 2,628,000, year 31,536,000).
3. A rounded span rounds amount to an integer (halves toward +∞) when the unit is below months; a precise span keeps it.
4. The long form is `<amount> second(s)`, `minute(s)`, `hour(s)`, `day(s)`, `month(s)`, `year(s)`; the short form is `<amount>s`, `m`, `h`, `d`, `mo`, `y`. amount is written by F.

| s | Form | Text |
|---|---|---|
| 0 | long, precise | `0 seconds` |
| 90 | long, precise, cap hours | `1.5 minutes` |
| 432,000 (5 days) | long, precise | `5 days` |
| 3,888,000 (45 days) | long, precise | `1.48 months` |
| 34,560,000 (400 days) | long, precise | `1.1 years` |
| 360,000 (100 hours) | short, precise, cap hours | `100h` |
| 2,628,000 | long, precise | `1 month` |

Rows that pin the unit boundaries of step 1:

| s | Form | Text |
|---|---|---|
| 59 | long, precise | `59 seconds` |
| 60 | long, precise | `1 minute` |
| 3,599 | long, precise | `59.98 minutes` |
| 3,600 | long, precise | `1 hour` |
| 86,400 | long, precise | `1 day` |
| 2,678,400 (31 days) | long, precise | `1.02 months` |

## P3. Day labels

D(a, b) names the days of a bin from a to b. Let big = max(|a|, |b|) and small = min(|a|, |b|).

- When big − small ≤ 1 (one day): for a ≥ 0, `Today` when a = 0, `Tomorrow` when a = 1, else `In <a> days`; for a < 0, `Yesterday` when a = −1, else `<−a> days ago`.
- Otherwise (a range): for a ≥ 0, `In <a>-<b − 1> days`; for a < 0, `<|b − 1|>-<−a> days ago`.
- Numbers are written by F (`1,000 days ago`).

| a, b | D(a, b) |
|---|---|
| 0, 1 | `Today` |
| 1, 2 | `Tomorrow` |
| 5, 6 | `In 5 days` |
| 0, 5 | `In 0-4 days` |
| 30, 32 | `In 30-31 days` |
| −1, 0 | `Yesterday` |
| −10, −9 | `10 days ago` |
| −10, 0 | `1-10 days ago` |
| −4, 1 | `0-4 days ago` |
| −1, 1 | `Yesterday` |

Each graph says which a and b it passes (P6, P8, P15).

Decided 2026-10-27: the plan's U226 test "−10…−6 → `10-6 days ago`" is superseded by this table. The bin holding days −10 to −6 is [−10, −5), and D(−10, −5) is `6-10 days ago`, the smaller number first, as on Anki's page.

## P4. Binning

These rules are the arithmetic of the page's chart library. Every number is binary64; "round" is to the nearest integer with halves toward +∞.

### P4.1 Step and ticks

The step for a span from a to b (a < b) and a desired count c > 0:

1. raw = (b − a) ÷ c; p = ⌊log10(raw)⌋; e = raw ÷ 10^p. log10 must give exact integers for 1, 10, 100, 1000 and so on.
2. f = 10 when e ≥ √50, 5 when e ≥ √10, 2 when e ≥ √2, else 1.
3. For p ≥ 0 the step is f × 10^p. For p < 0 the step is 1 ÷ k with k = 10^(−p) ÷ f, and every multiple is computed as i ÷ k, never i × step.

The ticks T(a, b, c) are the multiples of the step from a to b inclusive:

- c ≤ 0 gives no tick; a = b gives the one tick a.
- For p ≥ 0: i1 = round(a ÷ step), plus 1 when i1 × step < a; i2 = round(b ÷ step), minus 1 when i2 × step > b. The ticks are i × step for i from i1 to i2.
- For p < 0: i1 = round(a × k), plus 1 when i1 ÷ k < a; i2 = round(b × k), minus 1 when i2 ÷ k > b. The ticks are i ÷ k.
- When i2 < i1 and 0.5 ≤ c < 2, the ticks are those of count 2c. Otherwise i2 < i1 gives no tick.

| a, b, c | Step | Ticks |
|---|---|---|
| 0, 31, 31 | 1 | 0 to 31 |
| −364, 1, 70 | 5 | −360 to 0 |
| 0, 200, 70 | 2 | 0 to 200 (101 ticks) |
| −500, 31, 70 | 10 | −500 to 30 |
| 1, 131, 70 | 2 | 2 to 130 |
| 40, 58, 20 | 1 (k = 1) | 40 to 58 |
| −1, 1, 1 | 2 | 0 |

### P4.2 Nice domain

N(a, b) widens [a, b] to round ends, with count 10: up to ten times, take the step s of P4.1 for (a, b, 10); stop when s equals the previous round's step; otherwise move a down and b up to multiples of s (for p < 0: a = ⌊a × k⌋ ÷ k, b = ⌈b × k⌉ ÷ k). When a = b nothing changes.

| [a, b] | N |
|---|---|
| [0, 121] | [0, 130] |
| [0, 401] | [0, 450] |
| [0, 6] | [0, 6] |
| [0, 26.5] | [0, 28] |
| [0, 1.5] | [0, 1.6] |
| [40, 57] | [40, 58] |
| [3, 99] | [0, 100] |
| [13, 87] | [10, 90] |

### P4.3 Bins

Bins over the domain [x0, x1] with ascending thresholds t:

1. Drop every threshold ≤ x0 and every threshold > x1. A threshold equal to x1 stays.
2. With the kept thresholds t1 … tm, the bins are [x0, t1), [t1, t2), …, [tm−1, tm), and the last bin [tm, x1], closed at both ends. With no threshold left there is one bin [x0, x1].
3. An entry with key v is in the domain when x0 ≤ v ≤ x1; it goes in the last bin whose lower edge is ≤ v. Entries outside the domain are in no bin.
4. A bin's value is the sum of its entries' counts. A bin holds data when it has at least one entry.

So when a threshold equals x1, the last bin is [x1, x1] and holds exactly the entries with key x1.

### P4.4 Drawn bins

A bin is drawn when its upper edge is above its lower edge (x1 > x0, read from the edges alone, never from a width on screen). Only the last bin can be undrawn ([x1, x1]). An undrawn bin has no bar, tooltip, accessibility element or query, but its value counts in the graph's totals, running totals and table (P6 to P15).

Anki's page gives that bin zero width too. It also draws each bar one pixel narrower than its span on a fixed 460-pixel plot, so a bin one pixel wide or narrower has no bar and no hover there either, for example the one-day first bin [−1,000, −999) of Reviews over all (P8). Meguri draws, and makes hoverable, every bin whose edges differ (P21). Decided 2026-10-27.

### P4.5 Quantiles

Both quantile rules read the map's cumulative counts, never one value per card (R1293). Let n be the sum of all counts, and value(j), for 0 ≤ j < n, the smallest key whose cumulative count is above j (the j-th value in ascending order, from 0).

- **Q(p), interpolated** (Intervals, Stability): none when n = 0; the smallest key when p ≤ 0 or n < 2; the largest key when p ≥ 1. Otherwise h = (n − 1) × p, i = ⌊h⌋, and Q = value(i) + (value(i + 1) − value(i)) × (h − i). Q can be fractional. Example: keys 1, 2, 3, 100 one card each: Q(0.5) = 2.5, Q(0.95) = 85.45 (binary64 85.44999999999996).
- **E(p), first key reached** (Difficulty, Retrievability): c = n × p; walk the keys in ascending order, subtracting each count from c; E is the first key after which c ≤ 0. When no key gets there, E is 0.

### P4.6 Bar count

The desired count of P4.1 is at most 70 for every histogram (the 70-bin rule), and lower when the span is shorter (each graph says). The step rule then gives the actual count, which can exceed 70: Reviews over 1 year has 73 drawn bins, Future Due over all from day 0 to a last due day of 200 has 100. Decided 2026-10-27: R1362's purpose is that each bar covers the same days or values as Anki's and its query finds the same cards, so Anki's edges win over the reading "never more than 70 bars". The plan's U226 test "bar count ≤ 70" means the desired count.

### P4.7 Query numbers

A number inside a bar query is written as the shortest decimal that reads back as the same binary64 value, with `-` for negative values, no grouping and no exponent: `0.07`, `-500`, `124.5`. Every bar query is Anki search syntax only (P10 decides the one case where Anki's page writes a query its search rejects) and parses with the core's parser (R1420, `stats/browse-queries` Q6). The Browser receives it through `stats/browse-queries` Q3 (R1417).

Decided 2026-10-27: bar queries are written by these rules as plain text and joined only by `StatsBrowseLink` (`stats/browse-queries` intro, Q3). They never go through U88's builder, its `join` or the canonical writer; the plan's U226 line "Bar queries built through U88's builder" is superseded. Its expected texts still hold (`"prop:ivl>=3" "prop:ivl<=5"`, `<=` on Difficulty's last bar, `added:` bounds). The first is the writer's text for an Intervals bin [3, 6), which P10 never makes: Intervals thresholds are 1 + k × s for a whole step s of 1, 2, 5, 10 and so on, so a bin from 3 has its next edge at 4 or 5. So it is a Meguri unit test of the query writer (P23), not a value a golden can hold.

## P5. Ranges and controls

### P5.1 Ranges

| Graph | Options, in order | Default | Option needing all history |
|---|---|---|---|
| Future Due | 1 month, 3 months, 1 year, all | 1 month | none (all is always offered, R1376) |
| Reviews | 1 month, 3 months, 1 year, all | 1 month | all |
| Intervals | 1 month, 50%, 95%, all | 95% | none |
| Stability | 1 month, 50%, 95%, all | 95% | none |
| Difficulty | 50%, 95%, 100%, all | all | none |
| Retrievability | 50%, 95%, 100%, all | all | none |
| Retention (picker) | Young, Mature, All | All | the All time row (P12) |
| Hourly Breakdown | 1 month, 3 months, 1 year, all | 1 year | all |
| Answer Buttons | 1 month, 3 months, 1 year, all | 1 year | all |
| Added | 1 month, 3 months, 1 year, all | 1 month | none (R1415) |

- The labels are P19's: `1 Month`, `3 Months`, `1 Year`, `All`, `50%`, `95%`, `100%`.
- An option that needs all history is not offered while the history is `last12Months` (R1367).
- Each range control is a segmented picker (R1366). The other controls: Backlog (Future Due, a toggle bound to `futureDueShowBacklog`, shown only when have backlog is true, R1375), Time (Reviews, a toggle, off at each open), Separate suspended/buried cards (Card Counts, bound to `cardCountsSeparateInactive`), the year (Calendar, P7), and the week start (Calendar, a pop-up menu labelled `Week starts on` holding exactly `Sunday`, `Monday`, `Friday` and `Saturday`, bound to `firstDayOfWeek`, `editor/config-keys` K5, R1381). The two bound toggles and the week-start menu write config only through `stats/graphs-common` G7.

### P5.2 Remembered per device

Each graph's range selection is kept per device for that graph and restored the next time the screen opens, in any collection (R1366). It is device-local: never in `collection.anki2`, never synced. A stored value that is not a valid option is read as the default. The Time toggle and the Retention picker are not kept: Time starts off and the picker starts at All. Decided 2026-10-27.

### P5.3 Following the history control

Reviews, Hourly Breakdown and Answer Buttons follow the history control (R1367):

- When the history changes to `allHistory`, each of the three selects all.
- When it changes to `last12Months`, each that has all selected selects 1 year. The other selections stay.
- Each such change is stored as P5.2 says.
- Opening the screen applies neither rule: the stored selection, or the P5.1 default when none is stored, is shown, except that a stored all shown with `last12Months` reads as 1 year (and stays stored as all). Decided 2026-10-27. Anki keeps no selection: each open starts at the default and then runs the first rule, so with `allHistory` its three graphs open at all. Meguri opens at the stored or default range (P21).

1 year, not the 1-month default, is the fallback Anki uses. The plan's U226 test "falls back to the spec'd default" means this 1 year.

## P6. Future Due

**Data.** The `future_due` map of `stats/graphs-cards` C3.6. With Backlog off, entries with a key below 0 are left out (AE57).

**Backlog** in this section is the stored `futureDueShowBacklog` value (`stats/graphs-common` G7, default true), whether or not the toggle is shown (P5.1). So with have backlog false and the value true, x0 is the smallest key, which can be above 0.

**Domain.** x0 is 0 with Backlog off, else the smallest key. x1 is 31, 90 or 365 for 1 month, 3 months and 1 year, and the largest key for all (R1376). Desired count min(70, x1 − x0). Thresholds T(x0, x1, that count) (P4.1), bins by P4.3.

**Empty.** No data (P16.5) when the map, after Backlog's filter, is empty or no bin has a value.

**Bin text.** For a drawn bin [a, b): let e = b + 1 when b = x1 and b otherwise.

- Tooltip lines: `D(a, e):`, then `<value> card(s) due`, then `Running total: <L(running)>`, where running is the sum of this bin's and every earlier bin's values.
- Query (when the bin holds data): with z = b when b = x1 and b − 1 otherwise, `"prop:due=<a>"` when a = z, else `"prop:due>=<a>" AND "prop:due<=<z>"`.

**Table.** total = the sum of all bins' values; days = x1 − x0.

| Label | Value |
|---|---|
| `Total` | `<total> review(s)` |
| `Average` | `<round(total ÷ days)> review(s)/day` (days 0 gives `∞ reviews/day`) |
| `Due tomorrow` | `<due tomorrow> review(s)` |
| `Daily load` | `<daily load> review(s)/day` |

Example, `future_due` {0: 3, 1: 4, 5: 1, 30: 2, 31: 5, 40: 1}, Backlog off, 1 month: domain [0, 31], 32 bins.

| Bin | Value | First tooltip line | Running | Query |
|---|---|---|---|---|
| [0, 1) | 3 | `Today:` | 3 | `"prop:due=0"` |
| [1, 2) | 4 | `Tomorrow:` | 7 | `"prop:due=1"` |
| [5, 6) | 1 | `In 5 days:` | 8 | `"prop:due=5"` |
| [30, 31) | 2 | `In 30-31 days:` | 10 | `"prop:due>=30" AND "prop:due<=31"` |
| [31, 31] | 5 | not drawn | 15 | none |

Total `15 reviews`, Average `0 reviews/day`, Due tomorrow `4 reviews`. The [30, 31) bar counts day 30 only while its text and query cover days 30 and 31, as in Anki; day 31's cards are in no visible bar.

No backlog, toggle hidden: `future_due` {1: 45}, have backlog false, `futureDueShowBacklog` true, 1 month: domain [1, 31], desired count 30, 31 bins: [1, 2) `Tomorrow:` with `"prop:due=1"`, …, [30, 31) `In 30-31 days:`, [31, 31] not drawn. Total `45 reviews`, Average round(45 ÷ 30) = `2 reviews/day`. With `futureDueShowBacklog` false: domain [0, 31], 32 bins, the first [0, 1) `Today:` empty, Average round(45 ÷ 31) = `1 review/day`.

More bins: 1 year without backlog gives [0, 5) `In 0-4 days` … [360, 365) `In 360-365 days` and [365, 365] undrawn (74 bins). With Backlog on and a smallest key of −500, 1 month gives a step of 10: [−500, −490) `491-500 days ago` with query `"prop:due>=-500" AND "prop:due<=-491"`, …, [20, 30) `In 20-29 days`, [30, 31] `In 30-31 days` (drawn, closed).

## P7. Calendar

The grid is U227's component, defined in `stats/heatmap-presentation` (its Calendar layout M6, colours M8.3, accessibility M15.6). A cell's date is the cell's own date (`stats/graphs-revlog` V3.2, V3.3, R1384): a date that two day indices share is one cell with the summed count, and no date is computed from a day index. This section gives what the Calendar adds; "today" is the record's `today_date`.

Decided 2026-10-27: the Calendar box's drawing (grid, palette, colour stops, empty colour) and its `AXChartDescriptor` (P18.3) come from `stats/heatmap-presentation`, written 2026-11-10; U226 builds them from it. The rules below (years, drawn dates, tooltip, query) do not wait and are tested now (P23).

- **Years.** The year shown starts at the current year (the year of `today_date`). Back stops at the previous year with `last12Months` and at 2000 with `allHistory`; forward never passes the current year (R1380). When the history changes to `last12Months` while an earlier year is shown, the shown year becomes the previous year.
- **Cells.** A date of the shown year is drawn when it is not after today and either it has reviews, or the history is `allHistory`, or it is after the same date one year ago (today's date with the year one less, a 02-29 date read as 03-01) (R1380). So with `last12Months` an empty date one year old or older is not drawn, and a date after today is never drawn, even with reviews from clock skew (P21). Every date of the year can be drawn, 31 December of a leap year included. Decided 2026-10-27: Anki compares each date's midnight with the current time one year back, which gives the same dates except at exactly midnight, and it fills only the first 365 dates of a year, so it never draws an empty 31 December of a leap year (P21).
- **Colour.** A day with reviews takes a square-root scale over [0, `max_count`] (all years, dates after today included); a day without reviews takes the empty colour (R1382). The palette is U227's.
- **Tooltip.** Two lines: the localized long date of `stats/heatmap-presentation` M10.2 (English: `Monday, October 12, 2026`), then `<count> review(s)` (R1383). Its placement and timing are `stats/heatmap-presentation` M12 and M13.
- **Query.** The cell's item text, when its record cell carries one (`stats/graphs-revlog` V3.4, `stats/browse-queries` Q4.2). A drawn date without a record cell has none.
- **No data** (P16.5) when the record has no cell, whatever the year shown.

## P8. Reviews

**Data.** The count map, or the time map (milliseconds) when Time is on (R1386). Each entry has the five kinds; a bin's value per kind is the sum of its entries', and its total is the sum over the kinds.

**Domain.** x1 = 1. x0 = −30, −89 or −364 for 1 month, 3 months and 1 year (R1387); for all, the smallest key. Call this first x0 the range start s. Desired count c = min(70, |s|).

1. Ticks t = T(x0, 1, c).
2. For all only: when t has at least two ticks, the spacing g = t2 − t1 is above 0, and the gap t1 − x0 is above 0 and below g, set x0 = t1 − g and take t = T(x0, 1, c) again.
3. For 1 year and all: replace each tick t by min(t + 1, 1), drop duplicates, sort. This shifts the bins one day so the newest bin is `0-4 days ago` rather than 0 to 5 (R1387).
4. Bins over [x0, 1] by P4.3.

So the domain ends at day 1: rows dated tomorrow (clock skew) are in the last bin, [1, 1] when a threshold is 1.

**Empty.** No data when the map is empty or no bin holds data.

**Tooltip** of a drawn bin [a, b): the day text is D(s, b′) for the first bin and D(⌊a⌋, b′) for the others, with b′ = 1 when b = 0 and b otherwise. The lines are label-value pairs, in this order:

| Label | Value |
|---|---|
| the day text | the bin's total |
| `Filtered` | its Filtered value |
| `Learning` | its Learn value |
| `Relearning` | its Relearn value |
| `Young` | its Young value |
| `Mature` | its Mature value |
| `Running total` | the sum of this bin's and every earlier bin's totals |

Each kind label carries its colour swatch (P17.2). A value is `<v> review(s)` by F, or with Time on the long precise span of v ÷ 1,000 seconds capped at hours (P2.2). All five kinds are listed even when 0. Reviews bars have no query: Anki makes them not clickable.

**Table** (R1389). days = −s + 1. studied = the number of entries in the domain (the days with an entry, the day-1 bin included). pct = studied ÷ days × 100. total = the sum of all bins' totals.

| Label | Value | When |
|---|---|---|
| `Days studied` | `<studied> of <days> (<P>%)`, the two counts by F, P as below | always |
| `Total` | `<total> review(s)`, or with Time the long precise span capped at hours | always |
| `Average over period` | `<round(total ÷ days)> review(s)/day`, or with Time `<round(total ÷ days ÷ 1,000 ÷ 60)> minute(s)/day`, divided in that order | always |
| `Average for days studied` | the same with studied in place of days | only when pct < 100 |
| `Average answer time` | `<a>s (<r> cards/minute)`, a = total seconds ÷ answers, r = answers × 60 ÷ total seconds, both by F | only with Time |

- P is L(2) of pct when pct < 99.5, L(1) when pct < 99.95, L(2) when pct < 100, and `100` otherwise. So 364 of 365 is `99.7`, 4,000 of 4,001 `99.98`, 30 of 31 `96.77`, and the table shows 100 only when every day was studied (R1389).
- answers is the sum of the count map's five kinds over the entries in the domain, read even when Time is on. total seconds = total ÷ 1,000. A zero time gives `0s (∞ cards/minute)`.

**Examples.** Count map with entries at −400, −364, −30, −5, −4, −1, 0 and 1:

| Range | Domain | Bins | First bin | Newest bins |
|---|---|---|---|---|
| 1 month | [−30, 1] | 32 | [−30, −29) `30 days ago` | [−1, 0) `Yesterday`, [0, 1) `Today`, [1, 1] not drawn |
| 3 months | [−89, 1] | 91 | [−89, −88) `89 days ago` | as 1 month |
| 1 year | [−364, 1] | 74 | [−364, −359) `360-364 days ago` | [−9, −4) `5-9 days ago`, [−4, 1) `0-4 days ago`, [1, 1] not drawn |
| all | [−400, 1] | 82 | [−400, −399) `400 days ago` | as 1 year |

With entries at −1,000 and 0 only, all gives a step of 20 and the bins [−1,000, −999) `1,000 days ago`, [−999, −979) `980-999 days ago`, …, [−19, 1) `0-19 days ago`, [1, 1]. With only the entry 0, all gives one bin [0, 1] `Today`.

Step 2, with the oldest entry at −403 and all (s = −403, c = 70): the step is 5 and t is −400 … 0. The gap 3 is below 5, so x0 = −405 and t is −405 … 0, which step 3 shifts to −404, −399, …, 1. The 83 bins are [−405, −404) `403 days ago` (one day wide and empty; its text starts at s), [−404, −399) `400-404 days ago`, …, [−4, 1) `0-4 days ago` and [1, 1]. The oldest bin is not as wide as the others. Table, 1 month, counts {−30: 1, −5: 1, −4: 2, −1: 3, 0: 5, 1: 7}: Days studied `6 of 31 (19.35%)`, Total `19 reviews`, Average over period `1 review/day`, Average for days studied `3 reviews/day`.

## P9. Card Counts

The record of `stats/graphs-cards` C4.4. The shown set is `excluding_inactive` when `cardCountsSeparateInactive` is true, else `including_inactive` (R1391).

- Rows, in order: New, Learning, Relearning, Young, Mature, then Suspended and Buried only when separating. Each row: its colour swatch (P17.2), its label, the count by L, and the percentage `L(count ÷ m × 100)%`, where m = max(1, the sum of all seven counts of the shown set). So percentages have at most two decimals with trailing zeros removed (`50%`, `33.33%`); they need not add up to exactly 100.
- `Total`: the sum of the seven counts by L, no percentage.
- `Notes` (Meguri, R1392): the distinct-notes count by L, no percentage, after Total.
- The donut has one sector per row in row order (the seven counts, suspended and buried 0 when not separating), drawn as `SectorMark`s. An empty scope draws no sector and still shows the table with zeros.
- Each row's label opens the Browser with its text from the record (`stats/browse-queries` Q4.1). Total and Notes have no query.

The plan's U230 check "percentages sum to 100.00% with 2 decimals" is superseded by the L rule above. Decided 2026-10-27.

## P10. Intervals and Stability

**Data.** Intervals: interval → cards (`stats/graphs-cards` C5, key 0 included). Stability: rounded stability → cards (C7). n is the total count. No data when n = 0.

**Domain.** Let M be the largest key, and m:

| Range | m | Widened |
|---|---|---|
| 1 month | min(M, 30) | no |
| 50% | Q(0.5) (P4.5) | yes |
| 95% | Q(0.95) | yes |
| all | M | yes |

1. u = m + 1. The domain is [0, u], widened to N(0, u) when the table says so (P4.2).
2. Intervals adds 1 to both ends; Stability does not. So Intervals never shows interval 0 (R1394) and Stability shows rounded stability 0.
3. Desired count c = min(70, u) (u before widening). t = T(domain, c).
4. Thresholds: shift every tick by 1 − t1 (so the first tick becomes 1), and after the last shifted tick add one more threshold, the last unshifted tick + 1.
5. Bins over the domain by P4.3. A bin's value is its number of cards.

**Empty.** No data also when no bin has a value (every interval 0, for Intervals).

**Tooltip** of a drawn bin [a, b), with v its value and w = Intervals' `interval` or Stability's `stability`:

- `<v> card(s) with a <a> day <w>` when b − a ≤ 1, else `<v> card(s) with a <a>~<b − 1> day <w>` (R1396);
- `Running total: <L(1) of running ÷ n′ × 100>%`, where running is the cards of this bin and every earlier bin, and n′ the cards of all bins.

**Query** (when the bin holds data), with z = b − 1:

- Intervals: with y = z when b is a whole number and y = ⌈b⌉ − 1 otherwise, `"prop:ivl=<a>"` when a = y, else `"prop:ivl>=<a>" "prop:ivl<=<y>"` (two terms and a space, R1396). Only the last bin's upper edge can be fractional; every lower edge is a whole number. There y is the largest interval in the bin, so the query finds the cards the bin counts. Anki's page writes z there (`"prop:ivl<=3.5"`), which its search rejects because `prop:ivl` takes an unsigned integer (`search/properties` P2). Decided 2026-10-27, for R1420 (P21).
- Stability: `"prop:s>=<a′ − 0.5>" "prop:s<<z + 0.5>"`, with a′ = 0.5 when a = 0 and a otherwise (R1399). So the bin [0, 1) is `"prop:s>=0" "prop:s<0.5"`. Stability keeps z even for a fractional edge: `prop:s` takes decimals, so Anki's query parses and Meguri writes it unchanged.

**Table.** One row: `Median interval` or `Median stability`, the long precise span of round(Q(0.5)) × 86,400 seconds (P2.2), with Q over every key, 0 included; `0 seconds` when there is no card.

**Examples.** The map {0: 3, 1: 4, 2: 2, 3: 1, 5: 6, 10: 3, 21: 2, 45: 1, 120: 2, 400: 1} (25 cards; Q(0.5) = 5, Q(0.95) = 120):

| Graph, range | Domain | Bins | First drawn bins | Last bins |
|---|---|---|---|---|
| Intervals, 1 month | [1, 32] | 32 | [1, 2) `4 cards with a 1 day interval`, `Running total: 22.2%`, `"prop:ivl=1"` | [31, 32), [32, 32] not drawn |
| Intervals, 95% | [1, 131] | 66 | [1, 3) `6 cards with a 1~2 day interval`, `"prop:ivl>=1" "prop:ivl<=2"` | [129, 131) `… 129~130 day interval`, [131, 131] |
| Intervals, all | [1, 451] | 91 | [1, 6) `13 cards with a 1~5 day interval`, `59.1%` | [446, 451), [451, 451] |
| Stability, 1 month | [0, 31] | 32 | [0, 1) `3 cards with a 0 day stability`, `14.3%`, `"prop:s>=0" "prop:s<0.5"` | [30, 31), [31, 31] |
| Stability, 95% | [0, 130] | 66 | [0, 1), then [1, 3) `"prop:s>=0.5" "prop:s<2.5"` | [127, 129), [129, 130] `… 129 day stability` (drawn, closed) |

Median `5 days` for both. In the 1-month range the cards with intervals 31 and 32 count in the running total although the range is named a month, as in Anki. The 95% axis ends at the widened 130 or 131, not at 120; the plan's U231 check "last bar ends at 120" is superseded by these rules.

With keys 1, 2, 3, 4 (one card each), Intervals 50%: Q(0.5) = 2.5, u = 3.5, domain [1, 4.5], bins [1, 2), [2, 3), [3, 4), [4, 4.5]. The last bin is drawn and holds interval 4: `1 card with a 4 day interval`, `Running total: 100%`, query `"prop:ivl=4"` (Anki's page: `"prop:ivl>=4" "prop:ivl<=3.5"`). A fractional edge appears only when the quantile is fractional and the widening keeps it, which is common in small collections.

**Footnote** (Stability only here; Difficulty and Retrievability in P11): when the left-out count of `stats/graphs-cards` C6.2 is above 0, the graph shows P19's footnote under it (R1398).

## P11. Difficulty and Retrievability

**Data.** Difficulty: 1% key → cards (`stats/graphs-cards` C8.3 `eases`). Retrievability: 1% key → cards (C9.7). No data when the map is empty.

**Domain.** With q = 0.75 for 50%, 0.975 for 95% and 1 for 100%:

1. lo = E(1 − q) and hi = E(q) (P4.5, 1 − q in binary64); for all, lo = 0 and hi = 100 (R1400).
2. d = N(lo, hi); ticks t = T(d, 20).
3. With g = t2 − t1: when g < 1, the ticks are lo, lo + 1, … while below hi (each lo + i), and g = 1.
4. off = lo − (d's lower end). When off is 0, or neither off nor g is a whole multiple of the other (off mod g ≠ 0 and g mod off ≠ 0), the domain is d with ticks t. Otherwise both the domain ends and the ticks move up by off.
5. Bins by P4.3.

When t has fewer than two ticks, steps 3 and 4 keep d and t as they are.

**Tooltip** of a drawn bin [a, b): `<v> card(s) with <a>%-<b>% difficulty` or `… retrievability` (R1401), a and b written as in P4.7. No running total and no cumulative overlay.

**Query** (when the bin holds data): `"prop:d>=<a ÷ 100>" AND "prop:d<<b ÷ 100>"`, with `<=` in place of `<` when b − 1 = 99; the same with `prop:r` (R1401, R1404).

**Table.**

| Graph | Label | Value |
|---|---|---|
| Difficulty | `Median difficulty` | `L(0) of the record's median%` (`stats/graphs-cards` C8.2) |
| Retrievability | `Average retrievability` | `L(0) of average%` (C9.4) |
| Retrievability | `Estimated total knowledge` | `<W of sum_by_card> card(s) / <W of sum_by_note> note(s)` (R1403) |

**Examples.**

| Map | Range | Domain | Bins |
|---|---|---|---|
| A = {3: 1, 12: 4, 40: 10, 41: 5, 57: 8, 63: 2, 88: 3, 99: 2} | all; 100% and 95% (lo 3, hi 99) | [0, 100] | [0, 5) `1 card with 0%-5% difficulty`, …, [95, 100) with `"prop:d>=0.95" AND "prop:d<=1"`, [100, 100] not drawn |
| A | 50% (lo 40, hi 57) | [40, 58] | [40, 41) `10 cards with 40%-41% difficulty`, `"prop:d>=0.4" AND "prop:d<0.41"`, …, [57, 58), [58, 58] |
| {11: 1, 87: 1} | 100% | [11, 91] (off 1) | [11, 16) `"prop:d>=0.11" AND "prop:d<0.16"`, …, [86, 91), [91, 91] |
| {13: 1, 87: 1} | 100% | [10, 90] (off 3, not moved) | [10, 15) … [85, 90), [90, 90] |
| {40: 5, 42: 1} | 100% | [40, 42] | [40, 41), [41, 42] |
| {40: 5} | 100% | [40, 40] | one bin [40, 40], not drawn: the chart shows no bar although the table has values, as in Anki |

**Footnote**: as P10's, with the same left-out count.

## P12. Retention

A table only, no chart (R1405 to R1409), from the counts of `stats/graphs-revlog` V4.3. Rows, in order: `Today`, `Yesterday`, `Last week`, `Last month`, `Last year`, and `All time` only with `allHistory` (R1406, R1367). Each row has young and mature pass and fail counts from the record.

- **All** (the default) shows the summary table: columns `Young`, `Mature`, `Total`, `Count`. Young, Mature and Total are PC of passed ÷ (passed + failed) for the young counts, the mature counts and both together, or `N/A` when that sum is 0. Count is the row's pass and fail counts summed, by L.
- **Young** and **Mature** show the single-group table: columns `Pass`, `Fail`, `Retention`. Pass and Fail by L; Retention by PC or `N/A` (R1407).
- The first column holds the row names and has no header.
- A help button shows P19's retention help text (R1409).
- The table is always shown; a period with no counted row reads `N/A` and a Count of `0`.

## P13. Hourly Breakdown

The record's 24 entries for the selected range (`stats/graphs-revlog` V5.3: 1 month `one_month`, 3 months `three_months`, 1 year `one_year`, all `all_time`), hour 0 to 23, each `{total, correct}`.

- No data when every total is 0.
- Bars: one per hour, value its total. A "% correct" line on a right axis from 0% to 100%, with a point at each hour whose total is above 0 (correct ÷ total) and a gap elsewhere (R1410).
- Tooltip lines (R1412): `From <h>:00~<h + 1>:00` (so hour 23 reads `From 23:00~24:00`), `<total> reviews`, `<p>% correct (<correct>)`, where p = correct ÷ total × 100, or 0 when total is 0, and every number is written by F. `reviews` has no singular form here (`1 reviews`), as in Anki.
- Bars have no query (R1412).
- Hour labels under the bars; in a compact horizontal size class only the even hours are labelled (R1412).

## P14. Answer Buttons

The record's three groups for the selected range (`stats/graphs-revlog` V6.2, ranges as P13), Learning, Young and Mature, each with the counts of buttons 1 to 4.

- No data when all twelve counts are 0.
- For a group: T = the sum of its four counts, C = its counts of buttons 2 to 4, and its percent `L(C ÷ T × 100)`, or `0` when T is 0.
- Group labels under the axis: `Learning`, `Young`, `Mature`. Meguri adds a second line under each, `<C>/<T> correct (<percent>%)` (R1414; P21).
- Tooltip lines of a bar: `Button: <n> (<name>)` with the names `Again`, `Hard`, `Good`, `Easy`; `Times pressed: <L(count)> (<share>%)` with share = `L(count ÷ T × 100)`, or `0` when T is 0; `<C>/<T> correct (<percent>%) → (not 'Again')`, with C and T by F.
- Bars have no query (R1414).

## P15. Added

**Data.** The `added` map (`stats/graphs-cards` C10). No data when the map is empty.

**Domain.** x1 = 1; x0 = −31, −90 or −365 for 1 month, 3 months and 1 year, and the smallest key for all (R1415). Desired count min(70, |x0|). Thresholds T(x0, 1, that count), bins by P4.3.

**Empty.** No data also when no bin has a value.

**Bin text** of a drawn bin [a, b), with start = |a| + 1 and end = |b| + 1:

- Tooltip lines (R1416): `D(a, b):`, `<value> card(s)`, `Running total: <running> card(s)`, where running is this bin's and every earlier bin's values.
- Query (when the bin holds data): `"added:1"` when start = 1, else `"added:<start>" AND -"added:<end>"` (R1416).

**Table.** total = the sum of all bins' values; days = |x0|.

| Label | Value |
|---|---|
| `Total` | `<total> card(s)` |
| `Average` | `<round(total ÷ days)> card(s)/day` (days 0 gives `∞ cards/day`) |

**Example**, `added` {−400: 1, −31: 2, −30: 1, −5: 1, −1: 3, 0: 5, 1: 7}:

| Range | Bins | Oldest | Newest |
|---|---|---|---|
| 1 month | 33 | [−31, −30) `31 days ago`, `"added:32" AND -"added:31"` | [−1, 0) `Yesterday`, `"added:2" AND -"added:1"`; [0, 1) `Today`, `"added:1"`; [1, 1] not drawn |
| 1 year | 74 | [−365, −360) `361-365 days ago` | [−5, 0) `1-5 days ago`, `"added:6" AND -"added:1"`; [0, 1] `Today` (drawn, closed: days 0 and 1), `"added:1"` |
| all | 81 | [−400, −395) `396-400 days ago`, `"added:401" AND -"added:396"` | as 1 year |

## P16. Selection, tooltips and Show in Browser

### P16.1 Clickable items

| Graph | Clickable |
|---|---|
| Future Due, Intervals, Stability, Difficulty, Retrievability, Added | each drawn bin that holds data (P4.3), with its query |
| Card Counts | each type and inactive row label (`stats/browse-queries` Q4.1) |
| Calendar | each drawn cell whose record cell carries an item text (`stats/graphs-revlog` V3.4, `stats/browse-queries` Q4.2) |
| Reviews, Hourly Breakdown, Answer Buttons, Retention, Today | nothing |

A drawn bin without data still shows its tooltip (with 0), but has no query.

### P16.2 Mac

- Hovering a drawn bin selects it, highlights its column and shows its tooltip next to the pointer; leaving the plot hides it (R1365). The Calendar is the exception: its hover, tooltip placement and outlines are `stats/heatmap-presentation` M9 and M12.
- Clicking a clickable item opens the Browser at once with its joined query (`stats/browse-queries` Q3, Q5; R1418). Clicking anything else does nothing.

### P16.3 iPhone and iPad

- A tap, or touch-and-hold, on a drawn bin selects it and shows its tooltip (R1365, R1418). Dragging across the plot moves the selection from bin to bin (`chartXSelection`).
- The tooltip of a clickable item adds a `Show in Browser` button, which opens the Browser with its joined query (`stats/browse-queries` Q5). Non-clickable graphs never show the button.
- Tapping outside the plot, or another range, clears the selection.
- The Calendar is the exception: its tap, callout, scrub and swipe rules are `stats/heatmap-presentation` M13 (a drag without a hold pages or scrolls; it does not move the selection).

### P16.4 Tooltip content

A tooltip is a list of lines in the order each graph gives (P6 to P15); Reviews' lines are label-value pairs. The same lines are the bin's accessibility label (P18.2). A pointer device and touch show the same text. Changing a control re-bins with no core call and clears the selection (R1290).

### P16.5 No data

A graph whose section says "no data" shows a native empty state with P19's `No Data` in place of the plot, keeps its title, subtitle and controls, and shows no table, except Card Counts (P9), which always shows its table.

## P17. Cumulative overlay and colours

### P17.1 Running total area

Future Due, Reviews, Intervals, Stability and Added draw the running total as a translucent area behind the bars against a right axis from 0 to the total of all bins, widened by N (R1364). The area starts at 0 on the first bin's lower edge and goes up to each bin's running total at its upper edge. It is drawn only when the total is above 0. Hourly Breakdown's "% correct" line (P13) uses the same right-axis style. Difficulty and Retrievability have no area.

### P17.2 Colours

Semantic chart colours come from U226's `ChartColors`, built on system colours with light, dark and Increase Contrast variants (R1363; `docs/design/native-interface-contract.md`):

| Meaning | Colour |
|---|---|
| New | blue |
| Learning | orange |
| Relearning | red |
| Young | light green |
| Mature | dark green |
| Suspended | yellow |
| Buried | grey |
| Filtered | purple |
| Future Due bars | green |
| Intervals, Stability, Added, Hourly bars | blue |
| Difficulty bins | red (hard, high) to green (low) by the bin's value |
| Retrievability bins | red (low) to green (high) |
| Answer Buttons | red (Again) to green (Easy) |

- Reviews stacks, from the bottom: Mature, Young, Relearning, Learning, Filtered (R1388).
- Anki shades each bar by its position on the axis; Meguri uses one colour per series. Bin edges, values and text do not depend on colour.
- No information is carried by colour alone (R1443): every series and sector is named in its tooltip, table row and accessibility label.

### P17.3 Axes

Axes and their tick marks are Swift Charts' automatic ones. Count ticks are written by L; Reviews' time ticks are the short precise span capped at hours (P2.2); percentage axes read `<n>%`. Tick positions are not part of the presentation goldens.

## P18. Accessibility

### P18.1 Chart summary

Each chart is one accessibility container whose label is its summary (R1439): the title, the subtitle when there is one, the scope (`Collection`, `Deck <full name with ::>` or `Search <text>`), the range label when the graph has a range, then each table row as `<label>: <value>`. The parts are joined with `. `, and a part that already ends with `.` takes only a space. A graph with no data reads `<title>. <scope>. No Data`.

Example (P6's data with a daily load of 3, Deck `Japanese`): `Future Due. The number of reviews due in the future. Deck Japanese. 1 Month. Total: 15 reviews. Average: 0 reviews/day. Due tomorrow: 4 reviews. Daily load: 3 reviews/day`.

### P18.2 Bins, sectors and rows

- Swift Charts' automatic mark elements are replaced by one element per drawn bin, in axis order (R1440). Its label is the bin's tooltip lines joined with `, `, without the colon that ends the first line of Future Due and Added; a label-value line reads `<label> <value>`.
- Clickable items expose `Show in Browser` as a named accessibility action that opens the Browser as P16 does. Non-clickable graphs expose no such action.
- Bins are grouped in runs of ten consecutive drawn bins (the last run may be shorter), each run a container labelled `<first bin's name> to <last bin's name>` with the bin names of P18.5, so a 70-bar chart can be skimmed by run and then explored (R1440). Example, Reviews over 1 month (P8): the first run reads `30 days ago to 21 days ago`. A run of one bin reads that bin's name alone.
- Card Counts sectors and rows, Retention rows, Hourly bars and Answer Buttons bars are elements labelled with their row or tooltip text.
- The Calendar is the exception to the runs of ten: its day elements sit in month containers (`stats/heatmap-presentation` M15.1, M15.4), each labelled with its tooltip lines joined with `, ` as above.

### P18.3 Audio Graphs

Each chart with marks publishes an `AXChartDescriptor` (R1439): an x axis of categories, one per drawn bin, hour or bar, named by its bin name (P18.5); a numeric y axis named by its unit (cards, reviews, minutes, percent); one data series per stacked kind (Reviews), per bar and line (Hourly), or one series. The Calendar's descriptor is U227's (`stats/heatmap-presentation` M15.6). Retention, a table, has none.

### P18.4 Text size and motion

- Every stats text uses platform text styles and follows Dynamic Type up to the largest accessibility size (R1444). At accessibility sizes, table rows stack the value under its label, and the plot keeps its default height (it never shrinks to make room for text).
- Range changes use the system's default chart animation, which Reduce Motion already shortens; no custom motion (R1368, `docs/design/native-interface-contract.md` N11.7).

### P18.5 Bin names

The short name of a drawn bin [a, b), a bar or a sector, used by the run labels of P18.2 and the Audio Graphs categories of P18.3. Decided 2026-10-27 (Meguri's own; Anki's page has no such text):

| Graph | Name | Examples |
|---|---|---|
| Future Due | D(a, e), e as P6's bin text | `Tomorrow`, `In 30-31 days` |
| Reviews | P8's day text of the bin | `30 days ago`, `0-4 days ago` |
| Added | D(a, b) | `31 days ago`, `Today` |
| Intervals, Stability | `<a> day(s)` when b − a ≤ 1, else `<a>~<b − 1> days`, the numbers written as in P10's tooltip | `0 days`, `1 day`, `1~2 days`, `129~130 days` |
| Difficulty, Retrievability | `<a>%-<b>%`, a and b as in P11's tooltip | `40%-41%`, `95%-100%` |
| Hourly Breakdown | `<h>:00`, h by F | `9:00`, `23:00` |
| Answer Buttons | `<group>, <button name>` (P14's group labels and button names) | `Young, Good` |
| Card Counts | the row label (P9) | `Suspended` |
| Calendar | `stats/heatmap-presentation` M15.6 | `Oct 12` |

## P19. Strings

Meguri takes these texts only from this table. `<…>` marks a number or name; `(s)` the plural ending of P2.1.

| Where | Text |
|---|---|
| Titles | `Future Due`, `Calendar`, `Reviews`, `Card Counts`, `Review Intervals`, `Card Stability`, `Card Difficulty`, `Card Retrievability`, `Retention`, `Hourly Breakdown`, `Answer Buttons`, `Added` |
| Future Due subtitle | `The number of reviews due in the future.` |
| Reviews subtitle | `The number of questions you have answered.`; with Time `The time taken to answer the questions.` |
| Intervals subtitle | `Delays until review cards are shown again.` |
| Stability subtitle | `The delay at which retrievability falls to 90%.` |
| Difficulty subtitle | `The higher the difficulty, the slower stability will increase.` |
| Retrievability subtitle | `The probability of recalling a card today.` |
| Retention subtitle | `Pass rate of cards with an interval ≥ 1 day.` |
| Hourly subtitle | `Review success rate for each hour of the day.` |
| Answer Buttons subtitle | `The number of times you have pressed each button.` |
| Added subtitle | `The number of new cards you have added.` |
| Range labels | `1 Month`, `3 Months`, `1 Year`, `All`, `50%`, `95%`, `100%`: title case, as `docs/design/native-interface-contract.md` N11.7 sets for the history labels. Anki writes `1 month`, `3 months`, `1 year`, `all`; P22's `range` values keep that wording. |
| Toggles | `Backlog`, `Time`, `Separate suspended/buried cards` |
| Calendar week start (U230, R1381) | `Week starts on`; items `Sunday`, `Monday`, `Friday`, `Saturday` (the K5 values 0, 1, 5, 6) |
| Days (P3) | `Today`, `Tomorrow`, `In <n> days`, `In <a>-<b> days`, `Yesterday`, `<n> days ago`, `<a>-<b> days ago` |
| Running total | `Running total` |
| Counts | `<n> card(s)`, `<n> note(s)`, `<n> review(s)`, `<n> card(s) due`, `<n> review(s)/day`, `<n> minute(s)/day`, `<n> card(s)/day` |
| Spans (P2.2) | `<n> second(s)`, `minute(s)`, `hour(s)`, `day(s)`, `month(s)`, `year(s)`; short `<n>s`, `<n>m`, `<n>h`, `<n>d`, `<n>mo`, `<n>y` |
| Tables | `Total`, `Average`, `Due tomorrow`, `Daily load`, `Days studied`, `<a> of <b> (<p>%)`, `Average over period`, `Average for days studied`, `Average answer time`, `<a>s (<r> cards/minute)`, `Median interval`, `Median stability`, `Median difficulty`, `Average retrievability`, `Estimated total knowledge` |
| Card Counts rows | `New`, `Learning`, `Relearning`, `Young`, `Mature`, `Suspended`, `Buried`, `Total`, `Notes` (Meguri) |
| Reviews kinds | `Filtered`, `Learning`, `Relearning`, `Young`, `Mature` |
| Intervals tooltip | `<n> card(s) with a <d> day interval`, `<n> card(s) with a <a>~<b> day interval` |
| Stability tooltip | `<n> card(s) with a <d> day stability`, `<n> card(s) with a <a>~<b> day stability` |
| Difficulty, Retrievability tooltips | `<n> card(s) with <a>%-<b>% difficulty`, `<n> card(s) with <a>%-<b>% retrievability` |
| Hourly | `From <h>:00~<h2>:00`, `<n> reviews`, `<p>% correct (<n>)` |
| Answer Buttons | `Button`, `Times pressed`, `Again`, `Hard`, `Good`, `Easy`, `<c>/<t> correct (<p>%)`, `→ (not 'Again')` |
| Retention | `Young`, `Mature`, `All`, `Total`, `Count`, `Pass`, `Fail`, `Retention`, `Today`, `Yesterday`, `Last week`, `Last month`, `Last year`, `All time`, `N/A` |
| Retention help | `If you are using FSRS, your retention is expected to be close to your desired retention. Please keep in mind that data for a single day varies a lot, so it's better to look at monthly data.` |
| Calendar tooltip | `<n> review(s)` under the date |
| Memory-state footnote (Meguri) | `1 review card without a memory state is not included.`, `<n> review cards without a memory state are not included.` |
| Empty state (Meguri) | `No Data` |
| Action | `Show in Browser` |
| Accessibility (Meguri, P18) | summary scope parts `Collection`, `Deck <name>`, `Search <text>`; run label `<a> to <b>`; bin names `<n> day(s)`, `<a>~<b> days`, `<a>%-<b>%`, `<h>:00`, `<group>, <button name>`; y axis names `cards`, `reviews`, `minutes`, `percent` |

## P20. Decisions

Decided 2026-10-27:

1. **Anki's edges over the 70 wording.** P4.6.
2. **Undrawn last bins.** P4.4: the zero-width bin Anki's arithmetic makes is kept in the values and hidden from view, as Anki shows it. Merging it into the bar before would change Future Due's and Intervals' numbers against the page.
3. **Range memory and history following.** P5.2, P5.3.
4. **Card Counts percentages** keep Anki's at-most-two-decimals form (P9).
5. **Answer Buttons group labels** add the correct line (P14), because R1414 asks for it and the pinned page puts it only in the tooltip.
6. **Hourly text without the chart emoji.** Anki's lines start with a bar-chart and a line-chart emoji. Meguri leaves them out, as R1412 words the lines: VoiceOver would read the emoji names in every bar's label.
7. **Goldens.** Presentation goldens are recorded from the pinned graphs page in the quarantined image `oracle/graphs-page/` (`stats/graphs-common` G12, decision 2). Swift tests read only the goldens.
8. **Fractional Intervals edge.** P10: the query ends at ⌈b⌉ − 1, so it parses and finds the bin's cards.
9. **Drawn bins by their edges.** P4.4: every bin with x1 > x0 is drawn, whatever its width on screen.
10. **Calendar's empty dates.** P7: after the same date one year ago, every date of the year.
11. **Range labels and motion.** P19 title-cases the range labels and P18.4 keeps the system animation, both with `docs/design/native-interface-contract.md` N11.7.
12. **Bin names.** P18.5 gives each bin a short name for VoiceOver runs and Audio Graphs categories.
13. **Calendar drawing waits.** P7: the grid, palette and descriptor come from `stats/heatmap-presentation`; the Calendar's other rules are in force now.
14. **Plan tests superseded.** P3 (`6-10 days ago`) and P4.7 (no U88 builder).

## P21. Differences from Anki

| Difference | Why | Golden effect |
|---|---|---|
| Ranges remembered per device; Anki resets them at each open (P5.2) | R1366 | none (goldens fix the range) |
| No U+2068, U+2069 or U+200E marks (P2) | `stats/studied-today` T6.3 | the comparison removes them from the page's text |
| Opening with `allHistory` and no stored selection shows the default range (1 month or 1 year); Anki opens at all (P5.3) | R1366 | none (goldens fix the range) |
| Hourly lines without the leading emoji and space (P13) | P20 item 6 | the comparison removes them from the page's text |
| Intervals' last bin with a fractional upper edge queries up to ⌈b⌉ − 1; Anki writes b − 1, which its search rejects (P10) | R1420 | the `intervals-quantiles` map has whole quantiles, so no golden holds one; Meguri-only test |
| Every bin with x1 > x0 is drawn and hoverable; Anki gives no width to a bin one pixel wide or narrower on its fixed plot (P4.4) | native charts scale to the view | `drawn` is computed from the edges (P22) |
| Calendar fills empty dates strictly after the same date one year ago, without the time of day, and every date of a leap year; Anki fills by the current time and only the first 365 dates (P7) | simpler, clock-free rule | no Calendar golden; Meguri-only test |
| Range labels in title case (P19) | N11.7 | none (requests use Anki's wording) |
| Answer Buttons second label line (P14) | R1414 | not in the page; Meguri-only test |
| `No Data` empty state; Anki overlays `NO DATA` (P16.5) | native empty state | the golden records only that the overlay shows |
| Calendar never draws dates after today, even with reviews (P7) | R1380 | Meguri-only test |
| Card Counts `Notes` row (P9), the memory-state footnote (P10, P11) | R1392, R1398 | Meguri-only tests |
| Stability, Difficulty and Retrievability shown whatever `fsrs` says | R1397 (`stats/graphs-cards` C6.3) | scenarios set `fsrs` true |
| One colour per series (P17.2) | native charts | none |
| Card Counts' Mature label query (`stats/browse-queries` Q7) | R1393 | the comparison reads Q4.1's texts |
| Retention help text: in the sentence about single-day data, P19's `varies a lot` replaces Anki's wording, because the repository's private-token scan rejects one of Anki's words. Decided 2026-11-10 (proposed): P19's text is final for U230 and U231 | R1409 asks for Anki's text; the scan rule wins | none (the help text is in no golden) |

The text differences are one pending `conformance/deviations.yaml` entry, `stats-presentation-text-marks`, added with the first presentation golden. The fractional Intervals query is a second, `stats-intervals-fractional-edge`, added with its Meguri-only test (P23).

## P22. Observation for scenarios

A draft observation kind, `stats_presentation` (`conformance/observations` O4, `conformance/stats-ops` H14), for the oracle-maintainer. As of 2026-11-10 its scenarios are in the tree (P23), inactive and without goldens; no executor has the kind and the graphs-page image does not exist yet.

- **Request.** `{stats_presentation: [<request>, ...]}`, a non-empty list. A request is `{scope, history, graph, controls}`: `scope` and `history` as `stats/graphs-common` G14; `graph` one key of P1 other than `today`; `controls` an object with exactly the members its graph has:

  | Member | Graphs | Value |
  |---|---|---|
  | `range` | `future_due`, `reviews`, `intervals`, `stability`, `difficulty`, `retrievability`, `hours`, `buttons`, `added` | one of the graph's P5.1 options in Anki's wording: `1 month`, `3 months`, `1 year`, `all`, `50%`, `95%`, `100%` |
  | `backlog` | `future_due` | boolean: the stored `futureDueShowBacklog` value the graph is drawn with (P6) |
  | `time` | `reviews` | boolean: the Time toggle (P8) |
  | `separate` | `card_counts` | boolean: the stored `cardCountsSeparateInactive` value the graph is drawn with (P9) |
  | `mode` | `retention` | `young`, `mature` or `all` (P12) |
  | `year` | `calendar` | an integer, the year shown (P7) |

  `backlog` and `separate` are preference values, not clicks: the graph is drawn as if the collection held that value (`stats/graphs-common` G7), whether or not the toggle is shown. Backlog's toggle is hidden whenever have backlog is false (P5.1), and the oracle never clicks a hidden control. A scenario that also wants the stored key to agree writes it with `set_config` before the observation and passes the same value (`future-due-ranges`).
- **Value.** One object per request, in order: `{data, page}`.
  - `data`: the `stats` parts (G14, `conformance/stats-ops` H10) of the request's scope and history that the graph is built from, as an object with one member per part:

    | `graph` | Parts |
    |---|---|
    | `future_due` | `future_due` |
    | `calendar` | `reviews` (the Calendar's dates, `max_count` and item texts are not in `stats`, H10) |
    | `reviews` | `reviews` |
    | `card_counts` | `card_counts` |
    | `intervals`, `stability`, `difficulty`, `retrievability`, `hours`, `buttons`, `added` | the part of the same name |
    | `retention` | `true_retention` |

  - `page`: what the page presents, `{no_data, bins, table}`. `no_data` is whether the no-data state shows (P16.5). `bins` lists every bin in axis order as `{x0, x1, value, drawn, tooltip, query}`: `drawn` is x1 > x0 computed from the recorded edges, never from the page's bar or hover width (P4.4); `tooltip` is the list of the bin's tooltip lines, each a string or, for a label-value line, `[label, value]`; `query` is the item text a click sends, or `null`. `table` lists the table's rows in order, each `[label, value, ...]`. Card Counts' `page` adds `sectors`, the seven sector values in row order (P9). Retention's `page` is `{table}` only, and the Calendar's `{no_data}` only: its cells and tooltips are Meguri-only tests (P7, P21, P23).
- **Oracle.** The graphs-page image (`stats/graphs-common` G12, decision 2; `oracle/graphs-page/`) runs the pinned page against Anki's Python-side stats backend for the request's search and days (G14's mapping), gives it the preference values of `controls`, sets the other controls, and reads for each bin its edges, value, tooltip text (one entry per line, with U+2068, U+2069, U+200E and the Hourly lines' leading emoji and space removed, P21) and the query a click sends, the table rows, the sector values and whether the no-data overlay shows. The observation changes nothing in the collection (G11): no preference the page would write reaches it. The page and its assets stay in the image (R1960).
- **Meguri.** U226's Swift tests load `data` into the graph's record, with `backlog` and `separate` as its `futureDueShowBacklog` and `cardCountsSeparateInactive` and `history` as its history value (P1), run the presentation with the other controls, and compare with `page`. The core is not involved; the core's `data` is checked by the `stats` kind.
- **Comparison.** Before comparing, Meguri's page drops what P21 makes Meguri's own: the Card Counts `Notes` row. The memory-state footnote (P10, P11) and Answer Buttons' second label line (P14) are not members of `page`. Text compares exactly, booleans and `null` exactly; numbers inside `data` as the `stats` kind, and `value`, `x0`, `x1` and the sector values with the 1e-6 relative tolerance of `conformance/observations` O7.

## P23. Scenarios and tests

`conformance/scenarios/stats/presentation/`, for the oracle-maintainer with the kind of P22. As of 2026-11-10 all sixteen are in the tree, inactive; the fifteen below have no golden (P22). All in Europe/Paris, rollover 4, and every scenario that observes `stats_presentation` sets `fsrs` true in `setup.config`. The sixteenth, `preferences-read`, observes `graph_preferences` and `stats` only, sets `fsrs` itself and has its golden; it is listed with `stats/graphs-common` G15's scenarios (G7), not in the table below.

| Scenario | Pins | Requests |
|---|---|---|
| `future-due-ranges` | P6, P4.3, P4.4 | the P6 example cards, every range, Backlog on and off; then 45 cards due tomorrow (the {1: 45} example, have backlog false), 1 month, `futureDueShowBacklog` true and false; each observation first writes `futureDueShowBacklog` with `set_config` and passes the same `backlog` |
| `future-due-large-backlog` | P4.1 steps, P6 | overdue down to −500 days and due on 0, 5, 20 and 31; 1 month with Backlog on and off, 1 year and all with Backlog on |
| `reviews-ranges` | P8 domain steps 1 to 4, table | the P8 example rows, every range, Time off and on, `all` history; then rows on −1,000 and 0 only; then day 0 only |
| `reviews-aligned-start` | P8 step 2 | oldest row 403 days ago, all, `all` history |
| `reviews-table` | P8 table, Days studied percent | 30 of 31 days and 364 of 365, then every day, then 4,000 of 4,001; 1 month, 1 year and all, Time off and on, `all` history |
| `card-counts-table` | P9 | one card per state, separating on and off |
| `intervals-quantiles` | P10, P4.5 Q | the P10 example map, every range |
| `stability-quantiles` | P10 Stability | the same map as stabilities, every range |
| `difficulty-ranges` | P11, P4.5 E | the P11 50% example, every range |
| `retrievability-narrow` | P11 steps 3 and 4 | Difficulty and Retrievability at 100% on the maps {40: 5, 42: 1}, {11: 1, 87: 1}, {13: 1, 87: 1} and {40: 5} |
| `retention-tables` | P12 | rows in each period, each mode, `year` and `all` history |
| `hours-text` | P13 | answers at 09:30 and 23:10, and a winter 09:30 answer read in hour 10; 1 month and 1 year, and all with `all` history |
| `buttons-text` | P14 | each button in each group, a Manual row in none, older Young and Mature rows; 1 month and 1 year |
| `added-ranges` | P15 | the P15 example cards, every range |
| `empty-collection` | P16.5 | every graph at its default range, the Calendar for 2026 (its no-data state only) |

Meguri-only tests (no oracle): P2's writers and spans and P3's labels from their tables; P4.1 and P4.2 from their tables; range memory and history following (P5.2, P5.3); the Calendar's year bounds, drawn cells, tooltips, future dates and the AE58 presentation (P7); selection, tooltips and the Browser hand-off (P16, U226 UI tests); the summary, element tree and actions of a 70-bar chart (P18), with the run labels and P18.5's bin names for every graph; the P21 Meguri additions; every bar query parsed by the core's parser (P4.7), and the query writer on P4.7's example texts, which no recorded bin gives; the fractional Intervals edge of P10 (keys 1, 2, 3, 4, 50%: `"prop:ivl=4"`, which parses and finds interval 4); the Calendar's empty dates around one year ago and 31 December of a leap year (P7); re-binning the largest Reviews and Intervals records within 16 ms with no core call (R1290); the quantile rules computed from cumulative counts with no per-card array (R1293).

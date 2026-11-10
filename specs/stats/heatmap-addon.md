# Heatmap grid data and streak stats

Spec: stats/heatmap-addon
Requirements: R289, R290, R291, R292, R293, R294, R295, R297, R298, R299, R300, R302, R303, R1280, R1284, R1302, R1303, R1304, R1305, R1309, R1310, R1311, R1312, R1313, R1314, R1317, R1318, R1320, R1322, R1323, R1326, R1327, R1328, R1329, R1330, R1331, R1332, R1336, R1340, R1341, R1343, R1346, R1441, R1447
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/heatmap/, conformance/scenarios/stats/streaks/

The data behind the Home and deck-page heatmaps and their four streak stats: what each day of the grid holds, which days a period covers, how far navigation goes, the colour level of each cell and each streak value, and the search a day opens in the Browser. The rules are those of the Review Heatmap add-on 1.0.1 (R1447), except the deviations of A14. The core computes everything here and returns dates and numbers. Labels, long dates, tooltip text and colours are the UI's (U227, U228). Days, rows and settings come from `stats/day-buckets`, `stats/counting-rules` and `stats/heatmap-config`; this file does not restate them.

No add-on source is used for this spec. Its content comes from the plan's requirements and from the U95 specs, whose rules the add-on oracle confirmed. Where the requirements leave a detail open, A15 decides it and names the scenario whose golden settles it. A golden that disagrees wins, and the section is corrected.

## A1. Terms

- **T**: the heatmap's today (`stats/day-buckets` B4.2).
- **Heatmap day** of a row: `stats/day-buckets` B4.1. A row whose heatmap day is after T counts on T (B4.3).
- **Counted row**: a row that `stats/counting-rules` N2 keeps and that belongs to the scope (N3 for Home, N4 for a deck).
- **Scope**: Home, or Deck(id): that deck and its descendants by home deck (N4, R289, R1284, R1346).
- **Settings**: the members of the `heatmap` key after `stats/heatmap-config` K3, read when the query runs (K3 rule 6).
- **Anki's today**: the scheduler's day number (`days_elapsed`) from the day clock (`sched/day-clock`, U40) at the query's instant.
- Dates are of the proleptic Gregorian calendar, written `YYYY-MM-DD` (B4.5). `d + k` is the date k calendar days after `d`; `d₂ − d₁` is the number of calendar days between two dates.

## A2. History

### A2.1 Per-day count

The history count of a date `d` is the number of counted rows whose heatmap day is `d` (rows after T on T). A date with no counted row has count 0. `heatmap_totals` observes these counts after A2.2's window (`specs/conformance/stats-ops.md` H1). Counts are rows, not cards (N1).

### A2.2 History window

`limhist` and `limdate` drop the oldest days (R1312). Each gives a first allowed date. A date `d` is in the window when `d ≥` every first allowed date that is set. With neither set, every date is in the window.

| Setting | Set when | First allowed date |
|---|---|---|
| `limhist` | it is greater than 0 | `T − (limhist − 1)`: the window holds the last `limhist` dates, T included |
| `limdate` | it is greater than 0 and its date `L` is not the creation date | `L` |

- `L` is the local date, in the device zone (`stats/day-buckets` B5), of the instant `limdate` epoch seconds (no rollover shift). `limdate` is a JSON number. A fraction is dropped toward negative infinity before the date is read.
- The creation date is the local date, in the device zone, of the collection's `crt` (epoch seconds). A `limdate` whose date is the creation date is ignored (R1312).
- `limhist` or `limdate` at 0 or below is not set.
- With both set, the later first allowed date applies (the more restrictive bound).
- A first allowed date after T leaves the window empty.
- Decided 2026-10-27 (data safety; the add-on writes no such values): the bounds are computed in signed 64-bit day numbers that saturate, never wrap or fail. A `limhist` whose first allowed date would be before 0001-01-01 bounds nothing, as if it were not set: K6.1 accepts up to 2,147,483,647, and another writer may store any integer that `stats/heatmap-config` K3 reads. A `limdate` whose instant is after 9999-12-31T23:59:59Z (above 253,402,300,799, however written: `1e300`, or a number beyond the binary64 range) gives a first allowed date after T, so the window is empty.

The **filtered history** of the scope is the set of dates in the window whose history count is at least 1, with their counts. Every other value in this file that depends on history (levels, streaks, bounds, totals) reads the filtered history and nothing else (R1331).

The **first history day** is the earliest date of the filtered history. When the filtered history is empty, the scope has **no history** (A11 `no_history`): R1309's "No reviews yet" replaces the heatmap and the streak stats, and the query returns no streak values.

### A2.3 Dates outside the window

A date before the window has no history count in the grid: it is drawn as an empty past day, even when rows exist on it.

## A3. Forecast

The forecast count of a date `T + k` (`k ≥ 0`) is the number of cards that (R293, R1310):

1. exist and have `queue` 2 (review) or 3 (interday learning). New, intraday learning (queue 1), preview, suspended and buried cards never count. A card in a filtered deck (`odid` not 0) counts only when it would count in its home deck: its `type` is 2 (review), or 1 or 3 (learning, relearning) with item 3's D at most 1,000,000,000 (a day number; a larger value is the epoch-seconds due of an intraday step). A filtered deck that does not reschedule puts every card it takes in queue 2 and keeps the home due in `odue` (a home due of 0 leaves `odue` 0 and stays in `due`), so this test leaves out its new cards (`type` 0, at any position, 0 included) and its intraday learning cards (R293). Decided 2026-10-27: membership is judged by `odid`, not `odue`, so a new card at position 0 in such a deck never counts;
2. belong to the scope: on Home, the card's current deck (`did`) is neither a deck listed in `limdecks` nor its descendant (N3.3; with `limdecks` empty, every card); on a deck page, the card's home deck (`odid` when not 0, else `did`) is the deck or a descendant (N4);
3. have a due offset `k = D − today`, where D is the card's due day as `specs/search/properties.md` P4.1 defines it for queues 2 and 3 (`odue` when it is not 0, else `due`) and today is Anki's today. So the resident card table (`stats/counting-rules` N7 item 3) holds each card's `type` and `odue` as well as its queue and `due`;
4. have `0 ≤ k ≤ E`.

The forecast end E, in days after T:

| `limfcst` | E |
|---|---|
| greater than 0 | the smaller of `limfcst` and 73,000 |
| 0 or below | the largest `k` among the cards that pass 1 to 3 with `k ≥ 0`, capped at 73,000; no forecast when there is no such card (overdue cards alone give none) |

- A card with `k < 0` (overdue) is not drawn (R1310). A card with `k > E` is not counted.
- E is computed over the cards that pass items 1 to 3 only: a card left out by item 1 or 2 never moves it.
- With `limfcst` greater than 0, E and the last forecast day exist even when no card is counted, so A6's last period can be after period 0 with an empty forecast. "No forecast" happens only with `limfcst` at 0 or below.
- The **last forecast day** is `T + E`. With no forecast, there is none.
- `limhist`, `limdate`, `limresched` and `limcdel` never change the forecast. Deleted cards have no due and never count.
- Each card adds 1. The forecast is a count of cards, the history a count of rows.

Examples (T = 2026-10-12, Anki's today 400):

| Cards | `limfcst` | Forecast |
|---|---|---|
| queue 2 due 400, 401, 401; queue 3 due 402; queue 1 due now + 1 h; queue 2 due 399 | 0 | 10-12: 1, 10-13: 2, 10-14: 1; E = 2 |
| queue 2 due 410, 430, 431 | 30 | 10-22: 1, 11-11: 1; E = 30, last forecast day 2026-11-11 |
| queue 2 due 405 and 1,000,000,400 | 0 | 10-17: 1; E = 73,000 |
| queue −1, −2, −3 or 0, any due | 0 | none |
| queue 2 due 398, 399 (overdue only) | 0 | none: no forecast, no last forecast day |
| in a filtered deck that does not reschedule, all queue 2: a review card (`type` 2, `odue` 403), a new card (`type` 0, `odue` 7, a position), a learning card (`type` 1, `odue` 1,791,000,000, an instant) | 0 | 10-15: 1; E = 3 |

On a collection's first day (Anki's today 0), a new card at position 0 that such a deck takes (`odid` set, `odue` 0, `due` 0, queue 2) does not count on T.

## A4. Day kinds and today's cell

Each date of the grid has one kind (R1304, R1305, R294):

| Date | Kind | Count | Level scale |
|---|---|---|---|
| before T | `past` | history count | history |
| T, history count ≥ 1 | `today` | history count | history |
| T, history count 0, forecast count ≥ 1 | `future` | forecast count | forecast |
| T, both 0 | `today` | 0 | history (level 0) |
| after T | `future` | forecast count | forecast |

So today's cell shows its reviews once there is one, and its forecast before that (AE54: 12 cards due today and no review give kind `future`, count 12; after one answer, kind `today`, count 1). Only today's cell has the outline (R1306, UI).

## A5. Periods

The query asks for one period by its offset `p` from the period that contains T (`p` = 0). The mode is the `mode` setting when the query runs (R1302, R1303, R297):

| Mode | Period `p` |
|---|---|
| `year` | the calendar year `year(T) + p`, January 1 to December 31 |
| `months` | 9 whole months: from the 1st of the month that is `9p − 4` months after T's month, to the last day of the month 8 months after that one |

The period that contains a date `d`: in `year` mode `year(d) − year(T)`; in `months` mode `floor((M(d) − M(T) + 4) / 9)`, where `M` counts months (12 × year + month).

| T | Mode | `p` | Period |
|---|---|---|---|
| 2026-10-12 | `year` | 0 / −1 | 2026-01-01 … 2026-12-31 / 2025-01-01 … 2025-12-31 |
| 2026-10-12 | `months` | 0 | 2026-06-01 … 2027-02-28 (October is the fifth month) |
| 2026-10-12 | `months` | −1 / 1 | 2025-09-01 … 2026-05-31 / 2027-03-01 … 2027-11-30 |
| 2026-01-15 | `months` | 0 | 2025-09-01 … 2026-05-31 |

The grid's layout (week columns, week start, month blocks, labels) is U227's (`stats/heatmap-presentation` M2 to M4). When the mode changes, an offset from the old mode means nothing: hosts ask for `p` = 0 again.

## A6. Navigation bounds

- The **first period** is the period that contains the first history day. With no history, it is period 0.
- The **last period** is the period that contains the last forecast day. With no forecast, it is period 0.
- For a requested `p`: `can_go_back` is `p > first period`, `can_go_forward` is `p < last period` (R1317, R1318, R298). A `p` outside the bounds is still answered, with its data (usually none).

Example, `year` mode, T in 2026, first history day 2024-12-03, last forecast day 2027-08-01: first period −2, last period 1. At `p` = −2, `can_go_back` is false; at `p` = −3, also false.

## A7. Colour levels

Every grid cell gets a level from 0 to 10 (R295, R1313, R1314). Level 0 is a count of 0. For a count `c ≥ 1`, with `a = max(20, daily average)` (A8.1, the scope's Daily average value; 20 with no history):

| `c` at most | Level |
|---|---|
| 0.125 a | 1 |
| 0.25 a | 2 |
| 0.5 a | 3 |
| 0.75 a | 4 |
| a | 5 |
| 1.25 a | 6 |
| 1.5 a | 7 |
| 2 a | 8 |
| 4 a | 9 |
| above 4 a | 10 |

- History cells (kinds `past` and `today`) and forecast cells (kind `future`) use the same `a` and the same table. The kind tells the UI which ramp to draw: the green levels for history, the grey levels for the forecast (R1315).
- `a` comes from the scope's whole filtered history, not from the shown period.
- The comparisons are exact. With `a` a whole number, they are the integer tests `8c ≤ a`, `4c ≤ a`, `2c ≤ a`, `4c ≤ 3a`, `c ≤ a`, `4c ≤ 5a`, `2c ≤ 3a`, `c ≤ 2a`, `c ≤ 4a`.

| `a` | Count → level (AE55) |
|---|---|
| 410 | 300 → 4 (≤ 307.5); 1,700 → 10 (> 1,640); 410 → 5; 411 → 6; 51 → 1; 52 → 2 |
| 20 (daily average 8) | 8 → 3 (≤ 10); 2 → 1; 3 → 2; 80 → 9; 81 → 10 |

## A8. Streak stats

Four values, in this order (R302, R1326), over the filtered history (R303, R1331). An **active day** is a date of the filtered history. With no history there are no values.

### A8.1 Values

| Stat | Value |
|---|---|
| Daily average | the sum of the filtered history's counts divided by the number of active days, rounded to a whole number (R1327) |
| Days learned | the number of active days divided by `T − first history day + 1`, times 100, rounded to a whole number; 100 when the first history day is T (R1328) |
| Longest streak | the length in days of the longest run of consecutive active dates (R1329) |
| Current streak | when the latest active day is T or `T − 1`, the length of the run of consecutive active dates that ends on it; otherwise 0 (R1330) |

- Rounding is to the nearest whole number. A value exactly halfway rounds to the even neighbour (A15 item 6).
- Days learned is at most 100, since every active day is in the span.
- The current streak does not break during today: with the last active day Y, the current streak keeps Y's run while T is Y or `Y + 1`, and is 0 from `T = Y + 2` on (AE5). Rows after T count on T, so they can make T active (B4.3, deviation `heatmap-future-rows-today`).

Example (AE51): one counted row on every date from 2025-10-13 to 2026-10-11, rollover 4.

| Now | T | Daily average | Days learned | Longest | Current |
|---|---|---|---|---|---|
| 2026-10-12 21:00 | 2026-10-12 | 1 | 100 (364 of 365 = 99.73 %) | 364 | 364 |
| 2026-10-13 04:00 | 2026-10-13 | 1 | 99 (364 of 366 = 99.45 %) | 364 | 0 |

### A8.2 Tint levels

Each value gets a level from 0 (neutral) to 10, drawn with the history ramp (R303, R1332):

| Stat | Level |
|---|---|
| Daily average | A7's table applied to the value as a count, with `a = max(20, value)`: 5 for a value of 20 or more; for less, 1 to 5 (value 8 → 3) |
| Days learned | 0 → 0; 1–25 → 1; 26–50 → 2; 51–60 → 3; 61–70 → 4; 71–80 → 5; 81–85 → 6; 86–90 → 7; 91–95 → 8; 96–100 → 9 |
| Longest and current streak | 0 → 0; 1–14 → 2; 15–30 → 4; 31–90 → 6; 91–180 → 9; 181 and more → 10 |

The levels read the rounded values. In the AE51 example the streak of 364 has level 10, the current streak of 0 level 0, and Days learned 100 or 99 level 9.

## A9. Browse searches

A grid entry with data carries the search the Browser opens for it (R300, R1322). An entry without data carries none, and clicking it only moves the selection back to today (UI).

| Entry | Search term |
|---|---|
| kind `past` or `today`, count ≥ 1 | `prop:rated=-N` with `N = T − d` (`prop:rated=0` for T) |
| kind `future`, count ≥ 1 | `prop:due=N` with `N = d − T` (`prop:due=0` for T) |

- On a deck page, the search is that term joined with AND to the deck term for the deck's full name: U88's search builder (`specs/search/join-and-replace.md` J3) writes both, as a group of the "deck by name" item and either the parsed-text item `prop:rated=-N` (J3 has no `prop:rated` item) or the "due on day i" item `prop:due=` N, in canonical form (`specs/search/canonical-form.md`, deck name escaping included, R1078). For deck `A` and 2026-10-01 with T = 2026-10-12: `deck:A prop:rated=-11`; for a deck named `A B`: `"deck:A B" prop:rated=-11`. The text is the builder's, so R1322's `deck:"<deck>"` stands for that term, quoted only when F4 asks for it. On Home there is no deck term: `limdecks` is not part of the search.
- The Browser lists the cards that Anki's `prop:rated`, `prop:due` and `deck:` match, which need not be the cards the cell counts (R1323):
  - N counts calendar days between heatmap days, while `prop:rated` and `prop:due` count Anki's days (`specs/search/history.md` H8, `specs/search/properties.md` P4); the two can name different spans near a rollover after an offset change (`stats/day-buckets` B4.4);
  - the history counts rows, the Browser lists cards, and `prop:rated` finds only answers (`ease` > 0), while with `limresched` false the history also counts rows without a rating;
  - `prop:due=N` also finds intraday learning and preview cards (queues 1 and 4, P4), and queue-2 cards of a filtered deck that does not reschedule whose `odue` is a new card's position equal to today + N; A3 leaves both out;
  - on Home, both searches also find cards in `limdecks` decks;
  - `deck:` also matches the current deck, so a deck page also lists cards in a filtered deck under it whose home deck is elsewhere.

  So the Browser can list more cards than the count or fewer, and other ones. No notice is shown.
- Hosts pass the search on unchanged (U228). They compute no offset and no deck term.

## A10. Accessibility totals

For the summary of R1441, the query also returns `totals`, `{years, months, current_streak}`:

- `years`: `[{year, count}, ...]`, one entry for each calendar year that has a date in the period, in ascending order: the sum of the filtered history's counts in that year (the whole year, also in `months` mode), 0 allowed;
- `months`: `[{year, month, count}, ...]`, one entry for each month of the period, in period order, `month` 1 to 12: the sum of the filtered history's counts in that month, 0 allowed;
- `current_streak`: the current streak (A8.1); absent with no history, like `streaks`.

The UI words them ("N reviews in 2026, current streak N days").

## A11. Query

A query takes a scope and a period offset `p`. It reads the settings, the zone, `R`, Anki's today and now when it runs. It returns:

| Field | Value |
|---|---|
| `mode` | the mode used, `year` or `months` (A5) |
| `period_start`, `period_end` | the period's first and last dates |
| `today` | T |
| `days` | one entry per date of the period that has a count of at least 1, plus T when T is in the period, in date order: `date`, `kind`, `count`, `level` (A4, A7) and, when the count is at least 1, `search` (A9) |
| `first_period`, `last_period`, `can_go_back`, `can_go_forward` | A6 |
| `first_history_day`, `last_forecast_day` | A2.2, A3; absent when there is none |
| `no_history` | true when the filtered history is empty (A2.2) |
| `streaks` | absent with no history; otherwise `{daily_average, days_learned, longest_streak, current_streak}` (H11's names), each `{value, level}`: A8.1's value and A8.2's level |
| `totals` | A10 |

- A date of the period that has no entry is empty: `past` before T, `future` after T, count 0, level 0.
- A deck id that names no deck is an error (`unknown_deck`). A deck renamed or moved since the last query is read as it is now (N3).
- The query reads the core's resident values (`stats/counting-rules` N7) and the card table there. It never scans the review log (R304) and costs at most 5 ms at 150,000 cards and 1.2 million rows (R1290).

## A12. Read only

The query and everything it computes never write the collection (`stats/counting-rules` N8, R1295). An absent `heatmap` key stays absent (`stats/heatmap-config` K3 rule 5). Only the writer of `stats/heatmap-config` K6 writes the key. No setting here and no value of this file affects the Stats screen (R1343), and the Stats screen's code does not read this module.

## A13. Freshness

Every value is computed for the instant of the query. A change of T (rollover, zone change), of Anki's today, of the settings or of any row or card that A2 and A3 read shows in the next query (`stats/counting-rules` N7, `stats/day-buckets` B6). No result computed for another T or another state is returned.

## A14. Where Meguri differs from the add-on

- **Rows after today.** Counted on T (`stats/day-buckets` B4.3, R1280): deviation `heatmap-future-rows-today`. It reaches the history count of T, today's kind, the streaks and the levels. The add-on keeps such a row on its own date after T. The row is not in today's cell, and when it is the latest active day the add-on's current streak is 0 (golden of `streaks/future-row`). When every active day of the filtered history is after T, the add-on has no report: it fails while computing Days learned (`specs/conformance/stats-ops.md` H11, Errors). Meguri's first history day is then T, so Days learned is 100 (A8.1). The scenario therefore has a row on T − 1 too; the case with only rows after T is a Meguri unit test (A16).
- **Drawn day.** Every count stays on its B4.1 date (`stats/day-buckets` B4.6, R1270): deviation `heatmap-drawn-day-shift`. Meguri's streaks and totals read the same dates as its counts.
- Both entries are added to `conformance/deviations.yaml` with the goldens of the scenarios that show them (`specs/conformance/stats-ops.md` H4). Until then they are held, and this file only names them.
- **Deck pages by home deck** (R1284, N4), also for the forecast (A3 item 2): entry `heatmap-deck-home-deck`, added with the goldens of `stats/heatmap-totals/filtered-deck-card` and `stats/heatmap/forecast-filtered` (H4). The golden of `forecast-filtered-day0` shows no deck difference.
- **Forecast of cards in filtered decks** by `odue` (A15 item 10): the entry that item adds, with the goldens of `stats/heatmap/forecast-filtered` and `forecast-filtered-day0` (H4).
- **Clicked day.** For a history day the add-on opens a range search on review ids, which its own Browser hook resolves only when the range is the whole search. On a deck page it puts the current-deck term before the range, so the hook does not act, the range is read as a search on a field and nothing is found. For a forecast day it opens `prop:due=N` (with the current-deck term on deck pages). Meguri opens A9's searches, which on deck pages join the deck's name term (R1322). This is UI behaviour, outside the oracle.
- **No gestures, themes or legend.** The mode is a setting (R1325), `colors` is ignored (R1315). These are UI rules (U227, U229).

## A15. Points the requirements leave open

Decided 2026-10-27, in this order of priority: data safety, the plan's requirements, the add-on's behaviour as far as the goldens show it, simplicity. Each item names the scenario of A16 that settles it. A golden that disagrees with an item wins: the item is corrected, or a deviation is added when a requirement asks for Meguri's rule.

1. **`limdate`'s unit** (`stats/heatmap-config` K2 leaves it to this file): epoch seconds, read as the local date of that instant with no rollover shift (A2.2). This matches R1312's comparison with the collection's creation day. The writer stores the local midnight of the writing device's zone (`stats/heatmap-config` K6.1). A device in another zone reads that instant as its own local date, which can be the day before or after: its window gains or loses a day, and its creation-date test can differ. Accepted: nothing is written, the shift is at most one day, and another instant (local noon) would move the cut-off for an add-on that reads the value as an instant. If the `limdate-unit` golden shows the add-on reads it as a date, K6.1 moves to local noon. Scenarios `heatmap/limdate-unit`, `heatmap/limdate-creation`.
2. **`limhist`'s edge**: the last `limhist` dates, T included. Scenario `heatmap/limhist-edge`.
3. **The window applies to whole heatmap days**, not to row instants. A row between local midnight and the rollover hour belongs to the previous date for the window too. Scenario `heatmap/limdate-unit`.
4. **`limdate` in the future** empties the window instead of being ignored. Negative values of `limhist`, `limfcst` and `limdate` are not set (the add-on writes none; `stats/heatmap-config` K3 rule 4). Scenario `heatmap/limits-odd`.
5. **The first history day** for Days learned and the first period is the earliest active date in the window, not the window's first date. Scenario `streaks/limhist-span`.
6. **Rounding at .5** for Daily average and Days learned: to even. Scenario `streaks/rounding-half`.
7. **`a`** is the rounded Daily average of the scope's whole filtered history, and the forecast uses the same `a`. Meguri unit tests `levels-history`, `levels-forecast` (A16).
8. **Threshold edges** are "at most" for every table of A7 and A8.2, as R1313 says for the cells. Meguri unit tests `levels-history`, `tints` (A16).
9. **Today's forecast** counts only cards due today (`k` = 0), not overdue ones (R1310). Scenario `heatmap/today-cell`.
10. **The forecast's due day** of a card in a filtered deck is P4.1's (`odue`), so the Browser's `prop:due=N` finds every card the cell counts, and maybe others (A9). Only cards that would count in their home deck count (A3 item 1): a filtered deck that does not reschedule puts new and intraday learning cards in queue 2, and they stay out, as R293 asks. If the add-on reads `due` there, counting the home due of review and interday learning cards is a deviation under R1284's reasoning; the filtered-deck scenario adds its entry. Leaving out the other cards matches an add-on that reads `due`, since their `due` is then a negative position (overdue). Scenarios `heatmap/forecast-filtered`, `heatmap/forecast-filtered-day0`. Their goldens of 2026-11-10 show that the add-on reads `due`: in `forecast-filtered` no filtered card counts, on Home or on any deck page (forecast `{}` throughout); in `forecast-filtered-day0` Home also counts the new card whose `due` 0 is today (forecast and today's cell 2, where Meguri gives 1), while `H`'s page counts only the review card, as Meguri does. These differences take the entry this item adds (`specs/conformance/stats-ops.md` H4 lists the members).
11. **The forecast end** with `limfcst` set is `T + limfcst` even when no card is due that late, and without it the 73,000-day cap ends the forecast when a corrupt due lies beyond it (R1311). Scenarios `heatmap/forecast-limfcst`, `heatmap/forecast-cap`.
12. **No history** is judged on the filtered history (after the window), and the query still returns the period's forecast; the hosts show R1309's message instead of the grid. Scenario `heatmap/no-history-forecast`.
13. **Streaks across a drawn-day shift** read B4.1 dates in Meguri (A14). The add-on's do too: the 2026-11-10 golden of scenario `streaks/lord-howe-dst-start` has Meguri's streak values (current and longest streak 3, Days learned 75) beside a history keyed by drawn dates. Only that history falls under `heatmap-drawn-day-shift`.
14. **Levels and tints** may be computed only in the add-on's web view, which the oracle does not run. Then A7 and A8.2 are tested from their tables by Meguri's unit tests, as the Stats-screen text is (`stats/studied-today` T6.4), and the scenarios observe counts and streak values only.

## A16. Scenarios

As of 2026-11-10 every scenario below is in the tree and inactive. On 2026-11-10 each got a golden (`specs/conformance/stats-ops.md` H4). The goldens come from an observation that runs the add-on's whole report headless (history after its limits, forecast, today's cell, the four streak values), on the add-on image of `specs/conformance/stats-ops.md` H4, keyed by the date on which the add-on draws each count (H1). The kind is `heatmap_report` (`specs/conformance/stats-ops.md` H11); the add-on image's driver has it, its schema shape is in `conformance/schema/`, and `tools/mg` runs these scenarios on that image (H4). Each golden is recorded at the oracle pin of `conformance/oracle.json`; the goldens are then authoritative. Values below are Meguri's from this file. All run in Europe/Paris with rollover 4 unless a zone is named; T is 2026-10-12 unless a time is given.

`conformance/scenarios/stats/heatmap/`:

| Scenario | Pins | Setup and expected values |
|---|---|---|
| `forecast-queues` | A3 items 1, 3, 4, A4 | cards in queues 0, 1, 2, 3, −1, −2 and −3, due yesterday, today and tomorrow, plus a relearning card (type 3) in queue 3 due tomorrow and a preview card (queue 4) whose `due` reads as today: only queues 2 and 3 due today or later count, today 2 and tomorrow 3; today's cell `future`, 2 |
| `forecast-limfcst` | A3, A15 item 11 | `limfcst` 30, cards due at +10, +30, +31: 2 cards, last forecast day T + 30 |
| `forecast-cap` | A3, A15 item 11 | one card due at +5, one at +10⁹ days: forecast +5: 1, last forecast day T + 73,000 |
| `forecast-scope` | A3 item 2 | `limdecks` [Excl] with cards due in `Excl`, `Excl::Sub` and `Keep`: Home counts `Keep` only; deck page `Excl` counts `Excl` and `Excl::Sub` |
| `forecast-filtered` | A3 item 1, A15 item 10 | a review card moved into a filtered deck, `odue` +3: forecast +3 on its home deck's page and on Home; then a filtered deck that does not reschedule takes one review card due +5, one new card and one intraday learning card: the review card counts on +5, the other two count nowhere and E stays 5 |
| `forecast-filtered-day0` | A3 item 1 | on the collection's first day (Anki's today 0), a filtered deck that does not reschedule holds a new card of home deck `H` (`odid` set, `odue` 0), and a review card of `H` is due 0: only the review card counts, on T, on Home and on `H`'s page alike (membership is judged by the card, not by `odue`) |
| `forecast-overdue-only` | A3, A4 | only two review cards, both overdue: forecast `{}`, last forecast day `null`, today's cell `today`, 0 |
| `today-cell` (AE54) | A4, A15 item 9 | 12 cards due today, 3 overdue, no review: today `future`, 12; after one answer: `today`, 1 |
| `limhist-edge` | A2.2, A15 item 2 | `limhist` 90, rows on T − 89, T − 90, T − 91: only T − 89 is in the history |
| `limdate-unit` | A2.2, A15 items 1, 3 | `limdate` the epoch seconds of 2026-09-01 00:00 local; rows on 08-31 23:00, 09-01 02:00 (heatmap day 08-31) and 09-01 05:00: only the 05:00 row counts |
| `limdate-creation` | A2.2 | rows before and after the creation date; `limdate` equal to `crt`, then `crt` − 14,400 (the local midnight `stats/heatmap-config` K6.1 stores, still the creation date): all count both times; then `crt` + 86,400: the rows before that date drop |
| `limdate-fraction` | A2.2 (fraction rule), A15 item 6 | `limdate` 1,788,213,599.5 (2026-08-31 23:59:59.5 local) and then 1,788,213,600.5 (09-01 00:00:00.5): their dates are 08-31 and 09-01, so rows on heatmap days 08-30, 08-31 (two) and 09-01 give history {08-31: 2, 09-01: 1}, then {09-01: 1} |
| `limhist-and-limdate` | A2.2 | `limhist` 90 with a `limdate` 200 days back, then 30 days back: the later bound applies each time |
| `limits-odd` | A15 item 4 | `limdate` one day after T: no history; `limhist` −5 and `limfcst` −5: not set |
| `no-history-forecast` | A15 item 12 | rows only before a `limdate`, cards due: no history, forecast present |
| `today-dst-shift` | `stats/day-buckets` B4.2, B6, A1, A4 | Europe/Paris on 2026-03-29, a row at 04:15 +02:00 (after Anki's day starts, before T moves): at 04:30 T is 03-28 and the row is on 03-28, today's cell `today`, 1; at 05:00 (at(T + 1)) T is 03-29 and today's cell `today`, 0; studied today 1 row both times |

`conformance/scenarios/stats/streaks/`:

| Scenario | Pins | Setup and expected values |
|---|---|---|
| `ae51-current-streak` (AE51) | A8.1 | A8.1's example at 2026-10-12 21:00 and 2026-10-13 04:00 |
| `ae5-yesterday` (AE5) | A8.1 | last review yesterday: current streak kept at 03:59:59 next day, 0 at 04:00:00 |
| `single-day` | A8.1 | one active day, T: Days learned 100, streaks 1 |
| `gaps` | A8.1 | runs of 5, 2 and 3 days with gaps, the last ending T − 2: longest 5, current 0 |
| `rounding-half` | A15 item 6 | 5 rows over 2 days (2.5), 7 over 2 (3.5); 1 active day in 8 (12.5 %) |
| `limhist-span` | A15 item 5 | `limhist` 30, first row in the window on T − 10: Days learned over 11 days |
| `limdecks-home-only` | N3.3, R1340 | `limdecks` [A]: Home streaks without A's rows, deck page A unchanged |
| `future-row` | A8.1, A14, deviation `heatmap-future-rows-today` | a row on T − 1 and one dated after T (T + 1, noon): history {T − 1: 1, T: 1}, today's cell `today`, 1, longest and current streak 2, Days learned 100 in Meguri |
| `empty` | A2.2 | no rows: no history, no streak values |
| `lord-howe-dst-start` | A15 item 13 | Australia/Lord_Howe, active days around 2026-10-04 |
| `deleted-card` (AE6, deck part) | `stats/counting-rules` N3.1, N3.2, N4 | deck `A`, one card with rows on T − 1 and T, another on T − 3 and T − 2, then the second card's note is deleted: Home keeps 4 days (streaks 4), `A`'s page drops to T − 1 and T (streaks 2), and with `limcdel` true Home reads as `A` |

Meguri unit tests while H11 has no levels or tints (A15 item 14, `specs/conformance/stats-ops.md` H11); no scenario file exists for them:

| Test | Pins | Inputs and expected values |
|---|---|---|
| `levels-history` (AE55) | A7, A15 items 7, 8 | a daily average of 410 with days of 300, 307, 308, 1,640, 1,641 and 1,700 rows: levels 4, 4, 5, 9, 10, 10 |
| `levels-forecast` | A7 | 8 cards due on one day with a daily average of 8: level 3 |
| `tints` | A8.2, A15 item 8 | streaks of 14, 15, 30, 31, 180, 181; Days learned 25, 26, 99, 100 |

A5 (periods), A6 (bounds), A9 (searches) and A10 (totals) have no scenario: they are tested by Meguri's unit tests (U96 `heatmap_grid`), one per row of their tables and examples, plus months mode across a year end and the bounds with no history and with `limfcst` set and no card counted (A3). U96's unit tests also cover A2.2's range rule (`limhist` 2,147,483,647 and 4,294,967,296: no limit; `limdate` 253,402,300,800 and `1e300`: an empty window) and the A10 and A11 shapes with and without history; the add-on writes no such limits, so they have no oracle scenario.

A history whose only row is dated after T is a U96 unit test too, since the add-on fails there (A14): history {T: 1}, today's cell `today`, 1, Daily average 1, Days learned 100, longest and current streak 1.

Writer tests for `stats/heatmap-config` K6 are Meguri's own, except the oracle scenario `heatmap-config/writer-output`, which observes `heatmap_report` (K6.4).

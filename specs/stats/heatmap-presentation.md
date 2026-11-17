# Heatmap grid presentation

Spec: stats/heatmap-presentation
Requirements: R149, R183, R289, R294, R296, R297, R298, R299, R300, R301, R307, R308, R309, R310, R1301, R1302, R1303, R1304, R1305, R1306, R1307, R1308, R1313, R1314, R1315, R1316, R1317, R1318, R1319, R1320, R1321, R1322, R1323, R1324, R1325, R1346, R1379, R1380, R1381, R1382, R1383, R1439, R1441, R1443, R1444, R1445, R1839, R1997
Anki: 26.09.3 (the Stats Calendar's grid, M6, M8.3); the Review Heatmap add-on 1.0.1 for the Home and deck-page grid (R1447)
Scenarios: none. Every rule here is drawing, text or interaction. The oracle runs neither the add-on's web view nor Anki's graphs page drawing (`stats/heatmap-addon` A15 item 14, `specs/conformance/stats-ops.md` H11, `stats/graph-presentation` P22), so nothing here has a golden. M20 lists the Meguri tests built from this file's tables.

The one native heatmap grid (U227): how the cells of a period are laid out, labelled, coloured and outlined, the tooltip and callout text, the Back, Today and Forward controls, pointer, touch and keyboard interaction, and what VoiceOver reads. One component serves Home and deck pages (R289, R1346), the Stats Calendar (R1379, U230) and the widget's grids: the trailing grids of the small and medium families (R307, R308) and the year grid of the large and extra-large families (R309, R310; `stats/widget-families` F2, F3; U165).

The component computes no count, level, search, bound or streak. Its data comes from `stats/heatmap-addon` A11 (Home, deck pages), from `stats/graphs-revlog` V3 with `stats/graph-presentation` P7 (Calendar) and from `stats/widget-snapshot` W3 (widget). Days are civil dates; the component reads no clock, no time zone and no collection. Its layout, palette and text rules are pure functions of M1's input, tested as such (M20).

The Home and deck-page grid draws as the add-on draws, except where M18 names a difference. The Calendar keeps the rules of `stats/graph-presentation` P7. Colours are Meguri's own, never the add-on's or Anki's (R1315, R1382, R1960).

## M1. Inputs

The host passes a series; the component draws it as it is.

| Field | Value |
|---|---|
| `mode` | `year`, `months`, `trailing(N)` or `calendar` |
| `today` | T, a date. Home and deck pages: A11's `today`. Calendar: the record's `today_date` (P7). Widget: the entry's `today` (W4) |
| `weekStart` | 0 (Sunday), 1 (Monday), 5 (Friday) or 6 (Saturday): the synced `firstDayOfWeek` after `specs/editor/config-keys.md` K5 (R149, R1381) |
| `period` | the first and last dates shown: A11's `period_start` and `period_end` (`year`, `months`), January 1 to December 31 of the shown year (`calendar`), M5's weeks (`trailing`), or January 1 to December 31 of the year of the entry's `today` (the widget's `year`) |
| `days` | `year`, `months`: A11's `days`, date → `{kind, count, level, search}`. `calendar`: exactly the dates P7's drawn-cell rule draws, date → `{count, query}`, with no `query` where P7 gives none (`stats/browse-queries` Q4.2); U230's Calendar card computes the set, the counts and the queries by P7 (R1379 to R1384) and passes them, and the component computes none of them. `trailing`: date → `{kind, count, level}`, built by the widget host (U165) from W3 as below; the widget's `year`: the same mapping as `trailing`, for every date of its year (`stats/widget-families` F1) |
| `colouring` | `discrete` (levels 0 to 10, M8.1) for `year`, `months` and `trailing`; `continuous` with the record's `max_count` for `calendar` (M8.3) |
| `navigation` | `year`, `months`: A11's `first_period`, `last_period`, `can_go_back`, `can_go_forward` and the shown offset `p`. `calendar`: P7's year bounds as `can_go_back` and `can_go_forward`. `trailing` and the widget's `year`: none |
| `totals` | A10's `totals` (`year` and `months` on Home and deck pages only; none in the widget), for M15.2 and M15.3 |

- A date of the period with no entry is empty: in `year`, `months` and `trailing` it is a `past` cell before T and a `future` cell after T, with count 0 and level 0 (A11). In `calendar` a date with no entry is not drawn (M6); the component applies none of P7's rules itself.
- `trailing` from a W3 snapshot: a date before T takes kind `past` and the count and level of `history.counts` and `history.levels` at its index from `history.start` (level 0 before `history.start`); T takes `todayCell`'s kind, count and level; a date after T takes kind `future`, count 0 and `forecast.levels` at index `date − today` (level 0 past the array's end). For a W4 entry, T is the entry's `today` and its cell the entry's `todayCell`, the other dates as W4's last bullet says. Widget cells show no count (M5), so the forecast's missing counts are never read.
- The widget's grids are static. `trailing` and the widget's `year` have no controls (M11), no selection, hover, tooltip or callout (M9, M10.1, M12, M13), no period requests (M11.5) and one VoiceOver element (M15.1); wherever this file names `trailing` for one of these rules, the widget's `year` follows it too (`stats/widget-families` F3).
- Outputs: the day callback (M14) and the period requests (M11.5).
- `mode` is the `heatmap` key's `mode` (A5) on Home and deck pages, fixed `calendar` on the Stats screen, fixed `trailing(8)` or `trailing(20)` in the small and medium widgets, and fixed `year` in the large and extra-large widgets, for the year of the entry's `today` (`stats/widget-families` F3). The component offers no way to change it (R1325, M11.1).
- Dates are of the proleptic Gregorian calendar. `weekday(d)` is 0 for Sunday to 6 for Saturday. `d + k` and `d₂ − d₁` are calendar-day arithmetic.

## M2. Weekday rows

Every mode has 7 rows, top to bottom from the week start. The row of a date `d` is

`row(d) = ((weekday(d) − weekStart) mod 7) + 1`, the mod giving 0 to 6.

| Date | Weekday | `weekStart` 0 | 1 | 5 | 6 |
|---|---|---|---|---|---|
| 2026-01-01 | Thursday | 5 | 4 | 7 | 6 |
| 2026-10-12 | Monday | 2 | 1 | 4 | 3 |
| 2012-01-01 | Sunday | 1 | 7 | 3 | 2 |

The add-on always starts weeks on Monday; Meguri uses `weekStart` (R149, M18).

## M3. Year layout

Mode `year`: January 1 to December 31 of the period's year Y, in week columns (R1302, R297).

- `o = row(January 1) − 1`. The column of a date `d` is `floor((o + d − January 1) / 7) + 1`.
- The column count is `C = ceil((o + days in Y) / 7)`. It is 53, or 54 when Y is a leap year and `o` is 6 (January 1 on the week's last weekday). Decided 2026-10-27 (plan Open Questions): the count is computed per year, whatever R1302's "53" says. The add-on also sizes each year by its own weeks, except in years that start on its week's first day (M18).
- Column 1 has no cell above January 1's row, and column C none below December 31's. The slots there are not drawn: no cell, no empty fill, no hit target.
- Below the grid, centred on it: the year label (M7.3). No weekday labels and no month labels.

| Year | `weekStart` | C | January 1 | December 31 |
|---|---|---|---|---|
| 2026 | 1 | 53 | row 4, column 1 | Thursday, row 4, column 53 |
| 2026 | 0 | 53 | row 5, column 1 | row 5, column 53 |
| 2026 | 5 | 53 | row 7, column 1 | row 7, column 53 |
| 2026 | 6 | 53 | row 6, column 1 | row 6, column 53 |
| 2012 | 1 | 54 | Sunday, row 7, column 1 | Monday, row 1, column 54 |
| 2040 | 1 | 54 | Sunday, row 7, column 1 | Monday, row 1, column 54 |
| 2028 | 0 | 54 | Saturday, row 7, column 1 | Sunday, row 1, column 54 |
| 2000 | 0 | 54 | Saturday, row 7, column 1 | Sunday, row 1, column 54 |
| 2032 | 5 | 54 | Thursday, row 7, column 1 | Friday, row 1, column 54 |
| 2016 | 6 | 54 | Friday, row 7, column 1 | Saturday, row 1, column 54 |
| 2012 | 0 | 53 | row 1, column 1 | Monday, row 2, column 53 |

2026-10-12 is in column 42 for `weekStart` 0 and 1.

## M4. Nine-month layout

Mode `months`: the 9 months of A5's period, each a block (R1303, R297).

- Each month m is laid out like M3 on its own days: `o_m = row(1st of m) − 1`, the column of `d` within the block is `floor((o_m + d − 1st of m) / 7) + 1`, and the block has `ceil((o_m + days in m) / 7)` columns, 4 to 6.
- Blocks sit side by side in period order, separated by the block gap (M7.2). A week that crosses a month end is split: its days of each month sit in that month's block, so every date has one cell.
- Below each block, centred on it: the month label (M7.3), for example `Oct '26`.
- Before the first block: the weekday label column, one label per row, starting with `weekStart` (M7.3). The labels appear once, not per block.

T = 2026-10-12, `p` = 0: blocks 2026-06 to 2027-02; October is the fifth block, and 2026-10-12 is in its column 3.

| `weekStart` | Columns per block, 2026-06 to 2027-02 | Total |
|---|---|---|
| 1 | 5, 5, 6, 5, 5, 6, 5, 5, 4 | 46 |
| 0 | 5, 5, 6, 5, 5, 5, 5, 6, 5 | 47 |

## M5. Trailing-weeks layout

Mode `trailing(N)`, N 8 or 20, for the widget's small and medium families (R307, R308; U165).

- The last column holds the week that contains T: from `T − (row(T) − 1)` to 6 days later. The first column holds the week `N − 1` weeks before it. Each column has 7 rows by M2.
- Dates after T in the last column are drawn as `future` cells with the forecast levels the series carries (R1304; `stats/widget-snapshot` W3 `forecast`), empty when it carries none.
- No labels of any kind. The widget's own text (streak, today's count) is U165's.

| T | `weekStart` | N | First column starts | Last column |
|---|---|---|---|---|
| 2026-10-12 | 1 | 8 | 2026-08-24 | 2026-10-12 to 2026-10-18 |
| 2026-10-12 | 0 | 8 | 2026-08-23 | 2026-10-11 to 2026-10-17 |
| 2026-10-12 | 1 | 20 | 2026-06-01 | 2026-10-12 to 2026-10-18 |

## M6. Calendar layout

Mode `calendar`, the Stats screen's Calendar card (R1379; U230). This component (U227) draws the Calendar's grid, its colours (M8.3) and its `AXChartDescriptor` (M15.6); U230 hosts the card and passes the series (M1); `stats/graph-presentation` P7 gives the years, drawn dates, tooltip and query that U230 computes.

- The shown year's cells are placed by M3, 54 columns included.
- Only the dates the series holds are drawn (M1: the host passes P7's drawn dates). Every other date of the year has no cell, like the slots of M3. So no date after today has a cell (R1380).
- Before the first column: the weekday label column of M4, starting with `weekStart` (R1381). The labels are text only: the week start is changed from the card's "Week starts on" menu (U230), not by clicking a label as in Anki (R1381).
- No year label below: the year is shown between the Back and Forward controls (M11.1).
- Colours by M8.3. No today outline (Anki's calendar has none); the selection and hover outlines of M9 apply.

## M7. Geometry and sizing

### M7.1 Cells

- Cells are squares with continuous rounded corners, radius 20% of the side (R1307). The add-on draws sharp squares.
- Gap `g` between neighbouring cells, side `a`, pitch `p = a + g`.

### M7.2 Pitch

The grid fits the width W it is given, never scrolls sideways, and is centred in it when it is narrower (R1307):

1. Count the width in pitch units, `k`: `year`: C (M3); `months`: the total block columns plus 2.5; `calendar`: C; `trailing(N)`: N.
2. `p = min(14, (W − L) / k)` for `year`, `months` and `calendar`, where L is the label column's width (M7.3; 0 for `year`). For `trailing`, `p = min(W / N, H / 7)` for the widget's height H, with no cap. In the widget, `year` also takes at most H ÷ 7 for the height H the widget gives the grid without its label: `p = min(14, W ÷ C, H ÷ 7)` (`stats/widget-families` F3).
3. Round `p` down to the display's pixel grid (1 ÷ display scale).
4. `g = max(one pixel, p ÷ 6 rounded down to the pixel grid)`, `a = p − g`.
5. Block gap in `months`: between the last column of one block and the first column of the next the space is 3g in total, in place of the gap g between columns. The grid is then `(total columns) × p − g + 8 × 2g + L` wide, which is at most `k × p + L` since g ≤ p ÷ 6. At W 370 and scale 3 with 46 columns and L 11: 46 × 22⁄3 + 15 × 1 + 11 = 363⅓ pt.
6. When M7.3 shrinks the weekday labels or leaves some out, L is taken again from the labels drawn and steps 2 to 5 run once more; the labels keep the form and size already chosen.

So at full size the pitch is 14 pt: a 12 pt cell and a 2 pt gap on a 2× display. Mac and iPad usually draw 10 to 12 pt cells (R1307): a 2× width of 640 pt in a 54-column year gives `p` = 11.5, `g` = 1.5, `a` = 10. On iPhone and in narrow windows the cells shrink to fit. At display scale 3:

| W | `year`, 53 or 54 columns | `months`, 46 columns, L = 11 | `calendar`, 54 columns, L = 11 |
|---|---|---|---|
| 370 | `p` 20⁄3, `g` 1, `a` 17⁄3 | `p` 22⁄3, `g` 1, `a` 19⁄3 | `p` 19⁄3, `g` 1, `a` 16⁄3 |
| 330 | `p` 6, `g` 1, `a` 5 | `p` 19⁄3, `g` 1, `a` 16⁄3 | `p` 17⁄3, `g` 2⁄3, `a` 5 |

W is the width the host gives the grid, inside its own margins; the hosts' layouts (U228, U230, U165) fix it, not this file. Since g ≤ p ÷ 6, a ≥ 5 pt whenever p ≥ 6, that is whenever W ≥ 6k + L: 318 pt for a 53-column year, 324 for 54 columns, 6 × (columns + 2.5) + L in `months` (291 + L for 46 columns) and 324 + L for the Calendar. U227's plan test "cell ≥ 5 pt" on iPhone 17 Pro holds for any host width meeting this bound. The rule has no floor: a narrower W gives smaller cells.

The grid's height is `7p − g` plus the labels below it. Layout re-runs on every width or week-start change, keeping the shown period, the selection and an open callout (R1308; M13.1). The foldable iPhone's fold and unfold are width changes (R1308).

### M7.3 Labels

- **Year label** (`year`): the year as digits without grouping (`2026`), centred under the grid.
- **Month labels** (`months`): the locale's short stand-alone month name, a space, an apostrophe and the year's last two digits: format pattern `LLL ''yy`, `Oct '26` in English. Centred under the block. Every block uses the same form and size: the first of these that fits the width of every block (its columns × p − g): the label at its text size; the label shrunk, down to half its size; the narrow stand-alone month symbol (pattern `LLLLL`, `O` in English) at its text size, then shrunk down to half its size; no month labels.
- **Weekday labels** (`months`, `calendar`): the locale's narrowest stand-alone weekday symbol for each row (`M T W T F S S` for Monday weeks in English), centred vertically on its row. Every label uses the same size: the first of these that fits: a label on every row, its line height at most p, at its text size and then shrunk down to half its size; labels on rows 1, 3, 5 and 7 only, line height at most 2p (rows 1 and 7 may reach half a pitch beyond the grid), the same sizes; no weekday labels, with L = 0. L is the widest drawn label plus 4 pt. At display scale 3 and W 330 (`months`, `p` 19⁄3) only rows 1, 3, 5 and 7 are labelled at the default text size: a caption line is taller than 19⁄3 pt even at half size.
- Labels use the caption text style in the secondary label colour and follow Dynamic Type (R1444) up to the sizes these rules allow. Cells do not grow with Dynamic Type; they follow M7.2. Every day's weekday and month are also in its text (M10, M15.4), so a label left out loses nothing.
- The grid keeps left-to-right order in right-to-left layouts, as the add-on and Anki's calendar do: time runs left to right.

### M7.4 Hit-testing

- A point hits a drawn cell when it lies in the cell's square widened by g ÷ 2 on each side, so gaps between cells of one block are not dead zones. Points in a block gap, a label or an undrawn slot hit nothing.
- Hit-testing is geometry over M3 to M6, never per-cell views (M16).
- Touch takes the drawn cell whose centre is nearest the finger, within one pitch (M13).

## M8. Colours

### M8.1 Discrete mapping

| Cell | Fill |
|---|---|
| kind `past` or `today`, level L ≥ 1 | history step L |
| kind `future`, level L ≥ 1 | forecast step L |
| level 0, or no entry | empty fill |

Levels come from the core (A7, R1313, R1314; W3); the component holds no thresholds. The synced `colors` value is never read (R1315, R296). No legend is drawn (R1316).

### M8.2 Ramps

The palette is Meguri's (HeatmapKit, the U9 lint allow-list), with tokens `heatmap.history.1` to `.10`, `heatmap.forecast.1` to `.10` and `heatmap.empty`. Its values are Meguri's own choices under these rules; no add-on or Anki colour is used.

- **History**: ten greens, a ramp from yellow-green to deep green in the manner of the add-on's default theme (R1315). In the light appearance step 1 is the lightest and step 10 the darkest. In the dark appearance the order is reversed: step 1 the darkest, step 10 the lightest, so busy days stay bright on dark backgrounds, as the add-on's night mode does.
- **Forecast**: ten neutral greys. In the light appearance a higher step is darker (more due cards, darker grey; R1314); in the dark appearance a higher step is lighter, as in the add-on's night mode.
- **Empty**: the system's secondary fill (`heatmap.empty`): `UIColor.secondarySystemFill` on iPhone and iPad, `NSColor.secondarySystemFill` on the Mac, so it follows the platform's light, dark and contrast settings (R1315).
- Token values are given in sRGB; the ramp steps are opaque.
- Measurable rules, checked by the snapshot and lint gate (U9, KTD20), in each appearance and contrast setting. Colours are measured as drawn: each fill composited over the background of the host it is drawn in, then converted from sRGB to CIELAB with the D65 white point (CIELCh is CIELAB given as lightness, chroma and hue angle). The backgrounds are those of the hosts' snapshots (M20): the Home and deck-page List row (`secondarySystemGroupedBackground` on iPhone and iPad), the Stats group box (U230) and the widget container (U165), on each platform. The lint, which draws nothing, uses the iPhone List row background for the same rules.
  1. CIELAB lightness L* changes in one direction along each ramp, by at least 4 between adjacent steps (R1443: levels are ordered by lightness).
  2. Step 1 of each ramp differs from the empty fill by at least 4 in L*.
  3. History steps have CIELCh chroma at least 20 and a hue between 100° and 150°; forecast steps have chroma at most 4.
- The ramps carry the levels of `stats/heatmap-addon` A8.2's streak tints as well; using them for text, and its contrast, is U228's in the activity section and U165's in the widget (`stats/widget-families` F4).

### M8.3 Continuous mapping (Calendar)

R1382: a cell with count c ≥ 1 takes the position `u = 1 + 9 × √(c ÷ max_count)` on the history ramp, from 1 (lightest in light appearance) to 10. A fractional u blends steps ⌊u⌋ and ⌈u⌉ in OKLab by its fractional part. A drawn cell with count 0 takes the empty fill. `max_count` is P7's (all years, dates after today included).

| `max_count` | c | u |
|---|---|---|
| 100 | 1 | 1.9 |
| 100 | 25 | 5.5 |
| 100 | 100 | 10 |
| 1 | 1 | 10 |

Anki uses a blue ramp capped at 20% to 100% of its intensity (80% at night). Meguri uses its one palette (R1382, M18).

### M8.4 Display settings

- **Increase Contrast** (R1443): every drawn cell gets a 1 pt border inside its edge in the system's separator colour (`UIColor.opaqueSeparator` on iPhone and iPad; `NSColor.separatorColor` on the Mac, which has no opaque one, composited over the cell's fill), and the empty fill becomes the system's primary fill (`systemFill`, the same name on both platforms). Outlines (M9) keep their colours and widths.
- **Smart Invert**: the grid is excluded from inversion, so its levels keep their order.
- **Widget rendering modes**: in WidgetKit's accented mode (the tinted and clear appearances) and vibrant mode (the Mac desktop with an app in front of it) the levels stay apart by opacity steps of one colour, not by hue; the values are U165's (R313, R1437; `stats/widget-families` F3).
- No visual resembles Activity rings: square cells only, no circular progress (R1997).

## M9. Outlines

| Outline | Drawn on | Stroke |
|---|---|---|
| Today | T's cell when T is in the period; not in `calendar` | 1.5 pt (1 pt when a < 8 pt), primary label colour, inside the cell's edge (R1306, R294) |
| Selection | the selected date's cell, when it is not T and is in the period | 1.5 pt (1 pt when a < 8 pt), accent colour, on a rounded square larger than the cell by g on each side (R1306) |
| Hover | the cell under the pointer (M12) | 1 pt, secondary label colour, inside the cell's edge |

- The selection starts at T and stays on its date until another cell is chosen (R1306). It survives paging, width changes and data refreshes. While the selection is on T it moves with T when T changes (a rollover, a zone change, a new widget entry); a selection on another date stays where it is. The selection lives as long as the host's view: when the screen is shown again it starts at T. With the selection on T only today's outline is drawn, as in the add-on, where a click on an empty day leaves today alone highlighted.
- Today's outline wins over the hover outline on T's cell. A selected cell that is hovered keeps its selection outline and adds the hover outline inside.
- In `calendar` the selection is the cell whose tooltip or callout shows (P16), and it clears with it. Anki's calendar draws no hover or selection outline (M18).
- `trailing` and the widget's `year` draw today's outline only.
- The three outlines differ in place and width as well as colour, so none is told by colour alone (R1443).

## M10. Day text

### M10.1 Tooltip and callout text

`year`, `months` and `trailing` cells (R1320, R299), by the cell's kind and count (A4):

| Kind | Count | Text |
|---|---|---|
| `past`, `today` | N ≥ 1 | **N** card(s) **reviewed** on <date> |
| `future` | N ≥ 1 | **N** card(s) **due** on <date> |
| `past`, `today` | 0 | **No** reviews on <date> |
| `future` | 0 | **No** cards due on <date> |

- `card` for N = 1, `cards` otherwise. The bold runs are the number or `No`, and `reviewed` or `due`. VoiceOver reads the same text without the emphasis.
- Today's cell follows its kind: with no review yet and cards due it reads `**12** cards **due** on …`; after one answer, `**1** card **reviewed** on …` (AE54).
- The empty wording follows the kind, so T's empty cell reads `No reviews` and every later cell `No cards due`. The add-on compares the cell's midnight with the wall clock instead (M18).
- `calendar` cells take P7's two lines: the date (M10.2), then `<n> review(s)`.
- The widget (`trailing` and its `year`) shows none of these texts: it has no tooltip or callout (`stats/widget-families` F3).

| Cell | English text |
|---|---|
| 2026-10-12, `today`, 1 | **1** card **reviewed** on Monday, October 12, 2026 |
| 2026-10-11, `past`, 1,234 | **1,234** cards **reviewed** on Sunday, October 11, 2026 |
| 2026-10-13, `future`, 0 | **No** cards due on Tuesday, October 13, 2026 |
| 2026-10-12, `future`, 12 | **12** cards **due** on Monday, October 12, 2026 |
| 2026-10-12, `today`, 0 | **No** reviews on Monday, October 12, 2026 |
| Calendar, 2026-10-12, 3 | Monday, October 12, 2026 / 3 reviews |

### M10.2 Long date

- The locale's complete date style (weekday, month name, day, year) with no time: `Monday, October 12, 2026` in English (US). P7's Calendar date line uses this writer.
- Every date text of this file (the long date, M7.3's labels, M15's values and M15.6's bin names) is written in the Gregorian calendar with the user's language and region, whatever calendar the user has chosen in system settings, so the text names the year the grid shows.
- It names the cell's own civil date in every time zone: it is built from the date's components and written with the zone those components were built in, never through the device zone. 2026-10-12 reads `Monday, October 12, 2026` with the device in Pacific/Kiritimati and in Pacific/Niue.
- The add-on writes the English date with no comma after the weekday (`Monday October 12, 2026`), and R1320 quotes that form. Meguri uses the localized long date R1320 asks for, which in English has the comma (M18).

### M10.3 Numbers

Counts in M10, M15 and M17 use the locale's integer format with grouping: `1,234` in English, as the add-on writes it. A printf-style integer conversion does not group, so each `%lld` of M17 only chooses the plural form: the digits shown in its place are the count written by this format and passed as a separate string argument (in the String Catalog, the key takes the count for its plural rule and the written number where M17 shows the number).

## M11. Controls and navigation

### M11.1 Controls row

- Above the grid, centred: Back, Today and Forward, small borderless native buttons with the symbols `chevron.left`, `circle.fill` (small scale) and `chevron.right` (R1317). The symbols do not mirror in right-to-left layouts (M7.3).
- `calendar`: Back, the shown year as text, Forward; no Today button (R1380).
- `trailing` and the widget's `year`: no controls.
- Nothing else: no options, support or theme button, and no hidden gesture on any control (R1325). The mode is a setting (R1339).

### M11.2 Bounds and paging

- Back shows `p − 1`, Forward `p + 1`, Today `p` = 0, in A5's periods: a year in `year`, nine months in `months` (R1317). Today is always enabled; at `p` = 0 it does nothing.
- Back is enabled only when `can_go_back`, Forward only when `can_go_forward` (A6, R1318, R298). A disabled control takes no click, key or gesture.
- `calendar`: Back and Forward move a year within P7's bounds (R1380).
- A period change keeps the selection's date and closes an open callout. A mode change (the setting) shows period 0 (A5) and does the same.

### M11.3 Jumps, keyboard and swipes

| Platform | Input | Effect |
|---|---|---|
| Mac | Shift-click or Option-click on Back / Forward | first period / last period (R1319, R298) |
| Mac; iPad with a hardware keyboard | ← / → with the grid focused | Back / Forward (R1324) |
| Mac; iPad with a hardware keyboard | Shift-← / Shift-→ with the grid focused | first period / last period |
| iPhone, iPad | long press (the system's duration) on Back / Forward | first period / last period (R1319) |
| iPhone, iPad | horizontal swipe on the grid, finger moving right / left | Back / Forward (R1319; M13.3), in `calendar` too |
| VoiceOver, every platform | Back's and Forward's custom actions (M17) | first period / last period |

- An input whose control is disabled does nothing and makes no sound. Back's first-period action and Forward's last-period action are present only while their control is enabled.
- The grid is one keyboard focus target, with the system focus ring, after the buttons: the focus order is Back, Today, Forward, grid (`calendar`: Back, Forward, grid). Focus does not move cell by cell; VoiceOver reads cells (M15). Return, Space and every other key on the focused grid do nothing.
- Horizontal scrolling with a trackpad or mouse over the grid does not page; it passes to the enclosing view.
- `calendar` has no jumps: no Shift- or Option-click, Shift-arrow, long press or first and last actions. ← and → and a swipe move a year.

### M11.4 Period changes

- The host asks for the new period's data (A11 with the new `p`; P7 for a year) on the component's period request (M11.5). The old grid stays until it arrives; no progress indicator (A11 answers within 5 ms).
- `year`, `months`: the new grid comes in with the system's push transition from the side of the move (Back from the left, Forward from the right, as time runs in M7.3), with the default animation. With Reduce Motion it uses the system's opacity transition instead.
- `calendar`: a year change follows P18.4: the default animation, no custom motion.

### M11.5 Period requests

- The component asks the host for a period; it never computes data. `year`, `months`: a request carries the target `p`: Back `p − 1`, Forward `p + 1`, Today 0, a jump to the first period `first_period`, to the last period `last_period`, M15.3's paging `p − 1` or `p + 1`. `calendar`: the target year, the shown year minus or plus 1.
- A further request while one is pending counts from the last requested period, not the shown one.
- A series for a period other than the last one requested is dropped, so answers arriving out of order never show an older request.
- A period is identified by its mode and offset `p` in `year` and `months` (M1's `navigation`, which the host passes with each series), and by its year in `calendar`; never by its dates. A series of another mode than the last request is a mode change (M11.2) and is always taken, at its `p`.
- A host refresh (new data, the derived store's next event, a rollover or zone change that moves T: `stats/heatmap-addon` A11) asks again for the last requested period: the same `p` in `year` and `months`, the same year in `calendar`. So when a change of T moves the periods, the grid shows the new dates of that `p`: in `months` at `p` = −1, T moving from 2026-10-31 to 2026-11-01 changes the shown period from 2025-09-01 … 2026-05-31 to 2025-10-01 … 2026-06-30 (A5); in `year` at `p` = −1, T moving from 2026-12-31 to 2027-01-01 changes it from 2025 to 2026. The selection follows M9, and the bounds come with the new series. A year that P7 moves in `calendar` (a history change) replaces the last requested year.
- The component draws nothing before its first series. The host requests period 0 (the current year in `calendar`) when it first appears.

## M12. Pointer interaction (Mac)

- **Hover.** The cell under the pointer gets the hover outline at once (M9). The tooltip's delay of 0.3 s starts when the pointer comes onto a cell from no cell (R1321; the add-on fades its tooltip in after about a third of a second). Moving from cell to cell does not restart it, and once the tooltip shows, moving to another cell updates it with no new delay. A point that hits no cell (M7.4) hides the tooltip and cancels the delay. `calendar` shows its tooltip at once, as Anki's page does (P16.2).
- **Tooltip.** The system's own help tag cannot be placed on a cell, show bold runs or take this delay, so the tooltip is a small borderless child panel of the window that never becomes key or takes focus or clicks, with the system tooltip material (`NSVisualEffectView.Material.toolTip`), the system tooltip font (`NSFont.toolTipsFont(ofSize:)` at its default size) and the primary label colour. It sits above the cell, centred on it, below the cell when there is no room above, kept inside the window, in every mode, the Calendar included. It holds M10.1's text, with its bold runs.
- When the data changes while the tooltip shows, its text follows; when its cell is no longer drawn, it hides.
- **Click a cell with data** (count ≥ 1 with a `search`; in `calendar`, a cell with a `query`): the selection moves to it and the callback runs with its day descriptor (M14); the host opens the Browser in the same window (R1322, R300; P16.2).
- **Click a cell without data**: the selection moves back to T; no callback (R1322). In `calendar` such a click does nothing, as in Anki.
- A click that hits no cell does nothing.
- On iPad with a pointer, hover draws only the hover outline; a click acts as a tap (M13).

## M13. Touch interaction (iPhone, iPad)

### M13.1 Tap and callout

- A tap selects the cell nearest the finger (M7.4), whether or not it has data, and shows its callout (R1321): a popover anchored to the cell, kept a popover at compact width too (compact-width presentation `.popover`, never a sheet), with M10.1's text and, when the cell has data (in `calendar`, a `query`), a `Show in Browser` button. The button runs the callback (M14). The first tap never opens the Browser (R1321's deviation: cells are about 6 pt wide on iPhone).
- The selection stays on the tapped date after the callout closes (R1306).
- The grid stays live while the callout shows (it is a pass-through region of the popover): a tap on another cell moves the selection and the callout to that cell in one tap; a touch and hold closes the callout and starts a scrub (M13.2); a page swipe pages (M11.3) and so closes it. A tap anywhere else closes it without other effect.
- The callout closes on a tap outside the grid and the callout, a scroll, a period or mode change, or the Browser opening. A width change re-anchors it (R1308).
- When the selected cell's data changes while the callout shows, the text and button follow (AE54).

### M13.2 Scrub

- Touch and hold on the grid, then drag: the selection follows the finger from cell to cell (R301). A selection haptic plays at each change on iPhone.
- While scrubbing, a magnified callout sits above the finger, never under it: the day's text and the cell drawn larger, at least 24 pt, in its own colour and outline.
- A finger over no cell keeps the last cell.
- On release the selection stays and the callout becomes M13.1's, with `Show in Browser` when the day has data.

### M13.3 Gesture resolution

| Touch | Result |
|---|---|
| down and up within the hold delay, moving less than 10 pt | tap (M13.1) |
| held still (less than 10 pt) for the hold delay (0.3 s) | scrub (M13.2) starts at once, with the magnified callout; moving drags it |
| held for the hold delay and released without moving | M13.1's callout for the cell, as on any scrub release |
| moving more than 10 pt before the hold delay, sideways, ending with 40 pt of horizontal travel or 300 pt/s | page: Back when the finger moved right, Forward when it moved left (M11.3) |
| moving more than 10 pt before the hold delay, sideways, ending with less travel and speed | nothing: no tap, no page |
| moving more than 10 pt before the hold delay, vertically | passed on: the enclosing list or scroll view scrolls |

- The direction is judged once, when the touch first leaves its 10 pt circle: sideways when the horizontal distance from the start is greater than the vertical one, else vertically.
- A page swipe toward a disabled control does nothing.
- `calendar` follows the same table: a swipe moves a year, a drag without a hold never moves the selection (P16.3's exception).

## M14. Day descriptor and hosts

The callback gets `{date, history, count, search}`: `history` is true for kinds `past` and `today` and false for `future`; `search` is the entry's A9 search (`calendar`: the cell's P7 item text). The component builds and edits no query (R1322). Home and deck-page hosts pass the search on unchanged (U228); the Calendar's host joins the item text with its scope (`stats/browse-queries` Q3, Q5; U230). Both open the Browser. A Browser listing fewer or other cards than the count needs no notice (R1323, A9).

AE54: 12 cards due today, no review → today's descriptor is `{2026-10-12, false, 12, prop:due=0}`; after one answer, `{2026-10-12, true, 1, prop:rated=0}`.

The widget has no interaction; its tap opens the app (U165).

## M15. Accessibility

### M15.1 Element tree

`year` (outside the widget) and `months`, in reading order (R1441, R183):

1. the Back, Today and Forward buttons (M17 labels);
2. the grid element (`heatmap.grid`), a container labelled `Heatmap` (M17) that holds, in order:
   1. the summary element (M15.2);
   2. the month element (M15.3);
   3. one container per month of the period, labelled with the month (M17), holding that month's day elements in date order (M15.4).

The grid element is the keyboard focus target of M11.3. A custom rotor `Months` moves between the month containers. The drawn year, month and weekday labels are hidden from VoiceOver: each day's text already names its date. `calendar`: Back, the shown year as static text (read as its digits), Forward, then P18.1's chart container in the grid element's place, holding the month containers and day elements (no summary or month element of its own: P18.1's summary is the container's label), and M15.6. `trailing` and the widget's `year` are one element whose label is the widget's (U165, `stats/widget-families` F3); M15.2 to M15.4 do not apply to them.

### M15.2 Summary

From A10's `totals`, one part per year of `totals.years`, joined with `, `: `<n> review(s) in <year>`; then `, current streak <n> day(s)` from `totals.current_streak`. Examples: `5,210 reviews in 2026, current streak 364 days`; in months mode across a year end, `3,102 reviews in 2026, 0 reviews in 2027, current streak 1 day`. Decided 2026-11-10: R1441's wording. R183's other values (today's reviews, the daily average) are read on today's day element and on the streak stats (R1442, U228).

### M15.3 Month element

- One adjustable element labelled `Month`. Its value is the month and its count from `totals.months`: `October 2026, 312 reviews`.
- It starts on T's month when T is in the period, else on the period's first month. Increment and decrement move one month. They change only the element's value and, past a period's edge, the period; the selection, the VoiceOver cursor and the drawn outlines stay as they are. Past the period's last or first month they page (M11.2) when the bound allows, landing on the new period's first or last month; at a bound they do nothing.

### M15.4 Days

- One element per drawn cell. Label: M10.1's text. A Calendar cell's label is P7's two lines joined with `, ` (P18.2): `Monday, October 12, 2026, 3 reviews`. Today's element has the value `Today`; the selected one the selected trait.
- A cell with data (count ≥ 1 with a `search`; in `calendar`, a cell with a `query`, whatever its count) has the button trait and the custom action `Show in Browser`, which runs the callback; activating it runs the same action. A cell without data activates as a tap (M13.1) or a click (M12) would. A Calendar cell with a count and no query (a date with rows only on day index 1, `stats/graphs-revlog` V3.3, `stats/browse-queries` Q4.2) has neither the trait nor the action.
- Elements are built from the layout on demand, not as views (M16).

### M15.5 Identifiers

| Element | Identifier |
|---|---|
| Back, Today, Forward | `heatmap.nav.back`, `heatmap.nav.today`, `heatmap.nav.forward` (R1445) |
| grid | `heatmap.grid` |
| summary | `heatmap.summary` |
| month element | `heatmap.month` |
| day | `heatmap.day.YYYY-MM-DD`, for example `heatmap.day.2026-10-12` |
| callout and its button | `heatmap.callout`, `heatmap.callout.showInBrowser` |

In `calendar` the grid's identifiers start with `stats.calendar.` instead of `heatmap.` (`stats.calendar.day.2026-10-12`), so the two grids never clash in UI tests.

### M15.6 Calendar chart descriptor and bin names

- The Calendar is a Stats chart, so R1439 asks it for an audio graph. The `year` and `months` grids publish no `AXChartDescriptor`: R1441 gives the heatmap its own reading (summary, month element, days), and R1439 covers the Stats screen's charts.
- P18.3's `AXChartDescriptor` for the Calendar: an x axis of categories, one per drawn cell in date order, named by its bin name; a numeric y axis `reviews` from 0 to `max_count`; one series, `Calendar`.
- A Calendar cell's bin name (P18.5): the locale's short month and day, `Oct 12` in English.

## M16. Drawing performance

- One drawing pass draws every cell, batched by fill, then the outlines and the labels. No view per cell (R1839).
- A full period with every cell filled draws within one refresh interval (8.3 ms at 120 Hz) on the reference device, measured with the `heatmap.render` signpost from data arrival to the drawn grid (U244).
- Width changes, hover and selection redraw from the cached layout, without a core call.

## M17. Strings

Meguri takes these texts only from this table. Keys are entries of the Stats String Catalog; `(s)` marks a plural on the number before it, with English using the singular for exactly 1 and the plural otherwise, 0 included.

| Key | English | Where |
|---|---|---|
| `heatmap.tooltip.reviewed` | **%lld** card(s) **reviewed** on %@ | M10.1 |
| `heatmap.tooltip.due` | **%lld** card(s) **due** on %@ | M10.1 |
| `heatmap.tooltip.noReviews` | **No** reviews on %@ | M10.1 |
| `heatmap.tooltip.noDue` | **No** cards due on %@ | M10.1 |
| `heatmap.callout.showInBrowser` | Show in Browser | M13.1, M15.4 |
| `heatmap.nav.back.year` | Previous Year | Back's label in `year` and `calendar` |
| `heatmap.nav.back.months` | Previous 9 Months | Back's label in `months` |
| `heatmap.nav.forward.year` | Next Year | Forward's label in `year` and `calendar` |
| `heatmap.nav.forward.months` | Next 9 Months | Forward's label in `months` |
| `heatmap.nav.today` | Today | Today's label |
| `heatmap.nav.back.help.year` | Show the previous year. Shift-click to show the first year. | Mac help on Back, `year` |
| `heatmap.nav.back.help.months` | Show the previous 9 months. Shift-click to show the first 9 months. | Mac help on Back, `months` |
| `heatmap.nav.forward.help.year` | Show the next year. Shift-click to show the last year. | Mac help on Forward, `year` |
| `heatmap.nav.forward.help.months` | Show the next 9 months. Shift-click to show the last 9 months. | Mac help on Forward, `months` |
| `heatmap.nav.back.help.calendar` | Show the previous year. | Mac help on Back, `calendar` |
| `heatmap.nav.forward.help.calendar` | Show the next year. | Mac help on Forward, `calendar` |
| `heatmap.nav.today.help` | Show the period that contains today. | Mac help on Today |
| `heatmap.nav.back.hint.year` | Touch and hold to show the first year. | VoiceOver hint on iPhone and iPad, `year` |
| `heatmap.nav.back.hint.months` | Touch and hold to show the first 9 months. | the same, `months` |
| `heatmap.nav.forward.hint.year` | Touch and hold to show the last year. | the same, `year` |
| `heatmap.nav.forward.hint.months` | Touch and hold to show the last 9 months. | the same, `months` |
| `heatmap.nav.first.year` | Show First Year | VoiceOver action on Back, `year` |
| `heatmap.nav.first.months` | Show First 9 Months | VoiceOver action on Back, `months` |
| `heatmap.nav.last.year` | Show Last Year | VoiceOver action on Forward, `year` |
| `heatmap.nav.last.months` | Show Last 9 Months | VoiceOver action on Forward, `months` |
| `heatmap.a11y.yearTotal` | %lld review(s) in %@ | M15.2, one per year |
| `heatmap.a11y.currentStreak` | current streak %lld day(s) | M15.2 |
| `heatmap.a11y.grid` | Heatmap | M15.1 grid element label |
| `heatmap.a11y.month` | Month | M15.3 label |
| `heatmap.a11y.monthValue` | %1$@, %2$lld review(s) | M15.3 value: the month (pattern `LLLL yyyy`, `October 2026`), its count |
| `heatmap.a11y.today` | Today | M15.4 value |
| `heatmap.a11y.monthsRotor` | Months | M15.1 rotor |
| `stats.calendar.a11y.series` | Calendar | M15.6 series |
| `stats.calendar.a11y.yAxis` | reviews | M15.6 y axis |

- Hints: on the Mac, Back, Today and Forward have no VoiceOver hint; VoiceOver reads their help. On iPhone and iPad, Today has no hint, and in `calendar` Back and Forward have none (no jumps, M11.3).
- `%lld` chooses the plural form; the number shown is M10.3's (M10.3).

Formats, not strings: the year label (M7.3, digits), the month label `LLL ''yy`, the month container label `LLLL yyyy`, the weekday labels (narrowest stand-alone symbols), the long date (M10.2) and the Calendar bin name (M15.6). The year argument of `heatmap.a11y.yearTotal` is the year in digits without grouping. The Calendar's tooltip count is P19's `<n> review(s)`.

## M18. Differences from the add-on and from Anki's calendar

None of these reaches a golden, so none has a `conformance/deviations.yaml` entry.

| Difference | Add-on or Anki | Meguri | Why |
|---|---|---|---|
| Week start | always Monday | `weekStart`, the synced `firstDayOfWeek` (M2) | R149 (plan Open Questions) |
| Nine-month periods | a sliding window: paging moves nine months but stops at the month of the first history day or of the last forecast day, so a page at a bound moves less | A5's fixed periods (M11.2) | R1317, A5 |
| First nine-month window | starts at the month of the first history day when that is later than four months before the wall-clock month; reaches at least the sixth month after T going forward | always A5's period 0; bounds are A6's | R1303, R1318 |
| Window month | four months before the wall-clock month; when today's day of the month does not exist in that month (for example the 31st), the window starts one month later (2026-10-31 starts it in 2026-07, not 2026-06) | A5 from T | R1303, A5 |
| Shown year and today's period | the wall-clock year and month; between midnight and the rollover hour they can differ from T's | from T (A5) | `stats/day-buckets` B4.2 |
| Empty-day wording | `No reviews` when the cell's midnight has passed by the wall clock, else `No cards due`: T + 1's empty cell reads `No reviews` between midnight and the rollover hour | by kind (M10.1) | R1320 |
| Long date | `Monday October 12, 2026`, English only | the localized long date, `Monday, October 12, 2026` in English (M10.2) | R1320 |
| Controls at bounds | stay enabled and do nothing | disabled (M11.2) | R1318 |
| Jumps on touch, keyboard paging, swipes | none | M11.3 | R1319, R1324 |
| Hidden gestures and extra buttons | Shift-click on Today switches the mode; options and support buttons; Shift-click on options cycles themes | none (M11.1) | R1325 |
| Colours | five themes, square cells, grey empty cells (light grey by day, dark grey at night) | one Meguri palette, rounded cells, system fills (M7.1, M8) | R1315, R1307 |
| Touch | none | tap, callout, scrub (M13) | R301, R1321 |
| Clicked day's search | history days: a review-id range, which finds nothing on deck pages, where the add-on puts the current-deck term before it and the range is then not recognised; forecast days: `prop:due=N` | the core's A9 search (M14) | R1322 |
| `year` mode in years that start on a Monday, the add-on's first weekday (2018, 2024, 2029) | the first column is left empty and the last week's days fall past the year's last column, outside the drawn width | columns 1 to C, every date drawn (M3) | M3 |
| Tooltip delay between cells | the fade-in starts again on every cell | no new delay once the tooltip shows (M12) | R1321 |
| Tooltip below the cell | always above, even at the window's top | below when there is no room above (M12) | native placement |
| Calendar colours | Anki's blue ramp, 20% to 100% (night 80%) | M8.3 on Meguri's history ramp | R1382 |
| Calendar's 54th column | drawn in the plot's right margin, past its 53-week axis | a column of the grid like the others (M6) | M3 |
| Calendar hover and selection | no hover or selection outline; the tooltip follows the pointer | M9's outlines; the tooltip above the cell (M12) | one component (R289) |
| Calendar weekday labels | clicking Sunday, Monday, Friday or Saturday sets the week start | text only; the card's menu sets it (M6) | R1381 |

## M19. Decisions

Decided 2026-11-10, in this order of priority: the plan's requirements, the add-on's drawing, Apple platform conventions, simplicity.

1. **Pitch rule.** M7.2 gives one formula for every platform: 12 pt cells with 2 pt gaps at most, shrinking to the width. R1307's 10 to 12 pt on Mac and iPad follows from usual widths; narrow windows shrink rather than scroll.
2. **Selection on T draws only today's outline** (M9), as the add-on's highlight does.
3. **Touch selection of an empty day stays** (M13.1), while a Mac click on one returns the selection to T (M12, R1322): on touch the tap must show the empty day's callout (R1321).
4. **The empty wording follows the kind** (M10.1), not the wall clock (M18).
5. **Calendar has no today outline** and shows its tooltip without delay (M6, M12), as Anki's page does.
6. **Summary wording** (M15.2) follows R1441; R183's extra values are on other elements.
7. **Left-to-right in every layout** (M7.3, M11.1), as both sources draw time.
8. **Option-click** jumps like Shift-click (M11.3), as the plan's U227 approach adds.
9. **Gesture thresholds** (M13.3): 10 pt slop, 0.3 s hold, 40 pt or 300 pt/s for a page.
10. **Calendar identifiers** take the `stats.calendar.` prefix (M15.5).
11. **Labels that do not fit** shrink, thin out or are left out (M7.3) rather than grow the grid or overlap: cells keep M7.2's size, and every day's text names its date. This caps R1444's Dynamic Type for the grid's labels only.
12. **Mac tooltip panel** (M12): a minimal exception to native controls. The system help tag cannot sit on a cell, show R1320's bold runs or keep R1321's 0.3 s, so the panel uses the system tooltip material and font.
13. **One tooltip and touch model for every mode** (M12, M13): the Calendar's tooltip sits above its cell and its touch rules are M13's, not P16.2's pointer placement or P16.3's drag selection, so one component behaves one way (R289).
14. **R1441's per-day elements replace R183's day-by-day adjustable element** (M15.1, M15.4): days are elements in month containers, and the adjustable element moves by month (R1441). The `year` and `months` grids publish no audio graph (M15.6).
15. **Period change motion** (M11.4) is the system's push transition with its default animation, not a custom slide (N11.7's no custom motion).
16. **Dates are Gregorian** in every text (M10.2), as the grid is (M1).

## M20. Tests

No conformance scenario (the Scenarios line). U227's tests, from this file's tables:

- Layout (Swift Testing, HeatmapKit): every row of M2, M3, M4 and M5; the column count of every year 2000 to 2050 for each week start against `ceil((o + days) / 7)`; months mode across a year end; trailing mode for both N and every week start, built from a W3 snapshot and a W4 entry (M1); the widget's `year` built from a W3 snapshot and a W4 entry, for Tₛ on December 31 and January 1 (`stats/widget-families` F1, F3); a `calendar` series without some dates of the year draws no cell for them (M6).
- Pitch (M7.2): every row of M7.2's table, the 640 pt Mac example, a 1,400 pt Mac width (`p` 14, centred), M7.2 step 5's total width of 363⅓ pt, a grid never wider than W, a side of at least 5 pt in every mode whenever W ≥ 6k + L, and the widget's `year` limited by H ÷ 7.
- Labels (M7.3): each step of the month and weekday label rules, chosen by width, pitch and text size; L and the pitch taken again after the weekday labels shrink (M7.2 step 6).
- Hit-testing (M7.4): points in cells, in widened gaps, in block gaps, in undrawn slots and on labels.
- Colours (M8): every M8.1 row; M8.2's three rules in light, dark and both contrast settings; M8.3's table.
- Outlines (M9): today, selection and hover combinations; selection kept across paging and width change; a selection on T moving with T at a rollover, and one on another date staying.
- Period requests (M11.5): each control's target `p`; two quick Backs requesting `p − 1` then `p − 2`; an out-of-order answer dropped; a refresh in `months` at `p` = −1 with T moving from 2026-10-31 to 2026-11-01 requesting `p` = −1 again and showing 2025-10-01 … 2026-06-30, and the same in `year` from 2026-12-31 to 2027-01-01 showing 2026; a series of another mode taken as a mode change.
- Text (M10, M17): every M10.1 table row; plurals at 0, 1 and 2; grouping at 1,234; the long date in Pacific/Kiritimati and Pacific/Niue; the summary examples of M15.2.
- Navigator: A6's example (first history day 2024-12-03, last forecast day 2027-08-01, `year`: Back disabled at 2024, Forward at 2027, Shift-click Back from 2026 shows 2024); `months` bounds; Today; mode change to period 0; a width change from iPhone to iPad width keeps the period and the selected date (R1308).
- AE54 (UI side): iPhone, 12 due today and no review: tapping today's cell shows `**12** cards **due** on …` with `Show in Browser`, whose callback gets a forecast descriptor with `prop:due=0`; after one answer the same cell reads `**1** card **reviewed** on …` and the callback gets a history descriptor with `prop:rated=0`.
- AE55 (display side): a series with levels 4 and 10 draws `heatmap.history.4` and `heatmap.history.10` (snapshot).
- Mac UI tests (VM): hover 0.35 s shows the tooltip, and moving to the next cell updates it at once; a click on an empty future day returns the selection to T with no callback; with the grid focused, → shows the next period and Shift-← the first; the focus order Back, Today, Forward, grid.
- Touch UI tests: tap, scrub with the magnified callout, hold and release without moving, swipe paging (Calendar included), a short sideways drag that does nothing, long press jumps, a vertical drag that scrolls the list, and with a callout open a tap on another cell that moves it in one tap.
- VoiceOver tree: the order of M15.1, the adjustable month element, the `Months` rotor, the day element `heatmap.day.2026-10-12` with `Show in Browser`; a Calendar cell with a count and no query, with no button trait and no `Show in Browser` action (M15.4).
- Snapshots: `year` and `months`, light and dark, Increase Contrast, an empty history, a very busy history, 53 and 54 columns, iPhone and iPad and Mac widths, the widget families (small, medium, large, extra large, in full-colour, accented and vibrant modes), and the Calendar with sparse and dense years; `months` and `calendar` at iPhone width with the default and an accessibility text size (alternate and absent weekday labels, narrow and absent month labels).
- Drawing: M16's signpost within one frame at 120 Hz on the reference fixture.

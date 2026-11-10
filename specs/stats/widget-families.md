# Widget families

Spec: stats/widget-families
Requirements: R306, R307, R308, R309, R310, R311, R312, R313, R314, R317, R318, R1309, R1431, R1432, R1435, R1436, R1437, R1438
Anki: none (Meguri's widget; the grid, its levels, the streak stats and their wording are the Review Heatmap add-on 1.0.1's through `stats/heatmap-addon`, `stats/heatmap-presentation` and `stats/activity-strings`, which carry the pins)
Scenarios: conformance/scenarios/stats/widget-today-count/ (F5's scope rule, through the Home values the snapshot copies)

What each family of the heatmap and streak widget (U165) draws from one state of the widget snapshot (`stats/widget-snapshot`). The snapshot already holds every count, level, streak value and date, computed by the Home heatmap's rules (R1431), so the widget runs no date, count, level or streak logic (R1435). This file binds those values to the add-on's grid and streak stats in the widget, so the widget looks and reads as the Home activity section does. The widget's layout, typography, the look of its rendering modes, its timeline, its staleness line and its deep link are U165's design and are not specified here.

## F1. The entry state

- A timeline entry draws one state S: the snapshot's own top-level `today`, `todayCell`, `streaks` and `studiedToday`, or one `upcoming[i]` entry (`stats/widget-snapshot` W4). Which state an entry draws, and the instant it starts, are U165's timeline planner; the instants are the snapshot's `at` values.
- T₀ is the snapshot's `today`, Tₛ the state's `today` (T₀ for the top-level state, later for an `upcoming` entry).
- Every date d of a grid gets a kind, a count and a level from S, as `stats/heatmap-presentation` M1 says for `trailing` and a W4 entry:

| Date | Kind | Count | Level |
|---|---|---|---|
| d < Tₛ, `history.start` ≤ d ≤ T₀ | `past` | `history.counts[d − history.start]` | `history.levels[d − history.start]` |
| d < Tₛ, d < `history.start` or d > T₀ | `past` | 0 | 0 |
| d = Tₛ | S's `todayCell.kind` | S's `todayCell.count` | S's `todayCell.level` |
| d > Tₛ | `future` | 0 (not read) | `forecast.levels[d − T₀]`, 0 past the array's end |

- The forecast index counts from T₀, never from Tₛ. Snapshot built on 2026-12-28 (T₀), state `upcoming[3]` (Tₛ 2027-01-01): 2026-12-30 is `past` with level 0; 2027-01-01 is S's `todayCell`; 2027-01-05 takes `forecast.levels[8]`.
- A `no-history` snapshot has no grid (F6).

## F2. Families (R307 to R310, R1432)

| Family | WidgetKit family | Where | Grid | Text |
|---|---|---|---|---|
| small | `systemSmall` | iPhone, iPad, Mac | `trailing(8)` (`stats/heatmap-presentation` M5) | current streak (F4); today's count (F5) |
| medium | `systemMedium` | iPhone, iPad, Mac | `trailing(20)` (M5) | current streak (F4); today's count (F5) |
| large | `systemLarge` | iPhone, iPad, Mac | `year` (F3) | the four streak stats (F4); the studied-today line (F5) |
| extra large | `systemExtraLarge` | iPad, Mac | `year` (F3), larger cells | the four streak stats (F4); today's count (F5) |

- R1432 says each size shows what R307 to R310 define. An older shell coverage note gave the small family a shorter list (streak and today only, no grid); R1432 as now written resolves this, and the small family has the 8-week grid.
- Placements (R306, R311): the configuration supports only the four families above. It offers no accessory family (so no Lock Screen or Watch widget) and no control (so no Control Center widget). StandBy and iPhone widgets on the Mac take the system's `systemSmall` family, so the configuration marks them as disfavored locations (`.disfavoredLocations`, as the plan's U165 says). A disfavored location is not blocked: the system can still offer the widget there, and it then draws as here. Marking any further location the system adds for these families is U165's.
- Content (R312, R318): the widget shows only these grids and texts, F6's messages, and U165's staleness line ("Updated <relative time>" when the snapshot's `refreshedAt` is more than a day before the entry's date, R317, R1436; its wording, form and timeline entries are U165's). It shows no due count, card content or deck name.

## F3. The year grid in the widget

The large and extra-large families draw `stats/heatmap-presentation` M3's `year` layout with no navigation. `stats/heatmap-presentation` M1 names this as the widget's fixed `year` mode (edit request); until that edit lands, this section is the rule.

- Y is the year of Tₛ, so on and after an entry whose Tₛ is January 1 the grid shows the new year. Its period is January 1 to December 31 of Y. Its cells come from F1: dates of Y before Tₛ are `past` (level 0 when they are after T₀), and its dates after Tₛ take the forecast, which reaches December 31 of that year (`stats/widget-snapshot` W3 ranges). Built on 2026-12-28, the entry of 2027-01-01 draws 2027 with its forecast cells.
- `weekStart` is the snapshot's (M2), so `firstDayOfWeek` 1 draws Monday-first rows, as in the trailing grids.
- **Rendering modes** (R313, R1437). In WidgetKit's full-colour mode, cells take M8.1's discrete mapping and M8.2's ramps. WidgetKit's two other modes are accented (the system's tinted and clear widget appearances) and vibrant (the Mac desktop while an app is in front of it, which desaturates the widget). In both, the 10 levels and the empty fill stay distinguishable by opacity steps of one colour, not by hue; the opacity values are U165's (M8.4). A history cell and a forecast cell of the same level may then look alike; today's outline marks where the forecast starts. These rules hold for the trailing grids too.
- Outlines: today's outline only (M9, as for `trailing`). No selection, hover, tooltip, callout, navigation control, totals or Browse search: the widget has no per-day link or button (R314), and a tap anywhere opens Home.
- The year label is drawn below the grid as in M3 (M7.3).
- Pitch: M7.2 with `k` = C, and also at most H ÷ 7 for the height H that U165's layout gives the grid without its label, as `trailing` does: `p = min(14, W ÷ C, H ÷ 7)`, then M7.2 steps 3 and 4. The extra-large family's larger cells come from its larger W and H, not from another rule.
- VoiceOver reads the widget as one element whose label is U165's (M15); the grid has no day elements.

## F4. Streak stats

- The values are S's `streaks` (`stats/heatmap-addon` A8.1; for an `upcoming` entry, A8 with T = Tₛ, `stats/widget-snapshot` W4): AE51's entry of 2026-10-13 04:00 shows Current streak 0 days and Longest streak 364 days.
- The large and extra-large families show the four stats in A8's order, Daily average, Days learned, Longest streak, Current streak, each written with `stats/activity-strings` S2's label and value keys (`activity.streak.dailyAverage.label` and `.value`, `activity.streak.daysLearned.label` with S2's percent format, `activity.streak.longest.label` and `.value`, `activity.streak.current.label` and `.value`), the label in regular weight and the value in bold (R1326).
- The small and medium families show the current streak: its value is S2's `activity.streak.current.value` text; where U165's layout shows a label for it, the label is `activity.streak.current.label`.
- The keys and their English are S2's in every target that shows them. If the widget extension cannot read the Stats String Catalog's bundle (U165 builds the extension on HeatmapKit only), it holds its own catalog entries with the same keys and the same English and comments, and a catalog test compares them with S2. Which bundle holds them is U165's choice.
- In the full-colour mode each value is tinted with its A8.2 level on the history ramp, as the activity section does; level 0 is the neutral text colour. AE51's 364-day streak has level 10, the current streak of 0 level 0. The tinted text must keep the system's text contrast on the widget container in light and dark appearance; meeting that, by the tint's shade or weight, is U165's, as it is U228's in the activity section (`stats/heatmap-presentation` M8.2). In the accented and vibrant modes the values take the mode's text colour, with no level tint.
- S2's explanations (`.help`) are not shown and are no VoiceOver hint: a widget has no hover, long press or hint, and it is one VoiceOver element (F3). S2's pair elements do not apply in the widget.
- No other wording is used for a streak stat in the widget.

## F5. Today's count and the studied-today line

- **Today's count** (small, medium, extra large) is S's `todayCell.count`: the review count behind Tₛ's cell. It is `stats/heatmap-addon` A4's count when the kind is `today`, and 0 when the kind is `future` (cards due, no review yet), because the snapshot holds no due count (`stats/widget-snapshot` W3). Its wording is Meguri's (U165's String Catalog entry, with plural variants, R191).
- It counts the Home scope, so the `heatmap` settings apply to it: `limdecks` and `limcdel` (`stats/counting-rules` N3), `limresched` (N2), and `limhist` and `limdate` (N5, `stats/heatmap-addon` A2.2). The studied-today line ignores all of them (N6, R1343). With `limdecks` [Lang], five answers today in Lang and earlier history in other decks, today's count is 0 while the line reads `Studied 5 cards …`. Both come from the snapshot and the widget never reconciles them.
- **The studied-today line** (large) is S's `studiedToday.text` (`stats/widget-snapshot` W3, W4), shown exactly as it is: the line of `stats/studied-today` T2, English in every locale (T6.3), no number reformatted, no catalog entry (`text/interface-text` I4). The zero line shows too (T2.3, R1298). An `upcoming` entry carries its own line, usually the zero line, so the line never outlives its day.

## F6. States

| Snapshot | Small, medium, extra large | Large |
|---|---|---|
| missing, undecodable, or a `schema` the widget does not know (`stats/widget-snapshot` W1, W2) | R1438's "Open Meguri to get started" in place of everything | the same |
| `state` `"no-history"` (W3) | R1309's "No reviews yet" in place of the grid, the streak and today's count | "No reviews yet" in place of the grid and the four streak stats; the studied-today line stays |
| `state` `"ready"` | F2 | F2 |

- The large family keeps the line in `no-history` because the Home activity section does: R1309's message replaces only the heatmap and the streak stats (`stats/heatmap-addon` A2.2), and the line ignores the `heatmap` settings, so it can show answers while the Home scope has no history.
- Both messages are Meguri's String Catalog entries with the Product Contract's English (`stats/activity-strings` S4 names R1309's message as Meguri's).
- U165's staleness line (F2) can show with `no-history` and `ready`; with no snapshot there is no `refreshedAt` and no staleness line.
- Every state opens Home when tapped (R314, R1438).

## F7. Tests

No oracle observes a widget. U165's tests, from the tables here:

- HeatmapKit unit tests: F1's rows and its year-end example; F3's year choice for Tₛ on December 31 and January 1; F4's texts for 0, 1 and 1,234 and the AE51 tints; F5's today's count for kinds `today` and `future` and an empty cell; F6's three states, including a `no-history` snapshot whose `studiedToday.cards` is 3.
- A catalog test of F4's keys in every bundle that holds them.
- Snapshot tests per family and per rendering mode (full colour, accented, vibrant on the Mac), in U165's matrix.
- The data behind F5's scope rule is pinned by the inactive oracle scenario `stats/widget-today-count/limdecks-vs-studied` (Home today's cell against studied today under `limdecks`), which waits for the add-on oracle image like the other `heatmap_report` scenarios (`specs/conformance/stats-ops.md` H4, H11).

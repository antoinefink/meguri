# Widget families (U165)

Spec-writer note for `specs/stats/widget-families.md`. Session
s-20261110T085429Z-165d83d3 (2026-11-10), shell-platform item (U165, U167,
U168).

| Spec section | Source | Read for |
|---|---|---|
| F1 to F7 | no Anki or add-on file | The file binds existing specs (`stats/widget-snapshot`, `stats/heatmap-addon`, `stats/heatmap-presentation`, `stats/activity-strings`, `stats/counting-rules`, `stats/studied-today`) to the widget families of the plan's U165 and the Product Contract (R306 to R318, R1431 to R1438). Neither Anki 26.09.3 nor the Review Heatmap add-on has a widget. The add-on's wording reaches F4 only through `stats/activity-strings` S2, whose own provenance is `ui-strings-stats-decks.md`. WidgetKit's family and rendering-mode names (F2, F3) are Apple's public API. |
| F5, scenario `stats/widget-today-count/limdecks-vs-studied` | no Anki file | The scope difference follows `stats/counting-rules` N3.3 and N6, whose provenance is `stats.md`. The scenario is new, inactive and has no golden; it observes `heatmap_report` (`specs/conformance/stats-ops.md` H11) and `studied_today_raw`, and waits for the add-on oracle image (H4). |

Review round 1 (2026-11-10): F2 gained the WidgetKit families, the placement
mechanism and U165's staleness line; F3 the three WidgetKit rendering modes;
F4 the catalog bundle and tint contrast rules; F5 corrected the
`todayCell.count` reading of A4 and the per-setting citations; the scenario
now observes today's cell directly.

Wave 9 edit requests (2026-11-10, same session): the widget `year` rules
added to `stats/heatmap-presentation` (intro, M1, M7.2, M8.2, M8.4, M9,
M10.1, M11.1, M15.1, M20), the widget lines of `stats/activity-strings`
(intro, S1, S2), `stats/widget-snapshot` (intro, W3 `studiedToday`),
`stats/studied-today` T6.3 and `stats/counting-rules` N9 restate F1 to F5
and `text/interface-text` I4, I6. No Anki or add-on file was read for them.

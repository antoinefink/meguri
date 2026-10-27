# Stats specs (U95, U97 to U101, U226, U92)

Spec-writer notes for the files of `specs/stats/`: `day-buckets`,
`counting-rules`, `heatmap-config`, `studied-today`, `heatmap-addon`,
`graphs-common`, `browse-queries`, `card-info`, `graphs-revlog`,
`graphs-cards` and `graph-presentation`, and for `specs/conformance/stats-ops.md`.
Read at tag 26.09.3 (`refs/anki-26.09.3`); no code was copied, and every rule
was restated in the spec writers' words. `stats/widget-snapshot` is Meguri's own
format and has no Anki source (its `Anki:` line says so).

The first block of rows (day buckets, counting rules, heatmap config) comes from
the U95 spec session's notes on `spec/U95`, the studied-today rows from U97's on
`spec/U97`; both were checked against the current section ids. The other rows
were written 2026-10-27 by mapping each section of the committed text to the
files that back it. Sections a table does not list are Meguri's own design (the
resident aggregates, caches, freshness, request and result shapes, the widget,
the Previous Card Info history, the chart points, accessibility) or restate
other specs.

## Day buckets, counting rules, heatmap config (U95)

| Spec section | Source | Read for |
|---|---|---|
| day-buckets B1 | `rslib/src/scheduler/timing.rs`, `rslib/src/scheduler/mod.rs`, `rslib/src/config/mod.rs` | where the next day boundary comes from (current fixed offset, rollover hour read as a small integer, 4 when missing); the scheduler caches its timing until the boundary passes (relevant to how scenarios move the clock) |
| day-buckets B2, counting-rules N6 | `rslib/src/storage/revlog/mod.rs` (studied-today query), `rslib/src/storage/revlog/studied_today.sql`, `rslib/src/stats/today.rs` | the window starts one day before the next day boundary and is open at its start (`id >` start); no upper bound; types Manual and Rescheduled excluded by type, nothing else filtered; time summed as stored |
| day-buckets B2 (note on the Today section) | `rslib/src/stats/graphs/today.rs` | the Today section skips rows before the start, so it counts a row stamped exactly at the start |
| day-buckets B3 | `rslib/src/stats/graphs/mod.rs`, `rslib/src/stats/graphs/reviews.rs` | day index = whole seconds minus the next day boundary, divided by 86,400 with truncation toward zero |
| day-buckets B4.4, B6 | `rslib/src/scheduler/timing.rs` | the fixed 86,400 s spans back from the boundary, computed with the offset in force when the boundary was computed |
| counting-rules N1 | `rslib/src/revlog/mod.rs`, `rslib/src/scheduler/new.rs`, `rslib/src/scheduler/reviews.rs`, `rslib/src/scheduler/fsrs/memory_state.rs`, `rslib/src/scheduler/answering/mod.rs`, `rslib/src/scheduler/answering/revlog.rs` | which operations write Manual and Rescheduled rows with no rating and no time; answer time capped when logged |
| day-buckets B4.1 to B4.3, B5; counting-rules N2 to N5; heatmap-config K1 to K3 | Review Heatmap add-on 1.0.1, through the project's private research notes on the add-on (the add-on was not opened in the U95 session) | the history query: rows shifted back by the rollover hours, then the local date of the shifted instant; ease filter for `limresched`; card-existence filter for `limcdel`; deck filter on the current deck for `limdecks` and for the deck screen; setting names, types and defaults |
| day-buckets B4.6; heatmap-config K5 (added 2026-10-16, session s-20261016T053626Z-8bb17255) | the oracle maintainer's report in `driver-stats.md` on `spec/U95` (statements 2, 4 and 5), from Review Heatmap 1.0.1's web view and its config manager; the add-on was not opened in the spec session | the calendar draws a key at 00:00 UTC of the date moved back by the offset at that instant (the 2026 dates of other zones in B4.6 were computed from that rule with Python's zoneinfo, not recorded); defaults merged and stored only when `version` is missing or older; the key read once per session and written on first read |
| heatmap-config K4, K6 | none (Meguri's writer, U96) | K6 follows `storage/config-table` C6; its tests are Meguri's own (K6.4) |

U95's five statements for the oracle maintainer were all answered in
`driver-stats.md` on `spec/U95` (2026-10-16). Statements 1 and 3 held (the
offset at the shifted instant; `limdecks` on `cards.did` with descendants by
name). Statement 2 failed for Lord Howe's DST start: day-buckets B4.6 and the
deviation `heatmap-drawn-day-shift` keep Meguri on the computed date.
Statement 4 holds for the scenarios' keys, which have no `version`, but not for
a key with `version` 1.0.1, and the add-on reads the key once per session.
Statement 5 failed: the add-on writes the key on its first read, and the
driver keeps that write out of the collection. Both are heatmap-config K5.

## Heatmap data and streaks (U95, U96)

`stats/heatmap-addon` was written from the plan's requirements and the U95
specs, with no add-on source (its intro says so) and no Anki file of its own:
its Anki facts (the day number of A1, the due day of A3, the searches of A9)
are cited from `stats/day-buckets`, `search/properties` P4 and `search/history`
H8, whose provenance is `search-terms.md` and `search-state-and-flags.md`.

## Studied today and the Today section (U97)

| Spec section | Source | Read for |
|---|---|---|
| studied-today T2.1 | `rslib/src/storage/revlog/studied_today.sql`, `rslib/src/storage/revlog/mod.rs`, `rslib/src/stats/today.rs` | rows `id >` day start, types 4 and 5 excluded by raw value, the row count and the summed time divided by 1000 as a float |
| studied-today T2.2, T2.3, T5.1, T5.4, T6.1, T6.3 | `rslib/src/stats/today.rs`, `rslib/src/scheduler/timespan.rs`, `rslib/i18n/src/lib.rs`, `rslib/i18n/write_strings.rs`, `ftl/core/statistics.ftl` | unit capped at minutes, f32 arithmetic, f32 args rounded to two decimals before formatting, formatting with two decimals then trimming trailing zeros, isolation marks on outside tests, the message's two line breaks, plural selection on the rounded value |
| studied-today T3.1, T3.3, T6.2 | `rslib/src/stats/graphs/today.rs`, `rslib/src/storage/revlog/mod.rs` (row decoding), `rslib/src/storage/revlog/get.sql`, `rslib/src/revlog/mod.rs` | `id >=` day start; kind from the type's low byte with Learning as the fallback; time read as u32 with 0 as the fallback; `lastIvl` cast to integer; correct = ease > 1; mature = last interval >= 21 |
| studied-today T3.2 | `rslib/src/stats/graphs/mod.rs`, `rslib/src/search/sqlwriter.rs` (deck terms), `ts/routes/graphs/RangeBox.svelte` | a trimmed-empty search reads every revlog row; any other search reads rows of searched cards; `deck:` matches `did` or non-zero `odid` with children; the screen's collection search is the empty string |
| studied-today T4, T5.2, T5.3 | `ts/routes/graphs/today.ts`, `ts/routes/graphs/TodayStats.svelte`, `ts/lib/tslib/time.ts`, `ts/lib/tslib/i18n/utils.ts`, `ts/lib/generated/ftl-helpers.ts`, `package.json` (`@fluent/bundle` 0.19.1) | the four lines and the empty line; numbers as FluentNumber with at most two fraction digits (Intl, grouping on); the again count interpolated raw and its percentage rounded to two decimals then locale-formatted; the mature percentage as a FluentNumber |
| studied-today T8 | `ftl/core/statistics.ftl` | the English strings of the Home line and the Today section |
| conformance/stats-ops H5 to H7 | `pylib/anki/collection.py` (studied today, search builder), `pylib/anki/lang.py`, `proto/anki/stats.proto` | the calls the oracle makes and the fields it reads |

Validation done in the U97 session (throwaway scripts, not in the repo): the
T5.1 rules reproduced Anki's studied-today message on 29,220 inputs, and the
T5.2/T5.3 rules reproduced Intl (`en-US`, `maximumFractionDigits: 2`) and
`Math.round` on 71,903 inputs. A faketime simulation of every U97 scenario
against the oracle image gave the values in `studied-today` T9.

## Graph data: common rules, review-log graphs, card graphs (U98 to U101)

| Spec section | Anki files read | Read for |
|---|---|---|
| graphs-common G1 | `rslib/src/stats/graphs/mod.rs`, `rslib/src/scheduler/timing.rs`, `proto/anki/stats.proto` | the timing a graph request reads (next day boundary, rollover hour, local offset) and what the response carries of it |
| graphs-common G2, G2.1 | `rslib/src/stats/graphs/mod.rs` (search into a card table), `rslib/src/search/sqlwriter.rs` (deck terms: `did` or non-zero `odid`, children by name), `ts/routes/graphs/RangeBox.svelte`, `ts/routes/graphs/index.ts` | Collection as the empty search, Deck as `deck:current`, Search as the typed text; which cards a deck scope holds |
| graphs-common G3 | `ts/routes/graphs/RangeBox.svelte`, `rslib/src/stats/graphs/mod.rs` (days to a review-log start stamp) | the two history choices (one year, all) and the review-log window each reads |
| graphs-common G4 | `rslib/src/storage/revlog/mod.rs` (row decoding), `rslib/src/storage/card/mod.rs`, `rslib/src/revlog/mod.rs`, `rslib/src/card/mod.rs` | the typed reading of review-log and card rows and the values that make the graph request fail |
| graphs-common G5 | `ts/routes/graphs/index.ts`, `ts/routes/graphs/StabilityGraph.svelte`, `ts/routes/graphs/DifficultyGraph.svelte`, `ts/routes/graphs/RetrievabilityGraph.svelte` | the order of the graphs on the page; the three memory-state graphs shown only with the `fsrs` key on |
| graphs-common G7 | `rslib/src/stats/graphs/mod.rs` (graph preferences), `rslib/src/config/bool.rs`, `proto/anki/stats.proto` | the preference members and the config keys behind them |
| graphs-common G9.1 | `rslib/src/stats/graphs/retrievability.rs`, `rslib/src/card/mod.rs` | the card's last review instant taken from its own columns when its data has no `lrt` |
| graphs-common G13 | `ts/routes/graphs/RangeBox.svelte`, `qt/aqt/stats.py` (deck change sets the current deck), `rslib/src/search/sqlwriter.rs` (current deck), `rslib/src/search/writer.rs` (deck node text) | Anki's `deck:current` scope and its side effect; the deck names its builder writes specially |
| graphs-common G14 | `proto/anki/stats.proto`, `rslib/src/stats/service.rs`, `pylib/anki/collection.py` (search builder) | the message and field names of the graph data, the call the oracle makes for each scope |
| graphs-revlog V1 | `rslib/src/stats/graphs/reviews.rs`, `rslib/src/stats/graphs/retention.rs`, `rslib/src/stats/graphs/hours.rs`, `rslib/src/stats/graphs/buttons.rs`, `rslib/src/stats/graphs/mod.rs` | which review-log rows each graph keeps (types, manual entries, the window) |
| graphs-revlog V2, V3.1, V9 (time sums) | `rslib/src/stats/graphs/reviews.rs`, `proto/anki/stats.proto` | per-day counts and times by kind, the day index, the 32-bit time sums |
| graphs-revlog V3.2, V3.3, V3.4 | `ts/routes/graphs/calendar.ts`, `ts/routes/graphs/CalendarGraph.svelte` | how the page turns a day index into a calendar date (move by days, then back by the rollover hours), its DST edges, the cells and their maximum |
| graphs-revlog V4 | `rslib/src/stats/graphs/retention.rs`, `ts/routes/graphs/true-retention.ts`, `ts/routes/graphs/TrueRetention.svelte` | the rows the retention table counts, its periods, young and mature split |
| graphs-revlog V5, V9 (epoch hours) | `rslib/src/stats/graphs/hours.rs`, `ts/routes/graphs/hours.ts` | the hour of a row from its instant and the offset, the ranges, rows just after the epoch |
| graphs-revlog V6 | `rslib/src/stats/graphs/buttons.rs`, `ts/routes/graphs/buttons.ts` | the learning, young and mature groups and the button index per row |
| graphs-cards C1 | `rslib/src/stats/graphs/mod.rs`, `rslib/src/storage/card/mod.rs` | the cards of the scope and the context each card graph reads |
| graphs-cards C2, C3 | `rslib/src/stats/graphs/future_due.rs`, `ts/routes/graphs/future-due.ts` | the due day per queue (the home deck's columns), buried cards, the daily load, the backlog |
| graphs-cards C4 | `rslib/src/stats/graphs/card_counts.rs`, `ts/routes/graphs/card-counts.ts` | the counts per state with and without suspended and buried cards set apart |
| graphs-cards C5, C7 | `rslib/src/stats/graphs/intervals.rs` | the interval and stability maps (stability in the Intervals message) |
| graphs-cards C6, C8 | `rslib/src/stats/graphs/eases.rs`, `rslib/src/stats/graphs/mod.rs` | cards with a memory state, the difficulty bins and their median |
| graphs-cards C9 | `rslib/src/stats/graphs/retrievability.rs`, `rslib/src/card/mod.rs`, fsrs 6.6.2 `inference.rs` (MIT/BSD, not Anki) | elapsed seconds from the card's own columns, the retrievability value, bins and sums per card and per note, NaN handling |
| graphs-cards C10 | `rslib/src/stats/graphs/added.rs` | the added day of a card from its id |
| graphs-cards C11, C12 | `proto/anki/stats.proto` | the record members and their protobuf names |

## Graph presentation (U226)

| Spec section | Anki files read | Read for |
|---|---|---|
| graph-presentation P1 | `ts/routes/graphs/graph-helpers.ts`, `ts/routes/graphs/GraphRangeRadios.svelte`, `ts/routes/graphs/RangeBox.svelte` | the controls each graph takes and their values |
| graph-presentation P2 | `ts/lib/tslib/i18n/utils.ts`, `ts/lib/tslib/time.ts`, `ts/lib/generated/ftl-helpers.ts` | the page's two number writers and its time spans |
| graph-presentation P3 | `ts/routes/graphs/graph-helpers.ts`, `ts/lib/tslib/time.ts` | the day labels of the axes and tooltips |
| graph-presentation P4 | `ts/routes/graphs/histogram-graph.ts`, `ts/routes/graphs/intervals.ts`, `ts/routes/graphs/future-due.ts`, `package.json` (`d3` 7), the d3 7 scale, tick, bin and quantile modules (ISC, not Anki) | step and ticks, the nice domain, bins, zero-width last bins, quantiles, the bar count, the numbers inside bar queries |
| graph-presentation P5 | `ts/routes/graphs/GraphRangeRadios.svelte`, `ts/routes/graphs/graph-helpers.ts` | the ranges per graph, their defaults, the reset when the history changes |
| graph-presentation P6 | `ts/routes/graphs/future-due.ts`, `ts/routes/graphs/FutureDue.svelte` | bins, table rows, backlog toggle, bar queries |
| graph-presentation P7 | `ts/routes/graphs/calendar.ts`, `ts/routes/graphs/CalendarGraph.svelte` | the shown year, which dates are drawn and filled |
| graph-presentation P8 | `ts/routes/graphs/reviews.ts`, `ts/routes/graphs/ReviewsGraph.svelte` | counts and times per kind, tooltip and table lines |
| graph-presentation P9 | `ts/routes/graphs/card-counts.ts`, `ts/routes/graphs/CardCounts.svelte` | labels, percentages, the table |
| graph-presentation P10 | `ts/routes/graphs/intervals.ts`, `ts/routes/graphs/IntervalsGraph.svelte`, `ts/routes/graphs/StabilityGraph.svelte` | interval and stability bins, the percentile ranges, bar queries, median |
| graph-presentation P11 | `ts/routes/graphs/difficulty.ts`, `ts/routes/graphs/DifficultyGraph.svelte`, `ts/routes/graphs/retrievability.ts`, `ts/routes/graphs/RetrievabilityGraph.svelte`, `ts/routes/graphs/percentageRange.ts`, `ts/routes/graphs/PercentageRange.svelte` | percentage bins and ranges, averages, bar queries |
| graph-presentation P12 | `ts/routes/graphs/true-retention.ts`, `ts/routes/graphs/TrueRetentionCombined.svelte`, `ts/routes/graphs/TrueRetentionSingle.svelte` | the retention table's rows and percentages |
| graph-presentation P13 | `ts/routes/graphs/hours.ts`, `ts/routes/graphs/HourGraph.svelte` | hour ranges, tooltip lines, the leading emoji |
| graph-presentation P14 | `ts/routes/graphs/buttons.ts`, `ts/routes/graphs/ButtonsGraph.svelte` | group labels, tooltip lines with the correct percentage |
| graph-presentation P15 | `ts/routes/graphs/added.ts`, `ts/routes/graphs/AddedGraph.svelte` | added-day bins and bar queries |
| graph-presentation P16 | `ts/routes/graphs/HoverColumns.svelte`, `ts/routes/graphs/Tooltip.svelte`, `ts/routes/graphs/tooltip-utils.svelte.ts`, `ts/routes/graphs/NoDataOverlay.svelte`, `ts/routes/graphs/graph-styles.ts` | which items are clickable, tooltip content, the no-data overlay |
| graph-presentation P17 | `ts/routes/graphs/CumulativeOverlay.svelte`, `ts/routes/graphs/histogram-graph.ts`, `ts/routes/graphs/AxisTicks.svelte` | the running total area and Anki's per-bar shading, which Meguri does not copy (colours are Meguri's) |
| graph-presentation P19 | `ftl/core/statistics.ftl` | the English strings the page uses |
| graph-presentation P21 | the files of the rows above | the Anki column of each difference |

No Anki graphics, CSS or JavaScript asset was carried into a spec (R1960): the
specs give arithmetic and text only, and colours, layout and accessibility
(P17.2, P18) are Meguri's.

## Browse queries from Stats (U101, U226)

| Spec section | Anki files read | Read for |
|---|---|---|
| browse-queries Q2 | `ts/routes/graphs/RangeBox.svelte`, `ts/routes/graphs/index.ts`, `rslib/src/search/writer.rs` (deck node text), `pylib/anki/collection.py` (search builder) | Anki's scope texts and how its builder writes a deck name |
| browse-queries Q3, Q5, Q7 | `ts/routes/graphs/GraphsPage.svelte` (browser search command), `qt/aqt/stats.py` | how Anki joins the scope and the item text (space, scope, space, item) and opens the Browser |
| browse-queries Q4.1 | `ts/routes/graphs/card-counts.ts` | the Card Counts label texts and the suffix that sets suspended and buried cards apart |
| browse-queries Q4.2, Q7 | `ts/routes/graphs/calendar.ts`, `rslib/src/search/parser.rs` (`prop:rated` range) | the calendar cell text, future cells' texts that the search rejects |

## Card Info (U92)

| Spec section | Anki files read | Read for |
|---|---|---|
| card-info C1, C3.1 to C3.4, C3.6 | `rslib/src/stats/card.rs`, `proto/anki/stats.proto`, `rslib/src/card/mod.rs` | the raw values of the card-stats response: card, note and decks, first and latest review, due and position, memory state, average and total time |
| card-info C3.5 | `rslib/src/stats/card.rs`, `rslib/src/scheduler/fsrs/memory_state.rs`, fsrs 6.6.2 `inference.rs` (MIT/BSD, not Anki) | retrievability only from a stored memory state, the elapsed time from the last review |
| card-info C4 | `ts/routes/card-info/CardStats.svelte`, `ts/routes/card-info/lib.ts`, `ftl/core/card-stats.ftl` | row order and which rows show for which card (the Ease row Meguri leaves out) |
| card-info C5 | `ts/lib/tslib/time.ts`, `ts/lib/tslib/i18n/utils.ts`, `ts/routes/card-info/CardStats.svelte` | dates, time spans, integers and percentages as the page writes them, custom data |
| card-info C6.1, C6.2, C6.4 | `rslib/src/stats/card.rs` (review log entries), `ts/routes/card-info/Revlog.svelte`, `ftl/core/card-stats.ftl` | entries and their order, column texts, the Ease column with difficulty, the hidden Stability column |
| card-info C6.3 | `rslib/src/stats/card.rs` (entries with memory state), `rslib/src/scheduler/fsrs/memory_state.rs` | which state each entry shows (the state after the following review for a row between two kept rows) |
| card-info C7 | `ts/routes/card-info/forgetting-curve.ts`, `ts/routes/card-info/ForgettingCurve.svelte`, `ts/routes/card-info/CardInfo.svelte` | when the curve shows, its reviews, the offered ranges and their default, the desired retention line, the tooltip |
| card-info C8 | `qt/aqt/browser/card_info.py` | that Anki's dump follows its internal message layout (Meguri's shape is its own) |
| card-info C9 | `qt/aqt/reviewer.py`, `qt/aqt/browser/card_info.py` | which card is Anki's previous card |
| card-info C11 | `rslib/src/stats/card.rs` | the errors for a missing card, note, note type or deck |
| card-info C12 | `ftl/core/card-stats.ftl` | the English strings |
| card-info C13.1, C13.3 | `pylib/anki/collection.py` (card stats data), `rslib/src/stats/card.rs`, `rslib/src/storage/revlog/time_of_last_review.sql`, `rslib/src/card/mod.rs` (card data) | the oracle call; Anki writing the card row back with an `lrt` taken from the review log when the data has none, `mod` and `usn` kept |

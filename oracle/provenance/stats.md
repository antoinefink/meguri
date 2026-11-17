# Stats specs (U95, U97 to U101, U226, U227, U92)

Spec-writer notes for the files of `specs/stats/`: `day-buckets`,
`counting-rules`, `heatmap-config`, `studied-today`, `heatmap-addon`,
`graphs-common`, `browse-queries`, `card-info`, `graphs-revlog`,
`graphs-cards`, `graph-presentation` and `heatmap-presentation`, and for `specs/conformance/stats-ops.md`.
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
| studied-today T3.1 (history), T3.4, T3.5, T6.2 (2026-11-10 review) | `rslib/src/stats/graphs/mod.rs`, `rslib/src/stats/graphs/today.rs`, `rslib/src/storage/revlog/get.sql`, `rslib/src/storage/revlog/mod.rs` (row decoding), `rslib/src/revlog/mod.rs` (column types) | Today is one part of the graphs response, computed from the same revlog and card reads as every graph; the revlog read starts at the history window, not at today; `usn` (i32), `ease` (u8), `ivl`/`lastIvl` after an integer cast (i32) and `factor` (u32) are decoded strictly and any failure fails the whole response, while `time` and `type` fall back to a default; the day is read through the timing call first |
| studied-today T3.2 (2026-11-10 review) | `rslib/src/search/sqlwriter.rs` (deck terms, required tables), `ts/routes/graphs/RangeBox.svelte` | the screen's deck scope is `deck:current`, which escapes the current deck's name and falls back to the deck named `Default` when the current id names no deck; a typed `deck:` name turns `*` and `_` into wildcards; deck terms read only the cards table (no note join, no queue test); `deck:*` matches every card |
| studied-today T6.3 (2026-11-10 review) | `ftl/core/statistics.ftl`, `ts/routes/graphs/today.ts`, `ts/lib/tslib/i18n/utils.ts`, `rslib/i18n/src/lib.rs`, `Cargo.lock` (`fluent-bundle` 0.16.0), `package.json` (`@fluent/bundle` 0.19.1) | the studied-today message's count reference, unit selection and per-card number are separate inserted parts; the Again line appends its number to a message with no inserted part; the web bundles keep the default isolation; the core bundles isolate unless no locale is given or in tests. The Fluent libraries' placement rule (web: every inserted part of a multi-part pattern; core: the same except message and term references and string literals) is stated from their documented behavior, not read from their sources |
| studied-today T2.4, T9 (2026-11-10 review) | `conformance/scenarios/stats/`, `conformance/goldens/stats/` (listing) | `today/time-wrap` has no scenario file and no golden; every other T9 scenario has both |
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
| graphs-revlog V1 (card state), V2.1 (32-bit key), V3.2 (page clock, d = 0, skipped day), V3.3, V3.4 (no cell, encoding), V5.1, V8 (empty parts), V9 (register, epoch panic), V10 (2026-11-10 review) | `rslib/src/storage/revlog/mod.rs` (graph row reads by card id only, no queue or type filter), `rslib/src/stats/graphs/reviews.rs` and `proto/anki/stats.proto` (day key cast from 64 to 32 bits; `int32` map keys), `rslib/src/stats/graphs/hours.rs` (truncating division and signed remainder; a negative hour index panics), `rslib/src/backend/mod.rs`, `rslib/src/backend/collection.rs`, `rslib/rust_interface.rs` (the service call runs while the collection mutex is held, so the panic poisons it and every later lock, closing included, panics), `rslib/src/stats/graphs/buttons.rs` and `retention.rs` (zero-filled groups and periods), `rslib/src/revlog/mod.rs` (rating and cram tests), `ts/routes/graphs/calendar.ts` and `CalendarGraph.svelte` (a fresh page clock on every render, maximum taken before the year filter, invalid or far years skipped), d3-time 3.1.0, the version Anki's JavaScript lock file pins (day offset through local date arithmetic, hour offset as a fixed span) | the 2026-11-10 judge pass over U99's audit: confirmed rules restated in V1 to V10. The DST and skipped-day cases of V3.2 rows 9 to 11 and V3.3 were checked with a script that applies V3.2's four steps under the ECMAScript rules for local times (gap read with the earlier offset, repeated time as its first occurrence), every 15 minutes of the change dates of Europe/Paris, Australia/Lord_Howe, America/St_Johns and a zone that changes its clocks at midnight, for `R` 0 to 23, and Pacific/Apia's 2011-12-30. The 32-bit keys were computed for ids 1791856800000000, 1791856800000000000 and the largest 64-bit id |
| graphs-cards C1 | `rslib/src/stats/graphs/mod.rs`, `rslib/src/storage/card/mod.rs`, `rslib/src/storage/card/data.rs` | the cards of the scope and the context each card graph reads; `decay` and `lrt` taken from the card data |
| graphs-cards C2, C3 | `rslib/src/stats/graphs/future_due.rs`, `rslib/src/scheduler/new.rs` (home due), `rslib/src/scheduler/filtered/card.rs` (filtered test: `odid` above 0), `rslib/src/scheduler/timing.rs` (the 1,000,000,000 threshold), `ts/routes/graphs/future-due.ts` | the due day per queue (the home deck's columns), buried cards, the daily load, the backlog |
| graphs-cards C3.2 (`prop:due` comparison, 2026-11-10) | `rslib/src/search/sqlwriter.rs` (due property) | `prop:due` chooses day numbers for queues 2 and 3 and timestamps for queues 1 and 4, and matches no other queue |
| graphs-cards C4 | `rslib/src/stats/graphs/card_counts.rs`, `ts/routes/graphs/card-counts.ts` | the counts per state with and without suspended and buried cards set apart |
| graphs-cards C5, C7 | `rslib/src/stats/graphs/intervals.rs` | the interval and stability maps (stability in the Intervals message) |
| graphs-cards C6, C8 | `rslib/src/stats/graphs/eases.rs`, `rslib/src/stats/graphs/mod.rs`, `rslib/src/card/mod.rs` ((d − 1) / 9) | cards with a memory state, the difficulty bins and their median |
| graphs-cards C9 | `rslib/src/stats/graphs/retrievability.rs`, `rslib/src/browser_table.rs` (the day-due test, the due instant for queue 1 or the day formula, seconds since the last review), `rslib/src/timestamp.rs` (second differences), `rslib/src/card/mod.rs`, `rslib/src/stats/graphs/eases.rs` (percent bins), fsrs 6.6.2 `inference.rs` (MIT/BSD, not Anki; the default decay constant and the retrievability function) | elapsed seconds from the card's own columns, the retrievability value with the card's decay or 0.5 (never the preset's), bins, and sums per card and per note grouped by the card's note id, NaN and odd-decay cases |
| graphs-cards C10 | `rslib/src/stats/graphs/added.rs`, `rslib/src/card/mod.rs` (id to seconds), `rslib/src/timestamp.rs` (second differences) | the added day of a card from its id: integer seconds truncated toward zero, the ceiling of the day fraction, no offset read |
| graphs-cards C11, C12 | `proto/anki/stats.proto` | the record members and their protobuf names |
| graphs-cards C9.4 (`sum_by_note` groups, 2026-11-10) | `rslib/src/stats/graphs/retrievability.rs`, `rslib/src/search/sqlwriter.rs` (an empty card search reads the cards table alone), `rslib/src/storage/card/mod.rs` (searched cards) | groups keyed by the card's note id; a card whose note row is missing is still in the Collection scope |
| graphs-cards C3.2, C9.1 (wrapping, 2026-11-10) | `Cargo.toml` (workspace profiles) | no profile turns overflow checks on for release builds, so the 32- and 64-bit differences wrap |
| stats-ops H13 (driver `graph_preferences`) | `rslib/src/stats/graphs/mod.rs` (graph-preferences read), `rslib/src/stats/service.rs`, `rslib/src/config/bool.rs` (keys that default to true), `rslib/src/config/mod.rs` (weekday key and its four stored values), `proto/anki/stats.proto` (`GraphPreferences`) | the backend call the oracle makes and the three members it reports; `browser_links_supported` always set |

## Graph presentation (U226)

| Spec section | Anki files read | Read for |
|---|---|---|
| graph-presentation P1 | `ts/routes/graphs/graph-helpers.ts`, `ts/routes/graphs/GraphRangeRadios.svelte`, `ts/routes/graphs/RangeBox.svelte` | the controls each graph takes and their values |
| graph-presentation P2 | `ts/lib/tslib/i18n/utils.ts`, `ts/lib/tslib/time.ts`, `ts/lib/generated/ftl-helpers.ts` | the page's two number writers and its time spans |
| graph-presentation P3 | `ts/routes/graphs/graph-helpers.ts`, `ts/lib/tslib/time.ts` | the day labels of the axes and tooltips |
| graph-presentation P4 | `ts/routes/graphs/histogram-graph.ts`, `ts/routes/graphs/intervals.ts`, `ts/routes/graphs/future-due.ts`, `package.json` (`d3` 7), the d3 7 scale, tick, bin and quantile modules (ISC, not Anki) | step and ticks, the nice domain, bins, zero-width last bins, quantiles, the bar count, the numbers inside bar queries |
| graph-presentation P5 | `ts/routes/graphs/GraphRangeRadios.svelte`, `ts/routes/graphs/graph-helpers.ts` | the ranges per graph, their defaults, the reset when the history changes |
| graph-presentation P6 | `ts/routes/graphs/future-due.ts`, `ts/routes/graphs/FutureDue.svelte` | bins, table rows, backlog toggle, bar queries |
| graph-presentation P7 | `ts/routes/graphs/calendar.ts`, `ts/routes/graphs/CalendarGraph.svelte`, `ts/lib/tslib/i18n/utils.ts` | the shown year, which dates are drawn and filled; the tooltip date written in the UI languages with long weekday and month names (revised 2026-11-10: it was recorded as English only) |
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
| card-info C1 (card-data rule), C3.1 custom data, C13.3 (no-keys data) (2026-11-10 review) | `rslib/src/storage/card/data.rs`, `rslib/src/serde.rs` (lenient reader) | every card-data key but the custom-data string uses the lenient reader; the custom-data member is a plain string with only a default, so a value of another JSON type fails the whole parse and the column reads as empty data |
| card-info C3.5 steps 1, 2, 4, C7.4 decay (2026-11-10 review) | `rslib/src/storage/revlog/time_of_last_review.sql`, `rslib/src/storage/revlog/mod.rs` (kind read from the low 8 bits), `rslib/src/timestamp.rs` (plain 64-bit second difference), `Cargo.toml` (no overflow checks in release profiles), `rslib/src/scheduler/fsrs/memory_state.rs` (decay from the parameter list, unclamped), `ts/routes/card-info/CardInfo.svelte` (curve decay: 21st value unclamped, 0.5 below 21, 0.1542 for none) | the last-review lookup tests the stored `type` and `factor` columns; the elapsed difference wraps; the decays the core and the page use are the stored values |
| card-info C4.1 rows 12, 13, Ease note, C4.2, C4.3, C5.2, C5.4 (2026-11-10 review) | `ts/routes/card-info/lib.ts`, `ts/lib/tslib/time.ts`, `ts/lib/generated/ftl-helpers.ts` (numbers as Fluent numbers with at most two fraction digits), `ftl/core/scheduling.ftl` (plural selection) | the time rows test the reported total; the Ease row needs a non-zero ease and is written with the number-to-string form; amounts are formatted and pluralized by the Intl number and plural rules (sign kept, negative zero kept, `∞` for infinity, `-1` singular); percents use fixed-point formatting (non-finite and 1e21 and above in number-to-string form). The Intl and ECMAScript behaviors are stated from their published specifications, not read from engine sources |
| card-info C5.1 out of range, negative years (2026-11-10 review) | `ts/lib/tslib/time.ts` (dates from a JavaScript date built from seconds × 1,000, two-digit padding of month, day, hour and minute) | an invalid date gives `NaN` for the year and `aN` for padded parts; the year is the plain astronomical year. The ±8.64e15 ms date range is the ECMAScript one |
| card-info C5.5 (2026-11-10 review) | `ts/routes/card-info/lib.ts` | custom data read with the JavaScript JSON parser, its entries joined as template strings; the number, surrogate and nesting behaviors are ECMAScript's |
| card-info C6.3 (crate call, 2026-11-10 review) | `rslib/src/stats/card.rs`, fsrs 6.6.2 `inference.rs` (MIT/BSD, not Anki) | the per-entry states come from one historical-states call returning the starting state then one state per review |
| card-info C7.2, C7.5, C7.6 (2026-11-10 review) | `ts/routes/card-info/forgetting-curve.ts` | the curve filter tests kind and factor only, never the rating; the page samples from the clock and filters by days since the first review, with no rule for equal instants, a review at the range end, future reviews or span 0 (Meguri's rules are its own); the tooltip date form depends on the range span |
| card-info C9 (queue refresh, 2026-11-10 review) | `qt/aqt/reviewer.py` | the previous card is set on every next-card fetch, which a study-queue refresh also triggers |
| card-info C11, C13.3 order (2026-11-10 review) | `rslib/src/stats/card.rs` (read order: card, note, note type, current deck, review log, then the write, then home deck, preset, memory state, template, model and cutoff), `rslib/src/storage/card/mod.rs`, `rslib/src/storage/card/get_card.sql`, `rslib/src/storage/revlog/mod.rs`, `rslib/src/storage/revlog/get.sql`, `rslib/src/revlog/mod.rs` (strict column types), `rslib/src/storage/revlog/fix_props.sql`, `rslib/src/notetype/mod.rs` (template lookup: first template for a cloze kind), `rslib/src/stats/service.rs` (no transaction around the call), fsrs 6.6.2 `model.rs` (MIT/BSD, not Anki: parameter count check, then a finiteness check) | unreadable card and review-log rows fail the call; the write stays when a later lookup fails; a cloze note type without templates fails; non-finite parameters are refused |

## Heatmap grid presentation (U227)

`stats/heatmap-presentation` was written 2026-11-10 (session
s-20261110T085429Z-165d83d3) from the plan's requirements, the add-on's
installed copy in the project's private local data (Review Heatmap 1.0.1,
add-on id 1771074083; third-party, AGPL, read only, nothing copied) and Anki
at tag 26.09.3. No code, CSS, colour value, asset or identifier of either was
carried into the spec: it gives layout arithmetic, text and Meguri's own
palette rules (R1960). Sections the table does not list (M1, M5's trailing
weeks for the widget, M7.2's pitch rule, M7.3's label fitting, M7.4's
hit-testing, M8.2's palette rules and measurement, M8.4, M11.5, M12's
tooltip panel and delay rule, M13 to M16, M17's Meguri strings, M19, M20)
are Meguri's own design.

| Spec section | Source files read | Read for |
|---|---|---|
| heatmap-presentation M2, M3 | add-on `web/anki-review-heatmap.js` (the bundled calendar library's day, week and year placement, its week-start option and dynamic column count) | rows by weekday from a Monday week start, week columns from January 1's offset, 53 or 54 columns per year, slots outside the year not drawn, no weekday labels and the year label below in year mode |
| heatmap-presentation M4 | add-on `web/anki-review-heatmap.js` (month domain, day labels, label options, the add-on's view script that sets the nine-month range and start), `config.py` (mode table) | month blocks with split weeks, the gap between blocks, the short month name and two-digit year label below each block, one column of single-letter weekday labels before the first block |
| heatmap-presentation M6, M8.3 | Anki `ts/routes/graphs/calendar.ts`, `ts/lib/tslib/i18n/utils.ts` (narrow weekday label, localized date), `ftl/core/statistics.ftl` (`statistics-reviews`) | the calendar's week columns per week start, narrow weekday labels before the grid and their click to set the week start, undrawn dates, the square-root scale and its capped blue range, the week axis of 53 positions with a 54th column drawn in the right margin, the tooltip placed at the pointer and the cells drawn without hover or selection outlines |
| heatmap-presentation M7.1, M7.3 | add-on `web/anki-review-heatmap.js` (cell size and padding, sharp corners, label position) | the add-on's 10 px cells and 2 px spacing that M7.2's ratio follows; square cells; labels centred below |
| heatmap-presentation M8.1, M8.2 | add-on `web/anki-review-heatmap.js` (its bundled style sheet: theme ramps, forecast greys, night-mode reversed order, empty-cell fill), `renderer.py` (level classes, inverted forecast legend) | ten history steps light to dark, reversed at night; ten forecast greys darker for more due in day mode and lighter at night; an empty fill; no legend. No colour value was taken |
| heatmap-presentation M9 | add-on `web/anki-review-heatmap.js` (today and clicked-day highlights, hover stroke, the click handler resetting the highlight to today) | today's outline, a second outline for the clicked day kept with today's, a thin hover outline, an empty-day click leaving only today highlighted |
| heatmap-presentation M10.1, M10.2, M10.3 | add-on `web/anki-review-heatmap.js` (the add-on's tooltip formatter, the library's day date format, connector and number format) | the four tooltip forms and their bold parts, `card` for a count of 1, the minus sign removed from forecast counts, the empty wording chosen by wall clock against the cell's midnight, the English long date without a comma after the weekday, grouped counts |
| heatmap-presentation M11 | add-on `web_content.py` (control row and button titles), `web/anki-review-heatmap.js` (the add-on's navigation handlers, the library's next, previous, jump and rewind with minimum and maximum dates) | Back, Today and Forward centred above the grid; one period per click; Shift-click to the first or last period; Shift-click on Today switching the mode; options and support buttons; no disabled state; the sliding nine-month window clipped at the bounds |
| heatmap-presentation M12 | add-on `web/anki-review-heatmap.js` (tooltip placement and its delayed fade-in), `web_content.py` | the tooltip above the cell, centred, appearing after about a third of a second; a click on a day with data opening the Browser, on an empty day only re-highlighting today |
| heatmap-presentation M18 | the files of the rows above; add-on `activity.py` (report start, stop and today), `finder.py` (the Browser hook for the review-id range), `views.py` (deck-page scope), `web/anki-review-heatmap.js` (the bundled library's year-domain column placement and dynamic column count, its tooltip show and hide on each cell, the stylesheet's empty-cell fills and transparent domain background); Anki `rslib/src/search/parser.rs` (an unknown search key read as a field search) | the add-on and Anki column of each difference: the window start from the wall clock and the day-of-month overflow of its month arithmetic, the first-history bound and the sixth-month bound counted from the report's today, the click searches (review-id range for history days, not recognised after the current-deck term on deck pages; due-day property for forecast days), the empty first column and lost last week in Monday-starting years, the fade-in restarted on every cell, the tooltip always above, grey empty cells |

The add-on's tooltip wording (M10.1) is user-visible text R1320 asks Meguri
to reproduce; it is listed in M17 as R1320 words it. Its long date is not
reproduced (M10.2, M18).

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| card-info C3.1, C3.2, C3.4, C3.5, C3.6, C6.1 scenarios memory-and-retention, no-item, back-to-back-answers | `pylib/tests/test_stats.py`, `rslib/src/stats/card.rs` (test module) | which behaviors the harvested tests require: the card stats report names the note and lists every answer after consecutive answers; a report is built for a stored card; a missing memory state is computed for the report and not stored on the card. Golden values come from the oracle only. |
| card-info C4.1 row rules (harvest rows for Stability/Difficulty/Ease rows; native-ui classification) | `ts/routes/card-info/lib.test.ts` | which row-applicability behaviors are required (FSRS rows with a memory state with or without desired retention; desired retention alone gives no FSRS rows; Ease hidden whenever desired retention or a memory state is present). These were checked against the existing card-info C4.1 text; no scenario values were taken from the test. |
| studied-today T2 Home line format (new scenario line-forms; existing time-units) | `rslib/src/stats/today.rs` (test module) | the requirement that the studied-today line formats count, total time and per-card average across seconds and minutes. The new line-forms scenario uses writer-chosen pairs only. Review found that the existing format-table scenario and the studied-today/stats-ops examples carry two of this test's input pairs and lines; the proposed replacements are in spec_gaps, and format-table itself needs the cleanup listed in concerns. format-table is no longer cited as coverage. |
| Scenario storage/revlog/studied-today-window (specs/stats/counting-rules N6: the Home studied-today count and time) | `rslib/src/storage/revlog/mod.rs`, `rslib/src/storage/revlog/studied_today.sql` | The window start (day cutoff minus one day, strict) and the row types left out of the studied-today count and time sum. |
| stats/graph-presentation P2.2 step 1 (native-ui row) | `ts/lib/tslib/time.test.ts`, `ts/lib/tslib/time.ts`, `ts/routes/congrats/lib.ts` | the natural-unit boundaries (month as a twelfth of 365 days) and where the rule is used outside the charts |

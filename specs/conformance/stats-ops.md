# Stats observations

Spec: conformance/stats-ops
Requirements: R1883, R1447, R1296, R1286, R1288, R1448, R1369, R1370, R1371, R1887, R1248, R1251, R1252, R1425, R1293, R1446, R1886, R1309, R1310, R1311, R1326, R1327, R1328, R1329, R1330, R1331, R1431, R1433
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/

The observation kinds that the stats scenarios need, and how those scenarios build their state. Both executors implement the kinds with the meanings below, except `widget_snapshot`, which only Meguri's has (H12). Their shapes go into `conformance/schema/scenario.schema.json` and `observation.schema.json` with the executor support (H4, H8). The rules they observe are `specs/stats/day-buckets.md`, `specs/stats/counting-rules.md`, `specs/stats/heatmap-config.md` (H1, H2), `specs/stats/studied-today.md` (H5 to H7), `specs/stats/card-info.md` (H9), `specs/stats/graphs-common.md` with `graphs-revlog.md` and `graphs-cards.md` (H10), `specs/stats/heatmap-addon.md` (H11) and `specs/stats/widget-snapshot.md` (H12). H9 to H12 are drafts: no executor has them yet, and each states what lands with its support. H10 is the draft `stats` kind of `observations.md` O4, the Stats screen's graph data.

## H1. `heatmap_totals` (observation kind)

The heatmap's review count per day: the history part of the Home heatmap and of deck-page heatmaps.

- **Request.** `heatmap_totals` gives Home only. `{heatmap_totals: {decks: [<name>, ...]}}` gives Home and each listed deck. Deck names are full names with `::`, as the collection holds them when the step runs. The list has at least one name, each at most once.
- **Value.** `{"home": <days>}`, plus `"decks": {<name>: <days>, ...}` with one member per listed deck when `decks` was requested. `<days>` is an object from a date `YYYY-MM-DD` (`stats/day-buckets` B4) to a positive integer, the number of rows counted on that day. Days with no counted row are absent, and a scope with no counted row is `{}`.
- **Meaning.**
  - `home`: `stats/counting-rules` N2 and N3 under the `heatmap` settings the collection holds when the step runs (`stats/heatmap-config`).
  - each deck: N2 and N4 for that deck.
  - Both: the history windows of N5 when `limhist` or `limdate` is set, and rows after the heatmap's today counted on today (`stats/day-buckets` B4.3).
- **Meguri.** The core's heatmap query API reading the resident aggregates (`stats/counting-rules` N7), the path the Home screen uses. A separate scan of the review log does not count as an implementation.
- **Oracle.** The Review Heatmap add-on 1.0.1's own history computation, the per-day review counts it draws, run headless against the oracle's collection in an add-on oracle image:
  - `home` is its main-screen computation;
  - a deck is its deck-screen computation with that deck as the current deck;
  - only the history counts are reported, before the add-on merges in the forecast and applies its rule for today's cell;
  - each count is reported under the calendar date on which the add-on draws it;
  - the add-on reads the `heatmap` key and `rollover` from the collection at each observation, never from a copy loaded earlier.
- **Read only.** Computing it changes nothing in the collection on either side. That includes the add-on's own config, which it must not write while observed (scenario `stats/heatmap-totals/read-only`).
- **Errors.** A deck name that matches no deck is a run error (`unknown_ref`).
- **Deviations.** `heatmap-future-rows-today` (R1280): Meguri counts a row whose heatmap day is after today on today. `heatmap-drawn-day-shift` (R1270): where the add-on draws a date's count on the date before (`stats/day-buckets` B4.6), Meguri keeps it on its own date. Deck scopes by home deck (R1284) differ only for cards in filtered decks; the unit that adds such a scenario adds its entry.

## H2. `studied_today_raw` (observation kind)

The two numbers the Home "studied today" line is built from.

- **Request.** `studied_today_raw`, with no arguments.
- **Value.** `{"cards": <integer ≥ 0>, "time_ms": <integer>}`: the number of counted rows and the sum of their stored `time` values (`stats/counting-rules` N6), at the clock's current instant.
- **Meguri.** The core's resident studied-today values (N7), with no database query.
- **Oracle.** The count and time Anki's own studied-today line is built from at the current instant, as numbers, never the line's text (R1887). The line's text is `stats/studied-today`'s observation (U97).

## H3. How stats scenarios build their state

- **Review-log rows.** `db_exec` inserts rows with explicit ids (`INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES ...`). That gives exact instants (milliseconds included) and row kinds no answer produces (Manual, Rescheduled, Filtered, future rows). A row whose `cid` matches no card is a deleted card's row, so scenarios that only need Home and studied-today values have no setup. Rows for setup cards select the card ids in `db_exec` (`WITH c(k, id) AS (SELECT ROW_NUMBER() OVER (ORDER BY id) - 1, id FROM cards) ...`). `ivl`, `lastIvl` and `factor` are arbitrary there.
- **Real answers and deletions.** `answer` and `delete_notes` where the scenario is about Meguri's incremental path (`stats/heatmap-totals/answers-and-deletes`).
- **The `heatmap` key.** `set_config` with a whole or partial object. When the key must hold a deck id, which comes from the clock, `db_exec` writes it from the `decks` table (`INSERT OR REPLACE INTO config (KEY, usn, mtime_secs, val) SELECT 'heatmap', -1, <s>, CAST('{...' || id || '...}' AS BLOB) FROM decks WHERE name = ...`).
- **Card moves.** `db_exec` `UPDATE cards SET did = ...`. Stored deck names separate levels with U+001F, so a child deck is `'A' || char(31) || 'B'` in SQL.
- **The rollover hour.** `set_config: {key: rollover, value: <hour>}` as the first step, or `setup.config`.
- **Travel.** A scenario runs in one zone (`dsl.md` D5). A zone change is two scenarios with the same instants in two zones (`stats/day-buckets` B5).
- **Read-only checks.** Anki writes the config key `localOffset` when it computes the day (the first time, and again after the day has changed) or builds Stats-screen data, if the stored value differs from the zone's current offset (a missing value counts as UTC). That write belongs to Anki's day computation, not to the observation. A scenario that compares `db_dump`s around a stats observation therefore observes `timing` first and takes its first dump after that step (`stats/heatmap-totals/read-only`).

## H4. Executor status

- **Oracle.** As of 2026-10-27 the driver registers `studied_today_raw` (and H5 to H7's kinds, H8) but not `heatmap_totals`. `studied_today_raw` runs in the Anki oracle. `heatmap_totals` runs in an add-on oracle image, which adds the Review Heatmap add-on 1.0.1, pinned by hash, to the Anki oracle. `tools/mg oracle record` and `compare`, and the re-recording and `--live` replay of `tools/mg conformance`, run a selection on that image when it has a scenario that observes `heatmap_totals`. Decided 2026-10-27: every other stats scenario (H8's, and any that observes only `studied_today_raw` or H5 to H7's kinds) runs on the plain Anki oracle image, so it needs neither the add-on image nor its copy.
- **Add-on copy.** Decided 2026-10-27: the add-on image is built from a pinned copy of the Review Heatmap add-on that the user provides once, at a path outside `local-data/`, named by the environment variable `MEGURI_HEATMAP_ADDON`. Providing it is a one-time user step, `heatmap-addon`, which `docs/process/one-time-setup.md` lists and `tools/mg doctor` reports while the copy is missing (the infra owner adds both); `tools/mg` never reads `local-data/` for it. `tools/mg oracle test` builds the add-on image and runs its tests only when the copy is present and its hash matches the pin, and reports them skipped otherwise. Until the copy is present with that hash and the driver registers `heatmap_totals`, the scenarios that observe `heatmap_totals` (all of `stats/day-buckets` B7 and `stats/counting-rules` N9), their goldens and the deviation entries `heatmap-future-rows-today`, `heatmap-drawn-day-shift` and `heatmap-deck-home-deck` (`stats/counting-rules` N4) stay held. Nothing else waits for the copy.
- **Scenarios and goldens.** The scenarios of `stats/day-buckets` B7 and `stats/counting-rules` N9 are added together with the oracle support, the schema shapes and the `conformance/deviations.yaml` entries `heatmap-future-rows-today`, `heatmap-drawn-day-shift` and `heatmap-deck-home-deck`. Each gets a golden recorded at the oracle pin of `conformance/oracle.json`. Until both conditions of the add-on copy bullet hold, none of these scenarios, goldens or deviation entries exists; H8's scenarios do not wait for them.
- **Meguri.** U95's implementer adds both kinds to the testkit's scenario model and their adapter (`core/crates/meguri-testkit/src/observe/stats.rs`), as `conformance/AGENTS.md` asks. Until then T0 reports the stats scenarios as `invalid`, as it did for `set_config` (`record-ops.md` W5).
- **Activation.** Once the adapter, the goldens and U40's day clock exist, these scenarios use only ops Meguri already has (`set_config`, `db_exec`, `set_clock`, `advance`, `observe`) and can be listed as active: every `stats/day-buckets/*` scenario, and `stats/heatmap-totals/row-kinds`, `partial-config` and `read-only`. The other `stats/heatmap-totals/` scenarios also need the setup sections `config`, `decks`, `notetypes` and `notes` and the ops `delete_notes` (all of them) and `answer` (`answers-and-deletes`). They become active when those adapters land.

## H5. `studied_today_text` (observation kind)

The Home studied-today line (`stats/studied-today` T2).

- **Request.** `studied_today_text`, with no arguments.
- **Value.** A string: the line at the clock's current instant, for example `"Studied 3 cards in 13 seconds today (4.33s/card)"`.
- **Meguri.** The text of the core's Home studied-today payload, built from the resident values (`stats/counting-rules` N7) with no database query.
- **Oracle.** Anki's studied-today line for the collection (the one its deck list shows), produced in English (`en`), with every U+2068 and U+2069 removed and each line feed replaced by one space (`stats/studied-today` T6.3).
- **Text.** R1887 keeps localized text out of observations. This kind is the exception R1448 requires: the line itself must match Anki's, and the oracle produces it in one fixed language. No other text of Anki's is observed.

## H6. `studied_today_format` (observation kind)

The Home line for given numbers, without review-log rows (`stats/studied-today` T2.2, T5.1).

- **Request.** `{studied_today_format: {inputs: [[<cards>, <time_ms>], ...]}}`: a non-empty list of pairs. `cards` is an integer from 0 to 4,294,967,295; `time_ms` an integer from −9,007,199,254,740,991 to 9,007,199,254,740,991.
- **Value.** A list of strings, one per pair, in order: the line T2.2 gives for those two numbers.
- **Meguri.** The core's Home-line writer applied to the pair.
- **Oracle.** Anki's studied-today message for that card count and `time_ms / 1,000` seconds (a 64-bit float), normalized as in H5.
- It never reads or changes the scenario's collection, so it may appear in any observe step, with or without setup.

## H7. `today_stats` (observation kind)

The values of the Stats screen's Today section for one or more scopes (`stats/studied-today` T3).

- **Request.** `{today_stats: [<scope>, ...]}`: a non-empty list. A scope is one of:
  - `collection`;
  - `{deck: <name>}`: a deck's full name with `::`, as the collection holds it when the step runs;
  - `{search: <text>}`: a search text, passed unchanged (it may be empty or white space only, T3.2).
- **Value.** A list with one object per scope, in request order: `{answers, time_ms, correct, learn, review, relearn, filtered, mature, mature_correct}`, all integers of at least 0, with the meanings of T3.3, at the clock's current instant. `again` is not observed: it is `answers − correct`.
- **Meguri.** The core's Today query for the scope: Collection; Deck with the named deck's id; Search with the text exactly as given (the core itself resolves an empty or white-space text to the Collection scope, T3.2).
- **Oracle.** The `today` part of Anki's Stats-screen graph data, for a history of 365 days (the Today values never depend on it, T3.1):
  - `collection`: the empty search;
  - `{deck: <name>}`: the search Anki's own search builder writes for a deck node with that name;
  - `{search: <text>}`: the text exactly as given.

  Fields: `answers` ← `answer_count`, `time_ms` ← `answer_millis`, `correct` ← `correct_count`, `learn` ← `learn_count`, `review` ← `review_count`, `relearn` ← `relearn_count`, `filtered` ← `early_review_count`, `mature` ← `mature_count`, `mature_correct` ← `mature_correct`. A field the message leaves at its default is 0.
- **No text.** Anki writes the Today lines in its web page, which the oracle does not run. Their tests are `stats/studied-today` T4.4 and T5, in Meguri's unit tests (T6.4).
- **Errors.** A deck name that matches no deck is a run error (`unknown_ref`). A search Anki refuses is a run error (`anki_error`); search errors on the Stats screen belong to U98.
- **Read only.** Computing it changes nothing in the collection on either side, apart from Anki's `localOffset` write (H3, read-only checks).

## H8. Executor status (U97 kinds)

- **Oracle.** As of 2026-10-27 the driver has all three (`oracle/driver/observe/studied_today.py`). They need only the Anki oracle, where `tools/mg` runs them today; once it runs heatmap selections on the add-on image (H4), that image adds nothing they use.
- **Scenarios and goldens.** The scenarios under `conformance/scenarios/stats/studied-today/` and `conformance/scenarios/stats/today/` (`stats/studied-today` T9) exist, each with a golden recorded at the current oracle pin. Their values equal `stats/studied-today` T2.4, T4.4 and T9.
- **Meguri.** U97's implementer adds the three kinds to the testkit's scenario model and the stats adapter (`core/crates/meguri-testkit/src/observe/stats.rs`). Until then T0 and `tools/mg goldens-check` report these scenarios as `invalid` (unknown observation kind), as for H1 and H2. Several scenarios also observe `studied_today_raw` (H2).
- **Activation.** Once the adapters, the goldens and U40's day clock exist: `studied-today/format-table`, `zero-line`, `time-units`, and `today/lines-mix`, `no-mature`, `empty`, `odd-values` and every `today/day-start-*` use only `set_config`, `set_clock`, `db_exec` and `observe`. `studied-today/refresh`, `today/ae53-deleted-card` and `today/deck-subtree` also need the setup sections `config`, `decks`, `notetypes` and `notes`, and `answer` (`refresh`) or `delete_notes` (`ae53-deleted-card`). `today/search-scopes` also needs the `deck:` search term (U82, tag `needs-deck`).

## H9. `card_stats` (observation kind, draft)

The raw values of Card Info for one or more cards (`stats/card-info` C3, C6.1).

- **Request.** `{card_stats: [#cN, ...]}`: a non-empty list of card references.
- **Value.** `{"#cN": <stats>}`. `<stats>` holds the fields of the `CardStatsResponse` message for that card, as the backend returns them at the clock's current instant:
  - `card_id` as `#cN`, `note_id` as `#nN`;
  - `deck`, `original_deck` (`null` when not set), `card_type`, `notetype`, `preset`, `custom_data`: strings;
  - `added`, `first_review`, `latest_review`, `due_date` (epoch seconds), `due_position`: integers, `null` when the optional field is not set;
  - `interval`, `ease`, `reviews`, `lapses`: integers;
  - `average_secs`, `total_secs`, `desired_retention` (`null` when not set), `fsrs_retrievability` (`null` when not set): f32 values written as their shortest f32 decimal;
  - `memory_state`: `{stability, difficulty}` (f32) or `null`;
  - `fsrs_params`: the list of f32 values;
  - `revlog`: the entries in the message's order (newest first), each `{time, review_kind, button_chosen, interval, ease, taken_secs, memory_state, last_interval}`, `review_kind` as its number 0 to 5 and `memory_state` as above or `null`.
- **Oracle.** The backend's card-stats call for the card's id. Scenarios set the config key `fsrs` to true in `setup.config` (Anki computes a missing memory state only with FSRS on, and Meguri always runs it). The frozen clock fixes `now`, including the instant the due estimate adds days to.
- **Meguri.** The raw values of the core's snapshot (`stats/card-info` C3, C6.1) under the same names.
- **Comparison.** Exact, except `memory_state` (top level and per entry) and `fsrs_retrievability`, within a relative 1e-5 under `conformance/observations#O7`'s rule (the row the executor support adds there, `stats/card-info` C13.2).
- **Errors.** A card stats failure is a run error (`anki_error` in the oracle; the matching typed error in Meguri, `stats/card-info` C11).
- **No text.** Anki's Python-side stats backend returns no display text (R1887). If U226's headless graphs-page image lands, a later `card_info_text` kind may record the card-info page's texts; until then `stats/card-info` C4 to C7 are tested from that spec.
- **Anki's write.** Anki may rewrite the card's data while it computes the value (`stats/card-info` C13.3); Meguri never writes. Scenarios follow `stats/card-info` C13.4.
- **Status.** Draft: the driver has no op and the runner no adapter. What lands with the support is `stats/card-info` C13.2; the scenarios are `stats/card-info` C14.1.

## H10. `stats` (observation kind, draft)

The Stats screen's graph data for one or more requests: the draft `stats` kind of `observations.md` O4, which `stats/graphs-common` G14 defines. U99's parts are `stats/graphs-revlog` V8, U101's `stats/graphs-cards` C12. This section gathers them; on a difference, the section that owns the part wins and this one is corrected. Here G, V and C ids are those of `stats/graphs-common`, `stats/graphs-revlog` and `stats/graphs-cards`.

- **Request.** `{stats: [<request>, ...]}`, a non-empty list. A request is `{scope, history, parts}`: `scope` is `collection`, `{deck: <full name with ::>}` or `{search: <text>}` (passed unchanged); `history` is `year` or `all`; `parts` is a non-empty list of the part names below.
- **Value.** A list with one output per request, in order: an object with one member per requested part, or an error `{error: <kind>}` with the search error members of `conformance/search-ops` S3, or `{error: "unreadable_row"}` (G14).
- **Form.** Each part is one message of Anki's graph data, whole, wrapper included, written as JSON with the field names as the protobuf file declares them (`including_inactive`, `newCards`, `rollover_hour`), map keys as decimal strings, and members left at their default omitted (G14). U99's four parts narrow that rule: only scalar members at 0 are omitted; every message member and every list is written, empty or not; an `hours` entry with both counts 0 is written `{}` (V8).
- **Parts.**

  | Part | Shape | Owner |
  |---|---|---|
  | `reviews` | `{count, time}`, each day index → `{learn, relearn, young, mature, filtered}`; also the Calendar's observation | G14, V8 |
  | `true_retention` | `{today, yesterday, week, month, year, all_time}`, each `{young_passed, young_failed, mature_passed, mature_failed}` | V8 |
  | `hours` | `{one_month, three_months, one_year, all_time}`, each a list of 24 `{total, correct}` | V8 |
  | `buttons` | `{one_month, three_months, one_year, all_time}`, each `{learning, young, mature}`, each a list of 4 counts | V8 |
  | `future_due` | `{future_due: {<k>: <cards>}, have_backlog, daily_load}` | C12 |
  | `card_counts` | `{including_inactive, excluding_inactive}`, each `{newCards, learn, relearn, young, mature, suspended, buried}` | G14, C12 |
  | `intervals` | `{intervals: {<ivl>: <cards>}}` | C12 |
  | `stability` | `{intervals: {<rounded s>: <cards>}}` | C12 |
  | `difficulty` | `{eases: {<key>: <cards>}, average}` | C12 |
  | `retrievability` | `{retrievability: {<key>: <cards>}, average, sum_by_card, sum_by_note}` | C12 |
  | `added` | `{added: {<day index>: <number of cards>}}` | G14, C12 |
  | `context` | `{rollover_hour, fsrs}` | G14 |

  With history `year`, the `all_time` members of `true_retention`, `hours` and `buttons` are left out on both sides (V8). The Calendar's dates, `max_count` and item texts, and the Meguri extras of `stats/graphs-cards` C11, are not observed.
- **Oracle.** G14: Anki's graph data for the search text (`collection` → the empty search, `{deck}` → the search Anki's own search builder writes for a deck node with that name, as H7, `{search}` → the text as given) and days (`year` → 365, `all` → 0), after the search text alone has run as a card search; the error mapping is G14's. Its graph data reads Anki's clock, which `set_clock` and `advance` fix (C12).
- **Meguri.** The core's graph request (`stats/graphs-common` G8) with `Collection`, `Deck(<id of the named deck>)` or `Search(<text>)`, mapped into these shapes. A deck name that names no deck is a run error (`unknown_ref`).
- **Comparison.** Numbers compare with the 1e-6 relative tolerance of `conformance/observations` O7; keys and counts compare exactly (C12).
- **Read only.** As H7 (`stats/graphs-common` G11).
- **Status.** Draft: no executor has the kind. The oracle-maintainer adds the driver op (`oracle/driver/ops/stats.py`) with the scenarios of `stats/graphs-common` G15, `stats/graphs-revlog` V10 and `stats/graphs-cards` C13. Meguri's side goes into the testkit's stats adapter (`core/crates/meguri-testkit/src/observe/stats.rs`) with the graph units' executor support; the schema shape lands with that support.

## H11. `heatmap_report` (observation kind, draft)

The Review Heatmap add-on's whole report for a scope, as numbers and dates (`stats/heatmap-addon` A16): the history after its limits, the forecast, today's cell and the four streak values. H1 observes the history counts only, before the forecast and today's rule; this kind observes what the grid and the streak line are built from.

- **Request.** `heatmap_report` gives Home only. `{heatmap_report: {decks: [<name>, ...]}}` gives Home and each listed deck, with names as in H1.
- **Value.** `{"home": <report>}`, plus `"decks": {<name>: <report>, ...}` when `decks` was requested. A `<report>` is `{today, history, forecast, last_forecast_day, today_cell, streaks}`, dates as `YYYY-MM-DD`:
  - `today`: T, the heatmap's today (`stats/heatmap-addon` A1);
  - `history`: date → count of at least 1, the filtered history (A2.1, A2.2: after `limhist` and `limdate`); `{}` when it is empty;
  - `forecast`: date → count of at least 1, for the dates T to T + E (A3); `{}` when no card is counted;
  - `last_forecast_day`: T + E, or `null` when there is no forecast (A3);
  - `today_cell`: `{kind, count}`, `kind` `"today"` or `"future"` (A4);
  - `streaks`: `{daily_average, days_learned, longest_streak, current_streak}`, integers (A8.1), or `null` when the filtered history is empty.

  Colour levels and tints are not observed while the add-on computes them only in its web view, which the oracle does not run (A15 item 14). Decided 2026-10-27: in that case the A16 scenarios `heatmap/levels-history` (AE55), `heatmap/levels-forecast` and `streaks/tints` are not oracle scenarios but Meguri unit tests of A7 and A8.2, with the same inputs and expected levels and tints; a later edit of A16 moves those rows with it. If the oracle-maintainer finds the levels and tints computed outside the web view, the driver support adds them to this value (a `level` per date of `history`, `forecast` and `today_cell`, and a tint per streak value) and those three stay oracle scenarios.
- **Oracle.** The add-on 1.0.1's own report computation, run headless on the add-on image (H4) for the scope (Home, or the deck screen with that deck as the current deck, as H1). Every count, today's cell included, is reported under the calendar date on which the add-on draws it (H1). The add-on reads the `heatmap` key and `rollover` from the collection at each observation.
- **Meguri.** The core's heatmap query (`stats/heatmap-addon` A11) over every date of the history and the forecast rather than one period, reading the resident values and card table (`stats/counting-rules` N7). A separate scan of the review log does not count as an implementation.
- **Comparison.** Exact.
- **Read only.** As H1: the add-on must not write its config while observed.
- **Errors.** A deck name that matches no deck is a run error (`unknown_ref`).
- **Deviations.** Those of H1 (`heatmap-future-rows-today`, `heatmap-drawn-day-shift`, R1284), which also reach the forecast's scope, today's cell and the streaks (`stats/heatmap-addon` A14), and the filtered-deck forecast entry `stats/heatmap-addon` A15 item 10 may add.
- **Status.** Draft: the driver has no op. The oracle-maintainer adds it with the scenarios of `stats/heatmap-addon` A16; the schema shape lands with that support.

## H12. `widget_snapshot` (observation kind, draft, Meguri only)

The content of the widget snapshot (`stats/widget-snapshot` W3, W4).

- **Request.** `widget_snapshot`, with no arguments.
- **Value.** The snapshot object, with exactly the members W3 gives for its `state`, as the core's builder produces it at the clock's current instant for the open collection. Integers and strings only (W1).
- **Meguri.** U100's builder, run in memory. The observation writes no file; the write rules of W6 are core tests.
- **Oracle.** None: the snapshot is Meguri's own format. The scenarios of `stats/widget-snapshot` W9 have Meguri goldens, which U165 also uses as decoding fixtures.
- **Comparison.** Exact.
- **Read only.** Building it never writes the collection (`stats/counting-rules` N8).
- **Status.** Draft: no executor has the kind. U100's executor support adds it, its shape and the way its Meguri goldens are recorded.

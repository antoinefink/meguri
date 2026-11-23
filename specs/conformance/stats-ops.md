# Stats observations

Spec: conformance/stats-ops
Requirements: R1883, R1447, R1296, R1286, R1288, R1448, R1369, R1370, R1371, R1887, R1248, R1251, R1252, R1425, R1293, R1446, R1886, R1309, R1310, R1311, R1326, R1327, R1328, R1329, R1330, R1331, R1431, R1433
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/

The observation kinds that the stats scenarios need, and how those scenarios build their state. Both executors implement the kinds with the meanings below, except `widget_snapshot`, which only Meguri's has (H12). Their shapes go into `conformance/schema/scenario.schema.json` and `observation.schema.json` with the executor support (H4, H8). The rules they observe are `specs/stats/day-buckets.md`, `specs/stats/counting-rules.md`, `specs/stats/heatmap-config.md` (H1, H2), `specs/stats/studied-today.md` (H5 to H7), `specs/stats/card-info.md` (H9), `specs/stats/graphs-common.md` with `graphs-revlog.md` and `graphs-cards.md` (H10), `specs/stats/heatmap-addon.md` (H11), `specs/stats/widget-snapshot.md` (H12), `specs/stats/graphs-common.md` G7 (H13) and `specs/stats/graph-presentation.md` (H14). H9 to H14 are drafts: Meguri's executor has none of them yet but H9 (U92; changed 2026-11-21, implementation review), and each states its oracle status and what lands with its support. H10 is the draft `stats` kind of `observations.md` O4, the Stats screen's graph data.

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
  - each count is reported under the calendar date on which the add-on draws it (drawn date, below);
  - the add-on reads the `heatmap` key and `rollover` from the collection at each observation, never from a copy loaded earlier.
- **Drawn date.** The add-on's calendar places its counts in its web view, which the oracle does not run (`stats/heatmap-addon` A15 item 14). Decided 2026-11-10: the driver takes each date the add-on's computation keys a count by (its B4.1 date, the date it hands its calendar as 00:00 UTC of that date) and moves it to the date `stats/day-buckets` B4.6 gives, with the zone data of the oracle image for the scenario's zone. Counts that land on one date add up, and a date left with no count is absent. The same move applies to the dates of H11's `history` and `forecast`. H11's `today`, `today_cell` and `last_forecast_day` are values of the add-on's computation, written as it computes them, and streak values are its own numbers: none of them is moved. B4.6 records the placement the oracle-maintainer saw in the add-on's web view (`oracle/provenance/`); if a later reading of the web view disagrees, B4.6 and this rule change together and the affected goldens are recorded again.
- **Read only.** Computing it changes nothing in the collection on either side. That includes the add-on's own config, which it must not write while observed (scenario `stats/heatmap-totals/read-only`).
- **Errors.** A deck name that matches no deck is a run error (`unknown_ref`).
- **Deviations.** `heatmap-future-rows-today` (R1280): Meguri counts a row whose heatmap day is after today on today. `heatmap-drawn-day-shift` (R1270): where the add-on draws a date's count on the date before (`stats/day-buckets` B4.6), Meguri keeps it on its own date. `heatmap-deck-home-deck` (R1284): deck scopes by home deck differ only for cards in filtered decks (`stats/heatmap-totals/filtered-deck-card`). Each entry is added with the goldens it transforms (H4).

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

- **Oracle.** As of 2026-11-10 the driver registers `studied_today_raw` (and H5 to H7's kinds, H8) in the Anki oracle, and `heatmap_totals` and `heatmap_report` (H11) only in an add-on oracle image (`oracle/heatmap/`), which adds the Review Heatmap add-on 1.0.1, pinned by hash, to the Anki oracle. `tools/mg oracle record` and `compare`, and the re-recording and `--live` replay of `tools/mg conformance`, run the whole selection on that image when it holds a scenario that observes `heatmap_totals` or `heatmap_report`; `tools/mg` does this since 2026-11-10, and `tools/mg oracle test` without a pattern also runs the add-on image's tests. The add-on image also runs every other kind such a scenario observes, in the same step or another: `studied_today_raw` (most of B7 and N9, `stats/heatmap/today-dst-shift`), `timing` (`stats/day-buckets/paris-spring-forward`) and `db_dump` (`stats/heatmap-totals/read-only`). Decided 2026-10-27: every other stats scenario (H8's, H9's, H10's, and any that observes neither add-on kind) runs on the plain Anki oracle image, so it does not need the add-on image.
- **Add-on source.** As of 2026-11-10 the add-on image installs the add-on's public source, tag v1.0.1 of its repository, pinned by the archive's hash and by each installed file's hash in the oracle tree. It needs no copy from the user and no user step: this replaces the user-provided copy of 2026-10-27 (`MEGURI_HEATMAP_ADDON`, user step `heatmap-addon`), which was never set up. `tools/mg` never reads `local-data/` for it. Nothing else waits for the add-on image.
- **Scenarios.** The scenarios of `stats/day-buckets` B7 (`stats/day-buckets/`), `stats/counting-rules` N9 (`stats/heatmap-totals/`, and `stats/widget-today-count/` for `stats/widget-families` F5) and `stats/heatmap-addon` A16 (`stats/heatmap/`, `stats/streaks/`, `stats/heatmap-config/writer-output`) are prepared and held, not committed: as of 2026-11-17 none is in the tree and none has a golden. No commit holds the current set; the older copies of some of them on the unmerged branch `spec/U95` must not be used. Both kinds have their schema shapes, but `tools/mg goldens-check` and T0 report a scenario of an unknown observation kind as `invalid`, so these scenarios land with the testkit kinds (Meguri, below), inactive, and their goldens are recorded then. On 2026-11-10 the prepared set ran once on the add-on image as a probe whose outputs were discarded (`oracle/provenance/driver-heatmap-addon.md`). `stats/streaks/future-row` then had only a row dated after T and failed on the add-on (`internal_error`, H11 Errors); it has since gained a row on T − 1 and has not run again.
- **Goldens and deviations.** Each scenario's golden will be recorded on the add-on image at the oracle pin of `conformance/oracle.json`. The `conformance/deviations.yaml` entries are added with the goldens they transform; the oracle values below are what the 2026-11-10 probe showed, or what is expected where it did not run:
  - `heatmap-future-rows-today` (R1280): `stats/day-buckets/future-row` (`heatmap_totals`) and `stats/streaks/future-row` (`heatmap_report`, expected, since the probe ran only its first version: the history, 10-13: 1 on the oracle and 10-12: 1 in Meguri; today's cell, `today` 0 on the oracle and `today` 1 in Meguri; and the streaks' `longest_streak` and `current_streak`, 1 and 0 on the oracle and 2 and 2 in Meguri; `daily_average` 1 and `days_learned` 100 are equal);
  - `heatmap-drawn-day-shift` (R1270): `stats/day-buckets/lord-howe-dst-start` (`heatmap_totals`) and the `heatmap_report` history of `stats/streaks/lord-howe-dst-start`. The probe's streaks and today's cell for it equal Meguri's: the add-on computes its streaks on `stats/day-buckets` B4.1 dates (`stats/heatmap-addon` A15 item 13);
  - `heatmap-deck-home-deck` (R1284, `stats/counting-rules` N4): the deck members of `heatmap_totals` in `stats/heatmap-totals/filtered-deck-card` and of the `heatmap_report` forecast in `stats/heatmap/forecast-filtered`;
  - `heatmap-forecast-filtered-odue` (`stats/heatmap-addon` A15 item 10): the Home forecast and last forecast day of `stats/heatmap/forecast-filtered`, and the Home forecast and today's cell of `stats/heatmap/forecast-filtered-day0`, where the 2026-11-10 probe showed the add-on reading a filtered card's `due`. The deck members of `forecast-filtered-day0` equal Meguri's, so that scenario's deck part needs no entry.

  A golden, once recorded, that shows no difference where an entry is named gets no entry, and the spec rows that name it are corrected (`stats/day-buckets` B4.6, `stats/heatmap-addon` A15).
- **Meguri.** U95's implementer adds `heatmap_totals` and `studied_today_raw` to the testkit's scenario model and their adapter, the testkit's stats executor (`core/crates/meguri-testkit/src/exec/stats.rs`; changed 2026-11-22, it named `observe/stats.rs`, which does not exist), and U96's adds `heatmap_report`, as `conformance/AGENTS.md` asks. Until then T0 reports the stats scenarios as `invalid`, as it did for `set_config` (`record-ops.md` W5).
- **Activation.** Once the adapters, the goldens and U40's day clock exist, a scenario can be listed as active when Meguri has every op and setup section it uses:
  - `set_config`, `db_exec`, `set_clock`, `advance` and `observe` only (with `timing` and `db_dump` where named): every `stats/day-buckets/*` scenario; `stats/heatmap-totals/row-kinds`, `partial-config` and `read-only`; `stats/heatmap/limdate-creation`, `limdate-fraction`, `limdate-unit`, `limhist-and-limdate`, `limhist-edge` and `today-dst-shift`; `stats/streaks/ae5-yesterday`, `ae51-current-streak`, `empty`, `future-row`, `gaps`, `limhist-span`, `lord-howe-dst-start` and `single-day`;
  - also the setup sections `config`, `decks`, `notetypes` and `notes`: `stats/heatmap-totals/deck-names`, `deck-scope-move`, `limcdel`, `limdecks-deleted-card`, `limdecks-subtree` and `suspended-buried-rows`; `stats/heatmap/forecast-cap`, `forecast-limfcst`, `forecast-overdue-only`, `forecast-queues`, `forecast-scope`, `limits-odd` and `no-history-forecast`; `stats/streaks/limdecks-home-only` and `rounding-half`; `stats/heatmap-config/writer-output`; `stats/widget-today-count/limdecks-vs-studied`;
  - those sections and `delete_notes`: `stats/heatmap-totals/ae53-deleted-card` and `ae6-manual-rows`, `stats/streaks/deleted-card`;
  - those sections and `answer`: `stats/heatmap/today-cell`; with `delete_notes` too, `stats/heatmap-totals/answers-and-deletes`; with `undo` and `redo` too, `stats/heatmap-totals/answer-undo`;
  - those sections and `rename_deck` and `reparent_decks`: `stats/heatmap-totals/limdecks-rename`;
  - those sections, `reopen` and a filtered `decks` row written as `deck-ops.md` K13 says: `stats/heatmap-totals/filtered-deck-card`, `stats/heatmap/forecast-filtered` and `forecast-filtered-day0`.

## H5. `studied_today_text` (observation kind)

The Home studied-today line (`stats/studied-today` T2).

- **Request.** `studied_today_text`, with no arguments.
- **Value.** A string: the line at the clock's current instant, for example `"Studied 6 cards in 20 seconds today (3.33s/card)"`.
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
- **Meguri.** U97's implementer adds the three kinds to the testkit's scenario model and the stats executor (`core/crates/meguri-testkit/src/exec/stats.rs`; changed 2026-11-22, it named `observe/stats.rs`, which does not exist). Until then T0 and `tools/mg goldens-check` report these scenarios as `invalid` (unknown observation kind), as for H1 and H2. Several scenarios also observe `studied_today_raw` (H2).
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
- **Errors.** A card stats failure is a run error (`anki_error` in the oracle; the matching typed error in Meguri, `stats/card-info` C11), except that a card whose home deck is filtered makes the pinned backend panic and the oracle reports `worker_died`. No scenario/golden covers that panic case; Meguri retains the typed `filtered_home_deck` error of C11. This is distinct from a card currently in a filtered deck whose home deck is normal.
- **No text.** Anki's Python-side stats backend returns no display text (R1887). The graphs-page image (H14) exists, and the card-info page ships in the same page bundle, so a kind that reads that page's texts for the C14.1 cards is possible. It is planned, with no draft yet; it needs its own section here and its own driver module (oracle-maintainer work). Until it lands, `stats/card-info` C4 to C7 are tested from that spec.
- **Anki's write.** Anki may rewrite the card's data while it computes the value (`stats/card-info` C13.3); Meguri never writes. Scenarios follow `stats/card-info` C13.4.
- **Status.** Draft. The oracle driver has the kind (`oracle/driver/ops/stats.py`), with its shape in `conformance/schema/` and its O4 and O7 rows (`stats/card-info` C13.2). As of 2026-11-17 the testkit parses and validates `card_stats` requests and applies the O7 tolerances; the observe executor that builds Meguri's value is missing (U92 adds it). Changed 2026-11-21 (implementation review): U92 (PR #82) adds it, and the 18 scenarios with goldens (all but `damaged`) pass T0 with it and are active (`conformance/scenarios/stats/area.yaml`, `stats/card-info` C13.2). The scenarios of `stats/card-info` C14.1 are under `conformance/scenarios/stats/card-info/`, each with a golden at the current pin (`params`, `zone-dst` and `edge-values` added 2026-11-17), except `damaged`: its two cases are run errors, which a golden cannot hold (`dsl.md` D11), so the driver's self-tests cover them. `ae46-no-state` carries the deviation entries of C13.4.

## H10. `stats` (observation kind, draft)

The Stats screen's graph data for one or more requests: the draft `stats` kind of `observations.md` O4, which `stats/graphs-common` G14 defines. U99's parts are `stats/graphs-revlog` V8, U101's `stats/graphs-cards` C12. This section gathers them; on a difference, the section that owns the part wins and this one is corrected. Here G, V and C ids are those of `stats/graphs-common`, `stats/graphs-revlog` and `stats/graphs-cards`.

- **Request.** `{stats: [<request>, ...]}`, a non-empty list. A request is `{scope, history, parts}`: `scope` is `collection`, `{deck: <full name with ::>}` or `{search: <text>}` (passed unchanged); `history` is `year` or `all`; `parts` is a non-empty list of the part names below.
- **Value.** A list with one output per request, in order: an object with one member per requested part, or an error `{error: <kind>}` with the search error members of `conformance/search-ops` S3, or `{error: "unreadable_row"}` (G14).
- **Form.** Each part is one message of Anki's graph data, whole, wrapper included, written as JSON with the field names as the protobuf file declares them (`including_inactive`, `newCards`, `rollover_hour`), map keys as decimal strings, and members left at their default omitted (G14). U99's four parts narrow that rule: only scalar members at 0 are omitted; every message member and every list is written, empty or not; an `hours` entry with both counts 0 is written `{}` (V8). U101's parts follow G14 with the same rule for message members (`stats/graphs-cards` C12).
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
- **Status.** Draft. As of 2026-10-27 the oracle driver has the kind (`oracle/driver/ops/stats.py`) and its shape is in `conformance/schema/`. The scenarios of `stats/graphs-common` G15, `stats/graphs-revlog` V10 and `stats/graphs-cards` C13 are under `conformance/scenarios/stats/graphs/`, each with a golden at the current pin; V10's `buttons/tz-matrix` is four scenarios, one per zone (`dsl.md` D5), with four more, `buttons/tz-matrix/<zone>-scopes`, that also observe the Deck and Search scopes, and C13's `future-due/tz-matrix` is five, one per zone (`paris-fall-back` included), each observing every card part in the Collection and Deck scopes; those nine and `reviews/time-wrap` were added with their goldens on 2026-11-17. `calendar/ae58-paris-spring-forward` observes only `stats` until the held add-on scenarios land (H4). Meguri's side is U104's: it adds the `stats` kind to the testkit's stats executor (`core/crates/meguri-testkit/src/exec/stats.rs`) from U99's and U101's records (`stats/graphs-revlog` V8). Changed 2026-11-22: this named the graph units and a `observe/stats.rs` adapter that does not exist.

## H11. `heatmap_report` (observation kind, draft)

The Review Heatmap add-on's whole report for a scope, as numbers and dates (`stats/heatmap-addon` A16): the history after its limits, the forecast, today's cell and the four streak values. H1 observes the history counts only, before the forecast and today's rule; this kind observes what the grid and the streak line are built from.

- **Request.** `heatmap_report` gives Home only. `{heatmap_report: {decks: [<name>, ...]}}` gives Home and each listed deck, with names as in H1.
- **Value.** `{"home": <report>}`, plus `"decks": {<name>: <report>, ...}` when `decks` was requested. A `<report>` is `{today, history, forecast, last_forecast_day, today_cell, streaks}`, dates as `YYYY-MM-DD`:
  - `today`: T, the heatmap's today (`stats/heatmap-addon` A1);
  - `history`: date → count of at least 1, the filtered history (A2.1, A2.2: after `limhist` and `limdate`); `{}` when it is empty;
  - `forecast`: date → count of at least 1, for the dates `T + k` with `0 ≤ k < L` (A3); `{}` when no card is counted;
  - `last_forecast_day`: the latest date of `forecast`, or `null` when `forecast` is `{}` (A3);
  - `today_cell`: `{kind, count}`, `kind` `"today"` or `"future"` (A4);
  - `streaks`: `{daily_average, days_learned, longest_streak, current_streak}`, integers (A8.1), or `null` when the filtered history is empty.

  Colour levels and tints are not observed. The add-on computes its cell levels (A7) only in its web view, which the oracle does not run, and its streak tints (A8.2) outside it (A15 item 14). Decided 2026-10-27: the A16 cases `heatmap/levels-history` (AE55), `heatmap/levels-forecast` and `streaks/tints` are not oracle scenarios but Meguri unit tests of A7 and A8.2, with the same inputs and expected levels and tints; A16 lists them with the unit tests, and no scenario file exists for them. If the driver support adds a tint per streak value to this value, `streaks/tints` becomes an oracle scenario with the same inputs; the oracle-maintainer decides. The two levels cases stay unit tests while the levels exist only in the web view.
- **Oracle.** The add-on 1.0.1's own report computation, run headless on the add-on image (H4) for the scope (Home, or the deck screen with that deck as the current deck, as H1). The counts of `history` and `forecast` are reported under the calendar date on which the add-on draws them (H1, drawn date); `today`, `today_cell`, `last_forecast_day` and `streaks` are written as the add-on computes them. The add-on reads the `heatmap` key and `rollover` from the collection at each observation. With an empty filtered history the add-on builds no report (it shows only its no-data message) after running both queries; the oracle then gives `history` `{}`, the forecast its query returned, `streaks` null, and `last_forecast_day` and `today_cell` from that forecast as the add-on's report would take them.
- **Meguri.** The core's heatmap query (`stats/heatmap-addon` A11) over every date of the history and the forecast rather than one period, reading the resident values and card table (`stats/counting-rules` N7). A separate scan of the review log does not count as an implementation.
- **Comparison.** Exact.
- **Read only.** As H1: the add-on must not write its config while observed.
- **Errors.** A deck name that matches no deck is a run error (`unknown_ref`). When every active day of a scope's filtered history is after T, the add-on fails while it computes the report (`stats/heatmap-addon` A14), and the oracle gives a run error (`internal_error`). Scenarios with rows after T therefore also have an active day on or before T. Meguri gives a report there (its first history day is T).
- **Deviations.** Those of H1 (`heatmap-future-rows-today`, `heatmap-drawn-day-shift`, R1284), which also reach the forecast's scope, today's cell and the streaks (`stats/heatmap-addon` A14), and `heatmap-forecast-filtered-odue`, the filtered-deck forecast entry of `stats/heatmap-addon` A15 item 10.
- **Status.** Draft. As of 2026-11-10 the driver has the kind on the add-on image and its shape is in `conformance/schema/`. The add-on computes its streak tints (A8.2) outside its web view but its cell levels (A7) only in it, so the value has neither; adding the tints is open. The scenarios of `stats/heatmap-addon` A16 are held: not in the tree, no goldens (H4). Once they land, `tools/mg` runs them on the add-on image.

## H12. `widget_snapshot` (observation kind, draft, Meguri only)

The content of the widget snapshot (`stats/widget-snapshot` W3, W4).

- **Request.** `widget_snapshot`, with no arguments.
- **Value.** The snapshot object, with exactly the members W3 gives for its `state`, as the core's builder produces it at the clock's current instant for the open collection. Integers and strings only (W1).
- **Meguri.** U100's builder, run in memory. The observation writes no file; the write rules of W6 are core tests.
- **Oracle.** None: the snapshot is Meguri's own format. The snapshot scenarios of `stats/widget-snapshot` W9 are planned Meguri-format expected snapshots (their place and writer are still open, W9), which U165 also uses as decoding fixtures.
- **Comparison.** Exact.
- **Read only.** Building it never writes the collection (`stats/counting-rules` N8).
- **Status.** Draft: no executor has the kind. U100's executor support adds it and its shape. `dsl.md` D11 lets only `tools/mg oracle record` write a golden, so W9's scenarios also wait for a decided way to record Meguri goldens (a change to D11 and `conformance/AGENTS.md`, outside this file). As of 2026-11-17 none of W9's snapshot scenarios exists; `conformance/scenarios/stats/widget/` holds only W9's seven oracle checks of e(D) (`day-start-summer`, `-spring-forward-r2`, `-spring-forward-r4`, `-fall-back`, `-legacy`, `-spring-forward-r2-early-row` and `-future-row`), each with a golden from the Anki oracle. They observe `timing`, `studied_today_raw` and `studied_today_text` and need neither this kind nor the add-on image.

## H13. `graph_preferences` (observation kind, draft)

The three graph preferences as the Stats screen reads them (`stats/graphs-common` G7).

- **Request.** `graph_preferences`, with no arguments.
- **Value.** `{"calendar_first_day_of_week": <integer>, "card_counts_separate_inactive": <boolean>, "future_due_show_backlog": <boolean>}`: the values read from the collection's config when the step runs, defaults included. The weekday is the integer 0, 1, 5 or 6 that `editor/config-keys` K5 reads from `firstDayOfWeek` (0 for a missing key or any other value). Each boolean is G7's reading of `cardCountsSeparateInactive` and `futureDueShowBacklog`: a JSON boolean reads as itself, anything else or no key as `true`. Member names are those of Anki's `GraphPreferences` message; its fourth field, `browser_links_supported`, is left out (Anki always sets it, and it has no Meguri meaning).
- **Meguri.** The G7 values the core's graph result carries (`stats/graphs-common` G8.2) for a Collection-scope request at the clock's current instant, under these names.
- **Oracle.** Anki's graph-preferences read (the backend call that returns `GraphPreferences`), written with the three members above.
- **Comparison.** Exact.
- **Read only.** Reading changes nothing in the collection on either side (G11).
- **Status.** Draft. As of 2026-11-10 the oracle driver has the kind (`oracle/driver/ops/stats.py`) and its shape is in `conformance/schema/`; Meguri's executor does not have it yet. Its one scenario, `stats/presentation/preferences-read` (G15), is planned: as of 2026-11-17 the file is not in the repository and has no golden. It also observes G7's `fsrs` reading through the `stats` kind's `context` part (H10); until it lands, that reading is pinned by `stats/graphs/scope/fsrs-read`. It waits only for Meguri's executor support of this kind, without which `tools/mg goldens-check` reports it `invalid`; then it lands inactive and `tools/mg oracle record` records it on the base image, which has the kind.

## H14. `stats_presentation` (observation kind, draft)

What the Stats screen's graphs page presents from a graph's data: bins with their tooltips and click queries, table rows and the no-data state. `stats/graph-presentation` P22 defines the request, value, oracle and comparison; this section gathers its status. On a difference, P22 wins and this section is corrected.

- **Request.** `{stats_presentation: [{scope, history, graph, controls}, ...]}` (P22).
- **Value.** `[{data, page}, ...]`, one per request (P22), shape `stats_presentation` in `conformance/schema/observation.schema.json`:
  - `data`: an object with the one `stats` part of P22's table (H10's form, schema `graph_parts`).
  - `page` of the chart graphs (Future Due, Reviews, Intervals, Stability, Difficulty, Retrievability, Hourly Breakdown, Answer Buttons, Added): `{no_data, bins, table}`. With `no_data` true, `bins` is empty (the no-data state replaces the plot, P16.5). Each bin is `{x0, x1, value, drawn, tooltip, query}` in axis order, the last undrawn bin included (P4.4). `value` is the bin's value (P4.3 item 4). `tooltip` is a non-empty list of lines, or null for an undrawn bin. `query` is the item text, or null for an undrawn bin and for a drawn bin that opens nothing (P16.1). A line is a string, or `[label, value]` for a label-value line (Reviews, P16.4). Text has U+2068, U+2069 and U+200E removed (P21), and in Hourly Breakdown also the leading emoji and space.
  - Reviews' `value` is the bin's total over the five kinds, from the map shown (milliseconds with `time` true, P8). Hourly Breakdown has 24 bins and Answer Buttons 12, with index edges (x0 = i, x1 = i + 1), every bin drawn, zero counts included, and `query` null (P13, P14).
  - `table`: the rows in order, each a list of cell texts. The label cell has no trailing colon (P6's tables). Trailing empty cells are left out. Retention's first row is its header row, whose leading `""` cell stays (P12).
  - Card Counts: `{no_data: false, bins: [], table, sectors}`. The rows are the type rows, then Total (P9), each `[label, count, percent]`, and Total `[label, count]`. `sectors` holds the seven sector values in row order, 0 for a row not shown.
  - Retention: `{table}`. Calendar: `{no_data}`.
- **Oracle.** The quarantined graphs-page image `oracle/graphs-page/` (`stats/graphs-common` G12, decision 2; R1960). It is a separate image from the Anki oracle and the heatmap add-on image (H4). It runs the graphs page built into the pinned Anki desktop package offscreen, with its backend calls answered from the scenario's collection. Its graph-data call gets the data that `data` holds. Its preferences read gets the collection's preferences with `backlog` and `separate` replaced by the request's values. Its preference writes are dropped, so no write reaches the collection (G11). The page loads fresh for each request at its own defaults, and the oracle then sets the history and the request's other controls. Values are read from what the page shows and from the data behind each drawn element. A tooltip and a query are read by pointing at and clicking each drawn bin, whatever its width on screen (P4.4, P21). A search the backend refuses, or a row it cannot read, is a run error: P22 defines no error value.
- **Meguri.** U226's Swift tests compare the presentation of `data` with `page` (P22).
- **Comparison.** P22: text exact; `data`, `value`, `x0`, `x1` and the sector values within relative 1e-6 (`observations.md` O7).
- **Status.** Draft. As of 2026-11-10 the request and value shapes are in `conformance/schema/`. The graphs-page image and its driver module exist and read Future Due, Intervals, Stability, Difficulty, Retrievability, Added and Card Counts. Reviews, Hourly Breakdown, Answer Buttons, Retention and the Calendar fail as `unsupported_observation` before any collection (`oracle/graphs-page/README.md` lists what they need). P8's `value`, P12's header row and the index edges of P13 and P14 (decided 2026-11-17) are the spec decisions the probe needed for those graphs. Meguri's executor does not have the kind, so T0 and `tools/mg goldens-check` would report any such scenario as `invalid` (unknown observation kind). `tools/mg oracle record` does not yet run a selection on the graphs-page image, and the golden format has no member for that image's aqt lock (`conformance/dsl` D11). As of 2026-11-17 P23's fifteen scenarios are planned: none is in the repository and none has a golden. On 2026-11-10 copies of the eight on the graphs the image reads (`added-ranges`, `card-counts-table`, `difficulty-ranges`, `future-due-large-backlog`, `future-due-ranges`, `intervals-quantiles`, `retrievability-narrow`, `stability-quantiles`) were run by hand on the image from a scratch directory. Two runs gave the same bytes, and they matched the worked examples of P6, P10, P11 and P15. Neither the scenario files nor those outputs were committed, so the scenarios must be written again from P23 (goldens come only from `tools/mg oracle record`).

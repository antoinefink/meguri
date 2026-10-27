# Stats observations

Spec: conformance/stats-ops
Requirements: R1883, R1447, R1296, R1286, R1288, R1448, R1369, R1370, R1371, R1887
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/

The observation kinds that the stats scenarios need, and how those scenarios build their state. Both executors implement the kinds with the meanings below. Their shapes go into `conformance/schema/scenario.schema.json` and `observation.schema.json` with the executor support (H4, H8). The rules they observe are `specs/stats/day-buckets.md`, `specs/stats/counting-rules.md`, `specs/stats/heatmap-config.md` (H1, H2) and `specs/stats/studied-today.md` (H5 to H7). The `stats` kind of `observations.md` O4 stays a draft for the Stats screen's graph data.

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

- **Oracle.** As of 2026-10-27 the driver has neither kind. `studied_today_raw` runs in the Anki oracle. `heatmap_totals` runs in an add-on oracle image, which adds the Review Heatmap add-on 1.0.1, pinned by hash, to the Anki oracle. `tools/mg oracle record` and `compare`, and the re-recording and `--live` replay of `tools/mg conformance`, run a selection that has a stats scenario on that image.
- **Scenarios and goldens.** The scenarios of `stats/day-buckets` B7 and `stats/counting-rules` N9 are added together with the oracle support, the schema shapes and the `conformance/deviations.yaml` entries `heatmap-future-rows-today` and `heatmap-drawn-day-shift`. Each gets a golden recorded at the oracle pin of `conformance/oracle.json`. Until then no stats scenario, golden or deviation entry exists.
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

- **Oracle.** As of 2026-10-27 the driver has none of the three. They need only the Anki oracle; `tools/mg` runs them on the add-on image like every stats scenario (H4), which adds nothing they use.
- **Scenarios and goldens.** The scenarios under `conformance/scenarios/stats/studied-today/` and `conformance/scenarios/stats/today/` (`stats/studied-today` T9) are added with that support, each with a golden recorded at the current oracle pin. Their values equal `stats/studied-today` T2.4, T4.4 and T9.
- **Meguri.** U97's implementer adds the three kinds to the testkit's scenario model and the stats adapter (`core/crates/meguri-testkit/src/observe/stats.rs`). Until then T0 and `tools/mg goldens-check` report these scenarios as `invalid` (unknown observation kind), as for H1 and H2. Several scenarios also observe `studied_today_raw` (H2).
- **Activation.** Once the adapters, the goldens and U40's day clock exist: `studied-today/format-table`, `zero-line`, `time-units`, and `today/lines-mix`, `no-mature`, `empty`, `odd-values` and every `today/day-start-*` use only `set_config`, `set_clock`, `db_exec` and `observe`. `studied-today/refresh`, `today/ae53-deleted-card` and `today/deck-subtree` also need the setup sections `config`, `decks`, `notetypes` and `notes`, and `answer` (`refresh`) or `delete_notes` (`ae53-deleted-card`). `today/search-scopes` also needs the `deck:` search term (U82, tag `needs-deck`).

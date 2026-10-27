# Conformance observations and comparison

Spec: conformance/observations
Requirements: R656, R1884, R1886, R1887, R1888, R1889, R1927
Anki: 26.09.3
Scenarios: conformance/scenarios/smoke/

What each observation kind records, how values are written, and how the runner compares Meguri's observations with a golden. The scenario language is `dsl.md`; value schemas are `conformance/schema/observation.schema.json`.

## O1. Observations of a run

- A run's observations are `{"steps": [<output of step 0>, <output of step 1>, ...]}`, one output per scenario step, so `/steps/<i>/...` points into step `i`.
- An op step's output is `{}`, `{<op>: <op output>}` (`dsl.md` D7.2) or a rejection `{<op>: {"error": <kind>}}` (`dsl.md` D7.3). An observe step's output is `{<kind>: <value>, ...}` with one member per requested kind. A `handoff` step's output is `{}`.
- Values are plain JSON keyed by logical ids (`#nN`, `#cN`). A real id appears only in the `ids` kind, or where a value is a real id by definition (revlog row ids are review instants, fuzz-table card ids are inputs, a `db_dump` holds the stored rows, and so do `file_col` and `file_query` for a session file, `decks` reports each deck's id and preset id).
- In an actor scenario (`sync-ops.md`), an observe step runs on the current actor's collection (S2.3).
- No localized text is ever an observation (R1887): errors and results are kinds, counts and enums. Driver-authored English (`detail` of a protocol error) is never compared. The one exception is the English studied-today line, which R1448 requires to match Anki's text (`studied_today_text`, `studied_today_format`; `stats-ops.md` H5).

## O2. Requesting observations

`observe` takes a list. Each request is one of:

- `kind`: the kind with no arguments; for per-card kinds, every card in logical-id order;
- `kind(#c1, #c3)`: the kind for these references, in the order given;
- `{kind: <arguments>}`: kinds that take structured arguments.

Requesting a per-card kind twice in one step merges the two results when they name different cards; naming a card twice in one step is invalid. A kind whose value is a list (`template`, `review_html`) may be requested once per step.

## O3. Pointers and patterns

- A pointer is an RFC 6901 JSON pointer into the observations: `/steps/3/card/#c1/due`. `~` is written `~0` and `/` is written `~1` inside a segment.
- A pointer pattern (used by `compare.set` and deviations) is a pointer whose segments may be `*`, which matches any one segment.

## O4. Kinds

Kinds marked *draft* have no driver support yet; the owning area defines their values with the driver op, in this file and the schema.

| Kind | Arguments | Value |
|---|---|---|
| `ids` | note and card references, or none for all | `{"#nN": <note id>, "#cN": <card id>}`: real ids, allocated by `specs/storage/ids-usn-mtime.md` I2: the clock's instant in milliseconds, or the table's largest id plus one when that instant is taken |
| `timing` | none | `{days_elapsed, next_day_at, now, utc_offset_secs}`: the scheduler's day number, the next day boundary and the current instant in epoch seconds, and the local UTC offset now (seconds east). Observing it runs one day query with its writes and unbury check (`sched/day-clock#D5`, `#D7`, `#D14.2`) |
| `card` | card references or none | `{"#cN": <row> \| null}`: the stored card row with columns `id` (as `#cN`), `nid` (as `#nN`), `did` and `odid` (deck names; `null` for 0), `ord`, `mod`, `usn`, `type`, `queue`, `due`, `ivl`, `factor`, `reps`, `lapses`, `left`, `odue`, `flags` and `data` (parsed JSON object; `{}` when empty; the text when it is not JSON) |
| `revlog` | card references or none | `{"#cN": [<row>, ...]}`: the card's review-log rows in id order, columns `id`, `cid` (as `#cN`), `usn`, `ease`, `ivl`, `lastIvl`, `factor`, `time`, `type` |
| `next_states` | card references or none | `{"#cN": {current, again, hard, good, easy}}`: the scheduling state now and after each rating (O5) |
| `fuzz_table` | `{card: #cN, pairs: [[card_id, reps], ...], intervals: [days, ...]}` | `{intervals, rows: [[card_id, reps, delta, ...], ...]}`: the interval fuzz applied when answering a card with that id and review count (reviews before the answer), one delta in days per interval, bounded by the card's preset (`sched/fuzz#Z6`). A pair's card id must not be the id of another card of the collection and its review count must be at most 4,294,967,294: the oracle computes the table by giving the card that id and count + 1 |
| `answer_labels` | card references or none | `{"#cN": {again, hard, good, easy}}`: the answer button labels now, each as its parts (O10) |
| `net_probe` | `{host, port, timeout_ms?}` | `{connected, error}` (diagnostic, oracle only: proves the worker has no route out) |
| `schema_objects` | none | `[{type, name, tbl_name, sql}]`, every object of the collection file's `sqlite_master` with its SQL text normalized (`storage-ops.md` P3) |
| `queue` | none | `{cards, kinds, counts}`: current-deck presentation in full ordered ids, each kind and `{new, learning, review}` counts (`queue-ops.md` Q2) |
| `deck_counts` | none | `[{name, new, learning, review, children}]`: ordered count tree after the explicit compatibility prelude (`queue-ops.md` Q3); production uses D8 automatic maintenance |
| `congrats` | none | `{learn_remaining, secs_until_next_learn, review_remaining, new_remaining, have_sched_buried, have_user_buried}`: the six compatibility facts, including all queue-1 cards (`queue-ops.md` Q4) |
| `deck_tree` | none | `[{name, children}]`: the deck tree's top-level nodes in order, nested (`deck-ops.md` K12); `deck_counts` reports counts separately |
| `decks` | none | `{<name>: {id, filtered, config_id, study_collapsed, browser_collapsed, mtime_secs, usn}}`: every deck by its stored human name (`deck-ops.md` K11) |
| `render` | card references or none | `{"#cN": {review, browser} \| null}`, the card render pipeline's outputs for the stored templates: question and answer text with play markers, AV entries, blank kind or template error, in review and in browser mode (`render-ops.md` Q5) |
| `render_status` | card references or none | `{"#cN": {ord, status} \| null}`: whether the card renders for review from its stored templates, `status` `ok`, `empty` (blank question) or `template_error`, with no text or length (`render-ops.md` Q4) |
| `template` | `{notetype?, cases: [...]}` | `[<result>, ...]`, one per case: `{question, answer, empty}` or `{error: {kind, side}}`, the template engine's output for given template texts on a note not yet added or an existing card (`render-ops.md` Q1); at most one request per step; never changes the scenario's collection |
| `image_occlusion` | `{fns: [<function>, ...], inputs: [<input>, ...]}` | `{<function>: [<output>, ...], ...}`: Anki's raw reading of an Occlusion field and its image-occlusion field roles (`image-occlusion-ops.md` J1); never reads or changes the scenario's collection |
| `add_defaults` | `{review_card?: #cN}` | draft: `{deck, notetype}` (names) or an error kind, the Add screen's default deck and note type (`notes/add-defaults` D7); never changes the collection |
| `notetype_last_deck` | `{notetype: <name>}` or `{notetype_id: <integer>}` | draft: `{deck: <name> \| null}`, the deck last used with that note type (`notes/add-defaults` D7); never changes the collection |
| `fields_check` | `{cases: [{notetype, fields} \| {note: #nN, fields?}, ...]}`, at most one request per step | `[{state, duplicates?}, ...]`, one per case: the field check of `specs/notes/field-check.md` on unsaved drafts, `duplicates` (logical note ids) only with the state `duplicate` (`note-ops.md` N4); never changes the collection |
| `search` | list of `{query, kind?: cards \| notes, order?: {column, reverse?}}` | `search-ops.md` S1: one output per search, `{ids: [#cN or #nN, ...]}` in logical-id order, or an error `{error: <kind>, provided?, context?}`; with `order` (`browser-ops.md` B1, B2), the sorted ids with equal-key runs in logical-id order, or also `{error: "unsortable"}` |
| `browser_rows` | `{columns: [<column key>, ...], rows: [<reference>, ...]}` | `browser-ops.md` B3, B4: one entry per reference, `{tint, cells: {<column key>: <cell>}}` or `{error: deleted \| damaged}`, cards or notes as `browserTableShowNotesMode` says; at most one request per step |
| `search_build` | a list of built search items in the neutral form of `search/join-and-replace` J11 | draft (search): one output per item, `{text}` or a parse error (`search/join-and-replace` J3, J4, J11); never reads or changes the scenario's collection |
| `search_join` | a list of `{existing, item, joiner, negate}` | draft (search): one output per case, `{text}` or a parse error (`search/join-and-replace` J6, J11); never reads or changes the scenario's collection |
| `search_replace` | a list of `{existing, item}` | draft (search): one output per case, `{text}` or a parse error (`search/join-and-replace` J7, J11); never reads or changes the scenario's collection |
| `canonical_search` | list of search texts | `search-ops.md` S2: one output per text, `{text: <canonical text>}` or a parse error; never reads or changes the scenario's collection |
| `card_extractors` | `{inputs: [{id, data, due, ivl, today, now}, ...]}`, at most once per observe step | `{rows: [{id, s, d, dr, r, rr, pos}, ...]}`: the card-data SQL functions of `specs/search/card-extractors.md` on given SQL values, one row per input (`extractor-ops.md` E1–E3); never reads or changes the scenario's collection |
| `db_dump` | none or `{tables?: [...], raw?: bool}` | `{<table>: {<key>: <row>}}`: the stored rows of the collection's tables, normalized (O8), or raw (`normalized-dump.md` N4) |
| `stats` | `[{scope, history, parts}, ...]` | draft (stats): `[{<part>: <graph data>} \| {error}, ...]`, the Stats screen's graph data per request (`stats-ops.md` H10) |
| `stats_presentation` | `[{scope, history, graph, controls}, ...]` | draft (stats): `[{data, page}, ...]`, a graph's data and what the graphs page presents from it: no-data state, bins with tooltips and click queries, table rows (`stats/graph-presentation` P22) |
| `card_stats` | a non-empty list of card references | draft (stats): `{"#cN": <stats>}`, the raw values of Card Info (`stats-ops.md` H9) |
| `heatmap_totals` | none, or `{decks: [<name>, ...]}` | draft (stats): `{home: {<YYYY-MM-DD>: <count>}, decks?: {<name>: {...}}}`, the heatmap's review count per day for Home and the listed decks (`stats-ops.md` H1; oracle: the add-on image, H4) |
| `studied_today_raw` | none | `{cards, time_ms}`: the count and time the Home studied-today line is built from (`stats-ops.md` H2) |
| `studied_today_text` | none | the Home studied-today line, an English string (`stats-ops.md` H5) |
| `studied_today_format` | `{inputs: [[<cards>, <time_ms>], ...]}` | `[<line>, ...]`: the Home line for each pair, without review-log rows (`stats-ops.md` H6) |
| `heatmap_report` | none, or `{decks: [<name>, ...]}` | draft (stats): `{home: <report>, decks?: {<name>: <report>}}`, the heatmap add-on's history, forecast, today's cell and streaks (`stats-ops.md` H11) |
| `widget_snapshot` | none | draft (stats, Meguri only): the widget snapshot's content (`stats-ops.md` H12) |
| `today_stats` | `[collection \| {deck: <name>} \| {search: <text>}, ...]` | `[{answers, time_ms, correct, learn, review, relearn, filtered, mature, mature_correct}, ...]`: the Stats Today section's values per scope (`stats-ops.md` H7) |
| `digest` | none | draft: a hash over all card rows, for fuzzing |
| `ignored_before_count` | `{preset, dates: [<string>, ...]}`, at most once per observe step | `[{included, total} \| {error: "invalid_date"}, ...]`, one per date: the count behind "N of M cards included" for that date text (`deck-options-ops.md` E2, `sched/memory-state` M5); never changes the collection |
| `legacy_json` | `{notetypes?, decks?, presets?}`: lists of names | `{notetypes?: {<name>: <object>}, decks?: {...}, presets?: {...}}`, the schema-11 objects of the records as stored now (`model-ops.md` L1) |
| `blob_decode` | `{notetypes?, decks?, presets?}`: lists of names | `{notetypes?: {<name>: "ok" \| "invalid_blob"}, decks?: {...}, presets?: {...}}`, whether each record's protobuf blobs decode now (`model-ops.md` L4) |
| `notetype_templates` | `{notetypes: [<name>, ...]}` | `{<name>: [{name, front, back, browser_front, browser_back}, ...]}`, the stored template texts of note types the scenario wrote (`notetype-structure-ops.md` M3); never changes the collection |
| `notetype_structure` | none, or `{notetypes: [<name>, ...]}` | `{<name>: {kind, original_stock_kind, original_id, sort_field, fields, templates}}`: note types without template text, CSS or ids (`notetype-ops.md` T8) |
| `package_col` | `{name, columns}`: a package file an `export` step wrote, and a list of `col` columns | `{<column>: <value>}`: the `col` row of the legacy package's collection, `ver` an integer and the JSON columns parsed (`model-ops.md` L7) |
| `file_col` | `{name, columns}`: a session file a `schema11_copy` step wrote, and a list of `col` columns | `{<column>: <value>}`: the file's `col` row, integers as stored and the JSON columns parsed (`storage-ops.md` P5) |
| `file_objects` | `{name}`: a session file a `schema11_copy` step wrote | `[{type, name, tbl_name, sql}]`, as `schema_objects` for that file (`storage-ops.md` P6) |
| `file_query` | `{name, sql}`: a session file a `schema11_copy` step wrote, and one statement that only reads | `{rows: [[...], ...]}`, as `db_exec` writes rows (`storage-ops.md` P7) |
| `package_media` | `{name}` or `{fixture}`, as `package_listing` | `[{name, size, sha1, entry_size}, ...]`, the package's media map with each numbered entry's decoded size (`package-ops.md` E10); never changes the collection |
| `package_db_dump` | `{name \| fixture, tables?, raw?}` | the `db_dump` value (O8) of the package's collection entry, read with plain SQLite (`package-ops.md` E11); never changes the collection |
| `package_listing` | `{name}` (a package in the session directory) or `{fixture}` (a fixture package) | `{entries, meta_version, sqlite}`: the archive's entries with their compression methods, the `meta` version, and for each raw SQLite entry its `ver`, row counts, journal mode and whether the executor opens it (`package-ops.md` E3); never changes the collection |
| `text` | `{fns: [<function>, ...], inputs: [<input>, ...]}` | `{<function>: [<output>, ...], ...}`: pure text functions (O9); never reads or changes the scenario's collection |
| `tag_complete` | `{cases: [{input, limit}, ...]}` | draft: `[[<tag name>, ...], ...]`, one list of completions per case, in the order returned (`tags/complete` C7); never changes the collection |
| `tag_tree` | none | `[{name, collapsed, children}, ...]`: the tag tree of `specs/tags/tags.md` T13, top-level nodes first (`tag-ops.md` G9) |
| `undo_status` | none | `{undo, redo}`: the step an `undo` and a `redo` would apply now, each a step id or `null` (`undo-ops.md` V2, V4); reading it changes neither history |
| `cloze` | `{fns: [<function>, ...], inputs: [<input>, ...]}` | `{<function>: [<output>, ...], ...}`: cloze numbers, `cloze:` and `cloze-only:` output and type-in extraction (`cloze-ops.md` Z1); never reads or changes the scenario's collection |
| `type_answer` | `{fns: [<function>, ...], inputs: [[<expected>, <typed>], ...]}` | `{<function>: [<output>, ...], ...}`: the type-in comparison HTML, with combining characters counted (`compare`) or ignored (`compare_nc`) (`type-answer-ops.md` K1); never reads or changes the scenario's collection |
| `review_html` | `[{card: #cN, typed?}, ...]` | `[{question, answer}, ...]`, one per case: the HTML review shows for a stored card, media names encoded and the type-in box and comparison applied, play markers kept and warnings as tokens (`type-answer-ops.md` K2); at most one request per step; never changes the scenario's collection |
| `fsrs_next` | `{param_sets: {<name>: [<number>, ...]}, inputs: [<input>, ...]}`, at most once per observe step | `{rows: [<row>, ...]}`: the FSRS next states of a review card per input, as twelve values (days, stability and difficulty for again, hard, good and easy); never reads or changes the scenario's collection (`float-determinism.md` F3, F5) |
| `media_dir` | none | `{<name>: {sha1, size} \| {dir: true}}`: the media folder's entries as listed (`media-ops.md` F9) |
| `media_trash` | none | `[{name, sha1, size}]`: the media trash's entries, sorted by name, then sha1 (`media-ops.md` F10) |
| `media_pending` | none | `{<name>: <sha1> \| null}`: after a full registration of the folder, the entries pending for the next media sync, `null` for a deletion (`media-ops.md` F11) |
| `media_quick_check` | none | `{<name>: <sha1> \| null}`: after the change check a media sync begins with, which lists nothing while the folder's time equals its marker, the entries pending for the next media sync (`media-sync-ops.md` V1) |
| `browser_config` | none | `{<key>: <value>, ...}`: the 23 browser and search config keys as Anki's own reads give them, defaults included (`browser-config-ops.md` B2) |
| `card_excerpts` | a list of card references, repeats allowed; at most once per observe step | `[{question, deck} \| {error: missing \| damaged}, ...]`, one entry per reference in request order: the cards' Question and Deck cells from the card excerpts call of `specs/browser/columns.md` C16 (`card-excerpt-ops.md` E1, E2); never changes the collection |
| `backup_files` | none | `[<file name>, ...]`: the regular files of the scenario's backups folder, sorted by their UTF-8 bytes (`backup-ops.md` F5); never changes the folder |
| `backup_settings` | none | `{daily, weekly, monthly, minimum_interval_mins}`: the backup limits as read now, defaults included (`backup-ops.md` F6); never writes |

## O5. Scheduling states

`next_states` values follow the scheduler's protobuf state messages, flattened. A held scenario load-balancer cache participates under `answer-ops.md` H1–H4; the request and output shapes do not change:

- a state is `{kind, ...fields}` with `kind` one of `new`, `learning`, `review`, `relearning`, `preview`, `rescheduling`;
- `new`: `position`; `learning`: `remaining_steps`, `scheduled_secs`, `elapsed_secs`, `memory_state?`; `review`: `scheduled_days`, `elapsed_days`, `ease_factor`, `lapses`, `leeched`, `memory_state?`; `relearning`: `learning` and `review`, each the matching state without `kind`;
- `memory_state` is `{stability, difficulty}`, present only with FSRS on;
- labels are not part of `next_states`; the `answer_labels` kind reports them (O10).

## O6. Numbers

- Integers are written as integers.
- A value the engine keeps as 32-bit float (memory states, ease factors, desired retention) is written as the shortest decimal that parses back to the same f32, chosen as Ryu does (fewest digits, then nearest, ties to even), as a JSON number with a fraction or exponent: 11.262237, never the widened 11.262236595153809; 1.0, never 1.
- A 64-bit float is written as its shortest round-trip decimal.

## O7. Comparison

The runner compares the golden's observations (expected) with the executor's (actual) after applying, on macOS, the scenario's Apple overlay to expected (`float-determinism.md` F9), then deviation transforms to expected (`dsl.md` D13), and set comparison to both (`compare.set`).

- Objects compare member by member: a member on one side only is a difference, shown as `<absent>` on the other side.
- Arrays compare item by item; extra items on either side are differences. Under `compare.set` the two arrays are first sorted by the JCS text of their items, ties broken by a text that keeps the number kind (JCS writes 1 and 1.0 alike), so two equal multisets always line up.
- Strings, booleans, null and HTML compare exactly. Integers compare exactly. Order is significant everywhere except under `compare.set`.
- A number compares exactly, kind included (1 and 1.0 differ), unless a tolerance below applies.
- Tolerances (R1886), relative, `|actual − expected| ≤ tol × max(|expected|, |actual|)`, and only when both values are written with a fraction or exponent (an integer on either side compares exactly):

  | Where | Tolerance |
  |---|---|
  | `stability` and `difficulty` in any `memory_state` inside `next_states` | 1e-5 |
  | the stability and difficulty columns (1, 2, 4, 5, 7, 8, 10, 11) of `fsrs_next` rows | 1e-5 |
  | any number inside `stats` | 1e-6 |
  | `r` and `rr` in `card_extractors` rows (`extractor-ops.md` E4) | 1e-5 |

  Card rows, review-log rows and every other kind are exact: a stored value that a tolerance could hide is a real difference.

  These tolerances compare Meguri with the expected values. The `fp-libm-boundary` class (`float-determinism.md` F8.3) allows a wider stability difference between the Linux and Apple oracles. Such a difference reaches this comparison only as an overlay value, never as a tolerance (`float-determinism.md` F11.3).
- The first divergence is the smallest step index among the differing pointers. Differences are reported in pointer order (members in key order, items in index order).

## O8. `db_dump`

`db_dump` reports the stored rows of the current collection: `{"db_dump": {<table>: {<key>: {<column>: <value>, ...}, ...}, ...}}`.

- Tables: the twelve tables of a schema-18 collection file: `col`, `notes`, `cards`, `revlog`, `graves`, `decks`, `deck_config`, `notetypes`, `fields`, `templates`, `tags` and `config`. A bare `db_dump` reports all twelve; `{db_dump: {tables: [...]}}` reports the listed ones, each listed at most once. Other tables in the file (SQLite's own, an executor's own) are never reported.
- A table is an object of its rows, keyed by the row's primary key as text, integers in decimal:

  | Table | Key |
  |---|---|
  | `col`, `notes`, `cards`, `revlog`, `decks`, `deck_config`, `notetypes` | `id` |
  | `graves` | `<oid>,<type>` |
  | `fields`, `templates` | `<ntid>,<ord>` |
  | `tags` | `tag` |
  | `config` | `KEY` |

- A row holds every column of its table, named as the schema declares it (so `config`'s key column is `KEY`). A value follows the SQLite storage class of the stored value, not the column's declared type (a sort field stored as an integer is an integer):

  | Storage class | Value |
  |---|---|
  | INTEGER | integer |
  | REAL | number with a fraction or an exponent, the shortest decimal that reads back as the same 64-bit float (so REAL 1 is `1.0`) |
  | TEXT | string |
  | BLOB | `{"sha256": "<lower-case hex SHA-256 of the bytes>"}` |
  | NULL | `null` |

- A BLOB is reported by its digest, never by its bytes. Equal digests mean equal bytes. The protobuf and JSON blobs of a new collection hold Anki's stock note type templates and CSS, which stay out of goldens (R1959).
- Anki draws some stored values at random, so they differ in every run. The observation hides them, so a golden stays valid across runs:
  - `notes.guid` is the string `#random`;
  - the digest of `fields.config` covers the stored bytes without the field's random id, top-level protobuf field 9, and the digest of `templates.config` covers them without the template's random id, top-level protobuf field 8. Removing a field removes every occurrence of that top-level field number, tag and value, and keeps the other bytes in their order. The blob parses when it is a sequence of fields, each a varint tag with a field number of at least 1 and a wire type of 0 (varint), 1 (8 bytes), 2 (a varint length, then that many bytes) or 5 (4 bytes), that ends exactly at the blob's end. A blob that does not parse is digested as stored.

  `converge` compares the raw values instead (`sync-ops.md` S7.2), so actors that disagree on a random value still differ there.

  `{db_dump: {raw: true}}` reports the raw values too (`normalized-dump.md` N4): scenarios that start from a fixture, whose values are fixed, use it to show that they survive byte for byte.
- Real ids appear as stored. Meguri allocates ids as Anki does (`specs/storage/ids-usn-mtime.md` I2), so a Meguri collection's dump has the same ids as the oracle's, except where Meguri's one-day cap applies (I3, with its deviations).

## O9. Text functions

The `text` kind evaluates pure text functions of the text area on a list of inputs. It never reads or changes the scenario's collection, so it may appear in any observe step, with or without setup.

- `fns` is a non-empty list of distinct function names from the table; `inputs` is a list (possibly empty). Every function runs on every input. The value has one member per function, an array with one output per input, in input order.
- `unicase_compare` takes `[a, b]` pairs of strings and shares a request with no other function; every other function takes strings. A step may hold several `text` requests when their functions differ (the values merge, O2).
- "As saved" means: the input is a field value given to a note save in a collection with the default settings (note text normalization on), and the output is what that save stores or decides (`specs/text/checksum-and-sort-field.md` C1).

| Function | Input | Output | Defined in |
|---|---|---|---|
| `strip_markup` | string | string | `text/strip-html#H4` |
| `strip_markup_keep_media` | string | string | `text/strip-html#H5` |
| `text_line` | string | string | `text/strip-html#H6`, media names dropped |
| `text_line_keep_media` | string | string | `text/strip-html#H6`, media names kept |
| `media_refs` | string | list of strings | `text/media-references#M4` |
| `iri_encode` | string | string | `text/media-references#M6` |
| `iri_decode` | string | string | `text/media-references#M7` |
| `media_filename` | string that lowercasing does not change | string | `text/media-filenames#F1` |
| `sort_field` | string | string: the sort-field text as saved, before the column's affinity applies | `text/checksum-and-sort-field#C2` |
| `checksum` | string | integer 0 to 4294967295: the checksum as saved | `text/checksum-and-sort-field#C4` |
| `field_empty` | string | boolean: whether the field as saved counts as empty | `text/field-emptiness#E2` |
| `unicase_compare` | `[a, b]` | -1, 0 or 1: `a` before, equal to or after `b` | `text/unicode-primitives#U2` |

- An input to `media_filename` that lowercasing (the Unicode default lowercase mapping) would change is `invalid_scenario`: the oracle observes the name transform through the media store, which also lowercases new names (`text/media-filenames#F4`). An input whose transformed name is empty is a run error on the oracle; use the spec's examples for that case.
- Errors: a malformed request is `invalid_scenario`. No function refuses a valid input.

## O10. Answer labels

`answer_labels` reports the labels the answer buttons show for a card now (`specs/sched/answer-labels.md`), computed from the same states as `next_states` at the same instant. Its value is `{"#cN": {"again": <label>, "hard": <label>, "good": <label>, "easy": <label>}}`, every card in logical-id order when no reference is given. A label is an object with exactly these members:

| Member | Value |
|---|---|
| `less_than` | `true` when the label starts with `<` (`sched/answer-labels#A5`) |
| `amount` | the number as the label writes it (`sched/answer-labels#A3`): digits, and a full stop and one digit when it has a fraction (`"10"`, `"2.3"`); `""` for the end label |
| `unit` | `seconds`, `minutes`, `hours`, `days`, `months` or `years` (`sched/answer-labels#A2`), or `end` for a delay of 0 |

- Example: `<10m` is `{"amount": "10", "less_than": true, "unit": "minutes"}`; `2.3mo` is `{"amount": "2.3", "less_than": false, "unit": "months"}`; `(end)` is `{"amount": "", "less_than": false, "unit": "end"}`.
- The parts, not the text, are compared: no localized text and no Unicode isolation mark is ever an observation (R1887).
- The oracle reads Anki's English labels and splits them: it drops the isolation marks U+2068 and U+2069, maps `(end)` to the end label, and otherwise reads an optional `<`, the amount (digits with an optional full stop and digits) and the suffix `s`, `m`, `h`, `d`, `mo` or `y`. A label it cannot split is a run error (`anki_error`), never a value.
- A card in a filtered deck has labels in Anki (preview delays); filtered decks are out of scope for Meguri, and scenarios request labels only for cards of normal decks.

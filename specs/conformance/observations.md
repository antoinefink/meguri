# Conformance observations and comparison

Spec: conformance/observations
Requirements: R1884, R1886, R1887, R1888, R1889, R1927
Anki: 26.09.3
Scenarios: conformance/scenarios/smoke/

What each observation kind records, how values are written, and how the runner compares Meguri's observations with a golden. The scenario language is `dsl.md`; value schemas are `conformance/schema/observation.schema.json`.

## O1. Observations of a run

- A run's observations are `{"steps": [<output of step 0>, <output of step 1>, ...]}`, one output per scenario step, so `/steps/<i>/...` points into step `i`.
- An op step's output is `{}`, `{<op>: <op output>}` (`dsl.md` D7.2) or a rejection `{<op>: {"error": <kind>}}` (`dsl.md` D7.3). An observe step's output is `{<kind>: <value>, ...}` with one member per requested kind. A `handoff` step's output is `{}`.
- Values are plain JSON keyed by logical ids (`#nN`, `#cN`). A real id appears only in the `ids` kind, or where a value is a real id by definition (revlog row ids are review instants, fuzz-table card ids are inputs, a `db_dump` holds the stored rows).
- In an actor scenario (`sync-ops.md`), an observe step runs on the current actor's collection (S2.3).
- No localized text is ever an observation (R1887): errors and results are kinds, counts and enums. Driver-authored English (`detail` of a protocol error) is never compared.

## O2. Requesting observations

`observe` takes a list. Each request is one of:

- `kind`: the kind with no arguments; for per-card kinds, every card in logical-id order;
- `kind(#c1, #c3)`: the kind for these references, in the order given;
- `{kind: <arguments>}`: kinds that take structured arguments.

Requesting a per-card kind twice in one step merges the two results when they name different cards; naming a card twice in one step is invalid.

## O3. Pointers and patterns

- A pointer is an RFC 6901 JSON pointer into the observations: `/steps/3/card/#c1/due`. `~` is written `~0` and `/` is written `~1` inside a segment.
- A pointer pattern (used by `compare.set` and deviations) is a pointer whose segments may be `*`, which matches any one segment.

## O4. Kinds

Kinds marked *draft* have no driver support yet; the owning area defines their values with the driver op, in this file and the schema.

| Kind | Arguments | Value |
|---|---|---|
| `ids` | note and card references, or none for all | `{"#nN": <note id>, "#cN": <card id>}`: real ids, which come from the clock in milliseconds and are bumped by 1 on collision |
| `timing` | none | `{days_elapsed, next_day_at, now, utc_offset_secs}`: the scheduler's day number, the next day boundary and the current instant in epoch seconds, and the local UTC offset now |
| `card` | card references or none | `{"#cN": <row> \| null}`: the stored card row with columns `id` (as `#cN`), `nid` (as `#nN`), `did` and `odid` (deck names; `null` for 0), `ord`, `mod`, `usn`, `type`, `queue`, `due`, `ivl`, `factor`, `reps`, `lapses`, `left`, `odue`, `flags` and `data` (parsed JSON object; `{}` when empty; the text when it is not JSON) |
| `revlog` | card references or none | `{"#cN": [<row>, ...]}`: the card's review-log rows in id order, columns `id`, `cid` (as `#cN`), `usn`, `ease`, `ivl`, `lastIvl`, `factor`, `time`, `type` |
| `next_states` | card references or none | `{"#cN": {current, again, hard, good, easy}}`: the scheduling state now and after each rating (O5) |
| `fuzz_table` | `{card: #cN, pairs: [[card_id, reps], ...], intervals: [days, ...]}` | `{intervals, rows: [[card_id, reps, delta, ...], ...]}`: the interval fuzz applied when answering a card with that id and review count (reviews before the answer), one delta in days per interval, bounded by the card's preset |
| `net_probe` | `{host, port, timeout_ms?}` | `{connected, error}` (diagnostic, oracle only: proves the worker has no route out) |
| `queue` | none or `{deck}` | draft (sched): the cards in the order the scheduler would show them, as `#cN`, and the new, learning and review counts |
| `deck_tree` | none | draft (sched): deck names with their counts, nested |
| `render` | card references or none | draft (render): question and answer HTML, AV tags and error kind per card |
| `search` | `{query, kind?: cards \| notes, order?}` | draft (search): `{ids: [...]}` in result order, or `{error: <kind>}` |
| `db_dump` | none or `{tables: [...]}` | `{<table>: {<key>: <row>}}`: the stored rows of the collection's tables, normalized (O8) |
| `stats` | arguments defined by the stats area | draft (stats) |
| `digest` | none | draft: a hash over all card rows, for fuzzing |
| `legacy_json` | `{notetypes?, decks?, presets?}`: lists of names | `{notetypes?: {<name>: <object>}, decks?: {...}, presets?: {...}}`, the schema-11 objects of the records as stored now (`model-ops.md` L1) |
| `blob_decode` | `{notetypes?, decks?, presets?}`: lists of names | `{notetypes?: {<name>: "ok" \| "invalid_blob"}, decks?: {...}, presets?: {...}}`, whether each record's protobuf blobs decode now (`model-ops.md` L4) |
| `package_col` | `{name, columns}`: a package file an `export` step wrote, and a list of `col` columns | `{<column>: <value>}`: the `col` row of the legacy package's collection, `ver` an integer and the JSON columns parsed (`model-ops.md` L7) |
| `text` | `{fns: [<function>, ...], inputs: [<input>, ...]}` | `{<function>: [<output>, ...], ...}`: pure text functions (O9); never reads or changes the scenario's collection |

## O5. Scheduling states

`next_states` values follow the scheduler's protobuf state messages, flattened:

- a state is `{kind, ...fields}` with `kind` one of `new`, `learning`, `review`, `relearning`, `preview`, `rescheduling`;
- `new`: `position`; `learning`: `remaining_steps`, `scheduled_secs`, `elapsed_secs`, `memory_state?`; `review`: `scheduled_days`, `elapsed_days`, `ease_factor`, `lapses`, `leeched`, `memory_state?`; `relearning`: `learning` and `review`, each the matching state without `kind`;
- `memory_state` is `{stability, difficulty}`, present only with FSRS on;
- interval labels are never observed.

## O6. Numbers

- Integers are written as integers.
- A value the engine keeps as 32-bit float (memory states, ease factors, desired retention) is written as the shortest decimal that parses back to the same f32, chosen as Ryu does (fewest digits, then nearest, ties to even), as a JSON number with a fraction or exponent: 11.262237, never the widened 11.262236595153809; 1.0, never 1.
- A 64-bit float is written as its shortest round-trip decimal.

## O7. Comparison

The runner compares the golden's observations (expected) with the executor's (actual) after applying deviation transforms to expected (`dsl.md` D13) and set comparison to both (`compare.set`).

- Objects compare member by member: a member on one side only is a difference, shown as `<absent>` on the other side.
- Arrays compare item by item; extra items on either side are differences. Under `compare.set` the two arrays are first sorted by the JCS text of their items, ties broken by a text that keeps the number kind (JCS writes 1 and 1.0 alike), so two equal multisets always line up.
- Strings, booleans, null and HTML compare exactly. Integers compare exactly. Order is significant everywhere except under `compare.set`.
- A number compares exactly, kind included (1 and 1.0 differ), unless a tolerance below applies.
- Tolerances (R1886), relative, `|actual − expected| ≤ tol × max(|expected|, |actual|)`, and only when both values are written with a fraction or exponent (an integer on either side compares exactly):

  | Where | Tolerance |
  |---|---|
  | `stability` and `difficulty` in any `memory_state` inside `next_states` | 1e-5 |
  | any number inside `stats` | 1e-6 |

  Card rows, review-log rows and every other kind are exact: a stored value that a tolerance could hide is a real difference.
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
- Real ids appear as stored. Meguri allocates ids as Anki does (U23), so a Meguri collection's dump has the same ids as the oracle's.

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

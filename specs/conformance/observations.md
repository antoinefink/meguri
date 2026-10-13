# Conformance observations and comparison

Spec: conformance/observations
Requirements: R1884, R1886, R1887, R1888, R1889
Anki: 26.09.3
Scenarios: conformance/scenarios/smoke/

What each observation kind records, how values are written, and how the runner compares Meguri's observations with a golden. The scenario language is `dsl.md`; value schemas are `conformance/schema/observation.schema.json`.

## O1. Observations of a run

- A run's observations are `{"steps": [<output of step 0>, <output of step 1>, ...]}`, one output per scenario step, so `/steps/<i>/...` points into step `i`.
- An op step's output is `{}`, `{<op>: <op output>}` (`dsl.md` D7.2) or a rejection `{<op>: {"error": <kind>}}` (`dsl.md` D7.3). An observe step's output is `{<kind>: <value>, ...}` with one member per requested kind. A `handoff` step's output is `{}`.
- Values are plain JSON keyed by logical ids (`#nN`, `#cN`). A real id appears only in the `ids` kind, or where a value is a real id by definition (revlog row ids are review instants, fuzz-table card ids are inputs).
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
| `schema_objects` | none | `[{type, name, tbl_name, sql}]`, every object of the collection file's `sqlite_master` with its SQL text normalized (`storage-ops.md` P3) |
| `queue` | none or `{deck}` | draft (sched): the cards in the order the scheduler would show them, as `#cN`, and the new, learning and review counts |
| `deck_tree` | none | draft (sched): deck names with their counts, nested |
| `render` | card references or none | draft (render): question and answer HTML, AV tags and error kind per card |
| `search` | `{query, kind?: cards \| notes, order?}` | draft (search): `{ids: [...]}` in result order, or `{error: <kind>}` |
| `db_dump` | none or `{tables: [...]}` | draft (storage): normalized table rows; protobuf blobs as lower-case hex bytes, decoded only for display |
| `stats` | arguments defined by the stats area | draft (stats) |
| `digest` | none | draft: a hash over all card rows, for fuzzing |

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

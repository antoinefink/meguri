# Browser observations

Spec: conformance/browser-ops
Requirements: R1884, R1886, R1887, R1920; supports R1118–R1131, R1139–R1145
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/sort/, conformance/scenarios/browser/columns/, conformance/scenarios/browser/modes/

What the browser scenarios observe: sorted searches (an `order` member on `search` requests, `search-ops.md` S1) and Browse rows (the `browser_rows` kind). Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. What they exercise is `specs/browser/sorting.md` and `specs/browser/columns.md`.

## B1. Sorted searches

- **Request.** A `search` request (`search-ops.md` S1) may carry `order: {column: <column key>, reverse?: <boolean>}`: the search's rows sorted by that column (`browser/sorting`), ascending, or descending when `reverse` is true (false when missing). `column` is any string; the 19 keys are `browser/config-keys` K3.
- **Value.** `{ids: [...]}`: the matching cards (cards mode) or notes (notes mode) in the sorted order, each as its logical reference, or as its real id (an integer) when no logical id names it, at its place in the order; then B2 reorders equal-key runs. Or an error.
- **`unsortable`.** The output is `{error: "unsortable"}` when `column` is neither one of the 19 keys nor a field column key `fld:<name>`, or names a column that is not sortable in the request's mode (`browser/sorting` S1). Changed 2026-11-20 (round 8 review): a field column key `fld:<name>` is not refused; it sorts as `browser/workspace` V4.2 says (`browser/sorting` S1), and Meguri's executor runs it; the oracle has no such column, so no recorded scenario uses one.
- **Error order.** A parse error of the query (`search/errors` E1, E4) comes first. Then `unsortable`. Then the run errors (`invalid_regex`, `search_failed`; E3). So `(` sorted by Question is `unclosed_group`, and `"re:("` sorted by Question is `unsortable` on both sides, although Meguri finds the bad pattern while it builds the plan.
- **Oracle.** Anki's card or note search with its built-in sort for the column key and direction, on the collection as it is at that step, with the scenario clock. Anki refuses a column it cannot sort with an invalid-input error; the oracle tells it from a run error by also sorting a query that cannot fail (the empty search) by the same column in the same mode: when that is refused too, the output is `unsortable`.
- **Meguri.** Its executor parses the query, checks the column against the catalog for the mode, builds the plan and runs the sorted search (U84 keys, U85 entry point) in the product's fold mode, as `search-ops.md` S1 says for unsorted searches. The core refuses an unsortable column with kind `unsortable_column` (`browser/sorting` S1); the executor records that refusal as `unsortable`. Once U85 lands, the browser adapter runs every `search` request of a `browser/` scenario through the entry points of `browser/result-sets` (H16): an unsorted one through `search_ids`, a sorted one through `start_search` with slow-term plans streamed.
- Nothing in the collection changes. In particular Meguri creates no schema object (`browser/sorting` S10).
- Changed 2026-11-21 (implementation review): **A search never writes.** Every `search` request of a `browser/` scenario, sorted or not (an unsorted one runs as `search-ops.md` S1 says), computes today and next-day-at with the day calculation alone (`sched/day-clock` D8.1) and never runs a day query: it unburies nothing and writes no `rollover`, `localOffset` or `lastUnburied`. The core's searches do the same (`search/query-model` Q8). Anki's search can write `rollover` and `localOffset` when it computes the day afresh (Q8); no scenario reads those keys after a search without an earlier `timing` observation.

## B2. Equal-key runs

Rows that a column's keys do not order (`browser/sorting` S8) come back in an order that Anki's database chooses and Meguri's final id key decides. Both executors therefore list each **equal-key run** of a sorted result in logical-id order: within the positions the run occupies, its rows are placed as `search-ops.md` S1 orders an unsorted result (references by number, then real ids ascending). Everything else keeps its sorted position.

- **Meguri** finds the runs from its own keys: consecutive rows that are equal on every key of `browser/sorting` S4 or S5, the final id key aside.
- **The oracle** finds them from Anki alone. Anki reverses every key for a descending sort but keeps rows with equal keys in the same order in both directions. So it also runs the search sorted the other way, and two consecutive rows of the requested result belong to one run exactly when they are in the same relative order in both results.
- When Meguri's keys and Anki's differ, the runs differ, and so do the lists: a run on one side only is listed in logical-id order there and in key order on the other.

Example: cards c1, c2, c3 where c2 and c3 have equal `mod` and c1 a larger one. Sorted by `cardMod` descending, Anki returns c1, c2, c3 (its table order for the tie) and Meguri c1, c3, c2 (final key descending); both record `["#c1", "#c2", "#c3"]`. Ascending, both return c2, c3, c1 and record `["#c2", "#c3", "#c1"]`.

## B3. `browser_rows` (observation kind)

- **Request** `{browser_rows: {columns: [<column key>, ...], rows: [<reference>, ...]}}`: `columns` a non-empty list of distinct keys among the 19; `rows` a non-empty list of distinct logical references. At most one `browser_rows` request per observe step.
- **Mode.** The rows are cards or notes as the collection's config key `browserTableShowNotesMode` says (`browser/config-keys` K2.1: notes mode only when it holds `true`), the way Browse reads it. Scenarios set it with `set_config` (`record-ops.md` W4). In cards mode every reference must be `#cN`, in notes mode `#nN`; any other reference is `invalid_scenario`.
- **Value** a list with one entry per reference, in request order: `{tint: <tint>, cells: {<column key>: <cell>, ...}}` with a member for every requested column, or `{error: "deleted" | "damaged"}` (`browser/columns` C3.1).
- **Tint** (`browser/columns` C5): `none`, `flag_1` … `flag_7`, `marked`, `suspended` or `buried`.
- **Cells**: B4.
- **Clock and zone.** Now and today come from the scenario clock; dates and times are local times of the scenario's zone (`dsl.md` D5).
- Nothing in the collection changes.

## B4. Cells

Each cell is one of these JSON values. `browser/columns` C7 and C8 say which value a cell has; C12 gives the English text Anki shows for each.

| Cell | JSON | Columns |
|---|---|---|
| text | `{text: <string>}` | `noteFld`, `question`, `answer`, cards-mode `template`, `deck` (a deck count included: `{text: "(2)"}`), `note`, `noteTags` |
| count | `{count: <integer>}` | notes-mode `template`, `cardReps`, `cardLapses` |
| percent | `{percent: <integer>}` | `cardEase`, `difficulty`, `retrievability` |
| position | `{position: <integer>}` | `originalPosition` |
| date | `{date: "YYYY-MM-DD"}` | `cardDue` (an instant, as its local date) |
| new position | `{new_position: <integer>}` | `cardDue` |
| filtered | `{filtered: true}` | `cardDue` |
| date-time | `{datetime: "YYYY-MM-DD HH:MM"}` | `noteCrt`, `cardMod`, `noteMod` |
| marker | `{marker: "new" \| "learning"}` | `cardEase` (new), `cardIvl` |
| time span | `{span: {unit: <unit>, amount: "<decimal>"}}` | `cardIvl`, `stability` |
| empty | `null` | any but the text columns |

- A wrapped Due (`browser/columns` C7.5) adds `wrapped: true` to its date, new position or filtered object.
- `unit` is one of `seconds`, `minutes`, `hours`, `days`, `months`, `years`, whatever the amount; `amount` is the decimal text of `browser/columns` C12 (time span steps 1 to 3): `{unit: "months", amount: "1.3"}`, `{unit: "days", amount: "1"}`.
- Text cells hold the text exactly, an empty string included (`{text: ""}` for a note without tags).

**Oracle.** Anki's Browse rows for the requested columns (its active column list for the call, which is not stored) and its English messages. It removes every U+2068 and U+2069 from the cell texts, then reads each cell by its column:

| Column | Anki's text → cell |
|---|---|
| text columns | the text → text |
| notes-mode `template`, `cardReps`, `cardLapses` | decimal digits → count |
| `cardEase`, `difficulty`, `retrievability` | `` → empty; `(new)` (Ease only) → marker new; an optional `-`, digits, `%` → percent |
| `originalPosition` | `` → empty; an optional `-` and digits → position |
| `cardDue` | `` → empty; `(filtered)` → filtered; `((filtered))` → filtered, wrapped; `New #` and an optional `-` and digits → new position; the same in parentheses → wrapped; `YYYY-MM-DD` → date; in parentheses → wrapped |
| `noteCrt`, `cardMod`, `noteMod` | `YYYY-MM-DD @ HH:MM` → date-time |
| `cardIvl`, `stability` | `` → empty; `(new)`, `(learning)` (Interval only) → marker; a decimal number, U+0020 and one of `second`, `seconds`, `minute`, `minutes`, `hour`, `hours`, `day`, `days`, `month`, `months`, `year`, `years` → time span, the unit in its plural form |

A cell text that matches none of its column's forms is recorded as `{unparsed: <text>}`, which no Meguri cell equals, so the scenario fails and the spec writer looks at it. Scenarios keep Question and Answer away from template errors and blank cloze cards, whose cells hold Anki's localized messages (R1887). Anki's backend errors map to row errors: a deleted card or note to `deleted`; a missing record or a note without cards (Anki asks for Check Database) to `damaged`; any other failure is a run error (`anki_error`).

**Meguri.** Its executor asks the core for the rows (U84, through U85's row API), reads the tint and maps each typed value (`browser/columns` C9) to its cell: instants to their date in the scenario's zone, date-times to date and time to the minute, a time span to unit and amount by `browser/columns` C12 steps 1 to 3, a deck count n to the text `(n)`, a filtered card's deck to the text of `browser/columns` C7.4, the Question and Answer render outcomes to their text (a blank or error outcome has no cell form here; scenarios avoid them). A row error maps to its kind.

## B5. What scenarios hold

- Note types, decks and notes come from `setup`; card states, stamps and card data from `db_exec` steps (`record-ops.md` W1), which address cards by note position and ordinal (`nid = (SELECT id FROM notes ORDER BY id LIMIT 1 OFFSET k) AND ord = j`) because Anki creates a cloze note's cards in no set order. A card in a filtered deck is a card whose `did` names an ordinary deck and whose `odid` and `odue` hold its home deck and due: Browse shows it and sorts it exactly like a card of a real filtered deck, which the setup cannot create.
- A note whose first card has ordinal 1 is made by adding the note with only its second template's field filled and filling the first afterwards with `edit_note` (`record-ops.md` W2), which generates the ordinal 0 card with a higher id. `edit_note` gives that card no logical id (`note-ops.md` N3): a scenario that names it runs `adopt` (`normalized-dump.md` N5) after the edit.
- `db_exec` steps change only the `cards`, `notes` and `config` tables, never `notetypes`, templates, fields or `decks`: Anki's Browse rows read note types and decks through caches that a `db_exec` write does not clear, so the oracle would show the earlier state. Note type and deck changes go through their ops.
- Equal-key runs need no declaration (B2).

## B6. Status

Stable: the oracle driver implements `order` on `search` requests and `browser_rows`, and `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The 26 scenarios of `browser/sorting` S11 (17) and `browser/columns` C15 (9) have goldens recorded at the current oracle pin, and so do the 11 scenarios of `browser/result-sets` H16 (`browser/modes/`), which use the same two members. Changed 2026-11-21 (implementation review): the testkit's scenario model reads `order` and `browser_rows`, and Meguri's executor implements both (U84): a sorted `search` through meguri-search's `SortedQuery`, listing each equal-key run as B2 says, and `browser_rows` through meguri-engine's row records; neither, nor any other search, runs a day query or writes to the collection (B1, B3). A field column key `fld:<name>` (B1) is not run yet: its sort key is U261's, and until then the executor reports `unsupported_observation` for it (`dsl.md` D10); no recorded scenario uses one. Every scenario of `browser/sort/` and `browser/columns/` is active in `conformance/scenarios/browser/area.yaml`; U85 activates `browser/modes/` when its entry points land (`browser/result-sets` H16).

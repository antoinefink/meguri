# Search observations

Spec: conformance/search-ops
Requirements: R1075, R1076, R1078, R1079, R1080, R1085, R1887, R1916, R1919
Anki: 26.09.3
Scenarios: conformance/scenarios/search/

The observation kinds the search scenarios use: `search` (results and errors of searches on the scenario's collection), `canonical_search` (canonical texts and parse errors, independent of the collection), and `search_build`, `search_join` and `search_replace` (the text of built searches, `specs/search/join-and-replace.md`, S5 and S6). Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. What they exercise is `specs/search/*.md`.

## S1. `search` (observation kind)

- **Request** `{search: [<request>, ...]}`: a non-empty list. Each request is `{query: <text>, kind?: cards | notes, order?: {column, reverse?}}`; `kind` is `cards` when missing, and `order` asks for a sorted result (`browser-ops.md` B1). Several searches go in one list, since one observe step holds a kind once (`observations.md` O2).
- **Value** a list with one output per request, in request order. An output is `{ids: [...]}` or an error (S3).
- **`ids`.** The matching cards (`#cN`, cards mode) or notes (`#nN`, notes mode) as logical references, sorted by their number. A matching row that no logical id names (a collection opened from a fixture, a row a `db_exec` step inserted) is given as its real id, an integer, after the references, the integers ascending. The order in which the search returned the rows is not observed: these searches have no sort order, and Anki's unsorted order is SQLite's row order. Sorted results are requested with an `order` member, which `browser-ops.md` B1 and B2 define (U84).
- **What runs.** The query text exactly as given, in the given mode, with no sort order, on the collection as it is at that step: the scenario clock for day and time terms, the collection's config (`curDeck`, `collapseTime`, `normalize_note_text`, `ignoreAccentsInSearch`) for the rest. Nothing in the collection changes.
- **Oracle.** Anki's card search or note search with no order. A search error becomes its kind by matching Anki's message against Anki's own translations of each search error (never against literal English), and reading `provided` and `context` out of the message. Anki's invalid-regex error is `invalid_regex`. A database error raised while the search runs is `search_failed` (`specs/search/errors.md` E3). Any other exception is a run error (`anki_error`).
- **Meguri.** Its executor parses the query, builds the plan for the mode and runs it (`specs/search/query-model.md`) in the product's Unicode fold mode. Where that mode matches more rows than Anki (R1036), the scenario carries a deviation (U81, U91, R1916); the AsciiCompat mode is for U91's differential runs.

## S2. `canonical_search` (observation kind)

- **Request** `{canonical_search: [<text>, ...]}`: a non-empty list of search texts.
- **Value** a list with one output per text, in order: `{text: <canonical text>}` (`specs/search/canonical-form.md`) or a parse error (S3). Run errors (`invalid_regex`, `search_failed`) never occur here.
- **Oracle.** The normalized text Anki's browser puts in the search field when a search is committed: Anki's search-string builder given the text as one search.
- **Meguri.** Its executor parses the text and writes the canonical text.
- It never reads or changes the scenario's collection, so it may appear in any observe step, with or without setup.

## S3. Errors

- An error output is `{error: <kind>}` plus, for the kinds that carry them, `provided` and `context` (`specs/search/errors.md` E1, E2). The schema fixes which kinds carry which members.
- In `provided` and `context`, each backtick (U+0060) is replaced by an apostrophe (U+0027). Anki's message makes that replacement, and the oracle reads both texts from the message; Meguri's executor applies the same replacement to its error's texts.
- Scenarios keep U+2068 and U+2069 out of failing terms: the oracle finds the texts in Anki's message between those two characters.
- No message text is ever observed (R1887). Ranges are Meguri's only (`errors.md` E5) and are not observed either.
- `malformed` is in the schema so that a differential run can record it; no scenario expects it.

## S4. Status

Stable: the oracle driver implements all five kinds, and every scenario under `conformance/scenarios/search/` has its golden. Meguri's executor implements `search` and `canonical_search` in the search adapter of the testkit (U80). `search_build`, `search_join` and `search_replace` (S5, S6) are stable on the oracle side, with the goldens of `search/builder/` and `search/join-replace/`; those scenarios stay `pending` until U88's adapter lands.

Result scenarios name the term families they need beyond the whole collection in their `tags`: `needs-text` (U81); `needs-tag`, `needs-deck` (`deck:`, `preset:`, `did:`), `needs-notetype` (`note:`), `needs-template` (`card:`), `needs-ids` (`nid:`, `cid:`, `mid:`), `needs-dupe`, `needs-state` (`is:`, `flag:`), `needs-prop` (`prop:` other than `rated` and `resched`, `has-cd:`) and `needs-history` (`added:`, `edited:`, `introduced:`, `rated:`, `resched:`, `prop:rated`, `prop:resched`) (U82). Executors ignore tags (`dsl.md` D4); they tell the area which scenarios can become active (`dsl.md` D9.1) as each unit lands. Scenarios without such a tag need only U80, except that every scenario using S6's kinds (all of `search/builder/` and `search/join-replace/`) also needs U88's adapter.

## S5. Built items (neutral form)

The built items of `specs/search/join-and-replace.md` J3, written in scenarios as JSON. An item is one of the words or a mapping with one key (the parsed-text item may also carry `field_text`):

| Neutral item | J3 item | Anki's structured search item (oracle) |
|---|---|---|
| `whole_collection` | whole collection | an item with no filter |
| `current_deck` | current deck | deck item `current` |
| `any_deck` | any deck | deck item `_*` |
| `any_tag` | any tag | tag item `_*` |
| `untagged` | untagged | negation of tag item `_*` |
| `any_notetype` | any note type | note type item `_*` |
| `{deck: <name>}` | deck by name (full name, `::` between levels) | deck item |
| `{tag: <name>}` | tag by name | tag item |
| `{notetype: <name>}` | note type by name | note type item |
| `{template: <k>}`, k 0–65534 | template by ordinal (0-based) | template item |
| `{field_present: <f>}` | field present | field-name item |
| `{field: {name: <f>, text: <t>}}` | field text | field item, normal mode |
| `{literal: <t>}` | literal text | literal-text item |
| `{flag: "any" \| "none" \| 1–7}` | any flag, no flag, flag n | flag item: any, none, the n-th color |
| `{state: new \| learn \| review \| due \| suspended \| buried}` | state | card-state item |
| `{added: d}`, `{edited: d}`, `{introduced: d}`, d 1–4294967295 | added, edited, first review in d days | added-, edited- and introduced-in-days items |
| `{answered: {days: d, button?: 1–4}}`, d 1–4294967295 | answered in d days, any button or button b | rated item, any rating or the button's |
| `{rescheduled: d}`, d 0–4294967295 | rescheduled in d days | rated item, by reschedule |
| `{due_on: i}`, `{due_within: i}`, i signed 32-bit | due on day i, due within i days | due-on-day and due-in-days items |
| `{note_ids: [<id>, ...]}`, non-empty, ids 0–2^53−1 | note ids, in the given order | note-id list item |
| `{dupe: {notetype_id: <m>, text: <t>}}` | duplicates of t in note type m | duplicates item |
| `{text: <s>, field_text?: <boolean>}` | parsed text (`field_text` false when missing) | parsable-text item |
| `{group: {joiner: and \| or, items: [<item>, ...]}}`, one or more items | group | group item |
| `{negated: <item>}` | negation of an item (J5) | negated item |

- J3's note type by id, card-id and deck-id lists and the states `buried-manually` and `buried-sibling` have no structured counterpart in Anki and no neutral form; U88's tests cover them (J11).
- Scenarios hold neither J10's inputs nor what J3 refuses (`invalid_item`): Meguri writes another text there, by design. The oracle refuses them as `invalid_scenario`: a name `_*`; a deck named `current` or `filtered` and a tag named `none`; a deck or note type name J3.5 lists; a tag, field name, field text, literal or dupe text holding White_Space other than U+0020 or U+001C–U+001F; a field named like a reserved key (J3.3); an empty name or literal; 0 days for added, edited, first review and answered. The schema fixes the shapes and number ranges.

## S6. `search_build`, `search_join`, `search_replace` (observation kinds)

- **Requests.** `{search_build: [<item>, ...]}`; `{search_join: [{existing, item, joiner: and | or, negate}, ...]}`; `{search_replace: [{existing, item}, ...]}`: non-empty lists, `existing` a search text, `item` S5, `negate` a boolean.
- **Value** a list with one output per entry, in order: `{text: <search text>}` (J4 for `search_build`, J6 for `search_join`, J7 for `search_replace`) or a parse error (S3) of `existing` or of a parsed-text item. Run errors (`invalid_regex`, `search_failed`) never occur. `source` (J1) and ranges are not observed. Scenarios give at most one text per entry that fails to parse, since which error comes first is not specified.
- **Search-field text.** `existing`, and the text of a parsed-text item with `field_text` true, get J2 first (U+001C–U+001F become U+0020).
- **Oracle.** The item's structured search item (S5) given to Anki's backend: its search-string builder for `search_build`; its join of `existing` as one parsable-text item with the item, wrapped in a negated item when `negate` is true (the desktop browser's sidebar join), for `search_join`; its replacement of `existing`'s terms of the item's kind for `search_replace`. J2 is the desktop search field's rule, which the backend does not apply, so the oracle applies it before. Parse errors are read from Anki's message as in S1.
- **Meguri.** Its executor maps the item to the core's built item (J3) and calls `build`, `join` or `replace` (U88), recording a parse error's kind and texts. An `invalid_item` is a run error of the executor, since scenarios hold none.
- They never read or change the scenario's collection, so they may appear in any observe step, with or without setup.

# Search observations

Spec: conformance/search-ops
Requirements: R1075, R1076, R1080, R1085, R1887, R1916, R1919
Anki: 26.09.3
Scenarios: conformance/scenarios/search/

The two observation kinds the search scenarios use: `search` (results and errors of searches on the scenario's collection) and `canonical_search` (canonical texts and parse errors, independent of the collection). Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. What they exercise is `specs/search/*.md`.

## S1. `search` (observation kind)

- **Request** `{search: [<request>, ...]}`: a non-empty list. Each request is `{query: <text>, kind?: cards | notes}`; `kind` is `cards` when missing. Several searches go in one list, since one observe step holds a kind once (`observations.md` O2).
- **Value** a list with one output per request, in request order. An output is `{ids: [...]}` or an error (S3).
- **`ids`.** The matching cards (`#cN`, cards mode) or notes (`#nN`, notes mode) as logical references, sorted by their number. A matching row that no logical id names (a collection opened from a fixture, a row a `db_exec` step inserted) is given as its real id, an integer, after the references, the integers ascending. The order in which the search returned the rows is not observed: these searches have no sort order, and Anki's unsorted order is SQLite's row order. Sorted results belong to the sorting spec (U84), which will add an `order` member to the request.
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

Draft until the oracle driver implements both kinds; the scenarios under `conformance/scenarios/search/` have no goldens before then. Meguri's executor implements them in the search adapter of the testkit (U80).

Result scenarios name the term families they need beyond the whole collection in their `tags`: `needs-text` (U81), `needs-state`, `needs-template`, `needs-deck` (U82). Executors ignore tags (`dsl.md` D4); they tell the area which scenarios can become active (`dsl.md` D9.1) as each unit lands. Scenarios without such a tag need only U80.

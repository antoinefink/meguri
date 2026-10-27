# Browser columns, rows, sorting and card excerpts (U84, U85a)

Spec-writer notes for `specs/browser/columns.md`, `specs/browser/sorting.md`,
`specs/conformance/browser-ops.md` and `specs/conformance/card-excerpt-ops.md`.
Read at tag 26.09.3 (`refs/anki-26.09.3`); no code was copied. The rows below
come from the U84 spec session's notes (`provenance/browser.md` on `spec/U84`)
and the U85a session's (`provenance/card-excerpts.md` on `spec/U85a`), checked
2026-10-27 against the merged text's section ids. The browser config keys
(column lists, sort keys) are `specs/browser/config-keys.md`, whose sources are
in `browser-config.md`.

U84's rules were checked as a black box in the oracle image's Anki (three
probe collections through the backend's card and note searches with built-in
sorts and its Browse rows, then every `conformance/scenarios/browser/` scenario
through a throwaway prototype of the two observations on the real driver,
outside the repository). U85a's probes ran in `meguri-oracle:26.9.3-19fcf7e81ab2ab0e`
with no network: the backend's Browse rows with the active columns `question`
and `deck` for every Question-cell example of C10 and the Deck examples of C7.4
(all matched the spec text), a deleted card (the backend's deleted error), a
card whose `did` or `odid` names no deck and a card whose note row was removed
(the not-found error for all three), and `col.mod` before and after reading
rows (unchanged). A later spec session (s-20261018T061011Z-8f03905b) recorded
`damaged-notetype` with the driver's `card_excerpts` kind: a standard card
whose `ord` is past its note type's templates and a card whose note's `mid`
names no note type both come back damaged, as C3.1 states.

Meguri's own design, with no Anki source: the tooltips (C2), the leech state
(C4), the excerpt call's contract (C16.1, C16.3, C16.4), S8's final id key and
S10's constraints.

| Spec section | Anki files read | Read for |
|---|---|---|
| columns C1, C7.4 | `rslib/src/browser_table.rs`, `rslib/src/decks/name.rs` | the first card, the deck and home deck the cards-mode Deck cell joins, the human form of deck names |
| columns C2 | `rslib/src/browser_table.rs`, `proto/anki/search.proto`; the English labels as the oracle image's backend lists its columns | the column keys, labels per mode, alignment, elision, cell font, which columns have tooltips in Anki |
| columns C3, C3.1 | `rslib/src/browser_table.rs`, `rslib/src/notetype/mod.rs`, `qt/aqt/browser/table/model.py` | the deleted error for a missing card in cards mode and for a missing note in notes mode; the database-check-required error for a note without cards in notes mode (C3.1's damaged row); the not-found errors for a missing note, note type, deck or home deck, and for a missing template through the row's font lookup (first template for cloze); how the desktop shows a failed row |
| columns C4, C5 | `rslib/src/browser_table.rs` | the colour priority, the raw flags comparison, the marked tag test |
| columns C6 | `rslib/src/browser_table.rs`, `rslib/src/notetype/mod.rs` | font name and size from the card's template (the first template for cloze), the sort field's RTL option |
| columns C7, C7.1 to C7.12, C8, C9 | `rslib/src/browser_table.rs`, `rslib/src/card/mod.rs`, `rslib/src/scheduler/new.rs`, `rslib/src/timestamp.rs`, `rslib/src/decks/name.rs`, `rslib/src/tags/mod.rs`, `rslib/src/storage/card/mod.rs` | each cell per mode, the card states behind Due, the day-count rule, the notes-mode filters and averages with their integer types, the first card (cards of a note read in index order), the date and date-time formats, the tag split |
| columns C10 | `rslib/src/browser_table.rs`, `rslib/src/notetype/render.rs`, `rslib/src/template.rs`, `rslib/src/card_rendering/mod.rs`, `rslib/src/card_rendering/writer.rs`, `rslib/src/text.rs` | browser formats chosen per side, partial rendering (FrontSide empty), the empty-front check skipped in browser mode while the missing-cloze check stays, the joined node text, the audio prettifier, the answer prefix removal, the one-line text with media names kept, errors turned into the cell's message |
| columns C11 | `rslib/src/browser_table.rs`, `rslib/src/timestamp.rs`, fsrs 6.6.2 `inference.rs` (MIT/BSD, not Anki) | the elapsed-time branches and their 32-bit casts, the default decay |
| columns C12 | `rslib/src/scheduler/timespan.rs`, `rslib/i18n/src/lib.rs`, `ftl/core/scheduling.ftl` | unit choice, rounding, number text, plural forms, isolation marks |
| columns C13 | `Cargo.toml` (no release profile override; `release-lto` only inherits `release` and adds LTO) | Cargo's default of no overflow checks in release builds applies, so the 16-bit and 32-bit sums wrap (confirmed in the oracle image: 7% and 14.1 years) |
| columns C14 | `ftl/core/browsing.ftl`, `ftl/core/statistics.ftl` | the English marker strings `(new)`, `(learning)`, `(filtered)`, `New #` |
| columns C16.2 | `rslib/src/browser_table.rs`, `rslib/src/template.rs`, `rslib/src/card_rendering/service.rs` | the Question cell's outcomes: text, a blank cloze card, a template error and its side |
| columns C16, card-excerpt-ops E3 | `rslib/src/browser_table.rs`, `rslib/src/search/service/mod.rs`, `pylib/anki/collection.py` | the row call taking the notes-mode config key, the active columns as backend state, the row's day timing read without writes |
| sorting S1, S8 | `rslib/src/browser_table.rs`, `rslib/src/search/mod.rs`, `rslib/src/search/service/mod.rs`, `pylib/anki/collection.py`, `qt/aqt/browser/table/table.py` | sortability per mode, first directions, unknown keys mapping to no column, the refusal; the desktop reverses the held list when only the direction changes |
| sorting S2 to S6, S5.1 | `rslib/src/search/mod.rs`, `rslib/src/search/*_order.sql`; column collations and name indexes from Meguri's `specs/storage/schema-18.md` | the order clause per column and mode, the per-note precomputed orders, the collations used, the due expression; SQLite's own rules for bare columns in grouped queries and stable sorting |
| sorting S7 | `rslib/src/storage/sqlite.rs`, `rslib/src/storage/card/data.rs` | the retrievability function's arguments, branches and 32-bit casts |
| sorting S8 | `rslib/src/search/mod.rs` | reversing every key by text substitution |
| sorting S9 | `rslib/src/search/mod.rs`, `rslib/src/search/sqlwriter.rs` | table requirements of sort columns combined with the query's |
| browser-ops B1, B2 | `pylib/anki/collection.py`, `rslib/src/search/service/mod.rs`, `rslib/src/search/mod.rs` | the built-in sort request, the invalid-input refusal, the order of parse, sort and run errors |
| browser-ops B3, B4 | `pylib/anki/collection.py`, `rslib/src/search/service/mod.rs`, `rslib/src/browser_table.rs` | active columns as backend state, the notes-mode config key, the row texts |

Probed in the oracle image by U84 (SQLite bundled with the 26.9.3 wheel):

- Rows with equal sort keys come back in the same order for both directions:
  table order for most columns, the `(did, queue, due)` index order for the
  deck sort. Notes-mode per-note orders are numbered, so their ties follow the
  note id and reverse with the direction.
- In the per-note orders, a value taken from a single card (Position, the
  deck of a multi-deck note) comes from the note's lowest-id card.
- Deck names sort with U+001F before U+0020 under unicase; equal template
  names rank by note type id.
- A note added with several clozes gets its cards in no fixed id order (also
  noted by `specs/notes/card-generation.md` G8), so scenarios never depend on
  which of them is first.

Driver (oracle maintainer, U84): the sort and row observations use, besides the
files above, `rslib/src/backend/error.rs` (row errors by backend error kind:
Deleted, NotFound, and Check Database sent as invalid input),
`rslib/src/config/bool.rs` and `rslib/src/config/mod.rs` (Anki's own reading of
`browserTableShowNotesMode`), `rslib/src/scheduler/mod.rs` (the day computation
writes `rollover` when it is missing) and `pylib/anki/_backend.py` (the error
kinds pylib maps to classes). Their driver notes are `driver-browser.md` and
`driver-card-excerpts.md`.

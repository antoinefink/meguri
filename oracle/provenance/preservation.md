# specs/conformance/normalized-dump.md and conformance/scenarios/preservation/ (U36)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to state the preservation suite's
behavior in Meguri's own words and to build the kitchen-sink recipe; no code
was copied. Every behavior the spec states was then confirmed on the oracle by
running each scenario's steps after the recipe's steps (throwaway scenarios,
not kept).

| Anki file | Read for | Spec sections |
|---|---|---|
| `rslib/src/scheduler/filtered/card.rs`, `rslib/src/scheduler/filtered/mod.rs` | moving a card into a filtered deck: home deck into `odid`, due into `odue`, positions from -100000 for positive dues, the review queue for decks that do not reschedule; how answering returns a card home | N8, N9.3; the recipe's and `sync/filtered-deck`'s card moves |
| `rslib/src/scheduler/states/review.rs`, `rslib/src/scheduler/states/preview_filter.rs`, `rslib/src/revlog/mod.rs` | review-log kinds 0 to 5; kind 3 for an early review and for a preview | N8 |
| `rslib/src/scheduler/filtered/custom_study.rs`, `rslib/src/config/deck.rs`, `rslib/src/config/notetype.rs` | custom study's remembered tags per deck (`_deck_<id>_customStudyIncludeTags`, `…ExcludeTags`); the per-deck and per-note-type Add-screen keys | N8, N9.3 |
| `rslib/src/notes/mod.rs` | adding a note writes `_nt_<id>_lastDeck`, `_deck_<id>_lastNotetype` and `curModel` (the recipe's raw inserts collided with them) | N8 |
| `rslib/src/config/bool.rs`, `rslib/src/config/string.rs` | config key names: `restorePositionReviewer`, `resetCountsReviewer`, `setDueReviewer`, `cardStateCustomizer`, `fsrsLegacyEvaluate`, `renderLatex`, `dueCounts`, `estTimes` | N8, N11.1 |
| `rslib/src/scheduler/mod.rs` | the day computation writes `rollover` when missing and `localOffset` when the offset changed | N6.3 |
| `pylib/anki/collection.py` | the Python layer's write at open (`creationOffset`) | N6.3 |
| `rslib/src/storage/sqlite.rs` | opening a schema-18 file and closing without the downgrade write nothing | N6.3 |
| `rslib/src/sync/collection/status.rs`, `rslib/src/collection/timestamps.rs`, `rslib/src/storage/collection_timestamps.rs` | sync status from `mod`, `scm` and `ls`; a never-synced collection | N6.1 |
| `rslib/src/collection/mod.rs` (preparing an upload) | which tables' pending usns a full upload clears (config is not among them) | N6.1, N9.3 |
| `pylib/tests/test_schedv3.py`, `rslib/src/scheduler/answering/preview.rs`, `rslib/src/scheduler/service/mod.rs` | the harvested filtered-deck tests classified preservation-only for AE83 (`conformance/catalog/harvest.csv`; ids in `oracle/harvest/links.csv`) | N10 |

Black-box findings the recipe relies on (checked on the oracle):

- Anki creates the cards of one cloze note in no set order, so the recipe adds
  its second cloze by a note edit; an edit's new card gets no logical id from
  the driver, and `adopt` numbers it `#c19`.
- Anki writes several top-level members of a record's `other` in no set
  order, so each recipe record keeps at most one; nested objects come out
  sorted.
- Anki's new tags are stored collapsed (`tags.collapsed` 1).
- Opening and closing the recipe's collection 100 times over eight days, with
  rollovers between, writes nothing; 200 answers write no config key; a note
  edit changes only that note and `col.mod`.

## Notes for the oracle maintainer

- The maker runs the recipe as `tools/mg oracle record` would, minus the last
  observe step, and keeps the collection file (normalized-dump.md N9.2). The
  stock note types are deleted by the recipe's `db_exec` steps: turn secure
  deletion on for the connection before the recipe runs, or vacuum the closed
  file, then run the storage maker's stock-text check on the result.
- `adopt` (N5): number `session.notes` from `select id from notes order by id`
  and `session.cards` from `select id from cards order by id`, skipping ids
  already in the tables; output the two counts. Refuse it in actor scenarios
  (`invalid_scenario`).
- `db_dump` with `raw: true` (N4) is `converge`'s raw dump restricted to the
  requested tables.

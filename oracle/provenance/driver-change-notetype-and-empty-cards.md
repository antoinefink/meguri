# Oracle driver: Change Note Type and Empty Cards ops (U35)

Read at tag 26.09.3 to choose the `anki` Python APIs behind
`change_notetype_info`, `change_notetype`, `empty_cards` and
`delete_empty_cards` (`driver/ops/change_notetype.py`,
`driver/ops/empty_cards.py`; meanings in
`specs/conformance/change-notetype-ops.md` and
`specs/conformance/empty-cards-ops.md`); no code was copied. The specs' own
sources are in `change-notetype-and-empty-cards.md`.

| Anki file | Read for |
|---|---|
| `rslib/src/notetype/mod.rs` | the single-note-type check: the first id must exist (not found), then the notes of its note type among the ids must be as many as the ids, duplicates included (multiple note types) |
| `qt/aqt/changenotetype.py`, `qt/aqt/browser/browser.py` | Browse runs that check on its selected notes before the screen opens; the save adds the selection's ids to the screen's request and reports their count |
| `pylib/anki/models.py`, `pylib/anki/_backend.py` | `change_notetype_info` and `change_notetype_of_notes` (raw request bytes); the multiple-note-types error reaches Python as a plain invalid input, a missing note as `NotFoundError` |
| `rslib/src/notetype/notetypechange.rs`, `rslib/src/notetype/service.rs`, `proto/anki/notetypes.proto` | the info's request carries the default maps (-1 for none, no template entries and `is_cloze` when either side is cloze) and the stamp read from `col`; the change checks the stamp first (a generic invalid input) and fails on a missing note inside its transaction |
| `rslib/src/collection/transact.rs` | a failed operation rolls back; one that changes nothing bumps no `mod` |
| `ts/routes/change-notetype/lib.ts` | the unused fields and templates the screen lists, and its unchanged check before the save |
| `rslib/src/card_rendering/service.rs`, `rslib/src/notetype/emptycards.rs`, `proto/anki/card_rendering.proto` | the report's notes: empty card ids sorted by ordinal then id (the report text sorts them first), the all-empty flag, note types in the name query's order |
| `qt/aqt/emptycards.py`, `pylib/anki/collection.py` | the screen's delete list (an all-empty note keeps its first listed card when keep notes is on) and `remove_cards_and_orphaned_notes` |
| `rslib/src/card/service.rs`, `rslib/src/card/mod.rs` | the removal is one undoable Empty Cards operation; its count is the listed cards that still existed |

Probed in the oracle image (`driver/tests/test_change_notetype.py`,
`driver/tests/test_empty_cards.py`) and by recording every scenario under
`conformance/scenarios/notetypes/change/` and `conformance/scenarios/cardgen/empty/`:
each golden agrees with what its scenario's comment and the two specs state,
`cardgen/empty/recheck` included (the values its deviations replace).

The screen's unchanged check compares an absent template map (either side
cloze) with an empty list and finds them different, so Anki's screen saves a
cloze note type changed to itself with the identity field map. The driver
refuses it with `no_changes`, as `specs/notetypes/change-notetype.md` M7 says;
no scenario reaches it (reported to the spec writer).

Ported unchanged in behavior from the U35 driver (2026-10-16) on
2026-10-27 for the current specs: the change runs as accepted and no schema
question is shown or answered (the confirmation lives in the desktop UI, which
the oracle never runs; the pylib call asks nothing), so the scenarios carry
no confirmation input (`specs/conformance/change-notetype-ops.md` H2, H3).

# Oracle driver: deck and preset ops (U28)

Read at tag 26.09.3 (`refs/anki-26.09.3`) and in the oracle image's `anki`
26.9.3 wheel to choose the APIs behind the deck ops, the preset ops and the
`decks` and `deck_tree` observations (`driver/ops/decks.py`,
`driver/ops/presets.py`, `driver/observe/decks.py`; meanings in
`specs/conformance/deck-ops.md` K1-K12). No code was copied. The behavior the
ops run is the spec writer's reading in `decks.md`.

| Anki file | Read for |
|---|---|
| `qt/aqt/operations/deck.py` | the desktop's calls: Add Deck is `add_normal_deck_with_name`, rename `rename`, drag and drop `reparent`, delete `remove`, collapse `set_collapsed` once per deck, select `set_current` |
| `pylib/anki/decks.py` | those methods (an existing name returns its id without a backend op; `reparent` with 0 for the top level), `deck_and_child_ids`, `get_deck_configs_for_update`, `add_config` (a clone is the source's legacy object with id 0), `remove_config` (moves decks to preset 1, so not used: K9 moves them to the fallback) |
| `pylib/anki/_backend.py` | `FilteredDeckError` and `InvalidInput` are their own pylib classes, so the refusals are caught by class |
| `rslib/src/decks/addupdate.rs`, `name.rs`, `reparent.rs` | a filtered nearest parent or drop target is `FilteredDeckError` (must be a leaf), the nesting limit an `InvalidInput` from the parent search; each op runs in one transaction that a refusal rolls back |
| `rslib/src/decks/service.rs`, `remove.rs`, `tree.rs`, `current.rs` | the getters and every deck op read rows through storage, not through the deck cache that raw SQL leaves alone (`rslib/src/backend/dbproxy.rs`); a tree without a timestamp has no counts and runs no unbury; the default deck is hidden by the backend |
| `rslib/src/deckconfig/mod.rs`, `service.rs` | the legacy add assigns an id and stamps; the legacy update of an existing id always stamps and writes, unchanged or not; removal refuses id 1 (`InvalidInput`), sets `scm` and runs without undo |
| `rslib/src/deckconfig/update.rs` | the deck options screen's `defaults` (the built-in defaults with the FSRS-6 default parameters), removals before saves, decks of removed or missing presets moved through the deck update path, an unchanged preset skipped by its update path |

Probed with the 26.9.3 wheel (macOS native lane's venv, one-off runs) and in
the oracle image (`driver/tests/test_decks.py`):

- a preset added through the legacy storage call with the screen's FSRS-6
  defaults has the same config bytes as one added by the deck options save;
- a legacy clone (source object, id 0, new name) and a clone made by the deck
  options save both store the source's config bytes, `other` included;
- the legacy update of a preset to its own name stamps it and moves `col.mod`,
  so `rename_preset` makes no call for an unchanged name, as the screen does;
- the deck kind blob of a filtered deck reads as `filtered` through the deck
  getter; a `db_exec` deck row is found by its name at the next step.
- Anki writes the `other` JSON members of a deck saved from its legacy form in
  a random order (a hash map): `decks/rename/preserve` gave either order from
  one run to the next while it stored two members and read the common bytes.
  It now stores one member while it reads them (steps 1 and 4) and adds the
  second after that; five compares of the re-recorded golden matched.

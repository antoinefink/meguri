# Notes and card generation (U27)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/notes/note-records.md`,
`card-generation.md`, `deck-placement-and-position.md`, `field-check.md`,
`deleted-notes-log.md` and `specs/conformance/note-ops.md`. No code was copied.
Every statement the scenarios can show was checked against the oracle's
goldens (`conformance/goldens/notes/`, `conformance/goldens/cardgen/`).

| Spec section | Anki file | Read for |
|---|---|---|
| note-records N1, A2, U3 | `rslib/src/storage/note/add.sql`, `update.sql`, `mod.rs` | columns written on add and update (flags and data only on add); id candidate |
| note-records N2, A2 | `rslib/src/notes/mod.rs` (`prepare_for_update`, `normalize_field`), `rslib/src/config/bool.rs` | every field normalized on each save; `normalize_note_text` defaults to true and an unreadable value reads as true |
| note-records A2, A4 | `rslib/src/notes/mod.rs` (`add_note_inner`), `rslib/src/config/notetype.rs`, `rslib/src/config/deck.rs`, `rslib/src/config/mod.rs` | order of an add; the three keys written with the deck given, whatever deck the cards got; key spellings |
| note-records A5 | `rslib/src/notes/mod.rs` (`add_notes`), `rslib/src/notes/service.rs` | bulk add as one transaction of single adds |
| note-records U2 | `rslib/src/notes/mod.rs` (`update_note_inner`, `note_differs_from_db`) | the comparison runs on the values as given, before normalization and tag canonicalization; sfld, csum and mod are not compared (deviation `note-save-same-after-normalizing`) |
| note-records F1 | `rslib/src/notes/mod.rs` (`fix_field_count`), `rslib/src/dbcheck.rs` (`check_notetypes`) | pad and merge rule; Check Database tags `db-check` and saves without stamping |
| note-records D1, D2 | `rslib/src/notes/mod.rs` (`remove_notes_inner`), `rslib/src/card/mod.rs` (`remove_cards_and_orphaned_notes`), `pylib/anki/collection.py` (`remove_notes`, `remove_notes_by_card`) | removal order, skipped missing notes; the hook that the desktop shell uses for the log |
| deleted-notes-log L1 to L5 | `qt/aqt/main.py` (`onRemNotes`), `docs-site/manual/backups.mdx` | file name and place (profile folder, next to the collection), header lines, columns (Anki writes the note id under the guid header), append mode, rows selected with `id in (…)` |
| card-generation G2, G3 | `rslib/src/notetype/cardgen.rs` (`new_cards_required_normal`), `rslib/src/notetype/mod.rs` (`SPECIAL_FIELDS`), `rslib/src/template.rs` (`renders_with_fields`) | the non-empty name set and the strict analysis; a front that fails to parse makes no card |
| card-generation G4, G5 | `rslib/src/notetype/cardgen.rs` (`new_cards_required_cloze`), `rslib/src/cloze.rs` (`cloze_number_in_fields`) | union over all fields, number - 1 capped at 499; one card per number, so several numbers of 500 and above give several cards of ordinal 499 (deviation `cloze-ordinal-499-once`); hash-set order of creation |
| card-generation G6, G7 | `rslib/src/notetype/cardgen.rs` (`new_cards_required`, `extract_data_from_existing_cards`) | ordinal 0 when no card exists and none is required, also on existing notes; existing ordinals skipped |
| card-generation G9 | `rslib/src/notetype/cardgen.rs` (`generate_cards_for_notetype`), `rslib/src/notetype/schemachange.rs` (`update_cards_for_changed_templates`, `should_generate_cards`), `rslib/src/storage/notetype/existing_cards.sql` | notes with cards only; the count shortcut for standard note types; no last-deck target |
| deck-placement D1 to D4 | `rslib/src/notetype/cardgen.rs` (`add_generated_cards`, `deck_for_adding`, `deck_conf_if_normal`), `rslib/src/notetype/templates.rs` (`target_deck_id`), `rslib/src/storage/notetype/existing_cards.sql` | one candidate checked, then Default; home deck from odid; the G6 card has no override; cloze ignores overrides |
| deck-placement D2, D3 (deviations) | `rslib/src/notetype/cardgen.rs` at `refs/anki` main 2f62b5e67807, test `deck_for_adding_falls_back` only (read to classify the supplementary harvest row) | Anki's main branch walks two candidates before Default, as R791 and R792 describe; the 26.09.3 oracle does not |
| deck-placement P1 to P4 | `rslib/src/config/mod.rs` (`get_and_update_next_card_position`), `rslib/src/notetype/cardgen.rs` (`due_for_deck`, `random_position`), `rslib/src/storage/notetype/existing_cards.sql`, `rslib/src/deckconfig/schema11.rs`, `proto/anki/deck_config.proto`, `Cargo.lock` | counter read as u32 with wrapping increment, once per note; sibling position from type-0 cards (odue first, negatives as 0); i32 cast into `due`; seeded StdRng from rand 0.9.4 and its range; preset fallback; schema-11 `new.order` mapping |
| field-check K2 to K4 | `rslib/src/notes/mod.rs` (`note_fields_check`, `is_duplicate`, `field_cloze_check`), `rslib/src/notetype/mod.rs` (`cloze_fields`), `rslib/src/cloze.rs` (`contains_cloze`), `rslib/src/search/sqlwriter.rs` (`write_dupe`), `rslib/src/storage/note/mod.rs` (`note_fields_by_checksum`) | state order, cloze fields from the first template's front, top-level cloze presence, duplicate lookup through the stored csum, dupe search |
| note-ops N1 to N4 | `pylib/anki/models.py` (`add_dict`), `pylib/anki/collection.py`, `proto/anki/notetypes.proto` (template `Config` field 5) | setup note types are checked saves; template override field number |

Harvest: the notes-area rows of `conformance/catalog/harvest.csv` were
classified from the tests listed in `oracle/harvest/links.csv`. The card
generation rows of the notetypes area (cardgen.rs tests) were classified by
U29's spec writer as deferred to U27; their scenarios are the `cardgen/`
ones (see the U27 spec step's open points).

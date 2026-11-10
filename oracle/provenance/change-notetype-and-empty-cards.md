# Change Note Type and Empty Cards (U35)

Spec-writer notes for `specs/notetypes/change-notetype.md`, `specs/notes/empty-cards.md`,
`specs/conformance/change-notetype-ops.md` and `specs/conformance/empty-cards-ops.md`.
Read at tag 26.09.3 (`refs/anki-26.09.3`); no code was copied. The U35 spec session
ran black-box probes in the oracle image (`meguri-oracle:26.9.3-7bd2ad34fc46248d`)
through the `anki` Python API with no network: default maps for name matches, gaps
and letter case; card removal, renumbering and generation for standard,
cloze-to-standard and to-cloze changes; the deck and position of generated cards
(shared home deck, the new note type's last deck, the counter for a note without a
new card, the card of ordinal 0 when every template was dropped); NFD text and
control characters through a change; the stale-stamp and missing-note refusals and
their rollback; the single-note-type check's errors (an empty list, a missing first
note, a missing later note, mixed note types); the empty-cards report's order (note
types by name under unicase, notes by id, cards by ordinal then id), its all-empty
flag and labels, the screen's keep rule, the delete's count, graves and undo label.

Updated 2026-10-27 for the current text. The change takes no confirmation input
(`notetypes/schema-changes#G4`); its safety backup (M8 check 5), the second run of
checks 3 and 4 inside the write transaction, the shape check, `no_changes` in the op,
the structure report and the transfer rules of M10 are Meguri's own design and have
no Anki source. So are the delete's second look (E4 steps 1 and 2, E5) and the
`kept_valid` count.

| Spec section | Anki files read | Read for |
|---|---|---|
| change-notetype M1 to M3 | `rslib/src/notetype/notetypechange.rs`, `ts/routes/change-notetype/lib.ts`, `ts/routes/change-notetype/Mapper.svelte`, `ts/routes/change-notetype/Alert.svelte`, `ftl/core/change-notetype.ftl` | default maps (exact names, then unused old ordinals in order), no template map when either side is cloze, one old template per slot in the screen |
| change-notetype M4 | `rslib/src/notetype/mod.rs` (single note type of notes), `qt/aqt/changenotetype.py`, `qt/aqt/browser/browser.py`, `ftl/core/errors.ftl` | which errors the selection check gives, in what order, and its message |
| change-notetype M5, M6 | `rslib/src/notetype/service.rs` (info and request conversion), `proto/anki/notetypes.proto`, `ts/routes/change-notetype/lib.ts` (unused items), `ftl/core/change-notetype.ftl` | what Anki's info returns (names, default maps, stamp; no counts), how the screen lists discarded fields and removed cards and its wording |
| change-notetype M7 | `ts/routes/change-notetype/lib.ts` (unchanged check), `ftl/core/change-notetype.ftl` | the screen refusing an unchanged save before the backend sees it |
| change-notetype M8 checks 3, 4 | `rslib/src/notetype/notetypechange.rs`, `rslib/src/error/invalid_input.rs` | the stamp check and its generic error, the missing-note failure and rollback, no note type check at the change |
| change-notetype M9 | `rslib/src/notetype/notetypechange.rs`, `rslib/src/notes/mod.rs` (note update with card generation, field normalization), `rslib/src/notetype/cardgen.rs`, `rslib/src/collection/timestamps.rs`, `rslib/src/card/mod.rs`, `rslib/src/card/undo.rs`, `rslib/src/storage/card/mod.rs`, `rslib/src/config/notetype.rs`, `rslib/src/ops.rs`, `ftl/core/browsing.ftl` | order of effects, which cards are deleted or renumbered, the note rewrite without NFC or tag registration, generation for every note with the new note type's last deck, undo label, result count and its message |
| change-notetype M11 | `rslib/src/notetype/notetypechange.rs`, `rslib/src/notetype/mod.rs` (single note type of notes) | what the backend does with inputs no screen sends |
| empty-cards E1, E2 | `rslib/src/notetype/emptycards.rs`, `rslib/src/notetype/cardgen.rs`, `rslib/src/storage/notetype/mod.rs`, `rslib/src/storage/notetype/existing_cards.sql`, `rslib/src/storage/notetype/get_notetype_names.sql`, `rslib/src/card_rendering/service.rs`, `ftl/core/empty-cards.ftl` | emptiness without existing cards and without the always-one-card rule, report order (the name query is served by the name index), labels and report wording, all-empty flag |
| empty-cards E4, E5 | `qt/aqt/emptycards.py`, `qt/aqt/forms/emptycards.ui` (the keep option checked when the report opens, never saved), `rslib/src/card/mod.rs` (removal of cards and orphaned notes), `rslib/src/card/service.rs`, `rslib/src/storage/note/is_orphaned.sql`, `pylib/anki/collection.py`, `qt/aqt/main.py` (deleted-notes log hook), `ftl/core/empty-cards.ftl`, `rslib/src/card/undo.rs`, `rslib/src/notes/undo.rs`, `rslib/src/ops.rs` | the keep rule (first listed card of an all-empty note), no second look in Anki, the count and its message, note graves, no log line, the undo label; undo and redo of the delete (removed cards and notes restored as stored, `usn` included, their graves removed, then added again on redo; `maintenance/empty-cards/undo-redo`) |
| empty-cards E4 sync paragraphs | `rslib/src/collection/mod.rs` (state before a full upload), `rslib/src/sync/collection/finish.rs` (state after a normal sync) | `ls` equal to `scm` after a full upload, `ls` set to the server's finish time after a normal sync: why `maintenance/empty-cards/incremental-sync` reads `scm = ls` only before its normal sync |
| conformance/change-notetype-ops H1 to H3 | `pylib/anki/models.py` (change info, change of note type), `rslib/src/notetype/service.rs`, `ts/routes/change-notetype/lib.ts` | which calls the oracle makes and how the screen derives the preview |
| conformance/empty-cards-ops Y1 to Y3 | `pylib/anki/collection.py` (empty-cards report, card removal), `qt/aqt/emptycards.py`, `proto/anki/card_rendering.proto` | the report's shape and the screen's card list |

## Driver hints for the oracle maintainer (U35 role work)

- `change_notetype_info`: the selection check can be done in SQL with Anki's rule (the first listed note missing: `not_found`; any other missing or of another note type: `multiple_notetypes`); pylib raises the multiple-note-types case as a plain invalid-input error, so do not tell the two apart by exception class. Then the model manager's change info for the first note's note type and the target. Default maps come back with -1 for none and an empty template list when there is no template map (the request's cloze flag is set then); report `null` and `null`. Compute `discarded_fields` and `dropped_templates` as the screen's unused items, `removed_cards` with M6, `field_content` from `notes.flds` split at U+001F (non-empty strings; a note with fewer stored fields counts the missing ones as empty), `card_counts` with `GROUP BY ord`. `schema` is the info's stamp.
- Keep per session: the real note ids (first occurrences), old and new note type ids, the default maps and the stamp. A refused info step forgets what was kept, so the next `change_notetype` is `invalid_scenario` (H1).
- `change_notetype`: check the maps' shape against the kept note types first (`invalid_scenario`), then `no_changes` (same note type, identity field map, identity or absent template map), then compare the kept stamp with `col.scm` (`schema_changed`; Anki's own refusal is a generic invalid-input error), then build the request from the kept info with the maps (-1 for null, no template entries when absent) and call the model manager's change of note type. A not-found backend error is `not_found`; Anki rolls the transaction back. Never show or answer a schema-change question (`notetype-ops.md` T9). Do not add cards it creates to the session's logical ids.
- `empty_cards`: the collection's empty-cards report; keep the protobuf. For each listed note read `ord` of each listed card and the note's card count from the rows; group by the note type's stored name in report order. The report's card ids are already sorted by ordinal, then id (the report text is built first and sorts them).
- `delete_empty_cards`: as the screen: every listed card, or for an all-empty note with `keep_notes` all but its first listed card; one call of the collection's removal of cards and orphaned notes (it is undoable, op "Empty Cards"); `deleted` is the call's count; `kept_valid` 0.
- The probes above used the real clock; under the worker's frozen clock ids and stamps come from the scenario's instant as usual.
- H4 and Y4 give the order in which the language, the oracle and Meguri gain these ops; the scenarios of M12 and E7 are not on this branch yet.

## Harvest

The U35 rows of `conformance/catalog/harvest.csv` (the single note type of selected notes, the field and template mapping tests, Empty Cards with special fields, removal of cards with the notes they leave empty) stay `deferred` until the scenarios of M12 and E7 land with their goldens. The change screen's request-message conversion test fits not-applicable, like the Rust library's conversion tests U29 classified; the empty-cards report tests about markup in names belong to U131's report (E2 strips names).

# Note type structure changes and Restore to Default (U34)

Spec-writer notes for `specs/notetypes/structure-changes.md`,
`specs/notetypes/restore-to-default.md` and
`specs/conformance/notetype-structure-ops.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. The U34 spec session ran
black-box probes in the oracle image (`meguri-oracle:26.9.3-*`) through the
`anki` Python API with no network: how a field rename or removal rewrites the
four template texts (canonical tags, dropped alternate-delimiter directive,
the first field appended to empty browser fronts, broken texts left alone,
swapped names, names fixed by the save's normalization), the stale card
requirements after a rename, note stamps and `sfld`/`csum` after field moves,
sort field changes and removals, card renumbering, removal with graves and
placement of generated cards (the last deck skipped), note type removal
(graves, the `_nt_<id>_` keys, `curModel` chosen through the name index),
Restore to Default with stored, forced and unknown kinds (and its missing
`scm` stamp when a forced kind only adds templates), and undo of a template
add, a field move and a note type removal.

Updated 2026-10-27 for the current text. Meguri's ops take no confirmation
input (`notetypes/schema-changes#G4`); the safety backup, the base token,
`Stale`, `ProtectedField`, `NoteFieldCountMismatch`, `LastNotetype`, the
removal preview's note count, planning with progress and cancel, and the
structure report are Meguri's own design and have no Anki source.

| Spec section | Anki files read | Read for |
|---|---|---|
| structure-changes S1, S1.1 | `rslib/src/notetype/mod.rs` (update and add paths, save preparation), `rslib/src/notetype/schemachange.rs`, `rslib/src/notetype/service.rs`, `pylib/anki/models.py` (field and template helpers, `update_dict`), `qt/aqt/fields.py`, `qt/aqt/clayout.py`, `qt/aqt/models.py` | how Anki matches a saved note type to the stored one by ordinal, what its field and template editors send in one save, the undo name |
| structure-changes S1.2 items 2, 3 | `rslib/src/notetype/mod.rs` (save preparation), `rslib/src/notetype/fields.rs`, `qt/aqt/fields.py`, `qt/aqt/clayout.py` | the empty-list refusals; that the core removes a protected field when asked and only the field editor withholds it; the editors' last-field and last-template refusals |
| structure-changes S1.2 item 4 | `rslib/src/notetype/schemachange.rs` (notes for changed fields), `rslib/src/notes/mod.rs` | Anki padding and truncating a note's values when the field list changes, and refusing the save when only the sort field changes |
| structure-changes S2, S3 | `rslib/src/notetype/mod.rs`, `rslib/src/notetype/schemachange.rs` (ordinal change detection, front comparison) | which comparisons classify a save, the order of its effects |
| structure-changes S4, S4.1 to S4.4 | `rslib/src/notetype/mod.rs` (renamed and removed fields, template update), `rslib/src/template.rs` (renaming and removal on the parse tree, writing it back, replacement and cloze detection) | which tags are renamed or removed, the append rule for fronts and cloze texts, which texts are skipped, how Anki writes a template back (the Anki column of S4.4 and S14) |
| structure-changes S5 | `rslib/src/notetype/schemachange.rs` (notes for changed fields), `rslib/src/notes/mod.rs` (note preparation and stamps) | field remap by ordinal, normalization, `sfld`/`csum`, every note of the type stamped |
| structure-changes S6, S7 | `rslib/src/notetype/schemachange.rs` (cards for changed templates, when to generate), `rslib/src/notetype/cardgen.rs` (whole-note-type generation, placement, positions), `rslib/src/card/mod.rs`, `rslib/src/notes/mod.rs` (card removal with graves) | removal before renumbering, the generation trigger, notes without cards skipped, the last deck unused (deviation `deck-placement-regeneration`) |
| structure-changes S8 | `rslib/src/notetype/mod.rs` (requirement update order), `rslib/src/template.rs` (requirements) | requirements computed before the rewrite (deviations `notetype-reqs-after-field-change` and `notetype-reqs-after-field-change-removal`) |
| structure-changes S9 | `rslib/src/notetype/mod.rs`, `rslib/src/storage/notetype/mod.rs`, `rslib/src/notetype/schemachange.rs` | row writes and stamps of a structural save, when `col.scm` moves |
| structure-changes S11 | `qt/aqt/clayout.py` (card type removal), `ftl/core/card-templates.ftl`, `pylib/anki/models.py` (template use count) | the wording of the card type removal question and that Anki reports the card count only |
| structure-changes S12 | `rslib/src/notetype/mod.rs` (removal, the stock Basic note type added when the last one goes), `rslib/src/config/notetype.rs`, `rslib/src/config/mod.rs` (prefix removal), `rslib/src/storage/notetype/mod.rs`, `rslib/src/storage/notetype/get_notetype_names.sql`, `rslib/src/notes/mod.rs`, `qt/aqt/models.py`, `ftl/core/notetypes.ftl` | graves, removed config keys, the current note type after removal (unordered read answered from the name index), the delete questions' wording, the UI's last-note-type refusal |
| structure-changes S13 | `rslib/src/notetype/undo.rs`, `rslib/src/card/undo.rs`, `rslib/src/notes/undo.rs`, `rslib/src/collection/undo.rs` | undo names and what undo restores (added cards removed without graves) |
| structure-changes S12 Undo, S13 (scenarios `undo-add-template`, `undo-removals`, `undo-restore`, 2026-11-10) | `rslib/src/notetype/mod.rs` (removal: notes, schema stamp, per-type config keys, current note type), `rslib/src/config/mod.rs`, `rslib/src/config/undo.rs`, `rslib/src/card/undo.rs` (card removal with its grave, both undoable), `rslib/src/collection/undo.rs` (schema stamp restored), `rslib/src/collection/transact.rs` (no history clearing for schema changes) | that undo restores the removed config keys, `curModel`, the card graves and `col.scm`; that a structural save is undoable right after the oracle's reopen |
| structure-changes S14, S15 | the files of S4, S7 and S8 | the Anki column of each deviation; R924's wording against Anki's append rule |
| restore-to-default D1, D2 | `qt/aqt/clayout.py` (kind picker, the restore question and its default answer), `ftl/core/card-templates.ftl`, `rslib/src/notetype/restore.rs`, `proto/anki/notetypes.proto` (stock kind enum) | when the kind is asked for, the stored kind used otherwise, Anki's invalid-input refusal for an unknown kind |
| restore-to-default D3 | `rslib/src/notetype/restore.rs`, `rslib/src/notetype/stock.rs`, `rslib/src/notetype/mod.rs` (save preparation) | what is replaced and what is kept, the kind written only when forced, the sort field index lowered |
| restore-to-default D4 | `rslib/src/notetype/restore.rs`, `rslib/src/notetype/schemachange.rs`, `rslib/src/notetype/mod.rs` (rename map applied to the stock text) | ordinals by position, protected fields past the stock count removed, Anki's rewrite of the stock text by position |
| restore-to-default D5, D6 | `rslib/src/notetype/restore.rs`, `rslib/src/notetype/schemachange.rs`, `rslib/src/notetype/undo.rs` | the schema stamp skipped when only templates are added (deviation `restore-template-add-schema`), the undo name |
| conformance/notetype-structure-ops M1 to M3 | `pylib/anki/models.py` (removal, restore-to-stock with a forced kind), `rslib/src/notetype/restore.rs`, `proto/anki/notetypes.proto` | which calls the oracle makes and what Anki refuses with a generic error |
| structure-changes S1.2 item 2, S3 step 2 (2026-11-10 review) | `rslib/src/notetype/mod.rs` (the update preparation) | the no-field and no-template refusals come before the rename and removal rewrite, so the rewrite never runs without a first field |
| structure-changes S12 step 5, scenario `undo-removals` (2026-11-10 review) | `rslib/src/notes/mod.rs` (note add), `rslib/src/notetype/mod.rs` (removal) | every note add writes the current note type, so the last setup note decides `curModel` before the removal; the removal then writes the first name of the name-ordered read |
| conformance/notetype-structure-ops M5 item 2 (2026-11-10 review) | oracle driver `ops/notetypes.py` | `update_notetype`, `remove_notetype` and `restore_notetype` reopen before the step; `notetype_templates` and `notetype_structure` read through the protobuf getter with no reopen |

## Driver hints for the oracle maintainer (U34 role work)

- `remove_notetype`: reopen, find the note type by its stored name, remove it with the model manager's removal (the backend's note type removal). No output, no rejection.
- `restore_notetype`: reopen, find the note type; without `kind`, read `original_stock_kind` from the protobuf getter and record `needs_stock_kind` when it is not 1 to 6, before calling Anki (Anki's own refusal is a generic invalid-input error); otherwise call the model manager's restore-to-stock with the stock kind enum of the `kind` name (the names of `add_stock_notetype`) as the forced kind, or none.
- `notetype_templates`: read through the protobuf getter with no reopen (a reopen would empty the undo history between a step and its undo), and for each template in ordinal order report its name and `q_format`, `a_format`, `q_format_browser`, `a_format_browser` as stored.
- The oracle never shows or answers a schema-change question (`notetype-ops.md` T9); `notetype-structure-ops` M5 gives the order in which the language, the oracle and Meguri gain the three.
- While the specs were written, a throwaway prototype of these three (loaded through `ORACLE_EXTRA_MODULES` from the scratch directory, not committed) ran every scenario then written for `conformance/scenarios/notetypes/structure/`; its results agreed with what each scenario states and with the pointers of `template-rewrite-keeps-text` (`rewrite-format`) and `restore-template-add-schema` (`restore-forced-kind`). Those scenarios and their goldens are now on master (S16, D8).

## Harvest

The U34 rows of `conformance/catalog/harvest.csv` stay `deferred` until the scenarios of S16 and D8 land with their goldens. When they are classified, tests of the name format of per-template config keys (`_nt_<id>_<key>_<ordinal>`) fit preservation-only: Meguri never writes such keys, keeps them, and removes them with their note type (S12 step 3).

## Session s-20261117T144450Z-f49841c9 (spec writer): deferred harvest rows, batch 1

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows and write their scenarios; no code, test text or fixture value was copied, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| notetypes/structure-changes S2, S4.1-S4.3, S5; scenarios notetypes/structure/field-sequence, field-ord-changes, rename-then-remove, remove-crucial-fields | `pylib/tests/test_models.py` (test_fields), `rslib/src/notetype/schemachange.rs` (tests fields, field_renaming_and_deleting, ord_changes), `rslib/src/notetype/mod.rs` (tests update_templates_after_removing_crucial_fields, renaming_field_in_cloze_conditional_does_not_duplicate_replacement), `rslib/src/notetype/service.rs` (test update_notetype_field_name_persists), `pylib/anki/models.py` (scmhash helper only, to confirm it is a pylib-only helper) | Which behaviors the harvest rows oblige: field add, remove, move and rename effects on notes; ordinal-change classification; template reference rewrite on rename and removal, including the cloze append rule. Expectations come from the oracle goldens, not from these assertions. |
| notetypes/structure-changes S2, S6, S7; scenarios notetypes/structure/template-swap-replace, template-draft-mix | `pylib/tests/test_models.py` (test_templates), `rslib/src/notetype/schemachange.rs` (test template_changes) | Which template move, removal and add combinations must be proven (swap, removal with renumbering, same-count replacement, mixed draft with removals, moves and additions). Expectations come from the oracle goldens. |
| notetypes/restore-to-default D2-D5; scenario notetypes/structure/restore-field-count | `rslib/src/notetype/restore.rs` (test adding_and_removing_fields_and_templates) | Which restore sequence must be proven (forced kinds that add and then remove fields and templates, and a standard-to-cloze change). Expectations come from the oracle goldens. |

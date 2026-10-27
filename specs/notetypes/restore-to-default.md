# Restore to Default

Spec: notetypes/restore-to-default
Requirements: R914, R949, R1475, R1477, R2008 (core side)
Anki: 26.09.3
Scenarios: conformance/scenarios/notetypes/structure/ (`restore-*`)

"Restore to Default" (R914, offered from the Note Types screen and the template editor, R919, R932) replaces a note type's fields, templates and styling with Meguri's stock version of its kind, keeping the note type itself and its notes. The stock kinds are `notetypes/stock-kinds` (K1 to K6); what happens to notes and cards reuses `notetypes/structure-changes` S5 to S7.

## D1. Input

- The note type id.
- Optionally a stock kind (one of K1's six). The template editor asks for one only when the note type's kind is unknown (D2); when given, it wins over the stored kind.

The caller asks R914's question first, with No as the default, and submits the op only after the user accepts; the op itself asks nothing and takes no confirmation input (`notetypes/schema-changes#G4`).

## D2. Which stock kind

- With a kind given: that kind.
- Otherwise the note type's `original_stock_kind` when it is 1 to 6 (K1).
- Otherwise (0, unknown, or any other value): the op returns `NeedsStockKind` and writes nothing; the caller lets the user pick a kind and submits again with it. Anki refuses with an invalid-input error there.

A note type made from scratch (scenario setup, the local API's model creation) has `original_stock_kind` 1 (`notetypes/records` N5.1), so restoring it without a kind gives Basic.

## D3. The restored note type

| Value | After the restore |
|---|---|
| fields | the stock kind's fields (K4) with their stock options, tags and protection, each with a fresh random id (`notetypes/records` N5.2) |
| templates | the stock kind's templates (K4, K5) with Meguri's stock text, no browser formats, no deck override, fresh random ids, rows with `mtime_secs` 0 and `usn` 0 (N5.3) |
| `css` | the stock kind's styling (K6) |
| `kind`, `original_stock_kind` | the stock kind's, only when a kind was given; otherwise unchanged |
| `sort_field_idx` | the stored index, lowered to the last field when past it (`notetypes/records` N4 step 6) |
| `reqs` | computed from the stock templates (N6) |
| id, name, `latex_pre`, `latex_post`, `latex_svg`, `original_id`, `target_deck_id_unused`, `other` and unknown protobuf fields of the note type | unchanged (R914: "keeps name, id and preserved settings") |

The restore is a checked save (`notetypes/validation` V1); Meguri's stock templates always pass it.

## D4. Notes and cards: by position

The restore is a structure change whose draft (`notetypes/structure-changes` S1) is made by position:

- the stock field at position i has origin i when the note type had a field there, otherwise none; base fields past the stock count are removed;
- the stock template at position i has origin i when the note type had a template there, otherwise none; base templates past the stock count are removed.

Of `notetypes/structure-changes` S1.2's refusals only items 4 (`NoteFieldCountMismatch`) and 6 (`SafetyBackupFailed`) apply. Item 1 does not, since there is no base token (D6); items 2 and 5 do not, since D3's checked save covers the restored note type; item 3 does not either. Decided 2026-10-27: protected base fields past the stock count are removed like any other (Anki does the same). Their protection belongs to the field list being replaced, and the restored list carries the stock kind's own protected fields (K4). Example: an Image Occlusion note type restored with the given kind Basic loses `Header` and `Back Extra` (both protected) and `Comments`. The user chose the restore and accepted R914's question, and the safety backup (D5) and undo (D6) keep the dropped values recoverable.

So each note keeps its content field by position (field i of every note stays field i), loses the content of fields past the stock count, and gets empty values for stock fields past its old count (S5: rewritten, stamped). Cards of templates past the stock count are removed with graves (S6); notes left with no card stay. New templates generate their cards (S7); only notes that still have a card are visited.

Field references are not rewritten (`notetypes/structure-changes` S4 and `notetypes/field-rename` F3 do not run): the stock templates are already written for the stock field names. Anki runs its rewrite with the old names mapped to the stock names by position and applies it to the stock text whenever a name differs or a field is removed, which gives the empty browser fronts the first field and, where old and stock names collide at other positions, points stock references at the wrong field; these differences lie in template text, which scenarios never compare for stock kinds (R1959, K9).

Example (scenario `restore-basic`): a Basic-kind note type `Plain` with fields `Q`, `Back`, `Extra` (sort field `Extra`) and templates `Card 1`, `Card 2`, restored: fields `Front`, `Back` with stock options, template `Card 1`, sort field index 1 (`Back`); note `a`, `b`, `c` becomes `a`, `b`; every `Card 2` card is removed with a grave; a note that had only its `Card 2` card is left with no card; name and id unchanged.

## D5. Schema change and safety backup

- The restore is schema-classified with reason `notetype_restored` (`notetypes/schema-changes` G2) when the number of fields or the number of templates changes (the sort field index is lowered only when the field count shrinks). It then sets `col.scm` to `now_ms` (G5) and, as a structural edit, takes a safety backup after planning and before its first write (`package/backup-triggers` T6.1, T6.2, with T6.1's coalescing). A restore that keeps both counts (only names, options, text and styling change) is not a schema change, leaves `scm` and takes no backup.
- Deviation `restore-template-add-schema` (R1477): when the restore only adds templates (a forced kind with more templates than the note type had, and the same number of fields), Anki does not set `col.scm`, so a normal sync would send a note type whose template count changed. Meguri sets it, as the storage guard requires (G6). Scenario `restore-forced-kind`.
- The restore reports its structure as `notetypes/structure-changes` S9 says, with the by-position origins of D4.

## D6. One step

One transaction and one undo step named "Update Note Type" (U25 step id `update_note_type`), with progress and cancel during planning and the write-phase rules of `notetypes/structure-changes` S10. The live note type is read inside the transaction; there is no base token, since the restore discards the editor's draft by design (the template editor closes after it).

## D7. Result and refusals

- Refusals, writing nothing: `NeedsStockKind` (D2), `NoteFieldCountMismatch` (`notetypes/structure-changes` S1.2 item 4), `SafetyBackupFailed` (D5; `package/backup-triggers` T6.2). There are no others: no `Stale`, `invalid_notetype`, `ProtectedField` or template-check refusal (D4). Planning can also end with `Cancelled` (D6).
- Result as `notetypes/structure-changes` S1.3.

## D8. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `notetypes/structure/restore-basic` | D2 (stored kind), D3, D4, D5 | not yet in `conformance/`: awaits `restore_notetype` |
| `notetypes/structure/restore-forced-kind` | D2 (given kind), D3 kind change, D4 template add and removal, D5; deviation `restore-template-add-schema` | not yet in `conformance/`: awaits `restore_notetype` |
| `notetypes/structure/restore-unknown-kind` | D2: `needs_stock_kind`, then a given kind | not yet in `conformance/`: awaits `restore_notetype` |

Observations use `notetype_structure`, notes, cards and graves only: the restored template text and CSS are each executor's own (R908). The scenarios and the `restore-template-add-schema` entry of `conformance/deviations.yaml` are added with their goldens once the oracle and the scenario schema have `restore_notetype` (`specs/conformance/notetype-structure-ops.md` M5). Meguri's own tests (`notetype_structure.rs`) cover the safety backup of a structural restore, none for a restore that keeps both counts, and `SafetyBackupFailed` leaving the note type unchanged.

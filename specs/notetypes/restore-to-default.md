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
- Otherwise the note type's `original_stock_kind` when it is 1 to 6 (K1) and K1's `kind` for that stock kind (standard or cloze) is the note type's stored `kind`.
- Otherwise (0, unknown, any other value, or a stock kind whose K1 `kind` differs from the stored one): the op returns `NeedsStockKind` and writes nothing; the caller lets the user pick a kind and submits again with it. Anki refuses with an invalid-input error for an unknown kind. For a stock kind of the other `kind` it writes that stock kind's templates into the stored kind: a cloze note type naming a standard stock kind is then refused by the missing-cloze check (`notetypes/validation` V3), and a standard note type naming Cloze or Image Occlusion is saved with cloze templates. Decided 2026-10-27: Meguri asks for a kind in both cases (deviation `restore-kind-mismatch`, no scenario), since stock text of the other `kind` breaks every card of the type, and a picked kind then applies D3's kind change.

A note type made from scratch (scenario setup, the local API's model creation) has `original_stock_kind` 1 (`notetypes/records` N5.1), so restoring a standard one without a kind gives Basic, and restoring a cloze one without a kind returns `NeedsStockKind`.

## D3. The restored note type

| Value | After the restore |
|---|---|
| fields | the stock kind's fields (K4) with their stock options, tags and protection, each with a fresh random id (`notetypes/records` N5.2); the base fields' ids, tags, protection, `other` bytes and unknown protobuf fields are dropped, as in Anki, which replaces the whole field list |
| templates | the stock kind's templates (K4, K5) with Meguri's stock text, no browser formats, no deck override, fresh random ids, rows with `mtime_secs` 0 and `usn` 0 (N5.3); the base templates' ids, `other` bytes, unknown protobuf fields and row stamps are dropped likewise |
| `css` | the stock kind's styling (K6) |
| `kind`, `original_stock_kind` | the stock kind's, only when a kind was given; otherwise unchanged |
| `sort_field_idx` | the stored index, lowered to the last field when past it (`notetypes/records` N4 step 6) |
| `reqs` | computed from the stock templates (N6) |
| id, name, `latex_pre`, `latex_post`, `latex_svg`, `original_id`, `target_deck_id_unused`, `other` and unknown protobuf fields of the note type | unchanged (R914: "keeps name, id and preserved settings") |

The restore is a checked save (`notetypes/validation` V1). D2 makes the restored `kind` always the stock kind's own, so Meguri's stock templates always pass V3; a failure would be a bug in the stock text, reported as an internal error that writes nothing.

## D4. Notes and cards: by position

The restore is a structure change whose draft (`notetypes/structure-changes` S1) is made by position:

- the stock field at position i has origin i when the note type had a field there, otherwise none; base fields past the stock count are removed;
- the stock template at position i has origin i when the note type had a template there, otherwise none; base templates past the stock count are removed.

The origins serve only to map notes, cards and the report (S5 to S7, S9's maps, D5's classification). S1.1's rule that a field or template with an origin keeps the base one's id, tag, protection, `other` bytes, unknown fields and row stamps does not apply: every restored field and template takes D3's values (`notetypes/structure-changes` S9 is written to match).

Of `notetypes/structure-changes` S1.2's refusals only items 4 (`NoteFieldCountMismatch`) and 6 (`SafetyBackupFailed`) apply. Item 1 does not, since there is no base token from the caller (D6's internal precondition replans instead); items 2 and 5 do not, since D3's checked save covers the restored note type; item 3 does not either. Decided 2026-10-27: protected base fields past the stock count are removed like any other (Anki does the same). Their protection belongs to the field list being replaced, and the restored list carries the stock kind's own protected fields (K4). Example: an Image Occlusion note type restored with the given kind Basic loses `Header` and `Back Extra` (both protected) and `Comments`. The user chose the restore and accepted R914's question, and the safety backup (D5) and undo (D6) keep the dropped values recoverable.

So each note keeps its content field by position (field i of every note stays field i), loses the content of fields past the stock count, and gets empty values for stock fields past its old count (S5: rewritten, stamped). Cards of templates past the stock count are removed with graves (S6); notes left with no card stay. Every saved template whose front differs from the base front at its position, new templates included, generates the cards it requires (S7): a restore almost always replaces the fronts, so a kept template gets cards for notes that lacked one and whose fields its stock front now requires. Only notes that still have a card are visited. Example: a Basic and reversed note type whose `Card 1` front was `{{#Extra}}{{Front}}{{/Extra}}`, restored without a kind: a note with only its `Card 2` card and a non-empty `Front` gets a `Card 1` card.

### D4.1 A given kind that changes `kind`

Decided 2026-10-27, data safety first: when a given kind's K1 `kind` differs from the stored one, cards keep their `ord`, and no card is left with an `ord` that has no template.

- **Cloze to standard.** A cloze note type's card ordinals are cloze numbers, and it has one template. A card whose `ord` is below the stock template count becomes the card of the stock template at that position (cloze 1 → `Card 1`; cloze 2 → `Card 2` of a kind with two templates), as in Anki. Every card whose `ord` is at or past the stock template count is removed with its card grave in the write transaction, together with S6's removals and whether or not S6 runs, and counted in `cards_removed` and `notes_without_cards`. Anki leaves those cards on a note type with no template for them, which its Check Database later deletes as cards with missing templates (`notetypes/schema-changes` G2); Meguri removes them at once, so that the collection is never left damaged, and the safety backup (D5) and undo (D6) keep the removed cards recoverable (deviation `restore-kind-change`).
- **Standard to cloze.** The cloze stock kinds have one template, so cards past ord 0 are removed with graves by S6 as templates past the stock count; the ord 0 cards become cloze 1 cards, as in Anki.

Example: a Cloze note type whose notes have cards at ords 0, 1 and 2, restored with the given kind Basic: the ord 0 cards become `Card 1` cards; the ord 1 and 2 cards are removed with graves; a note left with no card stays.

Field references are not rewritten (`notetypes/structure-changes` S4 and `notetypes/field-rename` F3 do not run): the stock templates are already written for the stock field names. Anki runs its rewrite with the old names mapped to the stock names by position and applies it to the stock text whenever a name differs or a field is removed, which gives the empty browser fronts the first field and, where old and stock names collide at other positions, points stock references at the wrong field; these differences lie in template text, which scenarios never compare for stock kinds (R1959, K9).

Example (scenario `restore-basic`): a Basic-kind note type `Plain` with fields `Q`, `Back`, `Extra` (sort field `Extra`) and templates `Card 1`, `Card 2`, restored: fields `Front`, `Back` with stock options, template `Card 1`, sort field index 1 (`Back`); note `a`, `b`, `c` becomes `a`, `b`; every `Card 2` card is removed with a grave; a note that had only its `Card 2` card is left with no card; name and id unchanged.

## D5. Schema change and safety backup

- The restore is schema-classified with reason `notetype_restored` (`notetypes/schema-changes` G2) when the number of fields or the number of templates changes (the sort field index is lowered only when the field count shrinks), or when a given kind changes `kind` (D4.1: card ordinals change meaning, so Cloze to Basic, with two fields and one template on both sides, is schema-classified too; Anki sets `col.scm` for a kind change only when the field count changes or templates are removed, deviation `restore-kind-change`). It then sets `col.scm` to `now_ms` (G5) and, as a structural edit, takes a safety backup after planning and before its first write (`package/backup-triggers` T6.1, T6.2, with T6.1's coalescing). A restore that keeps both counts and the `kind` (only names, options, text and styling change) is not a schema change, leaves `scm` and takes no backup.
- Deviation `restore-template-add-schema` (R1477): when the restore only adds templates (a forced kind with more templates than the note type had, and the same number of fields), Anki does not set `col.scm`, so a normal sync would send a note type whose template count changed. Meguri sets it, as the storage guard requires (G6). Scenario `restore-forced-kind`.
- The restore reports its structure as `notetypes/structure-changes` S9 says, with the by-position origins of D4.

## D6. One step

One transaction and one undo step named "Update Note Type" (U25 step id `update_note_type`), with progress and cancel during planning and the write-phase rules of `notetypes/structure-changes` S10. There is no base token from the caller, since the restore discards the editor's draft by design (the template editor closes after it).

Decided 2026-10-27: the op carries an internal precondition instead, the U29 `AggregateDigest` of the note type as its planning snapshot read it (the same digest as `notetypes/structure-changes#S1.1`'s token), checked inside the write transaction (`engine/op-model` O7). Everything planned from that note type (D2's kind, the by-position draft, D5's classification and backup decision, the card removals and renumbering, the generation plan) is covered by it. On a mismatch (a sync or a local API edit changed the note type while the restore planned or backed up) the transaction rolls back and the op replans once from the live note type, without the caller and without reporting `Stale`; D2 to D5 are decided again, and `NeedsStockKind` or `NoteFieldCountMismatch` from the replan refuse as usual. The replan takes no second safety backup when one was taken (T6.2); when the replan is schema-classified and the first plan was not, it takes one then (T6.1's coalescing applies). If the precondition fails again, the op returns `Stale` and writes nothing; the caller may submit the restore again.

## D7. Result and refusals

- Refusals, writing nothing: `NotFound` when no note type has the id (Anki fails the same way); `NeedsStockKind` (D2, an unknown kind or one of the other `kind`); `NoteFieldCountMismatch` (`notetypes/structure-changes` S1.2 item 4); `SafetyBackupFailed` (D5; `package/backup-triggers` T6.2); `Stale` only when the internal precondition fails a second time after D6's replan. There are no others: no `invalid_notetype`, `ProtectedField` or template-check refusal (D3, D4). Planning can also end with `Cancelled` (D6).
- Result as `notetypes/structure-changes` S1.3.

## D8. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `notetypes/structure/restore-basic` | D2 (stored kind), D3, D4, D5 | recorded |
| `notetypes/structure/restore-forced-kind` | D2 (given kind), D3 kind change, D4 template add and removal, D5; deviation `restore-template-add-schema` | recorded |
| `notetypes/structure/restore-unknown-kind` | D2: `needs_stock_kind`, then a given kind | recorded |

Observations use `notetype_structure`, notes, cards and graves only: the restored template text and CSS are each executor's own (R908). The scenarios, recorded goldens and `restore-template-add-schema` deviation are now present (`specs/conformance/notetype-structure-ops.md` M5). Keep the scenarios inactive until U34 supplies the restore adapter and its required checks pass. Meguri's own tests (`notetype_structure.rs`) cover the safety backup of a structural restore, none for a restore that keeps both counts and the `kind`, and `SafetyBackupFailed` leaving the note type unchanged; `NotFound` for a missing note type id; `NeedsStockKind` for a cloze note type made from scratch (`original_stock_kind` 1) restored without a kind, and for a standard one naming Cloze (deviation `restore-kind-mismatch`); the given kind Basic on a Cloze note type with cards at ords 0, 1 and 2 (D4.1: ord 0 kept, ords 1 and 2 removed with graves, schema-classified with a backup although both counts stay, deviation `restore-kind-change`); D3's fresh ids, stock tags and protection, and dropped `other` bytes on fields and templates that have an origin; a kept template whose front changed generating cards (D4); and a field added by the local API while a restore plans (D6: one replan, the restore applied to the live note type, a backup taken when the replan becomes schema-classified, none twice), and a second change during the replan returning `Stale`. The deviations `restore-kind-mismatch` and `restore-kind-change` are not in `conformance/deviations.yaml`, since no scenario shows them: `restore-forced-kind` forces standard kinds onto a standard note type (for example Basic to Basic and reversed, which only adds a template), so its `kind` never changes.

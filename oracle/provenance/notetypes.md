# Note types: records, stock kinds, validation, schema changes (U29)

Spec-writer notes for `specs/notetypes/records.md`, `specs/notetypes/stock-kinds.md`,
`specs/notetypes/validation.md`, `specs/notetypes/schema-changes.md` and
`specs/conformance/notetype-ops.md`. Read at tag 26.09.3 (`refs/anki-26.09.3`);
no code was copied. Black-box probes ran in the oracle image
(`meguri-oracle:26.9.3-*`) through the `anki` Python API with no network:
name normalization and uniqueness, validation refusals and their help pages,
stock-kind structure and card requirements, stamps of an unchanged save,
which saves move `col.scm`, undo and redo of a schema change, clone, the
Image Occlusion on-demand add, the requirement quirk after a field rename.

| Spec section | Anki files read | Read for |
|---|---|---|
| records N1-N3 | `rslib/src/notetype/mod.rs`, `rslib/src/notetype/undo.rs`, `rslib/src/storage/notetype/mod.rs`, `rslib/src/storage/notetype/*.sql`, `rslib/src/notetype/templates.rs`, `rslib/src/notetype/fields.rs` | aggregate load and write (delete and re-insert of field and template rows), stamping on every update, template row mtime/usn pass-through, undo images |
| records N4 | `rslib/src/notetype/mod.rs` (save preparation, name uniqueness), `rslib/src/notetype/fields.rs`, `rslib/src/notetype/templates.rs`, `rslib/src/text.rs` (NFC), `pylib/anki/models.py` (the Python layer's own rename) | quote removal, NFC, field-name fixes, `+` uniqueness under unicase, sort index repositioning |
| records N5, N5.4, stock-kinds K2-K4 | `rslib/src/notetype/stock.rs`, `rslib/src/image_occlusion/notetype.rs`, `rslib/src/notetype/header.tex`, `rslib/src/storage/sqlite.rs` (new collection), `rslib/src/config/notetype.rs`, `rslib/src/config/mod.rs`, `pylib/anki/models.py` (`new`, `new_field`, `new_template`), `pylib/anki/stdmodels.py`, `proto/anki/notetypes.proto` (field tag enums, stock kind enums) | defaults of new note types, fields and templates; stock kinds, names, tags, protection; new-collection order, stamps and `curModel` |
| records N6 | `rslib/src/notetype/mod.rs` (requirement update order), `rslib/src/template.rs` (requirements) | when requirements are computed and from which template text |
| records N7 | `pylib/anki/models.py` (`copy`), `qt/aqt/models.py` (Add/Clone dialog), `rslib/src/notetype/service.rs` | what a clone keeps and clears, its default name |
| records N8, validation V1 | `qt/aqt/models.py`, `qt/aqt/fields.py`, `qt/aqt/clayout.py`, `qt/aqt/operations/notetype.py`, `qt/aqt/editor_legacy.py`, `ts/routes/editor/StickyBadge.svelte`, `rslib/src/notetype/restore.rs` | which UI saves skip the template checks |
| records N9 | `rslib/src/image_occlusion/notetype.rs` | first valid IO note type, on-demand add keeping the current note type |
| validation V2-V4 | `rslib/src/notetype/mod.rs` (template checks), `rslib/src/error/mod.rs`, `rslib/src/backend/error.rs`, `proto/anki/backend.proto` | check order, special field list, card type error kinds and help pages |
| validation V5 | `qt/aqt/fields.py` | the field editor's name check |
| stock-kinds K5-K7 | `rslib/src/notetype/stock.rs`, `rslib/src/image_occlusion/notetype.rs`, `rslib/src/image_occlusion/notetype.css`, `rslib/src/notetype/cloze_styling.css`, `ts/routes/image-occlusion/review.ts` | which fields each template references (not the text), the image-occlusion element ids, setup call, toggle button and CSS custom properties the runtime reads |
| conformance/render-ops Q4, stock-kinds K9 (live render status) | `rslib/src/template.rs` (card render: question errors, then the blank check, then answer errors), `rslib/src/notetype/render.rs` and `rslib/src/card_rendering/service.rs` (existing-card render: the template at the card's ordinal or the first for cloze, review versus browser templates, a missing template as invalid input), `pylib/anki/template.py` (partial render of an existing card, its empty flag, template errors raised as exceptions) | which statuses a review render of a stored card ends in, and in which order; the same as `collection-check` K8 without the byte counts |
| conformance/notetype-ops T10.2 (the stand-in's default deck and preset) | `rslib/src/notetype/cardgen.rs` (deck for new cards: the named deck when it is a normal deck, else deck 1, else an invalid-input error), `rslib/src/dbcheck.rs` (Check Database creates no default deck), `rslib/src/storage/sqlite.rs` (new-collection rows) | why the oracle cannot add a note to a collection without deck 1. Probes in the oracle image (2026-10-16, throwaway scenarios, not committed): a new collection with its `decks` and `deck_config` rows deleted refuses `add_note` (InvalidInput); one that keeps only the default deck row (no preset, none of the new-collection config keys but `creationOffset`, `curModel`, `schedVer`, `sortType`, `sortBackwards`) adds a note of each stock kind and Check Database reports nothing; the default deck and preset rows of a new collection are the bytes T10.2 gives |
| schema-changes G1-G5 | `rslib/src/collection/timestamps.rs`, `rslib/src/collection/undo.rs`, `rslib/src/storage/collection_timestamps.rs`, `rslib/src/notetype/schemachange.rs`, `rslib/src/notetype/mod.rs`, `rslib/src/notetype/notetypechange.rs`, `rslib/src/deckconfig/mod.rs`, `rslib/src/dbcheck.rs`, `rslib/src/import_export/package/colpkg/import.rs`, `rslib/src/sync/collection/normal.rs`, `rslib/src/sync/collection/finish.rs`, `rslib/src/collection/mod.rs` (upload preparation), `rslib/src/scheduler/upgrade.rs`, `pylib/anki/collection.py`, `qt/aqt/main.py`, `qt/aqt/schema_change_tracker.py`, `qt/aqt/preferences.py`, `qt/aqt/deckconf.py`, `qt/aqt/changenotetype.py` | every caller that marks the schema modified, which ones the UI confirms, undo of the stamp |

## Driver hints for the oracle maintainer (U29 role work)

- `add_notetype`: build the note type as the setup section does (Anki's `new` from the Basic kind, `type`, `css` given or `""`, `new_field` and `new_template` for each), then add it through the backend's legacy add directly. Never through the Python model manager's add helpers: they rename a colliding name with a hyphen and a checksum before the backend sees it, while N4 step 7 is the backend's `+` rule.
- `add_stock_notetype`: the backend's stock legacy getter for the kind, `name` set when given, then the same backend add.
- `clone_notetype`: the Python model manager's `copy` without adding (it clears `originalId` and sets id 0), `name` set to the argument or `<source> copy` (build that text in the driver, not from translations), then the backend add.
- `update_notetype`: read the stored note type's legacy JSON object from the backend getter, apply the edits to it (kept fields and templates keep their `ord`, which is their identity; `add_field` and `add_template` append the model manager's `new_field` and `new_template` objects, which carry a fresh random id and no `ord`; moves reorder the lists; `sort_field` sets `sortf` to the named field's stored `ord`; `deck` sets `did` to the deck's id or null; the option names map to the legacy keys), and save it with the backend's legacy note type update and `skip_checks`, called directly: that is the call Anki's field and card layout editors make, and the model manager's own update would rename on a collision. The legacy path writes the templates rows' `mtime_secs` and `usn` as 0 and rebuilds `other` from a map; scenarios never save a note type whose template rows hold other values or that has more than one `other` member.
- `ensure_io_notetype`: the backend's image-occlusion note type add, then report the first note type (lowest id) with original stock kind 6 and at least 4 fields.
- Rejections: a backend error of kind card type error maps by its help page (duplicate, no front field, missing cloze); the template-error help page is shared by parse errors and unknown fields, so tell them apart by comparing the message's detail with the backend's translations of the "see preview" and "field not found" strings, never with literal text. An invalid-input error of the save (empty names, no field, no template) is `invalid_notetype`.
- `notetype_structure`: from the protobuf getter; floats are not involved; `deck` names through the deck manager.
- `add_note`: resolve the note type by its stored name at the step (the backend's id-by-name getter compares under unicase; check the stored name equals the argument exactly).
- Reopen, or read through the backend getters that bypass the note type cache, before every op and observation here: scenarios rewrite note type rows with `db_exec`.
- `render_status` (render-ops Q4): per requested card, the review render of the existing card with partial rendering, as `collection_check.render_card` does for K8: a template error is `template_error`, the partial result's empty flag is `empty`, anything else `ok`; no full render is needed, since no text or length is reported. A missing card (deleted) is `null`; a card whose standard note type has no template at its ordinal is Anki's invalid-input error, an `anki_error` run error. It reads the collection only.
- While writing the specs, a throwaway prototype of these ops (loaded through `ORACLE_EXTRA_MODULES` in the scratch directory, not committed) ran every scenario under `conformance/scenarios/notetypes/`; its results agree with what each scenario states and with the two deviations' pointers (`notetypes/records/stamps` step 4, `notetypes/records/preserve-on-edit` step 7).

## Harvest classification

Rows of the `notetypes` area that this unit covers are classified in
`conformance/catalog/harvest.csv` with the scenarios above; rows owned by
later units (card generation U27, structure changes and Restore to Default
U34, Change Note Type U35, Empty Cards U131, merging on import U121, the
render pipeline U62) are `deferred` with that unit in the reason.

## Session s-20261117T144450Z-f49841c9 (spec writer): deferred harvest rows, batch 1

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows and write their scenarios; no code, test text or fixture value was copied, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| notetypes/records/incoming-stamps; notetypes/records#N3.5 and the N4 closing paragraph; conformance/model-ops#L2 first sentence | `rslib/src/notetype/service.rs`, `rslib/src/notetype/mod.rs`, `pylib/anki/models.py` | Which behavior the two add-or-update tests require (existing id updated, name changed, with and without keeping usn/mtime). In the fix pass: how the keep-stamps branch of the add-or-update service differs from the non-preserving branch; that the existing-id save runs the full save preparation, makes the name unique stamping a rename as a local change (usn -1, the current second), updates notes and cards for changed fields and templates when a record with that id exists, inserts the record when none exists, and runs without undo. Golden values come only from the oracle. |

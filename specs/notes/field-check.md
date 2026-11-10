# Field check

Spec: notes/field-check
Requirements: R807, R808; supports R774, R809, R810, R811, R812; AE34
Anki: 26.09.3
Scenarios: conformance/scenarios/notes/fields-check/

The check the editor runs on a note draft while the user types (R807, R812): whether its first field is empty, whether its cloze markup fits its note type, and whether another note already has the same first field. It reads the collection and writes nothing. U27 provides it; the editor services (U191) and the Add and Edit flows (U195, U196) call it and decide what each state means for the user, with their own wording. Decided 2026-10-27: there is one check, U27's. U191's field-check service is a query-lane wrapper that calls it and adds no logic of its own; it exposes the check to the app through the editor services FFI surface (`meguri-ffi` `editor.rs`, `EditorServices` in the core bridge). The conformance executor (`conformance/note-ops` N4) and the app therefore run the same code. Shared with U191, which may add to this file. U191's other editor services are `notes/add-defaults` (the starting deck and note type) and `tags/complete` (tag suggestions).

## K1. Input and output

- Input: the note type, the field values as typed (unsaved: no save normalization has run), and the note's id when the draft edits an existing note.
- Output: one state from K3 and, for `duplicate`, the duplicate notes (K4).
- Shape and errors. Decided 2026-10-27, as Anki: the field list is taken as given. When it is shorter than the note type's fields, the missing trailing fields count as empty (an empty list is `empty`). When it is longer, each extra value is checked as a field that is not a cloze field (K2). A note type id that names no note type is `NotFound` (`engine/op-model`). The note id is used only to exclude that note (K4): a note id that names no note (for example one a sync deleted while the user edits) excludes nothing, and a note id of another note type excludes nothing either, since only notes of the draft's note type are candidates. Neither is an error. The check reads the note type and the notes as they are when it runs, so a draft built from an older snapshot of its note type gets the current note type's cloze fields.

## K2. Terms

- **Comparison text**: the draft's first field, NFC-normalized when `normalize_note_text` is on (a missing or non-boolean value counts as on), then stripped by `text/strip-html` H5 (media names kept). No control character is removed (`text/checksum-and-sort-field` C5).
- **Has a cloze**: a field value has a cloze when one of its top-level clozes has a number other than 0 (`render/cloze` C5, top-level presence). `{{c0::x}}`, `{{c0::x {{c1::y}}}}` and unclosed markup `{{c1::x` have none.
- **Cloze fields** of a cloze note type: the fields whose names are cloze references (`render/template-syntax` T11.1: a replacement whose filter chain has a `cloze` piece) of the front of the note type's first template, matched to field names under unicase (`text/unicode-primitives#U2`), so `{{cloze:text}}` names the field `Text`. Field names are unique under unicase (`notetypes/validation`), so at most one field matches a name; a name that matches no field adds none. When that front does not parse, there are none. A standard note type has none.

## K3. States

The state is the first that applies, in this order:

| State | When |
|---|---|
| `field_not_cloze` | the note type is cloze and a field that is not a cloze field has a cloze |
| `empty` | the comparison text, with Unicode White_Space removed from both ends, is empty |
| `notetype_not_cloze` | the note type is standard and a field has a cloze |
| `missing_cloze` | the note type is cloze and no field has a cloze |
| `duplicate` | K4 finds at least one note |
| `normal` | otherwise |

So a cloze outside a cloze field is reported even with an empty first field, while an empty first field hides a misplaced or missing cloze in the other cases, and a cloze problem hides a duplicate.

Examples (scenario `notes/fields-check/states`), with a standard type Std (Front, Back), a cloze type Gap (Text, Extra) whose first front is `{{cloze:Text}}`, and Gap2 whose first front is `{{cloze:Text}}{{cloze:Extra}}`:

| Draft | State |
|---|---|
| Std `x`, (empty) | `normal` |
| Std (empty), `y` | `empty` |
| Std `<br> <div></div>`, (empty) | `empty` |
| Std `&nbsp;` U+3000, (empty) | `empty` |
| Std `<img src="a.jpg">`, (empty) | `normal` (the media name counts) |
| Std `{{c1::x}}`, (empty) | `notetype_not_cloze` |
| Std `x`, `{{c1::y}}` | `notetype_not_cloze` |
| Std (empty), `{{c1::y}}` | `empty` |
| Std `{{c0::x}}`, (empty); Std `{{c1::x`, (empty) | `normal` |
| Std with a stored note `dup`: `dup`, `{{c2::z}}` | `notetype_not_cloze` |
| Gap `{{c1::x}}`, (empty) | `normal` |
| Gap `x`, (empty) | `missing_cloze` |
| Gap (empty), (empty) | `empty` |
| Gap `x`, `{{c1::y}}`; Gap (empty), `{{c1::y}}`; Gap `{{c1::x}}`, `{{c2::y}}` | `field_not_cloze` |
| Gap `{{c0::x {{c1::y}}}}`, (empty); Gap `{{c0::x}}`, (empty) | `missing_cloze` |
| Gap2 `x`, `{{c1::y}}` | `normal` |
| Gap2 (empty), `{{c1::y}}` | `empty` |
| a cloze type whose first front does not parse: `{{c1::x}}`, (empty) | `field_not_cloze` |
| the same type: `x`, (empty) | `missing_cloze` |
| Gap3 (Text, Extra), first front `{{cloze:text}}`: `{{c1::x}}`, (empty) | `normal` (unicase match; scenario `notes/fields-check/cloze-unicase`) |

## K4. Duplicates (R807, R808)

The duplicates of a draft are the notes that:

- have the draft's note type;
- are not the draft's own note (when it edits an existing note);
- have the stored `csum` equal to the checksum of the comparison text as it is: SHA-1 of its UTF-8 bytes, the first 4 digest bytes read as a big-endian unsigned 32-bit integer (only steps 2 and 3 of `text/checksum-and-sort-field` C4), with no second H5, no NFC and no control-character removal, as `search/names-and-ids` N6. The comparison text is already stripped, and H5 is not idempotent (it decodes entities, and a second pass removes the tags they produced), so applying all of C4 to it would miss notes such as `&lt;b&gt;Cat` below; and
- have a stored first field whose `text/strip-html` H5 text equals the comparison text exactly.

The lookup uses the stored `csum` column (with the index on it, `storage/schema-18`), so a note whose stored `csum` is wrong is not found even when its first field matches. The stored first field is stripped as stored, without normalization. The duplicates are listed in ascending note id order.

Examples (AE34, scenario `notes/fields-check/duplicates`), with stored Kaishi notes `私`, `私 ` and `café`, and a note of the type Other with `私`:

| Draft (Kaishi unless named) | State | Duplicates |
|---|---|---|
| `<b>私</b>` | `duplicate` | the Kaishi note `私` |
| Other `<b>私</b>` | `duplicate` | the Other note |
| `&#31169;` | `duplicate` | `私` (the entity decodes) |
| `私 ` | `duplicate` | `私 ` only |
| `私` U+0001 | `normal` (the control character stays in the comparison text) |
| `cafe` U+0301, key on | `duplicate` | `café` |
| `cafe` U+0301, key off | `normal` |
| `Café` | `normal` (case counts) |
| the stored note `私` itself | `normal` (itself excluded), or `duplicate` once another Kaishi note `<b>私</b>` exists |
| `&lt;b&gt;Cat`, with a stored Kaishi note `&lt;b&gt;Cat` | `duplicate` | that note (both strip once to `<b>Cat`; scenario `notes/fields-check/entity-markup`) |

A duplicate never blocks adding (R808): the Add flow shows the state and adds the note when asked. Decided 2026-10-27: the "Show Duplicates" link of R807 opens Browse on a note-ids item (`search/join-and-replace` J3) holding exactly the duplicates of the last check, which never include the draft's own note (`search/names-and-ids` N6). It does not build a `dupe:` search, so no text is stripped twice and the J3.4 refusal cannot leave the link without a result. Anki opens a `dupe:` search on the raw first field, which also lists the edited note; Meguri's Browse shows the same other notes, with a note-ids search text.

## K5. What uses which state

- The Add screen (U195): `empty` refuses adding (R809); `notetype_not_cloze` and `field_not_cloze` refuse it (R810); `missing_cloze` asks whether to add anyway (R811); `duplicate` marks the field and offers Show Duplicates (R807); every state but `normal` shows its hint live (R812).
- The edit flows (U196) show the same marks without refusing a save.

## K6. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `notes/fields-check/states` | K2, K3 | recorded, with the `fields_check` observation (`specs/conformance/note-ops.md` N4) |
| `notes/fields-check/cloze-unicase` | K2 cloze fields under unicase, K3's Gap3 row | not yet recorded (added 2026-11-10; it uses only `db_exec`, `reopen` and `fields_check`) |
| `notes/fields-check/duplicates` | K2, K4, AE34 | recorded |
| `notes/fields-check/entity-markup` | K2, K4's `&lt;b&gt;Cat` row: entity-encoded markup, one strip only | not yet recorded (added 2026-11-10) |
| `notetypes/structure/first-field-duplicates` | K4 and AE34 after another field moves to position 0 (`notetypes/structure-changes` S5) | not yet recorded (added 2026-11-10) |

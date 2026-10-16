# Field check

Spec: notes/field-check
Requirements: R807, R808; supports R774, R809, R810, R811, R812; AE34
Anki: 26.09.3
Scenarios: conformance/scenarios/notes/fields-check/

The check the editor runs on a note draft while the user types (R807, R812): whether its first field is empty, whether its cloze markup fits its note type, and whether another note already has the same first field. It reads the collection and writes nothing. U27 provides it; the editor services (U191) and the Add and Edit flows (U195, U196) call it and decide what each state means for the user, with their own wording. Shared with U191, which may add to this file.

## K1. Input and output

- Input: the note type, the field values as typed (unsaved: no save normalization has run), and the note's id when the draft edits an existing note.
- Output: one state from K3 and, for `duplicate`, the duplicate notes (K4).

## K2. Terms

- **Comparison text**: the draft's first field, NFC-normalized when `normalize_note_text` is on (a missing or non-boolean value counts as on), then stripped by `text/strip-html` H5 (media names kept). No control character is removed (`text/checksum-and-sort-field` C5).
- **Has a cloze**: a field value has a cloze when one of its top-level clozes has a number other than 0 (`render/cloze` C5, top-level presence). `{{c0::x}}`, `{{c0::x {{c1::y}}}}` and unclosed markup `{{c1::x` have none.
- **Cloze fields** of a cloze note type: the fields whose names are cloze references (`render/template-syntax` T11.1: a replacement whose filter chain has a `cloze` piece) of the front of the note type's first template, matched to field names exactly. When that front does not parse, there are none. A standard note type has none.

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

## K4. Duplicates (R807, R808)

The duplicates of a draft are the notes that:

- have the draft's note type;
- are not the draft's own note (when it edits an existing note);
- have the stored `csum` equal to the checksum of the comparison text (`text/checksum-and-sort-field` C4 applied to that text); and
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

A duplicate never blocks adding (R808): the Add flow shows the state and adds the note when asked. The "Show Duplicates" link of R807 opens Browse on the search `dupe:<note type id>,<first field>` (search area, U80), which finds the same notes plus the draft's own note when it is stored.

## K5. What uses which state

- The Add screen (U195): `empty` refuses adding (R809); `notetype_not_cloze` and `field_not_cloze` refuse it (R810); `missing_cloze` asks whether to add anyway (R811); `duplicate` marks the field and offers Show Duplicates (R807); every state but `normal` shows its hint live (R812).
- The edit flows (U196) show the same marks without refusing a save.

## K6. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `notes/fields-check/states` | K2, K3 | needs the `fields_check` observation (`specs/conformance/note-ops.md` N4) |
| `notes/fields-check/duplicates` | K2, K4, AE34 | needs the `fields_check` observation |

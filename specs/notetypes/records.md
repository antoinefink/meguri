# Note type records

Spec: notetypes/records
Requirements: R903, R904, R905, R906, R907, R913, R916, R917, R1485, R1586, R1590
Anki: 26.09.3
Scenarios: conformance/scenarios/notetypes/records/

How Meguri reads a note type, what a local save of one writes, the normalization every save applies, the card requirements it stores, and how note types are added, cloned and renamed. Row layouts are `model/row-records` R7, blob fields `model/protobuf-records` P6.1–P6.3, ids and stamps `storage/ids-usn-mtime`. Template parsing and analysis are `render/template-syntax` (U60). The template checks are `notetypes/validation`, the stock kinds `notetypes/stock-kinds`, schema changes `notetypes/schema-changes`. Structure changes that rewrite notes and cards (field and template add, remove, reorder and rename, template text, sort field, deletion, Restore to Default) are U34's; they write through this file's rules.

Terms: `now_s` and `now_ms` as in `storage/ids-usn-mtime`; "equal under unicase" is `text/unicode-primitives#U2`; NFC is `text/unicode-primitives#U3`.

## N1. The aggregate

- A note type is its `notetypes` row with its `fields` rows and its `templates` rows, each list ordered by `ord`. A field's or template's position in its list is its ordinal.
- Meguri writes `ord` as 0 to n − 1 without gaps. A stored list whose `ord` values have gaps is read in `ord` order, positions as ordinals; the next save writes it back without gaps. Repairing such note types is Check Database's (U129).
- Every stored value is kept, including the ones Meguri neither shows nor uses (R904, R1485, `model/protobuf-records` P8): `latex_pre`, `latex_post`, `latex_svg`, `reqs` (N6), `original_stock_kind`, `original_id`, `target_deck_id_unused`, the `other` bytes of the note type, its fields and its templates, unknown protobuf fields at every level (P4), and the `mtime_secs` and `usn` columns of `templates` rows (N3.3).
- A record that was decoded and not changed is written back as the bytes it was read with (`model/protobuf-records` P5).

## N2. What the record holds (R903)

| Level | Value | Stored as |
|---|---|---|
| note type | name | `notetypes.name` |
| | standard or cloze | `kind` 0 or 1 |
| | sort field ("Sort by this field in the browser") | `sort_field_idx`, a field position |
| | styling | `css` |
| field | name | `fields.name` |
| | kept for the next note (the Add screen's pin, R805) | `sticky` |
| | "Reverse text direction (RTL)" | `rtl` |
| | editor font and size | `font_name`, `font_size` |
| | Description | `description` |
| | "Use HTML editor by default" | `plain_text` |
| | "Collapse by default" | `collapsed` |
| | "Exclude from unqualified searches (slower)" | `exclude_from_search` |
| | field id | `id` (optional; R905) |
| | image-occlusion field tag | `tag` (optional; `notetypes/stock-kinds` K4) |
| | protected from deletion | `prevent_deletion` |
| template | name | `templates.name` |
| | front, back | `q_format`, `a_format` |
| | browser front, browser back (Browser Appearance, R935) | `q_format_browser`, `a_format_browser` |
| | deck override (R934) | `target_deck_id`; 0 means none |
| | browser font and size | `browser_font_name`, `browser_font_size` |
| | template id | `id` (optional; R905) |

## N3. A local save

A local save writes a note type the user, the local API (U141, U139) or a Meguri op changed. It runs N4 (normalization), N6 (card requirements) and, for a checked save, the template checks of `notetypes/validation` V3, then compares and writes as below. Saves of rows arriving by sync or import are U107's and U120's.

### N3.1 A save that changes nothing writes nothing (R906, R1472)

The save compares the normalized note type with the stored one:

- the `notetypes` row: `name` and the decoded config (`model/protobuf-records` P4 equality: known values, floats bit for bit, captured unknown fields);
- the `fields` rows by `ord`: `name` and the decoded config;
- the `templates` rows by `ord`: `name`, `mtime_secs`, `usn` and the decoded config.

When all of them are equal, nothing is written: no row, no stamp, no `col.mod` (`storage/ids-usn-mtime` S4, S5), no change-set entry, no undo step. Viewing a note type, or saving it from an editor without a change, never re-uploads it.

Anki stamps every saved note type, changed or not, and moves `col.mod`. Deviation `notetype-unchanged-save` (`conformance/deviations.yaml`, scenario `notetypes/records/stamps`).

### N3.2 Rows written and stamps

- Only rows that differ are written: a `fields` or `templates` row is inserted, updated or deleted when its `ord` key is new, its content differs, or its `ord` no longer exists.
- Whenever anything in the aggregate differs (the `notetypes` row or any `fields` or `templates` row), the `notetypes` row is written and stamped as a local change: `mtime_secs` = `now_s`, `usn` = −1 (`storage/ids-usn-mtime` S2). Sync sends and merges a note type as a whole by that row, so a renamed field or a changed field option marks it pending and the next normal sync uploads it.
- `col.mod` moves once at commit (S5). The change set lists the note type (`engine/change-stream` S1).
- Anki deletes and inserts every `fields` and `templates` row of the note type on each save; the stored result is the same except for unknown protobuf fields, which Anki drops (N1, deviation `notetype-unknown-fields-kept`, scenario `notetypes/records/preserve-on-edit`).
- `fields.name` and `templates.name` each have a unique index on `(name, ntid)` with the `unicase` collation, checked per statement (`storage/schema-18`). A save that swaps names between positions must write so that no single statement leaves two rows of one note type with names equal under unicase, for example by deleting the changed rows first and inserting them again.

### N3.3 `templates.mtime_secs` and `templates.usn`

These columns are data, never stamps (`storage/ids-usn-mtime` S1). A template keeps the values it was loaded with wherever it moves in the list; a template the save adds gets 0 and 0. Anki writes 0 and 0 for templates that come through its legacy JSON path, and Anki-made collections hold 0 there; Meguri never writes another value of its own.

### N3.4 Undo

A local save that wrote something is one undo step (U25) unless its op says otherwise. Undo restores every row of the aggregate from its before-image, the `notetypes` row's `mtime_secs` and `usn` included, as Anki does; redo restores the after-images.

## N4. Save normalization

Every local save, add and clone applies these steps in this order. A step that refuses makes the whole save fail with `invalid_notetype` and write nothing.

1. Note type name: every `"` is removed. An empty result refuses. No trimming: `  spaced  ` stays as it is.
2. NFC: the note type name, every field name and every template name are put in NFC.
3. Field names (each field):
   1. an empty name refuses;
   2. every `:`, `{`, `}` and `"` is removed;
   3. Unicode White_Space is trimmed at both ends;
   4. then characters are removed from the start while they are `#`, `/`, `^` or White_Space;
   5. an empty result refuses.

   So `  # /^ Fr:o{n}t"` becomes `Front`, `a #` stays `a #`, and `::` refuses.
4. Template names (each template): an empty name refuses; every `"` is removed; an empty result refuses. No trimming: a template may be named `  `.
5. Unique template names, then unique field names: going through the list in order, a name equal under unicase to an earlier name in the same list gets `+` appended, again until it differs from every earlier name. So fields `Front`, `front`, `FRONT` become `Front`, `front+`, `FRONT++`, and `Straße`, `STRASSE` become `Straße`, `STRASSE+`.
6. Sort field: the sort field is a field, not a position. Its final index is that field's final position. When the save removes the sort field, the index keeps its old value, lowered to the last position when it is past the end (fields `A B C` sorting by `C`, `C` removed: sort by `B`; sorting by `A`, `A` removed: sort by the new first field).
7. Unique note type name: when another note type of the collection has a name equal under unicase, `+` is appended until no other note type has such a name. A note type never collides with itself, so a case-only rename keeps the new spelling (`Alpha` renamed `ALPHA` stays `ALPHA`). Examples with `Pair` stored: adding `pair` gives `pair+`, then `PAIR` gives `PAIR++`, then `Pa"ir` gives `Pair+++`.
8. Card requirements: N6.

Anki's Python layer has a rename of its own for note types added or saved through its legacy API (a hyphen and five characters of a checksum); Meguri follows the core rule of step 7 (R907).

Saves of rows arriving by sync or import go through the same normalization in Anki; whether and how Meguri applies it there is U107's and U120's.

## N5. Adding a note type

- The new `notetypes` row's id is allocated as `storage/ids-usn-mtime` I2 and I3 say for the `notetypes` table.
- The row is stamped as a local change (`mtime_secs` = `now_s`, `usn` = −1); its templates rows get `mtime_secs` 0 and `usn` 0.
- The collection config key `curModel` is set to the new id, as a local config change (`storage/config-table` C4). The on-demand Image Occlusion add is the exception (N9).
- Adding is never a schema change (R950): `col.scm` does not move.
- An add is a checked save (`notetypes/validation` V1).
- Ways to add: from scratch (N5.1), a stock kind (`notetypes/stock-kinds` K8), a clone (N7).

### N5.1 From scratch

Used by the local API's model creation (U142) and by scenario setup (`specs/conformance/dsl.md` D6.4). A note type made from scratch holds:

| Value | Content |
|---|---|
| `kind` | as asked (standard 0 or cloze 1) |
| `sort_field_idx` | 0 |
| `css` | the caller's text; Meguri's base card CSS when the caller gives none (scenario ops give `""`) |
| `latex_pre` | the default LaTeX header (N5.4) |
| `latex_post` | `\end{document}` |
| `latex_svg` | false |
| `reqs` | N6 |
| `original_stock_kind` | 1 (basic): Anki builds every new note type from its Basic kind, cloze ones included, and keeps that value |
| `original_id` | absent |
| `target_deck_id_unused` | 0 |
| `other` | empty |
| fields | N5.2, in the given order |
| templates | N5.3, in the given order |

### N5.2 A new field

Name as given (then N4); `font_name` `Arial`; `font_size` 20; `sticky`, `rtl`, `plain_text`, `collapsed`, `exclude_from_search` and `prevent_deletion` false; `description` empty; `tag` absent; `other` empty; `id` a fresh uniformly random signed 64-bit integer from the randomness seam (`meguri_model::rng`, R1875; R905). The same holds for a field added to an existing note type (U34).

### N5.3 A new template

Name, front and back as given; browser front and back empty; `target_deck_id` 0; `browser_font_name` empty; `browser_font_size` 0; `other` empty; `id` a fresh random signed 64-bit integer (R905); `mtime_secs` 0 and `usn` 0 in its row. The same holds for a template added to an existing note type (U34).

### N5.4 Default LaTeX header

The value Anki stores in every new note type, and other clients use when they make LaTeX images from its notes, so Meguri writes the same 178 bytes: seven lines, each ending with LF (U+000A):

```
\documentclass[12pt]{article}
\special{papersize=3in,5in}
\usepackage[utf8]{inputenc}
\usepackage{amssymb,amsmath}
\pagestyle{empty}
\setlength{\parindent}{0in}
\begin{document}
```

The footer is the 14 bytes `\end{document}` with no line end. Meguri does not render LaTeX itself (scope); it only stores these values.

## N6. Card requirements (`reqs`)

`reqs` is the legacy per-template list of fields a card needs (`model/protobuf-records` P6.1.1; `req` in schema-11 JSON). Current clients generate cards from the templates themselves; older clients used this list, and legacy packages carry it (U123).

- Value: one entry per template, in template order, `card_ord` = the template's position, `kind` and `field_ords` from that template's front by `render/template-syntax#T11.3`, with the note type's own field names and positions as they are after the save. `field_ords` ascending.
- Meguri computes it when it creates a note type (from scratch, stock or clone) and when a save changes any front text, the field list (count, order or a name) or the kind. Any other save keeps the stored `reqs` as read (R1485).
- Anki recomputes it on every save, from the fronts as they were before that save's field renames rewrote them: right after a field rename, Anki stores `none` for every template whose front referenced the renamed field, and the next save corrects it. Meguri computes from the final fronts. Note types whose stored `reqs` already match their templates (every note type Anki or Meguri saved since its last field rename) are unaffected; the area that shows the difference in a scenario (U34, field renames) records the deviation.

Examples (Anki's stock kinds, `notetypes/stock-kinds` K4): Basic `any [0]`; Basic (and reversed card) `any [0]`, `any [1]`; Basic (optional reversed card) `any [0]`, `all [1, 2]`; Basic (type in the answer) `any [0, 1]`; Cloze `any [0]`; Image Occlusion `any [0, 1, 2]`.

## N7. Clone (R905, R916)

A clone is a new note type copied from an existing one:

| Value | In the clone |
|---|---|
| id | newly allocated (N5) |
| name | the caller's, or `<source name> copy` (R916); then N4 steps 1, 2 and 7 |
| `original_id` | absent (cleared) |
| `notetypes` stamps | local (N5) |
| template rows' `mtime_secs`, `usn` | 0, 0 |
| `curModel` | the clone's id (N5) |
| everything else | as in the source: kind, sort field, CSS, LaTeX values, `original_stock_kind`, `target_deck_id_unused`, every field and template with its name, ordinal, options, deck override, browser formats and **id** (R905), the `other` bytes and unknown protobuf fields at every level |

- `reqs` are computed as for any new note type (N6); for a source whose `reqs` match its templates they come out equal.
- A clone is a checked save: cloning a note type whose templates fail `notetypes/validation` V3 is refused as Anki refuses it.
- Anki's clone goes through its legacy JSON path, which drops unknown protobuf fields and writes several `other` members in no set order; Meguri copies the bytes. Scenarios clone note types without unknown fields and with at most one `other` member.

## N8. Rename and option edits

- Rename (R917) changes the name only, then N4 steps 1, 2 and 7. It is not a schema change. Anki runs the template checks on a rename (a rename of a note type with a broken template is refused); Meguri does the same (checked save, `notetypes/validation` V1).
- Field options (R925; `sticky` through the Add screen's pin, R805), template renames, browser appearance (R935), deck override (R934) and styling are plain saves under N3 and N4. None is a schema change (`notetypes/schema-changes` G2). They touch no note and no card.
- Field renames and template text edits are U34's: the first rewrites template references (R923), the second may generate cards (R789). Neither is a schema change.

## N9. Image Occlusion note type on demand (R913)

- The collection's Image Occlusion note type is the note type with the lowest id among those whose `original_stock_kind` is 6 and that have at least 4 fields.
- Ensuring one exists (when the user first creates an occlusion, U63/U68): when such a note type exists, nothing is written. Otherwise the stock Image Occlusion kind (`notetypes/stock-kinds` K4) is added as N5 says, named `Image Occlusion` with N4 step 7 applied (`Image Occlusion+` when a note type of that name exists), except that `curModel` keeps its value.
- A note type of stock kind 6 with fewer than 4 fields does not count, so a second one is added beside it.

## N10. Meguri never edits note types on its own (R1586, R1590)

- Only an op that carries a requested change writes note type rows: a user action, a local API call, an import, a sync merge, or Check Database's listed repairs (U129). Empty Cards and Change Note Type write notes, cards and graves but no note type row (`notes/empty-cards` E4, `notetypes/change-notetype` M9). Opening a collection, launching, timers, rendering, the integration servers and status checks never write a note type, template or CSS.
- A save with no change writes nothing (N3.1), so re-sending the same definition (for example an integration that re-applies its note type on every connection) causes no sync traffic.
- No Meguri op sets `col.scm` except those `notetypes/schema-changes` G2 lists (R1590).

## N11. Deviations

| Id | Requirement | Meguri | Anki |
|---|---|---|---|
| `notetype-unchanged-save` | R906, R1472 | a save that changes nothing writes nothing | stamps the note type and moves `col.mod` |
| `notetype-unknown-fields-kept` | R1485, R904 | unknown protobuf fields of the note type, its fields and its templates survive a save | dropped from every row of the saved note type |

## N12. Scenarios

Every scenario below has its golden. Each runs on Meguri with U29's adapters and fresh collection (`specs/conformance/notetype-ops.md` T10) plus what the last column names; it is `pending` until those exist, and the unit that adds the last of them makes it active in `conformance/scenarios/notetypes/area.yaml` (D9.1). U29 makes active those with nothing in the last column. A live scenario needs on Meguri only what Meguri's own segments use (`specs/conformance/dsl.md` D9.2), and T1 alone runs it.

| Scenario | Covers | Meguri also needs |
|---|---|---|
| `notetypes/records/stock-new-collection` | `notetypes/stock-kinds` K2 | |
| `notetypes/records/names` | N4, N5, N8 rename | |
| `notetypes/records/validation` | `notetypes/validation` | |
| `notetypes/records/io-on-demand` | N9 | |
| `notetypes/records/stock-structure` | `notetypes/stock-kinds` K1, K3, K4, K8 | |
| `notetypes/records/stamps` | N3.1, N3.2, N8; deviation `notetype-unchanged-save` | `setup.decks` (U28) |
| `notetypes/records/clone` | N7 | `legacy_update` (U36) |
| `notetypes/records/preserve-on-edit` | N1, N3.2; deviation `notetype-unknown-fields-kept` | `legacy_update`, `blob_decode`, `legacy_json` (U36) |
| `notetypes/records/stock-render-handoff` (live, T1) | `notetypes/stock-kinds` K2, K4, K5, K7, K9: Anki adds notes of every stock kind on the note types Meguri wrote and renders their cards without errors | (Meguri's segment is empty: it needs `handoff` and the new collection of T10.2, which holds the default deck the oracle adds the notes to) |
| `notetypes/records/stock-handoff` (live, T1) | `notetypes/stock-kinds` K2, K4, K9, with the notes Meguri adds | `add_note` (U27) |

The gate scenarios (`notetypes/schema-changes` G8) also need `setup.notes` (U27) and U34's edits.

Behavior only Meguri's code shows (per-row writes, the unique-index order of N3.2, undo images) is covered by `core/crates/meguri-engine/tests/notetype_records.rs`.

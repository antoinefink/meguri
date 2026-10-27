# Note type merging on deck package import

Spec: package/notetype-merge
Requirements: R1605, R1606; R1475 and R1476 for the import-merge case; R2008 for the import-merge case (core side); supports R2053
Anki: 26.09.3
Scenarios: conformance/scenarios/package/notetype-merge/ (none recorded yet; J15 lists them)

What a deck package import (`package/apkg-import`, U120) does with Merge note types on (U121): which note types merge (J1, J2), how fields and templates match (J3), the merged note type (J4), the two stages that merge by id and by note GUID (J5, J6), the maps the import applies to incoming notes and cards (J7), the removal of emptied note types (J8), the schema stamp, safety backup and structure report (J9), the preview of destructive mappings (J10), the `mod` the note update compares (J11), the write phase and undo (J12), errors (J13), the differences from Anki (J14) and the scenarios (J15).

This spec implements the merge port of `package/apkg-import#I8`: its inputs, its outputs, its error and its confirmation token are I8's, and I8 says what the import does with the outputs. This file builds on: the note type record and save normalization (`notetypes/records` N1 to N4, N6), the rewrite of a note type's notes and of template references (`notetypes/structure-changes` S4, S5, S7; `notetypes/field-rename#F3`), the effects of Change Note Type (`notetypes/change-notetype#M9`), note type removal (`notetypes/structure-changes#S12`), card generation for a whole note type (`notes/card-generation#G9`, `notes/deck-placement-and-position` D4 and P2), and the schema rules (`notetypes/schema-changes` G2, G3, G5).

Terms. The **target** is the open collection. An **incoming note type** is a note type the import takes (`package/apkg-import#I6`), with its `original_id` already set to its own id (I7's first step). For an incoming note type whose id the target holds, **T** is the target's note type with that id. A **copy** of T is a target note type other than T whose `original_id` (`model/protobuf-records` field 10) equals T's id: an earlier import without a merge added it (`package/apkg-import#I7`, "copy added"). A note type's **kind** is standard or cloze (`notetypes/records#N2`). A **position** is a 0-based index in a field or template list. Stamps (`now_s`, `now_ms`, `usn` -1) are `storage/ids-usn-mtime`'s.

## J1. Which note types merge

- **Stage 1, by id** (J5). Each incoming note type whose id the target holds merges with T. An incoming note type whose id the target lacks is added as I7's row "added" says and is not merged in stage 1.
- **Copies.** In stage 1, T's copies merge into T too: their notes move to T and the copies are removed (J8). The copies are those the plan's snapshot holds, in ascending id. Decided 2026-10-27: two kinds of copy stay out of the merge and are left as they are: a copy whose own id is also the id of an incoming note type (that note type merges with it in its own right), and a copy whose kind differs from T's (J2).
- **Stage 2, by note** (J6). After stage 1, a target note whose GUID a package note holds and whose note type differs from that package note's brings its note type into a merge with the package note's.
- In stage 1, names and `original_stock_kind` play no part: two stock Basic note types with different ids and no `original_id` link are not merged by id, whatever their names (as Anki 26.9.3). Stage 2 (J6) can still merge them through a shared note GUID; there too names play no part.

## J2. Kinds (R1606)

- A standard and a cloze note type never merge. Stage 1 checks T against the incoming note type, before anything else of that note type; stage 2 checks each pair (J6 step 1). A mismatch ends the whole import with `NotetypeKindConflict{incoming, target}` (the incoming note type's id, the target note type's id) and nothing written, as I8 says. Anki 26.9.3 ends the whole import there too, rolling back what it had done. U162 words the message.
- Stage 1 checks the incoming note types in ascending id and stops at the first mismatch; stage 2 runs only when stage 1 had none.
- A copy whose kind differs from T's is not merged and not removed (J1); its notes stay where they are. Decided 2026-10-27 (J14): Anki merges it, which would move its notes across kinds and delete their cards past the template count.

## J3. Matching fields and templates

- Two fields **match** when both have an id (`notetypes/records#N2`) and the ids are equal, or when at least one of them has no id and their names are equal, byte for byte (no case folding, no normalization). Two fields that both have ids and whose ids differ never match, whatever their names. Positions are never compared. Templates match by the same rule with template ids and names.
- So a field renamed on one side keeps matching by its id; a package from a client that stores no ids matches by names; a field deleted and added again under the same name (a new id) does not match the old one.
- **One to one.** A record's fields are matched against a list (J4.1) in the record's order: each takes the first field of the list, in list order, that it matches and that no earlier field of the same record took. A field that takes none is **new** to the list. Templates likewise. Decided 2026-10-27: Anki can match two fields of one record with one field of the other (only when ids or names repeat inside a note type); Meguri never does (J14).

## J4. The merged note type

### J4.1 The merged lists

- The merged field list starts as T's fields in T's order. Then each merging copy, in ascending id, and last the incoming note type are matched against the list as it stands (J3), and each one's new fields are appended at the end, in that record's order. T's fields therefore keep their positions; fields only a copy or the incoming note type has follow them.
- The merged template list is built the same way from the templates. When T is a cloze note type, templates are not merged: the merged list is T's one template (or the governing record's, J4.3), and the incoming's and the copies' one template stands for it without matching: their card ordinals are cloze numbers and keep their values (J7).
- Decided 2026-10-27: this order differs from Anki's in two cases (deviation `apkg-merge-layout`, J14): with two or more new fields that the incoming note type places before a field T has, and with a copy whose fields are in another order than T's.

| T's fields (ids) | Merging records | Merged list |
|---|---|---|
| Front (f1), Back (f2) | incoming: Front (f1), Back (f2), Extra (f3) | Front, Back, Extra |
| Front (f1), Back (f2) | incoming: Front (f1), Extra (f3), Back (f2) | Front, Back, Extra |
| Front (f1), Back (f2) | incoming: Front (f1), Hint (f4), Source (f5), Back (f2) | Front, Back, Hint, Source (Anki: Front, Back, Source, Hint) |
| Front (f1), Back (f2) | copy 2001: Front (f1), Back (f2), Extra (f3); incoming: Front (f1), Back (f2), Extra (f3) | Front, Back, Extra (the incoming's Extra matches the copy's) |
| Front (f1), Back (f2) | incoming: Front (f1), Answer (f2) | two fields: f2 matched by id (a rename, J4.3) |
| Front, Back (no ids) | incoming: Front, Back, Extra (no ids) | Front, Back, Extra, matched by names |
| Front (f1) | incoming: Front (f9) | Front, Front+ (no match: the ids differ; `notetypes/records#N4` step 5 renames the second) |

### J4.2 The governing record

The **governing record** gives the merged note type its values (J4.3). It is the incoming note type or T, by Update note types (`package/apkg-import#I5`):

| Update note types | The incoming note type governs when |
|---|---|
| `IfNewer` | T's `mtime_secs` is below the incoming's |
| `Always` | always, also when the two `mtime_secs` are equal |
| `Never` | never |

Otherwise T governs. A copy never governs. `Always` here differs from I7, where it needs the two values to differ; this is Anki 26.9.3's rule for a merge.

### J4.3 Values

- The merged note type keeps T's id. Its name, kind, styling, LaTeX settings, `original_id` and every other stored value of `notetypes/records#N1`, unknown protobuf fields included, are the governing record's; its sort field is J4.4's.
- Each field of the merged list takes all its values (name, options, id, tag, protection, unknown fields) from the governing record when that record has the field (J3), otherwise from the first record that has it, in the order T, the copies in ascending id, the incoming note type. Templates likewise: name, the four texts, deck override, browser font and size, id, unknown fields, and the row's `mtime_secs` and `usn` (data, never stamps: `notetypes/records#N3.3`).
- So with the incoming governing, a field matched by id takes the incoming's name (a rename of T's field) and a field matched by name takes the incoming's id, or none when the incoming has none. With T governing, T's fields and templates keep their values and the incoming's new ones come with theirs.
- `original_id`: with the incoming governing, the incoming's (its own id, which is T's); with T governing, T's, unchanged.

### J4.4 Sort field

`sort_field_idx` is the governing record's number, read as a position in the merged list and lowered to the last position when past the end. With T governing it is T's own sort field, since T's fields keep their positions. With the incoming governing, the position can hold another field than the one the incoming sorted by, when the incoming's fields before it are in another order than the merged list (as Anki 26.9.3 does). Example: T `Front`, `Back`, sort 0; incoming `Front`, `Extra`, `Back`, sort 2 (`Back`), newer: the merged list is `Front`, `Back`, `Extra` and the sort field is position 2, `Extra`.

This replaces `notetypes/records#N4` step 6 for a merge.

### J4.5 Writing the merged note type

- It is written over T's rows (`notetypes/records#N3.2`: `fields` and `templates` rows by `ord`), as a save whose origins are T's positions: a field or template of T has its position in T as origin, the others none (the terms of `notetypes/structure-changes#S1.1`).
- It is normalized by `notetypes/records#N4` steps 1 to 5, 7 and 8 as an unchecked save (`notetypes/validation#V1`: no template check), J4.4 standing in for step 6.
- When a field with an origin has a name other than T's field at that origin (a rename, J4.3), template references are rewritten in every template of the merged note type by `notetypes/field-rename#F3` with `notetypes/structure-changes#S4.3`'s append rule (deviation `template-rewrite-keeps-text`, `notetypes/structure-changes#S14`). No field is removed, so S4.2 never applies.
- The `notetypes` row gets `mtime_secs` = `now_s` and `usn` -1 every time stage 1 or stage 2 writes the note type, also when no value differs from T's (Anki 26.9.3 stamps every merged note type).
- `curModel` is not touched (J8 can change it).

## J5. Stage 1: merging by id

**Inputs.** Beyond what `package/apkg-import#I8` lists under "Called with", the merge reads from the same plan snapshot: the ids of every note of T, of each copy and of each E (J5 steps 3 and 5, J6); the target notes' `flds` and `mod` (J5 step 3, J10, J11); the package notes' `mod` (J10); and the Update notes option (I5, for J10). It reads nothing else and nothing live. I8's "Called with" is to be extended to match (J9's follow-ups).

For each incoming note type whose id the target holds, in ascending id, with T, its merging copies and the plan's state as the earlier note types of this stage left it:

1. **Kinds**: J2.
2. **Merged record**: J4.1 to J4.4.
3. **T's notes.** When the merged field list is longer than T's, or the sort index (J4.4) differs from T's `sort_field_idx`, every note of T is rewritten as `notetypes/structure-changes#S5` says: each merged field gets the note's value at its origin, or the empty string; `notes/note-records#N2` normalization; `sfld` and `csum` from the merged note type; `mod` = `now_s` and `usn` -1 for every note of T, changed or not. A note storing more values than T has fields loses the extra values (J10). Otherwise T's notes are not written.
4. **T's cards** are never renumbered or removed: T's templates keep their positions. When the merged template list is longer than T's, or the list of front texts (`q_format`) by position differs from T's stored fronts (compared exactly, `notetypes/structure-changes#S2` "fronts changed"), `notes/card-generation#G9` runs on T's notes with the merged note type, placed by `notes/deck-placement-and-position#D4` and positioned by its P2 from the target's `nextPos`, which advances, as `package/apkg-import#I7` says for a front change. A front change alone marks no schema (J9).
5. **The copies' notes**, copy by copy in ascending copy id, and in ascending note id within a copy, move to T with the effects of Change Note Type (`notetypes/change-notetype#M9` steps 2 and 3): its field map has, for each merged position, the position in the copy of the copy's field matched there (J3), or none; for a standard T its template map has, for each merged template position, the copy's matched template, or none (M9.1); for a cloze T there is none and cards keep their ordinals (M9.3). Each map names every field and template of the copy, so no value is discarded and no card deleted, except values past the copy's field count (J10). New cards come from M9 step 3 (D3, whose item 3 is T's last deck; P2 from the target's `nextPos`). M9's checks and its steps 1 and 4 do not apply: the import sets `scm` and `col.mod` once (J9). The copy itself stays until J8.
6. **Write** the merged note type (J4.5).

Steps 3 to 5 run in this order, so G9 visits only T's own notes and the moved notes get their cards from M9 step 3, drawing counter values after G9's.

## J6. Stage 2: notes whose note type differs

After stage 1, the merge takes every package note in ascending id, leaving out those `package/apkg-import#I9` step 1 skips and, for a GUID two package notes hold, all but the first. When a target note holds its GUID and that target note's `mid` (as stage 1 left it) differs from the package note's `mid`, the target note joins the pair (E, I): E the target note's note type, I the package note's note type as the target holds it after stage 1 or I7's add. The pairs run in ascending (I, E), each against the state the earlier ones left. Decided 2026-10-27: Anki runs them in an unspecified order (J14). For each pair:

1. **Kinds**: E and I must be of one kind, otherwise `NotetypeKindConflict{incoming: I, target: E}` (J2).
2. **I's record** gains E's fields and templates that it lacks: J4.1 with I as the starting list and E as the only merging record (templates not merged when I is cloze). I governs, whatever Update note types says, and keeps its sort field index.
3. **I's notes** (the target's notes of I at this point) are rewritten as J5 step 3 by I's own origins when I's field list grew, and G9 runs on them as J5 step 4 when I's template list grew. Decided 2026-10-27: Anki remaps I's own notes and cards with E's maps here (deviation `apkg-merge-conflict-remap`, J14).
4. **The pair's target notes**, in ascending id, move from E to I as J5 step 5 says, with E's maps against I's merged lists.
5. **Write** I's record as J4.5 (with I's stored record standing for T).

E keeps its other notes and its rows; J8 removes it when it ends with none. With merge on, every target note whose GUID a package note holds therefore ends on that package note's note type, and I9 never logs `conflicting` (`package/apkg-import#I8`). Example: the target's note `g1` was moved by Change Note Type from 1001 to 3001; the package still holds `g1` under 1001: stage 2 merges 3001's fields that 1001 lacks into 1001, moves `g1` to 1001, and removes 3001 if no note uses it any more.

## J7. The maps the import applies

For each incoming note type id, the merge returns (`package/apkg-import#I8`):

- **Target note type**: the incoming note type's own id (T, or the note type I7's add created). A merge never sends incoming notes to another id.
- **Field map**: for each position of that note type's final field list (after both stages), the position in the incoming note type of the incoming field matched there in stage 1 (J3), or none. Fields stage 1 appended from the incoming note type are filled from their own positions; fields that only T or a copy has, and fields stage 2 appended, are none. For an added note type that stage 2 did not grow, the identity.
- **Template map**: each incoming template ordinal → the position of the template it matched in the final template list. For a cloze note type, the identity on every ordinal: card ordinals are cloze numbers.

Example: T `Front` (f1), `Back` (f2) with templates `Card 1` (t1), `Card 2` (t2); incoming `Front` (f1), `Extra` (f3), `Back` (f2) with templates `Card 2` (t2), `Card 3` (t3): merged fields `Front`, `Back`, `Extra`, field map 0, 2, 1; merged templates `Card 1`, `Card 2`, `Card 3`, template map 0 → 1, 1 → 2. An incoming card of ordinal 0 becomes ordinal 1, and I11's "pair exists" test runs on ordinal 1 (I8).

A GUID-matched note that I9 updates gets the incoming fields through this map, so its value in each field the map leaves none becomes the empty string (I8, as Anki 26.9.3); J10 previews that loss.

## J8. Removing merged note types

After I9 has written the notes, each copy merged in stage 1 and each E of stage 2 that no target note uses any more is removed with `notetypes/structure-changes#S12`'s effects 3 to 5: its `_nt_<id>_` config keys (`storage/config-table#C10`), its `notetypes`, `fields` and `templates` rows (no grave), and `curModel` set to the remaining note type whose name comes first under `unicase`, written only when it differs. It has no notes, so S12 step 1 removes nothing; `scm` is J9's. A merged copy never keeps a note: its notes moved in J5 step 5, and no package note uses its id (J1). An E that still has notes stays as it was.

## J9. Schema, safety backup and structure report (R1475, R1476, R2008)

- **Effect** (`notetypes/schema-changes#G3`). A merge has a structural effect when a merged field or template list is longer than the stored one (J5, J6 step 2), a sort index changed (J4.4), target notes moved (J5 step 5, J6 step 4), or a note type is removed (J8). Then its plan carries the reason `notetypes_merged` (G2), and only that reason: the import's write phase sets `col.scm` to `now_ms` once (G5), and the import is a structural edit whose safety backup `package/apkg-import#I14.4` takes after the confirmation (I8) and before the first write (`package/backup-triggers#T6.1`, T6.2).
- A merge without such an effect (equal layouts, nothing moved or removed; only values, names, texts, styling or stamps written) carries no reason: no `scm`, no safety backup, as Anki 26.9.3 sets `scm` only for those effects. A front change alone is never one (J5 step 4).
- **What this overrides.** Decided 2026-10-27: a merge's plan carries `notetypes_merged` and no other reason, whatever its effects. This overrides I8's "`notetypes_merged`, and whatever else its effect is" and, for a merge, `notetypes/schema-changes#G3`'s "carries all of them": a merge that changes a sort index or grows a field list carries `notetypes_merged`, not also `sort_field_changed` or `fields_changed`. One reason is enough for `scm` and for the backup (G3), so nothing is lost. I7's `sort_field_changed` for an update without a merge is unchanged. It also narrows I8's "Backup: a merge is a structural edit" to a merge with an effect, as `package/apkg-import#I14.4` already says ("one that marks either is a structural edit").
- **Follow-ups for the `package/apkg-import` owner** (this spec edits only cross references there): in I8, write "the structural reason `notetypes_merged` when the merge has an effect (`package/notetype-merge#J9`)" for "`notetypes_merged`, and whatever else its effect is", and "a merge with a structural effect (`package/notetype-merge#J9`) is a structural edit" for "a merge is a structural edit"; in I8's "Called with", add the reads J5 and J10 list; in I14.4, "with a merge 9" becomes "with a merge that has an effect (J9) 9".
- No Anki full-upload warning is shown (R1476). With `scm > ls` the next manual Anki transfer is one-way and its preview says so (`notetypes/schema-changes#G1`); routine CloudKit sync stays incremental (R2053).
- **Structure report** (`engine/change-stream#S2.5`, `sync/cloudkit-records#K19.1`, both under the R2066 review; this spec edits neither): reasons 8 `apkg_apply` and, when the merge has an effect, 9 `notetypes_merged`; `layouts` for each note type J5 or J6 wrote whose rows or sort field changed, with entry i of `field_map` the stored `ord` merged field i comes from or -1 (likewise `template_map`); `removed`, J8's ids; `type_changes`, one entry per (copy, T) and per (E, I) pair with J5 step 5's maps. Decided 2026-10-27: 9 is reported exactly when the plan carries the reason; `package/apkg-import#I14.4`'s "with a merge 9" and K19.1's wording are read that way (follow-ups above and below). Follow-up for that review: K19.1 and S2.5 should report 9 only for a merge with an effect, and its `engine/change-stream` op catalog case (8, 9) should be a merge with one; S2.5 says 9 is reported "whether or not a layout changed" and has one `type_changes` entry per old note type; an E whose notes go to two note types needs two.

## J10. Preview of destructive mappings (R1605)

The merge returns I8's preview: what the import would drop. A merge only adds fields and templates and moves cards with total maps, so it never deletes a card; what it can drop is listed here. It uses the inputs J5 lists beyond I8's: the target notes' fields and `mod`, the package notes' `mod`, and Update notes.

| Member | One entry per | Content |
|---|---|---|
| `emptied_fields` | note type X that receives package notes and position p of X's final field list that X's field map (J7) leaves none | X's id and name, p, the field's name, and the number of target notes of X that I9 will update (J11) whose text at p, as the merge leaves it, is not the empty string; only entries with a count above 0 |
| `removed_notetypes` | note type J8 removes | its id and name, the note type its notes moved to, and the number of notes this import moved out of it |
| `dropped_values` | note type whose notes J5 or J6 rewrites or moves | its id and the number of those notes storing more values than it has fields (only a damaged collection; Check Database repairs them, `notes/note-records#F1`) |

- Entries are in ascending note type id, then position. Text counts as in `notetypes/change-notetype#M5`: any stored text, markup and white space included.
- The preview is empty when every list is. An empty preview applies at once; otherwise `apply` returns `ConfirmationRequired{preview, token}` and writes nothing, with the token and replan rules of I8. Fields and templates the merge adds are not in the preview: they drop nothing.
- Declining is not calling `apply` again: the collection is unchanged and no backup is taken.
- Example: T has fields `Front`, `Back`, `Notes` (the user added `Notes`); the newer incoming note type has `Front`, `Back`; Update notes `IfNewer`; 40 target notes have text in `Notes` and older `mod`s than their package notes: `emptied_fields` holds T, position 2, `Notes`, 40. Nothing else changes T's layout, so no `scm` and no backup (J9); the import's undo restores the notes (J12).

## J11. The `mod` that I9 compares

With merge on, `package/apkg-import#I9` step 5 compares the package note's `mod` with the target note's `mod` as stage 1 leaves it: `now_s` for a note J5 step 3 rewrote or J5 step 5 moved, otherwise the snapshot's value. Stage 2's rewrites and moves do not change the value compared. This is Anki 26.9.3's behavior and matches I7's rule for a sort-field rewrite. So with `IfNewer`, a merge that adds a field to T updates none of T's notes (each was stamped `now_s` first) and logs them `duplicate`; with `Always` it updates every one whose package `mod` differs.

A note I9 updates gets the incoming `mod`; one it does not keeps the `mod` the merge gave it.

## J12. Write phase, undo and progress

- **Plan.** The merge runs in the plan (`package/apkg-import#I14.1` step 3), against the snapshot, cancellable there, with the plan half of every G9 run. A replan (I14.2) runs it again and compares the preview's digest (I8).
- **Write phase** (I14.1 step 8, one transaction): stage 1 per note type (J5 steps 3 to 6), stage 2 per pair (J6 steps 3 to 5), then I9's notes, then J8's removals, then the rest of step 8. `scm` is set once (J9) and `col.mod` once (I14.1).
- **Re-derived** in the write phase as I14.2 says for I7's regeneration: G9's and M9 step 3's new cards and the `nextPos` values they draw, from live rows; card renumbering by M9.1 reads live cards. The `TableUntouched` preconditions on `notes`, `notetypes`, `fields` and `templates` cover the rest: an edit there replans. The rows the merge writes count toward I14.2's long-write threshold.
- **Undo** is the import's one step "Import" (I14.3): it restores T's and every I's rows with their stamps, every rewritten or moved note (`mid`, `flds`, `sfld`, `csum`, `mod`, `usn`), every renumbered card, the removed note types with their `_nt_<id>_` keys, `curModel` and `col.scm` (`notetypes/schema-changes#G5`), and deletes the cards G9 and M9 step 3 added, without graves. Redo applies the after-images. Anki 26.9.3's merge is part of its one undo step "Import" too.

## J13. Errors

| Error | When |
|---|---|
| `NotetypeKindConflict{incoming, target}` | J2, in stage 1 or stage 2; the whole import ends with nothing written |
| `ConfirmationRequired{preview, token}` | J10 with I8's token rules |

The merge has no other error: a merge cannot miss a field or a template. Every other error of the import is `package/apkg-import#I16`'s. For scenarios, the kind conflict is recorded as `notetype_kind_conflict` (I16).

## J14. Differences from Anki

Decided 2026-10-27, in the order of `package/apkg-import#I17`: data safety first, then the plan's requirements, then Anki's behavior. Each difference a golden can see needs a `conformance/deviations.yaml` entry when its scenario is recorded (proposed ids):

| Situation | Anki 26.9.3 | Meguri |
|---|---|---|
| Two or more new fields placed before a field T has, or a copy whose fields are in another order than T's | the new fields can come out in another order than the incoming's (two inserted before a kept field come out reversed); a copy's order reorders T's own fields, so T's notes are rewritten | J4.1: T's fields keep their positions; new fields follow in each record's order (`apkg-merge-layout`) |
| Stage 2 (J6) when E's and I's layouts differ | I's own notes and cards are remapped with E's maps: values can land in the wrong field or be emptied, cards can be renumbered or deleted; I's sort index is read through E's order | I's notes use I's own maps; only E's moved notes use E's (`apkg-merge-conflict-remap`) |
| An incoming card after a merge that moved its template | "pair exists" is tested on the package ordinal; the template map is looked up by the target note's id, so a card of a note whose target id differs from its package id keeps the package ordinal | mapped by its package note's note type, the test on the mapped ordinal (`package/apkg-import#I8`) (`apkg-merge-card-ordinal`) |
| Stage 2 with several pairs | an unspecified order | ascending (I, E); no scenario needs more than one pair |
| Ids or names that repeat inside one note type | one field or template can be matched twice | one to one (J3); no scenario |
| A copy of the other kind | merged into T: its notes change kind and lose cards past the template count | left alone (J1, J2); no scenario |
| A copy whose id is an incoming note type's id | treated as a copy and again as an incoming note type | not a copy (J1); no scenario |
| Destructive mappings | imported without a question | J10's preview and I8's confirmation; no golden: scenarios run the import as accepted (J15) |

Anki behaviors Meguri keeps, which a reader might take for differences: `Always` governs also with equal `mtime_secs` (J4.2); every merged note type is stamped (J4.5); the sort index is kept as a number (J4.4); fields the incoming lacks are emptied in updated notes (J7, previewed by J10); `IfNewer` after a layout change updates no note of T (J11).

R1605's "by ids then names": a name is compared only when one side has no id; two different ids never match by name (J3). Neither side falls back to positions.

## J15. Scenarios and tests

### J15.1 Scenarios to record

None can be recorded yet: they need the deck-package `import` form with `merge_notetypes`, the latest deck-package `export` and the fixture packages that `package/apkg-import#I18.1` asks the oracle maintainer for. Each scenario defines its own note types with field and template ids (R1959), builds or loads the package, imports it with Merge note types on, and observes `db_dump` (note types with fields and templates, notes, cards, config `curModel` and `_nt_` keys), `col.scm` through `db_exec` (`conformance/notetype-ops#T9`) and the import log. On Meguri the adapter accepts every preview (it calls `apply` again with the token), as schema ops run accepted (T9); J10's preview is tested by Meguri's own tests.

| Scenario | Shows |
|---|---|
| `add-field` | incoming newer with an extra field `Notes`: T gains it at the end, T's notes rewritten, `scm` set; with `IfNewer` the GUID-matched notes are `duplicate` (J11), with `Always` they are updated with `Notes` filled |
| `insert-field` | incoming `Front`, `Extra`, `Back`, sort field `Back`, newer: merged `Front`, `Back`, `Extra`, field map 0, 2, 1, sort index 2 (J4.4) |
| `two-new-fields` | incoming `Front`, `Hint`, `Source`, `Back`: the merged order (deviation `apkg-merge-layout`) |
| `template-renamed` | T's template renamed locally, same id: matched by id, no second template; with `Never` T's name stays, with `Always` the incoming's |
| `by-name` | a fixture package whose note type stores no field or template ids (made with an old client by the maker of I18.1): matched by names; a field renamed on one side is added as new |
| `template-moved` | T `Card 1`, `Card 2`; incoming `Card 2`, `Card 3`: template map 0 → 1, 1 → 2; G9 adds `Card 3` cards to T's notes; an incoming card for an existing mapped pair is skipped (deviation `apkg-merge-card-ordinal` when a note id was shifted) |
| `copy-cleanup` | an import without a merge adds `Name+`; the next import with a merge moves its notes into T (M9 maps, cards renumbered), removes it with its `_nt_` keys, and `curModel` follows J8 |
| `kind-conflict` | target standard 1001, package cloze 1001: `notetype_kind_conflict`, target unchanged; the same for a stage 2 pair |
| `guid-conflict` | a target note moved by Change Note Type to E; the package holds it under I: E's extra fields appended to I, the note moved back, E removed when empty, I's other notes unchanged (with layouts that differ: deviation `apkg-merge-conflict-remap`) |
| `same-schema` | equal layouts, incoming newer: T's record replaced and stamped, notes untouched, `scm` unchanged |
| `emptied-field` | J10's example: updated notes lose `Notes`, `scm` unchanged |
| `update-always-equal` | Update note types `Always` with equal `mtime_secs`: the incoming governs (J4.2, unlike I7) |
| `undo` | a merge with a copy removed, then `undo`: the dump equals the pre-import dump, `scm` and `curModel` included; `redo` equals the import's |

Each golden whose Meguri result differs (J14) gets its `conformance/deviations.yaml` entry with the scenario.

### J15.2 Meguri's own tests

`core/crates/meguri-package/tests/apkg_merge.rs` (U121), with packages its own writer builds (never from `local-data/`):

- J10: each preview member, its order, an empty preview applying at once; declining (no second `apply`) leaves the collection and the backup folder unchanged; accepting with the token takes the safety backup before the first write for a structural merge and none for a merge without effect; an injected backup failure gives `SafetyBackupFailed` and writes nothing; answers committed between the two calls do not invalidate the token; a replan whose preview changed returns `ConfirmationRequired` again (I8).
- J2: a kind conflict in stage 1 and in stage 2 writes nothing; a copy of the other kind is left alone.
- J3 one-to-one matching with a repeated id; J1's copy that is an incoming note type's id.
- J6 with two pairs on one I, and with one E split over two I's; J6 step 3 leaves I's own notes' values in place when E's layout differs.
- J9: `scm` set exactly for the effects listed; the storage guard (`notetypes/schema-changes#G6`) passes. The structure report cases (reasons 8 and 9 exactly when J9 says, layouts, removed, type changes) go in `core/crates/meguri-engine/tests/op_catalog.rs`, which `engine/change-stream#S2.5` owns, once the R2066 review settles S2.5; until then `apkg_merge.rs` checks the reasons the plan carries.
- J12: undo and redo of a merge with removals restore `curModel` and the `_nt_` keys; the failpoint kill during the write phase leaves all or none of the merge (I18.3).

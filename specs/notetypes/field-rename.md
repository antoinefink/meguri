# Field rename prerequisite

Spec: notetypes/field-rename
Requirements: R903, R906, R921, R923, R949, R950, R1485; U29/U107 upload proof
Anki: 26.09.3 (prepared neutral contract, b70649df; no new reference research)
Scenarios: focused engine and real normal-job tests; prepared U34 rename cases are retained for reference

## F1. Scope

This is the production field-rename prerequisite for U107, not full U34.
U34 remains in M1 without an acceptance trailer for this slice. Field/template
add, removal, reorder, independent template edits, sort changes, deletion,
Restore to Default, editors and their full FFI surface remain outside it.
The plain save continues to refuse field renames and structural changes.

The typed rename operation uses the existing executor, journal and undo path.
It is not a test-only write or a flag that disables the plain-save guard.
Renaming fields is not a schema change and never asks for an Anki upload warning.

## F2. Base, identity and validation

Input binds the note type, its captured `AggregateDigest`, and one or more field
identities to requested names. Retain each field's stored optional ID and the
ordinal captured with it. The ordinal must still identify that same stored ID.
Never identify a field by its requested name or generate another field ID.
Because this slice preserves order, the ordinal also disambiguates legacy
missing or repeated IDs. This does not define identity for U34 reorders.

Compare the live aggregate with the base token before writing, under the
existing U29 digest contract. A changed aggregate returns `Stale`, writes
nothing and takes no undo step. Ordinary parent USN marking alone retains
the existing digest behavior. Duplicate or out-of-range field identities are
invalid input. Field count/order, sort field, kind and template count/order
must remain unchanged; unrelated draft changes are refused.

Apply `notetypes/records#N4` normalization and uniqueness before computing the
rename map. Map each changed field's base name to its final normalized name.
Case-only changes count. Field-editor callers retain V5's typed-name check;
the core uses V1's field-editor save mode and N4's complete final name list.
Do not reject simultaneous swaps by checking each new name against the old
names between edits. Invalid normalized input retains the no-write refusal.
If final names equal the base names, the operation writes nothing.

## F3. Template references

Reuse U60's parsed tags, delimiters and source spans. Apply the final rename map
simultaneously, once per original reference; swapped names must not cascade.
Process every template's front, back, browser front and browser back:

- Empty browser texts remain empty. A text with a structural parse error remains
  byte-identical, without inserted references; process its other texts normally.
- Match replacement keys after the existing filter split, and section keys
  after the existing trim. Compare exact code points. Rewrite matching plain,
  filtered, conditional, negated and closing tags in that text's delimiters.
  Preserve filter order. Comments and unrelated tags/text stay byte-identical.
- After a real rename, a standard front/browser front with no replacement tag
  gets the first field's replacement appended, without added whitespace.
  Sections alone do not count as replacements. Standard backs get no fallback.
  For cloze types, every other processed text without a cloze replacement gets
  the first field's cloze replacement appended. Retain alternate delimiters.

These are the rename-only rules of prepared structure-changes S4, including
its text-preservation deviation. There is no field-removal map in this slice.
Compute saved requirements from the final texts and final field names, as
`notetypes/records#N6` requires, not from the earlier fronts.

## F4. Planning and atomic application

Preserve field/template IDs, ordinals, options, protection, tags, row metadata
and unknown data except the known names/texts/requirements this operation edits.
Notes retain their field values, derived columns, stamps and identities.
Existing cards are not removed or remapped by a rename.

When rewritten fronts differ, use U27's existing G9 regeneration plan against
the new in-memory note type. Its commit re-derives live note/card membership
inside this operation's transaction. Preserve G9 placement, no-card-note and
existing-card rules; do not replace it with a rename-only shortcut.

Apply the renamed aggregate and any generated cards in one transaction and one
`update_note_type` undo step. Use existing long-write/progress/cancel contracts
when regeneration requires them. Failure rolls the whole operation back.
Undo restores before-images; undo removes newly generated cards without graves.
Redo applies the corresponding after-images.

Write/stamp through `notetypes/records#N3`: changed aggregate rows retain unknown
data; the parent gets current mtime and pending USN; collection modification
moves once. `scm` stays unchanged. Publish the existing coherent change set.
A repeated normalized no-op changes no stamp, journal, undo or publication.

## F5. Focused proof and integration

Check single and simultaneous renames, case-only names, stale bases, identity
refusals, normalization/no-ops, four-text rewriting, filters/sections/comments,
alternate delimiters, empty browser texts and structural errors. Preserve
unknown fields and existing notes/cards. Check final requirements, required G9
generation, one-step undo/redo and atomic rollback.

U107's real test must call this production operation after a completed baseline
round, then run the normal job. The whole edited note type reaches `applyChanges`
and a fresh peer, with matching `scm` and no one-way request. A raw scope write,
pending-row helper or synthetic acknowledgement does not satisfy that proof.

Reuse the prepared neutral rename expectations. Do not activate mixed U34
remove/template-edit scenarios or declare their unsupported capabilities.

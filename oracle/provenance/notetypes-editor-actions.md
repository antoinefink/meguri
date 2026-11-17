# Note type editor actions (U200 to U203 UI side)

Spec-writer notes for `specs/notetypes/editor-actions.md` and the draft scenario
`conformance/scenarios/notetypes/manage/list-order.yaml`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. No oracle probe was run (the oracle was busy):
every rule below is from reading the desktop screens, the Python model layer and the
backend queries; the list-order scenario has no golden yet.

Meguri's own design, with no Anki source: A1's changed-state rule (draft against its
base), A2's core reads and the selection after a delete, A3's clone order and the
disabled `"`-only names, A5's fallback for a card without a template, A10's selection
after a field delete, the sort field tracking and the kept values, A11's handling of a
dangling override, the check on confirm and deck creation at save with its order and
refusal handling, A12's kept unedited formats and the font pair rule, A14's choices,
cursor insertion, lead rule and disabled `%>` names, A16's disabled template choices.

| Spec section | Anki files read | Read for |
|---|---|---|
| A1 | `qt/aqt/clayout.py` (every change-tracker call: text edits, add card type, remove, reorder, flip, browser display OK, target deck after the dialog, add field; none in rename), `qt/aqt/fields.py` (add, delete, position, sort, rename, option writes) | when Anki's editors count as changed |
| A2 | `qt/aqt/models.py` (constructor with a selected id, the first load, refresh, list rebuild keeping the row index, select by id or row, delete), `pylib/anki/models.py` (use counts), `rslib/src/notetype/service.rs` (names and counts), `rslib/src/storage/notetype/get_use_counts.sql`, `rslib/src/storage/notetype/mod.rs`, `qt/aqt/browser/sidebar/tree.py` (manage note type), `ftl/core/browsing.ftl` | one query with counts per note type ordered by the name column (unicase collation), row text `name [N notes]`, selection by id after add and rename, the kept row index after a delete (none when it was the last row), the sidebar's id preferred whenever set |
| A3 | `qt/aqt/models.py` (add sheet and name prompt), `qt/aqt/operations/notetype.py` (legacy add and update), `pylib/anki/models.py` (add dict, update dict, the unique-name helper, id for name, copy), `pylib/anki/utils.py` (checksum), `rslib/src/storage/notetype/mod.rs` (id by name, `where name = ?` on the unicase column), `pylib/anki/stdmodels.py` (stock order), `ftl/core/notetypes.ftl` | stock choices first in proto order, clones sorted by name with a plain string sort, the prefilled names, a blank name ignored after Python strip only (no `"` removal before the test), the untrimmed name passed to the add, the hyphen plus five sha1 hex characters of the time string on a unicase collision before the backend sees the name |
| A4 | `qt/aqt/models.py` (rename), the same legacy update path | prefilled name, blank ignored, untrimmed name saved with the checks and the same collision rename (an id equal to the note type's own does not collide) |
| A5 | `qt/aqt/editor.py` (card layout entry: the editor card's ordinal, else 0), `qt/aqt/clayout.py` (constructor, cloze number box), `qt/aqt/browser/sidebar/tree.py` (manage note type opens the list with the id; manage template opens the card layout on the template ordinal with a new note and fill-empty on; manage fields opens the field dialog at the field ordinal), `qt/aqt/models.py` (cards from the list: a new note, ordinal 0, fill-empty on), `qt/aqt/fields.py` (open position) | the ordinal the template editor opens at from each entry, the sidebar preview state, the field the field editor selects |
| A6 | `qt/aqt/clayout.py` (add card type, new card name), `pylib/anki/models.py` (new template from the stock Basic template), `ftl/core/card-templates.ftl` | the question only when notes exist, front and back copied and nothing else, appended last and selected |
| A7 | `qt/aqt/clayout.py` (remove), `pylib/anki/models.py` (template use count) | the count by the template's stored ordinal (none for a new template), the selection after removal |
| A8 | `qt/aqt/clayout.py` (rename card type) | `"` removed, blank ignored, no trimming, no change mark |
| A9 | `qt/aqt/clayout.py` (reorder: returns before the schema question at the current position), `qt/aqt/fields.py` (position: only the range check, then the move with the schema question; drop; move), `pylib/anki/models.py` (reposition field returns early at the same index; reposition template), `ftl/core/card-templates.ftl`, `ftl/core/fields.ftl` | the position prompt, ignored inputs, remove-and-insert semantics, selection after the move, the field case at the current position |
| A10 | `qt/aqt/fields.py` (unique-name helper with the old name as default, add, delete selecting row 0, sort index set to the current row, load showing the sort field by original ordinal, option writes), `qt/aqt/forms/fields.ui`, `pylib/anki/models.py` (new field, set sort index), `rslib/src/notetype/mod.rs` (the sort index read back as an original field ordinal, clamped when none matches) | add at the end and select it, rename prefill, delete count and selection, sort toggle that cannot be cleared, the sort index quirk, size range 5 to 300, description untrimmed, values written back when a field's options are left |
| A11 | `qt/aqt/clayout.py` (target deck: Close button only, change mark after the dialog, blank clears, id by name with creation), `pylib/anki/decks.py` (id by name with creation, get with default), `ftl/core/card-templates.ftl` | prefilled full name, Default shown for a missing deck, blank clears, a name reuses or creates a deck when the dialog closes, filtered decks not excluded, the (on)/(off) suffix |
| A12 | `qt/aqt/clayout.py` (browser display and its OK handler, connected to accepted only), `qt/aqt/forms/browserdisp.ui`, `ftl/core/browsing.ftl` | both formats trimmed on every OK, one font toggle for name and size, defaults Arial and 12, size range 6 to 99, both keys removed when off |
| A13 | `qt/aqt/clayout.py` (copy markdown), `ftl/core/card-templates.ftl`, `ftl/qt/about.ftl` | the exact headings and fences, the backtick escape (non-overlapping, left to right), untranslated text, the tooltip |
| A14 | `qt/aqt/clayout.py` (add field: the note type's field names only; the button hidden while Styling is shown), `qt/aqt/forms/addfield.ui`, `ftl/core/fields.ftl` | appended styled `div` at the end of the shown front or back with plain braces, default font and size, size range, no special names |
| A15 | `qt/aqt/clayout.py` (flip), `ftl/core/card-templates.ftl` | the separator text, the last usable occurrence with text on both sides (a greedy dotall match), the trimmed new front, the new back's fixed prefix, the dropped text before the separator, one message whenever no occurrence qualifies, the change mark only on success |
| A16 | `ts/routes/change-notetype/lib.ts`, `ts/routes/change-notetype/Alert.svelte` (first three names joined by comma and space, then three ASCII full stops and `(+N)`), `ts/routes/change-notetype/NotetypeSelector.svelte`, `qt/aqt/changenotetype.py`, `rslib/src/notetype/service.rs` (names), `rslib/src/storage/notetype/get_notetype_names.sql` | the target list, the opening state on the old note type, a used old template moving to the new row, the unused-items lists and their collapsed form |

## Trim set

Anki's editors trim with Python's string strip, whose white space is the White_Space
property plus U+001C to U+001F (checked by listing the code points Python treats as
space); the spec's "trimmed" names that set without naming the language. The field
editor strips with the same set after removing `"`, while `notetypes/validation` V5
step 1 trims White_Space only: left to the owner (edit request).

## For the oracle maintainer

`notetypes/manage/list-order` uses only existing ops and `db_exec`; Meguri runs it once
U34's `remove_notetype` adapter lands. Record it with
`tools/mg oracle record notetypes/manage/list-order` when the oracle is free; the
expected first list is `_under` 0, `alpha` 1, `Straße` 1, `Strasse2` 0, `Zeta` 2,
`Äbc` 3; the second `_under` 0, `ALPHA` 1, `Straße` 1, `Strasse2` 0, `Äbc` 3; the last
step 0.

## Session s-20261117T144450Z-f49841c9 (spec writer): deferred harvest rows, batch 1

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows and write their scenarios; no code, test text or fixture value was copied, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| notetypes/editor-actions#A2 note counts (needs-op rows h68d09bf06f67, h706cf39b03b8) | `rslib/src/notetype/service.rs` | Which behavior the names-and-counts tests require: an unused note type has count 0, and a note added to one makes it 1. Confirmed that no existing observation reads this list. |

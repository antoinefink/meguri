# Search sessions and result sets (U85)

Spec-writer notes for `specs/browser/result-sets.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Most of the file is Meguri's own
contract; the Anki behavior it keeps is the toggle and the selection mapping
(H11, H12), not yet checked against the oracle (H16 lists the scenarios).

| Spec section | Anki files read | Read for |
|---|---|---|
| result-sets H11 | `qt/aqt/browser/table/table.py`, `qt/aqt/browser/table/model.py`, `qt/aqt/browser/table/state.py` | the toggle re-runs the last search in the other mode with that mode's sort, rolling back on failure |
| result-sets H12 | `qt/aqt/browser/table/table.py`, `qt/aqt/browser/table/state.py`, `rslib/src/storage/schema11.sql` | selection kept by id after a search, cards to their distinct notes, notes to all their cards, the current note to its first card by the card lookup's order (the note id index, so lowest id), the 500-row cap, the fallback current row |
| result-sets H10 | `qt/aqt/browser/table/model.py`, `qt/aqt/browser/browser.py` | after an operation Anki redraws cells without searching again and shows deleted rows until the next search |
| result-sets H12 (Select Notes) | `qt/aqt/browser/browser.py` | Select Notes searches the selected notes' ids and selects all |

## Audit fixes (2026-11-17, session s-20261117T172637Z-685ca4ee)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to fix spec readiness audit findings; no code was copied, and every rule was restated in the spec writer's words.

| Subject | Anki file | Read for |
|---|---|---|
| browser/result-sets#H12 (mapped current with no mapped selection; Anki treats a current row of 0 as none) | `qt/aqt/browser/table/table.py` (selection save/restore, the rule that qualifies selected rows, the helpers for intersected and toggled selections) | Checking U85:F5: with an empty mapped selection, Anki selects the mapped current row alone (or row 0 when there is none), and it replaces a mapped current of row 0 with the first selected row |

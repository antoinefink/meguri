# Deck options screen behaviour (U232 to U234)

Spec-writer notes for the 2026-11-10 additions to `specs/sched/deck-options.md`
(O12's new bullets, O14 including O14.7, O15, O16, and the matching wording in
O1, O3, O6.2 and O6.6). Read at tag 26.09.3 (`refs/anki-26.09.3`); no code was
copied. The oracle was not run: every rule here is screen state or desktop
dialog flow, not core output, so these sections add no scenario (O14.6 names
the existing `sched/options/deck-limits` for what each sent state writes).

A first draft added an O4.3 warning code for a Random insertion order. It was
dropped after review on 2026-11-10: no Product Contract item asks for it, so the
Anki warning is recorded in O12 instead.

| Spec section | Anki files read | Read for |
|---|---|---|
| deck-options O12 (Insertion order Random) | `ts/routes/deck-options/NewOptions.svelte`, `ftl/core/deck-config.ftl` | the warning shown whenever the insertion order is Random, and its gist (prefer Sequential plus a gather order); the insertion-order help saying a change updates existing positions |
| deck-options O14.1 to O14.4 | `ts/routes/deck-options/TabbedValue.svelte`, `ts/routes/deck-options/lib.ts` (the tab value class), `DailyLimits.svelte`, `FsrsOptions.svelte`, `DeckOptionsPage.svelte` | the three limit tabs and two retention tabs; each tab's remembered value, its start value (the stored Today-only limit whatever its day) and its disabled value; opening on the last tab that holds a value; on a switch, the new tab's value from its own, else its start value, else the shown value; earlier tabs reset to their initial values, later tabs set to none; the Preset tab rebuilt from the newly selected preset's current value on every preset change, so a left-behind Preset edit is dropped |
| deck-options O14.1 (emptied field) | `ts/lib/components/SpinBox.svelte` | a value that does not parse as a number leaves the bound value unchanged and the field is redrawn with it |
| deck-options O14.3 (opening not a change) | `ts/routes/deck-options/[deckId]/+page.svelte`, `lib.ts` (modified check) | the original state captured after the components mount, so the opening tab rules are part of it; f32 comparison |
| deck-options O14.6 | `DailyLimits.svelte`, `FsrsOptions.svelte` | warnings computed from the shown values |
| deck-options O14.7, O12 (Restore Default visibility) | `ts/routes/deck-options/SpinBoxRow.svelte`, `ts/routes/deck-options/SpinBoxFloatRow.svelte`, `ts/lib/components/RevertButton.svelte`, `DailyLimits.svelte`, `FsrsOptions.svelte` | one revert control per tabbed row, bound to the shown value with the preset option's built-in default (new per day, reviews per day, desired retention); shown when the two differ after rounding to 2 decimals; reverting writes the default into the shown value, which the tab component then stores on the active tab |
| deck-options O15.1 | `qt/aqt/deckoptions.py`, `qt/aqt/reviewer.py` | the reviewer's O key; with no active card, the studied deck's options; otherwise the "which deck" prompt when the card's home or current deck differs from the studied deck, normal decks first; a filtered deck opening its own filtered-deck dialog |
| deck-options O15.2, O12 | `qt/aqt/reviewer.py` | after an op that changes study queues, the reviewer fetches the next card (Meguri keeps the shown card first instead, by review-session C4 and P11) |
| deck-options O15.3 | `ts/routes/deck-options/SaveButton.svelte`, `ts/lib/tslib/shortcuts.ts`, `ts/lib/tslib/keys.ts`, `ts/routes/deck-options/lib.ts` (the commit-editing helper), `qt/aqt/deckoptions.py`, `[deckId]/+page.svelte` | save on Control+Enter, shown through the platform string helper and mapped to Command on macOS by the platform modifier table; the active edit ended before the save and before the pending-changes check; close after save; the close event asking to discard pending changes |
| deck-options O16, O12 | `ts/routes/deck-options/LapseOptions.svelte`, `AdvancedOptions.svelte`, `NewOptions.svelte` | Minimum interval and the relearning-steps-versus-minimum warning shown only without FSRS; historical retention and the ignore date shown with FSRS |

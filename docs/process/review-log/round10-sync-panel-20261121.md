# Round 10 sync panel: spec review (2026-11-21)

Scope: `docs/design/native-interface-contract.md` N23, `specs/sync/status-strings.md` T20 and the `sync.status` note in `docs/design/screen-inventory.yaml`, written for Antoine's round 10 decision `sync.statusPopover` (6A).

Reviewer: an independent Claude (Opus) subagent, given the spec diff, the decision row and the 6A option text, not the author's conclusions. Read-only; it read no `oracle/`, `refs/`, `local-data/` or Anki source.

## Findings and dispositions

Blocking:

1. Meguri Sync with no round reported, or media blocked, had no defined Sync Now. Fixed: N23.3 has a row; File › Sync Now and Y keep T18.7's routes; N23.5 lists T18.7's three popover buttons.
2. Return could run an action that replaces data. Fixed: Finish Setup, Resume and Cancel Setup are never the default (N23.3; `SyncStatusAction.neverDefault`).
3. Copy Debug Info in more states than the decision's two. Fixed: N23.3 and T20.1 name the decision's two states and say that `localUnusable` (2026-11-20) and T19.4 row 1 (repair screen not built) are as built.
4. The `icloud` symbols are reserved for Apple's iCloud. Confirmed from the system's SF Symbols metadata (`symbol_restrictions.strings`: "may only be used to refer to Apple's iCloud service"). Fixed: iCloud keeps them; Meguri Sync and Off draw the same shapes from the unrestricted `cloud` symbol (N23.2, N23.4 exception, `decisions.md` row of 2026-11-21).
5. Replaced texts missing from the lists. Fixed: N23.5 lists them; T20's intro refers to it.

Should fix, all fixed in the text: 6 (built actions show disabled; not-built ones named), 7 (Sync Now disabled while moving, even where P12.1 joins; no Cancel in the panel), 8 (Meguri bar is the Account row's value whenever it has one; signing in has a bar), 9 (bar only with the moving shape; no bar beside a checkmark), 10 (iCloud `syncingMedia` has no detail), 11 (a detail equal to the title is not shown, in both places), 12 (only the status action), 13 (where actions and confirmations show), 14 (`sync.status.cancelSetup`; Esc only closes), 15 (sheet detents, drag indicator, buttons stack), 16 (only the toolbar symbol turns), 17 (N16.2 presence and the symbol are separate rules; setup required is the plain cloud), 18 (`sync.button.label.off`), 19 (symbol table rows), 20 (sentence column wording), 21 (the issues line is deliberate; unknown count falls back), 22 (closing rules; iCloud's Choose… path as built), 23 (screen contract new version note; `screens.md` regenerated), 24 (tests named; VM routes). From 24, the Mac UI test of Return and Esc is added (`SyncStatusPanelKeyTests`) and recorded as queued while `mac-input` blocks (re-review item 6).

Nits: 25, 28, 29, 30 (needs you with no action has no default), 31 and 32 taken into the text; 26 (Copy Debug Info is disabled while copying, as in the Sync section) stated; 27 kept `sync.status.popover` for continuity with existing tests.

## Re-review (same reviewer, revised text)

Blocking 1 (N23.3's rows overlapped with no precedence): fixed, "The first row that applies decides, in this order."
Should fix, fixed: 2 (a needs you action that cannot apply leaves Sync Settings alone, no default), 3 (Resume Syncing opens the continue page as built, listed in N23.5), 4 (on Mac an action with a confirmation opens the Sync section; touch shows it over the panel), 5 (kept the view; N23.4 says why a custom symbol cannot be made here and that one replaces the view when made; "the same five badges in a cloud"), 6 (Mac key test added, queued under `mac-input`).
Nits 7 to 15 fixed in the text and the inventory's route list.

Decision: in force after these changes, approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs.

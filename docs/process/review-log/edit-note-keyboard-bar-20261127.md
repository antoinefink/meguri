# Edit Note keyboard bar and kept answer after an edit — review log, 2026-11-27

Start: `05539e947e527ba7ac7fc2abd7bd62e0424a308c`. Implementer session `s-20261127T072201Z-7ba414d7` (edit-keyboard-bar). Branch `edit-improvement`.

## Units

1. **Touch keyboard bar and field font** (owner-approved option B, N17.1 2026-11-27). The keyboard bar on iOS and iPadOS is Bold, Italic, Underline, Cloze (hold for the same number), then previous and next field, then the HTML control. It has no Done: dragging the form down hides the keyboard. Every form shows field text in the system font (body text style with Dynamic Type on touch). The note type's editor font stays on the card. Spec: editor/formatting#F12, editor/cloze-wrap#W2, editor/field-html#F2 (exception to R828).
2. **Reveal right after a present** (owner-reported `notPrepared` banner after Done in Edit Note). Saving an edit made on the answer side presents the card again and then reveals it in the same main-actor turn (`review/edit-current#E3`). The page counts as presented only once its Core Animation commit completes, so the reveal failed. `CardPagePair.showAnswer` now waits for that pending commit. The wait fails with `notPrepared` after 2 s, or when a newer present, a suspension, close or a terminated web process ends the commit.

## Verification

- `swift-test` (Mac host), `CardPagePairTests` and `FieldEditorTests`: all pass. This includes `aRevealRightAfterPresentWaitsForTheCommit`, `aRevealWaitingForTheCommitFailsWhenThePairCloses`, `fieldTextTakesTheSystemFontNotTheNoteTypes` and `theClozeGroupShowsOnlyForAShownClozeField`.
- Negative check: with the commit wait removed, `aRevealRightAfterPresentWaitsForTheCommit` fails with `Caught error: .notPrepared`.
- `build-app --platform ios-sim` (Debug): pass.
- Inherited: the `AppModelTests` bundle hangs after its suites pass when it is run with a filter (the process never exits). It was stopped by hand twice. The cause is not in this change.
- Not run on the host: `theKeyboardBarFormatsAndHasNoDone` (iOS only).
- VM drive of Edit Note on iPhone: see the pull request for the receipts and screenshots.

## Independent review

A read-only Opus reviewer found nothing blocking. Its findings and what was done:

- No time limit on the wait → 2 s limit, as in Browse's preview.
- Resumption failed waiting reveals → waiters now settle only where the commit ends.
- Missing failure-exit and hidden-cloze-field tests → added.
- Stale placeholder font line in field-html → reworded to "the field's text size (F2)".
- `UIAction.accessibilityIdentifier` compile check → the iOS build passed.

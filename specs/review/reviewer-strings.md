# Reviewer strings

Spec: review/reviewer-strings
Requirements: R86, R363, R370, R382, R383, R387, R429, R430, R432, R433, R434, R435, R436, R437, R439, R456, R581, R635
Anki: 26.09.3 for S3 to S9 (the wording, plural forms and triggers of Anki desktop's reviewer, congratulations page and unbury dialog); none for S10 and the keys
Scenarios: conformance/scenarios/review/ (none recorded yet, S12)

The text the review screen shows besides the card: toasts, the timebox prompt, Auto Advance reminders, the congratulations notices and the unbury choices. U175, U182, U185, U186 and U187 take every such string only from this file (KTD1). U155's finished state and U157's summary reuse S8 and S9 for the same facts.

## S1. Keys, arguments and plurals

- Every string is an entry of the Review String Catalog with the stable key given here. The English value is the one given; other languages are Meguri's own translations of the English. Keys never change once shipped; a changed meaning gets a new key.
- `%lld` is an integer argument, `%@` a text argument; with two arguments they are numbered (`%1$lld`, `%2$lld`). A plural entry has the catalog's plural variations on its integer argument: English uses **one** for exactly 1 and **other** for every other value, 0 included; other languages follow their CLDR rules. An entry with two integer arguments pluralizes each one separately.
- Text is shown as given, with the trailing period where it has one. Decided 2026-10-27: apostrophes are typographic (’), the platform's typography; Anki writes ASCII `'`. Every other difference from Anki's English is listed in S11.

## S2. Toasts (R581)

- A toast is non-blocking: it never takes taps on the card or the buttons, it hides after about 2 seconds, and VoiceOver announces its text when it appears. Decided 2026-10-27: one toast at a time; a new toast replaces the one on screen.
- A toast follows the op's success. An op that fails or is refused shows no toast; its error path applies (R362).
- An op's undo or redo never shows the op's toast again (S5).

## S3. After an answer (R363)

No toast follows an answer, except the leech toast.

| Key | English | Shown when |
|---|---|---|
| `review.toast.leech` | Card was a leech. | the accepted answer made the card a leech (`sched/leeches`) and the card is not suspended after it (leech action Tag Only) |
| `review.toast.leechSuspended` | Card was a leech. It has been suspended. | the same, and the card is suspended after the answer (leech action Suspend Card) |

- "Suspended after the answer" is read from the accepted outcome (the card's `queue` is -1 after it), not from the preset.
- Not shown again when the answer is redone (S2).
- When the answer also reaches the timebox (S7), the toast shows together with the sheet.

## S4. Card actions in review (R382, R383, R387)

| Key | English (one / other) | Argument | Shown after |
|---|---|---|---|
| `review.toast.cardsBuried` | `%lld card buried.` / `%lld cards buried.` | cards the op wrote | Bury Card, Bury Note, and Auto Advance's Bury Card answer action |
| `review.toast.cardSuspended` | Card suspended. | | Suspend Card |
| `review.toast.noteSuspended` | Note suspended. | | Suspend Note |
| `review.toast.noteDeleted` | Note deleted. | | Delete Note, with the action below |
| `review.toast.undo` | Undo | | the action button of `review.toast.noteDeleted`; it runs the undo of that step (U179) |
| `review.toast.dueDateSet` | `Set due date of %lld card.` / `Set due date of %lld cards.` | cards given to the op | Set Due Date… (U183) |
| `review.toast.cardsReset` | `Reset %lld card.` / `Reset %lld cards.` | cards given to the op | Forget… (U183) |

- The bury count is the op's count of cards it wrote (`sched/manual-ops#M2`, M9): Bury Note on a note with 3 cards of which one is suspended says "2 cards buried."; Bury Card on the current card says "1 card buried.".
- The suspend toasts carry no count: "Note suspended." also for a note whose other cards were already suspended.
- The delete toast carries no count, whatever the number of cards removed with the note (R387; Anki says "3 cards deleted." for a note with 3 cards). VoiceOver announces it with its Undo action.
- Flag and Mark Note show no toast; the flag and ★ indicators change in place (R380, R381) and their accessibility values change. (Anki's reviewer shows none either.)

## S5. Undo and redo (R370)

| Key | English | Argument |
|---|---|---|
| `review.undo.named` | `Undo %@` | step name |
| `review.undo.plain` | Undo | |
| `review.redo.named` | `Redo %@` | step name |
| `review.redo.plain` | Redo | |
| `review.announce.undone` | `%@ undone` | step name |
| `review.announce.redone` | `%@ redone` | step name |
| `undo.step.<id>` | the name of `specs/conformance/undo-ops.md` V4 for `<id>` | |

- The step name is `undo.step.<id>` for the step's V4 id (`undo.step.answer_card` = "Answer Card", `undo.step.bury` = "Bury", `undo.step.delete_note` = "Delete Note"). The named forms label the back button, its Mac tooltip and the Undo and Redo items; the plain forms are used when there is no step.
- After an undo or redo in review, VoiceOver hears `review.announce.undone` or `review.announce.redone` with the step's name ("Answer Card undone"). Decided 2026-10-27: no visible toast; the card coming back is the visible feedback (Anki shows the same text as a tooltip). R581's toast list does not include undo.

## S6. Auto Advance (R432, R433)

| Key | English | Shown when |
|---|---|---|
| `review.toast.autoAdvanceOn` | Auto Advance enabled | the user turns Auto Advance on (⇧A or the ⋯ item) |
| `review.toast.autoAdvanceOff` | Auto Advance disabled | the user turns it off |
| `review.toast.questionTimeElapsed` | Question time elapsed | the question countdown ends and the question action is Show Reminder |
| `review.toast.answerTimeElapsed` | Answer time elapsed | the answer countdown ends and the answer action is Show Reminder |
| `review.autoAdvance.unavailable` | Auto Advance needs a number of seconds to show the question or the answer in this deck’s options. | explanation of the disabled toggle, both seconds 0 (R433; Meguri's wording) |

- Not shown: when Auto Advance turns itself off because the app left the foreground or the Mac app deactivated (R433); the ⋯ item's checked state shows it. A reminder shows once per countdown.

## S7. Timebox (R86, R429, R430)

| Key | English | Arguments |
|---|---|---|
| `review.timebox.message` | `%1$lld card studied in %2$lld minute.`, each argument with its own plural: `card` / `cards`, `minute.` / `minutes.` | cards, minutes |
| `review.timebox.continue` | Continue | |
| `review.timebox.finish` | Finish | |

- Trigger: `timeLim` > 0, and the active time since the timebox started (R431) is greater than `timeLim` seconds when an answer is accepted. It is checked after accepted answers only, never after bury, suspend, delete or undo. `timeLim` 0 never shows it.
- cards: the answers accepted live since the timebox started. Neither undo nor redo changes that number, and a redo is not an accepted answer for the trigger (as in Anki): answer, undo, redo counts 1.
- minutes: `timeLim` / 60 rounded to the nearest whole number, an exact half to the even neighbour (90 → 2, 150 → 2, 600 → 10, 20 → 0). It is the limit, not the measured time.
- Examples: 1 answer, `timeLim` 600 → "1 card studied in 10 minutes."; 25 answers, `timeLim` 60 → "25 cards studied in 1 minute.".
- Continue restarts the timebox from 0 with a card count of 0; Finish ends the session into the summary (R430).
- Decided 2026-10-27: when the crossing answer leaves nothing to present (finished or waiting), no sheet appears; the congratulations content or the summary follows as usual, and the timebox ends with the session.

## S8. Congratulations (R434, R436, R437, R439)

Shown in place of a card when the head is empty (`Finished` or `WaitingUntil`), and passed to the summary (R439). The facts come from `sched/counts#C7` (`DeckStudyState`) for the session's deck at the instant shown. Paragraphs appear in this order:

| Key | English | Shown when |
|---|---|---|
| `review.congrats.title` | Congratulations! You have finished this deck for now. | always |
| `review.congrats.nextLearn.minutes`, `.hours` | `The next learning card will be ready in %lld minute.` / `… minutes.`; `… %lld hour.` / `… hours.` | `next_learning_ready_at` is present and after now |
| `review.congrats.learnRemaining` | one: `There is one remaining learning card due later today.` other: `There are %lld learning cards due later today.` | exactly when the line above is shown, in the same paragraph after a space; the count of `learning_later_today` |
| `review.congrats.reviewLimit` | Today’s review limit has been reached, but there are still cards waiting to be reviewed. For optimum memory, consider increasing the daily limit in the options. | `review_limit_reached` |
| `review.congrats.newLimit` | There are more new cards available, but the daily limit has been reached. You can increase the limit in the options, but please keep in mind that the more new cards you introduce, the higher your short-term review workload will become. | `new_limit_reached` |
| `review.congrats.buried` | `One or more cards were buried, and will be shown tomorrow. You can %@ if you wish to see them immediately.` | `sibling_buried` + `user_buried` > 0 |
| `review.congrats.unburyLink` | unbury them | the `%@` of the line above, shown as a link that starts S9; also the paragraph's accessibility action |
| `review.congrats.continue` | Continue | R437's button, once `next_learning_ready_at` is at or before now |
| (deck description) | the deck's own text, rendered as U155 does | the description is not empty after rendering |

- The ready-in amount: s = max(60, `next_learning_ready_at` − now) in seconds, as Anki clamps it. Below 3,600 the unit is minutes, below 86,400 hours; at 86,400 or more neither line shows. The amount is s divided by the unit's seconds, rounded to the nearest whole number with halves up. Examples: 600 → "10 minutes"; 90 → "2 minutes"; 3,599 → "60 minutes"; 5,400 → "2 hours". With a 1,200 s learn-ahead, a card due in 25 minutes is "ready in 5 minutes".
- The amount is recomputed at least once a minute while shown, by a system-driven schedule, never an app timer (R437, R1854). When `next_learning_ready_at` reaches now, the ready-in paragraph goes (both its sentences) and Continue appears; review never resumes on its own.
- Decided 2026-10-27: Meguri reads C7's native members (`next_learning_ready_at`, `learning_later_today`, `review_limit_reached`, `new_limit_reached`). Anki's page reads the compatibility members (`secs_until_next_learn`, `learning_remaining`, `review_remaining`, `new_remaining`). Those use C7's full named subtree, filtered descendants included, and coarser meanings (every queue-1 card, any due review card, any new card) where the native members use the studied subtree and "excluded by a daily limit", so Meguri's notices can differ from Anki's beyond C7's boundary and imported rows (S11).
- There is no custom-study paragraph or link (R436). The limit paragraphs carry no link here (R78's Deck Options links are the deck page's, U155).
- The content is computed again on rollover and on any change while shown (R438).

## S9. Unbury choices (R435)

| Key | English |
|---|---|
| `review.unbury.title` | What would you like to unbury? |
| `review.unbury.manual` | Manually buried cards |
| `review.unbury.siblings` | Buried siblings |
| `review.unbury.all` | All buried cards |

- When both `sibling_buried` and `user_buried` exceed 0, the link opens a confirmation dialog with the title and the three choices in this order, plus the system Cancel. They run `sched/manual-ops#M5` with mode manual, siblings or all. With one kind only, the link unburies all at once, with no dialog.
- Decided 2026-10-27: one chooser everywhere. The deck page's Unbury (M5, R635) uses these keys and this interaction too; its labels are M5's, spelled as here (sentence case, as R435 and R635 quote them; Anki writes title case). M5's preselection is the dialog's default choice: manual is the default button (Return on the Mac).

## S10. Changes made elsewhere (R456)

| Key | English | Shown when |
|---|---|---|
| `review.toast.changedElsewhere` | This card was changed elsewhere | the session reports `CurrentCardChanged{removed}` (`sched/review-session#C13`) for a change the review screen did not make |

- Not shown for the review screen's own ops (bury, suspend, flag, mark, Delete Note, set due date, forget, an edit in the edit panel; `sched/review-session#C13`, with C13.1 for Mark and Delete Note), for `{eligible}` or `{memoryOnly}`, or when the card leaves through a rollover. This wording is Meguri's (no Anki text).

## S11. Differences from Anki

- Anki 26.09.3 passes the timebox's minutes as text, so its plural form never matches: it says "1 minutes." Meguri uses the plural rule (S7).
- Anki shows the timebox prompt as a modal dialog before the next card; Meguri shows a non-blocking sheet over it (R429, R430).
- Anki shows the timebox dialog even when the crossing answer leaves nothing to present; Meguri shows no sheet then (S7).
- Anki shows undo and redo as tooltips; Meguri announces them only (S5).
- Delete Note says "Note deleted." with Undo (R387); Anki counts the cards and offers no Undo in the toast (S4). Apostrophes are typographic (S1).
- The unbury choices are in sentence case (R435, R635); Anki writes them in title case (S9).
- Decided 2026-10-27: the new-card limit notice uses "keep in mind", a small rewording of Anki's sentence (S8).
- The congratulations facts are C7's native members (S8): studied subtree and daily-limit meanings, where Anki's compatibility members use the full named subtree and "some due review / new / queue-1 card exists", so the notices can differ beyond C7's boundary and imported rows. There is no custom-study line (R436).

## S12. Tests and scenarios

The oracle runs the collection library, not Anki's UI, so the text itself is checked by Meguri's tests: `ReviewToastsTests` (every key of S3 to S6 and S10 with its trigger and its absence), `TimeboxControllerTests` (S7 examples and the half-to-even rule), `CongratsModelTests` (S8 examples, each paragraph's condition, the 86,400 cut-off, the Continue switch) and the S9 chooser cases. Every key must exist in the catalog with exactly this English value (U167's string lint). The counts behind the strings are collection facts; their oracle scenarios (bury counts, the timebox's card count across undo and redo, the C7 ready-in boundaries) are not recorded yet.

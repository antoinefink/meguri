# Reviewer strings

Spec: review/reviewer-strings
Requirements: R78, R86, R184, R185, R241, R339, R349, R351, R362, R363, R364, R365, R366, R370, R374, R376, R378, R382, R383, R384, R386, R387, R429, R430, R432, R433, R434, R435, R436, R437, R439, R456, R462, R581, R635, R724, R728, R1263, R1981
Anki: 26.09.3 for S3 to S9 (the wording, plural forms and triggers of Anki desktop's reviewer, congratulations page and unbury dialog, except the differences S11 lists) and for the English values only of S13 and S14 (the reviewer's answer bar and the Forget dialog's two toggles); none for S8.1's link, S10, S15 to S19, the keys, S13's spoken forms and where S13 and S14 show their strings
Scenarios: conformance/scenarios/review/ (none recorded yet, S12)

The text the review screen shows besides the card: toasts, the timebox prompt, Auto Advance reminders, the congratulations notices, the unbury choices, the grade names and Show Answer, the Forget choices, the notices of a refused or failed answer, the reasons undo is unavailable, the menu titles, and the titles and labels of the dialogs and panels over the review. U175, U177, U178, U179, U182, U183, U185, U186 and U187 take every such string only from this file (KTD1). U155's finished state (`decks/deck-list#H5.2`, S8.1) and U157's summary reuse S8 and S9 for the same facts. U153's toasts and U159's Edit menu take their undo and redo titles from S5.

## S1. Keys, arguments and plurals

- Every string is an entry of the Review String Catalog with the stable key given here. The English value is the one given; other languages are Meguri's own translations of the English. Keys never change once shipped; a changed meaning gets a new key.
- `%lld` is an integer argument, `%@` a text argument; with two arguments they are numbered (`%1$lld`, `%2$lld`). A plural entry has the catalog's plural variations on its integer argument: English uses **one** for exactly 1 and **other** for every other value, 0 included; other languages follow their CLDR rules. An entry with two integer arguments pluralizes each one separately. Every translation includes the integer argument in every plural form; the only exception is the English **one** form of `review.congrats.learnRemaining` (S8), because English uses **one** only for 1. A language whose **one** form also covers other numbers (Russian 21, French 0) shows the number.
- Text is shown as given, with the trailing period where it has one. Decided 2026-10-27: apostrophes are typographic (’), the platform's typography; Anki writes ASCII `'`. Every other difference from Anki's English is listed in S11.

## S2. Toasts (R581)

- A toast is non-blocking: it never takes taps on the card or the buttons, it hides after about 2 seconds, and VoiceOver announces its text when it appears. Decided 2026-10-27: one toast at a time; a new toast replaces the one on screen.
- A toast follows the op's success. An op that fails or is refused shows no toast; its error path applies (R362). For an answer, that error path's text is S15.
- An op's undo or redo never shows the op's toast again (S5).

## S3. After an answer (R363)

No toast follows an accepted answer, except the leech toast. A refused or failed answer shows S15's notice instead.

| Key | English | Shown when |
|---|---|---|
| `review.toast.leech` | Card was a leech. | C9's `Accepted` result (`sched/review-session#C9`) has `leech` set and `suspended` not set (leech action Tag Only) |
| `review.toast.leechSuspended` | Card was a leech. It has been suspended. | `Accepted` has both `leech` and `suspended` set (leech action Suspend Card) |

- `suspended` is the outcome's flag (`sched/leeches#L5`: the answer set `queue` −1 through the leech action), never read from the preset. The suffix is Meguri's behaviour from R363 on that outcome; Anki 26.09.3 desktop does not show it (S11).
- The catalog holds these two keys only, each translated as one whole sentence. `sched/leeches#L5`'s ids `leech-notice` and `leech-notice-suspended` are not catalog keys and nothing joins strings at run time: L5's two texts joined with a space are the English of `review.toast.leechSuspended`, and this table is the one source of the text.
- Not shown again when the answer is redone (S2).
- When the answer also reaches the timebox (S7), the toast shows together with the sheet.
- Changed 2026-11-23 (leech icon). How the leech notice shows on screen is `docs/design/native-interface-contract.md` N16.11: it is one yellow warning icon first in the review's top controls, not a banner, and a press shows `review.leech.title` with `review.leech.tagged` or `review.leech.suspended`, chosen by the same `suspended` flag. VoiceOver announces this table's sentence once when the icon appears (S2). The keys, their English and when each applies are unchanged. Decided by Claude (coordinator) under Antoine's delegation, pending his look.

## S4. Card actions in review (R382, R383, R387)

| Key | English (one / other) | Argument | Shown after |
|---|---|---|---|
| `review.toast.cardsBuried` | `%lld card buried.` / `%lld cards buried.` | cards the op wrote | Bury Note and Auto Advance's Bury Card answer action (not Bury Card, below) |
| `review.toast.cardSuspended` | Card suspended. | | none in this version (Suspend Card shows no toast, below) |
| `review.toast.noteSuspended` | Note suspended. | | Suspend Note |
| `review.toast.noteDeleted` | Note deleted. | | Delete Note, with the action below |
| `review.toast.undo` | Undo | | the action button of `review.toast.noteDeleted`; it undoes that Delete Note only (rule below, U179) |
| `review.toast.dueDateSet` | `Set due date of %lld card.` / `Set due date of %lld cards.` | cards given to the op | Set Due Date… (U183) |
| `review.toast.cardsReset` | `Reset %lld card.` / `Reset %lld cards.` | cards given to the op | Forget… (U183) |

- Decided 2026-11-20 (round 7, `docs/design/native-interface-contract.md` N20.8): Bury Card and Suspend Card show no toast. VoiceOver announces `review.announce.cardBuried` "Card Buried" or `review.announce.cardSuspended` "Card Suspended" instead. `review.toast.cardsBuried` and `review.toast.cardSuspended` stay listed for the later Bury Note and Auto Advance, whose units decide again.
- The bury count is the op's count of cards it wrote (`sched/manual-ops#M2`, M9): Bury Note on a note with 3 cards of which one is suspended says "2 cards buried."; Auto Advance's Bury Card answer action says "1 card buried.".
- The suspend toasts carry no count: "Note suspended." also for a note whose other cards were already suspended.
- The delete toast carries no count, whatever the number of cards removed with the note (R387; Anki says "3 cards deleted." for a note with 3 cards). VoiceOver announces it with its Undo action.
- Decided 2026-10-27: the toast's Undo is offered only while that Delete Note is the session's newest undo step. When the newest step changes while the toast is shown (any step is recorded, such as an answer, an Auto Advance action, a bury, Flag or Mark, or an undo or redo runs, including the back button's), the toast closes at once. A tap on Undo first reads `peek(Session)` (`engine/undo`): it runs one `undo(Session)` (U179) only when the top step is `delete_note` and no step was recorded or undone since the toast appeared; otherwise it does nothing and the toast closes. The toast never runs an undo of another step.
- Decided 2026-10-27: while VoiceOver runs, this toast does not hide after 2 seconds (S2): it stays until its Undo is used or the newest undo step changes (above), so its action can be reached. The back button (`Undo Delete Note`, S5) is the other way. Toasts without an action keep S2's timing.
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

- **Display names** (decided 2026-11-20, round 7, N20.8): where a review step has one, the named forms use it instead of the V4 name: `undo.display.answer_card` "Answer", `undo.display.bury` "Bury Card", `undo.display.suspend` "Suspend Card", `undo.display.update_note` "Edit Note". So the review's Undo reads "Undo Answer", "Undo Bury Card", "Undo Suspend Card" and "Undo Edit Note", and VoiceOver hears "Answer undone". Steps with no display name keep their V4 name ("Undo Rename Deck", "Undo Delete Deck"). The bury and suspend names hold while the review offers only the card forms; Bury Note and Suspend Note, when they come, decide their own.
- The step name is the step's display name in review where it has one (above), and otherwise `undo.step.<id>` for the step's V4 id (`undo.step.answer_card` = "Answer Card", `undo.step.bury` = "Bury", `undo.step.delete_note` = "Delete Note"). The named forms label the back button, its Mac tooltip and the Undo and Redo items; the plain forms are used when there is no step.
- Decided 2026-11-10: these forms are the app's only undo and redo titles, outside review too. The Edit menu's Undo and Redo items (U159, R173) use `review.undo.named` and `review.redo.named` with the `undo.step.<id>` name of the step that `peek(Global)` or `peek_redo(Global)` reports (`engine/undo`; "Undo Delete Deck"), and `review.undo.plain` and `review.redo.plain`, disabled, when there is none. The action name that U159 gives the system undo manager (the iPhone and iPad undo gestures) is the `undo.step.<id>` text alone; the system adds its own Undo and Redo words to it. The Undo button of a U153 toast is `review.undo.plain`. The `review.` prefix is part of the key, not a location: the String Catalog that holds this section and the `undo.step.<id>` names must be one that U153 and U159 can read as well as review.
- After an undo or redo in review, VoiceOver hears `review.announce.undone` or `review.announce.redone` with the step's name, its display name where it has one ("Answer undone", "Edit Note undone"; above). Decided 2026-10-27: no visible toast; the card coming back is the visible feedback (Anki shows the same text as a tooltip). R581's toast list does not include undo.

## S6. Auto Advance (R432, R433)

| Key | English | Shown when |
|---|---|---|
| `review.toast.autoAdvanceOn` | Auto Advance enabled | the user turns Auto Advance on (⇧A or the ⋯ item) |
| `review.toast.autoAdvanceOff` | Auto Advance disabled | the user turns it off |
| `review.toast.questionTimeElapsed` | Question time elapsed | the question countdown ends and the question action is Show Reminder |
| `review.toast.answerTimeElapsed` | Answer time elapsed | the answer countdown ends and the answer action is Show Reminder |
| `review.autoAdvance.unavailable` | Auto Advance needs a number of seconds to show the question or the answer in the deck options of this card. | explanation of the disabled toggle, and ⇧A's toast while it is disabled (rule below; R433; Meguri's wording) |

- Decided 2026-10-27: the current card's preset decides, as for every other Auto Advance value (R432). A time (`secondsToShowQuestion`, `secondsToShowAnswer`) is set when it is a finite number above 0. A stored 0, negative or NaN time (`sched/presets` keeps them as stored) is not set, and that countdown does not run. While Auto Advance is off and neither time of the current card's preset is set, the ⋯ item is disabled with `review.autoAdvance.unavailable` as its explanation, and ⇧A shows that text as a toast and changes nothing. While Auto Advance is on, the item stays enabled so that it can be turned off; a card whose preset sets neither time runs no countdown and shows no toast, and the next card with a set time counts down again. Anki never disables the toggle (S11).
- Not shown: when Auto Advance turns itself off because the app left the foreground or the Mac app deactivated (R433); the ⋯ item's checked state shows it.
- A reminder shows once per countdown: audio that ends later on that side shows nothing more (`review/auto-advance#A6`). This is Meguri's rule; Anki shows the reminder again each time audio ends on that side after its countdown has run out (S11).

## S7. Timebox (R86, R429, R430)

| Key | English | Arguments |
|---|---|---|
| `review.timebox.message` | each argument with its own plural, cards one / other × minutes one / other: `%1$lld card studied in %2$lld minute.` / `%1$lld card studied in %2$lld minutes.` / `%1$lld cards studied in %2$lld minute.` / `%1$lld cards studied in %2$lld minutes.` | cards, minutes |
| `review.timebox.continue` | Continue | |
| `review.timebox.finish` | Finish | |

- Trigger: `timeLim` > 0, and the active time since the timebox started (R431) is greater than `timeLim` seconds when an answer is accepted. It is checked after accepted answers only, never after bury, suspend, delete or undo. `timeLim` 0 never shows it.
- `timeLim` is read with the u32 read of `review/review-preferences` P2 (a stored `600.0` reads 0, no timebox; Anki's check reads the raw value, P7).
- The timebox clock starts when the session starts and restarts as below, whatever `timeLim` holds; with 0 only the check is off. A change of `timeLim` reported while a session is open replaces the limit for the following checks without resetting the timebox time or the card count, so turning the limit on mid-session can show the sheet at the next accepted answer (`review/review-preferences` P3).
- cards: the answers accepted live since the timebox started. Neither undo nor redo changes that number, and a redo is not an accepted answer for the trigger (as in Anki): answer, undo, redo counts 1.
- minutes: `timeLim` / 60 rounded to the nearest whole number, an exact half to the even neighbour (90 → 2, 150 → 2, 600 → 10, 20 → 0). It is the limit, not the measured time.
- Examples: 1 answer, `timeLim` 600 → "1 card studied in 10 minutes."; 25 answers, `timeLim` 60 → "25 cards studied in 1 minute.".
- Continue restarts the timebox from 0 with a card count of 0; Finish ends the session into the summary (R430).
- Decided 2026-10-27: when the crossing answer leaves nothing to present (head `Finished` or `WaitingUntil`), no sheet appears; the congratulations content or the summary follows as usual, and the timebox restarts from 0 with a card count of 0 at that answer, as Continue does.
- Decided 2026-10-27: time while no card is presented (the congratulations content, the summary) is not timebox time. The timebox's clock stops there and goes on from its value when a card is presented again: the congratulations content's Continue (R437, S8), an unbury there (R435), or an undo or unbury from the congratulations content or from a summary that the queue running out or ✕ opened. The summary's own Continue is not such a resume: it starts a new session on the same deck (U157), whose timebox starts from 0 (`review/timers#T3`). A resume from the summary after Finish, that is an Undo or Unbury on the summary that the timebox sheet's Finish opened, starts from 0 with a card count of 0 (`review/timers#T3`).
- Decided 2026-10-27: an undo while the sheet is shown closes the sheet and leaves the elapsed time and the card count unchanged (the count rule above), so the next accepted answer shows the sheet again. Esc on the Mac and swiping the sheet down count as Continue.
- Example: `timeLim` 600; the answer at 610 s leaves only a learning card due in 10 minutes, so no sheet appears and the timebox restarts; 10 minutes on the congratulations content add nothing; after Continue the timebox goes on from 0 s and 0 cards.

## S8. Congratulations (R434, R436, R437, R439)

Shown in place of a card when the applied head's `terminalState` (`sched/review-session#C17.1`) is `Finished` or `WaitingUntil`, and passed to the summary (R439). The head alone decides the phase, the ready-in paragraph, its instant and Continue. The other facts come from `sched/counts#C7` (`DeckStudyState`) for the session's deck, read once the session's accepted answers have committed (C11), so that they include those answers. Paragraphs appear in this order:

| Key | English | Shown when |
|---|---|---|
| `review.congrats.title` | Congratulations! You have finished this deck for now. | always |
| `review.congrats.nextLearn.minutes` | `The next learning card will be ready in %lld minute.` / `The next learning card will be ready in %lld minutes.` | the head is `WaitingUntil{instant}`, the instant is after now, and the amount's unit is minutes |
| `review.congrats.nextLearn.hours` | `The next learning card will be ready in %lld hour.` / `The next learning card will be ready in %lld hours.` | the same, and the unit is hours |
| `review.congrats.learnRemaining` | one: `There is one remaining learning card due later today.` other: `There are %lld learning cards due later today.` | exactly when the line above is shown, in the same paragraph after a space; the count of `learning_later_today` |
| `review.congrats.reviewLimit` | Today’s review limit has been reached, but there are still cards waiting to be reviewed. For optimum memory, consider increasing the daily limit in the options. | `review_limit_reached` |
| `review.congrats.newLimit` | There are more new cards available, but the daily limit has been reached. You can increase the limit in the options, but please keep in mind that the more new cards you introduce, the higher your short-term review workload will become. | `new_limit_reached` |
| `review.congrats.buried` | `One or more cards were buried, and will be shown tomorrow. You can %@ if you wish to see them immediately.` | `sibling_buried` + `user_buried` > 0 |
| `review.congrats.unburyLink` | unbury them | the `%@` of the line above, shown as a link that starts S9; also the paragraph's accessibility action |
| `review.congrats.continue` | Continue | R437's button: the head is `WaitingUntil` and its instant is at or before now, or the applied head is `Active` (below) |
| (deck description) | the deck's own text, rendered as U155 does | the description is not empty after rendering |

- The ready-in amount: s = max(60, floor((instant − now) / 1,000)) whole seconds, where instant is the head's `WaitingUntil` instant and both values are in milliseconds; the clamp is Anki's. Below 3,600 the unit is minutes, below 86,400 hours; at 86,400 or more neither line shows. The amount is s divided by the unit's seconds, rounded to the nearest whole number with halves up. Examples: 600 s → "10 minutes"; 90,000 ms → 90 → "2 minutes"; 89,999 ms → 89 → "1 minute"; 3,599 s → "60 minutes"; 5,400 s → "2 hours". With a 1,200 s learn-ahead, a card due in 25 minutes is "ready in 5 minutes".
- The sentence is the catalog string, redrawn by a system-driven schedule (a timeline with an entry at each instant the shown amount changes and one at the ready instant), never an app timer (R437, R1854). A timer-interval countdown view (`Text(timerInterval:)`, a clock such as 4:59) is not used; this overrides U186's approach. When the instant is reached, the ready-in paragraph goes (both its sentences) and Continue appears.
- Decided 2026-10-27: the screen follows the applied head (C17.1). `WaitingUntil` shows the ready-in paragraph and later Continue; `Finished` shows neither, whatever C7 reports. Until C7 has been read after the commit, the ready-in sentence shows alone and the C7 paragraphs follow when it is read. When the head turns `Active` while this content is shown (at readiness, or with cards another change made available), the content stays, Continue appears, and the head's item 0 is presented only when Continue is tapped; review never resumes on its own (R437). Continue tapped before the refreshed head is `Active` refetches the head and presents its item 0, or keeps this content while the head is still empty. The one exception is the screen's own unbury (S9): its result head is presented at once when it is `Active` (R435).
- Decided 2026-10-27: Meguri reads C7's native members (`learning_later_today`, `review_limit_reached`, `new_limit_reached`); on the review screen the ready instant is the head's, never C7's `next_learning_ready_at` (the deck page, which has no head, uses it: S8.1). Anki's page reads the compatibility members (`secs_until_next_learn`, `learning_remaining`, `review_remaining`, `new_remaining`). Those use C7's full named subtree, filtered descendants included, and coarser meanings (every queue-1 card, any due review card, any new card) where the native members use the studied subtree and "excluded by a daily limit", so Meguri's notices can differ from Anki's beyond C7's boundary and imported rows (S11).
- There is no custom-study paragraph or link (R436). The limit paragraphs carry no link on the review screen; R78's Deck Options links are the deck page's (S8.1).
- The content is computed again on rollover, on any change while shown (R438) and when the session's own pending answers commit.
- The English **one** form of `review.congrats.learnRemaining` intentionally has no `%lld` (S1).

### S8.1 The deck page (R78, U155)

U155's finished state (`decks/deck-list#H5.1`, H5.2) shows S8's paragraphs, keys and order, with two differences, because the deck page has no session head:

- The ready-in instant is C7's `next_learning_ready_at` from the deck's `DeckStudyState` record, with S8's amount and redraw rules. S8's rule that the instant is the head's, never `next_learning_ready_at`, is the review screen's.
- `review.congrats.continue` is not shown; the page returns to its normal state instead (`decks/deck-list#H5.3`).

| Key | English | Shown when |
|---|---|---|
| `review.congrats.deckOptionsLink` | Deck Options | on the deck page only, after `review.congrats.reviewLimit` and after `review.congrats.newLimit` while the `.deckOptions` route is attached (R78, `decks/deck-list#H5.2`); a link that opens the deck's Deck Options |

- The link's text is Meguri's wording: Anki's congratulations page has no Deck Options link.

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
| `review.toast.changedElsewhere` | This card was changed elsewhere | the screen applies `CurrentCardChanged{removed}` (`sched/review-session#C13`) |

- Decided 2026-10-27: the screen keys the toast on the event alone and guesses nothing from in-flight ops. It relies on this core contract: an op submitted with the session id (every op of C13's list: bury, suspend, flag, forget, set due date, and C13.1's Mark and Delete Note) emits no `CurrentCardChanged` for its own commit and reports the change only in its result's head diff, as C13.1 states for Mark and Delete Note; a rollover that drops the current card (C14, R600) reports it through the rebuilt head (`QueueHeadChanged`, or the `Accepted` result with `rolledOver`), never as `CurrentCardChanged{removed}`. `sched/review-session#C13` and C14 do not say so for every op yet; this section is the contract until they do.
- Decided 2026-11-10: a save in the edit panel is not one of those ops. It is the editor's note update, which names no session (`review/edit-current#E2`, `editor/field-html`), so the screen meets it as a change made elsewhere: `{eligible}` or `{memoryOnly}`, never `{removed}` (E2), so no toast.
- So it is not shown for the review screen's own ops, for `{eligible}` or `{memoryOnly}`, or when the card leaves through a rollover. This wording is Meguri's (no Anki text).

## S11. Differences from Anki

- Anki 26.09.3 passes the timebox's minutes as text, so its plural form never matches: it says "1 minutes." Meguri uses the plural rule (S7).
- Anki shows the timebox prompt as a modal dialog before the next card; Meguri shows a non-blocking sheet over it (R429, R430).
- Anki shows the timebox dialog even when the crossing answer leaves nothing to present; Meguri shows no sheet then (S7).
- Anki shows undo and redo as tooltips; Meguri announces them only (S5).
- Delete Note says "Note deleted." with Undo (R387); Anki counts the cards and offers no Undo in the toast (S4). Apostrophes are typographic (S1).
- The unbury choices are in sentence case (R435, R635); Anki writes them in title case (S9).
- Decided 2026-10-27: the new-card limit notice uses "keep in mind", a small rewording of Anki's sentence (S8).
- The congratulations facts are C7's native members (S8): studied subtree and daily-limit meanings, where Anki's compatibility members use the full named subtree and "some due review / new / queue-1 card exists", so the notices can differ beyond C7's boundary and imported rows. There is no custom-study line (R436).
- Anki 26.09.3 desktop reads the card from before the answer when it shows the leech notice, so without add-ons it never adds "It has been suspended."; Meguri adds it when the outcome leaves the card suspended (R363, S3). Anki also shows the leech notice only after its modal timebox dialog closes; Meguri shows the toast together with the sheet (S3).
- Anki never disables the Auto Advance toggle: with both times 0 it still says "Auto Advance enabled". Meguri disables it from the current card's preset with an explanation (R433, S6).
- Anki's timebox measures wall-clock time since it started; Meguri's counts active time only, without background time (R431) or time with no card presented, and restarts at a crossing that shows no sheet (S7).
- Anki 26.09.3 titles the dialog behind Forget… "Reset Card"; the review screen's dialog is the Forget dialog, and its title is Meguri's wording (S14). Only its two toggle labels are Anki's.
- Anki's reviewer has no Show Answer accessibility action, no Magic Tap and no spoken answer labels; Meguri adds them (R349, R185, S13). Anki's grade names and Show Answer are the same words.
- Anki's congratulations page clamps the ready-in amount to at least 60 seconds, so once the card is ready it still says "ready in 1 minute" until the page refreshes, and the page has no Continue button. Meguri removes the paragraph at the ready instant and shows Continue (R437, S8).
- Anki's congratulations page has no Deck Options link; Meguri's deck page adds one after each limit paragraph (R78, S8.1).
- Anki shows an Auto Advance reminder again each time audio ends on that side after its countdown has run out; Meguri shows it once per countdown (S6, `review/auto-advance#A9`).
- Anki raises an error when an answer meets a card that changed since it was shown; Meguri rebuilds silently and shows "Card changed, please answer again" while the card is still presented (R728, S15).
- Anki's More menu titles differ from R374's, which Meguri uses (S18): Anki says Flag Card, Reset Card..., Set Due Date..., Options, Create Copy..., Audio -5s and Audio +5s, and its bottom bar says Edit.
- Anki titles its deck options window "Options for" and the deck's name; Meguri's sheet from review says `Deck Options for %@` (S19). Anki's Set Due Date prompt and hint are not used (S19).

## S12. Tests and scenarios

The oracle runs the collection library, not Anki's UI, so the text itself is checked by Meguri's tests, each in a file its unit already lists:

| Test file (unit) | Covers |
|---|---|
| `AnswerPathTests` (U175) | S3: both leech keys from `Accepted.leech` and `Accepted.suspended`, none after a redo, the toast together with the timebox sheet |
| `CardActionPerformerTests` (U182) | S4's keys, counts and absences; the delete toast closing when another step is recorded or undone before Undo, a late Undo tap doing nothing, and the toast staying while VoiceOver runs |
| `ReviewUndoControllerTests` (U179) | S5's labels and announcements, with no toast; S16: every `engine/undo#M3` reason maps to its key and a non-empty text, and the disabled Undo item's hint is the text of `peek(Session)`'s reason |
| `UndoStoreTests` (U159) | S5's named Edit menu titles for a Global step, the disabled plain titles with no step, and the step name alone as the system action name |
| `AutoAdvanceControllerTests` (U185) | S6: the toasts, the toggle disabled from the current card's preset (0, negative and NaN times), ⇧A while disabled, a 0/0 card while Auto Advance is on |
| `TimeboxControllerTests` (U185) | S7: the examples, the four message forms, the half-to-even rule, the restart at a crossing with no sheet, no timebox time while no card is presented, undo while the sheet is shown, Esc as Continue |
| `CongratsModelTests` (U186) | S8: the examples (89,999 ms included), each paragraph's condition, the 86,400 cut-off, the Continue switch from the head, `Finished` with no ready-in line or Continue, and C17.1's two regressions (Easy on the last learning card inside learn-ahead; Again with a 60-minute step) while a committed C7 query disagrees; S9's chooser cases |
| `ExternalChangeHandlerTests` (U187) | S10: the toast for each applied `{removed}`, none for `{eligible}`, `{memoryOnly}` or a rollover's rebuilt head |
| `AnswerButtonsTests` (U178) | S13: the four grade names in their fixed order; Show Answer as the button with and without an assistive technology, as the menu command and as the accessibility action; the interval taken from `sched/answer-labels` and never from the catalog; the accessibility label examples, `.one` only for the amount `1` (`1.5` uses `.other`), `less than` for a `<` label, `(end)` spoken as it is; the Voice Control input labels |
| `ForgetDialogModelTests` (U183) | S14: both labels, their order, and each toggle's starting value from its config key; S19's Forget title and button |
| `AnswerPathTests` (U175) | S15: `review.toast.cardChanged` for `Stale` and `NotAtHead` while the refused card is presented, and for a `CommitFailed` whose error is the `Stale` precondition; none when the card is no longer presented; `review.banner.commitFailed` for every other `CommitFailed` |
| `OnScreenTimerTests` (U177) | removed 2026-11-19 with S17; the string lint checks that `review.timer.capped` is absent from the catalog |
| `ReviewMoreMenuTests` (U182) | S18: every title, in R374's order, the same in the ⋯ menu, the context menu and the Mac Card menu; the U+2026 and U+2212 characters |
| `SetDueDateDialogModelTests` (U183) | S19: the Set Due Date title, prompt, hint and button; `Malformed` and `TooLarge` mapped to their messages, no message for `Empty` |
| `ReviewPanelHostTests` (U183) | S19: the Current / Previous labels and the Deck Options title with the home deck's name (a filtered deck's card names its original deck) |
| `DeckPageStoreTests` (U155) | S8.1: the ready-in line from `next_learning_ready_at`, no Continue, the Deck Options link after each limit paragraph only while the route is attached |

Every key must exist in the catalog with exactly this English value (U167's string lint). This check belongs to the unit that adds the catalog, runs in the `tools/mg lint-ui` job with U167's `lint-strings`, and starts when the catalog exists (`text/interface-text` I3). The counts behind the strings are collection facts; their oracle scenarios (bury counts, the timebox's card count across undo and redo, the C7 ready-in boundaries) are not recorded yet.

## S13. Answer controls (R184, R185, R339, R349, R351, R1981)

| Key | English | Used for |
|---|---|---|
| `review.answer.again` | Again | grade name of the first answer button and of its menu command |
| `review.answer.hard` | Hard | the same, second |
| `review.answer.good` | Good | the same, third |
| `review.answer.easy` | Easy | the same, fourth |
| `review.showAnswer` | Show Answer | the question side's button, on every platform and whether or not an assistive technology is on (`docs/design/native-interface-contract.md` N11.2, which replaces R184's limit); the Show Answer menu command (N11.3); the card's accessibility action, which VoiceOver's Magic Tap also performs (R349) |
| `review.answer.accessibilityLabel` | `%1$@, %2$@` | an answer button's accessibility label: the grade name, then the spoken interval (R185: "Good, 3 days") |
| `review.answer.lessThan` | `less than %@` | the spoken interval of a label that starts with `<` (`sched/answer-labels` A5); `%@` is the rest of the spoken interval |

Spoken units. Each row is two plain entries, `<stem>.one` and `<stem>.other`, whose `%@` is the label's amount as text (`sched/answer-labels` A3: `10`, `1.5`, `2.3`). The `.one` entry is used when the amount is exactly `1`, the `.other` entry for every other amount. These are not catalog plurals (S1), because the amount can have a decimal digit.

| Key stem | Unit (`sched/answer-labels` A2) | `.one` | `.other` |
|---|---|---|---|
| `review.answer.unit.seconds` | seconds | `%@ second` | `%@ seconds` |
| `review.answer.unit.minutes` | minutes | `%@ minute` | `%@ minutes` |
| `review.answer.unit.hours` | hours | `%@ hour` | `%@ hours` |
| `review.answer.unit.days` | days | `%@ day` | `%@ days` |
| `review.answer.unit.months` | months | `%@ month` | `%@ months` |
| `review.answer.unit.years` | years | `%@ year` | `%@ years` |

- The answers are in the order Again, Hard, Good, Easy (R351). Where the name and the interval sit, and the button sizes and styles, are the design contract's (N11.2, N12.3 on the Mac, N13.3 on touch); this file gives only their text.
- The visible interval is `sched/answer-labels`' string, shown as it is and never a catalog entry. The spoken interval is built from the same label (its `<` prefix, amount and unit, A3 to A5), never from a second computation such as the interval's seconds (this overrides U178's approach), so it never disagrees with the visible one: `<10m` is "less than 10 minutes", `1d` is "1 day", `1.5h` is "1.5 hours", `2.3mo` is "2.3 months". The end label `(end)` (A5) is spoken as that text.
- Examples of accessibility labels: "Again, less than 10 minutes", "Hard, 1 minute", "Good, 3 days", "Easy, 2.3 months".
- Voice Control's input label of each answer button is its grade name alone, and of the Show Answer button `review.showAnswer`, so the spoken commands are "Tap Good" and "Tap Show Answer" (R185). The accessible answer controls that replace the tap zones (R1981) are these four buttons, with the same labels.
- The menu commands' titles are the grade names and `review.showAnswer` (N11.3, R339), with no interval.
- Until the plan settles R131 versus R354 (`review/review-preferences` P4.1), interval labels are shown whatever `estTimes` holds (R354) and nothing reads the key. P4.1 states the R131 branch's labels; this section changes only once the plan chooses it.

## S14. Forget choices (R386)

| Key | English | Starts from |
|---|---|---|
| `review.forget.restorePosition` | Restore original position where possible | `restorePositionReviewer`, default true (`sched/manual-ops#M6.4`) |
| `review.forget.resetCounts` | Reset repetition and lapse counts | `resetCountsReviewer`, default false |

- These are the two toggles of the Forget dialog that review's Forget… command opens (U183, R386), in this order, for the two choices of `sched/manual-ops#M6.1`; the op stores both values (M6.4).
- The dialog's title, its buttons and the whole Set Due Date dialog (prompt, hint, validation messages) are Meguri's wording, listed in S19: R384 and R386 quote no other Anki text. Their results are S4's toasts.

## S15. Refused and failed answers (R362, R728)

Meguri's wording; Anki has neither notice (S11).

| Key | English | Shown when |
|---|---|---|
| `review.toast.cardChanged` | Card changed, please answer again | the answer is refused with `Stale` or `NotAtHead` (`sched/review-session#C10`) and the refused card is still presented, with its fresh states and labels (R728); also for a `CommitFailed` whose error is the `Stale` precondition (C11) when the card is presented again |
| `review.banner.commitFailed` | Your answer couldn’t be saved. Answer this card again. | every other `CommitFailed(cardId, error)` (C11): the failed card is presented again on its question side (R362) |

- `review.toast.cardChanged` is a toast (S2). Its English is R728's, word for word. When the refused card is no longer presented, nothing is shown; the screen takes the fresh head (C10).
- `review.banner.commitFailed` is the error banner of R362: VoiceOver announces it when it appears. Its place and how it is dismissed are the design contract's; this file gives its text only. It is one banner however many later answers C11 dropped with the failed one.

## S16. When undo is unavailable (R365, R366, R462)

| Key | English | `engine/undo#M3` reason |
|---|---|---|
| `review.undo.unavailable.none` | Nothing to undo in this review | `None` |
| `review.undo.unavailable.clearedByRollover` | Undo is unavailable after the day changed | `ClearedByRollover` |
| `review.undo.unavailable.clearedBySync` | Undo is unavailable after changes from another device were applied | `ClearedBySync` |
| `review.undo.unavailable.clearedByOneWay` | Undo is unavailable after the collection was replaced | `ClearedByOneWay` |
| `review.undo.unavailable.clearedByNonUndoableOp` | Undo is unavailable after a change that can’t be undone | `ClearedByNonUndoableOp` |
| `review.undo.unavailable.changedExternally` | Undo is unavailable after the collection was changed elsewhere | `ChangedExternally` |
| `review.undo.unavailable.alreadyUploaded` | Changes that were already synced can’t be undone | `AlreadyUploaded` |
| `review.undo.unavailable.uploadPending` | This change is being synced and can be undone again if the sync fails | `UploadPending` |

- The disabled touch Undo item gives the text of `peek(Session(id))`'s reason as its accessibility hint and help text (`review/answer-flow#F6.2`). Where the Mac shows it is F6.2's open point.
- `ClearedByRollover`'s text is R462's (AE13), word for word. The others are Meguri's wording; U179's plan text fixes only their meaning. None ends with a period, as R462's does not.
- Every M3 reason has a text. A reason added to M3 later needs a key here before the app can show it.
- Anki has no such texts: its Undo item is only disabled.

## S17. On-screen timer (Removed 2026-11-19)

Removed with `review/timers#T2` (Antoine, 2026-11-19): Meguri has no on-screen timer. The key `review.timer.capped` is withdrawn: it is not in the String Catalog, no view names it, and the key is not reused. S17 holds no other key.

## S18. Menu titles (R339, R374, R376, R378)

Meguri's wording, taken from R374, never from Anki's menus (`review/card-actions#A13`, S11). In R374's order:

| Key | English | Notes |
|---|---|---|
| `review.menu.editNote` | Edit Note | |
| (Redo) | S5's `review.redo.named` | iPhone ⋯ menu only, while a step can be redone (R724) |
| `review.menu.flag` | Flag | the submenu; its rows are the flag names of `tags/flags-and-marked#F4` |
| `review.menu.markNote` | Mark Note | checked while the note is marked (R376) |
| `review.menu.buryCard` | Bury Card | |
| `review.menu.buryNote` | Bury Note | |
| `review.menu.suspendCard` | Suspend Card | |
| `review.menu.suspendNote` | Suspend Note | |
| `review.menu.setDueDate` | Set Due Date… | |
| `review.menu.forget` | Forget… | |
| `review.menu.cardInfo` | Card Info | |
| `review.menu.previousCardInfo` | Previous Card Info | |
| `review.menu.templateErrors` | Template Errors… | only while shown (R378) |
| `review.menu.deckOptions` | Deck Options | |
| `review.menu.replayAudio` | Replay Audio | |
| `review.menu.pauseAudio` | Pause Audio | |
| `review.menu.audioBack` | Audio −5 s | |
| `review.menu.audioForward` | Audio +5 s | |
| `review.menu.autoAdvance` | Auto Advance | checked while on (R376) |
| `review.menu.cardZoom` | Card Zoom | the submenu of the next three, in this order |
| `review.menu.zoomOut` | Zoom Out | |
| `review.menu.resetZoom` | Reset Zoom | |
| `review.menu.zoomIn` | Zoom In | |
| `review.menu.createCopy` | Create Copy | |
| `review.menu.deleteNote` | Delete Note | destructive, last |

- The ⋯ menu, the card's context menu and the Mac Card menu (R339) use the same titles, which are also the items' accessibility labels. Shortcuts are the system's menu glyphs, never part of a title.
- `…` is U+2026. In `Audio −5 s` the minus is U+2212, and both audio titles have an ordinary space before `s`, as R374 writes them.
- The zoom titles are the View menu's names for the same commands (`review/commands#K1`).
- The Mac Card menu's other review items take their titles from S13 (Show Answer, the grades) and S5 (Undo, Redo).

## S19. Dialogs and panels over the review (R241, R384, R386, R1263)

Meguri's wording (S14, `review/card-actions#A13`).

| Key | English | Used for |
|---|---|---|
| `review.setDue.title` | Set Due Date | the Set Due Date dialog's title (`review/card-actions#A7`, A12) |
| `review.setDue.prompt` | Days until due | the label of its text field |
| `review.setDue.hint` | 0 is today. Enter a number of days, such as 3, or a range, such as 3-7, for a random day within it. An ending ! is accepted but changes nothing more. | the explanation under the field |
| `review.setDue.error.malformed` | Enter a number of days, such as 3, or a range, such as 3-7. | the inline message for `Malformed` (`sched/set-due-date#S2`) |
| `review.setDue.error.tooLarge` | The number of days is too large. | the inline message for `TooLarge` |
| `review.setDue.apply` | Set | the confirmation button (A7's Apply) |
| `review.forget.title` | Forget Card | the Forget dialog's title (`review/card-actions#A8`, A12) |
| `review.forget.apply` | Forget | its confirmation button (A8's Apply) |
| `review.cardInfo.current` | Current | the first segment of Card Info's switch when opened from review (`review/card-actions#A9`, R1263) |
| `review.cardInfo.previous` | Previous | the second segment; disabled while there is no previous card (`stats/card-info#C9`) |
| `review.deckOptions.title` | `Deck Options for %@` | the title of the Deck Options sheet opened from review (`review/card-actions#A10`, R241); `%@` is the human name (`decks/decks`, components joined by `::`) of the current card's home deck, the original deck for a card in a filtered deck |

- `Empty` has no message: Apply stays disabled (A7). Whitespace-only text is `Malformed`.
- The hint says that `!` changes nothing more: Meguri always sets due dates with FSRS on (`sched/set-due-date#S1`, R702), and there `!` has no effect (S2). R384 still requires the dialog to accept it.
- Cancel in both dialogs is the system's.
- The Forget dialog's toggles are S14's.

# Card actions in review

Spec: review/card-actions
Requirements: R241, R375, R380, R381, R382, R383, R384, R385, R386, R387, R389, R390, R391, R392, R393; Meguri side of R374, R376, R377, R378, R379, R388, R1262, R1263, R1422; supports R1812
Anki: 26.09.3 for the Anki statements of A1 to A11 (what each action of Anki desktop's reviewer does to the current card and what the reviewer shows next), except the differences A14 lists; none for the bullets marked Meguri, A12 and A13 (Meguri's transition rule, dialogs, menu and panels, from the Product Contract and `docs/design/native-interface-contract.md` N11.1, N11.9)
Scenarios: conformance/scenarios/review/actions/ (drafts, not recorded yet, A15)

What each card action of the review screen does: which card and note it acts on, the op it runs, what it stores, and what the screen presents afterwards. The keys are `review/commands`; the toasts and dialog labels are `review/reviewer-strings` (S4, S14); the ops themselves are the specs named in each section. U182 and U183 build on this file, U181's `ReviewActions` exposes these actions, and editing is `review/edit-current`.

## A1. Shared rules (R392, R393, R1812)

- Every action acts on the current card (`sched/review-session#C4`) and, for a note action, on that card's note. It is available on both sides and never acts on a card that is only prepared.
- Meguri: no action is available in transition (`review/commands#K1`). Anki acts in transition on the card just answered (`review/commands#K1`); Meguri never does.
- Every action that writes is one op and makes one undo step (R393). The actions of U182 and U183 name the review session (`sched/review-session#C13`); the others are made by the screens they open, which name none:

| Action | Op | Names the session | Undo step (`specs/conformance/undo-ops.md` V4) |
|---|---|---|---|
| Flag | set flag (`tags/flags-and-marked#F2`) | yes | Set Flag |
| Mark Note | add or remove the `marked` tag (`tags/flags-and-marked#M2`) | yes | Update Tag or Remove Tag (`sched/review-session#C13.1`) |
| Bury Card, Bury Note | bury (`sched/manual-ops#M2`) | yes | Bury |
| Suspend Card, Suspend Note | suspend (`sched/manual-ops#M3`) | yes | Suspend |
| Delete Note | delete notes (`notes/note-records#D1`) | yes | Delete Note |
| Set Due Date… | set due date, context reviewer (`sched/set-due-date#S9`) | yes | Set Due Date |
| Forget… | reset, context reviewer (`sched/manual-ops#M6`) | yes | Reset Card |
| Edit Note | the editor's note update (`editor/field-html`; `review/edit-current#E2`) | no | Update Note, merged per note within 60 s (`engine/undo#A3`) |
| Create Copy | Add's add note (U195, `notes/add-defaults`) | no (`review/answer-flow#F6.2`) | Add Note |
| Deck Options, on Save | save deck options (`sched/deck-options`) | no (`review/answer-flow#F6.2`) | Deck Options |

- An action that takes the current card out of the presented head while its answer side is shown records no answer for it (R392): no review-log row, no answer in the session record. Anki never submits an answer for such a card either. The session record logs the action kind for the summary (R440).
- An op that fails or is refused changes nothing on screen except its error path (R362) and shows no toast (`review/reviewer-strings#S2`).
- Card Info, Previous Card Info and Deck Options without a save write nothing. Opening and closing them, Set Due Date… and Forget… cancelled, or Create Copy cancelled, leaves the current card on the same side with no new render (A12).
- Meguri (R1812): after Edit Note's close, Flag, Mark Note, Bury Card, Bury Note, Suspend Card, Suspend Note, Set Due Date… and Forget…, the card the screen presents next (or the same card's indicator or new render) is on screen within 50 ms at p95, from the op's input (the key-down, the menu choice or Apply) to the first frame that shows the result. The core part of this budget, the in-session op's head diff, is not yet a budget of `sched/review-session` (C13, C18 give none); A15's latency test measures the whole path.

## A2. Flag (R233, R375, R380)

- Choosing flag n (⌘n, or the Flag submenu) sets the current card's flag to n, or to 0 when the card's flag is already n (`tags/flags-and-marked#F3`). The card's other flag bits are kept.
- Anki: the flag indicator (`_drawFlag`) shows the new flag in place. The side is not drawn again, and the queue is neither rebuilt nor reordered: in Anki a flag change is the one card change that does not refresh the study queue.
- Meguri, the same on screen (R380): the flag indicator changes in place, the side on screen is not rendered again and the queue is not rebuilt. The head diff keeps the same items in the same order with the same tokens and counts (a flag is not a `stateToken` input, `sched/review-session#C5`). The card's flags are a `renderKey` input (C6), so the flagged item carries a new render key and is marked `rerender`. The screen keeps the page on screen under its old key, as `sched/review-session#C13.1` lets Mark Note do, until the side changes or the card is presented again; every prepared page that is not on screen follows C6.
- C13.1 grants this in-place exception to Mark Note only, and `tags/flags-and-marked#F2`/F3 name no session input and no head-diff output. This bullet is the review screen's requirement on those contracts (reported for their owners on 2026-11-10); U182 builds the flag action only once `sched/review-session` states it.
- No toast (`review/reviewer-strings#S4`).
- Anki's Flag submenu lists the seven flags in order by name (the user's `flagLabels` name, or the default name, `tags/flags-and-marked#F4`) and checks the current card's flag. Choosing the checked flag clears it. Meguri's submenu has the same names, order and check; how each row also shows its flag's colour is A13's.

## A3. Mark Note (R234, R381)

- When the current card's note has the `marked` tag (matched as tags match, case-insensitively), the action removes it; otherwise it adds it (`tags/flags-and-marked#M2`).
- The mark indicator (`_drawMark`) changes in place, the side is not rendered again and the queue is not rebuilt (`sched/review-session#C13.1`). No toast.

## A4. Bury Card and Bury Note (R235, R382)

- Bury Card buries the current card; Bury Note buries every card of the current card's note (`sched/manual-ops#M2`: suspended cards are skipped and not counted).
- The current card leaves the head; so do the note's other cards that the head held (Bury Note). The screen presents the new head's item 0 on its question side (C13), or the congratulations content when nothing is left (`review/reviewer-strings#S8`).
- Anki shows a tooltip with the count. Meguri (decided 2026-11-20, round 7, `docs/design/native-interface-contract.md` N20.8): no toast or banner; VoiceOver announces "Card Buried" (`review/reviewer-strings#S4`), and the Undo control's label names the step (S5). Bury Note is not offered yet.

## A5. Suspend Card and Suspend Note (R236, R383)

- Suspend Card suspends the current card; Suspend Note every card of its note (`sched/manual-ops#M3`).
- The screen moves on as A4 does. Meguri: no toast; VoiceOver announces "Card Suspended" (S4). Suspend Note is not offered yet, and Meguri has no unsuspend yet (N20.8, option 2A): until the notes and cards browser, a suspended card comes back only by Undo or by another client after a sync.

## A6. Delete Note (R239, R387)

- Deletes the current card's note and all its cards at once, with no confirmation (Anki's reviewer asks nothing), from the ⋯ menu, the context menu or ⌘⌫ alike (`review/commands#K1`).
- The screen moves on as A4 does (`sched/review-session#C13.1`). The toast and its Undo are `review/reviewer-strings#S4`.

## A7. Set Due Date… (R237, R384, R385)

- The dialog's field starts with the text in `setDueReviewer`, or `"1"` when the key is missing or not a string (`sched/set-due-date#S6`).
- Accepted text is `sched/set-due-date#S2`'s syntax (`0` today, `N`, `N-M`, an optional `!`). The parser runs as the user types (`sched/set-due-date#S9`), and Apply is enabled only while it accepts the text (R384). Return applies only while Apply is enabled; Esc and Cancel close the dialog and write nothing.
- Meguri, what the dialog shows for each refusal kind: `Empty` (the empty field) shows no error message, only Apply disabled, since an empty field is not yet a mistake; `Malformed` (whitespace-only text included) and `TooLarge` show their inline message under the field. The messages are `review.setDue.error.malformed` and `review.setDue.error.tooLarge` (`review/reviewer-strings#S19`).
- Anki checks only after its OK and shows a warning then; an empty or whitespace-only text there is a cancel (nothing written, the key included). In Meguri such a text cannot be applied, which writes nothing either (A14).
- Apply runs Set Due Date on the current card with the context reviewer, which stores the text as typed in `setDueReviewer` (S6), and shows `review.toast.dueDateSet` with the count 1.
- Afterwards the card does not keep its place as the current card (R385). The screen presents the head that a fresh build gives at the op's commit, without C4's current-card rule: the card is presented again only where that build presents it. With `0` it is a review card due today and takes its place in the deck's review order, which can be first again (then on its question side); with `1` or more it is not in today's queue. Anki builds its queue again after the op and shows its head, with the same result (golden draft `review/actions/set-due-requeue`).
- Contract dependency: `sched/review-session#C13` gives every in-session op's head the current-card rule, and C18 checks that head against it. R385 and R386 need set due date and forget with the session id to return a fresh build's head without that rule. The amendment of C13 and C18 is reported for the `sched/review-session` owner (U43) on 2026-11-10; until it lands, C13 stays in force, U183 does not compute a head of its own, and its tests of this bullet and A8's wait for it.

## A8. Forget… (R238, R386)

- The dialog has the two toggles of `review/reviewer-strings#S14`, starting from `restorePositionReviewer` (default true) and `resetCountsReviewer` (default false) (`sched/manual-ops#M6.4`).
- Apply runs Reset on the current card with both choices and the context reviewer, which stores them in those two keys, and shows `review.toast.cardsReset` with the count 1. Esc and Cancel write nothing.
- Afterwards the screen moves on as A7 does, with A7's contract dependency: the card, now new, is presented again today only where the fresh build presents it (when the deck's new-card limit and order allow it, possibly first). Anki shows its rebuilt queue's head the same way (golden draft `review/actions/forget-requeue`).

## A9. Card Info and Previous Card Info (R242, R388, R389, R1262, R1263, R1422)

- Card Info shows `stats/card-info` for the current card. Previous Card Info shows it for the session's previous card, chosen by `stats/card-info#C9` (Meguri's rule, R1264; Anki's differs and C9 records how). With no previous card, Previous Card Info is disabled (R1262).
- Opened from review, the view has a Current / Previous switch that changes the card shown without closing it (R1263).
- Nothing is written, and the review behind it does not change (A12).
- Anki opens each as its own window that does not block the reviewer: reviewing goes on while it is open, and each time the reviewer takes a card, both windows switch to the new current and previous cards. Meguri's view blocks review while open (A12) and keeps the card it opened with until the switch changes it (A14).

## A10. Deck Options (R241, R390)

- Meguri opens the options of the preset of the current card's home deck: its original deck (`odid`) when it is in a filtered deck, otherwise its deck (`did`). The title names that deck. Placement is N11.9's sheet over the review (A12).
- Anki opens the options of the deck being studied. When the current card's deck differs from it (a subdeck's card), or the card is in a filtered deck whose original deck differs, it first asks which deck to show ("Which deck would you like to display options for?"), listing normal decks before filtered ones. Meguri asks nothing (R390; A14).
- A save is a deck-config change of the collection (`sched/deck-options`). The session reacts as to any change (C13): it rebuilds; the current card stays item 0 while the rebuilt queue still presents it (C4), and its states and labels are those of the saved preset (R390). When the rebuilt queue does not present it (the limits now exclude it), the new head's item 0 is presented on its question side. Anki shows its rebuilt queue's head instead (`sched/review-session#P11`).
- Closing without a save changes nothing (A12).

## A11. Create Copy (R240, R391)

- Opens the Add editor (U195) with a new note of the current note's note type, in the current card's home deck (as A10 chooses it), with the note's fields exactly as stored and all its tags, `marked` and `leech` included. An image occlusion note opens with its image and masks in the mask editor, as a new note.
- Nothing is written until Add saves; Add's own rules then apply (`notes/add-defaults`), including the keys an add writes. Cancel writes nothing.
- When Add closes, review resumes on the same card and side with no new render (A12). A saved copy whose cards join the session's queue reaches it as a change (C13): the current card stays first (C4). Anki, which builds its queue again after the add, shows the new head on its question side (A14).

## A12. Panels and dialogs over the review (Meguri, R388, R6)

Every item opens over the review, never as a window (R6), in the platform's native presentation:

| Item | Mac | iPad | iPhone |
|---|---|---|---|
| Set Due Date…, Forget… | a sheet on the main window | a form sheet | a sheet |
| Card Info, Previous Card Info (R388) | an in-window panel over the card area, as the edit panel (N11.1) | a sheet or a trailing inspector, by width, as the edit panel (`review/edit-current#E7`) | a sheet |
| Deck Options | a sheet over the review (N11.9) | a sheet over the review (N11.9) | a sheet over the review (N11.9) |
| Create Copy (Add, U195) | an in-window panel over the card area (N11.1's `editor.add`) | a sheet | a sheet |

- Set Due Date… and Forget… are small forms: a title, their field or toggles, and Cancel and Apply as the sheet's cancel and confirmation actions (Apply the default action, Return; Cancel, Esc).
- While any item is open, review keys and tap zones are inactive (N4, N11.1) and Esc closes it first (`review/commands#K9`); on the Mac ⌘W also goes to its close, never to Back to Decks (N11.1).
- When the item changed nothing that the current card depends on, closing it returns to the same side with no new render: the card's pages stay in place under it. Otherwise the session's head diff decides (C13, C17).
- The answer timer keeps running while one of these is open, as it does in Anki while its separate windows are open. The time taken that an answer records stays capped at the preset's maximum answer seconds (`sched/answering`). (The edit panel pauses it instead, `review/edit-current#E2`.)

## A13. The ⋯ menu and the card context menu (Meguri, R374, R376, R377, R378, R379)

- Items in R374's order: Edit Note; Redo (iPhone only, when a step can be redone); Flag ▸; Mark Note; Bury Card; Bury Note; Suspend Card; Suspend Note; Set Due Date…; Forget…; Card Info; Previous Card Info; Template Errors… (only when shown, below); Deck Options; Replay Audio; Pause Audio; Audio −5 s; Audio +5 s; Auto Advance; Card Zoom (−, reset, +); Create Copy; Delete Note (destructive, last).
- The ⋯ menu is the iPhone and iPad toolbar button's menu (N11.1, N13.3). The card's context menu has the same items: on the Mac a secondary click on the card, and on the iPad a pointer secondary click on the card; when text is selected, the system's Copy, Look Up and Translate come first (R379). `M` opens it as `review/commands#K1.1` says.
- The Flag submenu shows each flag's name (A2) with a circle symbol in the flag's colour, and checks the current flag (A2). The colour is never the only cue: the name is the row's title and its accessibility label.
- Mark Note shows a check while the note is marked, and Auto Advance while it is on (R376).
- Disabled, not hidden, when they do not apply (R377, `review/commands#K12`).
- Template Errors… appears when the current card's page logged template script errors, and lists message, line and card template (R378, R522).
- Anki's More menu holds the same card actions in another order, with Record Own Voice and Replay Own Voice, without Edit Note, Redo and Template Errors; it checks the current flag and Auto Advance but not Mark Note.
- Titles: Meguri does not take Anki's menu titles. Every title of this menu and of the Mac Card menu, of the Set Due Date and Forget dialogs (A7, A8, A12), of the Current / Previous switch (A9) and of the Deck Options sheet that names the deck (A10) is a key of `review/reviewer-strings`: S18 for the menus, S19 for the dialogs, the switch and the sheet title. Neither unit takes a title from anywhere else (KTD1).

## A14. Differences from Anki

| Situation | Anki 26.09.3 | Meguri |
|---|---|---|
| A key or action in transition | acts on the card just answered | does nothing (A1) |
| Set Due Date text refused | warning after OK | inline error, Apply disabled (R384) |
| Set Due Date with an empty or whitespace-only text | OK cancels, writing nothing | Apply disabled; Cancel writes nothing (A7) |
| Deck Options when the card's deck is not the studied deck | asks which deck, studied deck included | opens the card's home-deck preset (R390) |
| After a Deck Options save, or an added copy that joins the queue | the rebuilt queue's head, on its question side | the current card stays while still presented (C4) |
| Previous Card Info | the card shown before the current one | the session's last answered card (`stats/card-info#C9`) |
| Delete Note toast | counts the cards, no Undo | "Note deleted." with Undo (`review/reviewer-strings#S4`) |
| Disabled items | every More menu item enabled | disabled when not applicable (R377) |
| Dialogs and panels | separate windows | over the review, never a window (R6, R388) |
| Card Info and Previous Card Info while open | non-blocking windows: reviewing goes on, and both follow each new current and previous card | the view blocks review keys and tap zones, and shows the card it opened with until the Current / Previous switch changes it (A9, A12) |

No observation of the oracle covers the reviewer, so none of these needs a `conformance/deviations.yaml` entry; the collection effects (A2 to A8) are Anki's.

## A15. Scenarios and tests

Draft scenarios, not recorded and not active (`conformance/scenarios/review/area.yaml` lists none). They observe the collection side of these actions with the ops the oracle has (`specs/conformance/sched-ops.md` J2, J3, J6, J7 and J10's writing rules; `queue-ops.md` Q2 and Q5):

| Scenario | Covers |
|---|---|
| `review/actions/set-due-requeue` | A7: `0` on the head review card puts it in its review-order place in the fresh build, `1` takes it out of today's queue; `setDueReviewer` as typed |
| `review/actions/forget-requeue` | A8: the reset card comes back as a new card in the fresh build; the two reviewer keys |
| `review/actions/bury-note-head` | A4: Bury Note takes the head card and its presented sibling out of the queue, Bury Card the next one, undo brings it back |

Meguri's tests: `CardActionPerformerTests` (U182: A1 to A6, A13, the transition rule, the flagged item's page kept on screen), `ReviewActionLatencyTests` (U182 with U183 and U184: A1's 50 ms p95 from input to the first frame, for each action of that bullet, on a 14.4k-card collection), `SetDueDateDialogModelTests` and `ForgetDialogModelTests` (U183: A7 and A8 dialogs: the prefill, Apply disabled for each refusal kind, no message for `Empty`, the inline message for `Malformed` and `TooLarge`, Return only while Apply is enabled, Cancel and Esc writing nothing), `ReviewPanelHostTests` (U183: A9 to A12, the presentation per platform, ⌘W and Esc closing, the render counter unchanged after a panel that changed nothing, the timer still running).

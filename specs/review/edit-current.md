# Editing the current note from review

Spec: review/edit-current
Requirements: R230, R398, R399, R400, R818, R824 (replaced here, E2); Meguri side of R394, R395, R396, R397, R679; supports R447, R1812
Anki: 26.09.3 for E1, E3 to E6 and the Anki column of E8 (what Anki desktop's reviewer edits, how its edit window behaves and how it shows the card after the note changed), except the differences E8 lists; none for E2 and E7 (Meguri's form, from the Product Contract and `docs/design/native-interface-contract.md` N4, N11.1, N17.1, N20.8)
Scenarios: conformance/scenarios/review/edit/ (drafts, not recorded yet, E9)

What the review screen does when the user edits the current card's note with Edit Note (the touch review's More menu, Card › Edit Note ⌘E and the card's context menu on Mac, the E key): what is edited, what holds while the form is open, and how the card is shown again when it closes. The form is round 5's Add Note form (`docs/design/native-interface-contract.md` N17.1) titled Edit Note, with Cancel and Done (N20.8, decided 2026-11-20, option 7A). The editor itself (fields, tags, saves, undo merging) is the editor area's (U190); this file is the review side, which U184 builds.

## E1. What is edited (R230, R394)

- The note of the current card (`sched/review-session#C4`), on either side: all its fields and its tags. The editor opens with the first field focused.
- Anki's Edit Current window changes only fields and tags: it has no deck or note type chooser. Meguri's form has none either: the Add Note form's Deck and Type rows are replaced by a subtitle under the title, "<deck> · <note type>" (the card's deck by its last name component, the note type's name), which cannot be edited there. The form offers no note action that removes or moves the card; E5 covers such a change made elsewhere.
- Opening the form writes nothing and does not render the card again.

## E2. While the form is open (Meguri, R395, R396)

- Review keys and tap zones are inactive (`review/commands#K4`, N11.1); Esc and the close actions go to the form (E7).
- Card audio stops when it opens (R395).
- The answer timer pauses while the form is open (R679; `review/answer-flow#F2`). Anki keeps counting while its edit window is open.
- **A draft until Done** (decided 2026-11-20, N20.8). Field and tag changes stay in the form: the field commits of `editor/field-html` S1 update the session targets and send no patch, and moving the app to the background writes nothing. Done flushes the editor and sends one field patch (`editor/field-html` S2) with every field whose target differs from its stored HTML and the tags when they changed, then closes the form when the patch commits: one "Update Note" undo step (R396; `engine/undo`, which may merge it with an earlier update of the note within 60 seconds). Done with no change sends nothing and closes. A failed patch keeps the form open with its text and S2's failure. Cancel closes and drops the draft; with any change it first asks `editor/editing-strings`' "Discard changes?" with Discard and Keep Editing (the wording of the editors that save on confirm). The swipe down on touch, Esc and ⌘W act as Cancel. This replaces, for this form, `editor/edit-flows` W3's saves while typing and its no-prompt rule (R824), and R863's Esc that saves.
- Done's save is the editor's note update naming the review session (decided 2026-11-20, round 7, `docs/design/native-interface-contract.md` N20.8: the review's Undo covers a saved Edit Note). The field patch carries the session's id (`editor/field-html` S2); the review admits it as it admits a card action (`sched/manual-ops` M9: the session must be the open one, not ending or restoring; the session's redo list is cleared), and the "Update Note" step records the session (`engine/undo` M3), so the review's Undo offers it as "Undo Edit Note" (`review/reviewer-strings` S5). Undoing it restores the note and shows the current card again, as E3 does after a save. When the core no longer has that session open (the review ended, the collection reopened under the screen), Done saves without the session id, so the draft is never lost. The writer records the session that is open for any user op, so a change another host makes to the same note during the session can merge into this step (`engine/undo` A3) and the review's Undo then restores both. The open session also reacts to the committed change as to any change (`sched/review-session#C13`): a patch, or a rebuild when the save adds a card (E4), then the head diff, and for the current card `CurrentCardChanged{eligible}` when its render key changed. A change made elsewhere while the form is open is `editor/edit-flows` W5's, with every changed field of the draft counted as an unsaved edit: the unchanged fields and tags reload, the changed ones keep the draft, and Done's patch lists only the changed ones, so another writer's other fields stay. W5.3's Keep mine only dismisses its banner; the draft is sent at Done. A collection replacement asks as Cancel does before it starts (`editor/add-flow` A7.1's guard); on Mac, quitting asks the same as N11.11's other screens.
- While the form is open, the screen holds what those reports ask of the side on screen: it takes the new head items, tokens, labels and counts into its model at once, but it does not render the side again, play audio or restart a countdown until the form closes; E3 then renders once. `{eligible}` shows no toast (`review/reviewer-strings#S10`). A save never removes the current card from the session (E4), so Done never leads to `{removed}`.
- `sched/review-session#C13` lists "editing" among the ops that name the session; since 2026-11-20 the review's Edit Note does (above). Browse's and the edit flows' saves pass no session id. `review/reviewer-strings#S10` no longer lists the edit panel (2026-11-10).

## E3. After the form closes (R398, R818)

When Done saved a change (any field or tag), the side that was showing is rendered again from the saved note, as a new presentation of that side:

| Side showing | What is rendered again | Also |
|---|---|---|
| question | the question side | a typed answer is discarded; the question's audio plays again when the preset autoplays; the Auto Advance question countdown starts again (U185); the on-screen timer starts again from 0 (R364; withdrawn 2026-11-19: Meguri has no on-screen timer, `review/timers#T2`) |
| answer | the answer side | the text typed in `#typeans` before reveal is kept and compared again with the new expected text (`render/type-answer`); the answer's audio plays again when the preset autoplays; the answer buttons are drawn again (U178's gate); the Auto Advance answer countdown starts again |

- The card's scripts run again on the new page (R398). Prepared pages of the note's cards are discarded and prepared again (R447; `sched/review-session#C6`).
- The queue is not rebuilt and its order does not change: in Anki a change of note text or tags redraws the current card but does not refresh the study queue. The button labels change only when the card's scheduling states changed (C5), which a field or tag edit does not do.
- The answer timer restarts when the side is rendered again (R398): the time an answer then records counts from that render. Anki restarts only the visible timer, and only on a question-side redraw; the time it records still counts from the card's first presentation (E8).
- When nothing was saved (Cancel, or Done with no change), closing changes nothing: no new render, no audio, and the paused answer timer goes on from its value (Anki redraws only after a note change). The exception is a render that E2 held while the form was open (another host changed the note or its card): the close renders it once, as E3's table says for that side, whatever the form saved. `review/answer-flow#F2`'s "it restarts when the edit panel closes" means a close after a save; the wording fix there is reported for its owner (2026-11-10).
- Anki redraws when its main window is focused again after the change; Meguri when the form closes.
- The session keeps its place: the same card and side, its answer timing as above and its undo history, to which Done adds one "Update Note" step of this session (E2).

## E4. A blank card after the edit (R399)

- When the edit leaves the current card blank (an empty front for a standard note type, no cloze of its number for a cloze note type; `render/card-render-pipeline`), the side shows the blank message as its content (R531, `render/card-render-pipeline`).
- The card stays in the queue and can still be answered, buried and suspended: an edit never deletes cards, and a blank card is studied like any other in Anki. Its answer side shows the blank result (no answer template output).
- An edit that adds content for another card (a new cloze number, a field that makes another template non-empty) adds that card as a new card (`notes/card-generation`). It is a card change: the session rebuilds (C13), the current card stays item 0 (C4) and the edited side is rendered again as E3 says. Anki refreshes its queue then and shows its head (E8).

## E5. The card leaves the session (R400)

- When the note was deleted elsewhere while the form was open (`editor/edit-flows` W5.4 closes the form) or the card moved out of the session's deck tree, the current card is no longer presented. When the form closes, the screen presents the new head's item 0 on its question side, or the congratulations content when nothing is left.
- The form offers no Delete Note; the review's own Delete Note (`review/card-actions#A6`) is an in-session op and shows no changed-elsewhere toast. A deletion elsewhere shows `review/reviewer-strings#S10`'s toast when the screen applies `CurrentCardChanged{removed}`; with the form open, the screen applies it when the form closes.
- Anki: when its note is deleted while the edit window is open, the window closes, and the reviewer shows its refreshed queue's head.

## E6. Other cards of the note

The note's other cards keep their places in the queue. Their prepared pages are rendered again before they are shown (E3, R447); nothing else about them changes.

## E7. The form (Meguri, R394, R397)

- Entries (N20.8): touch, the review toolbar's More menu (`ellipsis`, labelled "More", after Undo), whose first item is Edit Note (`pencil`); Mac, Card › Edit Note (⌘E) and the card's context menu, with no toolbar button; and the E key (`review/commands`). They are enabled while a card is presented, on either side.
- Placement, as Add Note's (N17.1): iPhone and the iPhone Duo folded or held tall, a sheet; the Duo held wide (any iPhone at regular width), the whole screen with the form in a centred column of the readable width; iPad, the system page sheet; Mac, N11.1's in-window panel over the card area, which owns the keys. Never a separate window (R6, N11.1).
- Done (the confirmation action; ⌘Return, Anki's Close shortcut being Ctrl+Return) saves and closes as E2 says. Cancel (the cancellation action), Esc, ⌘W (N11.1: never Back to Decks) and the swipe down on touch close as Cancel, asking first when the draft has a change. Esc while the tag editor shows suggestions first hides them (`editor/tag-editor` G4).
- The same form opens over the session summary for a missed card's Edit Note (U157); E3 then has no side to render.

## E8. Differences from Anki

| Situation | Anki 26.09.3 | Meguri |
|---|---|---|
| Where the editor opens | a separate window | the Add Note form over the card, as N20.8 places it (R394) |
| Saving | saves as the user types and when the window closes | a draft, saved by Done; Cancel asks before it drops a change |
| Reviewing while the editor is open | the window does not block the reviewer: the user can go on answering, and pressing E again loads the note of the card current at that moment into the open window | the form blocks review keys and tap zones (E2, R395); E is inactive while it is open |
| Audio while editing | keeps playing | stops (R395) |
| The answer timer while editing | keeps counting | paused (R679) |
| The recorded time after a redraw | counts from the card's first presentation | counts from the re-render (R398) |
| When the card is shown again | when the main window is focused after the change | when the form closes |
| An edit that adds cards | the refreshed queue's head, on its question side | the current card stays, on its side (C4) |

No oracle observation covers the reviewer, so these need no `conformance/deviations.yaml` entry.

## E9. Scenarios and tests

| Scenario (draft, not active) | Covers |
|---|---|
| `review/edit/blank-front-stays` | E4: after an edit that empties the front, the card is still in the queue and can be answered; the note's text and the card row |

Meguri's tests: `EditCurrentCoordinatorTests` (U184: E2's draft: no patch before Done, one patch and one "Update Note" step at Done, nothing at Done with no change, Cancel with and without a change, a foreign change to another field kept after Done; E2's held reports and one render on close, E3's table on both sides, nothing saved → no render, the timer restart, E4, E5 with the note deleted elsewhere while the form is open and with the card moved out), `EditCurrentUITests` (E7's entries and closing actions: the toolbar button, ⌘E, the context menu, E, Done, ⌘Return, Cancel, Esc, ⌘W and the swipe, with and without a change).

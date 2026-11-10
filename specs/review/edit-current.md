# Editing the current note from review

Spec: review/edit-current
Requirements: R230, R398, R399, R400, R818; Meguri side of R394, R395, R396, R397, R679; supports R447, R1812
Anki: 26.09.3 for E1, E3 to E6 and the Anki column of E8 (what Anki desktop's reviewer edits, how its edit window behaves and how it shows the card after the note changed), except the differences E8 lists; none for E2 and E7 (Meguri's panel, from the Product Contract and `docs/design/native-interface-contract.md` N4, N11.1)
Scenarios: conformance/scenarios/review/edit/ (drafts, not recorded yet, E9)

What the review screen does when the user edits the current card's note with E or ⋯ › Edit Note: what is edited, what holds while the editor is open, and how the card is shown again when it closes. The editor itself (fields, tags, saves, undo merging) is the editor area's (U190); this file is the review side, which U184 builds.

## E1. What is edited (R230, R394)

- The note of the current card (`sched/review-session#C4`), on either side: all its fields and its tags. The editor opens with the first field focused.
- Anki's Edit Current window changes only fields and tags: it has no deck or note type chooser. Meguri's panel is U190's editor component in edit mode; when it offers a note action that removes or moves the card (Delete Note, a deck change), E5 applies.
- Opening the editor writes nothing and does not render the card again.

## E2. While the editor is open (Meguri, R395, R396)

- Review keys and tap zones are inactive (`review/commands#K4`, N11.1); Esc and the close actions go to the panel (E7).
- Card audio stops when it opens (R395).
- The answer timer pauses while the panel is open (R679; `review/answer-flow#F2`). Anki keeps counting while its edit window is open.
- Field and tag changes are saved by the editor's own rules: debounced saves, and the "Update Note" undo step that merges the saves of one note within 60 seconds (R396; `editor/field-html`, `engine/undo`).
- Each save is the editor's note update, which names no review session, from this host as from every other (`editor/field-html`, decided 2026-10-27; `engine/undo#M8.1` counts such a save as a step of no session). The open session therefore reacts to it as to a change made elsewhere (`sched/review-session#C13`): a patch, or a rebuild when the save adds a card (E4), then the head diff, and for the current card `CurrentCardChanged{eligible}` when its render key changed.
- While the panel is open, the screen holds what those reports ask of the side on screen: it takes the new head items, tokens, labels and counts into its model at once, but it does not render the side again, play audio or restart a countdown until the panel closes; E3 then renders once, however many saves were made. `{eligible}` shows no toast (`review/reviewer-strings#S10`). A save never removes the current card from the session (E4), so the panel's saves never lead to `{removed}`.
- `sched/review-session#C13` lists "editing" among the ops that name the session, which contradicts `editor/field-html`, which this file follows. `review/reviewer-strings#S10` no longer lists the edit panel (2026-11-10). The amendment of C13's list (and of `review/answer-flow#F6.2`'s list of this session's steps) is reported for their owners on 2026-11-10.

## E3. After the editor closes (R398, R818)

When the note was saved while the editor was open (any field or tag change), the side that was showing is rendered again from the saved note, as a new presentation of that side:

| Side showing | What is rendered again | Also |
|---|---|---|
| question | the question side | a typed answer is discarded; the question's audio plays again when the preset autoplays; the Auto Advance question countdown starts again (U185); the on-screen timer starts again from 0 (R364) |
| answer | the answer side | the text typed in `#typeans` before reveal is kept and compared again with the new expected text (`render/type-answer`); the answer's audio plays again when the preset autoplays; the answer buttons are drawn again (U178's gate); the Auto Advance answer countdown starts again |

- The card's scripts run again on the new page (R398). Prepared pages of the note's cards are discarded and prepared again (R447; `sched/review-session#C6`).
- The queue is not rebuilt and its order does not change: in Anki a change of note text or tags redraws the current card but does not refresh the study queue. The button labels change only when the card's scheduling states changed (C5), which a field or tag edit does not do.
- The answer timer restarts when the side is rendered again (R398): the time an answer then records counts from that render. Anki restarts only the visible timer, and only on a question-side redraw; the time it records still counts from the card's first presentation (E8).
- When nothing was saved, closing changes nothing: no new render, no audio, and the paused answer timer goes on from its value (Anki redraws only after a note change). `review/answer-flow#F2`'s "it restarts when the edit panel closes" means a close after a save; the wording fix there is reported for its owner (2026-11-10).
- Anki redraws when its main window is focused again after the change; Meguri when the panel closes.

## E4. A blank card after the edit (R399)

- When the edit leaves the current card blank (an empty front for a standard note type, no cloze of its number for a cloze note type; `render/card-render-pipeline`), the side shows the blank message as its content (R531, `render/card-render-pipeline`).
- The card stays in the queue and can still be answered, buried and suspended: an edit never deletes cards, and a blank card is studied like any other in Anki. Its answer side shows the blank result (no answer template output).
- An edit that adds content for another card (a new cloze number, a field that makes another template non-empty) adds that card as a new card (`notes/card-generation`). It is a card change: the session rebuilds (C13), the current card stays item 0 (C4) and the edited side is rendered again as E3 says. Anki refreshes its queue then and shows its head (E8).

## E5. The card leaves the session (R400)

- When the note was deleted (from the panel, or elsewhere while it was open) or the card moved out of the session's deck tree, the current card is no longer presented. When the panel closes, the screen presents the new head's item 0 on its question side, or the congratulations content when nothing is left.
- A deletion from the panel, when the panel offers Delete Note, is the review screen's Delete Note: U184 runs it with the session id, as an in-session op (C13.1, `review/card-actions#A6`), so it shows no changed-elsewhere toast. A deletion elsewhere shows `review/reviewer-strings#S10`'s toast when the screen applies `CurrentCardChanged{removed}`; with the panel open, the screen applies it when the panel closes.
- Anki: when its note is deleted while the edit window is open, the window closes, and the reviewer shows its refreshed queue's head.

## E6. Other cards of the note

The note's other cards keep their places in the queue. Their prepared pages are rendered again before they are shown (E3, R447); nothing else about them changes.

## E7. The panel (Meguri, R394, R397)

- Mac: an in-window panel over the card area; iPad: a sheet or a trailing inspector, by width; iPhone: a sheet. Never a separate window (R6, N11.1).
- It closes with ⌘Return (Anki's Close shortcut, Ctrl+Return), Esc, ⌘W (N11.1: Esc and ⌘W go to the panel's close guard, never to Back to Decks), its Close button, or a swipe down on the iPhone, each after the editor's pending save (`editor/edit-flows#W3`; Anki also saves the note before its window closes).
- The same panel opens over the session summary for a missed card's Edit Note (U157); E3 then has no side to render.

## E8. Differences from Anki

| Situation | Anki 26.09.3 | Meguri |
|---|---|---|
| Where the editor opens | a separate window | a panel or sheet over the card (R394) |
| Reviewing while the editor is open | the window does not block the reviewer: the user can go on answering, and pressing E again loads the note of the card current at that moment into the open window | the panel blocks review keys and tap zones (E2, R395); E is inactive while it is open |
| Audio while editing | keeps playing | stops (R395) |
| The answer timer while editing | keeps counting | paused (R679) |
| The recorded time after a redraw | counts from the card's first presentation | counts from the re-render (R398) |
| When the card is shown again | when the main window is focused after the change | when the panel closes |
| An edit that adds cards | the refreshed queue's head, on its question side | the current card stays, on its side (C4) |

No oracle observation covers the reviewer, so these need no `conformance/deviations.yaml` entry.

## E9. Scenarios and tests

| Scenario (draft, not active) | Covers |
|---|---|
| `review/edit/blank-front-stays` | E4: after an edit that empties the front, the card is still in the queue and can be answered; the note's text and the card row |

Meguri's tests: `EditCurrentCoordinatorTests` (U184: E2's held reports, with several saves and one render on close, E3's table on both sides, nothing saved → no render, the timer restart, E4, E5 both ways, the panel's Delete Note with the session id and no toast), `EditCurrentUITests` (E7's closing actions: ⌘Return, Esc, ⌘W, Close and the swipe).

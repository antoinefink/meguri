# Browse preview

Spec: browser/preview
Requirements: R1216, R1217, R1218, R1219, R1220, R275; supports R273, R257, R1185, R1221
Anki: 26.09.3 for P2 to P8 (the desktop browser's previewer); none for the Meguri rules P10 lists
Scenarios: none (UI behaviour; P11 lists the tests)

What Browse's preview (U221's Preview tab on Mac and iPad, the pushed preview on iPhone, U223) shows, which side, how Previous and Next move through the results, when it renders again, which audio it plays and when it closes. The rendering is review's (`render/card-render-pipeline` K4, `render/display-transform`, `render/card-runtime`); the audio controller is `render/audio-playback`'s. Anki's previewer is a desktop dialog the oracle does not run, so Meguri's tests check this file (P11).

## P1. Terms

- **Shown card**: the card of Browse's single selected row (`browser/inspector` I1): in cards mode that row's card; in notes mode its note's card with the lowest `ord`, then the lowest id (`browser/bulk-actions` B2's rule). There is none when the selection is not exactly one row, or the row is a note without cards.
- **Side**: question or answer. **Both sides** is the stored setting `previewBothSides` (`browser/config-keys` K6.3), read when the preview opens.
- **Presentation**: one side of one card put on screen: opening, a change of side, another shown card. Re-rendering the same side of the same card after a change (P5) is not a new presentation.

## P2. Opening and closing (R273, R275, R1220)

- ⇧⌘P (R273), the View menu's Preview and, on Mac and iPad, choosing the inspector's Preview tab open the preview when there is a shown card. With none, the command is disabled and so is the tab.
- On Mac and iPad, open means the inspector shows its Preview tab; opening expands the inspector when it is collapsed. On iPhone, open means the preview is pushed (U223).
- It opens on the question side, or on the answer side when Both sides is on.
- ⇧⌘P again, and Esc while the preview has focus, close it (R275). It also closes when the selection stops being exactly one row or the shown card goes away, as Anki's previewer closes (R1220). Closing stops its audio and clears its waiting audio, always (`render/audio-playback` A1's owner rule; Anki does the same).
- On Mac and iPad, close means the inspector shows its Editor tab; the inspector stays expanded. On iPhone, the pushed preview is popped.

## P3. Previous and Next (R1217, R275)

← and → (and the two on-screen buttons) go through the side and then through the results:

| Key | Side now | Both sides | Does |
|---|---|---|---|
| → (Next) | question | off | shows the answer of the same card |
| → (Next) | answer | off or on | moves Browse's current row to the next row (P4) |
| ← (Previous) | answer | off | shows the question of the same card |
| ← (Previous) | answer | on | moves to the previous row |
| ← (Previous) | question | off | moves to the previous row |

- With Both sides on the side is always the answer, so ← and → move from card to card.
- The card reached by a move is shown on its question side, or its answer side with Both sides on (P5). ← never lands on the previous card's answer.
- "Next row" and "previous row" are the rows after and before Browse's **current row** in the table's order, as in Anki, also in the rare state where the single selected row is not the current row (`browser/inspector` I1): with row 3 selected and row 5 current, a move goes to row 6, and the selection then becomes that row.
- **Enabled**: Next when the side is the question (with Both sides off) or a next row exists; Previous when the side is the answer with Both sides off, or a previous row exists.
- Return does what → does (Anki's default button); R replays (P7); B toggles Both sides (P6). These keys act while the preview has focus.

## P4. Moving the row (R1217, R1221)

A move is Browse's own Previous or Next command (`browser/inspector` I4): the editor's pending edit is flushed first, the current row moves by one, the selection becomes that one row, the table scrolls to it, and the editor keeps its focused field (I3). When the flush fails, the row does not move (I5), and the preview stays as it is. The preview then follows the new shown card (P5). On iPhone the pushed preview moves through the results the same way (U223).

## P5. Rendering again (R1185, R1220)

- **Another card** (the shown card's id changed: a move, a click on another row, a search that keeps one row selected): a new presentation, on the question side, or on the answer side with Both sides on.
- **The same card**: after every change set that lists the shown card, its note, its note type or any deck (the inputs of `render/card-render-pipeline` K4, among them the card-dependent special fields `{{Deck}}`, `{{Subdeck}}` and `{{CardFlag}}`, `render/special-fields`), the side shown is rendered again from the stored data. When the result equals what is shown (the same side HTML and body classes), the page is not updated. This is not a new presentation, so it plays nothing (P7).
- The flag and mark indicators are updated at every render and after every change set that touches the card or its note (P8).
- Renders are at least 300 ms apart (Anki waits out the rest of 300 ms after the previous render, then renders once with the latest state), so a change is shown at most 300 ms after it happened (R1220).
- Anki renders the content again only when the side, the card's id or its note's `mod` changed; a change to the card alone (flag, deck, suspension), to a deck name or to a template updates only its flag and mark icons (P10).

## P6. Both sides (R1218)

- B, or the toggle, sets Both sides and writes `previewBothSides` with the new value (`browser/config-keys` K6.3; its undo class is K10.2's first row).
- Turning it on shows the answer of the shown card. Turning it off while the answer is shown goes back to the question. Either is a new presentation.
- The answer side holds the question through the template's `{{FrontSide}}`, so showing it "shows question and answer together" for templates that use FrontSide; the preview adds nothing of its own.

## P7. Audio (R1219)

The policy values come from the shown card's home deck's preset and the collection (`render/audio-playback` A1): autoplay, "skip question when replaying answer" and `interruptAudioWhenAnswering`.

- **Automatic playback**, only when autoplay is on, at each presentation (P1); a render of P5's "same card" kind plays nothing (P10):

  | Side shown | List played |
  |---|---|
  | question, Both sides off | the question AV list |
  | answer, Both sides off | the answer AV list |
  | answer, Both sides on | the question AV list, then each entry of the answer list that equals no entry of the question list |

  Two sound entries are equal when their names (`render/av-tags` A6) are equal; two TTS entries when their language and content are equal. So a sound on both sides plays once. The answer list never holds the question's FrontSide entries (`render/av-tags` A8). TTS entries are silent in Meguri (R564), so which of them are kept changes nothing.
- With autoplay off nothing native plays automatically. HTML media in the card follow `render/audio-playback` A8: the card's own `autoplay` and scripts work as in review (P10).
- **R** (Replay) is `render/audio-playback` A3 with the side shown: the question list on the question side; on the answer side (Both sides on or off), the answer list, preceded by the whole question list when "skip question when replaying answer" is off. Replay removes no repeated entries, as Anki's.
- **A change of side** of the same card (→, ←, B) replaces the audio as A2 says: the waiting entries are dropped, and the current clip stops only when `interruptAudioWhenAnswering` is on.
- **A move to another row** (P4) and **closing** (P2) stop the current clip and drop the waiting entries whatever `interruptAudioWhenAnswering` says (R1219: media stops when Preview closes or moves). On iOS and iPadOS the silent switch is respected (A7).
- A replay button inside the card plays its one entry (A3).

## P8. What the page shows (R1216, R1220)

- The card is rendered exactly as in review (`render/card-render-pipeline` K4: templates, CSS, card JavaScript, media, furigana, image occlusion), in a single card page (U67), with the body classes of `render/card-runtime` C3.1 for the card's ordinal and the appearance (R1216: the user's night-mode handling). The answer side passes that class string too (`_showAnswer` with a non-empty class string, as Anki's previewer does; C3.1), also when it is shown first with Both sides on.
- Before display every type-in marker is removed from both sides (`render/type-answer` A11.2): no box, no comparison.
- The flag and mark indicators show the shown card's flag and whether its note is marked (`render/card-runtime` C3, C10.1, C10.2; `tags/flags-and-marked` M1).
- Rendering changes nothing in the collection (`render/card-render-pipeline` K7).

## P9. After Delete Notes and other removals

- Delete Notes moves the current row and the selection to the nearest row whose note is not deleted before it deletes (`browser/bulk-actions` B11). So the selection usually still holds one row, and the preview shows that row's card as another card (P5). It closes only when no such row remains (the selection is cleared, P2).
- A selected row that leaves the table because another writer removed its card or note (a sync, another screen; `browser/result-sets` H10) leaves the selection; when that leaves no row selected the preview closes (P2).

## P10. Meguri rules and differences from Anki

- The preview is a tab of the inspector on Mac and iPad and a pushed screen on iPhone, never a separate window (R1216, R1232). Labels, buttons and help texts are Meguri's wording (`browser/browse-strings` W6). Anki's "Back Side Only" checkbox label for Both sides is not used.

| Difference | Anki | Meguri | Why |
|---|---|---|---|
| Window | a separate dialog with its own size | inspector tab or pushed screen (P2) | R1216, R1232 |
| Render after a change to the same card | only when its note's `mod` changed; a card-only, deck or template change updates only the flag and mark icons | after any change to the card, its note, its note type or a deck (P5) | R1220: refreshes after the note or card changes |
| Audio on a re-render after a note change | autoplay plays the side again | nothing plays; autoplay only at a presentation (P5, P7) | `render/audio-playback` A2: once per side presentation |
| HTML media with autoplay off | need a user gesture to start | as in review (P7) | `render/audio-playback` A8 |
| Audio on a move to another row | the current clip stops only when `interruptAudioWhenAnswering` is on | always stops (P7) | R1219 |

Open question for Antoine (third row): Anki replays the side's audio every time the shown note is saved, so typing in the editor with the preview open replays its sound after each save. Meguri's default above plays it once per presentation; changing to Anki's behaviour changes only P7's first bullet.

## P11. Tests

`PreviewModelTests` (U221): P3's table and enabled states, with the current row not the selected row; P5 with a fake clock (another card resets the side; a note edit renders the same side within 300 ms and plays nothing; a flag change renders `{{CardFlag}}` again; a render with an unchanged result leaves the page; two changes within 300 ms give one render); P6's writes; P7's lists, with sounds repeated across the sides, with autoplay off, a side change with `interruptAudioWhenAnswering` off and on, and a move with it off; P2's close on a second selected row and the Editor tab shown after it; P9 after Delete Notes with and without a remaining row; the type-in marker removed; the answer's class string. The UI flows are `InspectorUITests` (U221) and `PhoneBrowseUITests` (U223).

# Review keys and commands

Spec: review/commands
Requirements: R227, R228, R229, R230, R231, R232, R233, R234, R235, R236, R237, R238, R239, R240, R241, R242, R243, R244, R417, R418, R419, R420, R421, R422, R423, R424, R425; Meguri side of R339, R373, R377, R379, R427; supports R426, R428
Anki: 26.09.3 for the Anki statements of K1 to K10 (the keys of Anki desktop's reviewer, the side each one acts on, what Space and Return do, shared answer keys, undo from the reviewer, Esc and key matching); none for the bullets and sections marked Meguri, K11 and K12 (Meguri's transition rule, menu placement, Esc, routing and command enablement, from the Product Contract and `docs/design/native-interface-contract.md` N4, N11.1 to N11.3)
Scenarios: none. The oracle runs Anki's collection library, not its reviewer; K13 lists the Meguri tests that check these tables.

Which key does what during review, on which side, and what happens when the user presses it. What each action does to the card and what the review screen shows afterwards is `review/card-actions` (flags, mark, bury, suspend, delete, Set Due Date, Forget, Card Info, Deck Options, Create Copy) and `review/edit-current` (E). U181 builds the key map and the command catalog from this file; U159 mounts the menu bar commands; U183 and U184 attach their panels to the actions named here.

Terms: the **question side** and **answer side** are the sides of the current card (`sched/review-session#C4`). **In transition** means that an answer was accepted and the next card is not presented yet. A **single key** is a key pressed with neither ⌘ nor Control (K10 says which of Shift and ⌥ a binding allows). Anki's Ctrl is ⌘ on the Mac and Alt is ⌥; this file writes Meguri's Mac spelling, which iPad hardware keyboards use too.

## K1. The key table (R227 to R242, R419 to R424)

Every key acts on the current card, on both sides unless the last column says otherwise.

| Key | Anki's binding | Action | Sides |
|---|---|---|---|
| Space | Space | reveal, or answer (K2) | both, K2 |
| Return, keypad Enter | Return, Enter | the same as Space | both, K2 |
| the four answer keys, default `1` `2` `3` `4` | the same, rebindable | Again, Hard, Good, Easy | answer side only (K3) |
| `U` | `u` | undo (K5) | both |
| ⌘Z, ⇧⌘Z | the Edit menu's Undo and Redo | undo, redo (K5) | both |
| `E` | `e` | Edit Note (`review/edit-current`) | both |
| `M` | `m` | open the card actions menu (K1.1) | both |
| `R`, F5 | `r`, F5 | replay the side's audio (K6) | both |
| `5` | `5` | pause or resume audio (K6) | both |
| `6` | `6` | audio back 5 seconds (K6) | both |
| `7` | `7` | audio forward 5 seconds (K6) | both |
| ⌘1 to ⌘7 | Ctrl+1 to Ctrl+7 | toggle flag 1 to 7 (`review/card-actions#A2`) | both |
| `*` | `*` | toggle Mark Note (A3) | both |
| `-` | `-` | Bury Card (A4) | both |
| `=` | `=` | Bury Note (A4) | both |
| `@` | `@` | Suspend Card (A5) | both |
| `!` | `!` | Suspend Note (A5) | both |
| ⇧⌘D | Ctrl+Shift+D | Set Due Date… (A7) | both |
| ⌥⌘N | Ctrl+Alt+N | Forget… (A8) | both |
| ⌥⌘E | Ctrl+Alt+E | Create Copy (A11) | both |
| ⌘⌫ | Ctrl+Backspace (Anki on the Mac) | Delete Note (A6) | both |
| `O` | `o` | Deck Options (A10) | both |
| `I` | `i` | Card Info (A9) | both |
| ⌥⌘I | Ctrl+Alt+I | Previous Card Info (A9) | both |
| ⇧A | Shift+A | toggle Auto Advance (U185; `review/reviewer-strings#S6`) | both |
| ⌘+, ⌘−, ⌘0 | the View menu's Zoom In, Zoom Out and Reset Zoom | card zoom (R571, U67) | both |
| Esc | none of its own (K9) | K9 (Meguri) | both |

- Anki's reviewer acts on the keys of this table on either side, except the answer keys (K3). A key that is unbound in a context does nothing there.
- Anki checks the reviewer's state only on the answer path: the answer keys and Space and Return act only while a question or an answer side is shown. Its other keys also act in transition, on the card just answered, because that card stays the reviewer's card until the next one is taken.
- Meguri: while in transition no key acts, the undo key included. For the answer keys this is R414; for the other keys it is Meguri's rule, so that no action reaches a card that is not on screen (`review/card-actions#A1`).
- ⌘⌫ deletes at once, with no confirmation, as Anki's reviewer does and as the ⋯ menu's Delete Note does (`review/card-actions#A6`, R387). R239's "after confirmation" does not apply: the plan's Planning Contract lists this reading among the "Readings of Product Contract conflicts settled during review" ("R239 and R387: ⌘⌫ deletes the note at once with the Undo toast, as the ⋯ menu does (U181)").
- ⌥⌘N is review's Forget while review is shown; File › New Deck keeps its menu item without that key equivalent then (R238, U159).
- The global single keys of other screens (D, S, A, B, T, Y, `/`) are U159's. In Anki, D goes to the deck list and S to the deck's overview from the reviewer; Meguri's own uses are U159's.

### K1.1 The card actions menu (Meguri, R231, R379)

- iPhone and iPad: `M` opens the review's ⋯ menu from its toolbar button, as a tap on that button does (N11.1, N13.3).
- Mac: there is no ⋯ button (N11.1). `M` opens the card's context menu, which holds the same items (`review/card-actions#A13`), at the pointer when the pointer is over the card area, and otherwise at the centre of the card area. Anki also opens its More menu at the pointer.
- The menu opened by `M` is a menu like any other: it owns the keys while it is open (K4), and Esc closes it (K9).

## K2. Space and Return (R227, R419)

| Side | "Space and Return answer Good" (R134; Anki's "Spacebar (or enter) also answers card") | Result |
|---|---|---|
| question | on or off | reveal the answer; a type-in field's text is the typed answer (`render/type-answer`) |
| answer | on (the default) | answer with the rating of the answer button that has keyboard focus, or Good when none has it |
| answer | off | nothing |

- Return and keypad Enter are the same key here.
- One press acts once (K4): a press that revealed the answer never also answers, even when held (N4).
- An answer button has keyboard focus only when the user moved focus there with the keyboard (Tab, Full Keyboard Access). A click or tap on a button answers at once and leaves no focused button for this rule.
- Meguri: one press has one handler (N4, N11.2); the mechanism is U181's spike S3 (K11). When a row of the table acts (a reveal, or an answer with the setting on), the key map consumes the press, and the system's activation of the focused control (Full Keyboard Access, macOS keyboard navigation) does not also run: Space on a focused Again button answers Again once. When the table's result is "nothing" (the answer side with the setting off), the key map does not consume the press, and the system activates the focused control as a click would: Space on a focused Again button answers Again once, through that button's action, and with no focused button nothing happens. No answer button is a default button (N11.2).

## K3. Answer keys (R228, R418, R420)

- Four bindings, one per rating, each one key (a character), defaults `1`, `2`, `3`, `4` for Again, Hard, Good, Easy. They are device settings (R135, U160).
- An answer key acts only on the answer side. On the question side it does nothing: it does not reveal (R418).
- A binding left empty means that rating has no key; the other three still work. A rebound rating no longer answers to its old key: with Again on `j`, `1` does nothing.
- A letter binding matches its letter key with or without Caps Lock, but not with Shift (K10). `j` and `J` are the same binding. Anki's bindings behave the same: they ignore Caps Lock and treat Shift as part of the key.
- Anki accepts any text and lets two bindings share a key. The reviewer's own keys are one list in which the last entry for a key wins, and the answer keys sit after `e`, `m`, `r`, `o` and `i` and before `u`, `5`, `6`, `7` and Shift+A in that list. So with Hard and Good both on `j`, `j` answers Good; an answer key equal to `e`, `m`, `r`, `o` or `i` takes that key over (with Again on `e`, `e` answers Again on the answer side and does nothing on the question side); an answer key equal to `u`, `5`, `6`, `7` or Shift+A loses it to that action. Only a clash with a global key of the main window (`d`, `s`, `a`, `b`, `t`, `y`) makes neither work, since the two are then separate shortcuts on one key.
- Meguri's Settings refuses a key that another review key uses (R135, R420, U160), so Meguri never has two bindings on one key; this file adds no rule for that case.

## K4. One press, one action (R417)

- A key acts on its key-down only. Auto-repeat key-downs of a held key never act; Anki's shortcuts do not repeat either.
- Each action needs a new press. A held Space reveals once and does not then answer (K2).
- A key pressed while in transition (K1), while a panel or sheet over the review owns the keys (N4, N11.1), or while a menu is open, does not act on the review.

## K5. Undo and redo from the review (R229, R421)

- Anki: `u` and Ctrl+Z undo the newest undoable step of the collection, whatever made it (an answer, a bury, a note edit made in the browser while reviewing). Ctrl+Shift+Z redoes. With nothing to undo, nothing happens and no message is shown.
- Anki: what the reviewer shows after an undo or a redo depends on the step. The undo runs as an op of the undone step's kind with the same kinds of change, and the reviewer reacts to it as to that op (`review/answer-flow#F6.1`'s table):
  - a step that changed the study queue's inputs (an answer, a bury, a suspend, a set due date, a forget, an added or deleted note, a deck, a preset, the selected deck): the queue is built again and its head is shown on its question side; after undoing an answer that is the answered card (`sched/review-session#P9`);
  - a step that changed only notes or note types (a Mark Note, an edit of the note's text or tags): the current card is drawn again on the side it is on, with no queue refresh and no move to the question side;
  - a step that changed only a card's flag ("Set Flag"): only the flag indicator is drawn again, in place.
- Meguri: `U` while review, the congratulations content or the summary has focus, and ⌘Z / ⇧⌘Z once U159 binds the focused undo target, go through U179's single entry point: the session's own step first, otherwise the global step (R365, R366, `engine/undo#M3`). The step names, the Undo and Redo titles and the announcements are `review/reviewer-strings#S5`. With nothing to undo, nothing happens (no alert); the disabled Undo item's reason explains why (U179).
- What Meguri's screen shows after an undo or redo is U179's (`sched/review-session#C15`, `review/answer-flow#F6.2`). It differs from Anki's three cases above: an undone step of the session presents the affected card on its question side, also for a mark, a flag or an edit (F6.2), where Anki redraws in place.

## K6. Audio keys (R232, R422)

- `R` and F5 replay the side on screen: on the question side its question audio; on the answer side its answer audio, preceded by the question audio unless the card's preset skips the question when replaying the answer (`render/audio-playback#A3`). Replay works with autoplay off, while a clip is playing (it starts again) and after every clip of the side has ended.
- `5` pauses the current clip or resumes it; `6` and `7` move it 5 seconds back or forward, clamped to the clip (`render/audio-playback#A4`).
- Meguri: when the side shown has no audio (counting the question audio that the answer side replays), `R` and F5 do nothing. When no clip is loaded (nothing is playing or paused: before the first play with autoplay off, or after the last clip ended), `5`, `6` and `7` do nothing. The matching commands are disabled then (K12, R377), and their keys still do nothing.
- Anki shows its audio menu items enabled in every case, and its keys act on the player whatever it holds.

## K7. Keys while a text field has focus (R244, R425)

- When focus is in a text field of the card (an `input`, a `textarea` or a contenteditable element, `#typeans` among them) or of a panel, typed keys go to the field: no single key of K1 acts, the answer keys and the global keys included.
- Exceptions while the focused field is the card's `#typeans`: Return submits the typed answer and reveals the answer side, through the page's `_typeAnsPress` handler (`render/type-answer`, U66), never through K2 a second time; Esc leaves the field (focus returns to the card, the side does not change) and does nothing else.
- Meguri: ⌘ shortcuts while `#typeans` has focus (R244's "modifier shortcuts"). The field keeps the shortcuts that edit text: ⌘Z and ⇧⌘Z undo and redo the typing, ⌘⌫ deletes to the start of the line, and ⌘A, ⌘C, ⌘X, ⌘V and the ⌘ arrow keys do what they do in any text field. They never undo a review step and never delete the note. These review commands keep their key equivalents: ⌘1 to ⌘7 (flags), ⇧⌘D, ⌥⌘N, ⌥⌘E, ⌥⌘I, and the card zoom keys ⌘+, ⌘− and ⌘0. In a panel's text field, every ⌘ key goes to the panel (K11).
- Anki: characters typed into the type-in field go to the field and do not trigger reviewer keys, and Return there reveals the answer, as here. Esc there also leaves the field: the reviewer's page sends Esc to the main window, which then takes focus from the card page, so the type-in field loses focus and the side does not change (K9).

## K8. Keys of Anki's reviewer that Meguri does not have

| Anki key | Anki action | Meguri |
|---|---|---|
| `v`, Shift+V | replay and record own voice | not in the first version (the plan's scope decision) |
| Korean-layout duplicates of e, m, r, o, i, u, v, Shift+V, Ctrl+Alt+N and Ctrl+Alt+I | the same actions under a Korean layout | replaced by the physical-key fallback (K10, R198, R427) |
| Ctrl+Delete | Delete Note on Windows and Linux | ⌘⌫ only |

## K9. Esc (Meguri, R243, R373)

- Esc closes the topmost transient element first: an open menu, a dialog, a panel or sheet over the review, or a focused card text field (K7). With none open it acts as Back to Decks (`docs/design/native-interface-contract.md` N11.1), with the session end rules of U186.
- Anki: Esc in the reviewer's page only takes keyboard focus away from the page, which matters only in the type-in field (K7). It never leaves review: the user leaves with D or S (K1). Meguri's last step (Back to Decks) is the difference.

## K10. Matching a key (R198, R427)

- A letter binding (`a` to `z`; R420's answer keys can be letters) matches a key-down when the key's character, read without modifiers and with Caps Lock ignored, is that letter, and neither Shift, ⌥, ⌘ nor Control is held. ⇧A is the A key with Shift held and no other modifier; A alone is U159's global Add key. So Shift+J does not answer with a binding `j`.
- A digit or symbol binding (`1` to `7`, `*`, `-`, `=`, `@`, `!`) matches the character the key types, with the Shift or ⌥ the layout needs to type it and with neither ⌘ nor Control held. So `*`, `@` and `!` work whatever the layout needs (Shift on a US layout, its own key on AZERTY, ⌥L for `@` on a German Mac layout).
- Physical-key fallback (Meguri, R427): only while the active keyboard input source cannot type Latin letters (the system's ASCII-capable property is false: Japanese kana, Korean, Chinese, Cyrillic, Greek, Arabic, Hebrew and the like), and only when the typed character matches no binding, a key matches by its position on the US layout. Under a Cyrillic or Korean input source the physical key of `E` then edits while focus is not in a text field (K7). On a Latin layout (US, AZERTY, QWERTZ, Dvorak) there is no fallback: on AZERTY the key in the US `M` position types `,` and does not open the menu, and the key in the US `A` position types `q` and does not add.
- Anki matches characters only and adds Korean duplicates for some keys (K8); the physical fallback is Meguri's (R427's deviation).

## K11. Routing (Meguri, R426, R428)

- The card page sees every key first; Meguri acts only on keys the page did not cancel (`preventDefault`) and that are not auto-repeats (K4). The transport is `render/host-reports`; the fast path and the menu-key mechanism are U181's spike S3 (N11.3).
- Modified shortcuts (⌘ with or without ⇧ or ⌥) are menu commands. Their key equivalents never fire while a panel, sheet or menu owns the keys, or while a panel's text field has focus (N4). While the card's `#typeans` has focus, only the commands K7 lists keep their key equivalents.
- On the iPad, holding ⌘ shows the system overlay listing every K1 key that has a menu command (R428).

## K12. Commands and when they apply (Meguri, R339, R377)

Every K1 action has a menu command showing its key (R339). A command is enabled only while its action applies; the key of a disabled command does nothing after the page has seen it (K11). In Anki, the reviewer's More menu (its card context menu) shows every item enabled; its Edit menu enables Undo and Redo from the undo status and hides Redo while nothing can be redone.

| Command | Applies when |
|---|---|
| Show Answer | the question side is shown |
| Again, Hard, Good, Easy | the answer side is shown |
| Undo, Redo | `engine/undo` peek reports a step (U179) |
| Edit Note, Flag, Mark Note, Bury Card, Bury Note, Suspend Card, Suspend Note, Set Due Date…, Forget…, Card Info, Deck Options, Create Copy, Delete Note | a card is presented, on either side, and the action's unit has attached it (U182, U183, U184) |
| Previous Card Info | the session has a previous card (`stats/card-info#C9`) |
| Replay Audio | the side shown has audio, counting the question audio that the answer side replays, whether or not a clip is playing or has ended (K6) |
| Pause Audio, Audio −5 s, Audio +5 s | a clip of the side is loaded: playing or paused (K6) |
| Auto Advance | `review/reviewer-strings#S6` |
| Card Zoom (−, reset, +) | review is shown |
| Back to Decks | review is shown |

- Every command is disabled while in transition (K1).
- Before U183 and U184 attach, E, O, I, ⌥⌘I, ⇧⌘D, ⌥⌘N and ⌥⌘E do nothing on either side: no panel, no op, the side and the session record unchanged.

## K13. Tests

The oracle cannot run Anki's reviewer, so these rules are checked by Meguri's tests (the files are U181's):

| Test | Covers |
|---|---|
| `ReviewKeyMapTests` | K1 row by row on both sides and in transition, K1.1 per platform (iPhone and iPad ⋯, Mac context menu at the pointer and at the card centre), K2's table (setting on and off, a focused Again button with Space and with Return: one answer each, through the key map with the setting on and through the button with it off), K3 (answer keys on the question side, an empty binding, a rebound rating's old key, Caps Lock and Shift), K4 (auto-repeat, a held Space), K6 (Replay after the last clip ended, Pause with no clip loaded), K8's absent keys, K10 (US, AZERTY and German symbols, Shift+J against `j`, AZERTY's `,` and `q` with no fallback, a Cyrillic and a Japanese kana input source with the fallback, ⇧A against A), K12's enablement table |
| `EscapeStackTests` | K9: the topmost element first, then Back to Decks |
| `ReviewKeyboardUITests` | K7 with `#typeans` focused (a digit types, Return reveals, Esc leaves the field, ⌘1 flags, ⌘Z undoes the typing and not a review step, ⌘⌫ deletes typed text and not the note), K11 with a card that cancels a key |

# Review answer flow

Spec: review/answer-flow
Requirements: R325, R326, R327, R330, R346, R351, R356, R357, R359, R360, R361, R365, R366, R367, R368, R369, R371, R414, R456, R460, R462, R679, R724; reads R41, R353, R354 (F4), R363, R370, R398, R1264, R459 to R464
Anki: 26.09.3 for the paragraphs headed "Anki" (the desktop reviewer, its main window and the collection library behind them); none for the paragraphs headed "Meguri" and for F9
Scenarios: conformance/scenarios/review/ (drafts, not recorded yet, F10)

What the review screen (U175, U178, U179, U187, U188) does around one answer that must match what Anki users know: when the answer timer runs, which inputs reveal and which answer, which rating is the default, what the screen shows after an undo or redo, and how the screen meets a new day and changes made elsewhere. The collection writes are other files' (`sched/answering`, `sched/review-session`, `engine/undo`, `sched/day-clock`); this file says what the screen does with them and names the file for each fact. Layout, styling and controls are the design contract's (`docs/design/native-interface-contract.md` N11, N12.3, N13.3), never Anki's look. Every Anki reviewer string comes from `review/reviewer-strings`.

Terms: the **current card** is item 0 of the session's head (`sched/review-session#C4`). A card is **taken** when the screen moves to a head item that it was not showing (the first card, the next card after an answer, the card an undo or redo brings, a new head after a rebuild). A card is **redrawn** when the screen renders the side of the card it is already showing again.

## F1. Starting and ending a session (R325, R326, R327)

**Anki.**

- Studying a deck first makes it the current deck. The Select Deck op writes `curDeck`, records a "Select Deck" undo step and drops the held study queue. When `curDeck` already holds that deck, it writes nothing and records nothing (`decks/decks#D11`, `#D15.4`). The reviewer then studies the current deck and its descendants (`sched/queue-building#B2`). Anki writes only `curDeck`, never `activeDecks` (D11).
- There is one reviewer. Studying another deck leaves it first.
- Leaving the reviewer writes nothing: every answer was written when it was given.
- The `s` key works on every main-window screen. On the deck overview it starts a new timebox and enters review. On any other screen, the deck list and the reviewer included, it opens the deck overview.

**Meguri.**

- `startSession` does the select-deck write (`sched/review-session#C2` step 3). The write is skip-undo (`decks/decks#D11`, `engine/undo#M2`, R42), so Meguri records no "Select Deck" step and F6.1's last consequence never happens.
- R41's `activeDecks` write is an open point. D11 and C2 step 3 give it to U152 and U175, who must close it for deck-page opening and review start before final app acceptance.
- The engine has at most one active session per open collection (C1). With one open collection, that is R327's one session per device.
- The ways a session ends are the Product Contract's (R325, R90, R208, R22). None of them writes to the collection, and `endSession` only drains what was accepted (C16).
- R327 hands a session that another start ended to the end-of-session summary. That `startSession` has already ended the core session (C1, C16), so R371's undo from this summary has no session to undo in. Open point for U175 and U157: what the summary offers in that case.

## F2. The answer timer (R359, R360, R361, R368, R679)

**Anki.**

1. The timer starts when the reviewer takes a card, just before it draws the card's question side. Every take starts it again from zero. This is also true when the card taken is the card that was just on screen: after an answer, after an undo or a redo, and after any change that made the reviewer rebuild its queue (F6.1's table).
2. A redraw does not start it again. Redraws come from a note edit that adds or removes no card (in the reviewer's editor or on another screen), a note type edit that adds or removes no template, or a change of tags. A note edit that adds or removes cards (a new cloze number), a note type edit that adds or removes a template, and Change Note Type change card rows, so they rebuild and take the card again, as in item 1. Revealing the answer neither stops nor restarts the timer.
3. At the answer input, the time taken is the whole milliseconds from the start to the input, by the wall clock, with nothing removed. The collection stores at most the card's home-preset cap: `min(time taken, cap × 1000)`. `cap` is the "Maximum answer seconds" of the card's home preset (`maxTaken`, 1 to 9999, default 60; `sched/answering#A1`, `#A5`; `sched/presets#V1`). The selected deck's preset does not matter (R330).
4. The answer instant (the review-log id, the review time, the studied-today day) is the instant of the input, taken before the write starts (`sched/answering#A1`).

| Card's home preset cap | Time from take to input | Stored `time` |
|---|---|---|
| 60 s (default) | 7.4 s | 7,400 ms |
| 60 s | 95 s | 60,000 ms |
| 30 s (subdeck preset, parent deck selected with a 60 s preset) | 45 s | 30,000 ms |
| 60 s | 20 s, then a note edit that adds no card, then 10 s | 30,000 ms (the redraw does not restart) |
| 60 s | 20 s, then a note edit that adds a cloze card, then 10 s | 10,000 ms (the rebuild takes the card again) |
| 60 s | answered at 12 s, undone, answered again 8 s after the undo | 8,000 ms for the new answer |

**Meguri.** U175 owns the answer timer; `review/timers#T1` adds the timebox sheet and Auto Advance cases. These are its differences from Anki:

- **Active time (R360, R679).** The timer counts only while a card is presented, and pauses in these cases:
  - iPhone and iPad: while the app's scene is in the background, which includes a locked screen. The inactive state (Control Center, Notification Center, a system alert, the app switcher before the app leaves) does not pause it. Split View, Slide Over or Stage Manager beside another app does not pause it while the scene stays in the foreground.
  - Mac: while the review window is not visible, by its occlusion state: minimized, the app hidden, the window fully covered or on another Space, the screen locked, or the Mac asleep. Deactivation alone does not pause it: while another app is frontmost and the window is still visible, the time counts (U175).
  - Every platform: while the edit panel is open over the card (R679, `review/edit-current#E3`).
  - No other sheet, popover or menu over the review pauses it (Card Info, Deck Options, Set Due Date, Forget, the ⋯ menu, Add over the review). R679 lists only the edit panel, and Anki counts wall-clock time.
- **Restarts.** Every take starts the timer from zero, as in item 1. This includes an undo that presents a card again (R368, F6.2), `CurrentCardChanged{removed}` and a rebuild that takes a new head. When the edit panel closes after a save, the side is rendered again and the timer starts from zero (R398, `review/edit-current#E3`; unlike item 2). When it closes with nothing saved, the timer goes on from its paused value (E3). Other redraws leave it running, as item 2: `CurrentCardChanged{eligible}`, a mark, a flag and F7's new-day state refresh.
- **Cap.** The client caps `msTaken` with the head item's home-preset value. This is the same cap the core applies to the stored `time` (R359, R330). `answeredAtMs` is the input instant (`sched/review-session#C9`, R361), as item 4.

## F3. Revealing and answering (R346, R351, R356, R357, R414)

**Anki.**

- Every card has four answers, Again (1), Hard (2), Good (3) and Easy (4), in that order.
- On the question side, Show Answer, Space and Return (and Enter) reveal the answer. While the type-in box has focus, Space types into it and Enter in it reveals; the box's text is then the typed answer (`render/type-answer`). Answer inputs do nothing on the question side.
- An answer is taken only while the answer side of the card is shown. Taking one moves the reviewer out of the answer state at once, before anything is written. So every further answer input is ignored until the next card has been taken and revealed. A card is never answered twice from one reveal.
- Good is the default answer. The default answer keys are `1` to `4` for Again to Easy. On the answer side, Space and Return answer with the focused answer button, or with Good when no button has focus. A setting allows this and is on by default. Anki stores the answer keys (which can be rebound) and this setting in its global profile settings, so they apply to every profile on that computer.

**Meguri.**

- The four answers and their order are R351's. The gate of R414 is Anki's rule above, held by the session's one phase machine (U175).
- **States validity gate (R356, `sched/review-session#C5`, C10).** Anki does not have this rule. The screen gets new states for the current card only from the session:
  - a head diff that marks item 0 `restate` (C13, or C14's `RolledOver` diff);
  - `CurrentCardChanged{eligible}` or `{memoryOnly}` (C13);
  - a `head` call (C4).

  The gate closes when one of these reports a new `stateToken` for the current card. It also closes when the edit panel closes after a save (`review/edit-current#E3`). It opens again when the screen has drawn the labels that came with the new token. While it is closed, answer inputs are refused, not queued. There is no separate refresh call: the `refreshCard` that the plan's U178 and U188 name is this path. An answer that still reaches the core with an old token gets `Stale` (C10).
- The keys are `review/commands`: Space and Return in `#K2` (R134's device setting takes the place of Anki's global setting), the answer keys in `#K3`, and one press, one action in `#K4`.
- The buttons' look is the design contract's, not this file's: N11.2 (no key hints, which replaces R357), N12.3 (Mac) and N13.3 (touch).

## F4. Interval labels (R353, R354)

**Anki.** The label above each answer is the one `sched/answer-labels` defines. It is computed from the same states the answer applies. Anki shows the labels only while the collection key `estTimes` is true (its default, `storage/new-collection#B4`). When it is false, the buttons show only the answer names.

**Meguri.** The labels come with the head item (`sched/review-session#C4`), so they never wait on a computation.

**Open point, blocking U178: what `estTimes` = false does.** Two rules disagree, and an implementer cannot meet both:

- R354 and the plan's U178 say the labels are always shown. This is a difference from Anki that writes nothing. U178's test list has "estTimes = false → labels still shown".
- R131 and the design contract's N3 (which lists R131) show or hide the labels by the key, as Anki does, and let Settings write it. `review/review-preferences#P4.1` states both branches; until the plan settles it, Meguri follows the R354 branch, which reads and writes nothing.

Antoine decides. Until then, U178 implements and tests only the `estTimes` = true case, and this file states only Anki's rule above.

## F5. Leech notice (R363)

The notice after a leech answer, its two sentences and when each shows: `sched/leeches#L5`, `review/reviewer-strings#S3` and the differences of `#S11`.

Changed 2026-11-23 (leech icon): on screen the notice is one icon in the top controls with its details on a press (`docs/design/native-interface-contract.md` N16.11), not a row above the card.

## F6. Undo and redo on the review screen (R365 to R371, R724, R1264)

### F6.1 Anki

- In the reviewer, `u` and Edit ▸ Undo (Ctrl+Z, ⌘Z on the Mac) both undo the collection's top step, whatever screen made it. Anki's undo is global (`engine/undo#M3` scope `Global`; `review/commands#K5`). Edit ▸ Redo (Ctrl+Shift+Z, ⌘⇧Z) redoes; the reviewer has no redo key of its own.
- Steps carry the names of `engine/undo#A1` and `specs/conformance/undo-ops.md` V4 ("Answer Card", "Bury", "Suspend", "Delete Note"). At most 30 steps are kept (`engine/undo#A2`). Every answer is one step, so 30 undos step back through the 30 newest answers, one card each, and the 31st finds nothing.
- Undoing an answer reverts everything the answer wrote: the card row, its review-log row, the studied-today counters, the siblings it buried, the `leech` tag and a leech suspension (`sched/answering#A2`, `engine/undo`).
- An undo or a redo reports the kind of the step it undid or redid. The reviewer reacts to that kind and to what the step changed, by the first row that matches. It reacts the same way to an op made on another screen (F8).

  | The step, or the op | The reviewer then |
  |---|---|
  | an answer, undone | takes the queue's head, which is the answered card again (`sched/review-session#P9`: the entry goes back to the front, and the counts are those from before the answer), on its question side, timer from zero (F2) |
  | an answer, redone | takes the queue's head: the card that follows it (P9's redo) |
  | changed card rows, by any step other than "Set Flag" (bury, suspend, forget, set due date, Add Note, Delete Note, Change Note Type, a note edit that adds or removes cards, a note type edit that adds or removes a template); or changed a deck row; or changed a preset; or changed config by "Select Deck", "Preferences", "Deck Options" or "Toggle Load Balancer" | drops its queue, builds it again and takes the new head. The restored card takes its place in the new build and is not always first: a learning card that came due in the meantime goes before it (scenario `review/undo/bury-position`) |
  | otherwise changed a note or a note type (Update Note that adds no card, a mark, a template or styling edit that adds no template) | redraws the current card on the side it was on |
  | otherwise changed card rows ("Set Flag") | updates the flag indicator |
  | anything else (a tag-list op such as "Clear Unused Tags", a config change by another op) | nothing |

- When a take finds no card, the reviewer leaves for the deck overview's finished (congratulations) state. Examples: the redo of the last answer, or a rebuild after the last card was buried or suspended.
- After an undo, Anki shows "<step name> undone" as a tooltip, and after a redo "<step name> redone" (`review/reviewer-strings#S5`).
- Anki's congratulations page is the deck overview's finished state. An undo there refreshes the overview and stays on it: it does not return to the reviewer. When the undone answer made a card due again, the overview shows the counts and its Study Now button. Studying again takes that card first, because the undo put it back at the front of the held queue.
- The previous card after an undo: `stats/card-info#C9` (Anki: the card shown before the undo).
- A new day empties both histories (`sched/review-session#P10`, `engine/undo#M6`).
- When the session's answers have all been undone, the next undo is often the "Select Deck" step that started the study (F1). Undoing it puts back the deck that was current before. Its kind makes the reviewer rebuild (third row), so the reviewer then studies that earlier deck.

### F6.2 Meguri

- **Controls (R365, R724).** On iPhone and iPad, undo and redo are the review toolbar's Undo and Redo items (N13.3). The "back button" of R365 and R366 is that Undo item. The Mac has no on-screen undo or redo, only Edit ▸ Undo and Edit ▸ Redo (N11.1). On iPhone the ⋯ menu also holds Redo while a step can be redone (R724, `review/card-actions`).
- **This session's steps.** A step belongs to the session when its op names the session (`engine/undo#M1`): the session's answers, and the ops started from the review screen that `sched/review-session#C13` lists (bury, suspend, flag, forget, set due date, mark, Delete Note, edits). Add Note from Add over the review (R90) and a Deck Options save over the review do not name it.
- **Scopes.** The touch Undo item undoes only this session's top step (scope `Session(id)`). While the top step is not this session's, `peek` reports `None` and the item is disabled, also when session answers are below that step. U, ⌘Z and Edit ▸ Undo with review focused undo with `Session(id)` when the top step is this session's, and with `Global` otherwise (U179; `engine/undo#M3`; C15). So a step made outside the session (Add Note over the review) is undone as Anki would undo it, by U or ⌘Z. Redo, from any control, uses `Session(id)` (U179).
- **After an undo.** The screen shows:
  - for an answer, the answered card on its question side, timer from zero, as F6.1's first row (R365, R368);
  - for another step of this session, the **affected card**: the card that was current when the step was made. The screen presents it on its question side with the timer from zero (R365, R368, U179). This is also true when it is still the current card on its answer side (a mark, a flag or an edit undone). For a step of several cards (Bury Note, Suspend Note, Delete Note), the affected card is still that one card. This differs from F6.1's third, fourth and fifth rows. The mark and flag ops themselves still redraw in place (F2, C13.1); only their undo presents the card again;
  - when the undone step leaves no card for the session to show, the current card and side stay, and only the counts and labels change, from the head diff (U179). Examples: undoing Add Note removes the cards it added; a Deck Options save changes no card row; the affected card is not presented by this session's queue;
  - when the head after the undo presents nothing, the congratulations content (`sched/review-session#C17`).
- **Open point for U43, before U179: the affected card first.** No core contract puts the affected card first after an undo, except for an answer's undo (C15) and Delete Note's undo (C13.1). For the other session steps, C15 returns a head diff as C13 does: a fresh build under C4's current-card rule. That rule keeps first the card that is current at the time of the undo, not the restored card. `engine/undo#M3`'s `peek` also names no card. The owner of `sched/review-session#C15` must specify that this head diff puts the affected card first while the queue presents it. Until then, U179 tests this rule with a fake core only.
- **Redo** shows the head that follows, as F6.1's second row (R369), or the congratulations content when nothing follows.
- **From congratulations or the summary.** An undo there reopens review on the restored card's question side (R371, AE4). This differs from Anki, which stays on the overview (F6.1). R371's "Anki returns from the congrats screen to the reviewer" does not hold for 26.09.3 desktop.
- **Names.** The touch Undo item's name is "Undo" with the step's name (`review/reviewer-strings#S5`). Meguri announces an undo and a redo instead of showing a tooltip (S5, S11).
- **When undo is disabled.** The disabled touch Undo item gives `peek`'s reason (`engine/undo#M3`) as its accessibility hint (R365, R366, R462). The texts for the eight reasons are `review/reviewer-strings#S16`. On the Mac, Edit ▸ Undo is disabled with `review.undo.plain` (S5). Where the Mac gives the reason is not decided: open point for U159 and U179 against the design contract.
- **Limits.** The 30-step limit and the undoable set are the core's (`engine/undo#M1`, `#A2`); the screen never overrides them (R367).

## F7. A new day during review (R459 to R464)

**Anki.**

- The main window sets a one-time timer for the day's cutoff, and sets it again each time it fires.
- When the timer fires during review and the cutoff has changed, the reviewer rebuilds at once. Its queue read sees the new day: it drops the queue and both histories, and it unburies cards (`sched/review-session#P10`, `sched/day-clock#D7`).
- The reviewer then takes the new head on its question side, timer from zero (F2). It does so also when it was showing an answer side, and also when the new head is the same card.
- The card on screen keeps its old states only between the cutoff and the moment the timer runs. The timer can run late, for example after the Mac or PC was asleep. An answer in that interval uses the old states, and the next queue read drops the queue (P10).

**Meguri.** These rules come from `sched/review-session#C14`, `sched/day-clock#D9` and `sched/app-coordination#A1` (when the app checks). They differ from Anki (R460):

- At `RolledOver` the current card stays, on the side it was on, and gets the new day's states and labels at once. On the answer side, only the buttons redraw; on the question side, nothing visible changes. This is a redraw, so the timer goes on (F2). Anki instead takes the new head on its question side with a new timer.
- The rollover op empties both histories at once (D9 step 1). So the touch Undo item shows `ClearedByRollover` from then on, and the answer given across the rollover cannot be undone either (C14). R462's text for that reason is `review.undo.unavailable.clearedByRollover` (`review/reviewer-strings#S16`).
- A time-zone change that changes the day number takes the same path (`sched/day-clock#D12.1`, R464).
- The session stays one session (R463).
- `sched/review-session#P11`'s row for this case says Anki does "nothing until the next read". That is true only of the interval above; this has been reported to its owner.

## F8. Changes made elsewhere (R456, R457)

**Anki.**

- After an op on another screen (the browser, the editor, an add-on), the reviewer reacts to the op by F6.1's table: it rebuilds and takes the new head, redraws, updates the flag, or does nothing. A take restarts the timer (F2), also when the new head is the card that was shown.
- When the op ran in another window, the reviewer responds only when the main window has focus again. Until then the review page is shown faded. The take, and its timer start, come at that point.
- A collection sync ends with a refresh that reports every kind of change. So after any sync, also one that changed nothing or failed, the reviewer rebuilds, takes the head again and starts the timer from zero.

**Meguri.** The rules are `sched/review-session#C13` and C4:

- The current card stays first while the rebuilt queue still presents it, so a rebuild does not take a new card.
- `CurrentCardChanged{eligible}` redraws the side on screen. `{memoryOnly}` only refreshes the labels. `{removed}` takes the new head with `review/reviewer-strings#S10`'s notice.
- An iCloud apply is one change set that C13 classifies like any other, not a refresh of everything.
- The differences from Anki are listed in `sched/review-session#P11`.

## F9. Meguri-only rules (no Anki behavior)

These requirements of the same units reproduce no Anki behavior. The Product Contract, the design contract and the files named are their whole specification:

- the screen's regions, presentation per platform and idle lock (R331, R332, R336 to R338, R340), and the session record and its persistence (R440, R441);
- the reveal and answer budgets (R347, R348, R1809, R1810, R1811, R1817) and commit failure (R362, `sched/review-session#C11`);
- tap zones, their tint, exempt targets and Double Tap Prevention (R401 to R413, R415, R416, R1816; `render/host-reports`, `render/card-runtime`), which follow AnkiMobile, not the desktop reviewer;
  - Decided 2026-11-22 by Claude (coordinator) under Antoine's delegation, where those requirements leave a point open (`docs/design/decisions.md`, row `review.answer` of 2026-11-22; built in PR #120). On iPhone and iPad a tap anywhere on the card reveals, and on the answer side the left half answers Again and the right half Good; the answer buttons stay. Besides R405's elements, a tap never reveals or answers on an element styled as clickable (`cursor: pointer`) or one with its own tap listener below `<body>`, or when the card cancelled it (R406). Scrolls, long presses, second fingers and a tap that stops a scroll never count. Double Tap Prevention's window (R416, 250 ms by default) also applies right after the answer side appears: an answer tap whose touch begins within the window after the answer was shown does nothing. With VoiceOver or Switch Control on, the answer side's zones are off and the buttons answer (R184 as changed 2026-11-23, R1981); PR #120 checks both.
  - Built in PR #120, from its code and its review (not a decisions row): a tap acts only when the touch Meguri measured is matched with a page report by the rules below, which tie a report to its own tap; a match the rules cannot confirm fails closed, so a missed tap is preferred to a wrong reveal or answer. Each of the two waits at most 1 s, from its arrival, for the other. They match only when the page's point is within 8 pt of where the finger lifted. A single-finger touch that the page may report (not a scroll, a second finger or a cancelled touch) and that is not matched counts as unmatched for 1.5 s after it lifted: when a new touch began before its report, when a refused match consumed it, or when it waited out. A report arriving while no touch waits and a touch is unmatched is that touch's late report: it is dropped and the touch accounted for. While a touch is unmatched, the next match is refused unless that page already has a timing baseline (an earlier match) and the two lifts are at least 0.3 s apart; a refused match leaves its own touch unmatched in turn. Between two matches of a page, the time between the lifts on the touch clock and on the page's clock may differ by at most 0.15 s; otherwise the match is refused and the page's baseline is dropped, so the next match starts a new one (changed 2026-11-22, PR #129: a touch consumed by a refused match stays unmatched, and a late report with a baseline is refused when the lifts are under 0.3 s apart, which closes both gaps of the earlier text);
  - Decided 2026-11-22 by Claude (coordinator) under Antoine's delegation (`docs/design/decisions.md`, row `review.answer`; native-interface-contract N16.9; built in PR #129): the card runs on under the answer area, and a tap or click in the answer area that misses its buttons does nothing: the area takes it, so it reaches neither the card's page nor the tap zones;
  - Changed 2026-11-23 (decided by Claude (coordinator) under Antoine's delegation; `docs/design/decisions.md`, row `review.answer` of 2026-11-23): R184 names only the two modes an app can detect on iOS, VoiceOver and Switch Control, both read from UIKit. iOS 27 offers apps no public way to know that Voice Control or Full Keyboard Access is on, so those two cannot switch the zones off; the answer buttons stay the accessible way to answer in every mode, and Voice Control names them by their visible labels (R185). Before, R184 named all four and the two undetectable ones were an open item in `docs/process/verification-status.md`;
- background, page termination and relaunch (R452 to R455; `render/card-behaviors`), whose "same scheduler day" is the core's `today` (`sched/day-clock`);
- the one-way sync wait (R458; `engine/lifecycle`, `sync/one-way`);
- accessible answering (R184, R185, R349, R1981, R1982; `review/reviewer-strings#S13`).

## F10. Scenarios

Drafts under `conformance/scenarios/review/`. They are not in the `active` list of its `area.yaml` and have no golden yet; they are recorded when the oracle is free (`tools/mg oracle record`):

| Scenario | Checks |
|---|---|
| `review/timing/answer-cap-home-preset` | F2 row 3: in a queue held on the parent deck (60 s cap), the subdeck card's stored time is capped by its own home preset (30 s), and the parent's cards by 60 s (R330, R359) |
| `review/undo/thirty-answers` | F6.1: 31 answers, 30 undos bring the cards back newest first, the 31st is refused (R367) |
| `review/undo/bury-position` | F6.1 third row: after undoing a bury, a learning card that came due goes before the restored card in Anki's rebuilt queue (the Anki side of R365's difference) |

The screen rules are not observable through the oracle: F2's active time, F3's gates, F6.2's affected card first and F7's redraw. U175, U178, U179 and U188 test them with a fake core, as their units list.

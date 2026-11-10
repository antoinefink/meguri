# Auto Advance

Spec: review/auto-advance
Requirements: R432, R433, R755, R756; supports R330, R414, R1854
Anki: 26.09.3 for A2 to A7 (the desktop reviewer's Auto Advance: which preset it reads, when each countdown starts and ends, Wait for audio, the actions, turning it on and off), except the differences A9 lists; none for A8
Scenarios: none. Auto Advance lives in Anki's desktop reviewer, which the oracle does not run (`conformance/AGENTS.md`). The ops an action runs (answer, Bury Card) are covered by their own specs and scenarios (A6).

Auto Advance reveals the answer and then acts on the card after fixed delays, so a user can study hands-free. U185 implements it; its toasts and the disabled explanation are `review/reviewer-strings#S6`. This file gives the rules behind them.

## A1. Terms

- **Auto Advance state**: on or off, held by the open review session only (A3).
- **Question countdown**: runs while a card's question side is presented; its delay is `seconds_to_show_question`; when it ends, the **question action** applies.
- **Answer countdown**: runs while a card's answer side is presented; its delay is `seconds_to_show_answer`; when it ends, the **answer action** applies.
- **Set**: a time is set when it is a finite number above 0 (`review/reviewer-strings#S6`). A side whose time is not set runs no countdown.
- **Presentation**: the instant a side becomes visible and takes input, the presentation event of U176 (`presented(card, side, time)`). A question under the timebox sheet is not presented for this file until Continue (A4).

## A2. Values (R330)

- Every value comes from the current card's home preset (`sched/presets#V1`, `#V2`), as the head item carries it (`sched/review-session#C4`, home-preset values): a card of a subdeck with its own preset uses that preset, never the studied deck's. A card of a filtered deck would use its original deck's preset.
- When no preset has the home deck's `config_id`, every value of this file comes from the default preset (id 1), and from the built-in defaults (`decks/deck-presets#P2`) when there is no preset 1 either. This is the reviewer's own path, which `sched/presets#V1` leaves to its unit; the answer's stored-time cap still follows the answer path's rule (V1, built-in defaults).
- A countdown's delay is the value at the moment the countdown starts. The action and Wait for audio are the values at the moment the countdown ends. When the core restates the current card (a preset changed elsewhere, `sched/review-session#C13`), a countdown already running keeps its delay.
- The delay in milliseconds: the stored `f32` is written as its shortest round-trip decimal, that decimal is read as a binary64 number, multiplied by 1000 in binary64 and truncated toward zero. Most values the deck options editor stores (tenths, `sched/deck-options#O3`) give the exact delay, and some give 1 ms less: 2.3 → 2,300 ms, 0.1 → 100 ms, 9999 → 9,999,000 ms, 32.3 → 32,299 ms, 64.1 → 64,099 ms, 2.01 → 2,009 ms.
- A set time below 0.001 s gives 0 ms: the countdown ends at once, and its action follows the presentation without a visible delay. A delay above 2,147,483,647 ms (a set time above about 24.8 days, which only a synced or imported preset can hold, `sched/presets#V3`) is clamped to 2,147,483,647 ms (Meguri rule).
- `question_action` and `answer_action` take the values of `sched/presets#V4`. A number outside that list acts as 0: Show Answer and Bury Card.

| `question_action` | Question action |
|---|---|
| 0 | Show Answer |
| 1 | Show Reminder |

| `answer_action` | Answer action |
|---|---|
| 0 | Bury Card |
| 1 | Answer Again (rating 1) |
| 2 | Answer Good (rating 3) |
| 3 | Answer Hard (rating 2) |
| 4 | Show Reminder |

There is no Answer Easy action. Note that 2 is Good and 3 is Hard.

## A3. Turning it on and off

- Auto Advance is off when a session starts (`sched/review-session#C2`). It is not a preference: nothing stores it, nothing syncs it, and a new session starts with it off whatever the last one did.
- ⇧A (`review/commands#K1`) and the Auto Advance menu item (`review/card-actions#A13`, checked while it is on) turn it on and off (R432). Each turn shows S6's toast. While it is off and S6's disabled rule applies, neither turns it on.
- Turning it on starts the countdown of the side presented now, with the full delay, counted from that instant (not from the side's presentation). Turning it on while the answer side is presented starts the answer countdown.
- Turning it off cancels the countdown that runs or waits (A4); nothing acts.
- It turns itself off, with no toast (S6):
  1. when a countdown ends and the review screen does not have the user's attention (A5 step 3);
  2. when the review screen is left: the session ends, the summary is shown, or the congratulations content replaces the card (the applied head is `Finished` or `WaitingUntil`, `sched/review-session#C17.1`). Continue or an unbury on the congratulations content (`review/reviewer-strings#S8`, `#S9`) presents a card with Auto Advance off;
  3. at once when the iOS or iPadOS app leaves the foreground, when the Mac app deactivates, and when, on the Mac, another window becomes key while the app stays active: a Meguri window that is not attached to the review window (Browse, Card Info or Settings in its own window, a second main window). The review window's own sheets, popovers, alerts and menus are not such a window; they are A5 step 3's and A8's cases (R433, R755, A8).
- Nothing else changes it. It stays on across answers, undo and redo, bury, suspend, flag, mark, the timebox sheet, a day rollover, edits and changes made elsewhere.
- The toggle applies only while a card is drawn: a side is presented, or a question waits under the timebox sheet. On the congratulations content and the summary no card is drawn: the ⋯ item and ⇧A are disabled as the card commands of `review/commands#K12` are, with no toast and without S6's explanation (Auto Advance is off there, item 2). Under the timebox sheet S6's disabled rule reads the preset of the card under the sheet; turning Auto Advance on shows S6's toast and starts no countdown, and the question's countdown starts at Continue (A4).

## A4. When a countdown runs

- With Auto Advance on, each presentation of a side starts that side's countdown when its time is set: a new card's question, the answer after a reveal by any input (the question action included), the question of a card an undo restored, and a side drawn on screen again after a re-render (the edit panel closing, R398; a change made elsewhere that re-renders the current card, `sched/review-session#C17` `rerender`). What counts is the side being drawn again, not the `rerender` mark. A head diff that does not draw the side again leaves a running countdown alone: a restate or relabel of the current card (a rollover, `sched/review-session#C14`), and a Mark Note or flag change, whose `rerender` mark only updates its indicator in place (`review/card-actions#A2`, `#A3`, `sched/review-session#C13.1`).
- The timebox sheet (`review/reviewer-strings#S7`, R430): the question under it is presented when Continue is chosen, so its countdown starts then, with the full delay.
- At most one countdown exists. Starting one cancels any other. Leaving the side cancels it: a reveal, an answer, a bury, suspend or delete of the card, an undo that presents another card, the card leaving the head (`CurrentCardChanged{removed}`), the end of the session.
- A countdown is one single-shot deadline, never a timer that fires repeatedly (R1854). It measures elapsed time on a monotonic clock. Auto Advance is off whenever the app is in the background (A3), so background time never reaches a countdown.

**Wait for audio.** When the countdown ends, `wait_for_audio` is true and native card audio is playing on the card surface (`render/audio-playback`: a queue entry playing or waiting, including a clip of the previous side that interrupt-off left playing, a replay the user started and a paused clip), the action does not run yet. It runs when the audio controller next reports its queue idle, after A5's checks. The queue is idle when no entry is playing, paused or waiting. It becomes idle when its last entry ends, fails or is skipped, and when the queue is stopped (a stop, a side transition with interrupt on, a replay of an empty list), never through a pause. U185 needs U67's controller to report that transition. `render/audio-playback` does not name such a report yet. So the action comes at the later of the delay's end and the audio's end:

| Delay | Audio from presentation | Action at |
|---|---|---|
| 3 s | one 4 s clip | about 4 s, when the clip ends |
| 5 s | one 2 s clip | 5 s |
| 3 s | a 2 s clip, and the user replays it at 2.5 s | about 4.5 s, when the replay ends |
| 3 s | a 4 s clip paused at 1 s | when the clip is resumed and ends, or is stopped |
| 3 s | none (autoplay off, or no `[sound:]` on the side) | 3 s |

- Audio that ends before the delay ends changes nothing. HTML `<audio>` and `<video>` elements of the card are not native card audio and are never waited for (`render/audio-playback#A8`).
- With `wait_for_audio` false the action runs at the delay's end; the audio goes on or stops as `render/audio-playback#A2` says for the transition the action makes.
- This overrides U185's approach ("a countdown starts only after U67's audio queue reports completion") and its test of a 4 s question clip: the countdown starts at presentation and the audio only delays its action. With a 3 s delay and a 4 s clip the action comes at about 4 s (the table's first row), not at 7 s.

## A5. When a countdown ends

In this order:

1. When Auto Advance is off, or the card is no longer the current card on the side the countdown belongs to, nothing happens.
2. Wait for audio (A4): when it applies, stop here. When the queue becomes idle, run these steps again from step 1.
3. When the review screen does not have the user's attention, Auto Advance turns off and nothing acts. It does not have it while the app is not active, while another window is key (A3 item 3), or while a sheet, panel, alert, confirmation dialog or popover is presented over it: the Add sheet, the edit panel, the panels of `review/card-actions#A12` (Set Due Date…, Forget…, Card Info, Deck Options and the others), Settings, a share sheet. A menu of the review screen is A8's case.
4. The action runs (A6).

When Meguri shows an operation's blocking progress over review at the moment a countdown ends, the countdown waits until the progress goes away and then runs these steps from step 1. This also applies while the action waits for audio. Anki retries such a countdown every 100 ms until its progress window closes.

## A6. Actions

| Action | What happens |
|---|---|
| Show Answer | the answer is revealed exactly as the Show Answer input does it (`review/answer-flow#F3`, U178), the type-in comparison included (`render/type-answer`); the answer countdown then starts (A4) |
| Show Reminder (question) | `review.toast.questionTimeElapsed` (S6) shows; nothing else. The question stays, and no further countdown runs on it; the answer countdown starts when the user reveals |
| Bury Card | the Bury Card command on the current card (`review/card-actions#A4`): `sched/manual-ops#M2` in card mode, with its toast (`review/reviewer-strings#S4`, "1 card buried."), its undo step (`Bury`) and its head diff, exactly as the user's Bury Card |
| Answer Again, Answer Hard, Answer Good | an answer with that rating through the session's answer gate (R414), exactly as the button: `sched/review-session#C9` with `answeredAtMs` the instant the action runs and `msTaken` the answer timer's value then (`review/answer-flow#F2`, `review/timers#T1`). Both caps apply as for the button: the client caps `msTaken` with the head item's home-preset maximum answer seconds (F2), and the core caps the stored time (`sched/answering`, R756). It is an accepted answer for every rule: the leech toast (S3), the timebox (S7), the session record and its undo step |
| Show Reminder (answer) | `review.toast.answerTimeElapsed` (S6) shows; nothing else. The answer stays until the user answers |

- When the gate refuses the answer (another answer was accepted first, `Stale` or `NotAtHead`, `sched/review-session#C10`), the action ends there; a fresh head that presents a side starts that side's countdown (A4).
- A reminder shows once per countdown. Audio that ends on that side after the reminder shows nothing more (A9).

## A7. Examples

Question 3 s, answer 5 s, question action Show Answer, answer action Answer Good, Wait for audio off, unless a row says otherwise.

| Situation | Outcome |
|---|---|
| question presented at 0 s | revealed at 3 s; answered Good at 8 s; the next question's countdown starts at its presentation |
| the user reveals at 1 s | the answer countdown starts at 1 s; Good at 6 s |
| the user answers Hard at 7 s | Hard is recorded; nothing is answered at 8 s |
| answer action Bury Card | at 8 s the card is buried with "1 card buried." and the next card is presented |
| question action Show Reminder | "Question time elapsed" at 3 s; the question stays; after a manual reveal at 10 s, Good at 15 s |
| Auto Advance turned on at 2 s on the question | revealed at 5 s |
| the Add sheet is open over review at 3 s | Auto Advance turns off; the question stays |
| the subdeck card's preset has question 0 and answer 0 (Auto Advance on) | no countdown and no toast; the next card with a set time counts down (S6) |
| the answer at 8 s crosses the timebox | the sheet is shown over the next question; that question's countdown starts when Continue is chosen |
| the user undoes the Good at 9 s | the card's question is presented again and its countdown starts again: revealed at 12 s |
| the queue is empty after the answer at 8 s | congratulations content; Auto Advance is off |

## A8. Meguri rules (no Anki behaviour)

- R433, R755: Auto Advance turns off at once when the app leaves the foreground, the Mac app deactivates or another Mac window becomes key, not only when a countdown ends (A3 item 3).
- The 2,147,483,647 ms clamp on a delay (A2).
- The toggle is disabled from the current card's preset with an explanation (`review/reviewer-strings#S6`).
- A menu of the review screen (the ⋯ menu, the card's context menu, the Mac Card menu) open when a countdown ends: the action waits until the menu closes, then A5 runs again from step 1, so a menu choice that turned Auto Advance off, revealed, answered or buried the card wins and the waiting action does nothing.
- Auto answers go through the same answer gate as every input (R414); they record the answer timer of `review/answer-flow#F2`, which excludes background time (R360).

## A9. Differences from Anki

- Anki never disables the toggle (S6, S11).
- Both turn Auto Advance off when review is left (A3 item 2). For attention, Anki turns it off only when a countdown ends while keyboard focus is not in its main window. Meguri also turns it off at once when the app goes to the background or deactivates, or when another Meguri window becomes key (R433, R755, A3 item 3).
- Anki's automatic Show Answer does not read the type-in box, so its comparison does not see what was typed; Meguri's reveals as the Show Answer input does (A6).
- Anki checks its audio queue about 100 ms after the last clip ends; Meguri acts on the idle report.
- Anki's reviewer reads the times through the schema-11 JSON, where a NaN or infinite time is written as null and runs no countdown, as in Meguri. For a negative time Anki creates a countdown whose timer never starts, so it never ends by itself: its action runs only when a clip ends on that side and the audio queue is empty about 100 ms later, whatever Wait for audio says. Meguri runs no countdown for a time that is not set (S6).
- Anki checks again whenever a clip ends on a side whose deadline has passed and whose countdown was not cleared. After a Show Reminder the countdown is not cleared, so each later clip end on that side (a replay with R, a clip still playing) shows the reminder again. Meguri shows it once (A6).
- Anki has no rule for its menus; Meguri's is A8's.

## A10. Tests

`AutoAdvanceControllerTests` (U185), with a fake clock, a fake audio controller and a fake session:

- A2: the subdeck card's own preset values; a missing preset read from preset 1, then the built-in defaults; the delay kept when the card is restated while a countdown runs; 2.3 s → 2,300 ms, 32.3 s → 32,299 ms, 0.0001 s → 0 ms, the clamp; answer action 2 → Good and 3 → Hard; an unknown action number → Show Answer or Bury Card.
- A3: off at session start; ⇧A and the item on a question and on an answer side (the countdown counted from the toggle); off cancels; off when congratulations content is applied, after the summary, on background, on deactivation and when another Mac window becomes key (not for the review window's own sheet); still on after an undo, a rollover and the timebox sheet; the toggle disabled with no card drawn, and turned on under the timebox sheet.
- A4: each presentation kind starts a countdown; a restate without presentation, a Mark Note and a flag change do not; one countdown at most; every row of the Wait for audio table; HTML media not waited for.
- A5: the order of the checks, including a sheet presented while the action waits for audio, and a blocking progress at the deadline.
- A6: each action, the Bury Card toast and undo step, the answer's `answeredAtMs` and `msTaken`, a refused answer, one reminder per countdown, also when a replay ends after it.
- A7: every row.
- A8: a menu open at the deadline, closed with and without a choice.

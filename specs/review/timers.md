# Review timers

Spec: review/timers
Requirements: R364, R754, R86, R429, R431; supports R359, R360, R368, R430, R679, R756, R1854
Anki: 26.09.3 for T2 (the desktop reviewer's on-screen timer) and T3's start points (when Anki starts its timebox), except the differences T4 lists; none for T1 (`review/answer-flow#F2` holds the Anki answer timer)
Scenarios: none. These clocks live in Anki's desktop reviewer, which the oracle does not run (`conformance/AGENTS.md`). The cap on the recorded time is `sched/answering`'s, with its own scenarios (`sched/answer/deck-counters`, `sched/data/preset-bounds`).

Three clocks run during review: the answer timer, whose value each answer records; the on-screen timer, which shows it; and the timebox clock. U175 owns the answer timer (`review/answer-flow#F2`), U177 the on-screen timer, U185 the timebox. This file states how they start and stop and how they depend on each other, so that the three units agree.

Changed 2026-11-19 (T2): two clocks run, the answer timer and the timebox clock. Meguri has no on-screen timer and no setting for one. U177's timer is removed; R364 and the display side of R754 are withdrawn by Antoine's decision.

## T1. The answer timer (R359, R360, R368, R430, R679)

- `review/answer-flow#F2` defines the answer timer: when it starts and starts again, the active time it counts (R360, R679), and the cap on the `msTaken` it sends (`sched/review-session#C9`, `sched/answering`). The edit panel's close is U184's (R398). This section adds only the following.
- Under the timebox sheet (`review/reviewer-strings#S7`) the next card's timer starts when Continue is chosen, not when its question is drawn under the sheet (R430).
- A reveal does not stop or restart it, whether the user reveals or Auto Advance does (`review/auto-advance#A6`): an Auto Advance answer records the same time a button would at that instant.

## T2. The on-screen timer (Removed 2026-11-19)

Removed by Antoine's decision of 2026-11-19 (`docs/design/decisions.md`, `review.question`; `docs/design/native-interface-contract.md` N16.7): "I never use the timer so we can remove it entirely and remove it as an option". The id T2 is not reused.

- Review shows no timer on any platform, for any preset. No view, presentation model, capped state, per-second schedule, string or test of it exists.
- Nothing in Meguri offers one: Deck Options has no control for `show_timer` or `stop_timer_on_answer` (`sched/deck-options#O3`, where both are hidden options), and no preference, menu command or review control stands in for them.
- **The stored fields are preserved untouched.** `show_timer` and `stop_timer_on_answer` stay in every preset with the values they hold (`sched/presets#V2`). Meguri's review never reads them. No Meguri operation writes them: a Deck Options save, an optimizer commit, a preset copy and sync carry the stored bytes as they are, and a preset Meguri creates gets the defaults of `sched/presets#V2` (both false). Another Anki client of the same collection shows or hides its own timer from them, as before.
- **Answer time is measured as before.** The answer timer (T1) counts the active time of every card and each answer records it, capped by the home preset's `cap_answer_time_to_secs` (`sched/presets#V3`, `sched/answering`). Maximum answer seconds stays an option of Deck Options. Nothing about the recorded `msTaken`, the review log or the statistics changes.
- The session's progress bar (N16.1) is not a timer and reads no clock.

## T3. The timebox clock (R86, R429, R431)

`review/reviewer-strings#S7` gives the trigger, the card count, the minutes, the sheet and the rules for time with no card presented. This section adds when the clock starts and restarts, and which time it counts.

- `timeLim` is the collection key of `sched/config-keys` (seconds, 0 = off). A session starts with the value `sched/review-session#C2` returns; a change during the session applies from the next check without resetting the clock (`review/review-preferences#P3`).
- The clock starts at 0 s with a card count of 0 when a session presents its first card. Each new session starts a new timebox. The summary's Continue dismisses the summary and starts a new session on the same deck (U157), so its timebox starts from 0. Anki starts its timebox each time study starts from a deck: the deck overview's study button, the study key, and the top toolbar's Study link while the deck overview is shown.
- It restarts at 0 s and 0 cards:
  - on the timebox sheet's Continue (S7);
  - at a crossing that shows no sheet (S7);
  - when the summary that the timebox sheet's Finish opened resumes the open session through its Undo or Unbury (U157). This is S7's "resume from the summary after Finish".
- Nothing else restarts it. The clock goes on from its value after the congratulations content's Continue (R437, S8), an unbury there (R435), and an undo or unbury from the congratulations content or from a summary that the queue running out or ✕ opened (S7). A redo, a rollover, a bury, an edit, a change made elsewhere and Auto Advance do not restart it either.
- Decided 2026-11-10: it counts while a card is presented in the open session (a side visible and taking input, `review/auto-advance#A1`; a question under the timebox sheet is not presented) and the app is active in R431's sense. It does not count while the iOS or iPadOS app is in the background or the screen is locked, or while the Mac app is hidden or minimized (the exclusions of R360). It keeps counting while the edit panel, the Add sheet, a panel of `review/card-actions#A12` or another sheet is over review, and while the Mac app is deactivated but visible. Unlike the answer timer, it does not pause for the edit panel (R679 is the answer timer's). It stops while no card is presented (S7). U185 builds it on U175's `ActiveTimeClock` for the background exclusions, without the edit-panel pause.
- Its elapsed time and card count are part of the session snapshot (U187), so backgrounding and relaunch within the session keep them.

## T4. Differences from Anki

- The answer timer's differences are `review/answer-flow#F2`'s; T1 adds that Anki shows its timebox dialog before it draws the next card, so its timer starts after the dialog too.
- Anki's desktop reviewer shows an on-screen timer when the preset asks for one; Meguri shows none and offers no option for it (T2, decided 2026-11-19). The two earlier items here, on the counter and its minutes format, went with it.
- Anki's timebox counts wall-clock time and starts again each time study starts from a deck (T3); Meguri's counts active time per session and stops while no card is presented (S7, S11).
- Anki's Finish returns to the deck list; Meguri's opens the session summary (R86, R430).

## T5. Tests

| Test file (unit) | Covers |
|---|---|
| `ActiveTimeClockTests` (U175) | T1: the start at Continue under the timebox sheet; no restart at a manual or automatic reveal (the rest of the timer is `review/answer-flow#F2`'s tests) |
| `OnScreenTimerTests` (U177) | removed 2026-11-19 with T2. In its place, a review test checks that a card whose preset has `show_timer` true shows no timer element, that the answer still records its capped time (T1), and that the preset's `show_timer` and `stop_timer_on_answer` bytes are unchanged after a session and after a Deck Options save |
| `TimeboxControllerTests` (U185) | T3: start at the first presented card, the restarts and the non-restarts (a summary opened by Finish against one opened by the queue running out), the time counted with the edit panel and the Add sheet open and not counted in the background, the snapshot across relaunch (a changed `timeLim` is `review/review-preferences#P3`'s case) |

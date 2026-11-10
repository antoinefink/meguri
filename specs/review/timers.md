# Review timers

Spec: review/timers
Requirements: R364, R754, R86, R429, R431; supports R359, R360, R368, R430, R679, R756, R1854
Anki: 26.09.3 for T2 (the desktop reviewer's on-screen timer) and T3's start points (when Anki starts its timebox), except the differences T4 lists; none for T1 (`review/answer-flow#F2` holds the Anki answer timer)
Scenarios: none. These clocks live in Anki's desktop reviewer, which the oracle does not run (`conformance/AGENTS.md`). The cap on the recorded time is `sched/answering`'s, with its own scenarios (`sched/answer/deck-counters`, `sched/data/preset-bounds`).

Three clocks run during review: the answer timer, whose value each answer records; the on-screen timer, which shows it; and the timebox clock. U175 owns the answer timer (`review/answer-flow#F2`), U177 the on-screen timer, U185 the timebox. This file states how they start and stop and how they depend on each other, so that the three units agree.

## T1. The answer timer (R359, R360, R368, R430, R679)

- `review/answer-flow#F2` defines the answer timer: when it starts and starts again, the active time it counts (R360, R679), and the cap on the `msTaken` it sends (`sched/review-session#C9`, `sched/answering`). The edit panel's close is U184's (R398). This section adds only the following.
- Under the timebox sheet (`review/reviewer-strings#S7`) the next card's timer starts when Continue is chosen, not when its question is drawn under the sheet (R430).
- A reveal does not stop or restart it, whether the user reveals or Auto Advance does (`review/auto-advance#A6`): an Auto Advance answer records the same time a button would at that instant.

## T2. The on-screen timer (R364, R754)

- It is shown while the current card's home preset has `show_timer` true (`sched/presets#V2`), and hidden while it is false. Each card decides for itself (R330): the timer appears and disappears as cards of differently configured subdecks follow each other.
- When no preset has the home deck's `config_id`, `show_timer`, `stop_timer_on_answer` and the cap below come from the default preset (id 1), and from the built-in defaults (`decks/deck-presets#P2`) when there is no preset 1 either, as Anki's reviewer reads them (`sched/presets#V1` leaves this path to its unit).
- Under the timebox sheet the timer follows the preset of the card whose question waits under the sheet: shown or hidden by that card's `show_timer`, at 00:00 and not counting until Continue starts the answer timer (T1).
- It shows the answer timer (T1) in whole seconds, rounded down: it shows 0 whenever the answer timer starts or starts again, and it pauses with it. Anki's counter is its own and counts wall-clock seconds (T4).
- The cap is the home preset's `cap_answer_time_to_secs` as read with `sched/presets#V3`. At the cap it shows the cap and stays there, on both sides, while the answer timer goes on (the recorded time is capped too, T1). The timer then is in a capped state, which its model exposes and its tests check (`capped` true). Anki shows the capped value in red. Meguri must show the capped state by a cue that does not rely on colour alone, and must include it in the timer's accessibility value. The design contract gives the timer's place only (N11.1, N13.3) and does not give this cue yet, and `review/reviewer-strings` does not hold the accessibility wording yet.
- At the reveal, when the home preset's `stop_timer_on_answer` is true, it freezes at the value it showed at the reveal and keeps that value for the rest of the answer side. When it is false, it goes on counting on the answer side up to the cap.
- Format: minutes, a colon and two-digit seconds. Minutes are never carried into hours: 7,200 s shows as 120 minutes. Meguri writes the minutes with at least two digits (R364: "mm:ss"); Anki writes them without a leading zero.

| Seconds shown | Anki | Meguri |
|---|---|---|
| 0 | 0:00 | 00:00 |
| 5 | 0:05 | 00:05 |
| 60 | 1:00 | 01:00 |
| 754 | 12:34 | 12:34 |
| 7,200 | 120:00 | 120:00 |

- Examples: cap 60, 75 s on the question → 01:00, capped. Cap 60, `stop_timer_on_answer` true, revealed at 12 s → 00:12 on the answer side however long it stays. `stop_timer_on_answer` false, revealed at 12 s, answered at 20 s → 00:20 just before the answer. Background 600 s at 5 s on the question, back for 3 s → 00:08.
- It changes once per second while it is visible and counting, from a system-driven schedule; it causes no work while hidden, frozen or at the cap (R1854).

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
- Anki's on-screen timer is a separate counter that starts at 0 on each question display, also when the question is drawn again after an edit, and counts wall-clock seconds; Meguri's shows the answer timer, so the two always agree (T2).
- Anki writes the timer's minutes without a leading zero; Meguri writes at least two digits (R364, T2).
- Anki's timebox counts wall-clock time and starts again each time study starts from a deck (T3); Meguri's counts active time per session and stops while no card is presented (S7, S11).
- Anki's Finish returns to the deck list; Meguri's opens the session summary (R86, R430).

## T5. Tests

| Test file (unit) | Covers |
|---|---|
| `ActiveTimeClockTests` (U175) | T1: the start at Continue under the timebox sheet; no restart at a manual or automatic reveal (the rest of the timer is `review/answer-flow#F2`'s tests) |
| `OnScreenTimerTests` (U177) | T2: shown and hidden per card, a missing preset read from preset 1, the timebox sheet's 00:00, the table, the examples, freeze on reveal, the cap kept on the answer side with `capped` true, no updates while hidden, frozen or capped |
| `TimeboxControllerTests` (U185) | T3: start at the first presented card, the restarts and the non-restarts (a summary opened by Finish against one opened by the queue running out), the time counted with the edit panel and the Add sheet open and not counted in the background, the snapshot across relaunch (a changed `timeLim` is `review/review-preferences#P3`'s case) |

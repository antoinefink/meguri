# Review flow: Auto Advance and review timers (U157, U177, U185, U186)

Spec-writer notes for `specs/review/auto-advance.md` and `specs/review/timers.md`,
and for the inactive scenario `conformance/scenarios/review/congrats/learning-boundaries`.
Read at tag 26.09.3 (`refs/anki-26.09.3`) on 2026-11-10 and again on 2026-11-10 for the review fixes; no code was copied. The
behaviour is stated in Meguri's own words; the only Anki names in the specs are
preset and config keys (interface vocabulary).

Meguri's own design, with no Anki source: `review/auto-advance` A3 item 3 and A8
(R433 and R755, including the key-window turn-off, the disabled toggle and the toggle with no card drawn, the menu rule, the answer gate, the delay clamp at 2,147,483,647 ms, one reminder per countdown), T1 (the answer timer's
start at Continue under the timebox sheet, R430; the rest is `review/answer-flow#F2`'s), T2's two-digit minutes (R364), the rule that the
on-screen timer shows the answer timer, its 00:00 under the timebox sheet and its
capped state, and T3's per-session, active-time clock and its restarts
(R431, U157 and `review/reviewer-strings#S7`; the active-time definition decided
2026-11-10).

| Spec section | Anki files read | Read for |
|---|---|---|
| auto-advance A2 | `qt/aqt/reviewer.py` (the two countdown starts and their timeouts), `pylib/anki/cards.py` (the card's current deck), `pylib/anki/decks.py` (the legacy preset lookup and its fallback), `rslib/src/deckconfig/mod.rs` and `rslib/src/deckconfig/service.rs` (the legacy preset read with fallback to preset 1, then the defaults, serialized as schema-11 JSON), `rslib/src/deckconfig/schema11.rs` (the legacy keys and the two enums, `f32` times); serde_json 1.0 (a non-finite float is written as null) | the home preset read for each countdown, falling back to preset 1 and then the built-in defaults, delay read at start and action read at the end, the `f32` written as its shortest decimal, read by Python as binary64, × 1000 and truncated (checked with Python: 32.3 → 32,299, 2.01 → 2,009), the enum order (2 Good, 3 Hard), unknown values acting as the defaults |
| auto-advance A3 | `qt/aqt/reviewer.py` (init, cleanup, the toggle, the More menu item and the Shift+A shortcut) | off at reviewer start and when review is left, the toggle restarting the current side's countdown from the toggle instant, off cancelling both countdowns, the silent turn-off when focus has left the main window at a timeout |
| auto-advance A4 | `qt/aqt/reviewer.py` (question and answer display, the audio-finished hook, the flag and mark indicators), `qt/aqt/sound.py` (player queue and its finished callback), `qt/aqt/main.py` (redraws after changes) | a countdown started on every display of a side (redraws included), not on a flag or mark change, one countdown at a time, the audio check at the deadline and the re-check after the queue empties (about 100 ms later), a playing or paused player counting as audio |
| auto-advance A5 | `qt/aqt/reviewer.py` (the two timeouts), `qt/aqt/progress.py` (the timer helper) | the order of the checks: no card, audio still playing, auto advance off or focus elsewhere, then the action; a single-shot timer that fires during a progress window retried every 100 ms |
| auto-advance A6 | `qt/aqt/reviewer.py` (the timeouts, bury of the current card, answering, the tooltips), `ftl/core/studying.ftl` | reveal without reading the type-in box, the reminders, Bury Card through the ordinary bury with its count tooltip, answers through the ordinary answer path with the card's capped time taken |
| auto-advance A9 | the files of A2 to A6, `qt/aqt/main.py` (review cleanup on leaving review); Qt's single-shot timer behaviour for a negative interval (not started; an inactive timer reports remaining time -1) | the Anki column of each difference: null for NaN and infinity, a negative time acting only at an audio end, the reminder shown again at each later audio end on that side because the countdown is not cleared, the turn-off when review is left |
| timers T2 | `qt/aqt/reviewer.py` (bottom bar: the timer element, the values passed when the question and the answer show), `qt/aqt/data/web/js/reviewer-bottom.ts`, `pylib/anki/cards.py` (time limit, show timer, time taken), `pylib/anki/decks.py` (the legacy preset lookup and its fallback) | the counter reset at each question display, one-second steps, the cap at maximum answer seconds shown in red, the stop-on-answer freeze, the `m:ss` format with minutes not carried into hours, hidden when the option is off |
| timers T3 | `pylib/anki/collection.py` (timebox start and reached), `qt/aqt/overview.py` (study button), `qt/aqt/main.py` (study key), `qt/aqt/toolbar.py` (study link), `qt/aqt/reviewer.py` (timebox check, Finish) | the timebox started each time study starts from a deck (study button, study key, the toolbar's Study link while the overview is shown), `timeLim` read at every check, Continue restarting it, Finish returning to the deck list |
| timers T4 | the files of T2 and T3, `pylib/anki/cards.py` (timer start at fetch) | the Anki column of each difference |
| scenario `review/congrats/learning-boundaries` | `rslib/src/scheduler/congrats.rs`, `rslib/src/storage/card/congrats.sql` | which queue-1 cards the compatibility facts count (every one in the deck subtree, whatever its due) and the 60 s clamp, so the scenario probes decks whose learning cards fall past the end of the day, one beyond 86,400 s, and a deck that mixes one card due later today with one past it |

# Review answer flow (U175, U178, U179, U187, U188)

Spec-writer notes for `specs/review/answer-flow.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`) on 2026-11-10, and again on 2026-11-10 for the review
corrections (Select Deck step, rollover timer, rebuild triggers, sync reset,
global profile settings, empty-queue branch); no code was copied. Only the
paragraphs headed "Anki" come from these files. The "Meguri" paragraphs and F9
come from the plan (U175, U178, U179, U187, U188), the Product Contract, the
design contract and the existing specs they cite.

| Spec section | Anki files read | Read for |
|---|---|---|
| answer-flow F1 | `qt/aqt/main.py` (state changes, the study key and its two branches), `qt/aqt/overview.py` (Study Now), `qt/aqt/reviewer.py` (show, cleanup), `rslib/src/decks/current.rs` (the Select Deck op, the queue dropped when the value changed), `rslib/src/config/undo.rs` (the undoable config write that records the step only when the value changed), `rslib/src/ops.rs` (the op's step name) | Select Deck recorded as an undo step and the queue dropped; nothing when unchanged; only `curDeck` written; the study key; leaving the reviewer writes nothing; one reviewer |
| answer-flow F2 | `qt/aqt/reviewer.py` (taking the next card, the redraw after a note change, showing question and answer, building the answer, the op response order), `pylib/anki/cards.py` (timer start, time taken, home deck id, the preset's answer time cap), `pylib/anki/scheduler/v3.py` (the answer's instant and uncapped time), `rslib/src/ops.rs` (a card change dominating a note change) | the timer starts at every take and not at a redraw or a reveal; note edits that add cards rebuild; the time taken is uncapped wall-clock milliseconds, capped by the backend with the home deck's preset; the answer instant is taken at the input |
| answer-flow F3 | `qt/aqt/reviewer.py` (shortcut list, Enter key handling, default ease, answer button list, the answer state guard), `qt/aqt/profiles.py` (default answer keys, the Space-rates-card preference and its default, both in the global profile metadata), `pylib/anki/scheduler/legacy.py` (button count), `ts/reviewer/index.ts` (Enter in the type-in box) | four answers, the reveal keys, answers only in the answer state and the transition state at once, Good as default, keys 1 to 4, both settings global to all profiles |
| answer-flow F4 | `qt/aqt/reviewer.py` (button time text) | labels shown only when `estTimes` is true |
| answer-flow F6.1 | `qt/aqt/operations/collection.py` (undo, redo, their tooltips), `qt/aqt/forms/main.ui` (Undo and Redo shortcuts), `qt/aqt/reviewer.py` (undo key, response to op changes and its order, the empty-queue branch to the overview), `qt/aqt/overview.py` (response to op changes, the finished screen), `rslib/src/ops.rs` (which changes need a queue rebuild or a note redraw, by op kind and change kind), `rslib/src/undo/mod.rs` (an undo reporting the undone op's kind; steps recorded only with changes), `rslib/src/scheduler/queue/mod.rs` (queue kept after an answer, dropped after other card changes), `rslib/src/collection/transact.rs` (when queues are dropped after an op), `rslib/src/decks/current.rs` (undoing Select Deck) | global undo from the reviewer; the reviewer's response table by what the step changed; the empty take; undo on the congratulations page stays on the overview; undoing Select Deck after the answers |
| answer-flow F7 | `qt/aqt/main.py` (the one-time timer at the day cutoff, re-armed each time, and the reviewer refresh it triggers in review), `qt/aqt/reviewer.py` (the queue refresh taking the head), `rslib/src/scheduler/queue/mod.rs` (day check at a queue read: queues and history dropped, unbury) | the rollover refresh at the cutoff; the old-states gap; also `sched-session.md` for P10 |
| answer-flow F8 | `qt/aqt/reviewer.py` (response to op changes from other screens), `qt/aqt/main.py` (the response deferred until the main window has focus, the dimmed page, the refresh after a collection sync that reports every change), `qt/aqt/sync.py` (the sync's completion callback on every outcome), `rslib/src/ops.rs` | the same response table for changes made elsewhere; focus deferral; rebuild after any sync |

Scenario drafts (F10) were written from these readings and from
`specs/sched/review-session.md` P2, P8 and P9, `specs/sched/orders.md` O2 and
`specs/conformance/session-ops.md` E2 and E3; they are not recorded yet.

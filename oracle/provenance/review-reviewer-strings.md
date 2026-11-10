# Reviewer strings (U175, U178, U182, U183, U185 to U187)

Spec-writer notes for `specs/review/reviewer-strings.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-10-27 by mapping each
section of the committed text to the files that back it. The English values are
Anki's interface wording where the spec says so (S3 to S9, and the English values
only of S13 and S14), with the changes
S11 lists; Anki's string ids are not used as keys (the keys are Meguri's).

Meguri's own design, with no Anki source: the keys, argument forms and plural
variations (S1), the toast behaviour (S2), the announcement-only undo and redo
(S5), the Auto Advance explanation (S6), the non-blocking timebox sheet (S7),
the native congratulations facts (S8), the single unbury chooser (S9), the
changed-elsewhere toast (S10), S13's accessibility label, spoken interval and
unit words, Voice Control input labels and accessibility action (from R185, R349
and the design contract; Anki's reviewer has none), and S14's dialog title,
buttons and the Set Due Date dialog's text. Added 2026-11-10, also Meguri's
own: S8.1's Deck Options link, S15 (R728's notice word for word, and the
commit-failure banner), S16 (R462's text word for word, the other seven
reasons), S17, S18 (R374's titles) and S19; S10's removal of the edit panel
follows `editor/field-html` and `review/edit-current#E2`.

| Spec section | Anki files read | Read for |
|---|---|---|
| reviewer-strings S3 | `qt/aqt/reviewer.py` (after an answer: the leech check and its tooltip), `ftl/core/studying.ftl` | the two leech messages and when each shows; no other tooltip after an answer |
| reviewer-strings S4 | `qt/aqt/reviewer.py` (bury, suspend, delete), `qt/aqt/operations/scheduling.py`, `qt/aqt/operations/note.py`, `ftl/core/studying.ftl`, `ftl/core/scheduling.ftl`, `ftl/core/browsing.ftl` | the bury count from the op's result, the suspend messages without a count, the delete message with a card count, the Set Due Date and Forget messages and their counts; no tooltip for flag and mark |
| reviewer-strings S5 | `ftl/core/undo.ftl`, `qt/aqt/operations/collection.py`, `qt/aqt/main.py` (undo and redo actions) | the named and plain Undo and Redo labels and the undone and redone tooltips |
| reviewer-strings S6 | `qt/aqt/reviewer.py` (Auto Advance toggle, question and answer countdowns), `ftl/core/actions.ftl`, `ftl/core/studying.ftl` | the enabled and disabled messages, the reminders when a countdown ends with Show Reminder, no message when it turns itself off |
| reviewer-strings S7 | `qt/aqt/reviewer.py` (timebox check after an answer), `pylib/anki/collection.py` (timebox start and reached), `ftl/core/studying.ftl` | the trigger, the card count since the timebox started, the minutes from the limit passed as text (so the plural never matches), Continue and Finish; the raw `timeLim` read at every check and the start when study starts whatever the limit (S7's read and clock bullets, 2026-11-10) |
| reviewer-strings S8 | `rslib/src/scheduler/congrats.rs` (the next-learning delay clamped to at least 60 s), `proto/anki/scheduler.proto` (congratulations info), `ts/routes/congrats/lib.ts`, `ts/routes/congrats/CongratsPage.svelte`, `ftl/core/scheduling.ftl` | the paragraphs, their order and conditions, the minutes and hours amounts and their rounding, the unbury link, the deck description |
| reviewer-strings S9 | `qt/aqt/overview.py` (unbury), `ftl/core/studying.ftl` | the unbury dialog's title and three choices in title case, manual as its default choice, the single-kind case without a dialog |
| reviewer-strings S11 | the files of S4, S5, S7, S8 and S9 | the Anki column of each difference |
| reviewer-strings S13 | `qt/aqt/reviewer.py` (bottom bar: the Show Answer button and the four answer buttons), `ftl/core/studying.ftl` | the four grade names, their order, the Show Answer label, a Show Answer button always shown, no accessibility action (2026-11-10) |
| reviewer-strings S14 | `qt/aqt/forms/forget.ui`, `ftl/core/scheduling.ftl`, `ftl/core/actions.ftl`, `rslib/src/config/bool.rs` | the two Forget toggles' labels and order, the dialog's Anki title (Reset Card, S11), the two config keys' defaults (true and false; also in `sched-manual-ops.md`'s M6 row) (2026-11-10) |
| reviewer-strings S11 (S13, S14 bullets) | the files of S13 and S14 | the Anki column of those differences (2026-11-10) |
| reviewer-strings S6 (reminder once per countdown), S11 (its bullet) | `qt/aqt/reviewer.py` (the audio-end hook that checks again whether a side's countdown has run out; the question and answer countdowns) | Anki shows the reminder again at each later audio end on that side; Meguri's once-per-countdown rule is its own (2026-11-10) |
| reviewer-strings S8.1, S11 (Deck Options link bullet) | `ts/routes/congrats/CongratsPage.svelte`, `ftl/core/scheduling.ftl` | the congratulations page has no Deck Options link (2026-11-10) |
| reviewer-strings S11 (stale-answer bullet), S15 | none new; `sched-session.md`'s P11 row | Anki raises an error for a changed card; the notice is R728's (2026-11-10) |
| reviewer-strings S11 (menu and title bullets), S18, S19 | `qt/aqt/reviewer.py` (the More menu's entries and the bottom bar's Edit), `ftl/core/actions.ftl`, `ftl/core/studying.ftl`, `ftl/core/scheduling.ftl` (the Set Due Date prompt and hint), `qt/aqt/deckoptions.py` (the window title) | Anki's English, for comparison only: no S18 or S19 value is taken from it (2026-11-10) |

# Reviewer strings (U175, U182, U185 to U187)

Spec-writer notes for `specs/review/reviewer-strings.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-10-27 by mapping each
section of the committed text to the files that back it. The English values are
Anki's interface wording where the spec says so (S3 to S9), with the changes
S11 lists; Anki's string ids are not used as keys (the keys are Meguri's).

Meguri's own design, with no Anki source: the keys, argument forms and plural
variations (S1), the toast behaviour (S2), the announcement-only undo and redo
(S5), the Auto Advance explanation (S6), the non-blocking timebox sheet (S7),
the native congratulations facts (S8), the single unbury chooser (S9) and the
changed-elsewhere toast (S10).

| Spec section | Anki files read | Read for |
|---|---|---|
| reviewer-strings S3 | `qt/aqt/reviewer.py` (after an answer: the leech check and its tooltip), `ftl/core/studying.ftl` | the two leech messages and when each shows; no other tooltip after an answer |
| reviewer-strings S4 | `qt/aqt/reviewer.py` (bury, suspend, delete), `qt/aqt/operations/scheduling.py`, `qt/aqt/operations/note.py`, `ftl/core/studying.ftl`, `ftl/core/scheduling.ftl`, `ftl/core/browsing.ftl` | the bury count from the op's result, the suspend messages without a count, the delete message with a card count, the Set Due Date and Forget messages and their counts; no tooltip for flag and mark |
| reviewer-strings S5 | `ftl/core/undo.ftl`, `qt/aqt/operations/collection.py`, `qt/aqt/main.py` (undo and redo actions) | the named and plain Undo and Redo labels and the undone and redone tooltips |
| reviewer-strings S6 | `qt/aqt/reviewer.py` (Auto Advance toggle, question and answer countdowns), `ftl/core/actions.ftl`, `ftl/core/studying.ftl` | the enabled and disabled messages, the reminders when a countdown ends with Show Reminder, no message when it turns itself off |
| reviewer-strings S7 | `qt/aqt/reviewer.py` (timebox check after an answer), `pylib/anki/collection.py` (timebox start and reached), `ftl/core/studying.ftl` | the trigger, the card count since the timebox started, the minutes from the limit passed as text (so the plural never matches), Continue and Finish |
| reviewer-strings S8 | `rslib/src/scheduler/congrats.rs` (the next-learning delay clamped to at least 60 s), `proto/anki/scheduler.proto` (congratulations info), `ts/routes/congrats/lib.ts`, `ts/routes/congrats/CongratsPage.svelte`, `ftl/core/scheduling.ftl` | the paragraphs, their order and conditions, the minutes and hours amounts and their rounding, the unbury link, the deck description |
| reviewer-strings S9 | `qt/aqt/overview.py` (unbury), `ftl/core/studying.ftl` | the unbury dialog's title and three choices in title case, manual as its default choice, the single-kind case without a dialog |
| reviewer-strings S11 | the files of S4, S5, S7, S8 and S9 | the Anki column of each difference |

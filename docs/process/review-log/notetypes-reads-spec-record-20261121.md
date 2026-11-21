# U267 and U268 spec record: review log, 2026-11-21

Branch `unit/notetypes-reads-spec-record` (PR #71), spec-writer session `s-20261121T143206Z-e393d31c`. The lines marked "Changed 2026-11-21 (implementation review)", or carrying that marker inline, in `notetypes/workspace` W7, W7.1, W8, W10, W10.1, W10.2, W12 and `notetypes/draft-save` D2 record the readings U267 (PR #60) and U268 (PR #67) built. The coordinator approved the 13 sentences and the note on `none` colour components under Antoine's permission of 2026-11-20 to update specs (`docs/process/standing-decisions.md`).

## Decisions

1. The 13 sentences are written as approved, in the sections they name.
2. W10.2's "(CSS's own cascade within a rule)" is removed: sentence 13 says the last declaration is read and edited whatever CSS would apply, so the parenthesis was no longer true.
3. The note on the declined finding is a rule in the Text Colour row: an `rgb()`, `rgba()`, `hsl()` or `hsla()` value with a `none` component shows the well empty, in Text Colour and Background (PR #67 reads both with one colour reader).
4. Plan section 9 records U267 (#60) and U268 (#67) as merged.
5. No `docs/design/decisions.md` row: these are core readings, not design decisions.

## Independent review

Reviewer: an independent Claude subagent (Opus). It received the diff and the sentence file, not the writer's reasoning, and checked the text against the rest of both specs and the specs they cite (`notetypes/structure-changes`, `notetypes/validation`, `render/template-syntax`, `notes/card-generation`, `render/special-fields`, `text/field-emptiness`, N22).

It confirmed that every sentence landed in its section with its wording, that each replaced text is gone, and that W8's list of names that can be non-empty and D2's refusals agree with G2, P6 and S1.2.

- P1: none.
- P2:
  - Fixed. The Status lines dated Antoine's permission 2026-11-21; the recorded permission is of 2026-11-20. Both now say 2026-11-20.
  - Fixed. This log was missing.
  - Fixed. W8 said the count is recomputed only when a front changes; with S5's remap, a change to the draft's fields changes it too. W8 now says so.
  - Fixed. W7 said issues apply only field renames to the templates; W7 now says W8 also applies the rest of S4's rewrite and S5's remap.
  - Kept. W12's "its progress counts cards and has no total" has no progress string in W15. The sentence is approved as written; it states what the core read reports (a job's progress without a total). How the screen shows it is the UI unit's to specify.
- P3:
  - Fixed. The Font cell repeated "the family name".
  - Fixed. The plan's U267 entry read as if "the Note Types reads" were one item of the list.
  - Fixed. W7's reason text "Their <fields> are empty" now reads "Their <field> is empty": W8 names one field.

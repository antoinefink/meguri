# Working reviews first

Antoine changed the execution priority on 2026-10-22: reach a usable review
app before completing the remaining feature areas. This overrides the earlier
instruction to execute all units in manifest order. The full plan remains the
final goal. Milestone membership, safety requirements and acceptance gates stay.

## Next delivery

Build one local review path over the existing production core and renderer:

1. Open the app-private collection and show its actual deck tree and counts.
2. Choose a deck and begin a real core review session.
3. Show the question, reveal the answer, and offer Again, Hard, Good and Easy
   with the core's interval labels. Support touch and keyboard input.
4. Save actual answers, advance through the queue, undo an answer, and finish
   the session. Surface commit failures without losing or duplicating answers.
5. Flush and suspend on background; reopen safely and prove that recorded
   answers survive a process restart. Handle scheduler-day changes and the
   existing FSRS admission policy before study.
6. Provide a supported way to bring a local collection into the app. Keep the
   source intact and verify the copied/imported collection before use.

CloudKit, collection management, editing, statistics and advanced review tools
follow this path. No sync or Apple-account setup is needed to prove local review.
Unavailable features remain absent from the user flow.

## Execution

First connect the app to real review sessions using generated collections
installed by the existing test hooks. This is integration proof, not proof that
a user can import their own cards. Then add the safe local collection entry path;
the minimal app is not delivered until that path and persistent reviews work.
Do not build an alternative scheduler, mock the production session, or introduce
an importer that bypasses the plan's staging and file-safety rules.

Track the first coherent implementation as `RF1`, mapped to the needed parts of
U51, U151, U152, U153, U175, U176, U178, U179 and lifecycle/finish work. Keep the
existing attach points for later providers. The next step covers local entry,
using the package/replace primitives required by the chosen supported format.
Reuse prepared specs and code before writing replacements.

Work one delivery item at a time on master. Record its starting commit, use the
filtered implementer/fixer and one independent reviewer, and keep focused tests
and a full gate for the integrated Swift/FFI change. Review includes correctness,
plan conformance, clean-room boundaries and rendered UI quality. VM checks that
cannot run stay queued and must pass before final acceptance.

Use `Unit:` only when the unit is complete. Keep a record of unfinished
requirements. Complete every unit and retain every milestone gate.

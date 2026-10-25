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

Local review is the first intermediate checkpoint. Antoine expanded the first
usable build on 2026-10-22 to include file import/export and explicit manual
Import from Anki / Export to Anki through an Anki-compatible server. Prioritize
those transfers immediately after safe local entry and reviews, before the
remaining editing, statistics and advanced review features. Reuse prepared work.

The existing sync design remains: routine Meguri sync uses CloudKit; Anki server
access is an explicit Settings transfer with preview, confirmation and safety backup.
Antoine exempted custom servers entered by URL from the 60-minute wait on 2026-10-24,
including retained earlier deadlines. After completion, an explicit transfer may
follow immediately; active/uncertain-operation guards and confirmations remain.
Antoine chose a self-hosted Anki-compatible server
for the first usable version on 2026-10-22. Provide endpoint configuration and
explicit import/export in every build; test with the local official server rig.
No automated AnkiWeb contact is permitted. The routine-sync design is unchanged.
CloudKit requirements and all milestone gates remain.

No sync or Apple-account setup is needed to prove the local-review checkpoint.
The expanded first-version scope requires its transfer checks before acceptance.
Unavailable features remain absent from the user flow.

## Execution

For the current fast macOS pass, Antoine authorized local Xcode MCP build, run,
debug and UI feedback on 2026-10-25. Defer VM and mobile iteration checks during
this pass. Keep their integration and final requirements outstanding; a local
build or launch does not supply that evidence. Keep the clean room and privacy
boundaries unchanged.

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

Work on master in coherent batches of 3–5 related units when useful, following
Antoine's 2026-10-25 goal. Record each unit's starting commit, use the filtered
implementer/fixer and independent review, and retain separate commits and focused
checks per unit. Run one full gate covering the complete integration batch.
Review includes correctness,
plan conformance, clean-room boundaries and rendered UI quality. VM checks that
cannot run stay queued and must pass before final acceptance.

Use `Unit:` only when the unit is complete. Keep a record of unfinished
requirements. Complete every unit and retain every milestone gate.

# Working reviews first

Antoine changed the execution priority on 2026-10-22: reach a usable review
app before completing the remaining feature areas. This overrides the earlier
instruction to execute all units in manifest order. The active plan remains the
final goal, subject to the Anki server freeze below. Active safety requirements
and acceptance gates stay.

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

Local review is the first intermediate checkpoint. File import/export remains
required for the first usable version. Routine Meguri sync remains CloudKit.

## Anki server freeze — 2026-10-26

Anki server work stops here. Resume only when Antoine explicitly requests it. This replaces the October 22 server-delivery requirement and
the October 25 instruction to resume parked sync checks before acceptance.

Preserve the code, plan items, dependencies and verification evidence. Do not
build or fix server code, add server tests, start servers, or seek
acceptance for this scope. U13/U105/U106/U107/U109/U110/U111/U116 and the server
portions of mixed units and RF4 are retained reference work, outside current
delivery, milestone and release requirements. The plan's Frozen Unit Index
retains the original entries. Unfinished work is not marked complete or passing.

Dependencies, shared gate failures and general instructions to finish the plan
cannot start server work. If a validator cannot separate frozen server
work, record that limitation and continue active app work; never claim a pass.
Existing shared regression tests may run, but server-only failures do not
create a repair task under this scope. Do not disable shared safety checks.

File import/export, local Anki collection compatibility, shared media storage,
CloudKit and local Migaku/AnkiConnect integrations remain required. No sync or
Apple-account setup is needed to prove the local-review checkpoint.

## Execution

Continue the shared app and review workflow using RF1. The unfinished sync
batch and its failed receipts remain retained under the freeze above. Keep
`1416a520542a8c5daa38a23bfe398da704c3d6c9` as the integration base for pending
master changes. Resolve active-scope failures before acceptance.

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
requirements. Complete every active unit and retain every active milestone gate.
Frozen server requirements do not block delivery.

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
required for the first usable version. Routine sync offers iCloud Sync and the
own-server Meguri Sync provider under the narrow 2026-11-16 exception below.

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

## Own Meguri provider — 2026-11-16

Antoine explicitly requested two services: iCloud Sync and Meguri Sync using
Meguri's own server. Root adopted reviewed `sync/meguri-provider` MP1–MP5 as
delegated routine implementation authority. The review and follow-on mapping
are in `review-log/meguri-provider-contract-20261116.md`.

This exception authorizes only own-provider app/core/Swift/FFI integration and
its demonstrated dependencies. Legacy Anki transfers stay hidden and frozen;
no AnkiWeb contact or Anki code reuse is authorized. Keep their tests and failed
evidence. Existing CloudKit record/recovery rules and approval scopes stay.
The separate server plan's acceptance does not accept the app or retained peer.
Keep failure discipline and the original app integration base below; this
contract decision supplies no gate pass or completed unit.

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

Integrate on master in coherent batches of 3–5 related units when useful, following
Antoine's 2026-10-25 goal. Record each unit's starting commit, use the filtered
implementer/fixer and independent review, and retain separate commits and focused
checks per unit. Run one full gate covering the complete integration batch.
Review includes correctness,
plan conformance, clean-room boundaries and rendered UI quality. VM checks that
cannot run stay queued and must pass before final acceptance.

Use `Unit:` only when the unit is complete. Keep a record of unfinished
requirements. Complete every active unit and retain every active milestone gate.
Frozen server requirements do not block delivery.


## Next feature priority — 2026-10-27

Delivery priority (2026-10-29): finish reviews and iCloud sync first.

Antoine requests iCloud sync after the current backup, integrity and recovery work is verified. Give U113 and U114 and their dependencies integration priority over the remaining editing, search and statistics features. Cover collections, saved reviews and media across devices under the approved CloudKit contract. Before using a rule listed in `sync/policy#P24`, add its `conformance/cloudkit-acceptance#Q11` test and resolve the listed item through the spec-writer workflow. `sched/optimizer` O12 and O13 were approved on 2026-10-27. Before U47 builds on an O19 item, add its named test and settle the recorded correction through the spec-writer workflow; the approval alone does not close O19. Anki-server work stays frozen.

Preparation follow-up: the requested search R1881 command (`tools/mg similarity -- conformance/scenarios/search conformance/goldens/search`) reports pass but scans 0 files and skips both inputs (`similarity/20261027T153016Z-48035`). It does not prove coverage. Keep the 38 ported search scenarios inactive until U81/U82 and complete the required R1881 check before activation. Counts are retained in `build/unit-intake/U132-maintenance/search-r1881-status.json`.

Outcome (2026-11-21): the check now has real coverage. `tools/mg similarity --search-data -- conformance/scenarios/search conformance/goldens/search` scans both roots: 186 files, 93 scenario and golden pairs. On master `9afbaa831` it flagged 5 files in the `text` channel. Four were scenarios (`history/answers`, `dst-new-york`, `dst-paris`, `odd-ease`): their revlog inserts listed the columns in the same order and form as an Anki test. The fifth was the golden `builder/items.json`: it records real Anki output, but the scenario joined the same three states in the same order as an Anki test assertion. All 5 were rewritten and none needs an allowlist entry. The history inserts now name the columns in a different order. `builder/items` joins other states. All 5 goldens were re-recorded. The rescan flags 0 files. Its `warn` comes from the 35 files below the fingerprint limits; this lane always reports those, and they prove nothing about provenance either way. Activation decision: Claude (coordinator) under Antoine's delegation, 2026-11-20: a scenario the check does not flag may be active. Now active: the 4 history scenarios and `tags/hierarchy` and `tags/regex-and-nc`, which pass T0. The tags scenarios now write note three's tag `Animal` with `db_exec`, so Meguri's implicit-parent spelling (tags T3) no longer applies and no deviation is needed. Their 2 goldens were also re-recorded; the search observations did not change. 51 of 93 search scenarios are active. The 42 still inactive need Meguri implementation, not R1881 work: 7 need the `search_build`, `search_join` or `search_replace` observations (`builder/*`, `join-replace/*`), 34 need text, field, regex, `nc:`, `sc:` or `w:` term matching (U81, U82), and `unsorted/configured-sort-ignored` needs sorted search.

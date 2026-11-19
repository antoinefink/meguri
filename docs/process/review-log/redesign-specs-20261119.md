# Independent review of the redesign specs, 2026-11-19

Scope: the spec and contract changes for the 2026-11-19 design decisions
(`docs/design/decisions.md`): `specs/sync/policy.md` P25,
`specs/sync/cloudkit-recovery.md` V27, `specs/sync/meguri-provider.md` MP6,
`specs/sync/status-strings.md` T19, `specs/sched/fsrs-policy.md` A1,
`specs/sched/app-coordination.md` F5, `specs/package/transfer-sheets.md` X7,
`specs/editor/add-flow.md` A5.1, `specs/editor/field-html.md` F1 and H6, the
review timer removals, and contract sections N15 and N18.

Author: spec-writer session `s-20261119T192202Z-3c320a53`. Reviewer: a
separate read-only agent session that wrote none of the text
(`redesign_specs_review`). The reviewer read specs and design documents only.

## Passes

1. First pass on the four authoring commits. X7, A5.1, F1/H6 and the timer
   set: approved. P25, V27, MP6, T19, N15, A1, F5, N18: approved with changes,
   findings F1 to F13.
2. Second pass after the F1 to F12 changes. N15, A1, F5: clean. Remaining:
   R1 (the update hold was narrowed too far and let a paused device's publish
   continue after the update), R2 (precedence between two asking rules, and
   the status row for each settling outcome), R3 (one string key listed as
   removed while in use), R4 (a decision row missing from that worktree).
3. Third pass after R1 to R3: all eight sections approved, no new finding.

## Decisions taken on findings

Made by the orchestrating session under Antoine's delegation of 2026-11-19
("carry on with your best judgment"), each marked in the spec text with its
finding number:

- F1: re-fetch the replaced zone right before the switch; any change asks.
- F2: with a connected service that is offline or waiting to retry, the
  automatic FSRS step waits for an exchange or a study entry point.
- F3, F9: a changed endpoint or account, and an import or restore on a bound
  device, always ask before any upload.
- F4: the acceptance-unknown state re-reads server facts and offers Try Again.
- F5: a blocked provider leaves study, backups, import and export working.
- F6: iCloud chosen with no collection anywhere creates an empty one.
- F7: a Troubleshooting row shows replaced media in Files. An automatic
  30-day removal was written and then withdrawn the same day, because it
  contradicted R1650, R2018 and `media/media-files` M10. Nothing empties the
  media trash automatically.
- F8: "connected" is defined once.
- F11, R1: the update hold applies while a publish or adopt op exists.

Accepted as written, and open for Antoine to change later: the unguarded
interval of an upload to an empty Meguri Sync server, undo history cleared
by the automatic FSRS step, repeated silent re-enabling with a log entry,
and conflict copies other than media having no interface beyond a count.

## State of the review markers

The reviewer's verdict is that all eight sections may leave "under review".
The Status lines in `specs/sync/policy.md`, `specs/sync/cloudkit-recovery.md`
and `specs/sync/meguri-provider.md`, and the matching sentences in contract
N15 and N18.2, have NOT been changed: the session's permission rules refused
the edit that marks them approved, and it was not made another way. They read
"under review" until Antoine approves that change. Round 2 is not built
before then.

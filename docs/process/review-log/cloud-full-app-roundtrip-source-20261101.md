# App review and media sync workflow: implementation checkpoint

U113/U114 remain unaccepted. No Unit trailer is added.
Starting commit: `450f3ff04003ab0c1a4a1bf7cd2078700770dd1a`.
Original integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Authority: sync/cloudkit-records K11.3/K12/K18, sync/policy P4/P6/P12/P13,
conformance/cloudkit-acceptance Q2.

## Changes

The enabled full-workflow test uses ReviewCollection, SyncCoordinator and the
production backend factory against one retained Service process. It requires
actual saved Card, Revlog, MediaName and blob records, installed media bytes,
then exact stored rows and media after reopening A and adopting/reopening B.
Automatic sync stays off. No successful-exchange flag substitutes for stored data.

The test-only FFI reader observes Card and Revlog through one Query snapshot.
It retains every storage cell, including REAL bits and original text/blob bytes.
Only the two usn columns may differ across devices; modification times remain exact.
The test transport retains original replies, cursors and staging leases and uses
the production media upload admission. It does not acknowledge its own requests.

Actual adoption exposed a production ownership race: an old policy input could
return GenerationChanged before the ordered lane processed collection replacement.
The coordinator now captures original generation/revision before awaiting input.
It discards superseded completions. A GenerationChanged error is ignored only
when the retained Core proves a newer ready collection; the existing replacement
path receives that generation. Unproven and ordinary current-owner failures stay
failures. The sticky failure flag is never cleared to make a test pass.

## Review and repairs

Source author: cleanroom/20261101T035543Z-42570,
implementer session `s-20261025T134357Z-235af9a2`.
Fixer session: `s-20261025T162405Z-9dc656e4`.

Independent review covered correctness, plan conformance, original Service
receipts, snapshot consistency and resource ownership. No UI changes are included.
R1 identified the Rust closure lifetime and Swift compilation errors. The raw-row
reader replaced redundant field projections after the safe similarity finding.
R2 confirmed complete row comparisons. R3 confirmed the existing main-actor factory
pattern and comment-only privacy repair. R4 identified the actual old-owner race;
R5 reviewed its lifecycle-proof repair; R6 reviewed the final diagnostics. Neither
found new required source findings. Test-only
diagnostics now use bounded fixed error/action tags, without payloads or identities.

Filtered fixes: 042158Z-49860, 042618Z-54269, 042928Z-57581,
043315Z-60318 and 043749Z-64874. Each exports the complete component.

## Verification

- core-test/20261101T041407Z-45529: preserved Rust fixture compilation failure.
- swift-test/20261101T041519Z-46084 and 042451Z-52146: preserved frontend failures;
  these runs establish no workflow pass.
- core-test/20261101T042418Z-51167: physical-row snapshot/no-mutation test passes.
- swift-test/20261101T042716Z-54860: first runtime failure at adoption.
- swift-test/20261101T043046Z-58220: original GenerationChanged cause established.
- swift-test/20261101T043558Z-61191: all nine coordinator tests pass, including
  ordinary and unproven generation-error preservation.
- swift-test/20261101T043629Z-63283: adoption passes, first exchange fails.
- swift-test/20261101T043849Z-65483: identifies the remaining failures as
  coreCapabilityUnavailable for RunCleanup and DeleteZones.
- similarity/20261101T042436Z-51516 and 043604Z-62478: component scans pass.
- private/20261101T042727Z-56044, 043601Z-62198 and 043905Z-66825:
  repairs and final diagnostic text pass. Earlier failed privacy receipts remain.

- private/20261101T044043Z-67787: all 15 checkpoint files pass.
- similarity/20261101T044043Z-67788: final component passes before mechanical
  Rust formatting. The formatter changes no behavior.
- core-lint/20261101T044042Z-67562: formatting failed; clippy and layering pass.
  core-format/20261101T044104Z-68490 applied the standard formatter.
- core-lint/20261101T044123Z-68815: formatting, clippy and layering pass.

- swift-test/20261101T044201Z-69218: all 16 existing process, coordinator and
  adoption/review regressions pass after formatting.

## Remaining work

The full workflow remains enabled and red. RunCleanup and DeleteZones require
actual implementation; they are not skipped or reported as completed. The test
has not yet established the review/media/reopen portion. Manual media execution
with automatic sync off remains an unverified concern after this first exchange.

Original integration/milestone failures, the remaining CloudKit runtime and fault
contracts, signed native delivery and final human evidence remain required.
This checkpoint claims neither complete iCloud sync nor release acceptance.

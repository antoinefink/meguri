# Native initial fetch

Status: WIP. No Unit, P24, Q6 or F5 acceptance.
Start: `6c156ea851e2b3ace4d84201864bdab1b03c3399`.
U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Source and review

- R1 tests: `cleanroom/20261030T130734Z-16802`.
- R2 implementation: `cleanroom/20261030T133418Z-25705`.
- R3 fixes: `cleanroom/20261030T141405Z-45297`.
- R4 fixes: `cleanroom/20261030T142434Z-51923`.
- R5 test lint: `cleanroom/20261030T143055Z-57273`.
- Implementation session: `s-20261025T134357Z-235af9a2`.
- Fixer session: `s-20261025T162405Z-9dc656e4`.
- Tests use actual Service pages, Host operations, tracking and WAL receipts.
- The restart test uses 300 new note/card pairs and a killed child process.
  It does not prove power-loss behavior or signed CloudKit behavior.
- The pause retained the R2 export. Work resumed from the recorded start.
- Review R1 required three fixes: exported enum order, selected work after a
  missing-item result, and real server fields for the conditional-save test.
- Review R2 confirms those fixes. It asks for an exact local revision in the
  handed request when two flights have the same digest and server revision.
- A missing-item result must not create an ACK or clear a newer local change.
- The child test must allow reference entries between pages, then require an
  empty inbox and intents, all 300 pairs, ACKs and WAL receipts before kill.

## Checks

- `core-test/20261030T132340Z-22718`: failed at the missing implementation.
- `core-format/20261030T140929Z-41812`: pass.
- `core-test/20261030T140954Z-42334`: compile failure, missing commit option.
- `similarity/20261030T141102Z-43390`: pass before R3 fixes.
- `private/20261030T141023Z-42868`: failed; three files.
- `private/20261030T142035Z-48518`: failed; three files. No policy exception.
- `core-test/20261030T142033Z-48261`: 12 pass, one child test fails before kill.
- `core-test/20261030T142136Z-49124`: no tests selected; not a pass.
- `core-test/20261030T142241Z-49637`: both selected FFI tests pass.
- `gate/20261030T141613Z-46272`: plan only; no integration checks ran.
- `core-test/20261030T142826Z-55323`: all 16 selected tests pass, including
  kill/reopen and the three FFI missing-item cases.
- `core-lint/20261030T142935Z-56496`: three test-only lint errors.
- `core-lint/20261030T143232Z-59685`: pass after the test-only cleanup.
- `core-test/20261030T143404Z-62217`: all 380 affected tests pass, zero skipped.
- `swift-test/20261030T143216Z-58684`: native error mapping passes with rebuilt
  bindings and native core. This test does not contact CloudKit.
- `similarity/20261030T142841Z-55622`: pass for the repaired source.
- `private/20261030T142804Z-54783`: pass for all 56 selected files.
- `private/20261030T143233Z-60001`: pass for the final test and log.
- Final independent review is clear. The exact-flight case is fixed. The
  test-only cleanup preserves all assertions and production behavior.

## Open

The original-base integration gate remains required. Keep the remaining
collection payload assets, structural, deletion, root/A7, Q6, native and F5
work open. P24 entries remain until their required proof and spec review.
The VM is running and answers SSH after the drive reconnection; this is not
native test evidence. No full workflow or release acceptance is claimed.

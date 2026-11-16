# Test route inventories — 2026-11-16

Starting commit: `5e139565e5c06b19a4b9ead432f4215e5c93388c` (clean master).
Source repair: `e5b327513f43a6bd2d64989e8d736ff5353a9729`.
Keep integration base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Cause and repair

The app already registered `card/behaviors/replay-identifier`, but two test
inventories omitted it. The complete route list in the app's error therefore
differed from the expected list. This failed the route registry checks and the
bad-launch UI case in the retained Xcode and snapshot results.

Independent review also found `card/replay-accessibility` missing from the Mac
Debug expectations. Its registration is conditional. The app-hosted expectation
now uses the same build condition; the UI test reads the app's configuration
instead of relying on the test runner's configuration.

Only two test files changed. Both lists remain independent of production code.
The complete-list equality, expected error and UI assertions remain. The existing
presentation-style test includes the replay-identifier screen. No production
route, test limit or timeout changed.

## Review

- Author: filtered run `cleanroom/20261116T084945Z-19304`.
- Round 1: independent run `20261116T085159Z-21560` found the Mac Debug omission
  (P2). Its exported patch matched the input patch; review made no edits.
- Fixer: `20261116T085514Z-26310` added the conditional expectations.
- Round 2: `20261116T085743Z-28288` found no P0–P2 findings and confirmed the
  original assertions and source scope. Its exported patch also matched its seed.

## Verification

Retained failures: `xcode-check/20261116T023911Z-29156` and
`snapshots/20261116T033204Z-55003`. The mobile bad-launch case's actual error
included the missing route. The Mac Debug omission was caught in source review
before a selected native run.

Focused checks on clean `e5b32751`:

| Check | Receipt | Result |
|---|---|---|
| Mac complete route list | `mac-hosted-test/20261116T090109Z-33296` | One pass, zero skips |
| Mac unknown-route error | `mac-hosted-test/20261116T090326Z-44701` | One pass, zero skips |
| iPhone Debug bad launch | `ui-test/20261116T090521Z-50862` | One pass, zero skips |
| iPad Debug bad launch | `ui-test/20261116T090644Z-54979` | One pass, zero skips |
| iPhone Profile bad launch | `ui-test/20261116T090749Z-60090` | One pass, zero skips |

The initial selected Mac attempt `20261116T085300Z-22049` refused dirty source
before running a test. The WIP commit supplied the required clean input. That
refusal is retained and is not a passing check.

Privacy: `private/20261116T085938Z-30076` passes for both changed sources.
Similarity: `similarity/20261116T085938Z-30589` passes: two scanned, zero flags,
zero allowlisted files. Only the filtered implementer report was read.
Static checks: `static-checks/20261116T085938Z-30077` passes all three checks.

The full Xcode and snapshot suites have not passed. The parameterized
presentation test awaits the broader suite. No Unit trailer or milestone
acceptance is added. Other recorded failures remain required repair work.

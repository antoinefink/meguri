# Review-first checkpoint — 2026-11-12

Status: unfinished work. U113 and U114, P24, milestone gates and release acceptance remain open.

The source checkpoint starts at `9360ebfc`. The pending integration gate still uses batch base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Scope

Preserve the prepared collection and media loss handling, fresh account proof, original command ownership and regression tests. Source authors and fixers used filtered `tools/mg cleanroom` copies. No reference source or private collection entered those copies.

Five independent review rounds preceded this checkpoint. The sixth review has not run. Later compiler corrections, lifecycle repairs and test changes still require that review. No completed-unit trailer belongs on this checkpoint.

## Verification retained

| Check | Result |
|---|---|
| Rust sync crate, `core-test/20261112T053639Z-17963` | Failed: 719 passed, 15 failed, 7 skipped. Both 150,000-item tests passed. |
| Affected publication and reset tests after the driver correction, `core-test/20261112T055421Z-19751` | Passed: 26 tests, including all 15 earlier failures. This is separate evidence, not a fresh pass of the whole crate. |
| Swift Core lifecycle checks, `swift-test/20261112T063325Z-45236` | Passed: 6 tests, including fresh proof after a second loss and actual SDK cancellation and release. |
| Swift Core matrix, `swift-test/20261112T063841Z-50042` | Passed: 58 tests. Coverage: 37 codes, 2 operation forms, 9 callback forms, plus owner shutdown. |
| Reviews during sync, `swift-test/20261112T063600Z-47610` | Passed: collection and media cases; saved answers, original results, ACKs and account proof checked. |
| Retained publication startup after the test ordering correction, `swift-test/20261112T064908Z-66332` | Passed: 1 test. The test releases original work before joining the later account proof. |
| App ownership stress checks, `swift-test/20261112T064608Z-58860` | Failed: 6 of 9 test groups, 50 issues. Cancellation, owner shutdown, reopen and Resume findings remain open. |

Gate `gate/20261112T014846Z-84391` did not finish. Two Rust tests failed; their repairs are committed with passing focused checks. A full passing gate is still required.

Two startup runs, `swift-test/20261112T062603Z-41621` and `swift-test/20261112T063948Z-51631`, were cancelled through MG after test progress stopped. Their logs and incomplete receipts remain. Fixed stage observations identified the test-owned wait cycle. Temporary stage prints were removed after diagnosis.

## Next delivery

The next checkpoint is a signed Mac and iPhone review round trip: record a review, transfer it through iCloud, verify the other device, then reopen. Verify media with changed inputs. Prior transfer evidence does not prove this complete workflow.

Keep the failed stress checks, wider lifecycle and protected-file matrices, independent final review and the full batch gate pending. Actual encrypted-data-reset behavior and A18 remain unverified. This checkpoint does not grant a waiver or claim a signed acceptance run.

# U107 normal sync — in progress

Starting commit: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Batch base: the same commit. Planned batch: U107, U110, U111.
The start was recorded before spec intake in `build/unit-starts/U107-20261025.json`.
Previous full gate `20261025T110905Z-28599` passed on this base.

## Prepared contract intake

Reuse the neutral U107 packet at `spec/U107`, commit `8da0e61d`.
No existing U107 production implementation was found in the recorded audit.
Spec session: `s-20261025T132312Z-c047b13c`.
Golden intake session: `s-20261025T132746Z-35ca6910`.
The 18 prepared golden assets initially matched their source blobs exactly.
Their original recording commits are `ed4f2b52` and `e407410c`.

Keep Anki reconciliation behind explicit Settings transfers; automatic sync remains
CloudKit. Retain the custom-server wait exception and U29 real notetype-upload
regression. No actual connection values or private fixture data are used.

## Contract review

Independent pre-implementation review found one P2 at plan Approach 6: an extra
missing-note skip contradicted R1527, normal-sync N6.2 and hold-back H10.2.
The sentence now preserves the card's own unsent-grave skip, merges an otherwise
eligible card even when its note is absent, and never recreates the note.
Re-review found no remaining P0–P2 finding in this clause. This is contract intake, not implementation acceptance.

## Open verification

Golden check `20261025T132817Z-84583` failed: five hold-back scenarios lack
`review_session` parser support; schema validation has ten such step errors and
three observations using the two missing sync-error values. Thirteen parseable
prepared scenarios are fresh. The remaining five are uncertified.

Privacy `20261025T132907Z-86068` failed on documents and two generated test rows.
Intake scan `20261025T133335Z-90915` passes after text changes and new numeric fields.
Two oracle records still need observations from those updated inputs. Grammar support
is required first. U107 remains open.

Complete-intake privacy check `20261025T133607Z-94310` rejected only the two old
golden files for the changed test inputs. They are retained under ignored build
storage and will not be published. Both scenarios remain enabled and require
fresh records. The other 16 prepared goldens remain in the intake.

## Grammar support

Filtered implementer session `s-20261025T134357Z-235af9a2` exported support in
`cleanroom/20261025T134422Z-4951`; only standard MG formatting followed.
Rust grammar check `135224Z-17688` passed all 47 cases. Schema test `135212Z-17253`
failed because a marker result was accepted despite its no-output contract.
Independent review confirmed this P2; its earlier clear receipt was superseded.

Filtered fixer `s-20261025T135952Z-cc91c1ec`, run `140017Z-26669`, added one explicit
false property. It preserves draft keys and the original assertion. Schema run
`140155Z-28699` now passes all five inner tests. Re-review has no P0–P2 findings.
Similarity `140154Z-28468` passes. Rust source is unchanged from the 47-case pass.

Prepared oracle support is in `89de0e5c`: all 25 focused tests and privacy passed,
and independent review is clear. The two changed inputs still need oracle records.
The normal executor and its acceptance checks remain outstanding.

## Intake metadata

The early intake commits placed role data in their message bodies. Those pushed
commits stay intact. This table records the source sessions; new code commits use
one contiguous trailer block.

| Source commit | Role | Session |
| --- | --- | --- |
| `b74696b2` | spec-writer | `s-20261025T132312Z-c047b13c` |
| `8e201054`, `89de0e5c` | oracle-maintainer | `s-20261025T132746Z-35ca6910` |
| `1aac94de` | implementer | `s-20261025T134357Z-235af9a2` |

The session ledger and intake packets retain the recorded roles and source hashes.
U107 is not accepted.

## Fixture validation after support

Oracle run `140646Z-33304` recorded only the two changed cases, both successfully.
Goldens check `140717Z-34200`, based on the unit start, reports all 697 scenarios
fresh and valid schemas, including the 18 U107 cases. Asset privacy `140725Z-34489`
passes. The oracle pin and locks are unchanged; no unchanged case was recorded.
This proves the test inputs and expected data, not normal-sync execution. The
production job and its acceptance checks remain outstanding.

## Temporary local macOS feedback

Antoine requested Xcode MCP on this Mac for the current iteration on 2026-10-25.
The working baseline at `3ea7d5dd` built for My Mac in 89.282 seconds with no errors;
launch completed in 12.139 seconds. One local app instance was verified. VM and
mobile iteration checks are deferred, with required integration/final evidence
still outstanding. This baseline does not include the unverified production export.
The local bridge keeps workspace opening and its action in one connection; saved
workspace handles expired between separate calls in this setup. Mac-only catalog
extraction was archived separately and restored for later cross-platform review.

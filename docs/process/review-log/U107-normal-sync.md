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

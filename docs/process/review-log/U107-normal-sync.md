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

## Bounded prerequisite contracts, 2026-10-25

Recovered the four already-authored contract/provenance files from the saved work; every SHA matches the spec-writer packet. The independent review of neutral rename F1–F5 and ChangeSet S1.1 found no P0–P2 findings. Publication privacy passed. Source session remains s-20261025T132312Z-c047b13c. This is the rename-only prerequisite inside U107; full U34 stays in M1 without a Unit trailer. U107 remains incomplete. The batch base remains 1416a520542a8c5daa38a23bfe398da704c3d6c9.

## Session routing and retained payload contracts

Independent review of the published S8.4 per-session routing contract is clear.
The existing native live runner needs a separately owned loopback route for each
session; implementation and the 18 live sync cases remain outstanding.

The storage journal J1–J4 and change-stream S2.1–S2.3 contracts cap retained
before/after row bytes at 8 MiB. They require
immutable same-commit reads, ownership through queued consumers, pre-COMMIT
compact facts and explicit read-failure handling. Independent contract review
found no P0–P2 issues. Privacy `170557Z-96547` and static checks `170631Z-96994`
pass. Earlier rejected prose scans remain recorded in the contract packet.
Implementation, allocation measurements and device limits are not yet verified.

## Production checkpoint and focused diagnosis

Integrated the production export from implementer session
`s-20261025T134357Z-235af9a2`, then the filtered repairs from fixer session
`s-20261025T162405Z-9dc656e4`. Independent review round 1 found a schema race
between metadata and snapshot capture, plus missing undo targets for removed
configuration keys. Both repairs have focused regression cases.

Focused runs `162809Z-61925` and `164931Z-78983` failed during compilation;
neither executed tests. The next run compiled the workspace and is still active
at this checkpoint: 18 cases passed before its first 150k-card case. This is
partial evidence only. Round 2 review, the remaining prerequisites, full focused
results, privacy/similarity checks and the batch gate are pending. No Unit
trailer is added.

Focused run `170327Z-92729` finished: 35/38 pass, including both 150k cases.
Failures remain enabled: post-snapshot `curDeck` pending status, held-deck
reconciliation and the sanity-failure test's admission path. Round 2 source
review confirms the earlier schema/config-undo repairs and finds no new P0–P2
issue; the failed focused checks still require diagnosis. Static checks
`172050Z-9413` pass for the original batch base.

The six-file rename prerequisite has a clear independent source review and is
integrated as WIP. Its first focused run `172233Z-12378` failed compilation:
the new storage regression compares a note type without `PartialEq`. No rename
tests ran. The correction must preserve complete aggregate equality checks.
The initial formatting attempt could not reach Docker under sandbox limits;
the authorized Docker run formatted successfully. Full U34 remains outside
this prerequisite, and neither U34 nor U107 is accepted.

Filtered fixer `172405Z-14357` repaired the storage test comparison and two
invalid server fixtures: a server deck carried client-pending USN -1, and the
sanity test supplied equal metadata that correctly bypassed the round. Their
replacement assertions preserve and extend the original behavior checks.
Independent source review confirms these diagnoses. The same-instant config
pending bug remains open; counting all config USNs would incorrectly include
day-clock bookkeeping and received positive USNs.

The real rename/upload/fresh-peer regression is included from implementer
session `s-20261025T172143Z-1e9723c8`, with a separate filtered repair
`172954Z-18122` for its mistaken template-stamp expectation. Re-review is clear;
template stamps remain captured data. The test exercises complete wire objects
and unknown metadata, with actual normal jobs and unchanged schema stamps.

Focused compilation `173407Z-21279` exposed two more unsupported equality
assertions in the engine rename tests, after the storage compile repair.
No tests executed in that run. Both require complete-data comparisons; the
Use existing stored fields and opaque bytes for these test assertions.
Similarity `172439Z-14974` passed the prior checkpoint (58 files, zero flags).
Later deltas still need their checks. U107 and the batch gate remain open.

Filtered test repair `173619Z-22958` corrected the remaining unsupported
equalities using complete stored scalars and protobuf bytes. It also replaced
text rejected by the private scan; the guard is unchanged. Focused run
`173937Z-25330` compiled and executed: 79 pass, two fail. The actual rename
upload/fresh-peer regression passes, as do corrected sanity and held-deck cases.
The remaining failures are the known same-instant config pending bug and a
rename test that assumes card-ID order equals template-ordinal order
(actual ordinals 5, 0, 1 versus expected 0, 1, 5). Preserve exact membership
and duplicates when correcting this test-only ordering assumption.
The two large sync cases passed in the earlier full focused run; they were not
repeated by this small-case filter. U107 remains unaccepted.

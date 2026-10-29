# U113 adoption: engine dependency

Status: engine dependency verified; no Unit trailer or P24 acceptance.
Phase start: `efcf26cabdcb42653e6adff2811249aff1d97340`.
Tests applied after: `6fce8827`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The approved R6.6/R6.7 tests use the real replacement, DeviceState and aborting
child-process paths. They cover caller values, unchanged stamps, old and new
file proof, every R8 point, no old file, account change, strict reason parsing,
prior reason restore, generation/events, stale handles and active-review refusal.

Test author: `cleanroom/20261028T183637Z-819`.
R1 found missing active-review coverage. Repair
`cleanroom/20261028T184729Z-6776` adds it; R2 is clear of P0-P2.
Production was prepared in a filtered copy against those tests, without a main
checkout change. Its R1 found witness extensions crossing candidate identities.
Repair `cleanroom/20261028T194320Z-4853` adds one real two-Restore regression
for legacy and retained-chain records. Production R2 is clear of P0-P2, but
production remains unapplied for this tests-first commit.

Privacy-only edit `cleanroom/20261028T202850Z-35755` removes comments and
changes one assertion diagnostic. Independent exact-delta review is clear;
comparisons, gates, limits, helpers and test enablement are unchanged.

Observed checks before production application:

- `core-format/20261028T203154Z-37124`: pass.
- `core-test/20261028T203326Z-38025`: expected RED, 29 missing-API errors
  for CloudAdopt, its typed entries and pending reason. Zero tests run; no pass.
- `private/20261028T203326Z-38232`: both test targets pass.
- `similarity/20261028T203324Z-37793`: both test targets pass.

Production applied after `c15df1b5`, which also records publication's enabled
tests before production. The lifecycle bridge appends CloudAdopt without changing
existing wire tags. Its independent review is clear of P0-P2.

The first replacement run passed 40 of 41 tests. The new active-review test had
created staging before open, whose sweep correctly removed it. Filtered repair
`cleanroom/20261028T204256Z-42353` creates staging after open and retains the
checkpointed bytes and every assertion. Exact-delta review is clear. This was a
fixture failure before the refusal assertion, not a failed production guard.

Final source cleanup `cleanroom/20261028T211046Z-58104` removes an unused
private helper and one comment. Exact-delta review is clear; proof checks and
assertions are unchanged.

Observed production checks:

- `core-test/20261028T210155Z-48948`: all 41 replacement tests pass.
- `core-test/20261028T210344Z-49701`: 116 engine, lifecycle, open, session,
  suspension and WAL tests pass.
- `swift-test/20261028T210616Z-51480`: all 165 CoreBridge cases pass with
  regenerated bindings.
- After the final helper/comment cleanup,
  `core-test/20261028T211750Z-60925`: all 90 engine library and replacement
  tests pass, none skipped. Surrounding and bridge behavior is unchanged.
- `core-format/20261028T211402Z-59482`: pass.
- `private/20261028T212316Z-63520`: all nine changed source files pass.
- `similarity/20261028T212319Z-63779`: nine files, zero findings or exemptions.
- `core-lint/20261028T212313Z-63258`: still RED only on the two missing-import
  groups in the enabled publication test target. The obsolete-helper error is
  fixed. No lint pass is claimed; publication production remains required.

Next: connect publication and adoption to the real workflow. This dependency
alone does not deliver media, signed checks or complete iCloud sync. The earlier
incomplete integration gate remains unaccepted and the batch retains its base.

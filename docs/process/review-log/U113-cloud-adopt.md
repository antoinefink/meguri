# U113 adoption: engine dependency

Status: tests first; no Unit trailer or P24 acceptance.
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

Next: commit publication's enabled P24 tests too, then apply the reviewed engine
source and lossless lifecycle bridge. Compile and run focused replacement,
recovery and lifecycle checks. This dependency alone does not deliver adoption,
media, signed checks or complete iCloud sync.

# Creation-time admission repair

Starting commit: `b6201307`. Role: fixer.
Session: `s-20261110T214025Z-20fa15fd`.
Authority: approved `sync/policy#P6`, `sync/cloudkit-records#K20`.

Test-first checkpoint only. No production behavior changed and no unit or gate
is claimed. The expected failing tests remain enabled while repair proceeds.

Filtered test author: `cleanroom/20261112T201505Z-19202`.
Initial run `core-test/20261112T202205Z-20356` failed during fixture setup,
not at the target behavior. Its failures are not product regression evidence.
Filtered fixture repair `cleanroom/20261112T202444Z-21221` orders account/policy
inputs before SDK registration, and models the old store from an actual complete
publication after closing every owner. It preserves account, binding, epoch and
Saved receipts, removes only the new ledger and uses the V2 marker before actual
open/migration. No policy facts or epoch authority are set by the tests.

RED `core-test/20261112T202832Z-21986` compiles and reaches all three intended
failures:

- A committed local creation-time change still produces a nonempty batch.
- An absent legacy authority still produces a nonempty batch.
- A valid fetched unequal epoch is recorded, but the summary omits mismatch.

The tests also require real Undo to restore equality, a real accepted complete
epoch to resolve Unknown, no binding isolation, no stopped engine, and the
existing known-mismatch Sync Now outcome. Source privacy passes after rewording
one comment. Independent review and green verification follow the repair.

The read trigger for an Unknown legacy value with automatic sync off needs a
narrow reviewed contract amendment; this checkpoint does not implement it or
claim its ordering is already approved. Send and apply admission must stay
closed without established equality. No new status or fallback is authorized.

Pending batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

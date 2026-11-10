# Exact media refusal diagnostics

Fixer baseline: supplied filtered copy of root's current master. Original batch
base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Supplied native evidence: manual real Core/Service flow completes; automatic
preparation refuses with fixed tags `Tracking` / `RecoveryRequired`. The exact
Core predicate remains unknown. This change makes no cause or acceptance claim.

Diagnostics only, under `test-hooks`: split the existing ledger refusal in its
original short-circuit order into `revisionMismatch` and `feedRegression`;
label the existing refusals `applyCommitScope` and `applyPriorEntity`. Add
error-only `ledgerWrite`, `feedCommit` and valid-selection `stagedApply` sites
using the existing typed fixed refusal classifier. Logs contain fixed labels
and classifier tags only. Existing errors, reads, calls, guards, mutex scopes,
rollback and mutation order are preserved. No Release logging is added.

One source consistency check: exited 1 with `AssertionError` in its propagation
syntax assertion. Earlier assertions confirmed all seven feature-gated sites,
fixed classifier use, the single ledger read/decode and ordered predicates.
The checker incorrectly required `changed?;`: that result returns from the
tracking closure and propagates through the existing `})?;`. No recheck run.
No Git, history, network, builds or tests used.
Root owns compilation, both existing native variants, focused verification,
lint, privacy/similarity checks and independent review. Acceptance pending.

## Root result

Independent source review found no P0–P2 issue. Privacy and similarity passed.
`swift-test/20261110T123520Z-95914` compiled and ran both variants: manual
completed; automatic failed. The exact predicate is revisionMismatch at
feedCommit. No cursor regression or staged-apply refusal was reported. This
proves a stale feed proposal, not that the competing state was identical.
Keep every conflicting-write refusal.

The first lint run failed formatting only. Root applied core-format after the
Apple run exited; the follow-up lint result is retained separately. No behavior
change or acceptance is claimed.

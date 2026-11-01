# Signed CloudKit evidence contract

Status: specification correction only; U248 and all signed lanes remain unaccepted.

Starting commit: `c6c912a1892683488e69984916c27106a6ae9296`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Spec-writer session: `s-20261101T204048Z-b8f3c3a4`.

The MG result schema forbids extra top-level fields. The corrected contract
keeps that envelope, puts lane and scoped outcomes in `details`, and requires
a validated evidence artifact tied to the actual run, build and device logs.
Per-leg results cannot claim a whole-lane pass. Required assisted steps,
same-commit aggregation, permitted unverified assumptions and the eight-hour
soak remain required.

The signed engine leg now includes A23, already required by the record contract.
The first-sync contract names six real receipt scopes and requires validation
of their underlying receipts and evidence. Copied success fields cannot satisfy
those requirements.

Independent review: `native_bounds_review`, one round, no P0–P2 findings. Review
covered schema compatibility, A23, scope mapping, same-commit aggregation and
the distinction between fake, per-leg and signed whole-lane evidence.

No runner, signed delivery or first-sync validator repair is implemented by this
change. Their pending markers remain. No build or live CloudKit run is claimed.
Focused verification is the independent contract review, patch whitespace
check and privacy scan. Privacy receipt:
`private/20261101T204146Z-91013`, status pass, two files checked and no findings.
`git diff --check` passed.

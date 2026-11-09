# Seeded cloud runner in progress

U117 phase start: `6262fc4f`; original unit: `f7a83f2b`.
Original batch: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261109T094715Z-c7c7c45e`, export `38212`.
Fixer: `s-20261103T133750Z-f276f58e`, export `41644`.

The runner uses actual published/adopted CoreHost children and original
request/callback receipts. Its test-only ChaCha20 source leaves legacy RNGs
unchanged. Shared rig transitions retain original callback and file ownership.
Full Q4.4 remains unproved: six required observation categories are missing.
Whole Q5, Q6, fake lanes, faults, soak and U117 acceptance remain open.

Compile `40674` failed before tests. Its corrected field-byte assertion retains
the exact intended value. Independent source and repair reviews are clear.
Focused `20261109T212157Z-42917` passes all five tests, no skips: four behavior
cases and the child IPC entry. Real two/four-device exchange, retained callbacks,
paginated fetches and seeded replay are exercised. No positive drain is claimed.

Lint `45271` finds two style issues; repair is pending. Similarity checks pass.
The original failed integration gate and native UI failure remain open.

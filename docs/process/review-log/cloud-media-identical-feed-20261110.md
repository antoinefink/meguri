# Cloud media identical-feed review — 2026-11-10

Role: fixer. Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The new branch in `commit_media_feed` returns `Ok(false)` before writes. It requires:

- Empty `file_changes` and no `apply_commit`.
- An exact `Some(feed)` identity and cursor match.
- A greater current revision accepted by `runtime_store::checked`.
- Full ledger equality except revision, after `update_blob_reference_age`.

The generic CAS and its errors remain. Runtime authority, pending notifications
and retained-floor updates still run.

The corrected fixture checks current-open whole and note scans and asserts `!scan_drained`.

Recorded evidence:

- Before production change, run 23201: both actual real-consumer completion orders fail on the second completion with `Tracking(RecoveryRequired)`; all preconditions and first-commit state checks pass.
- Old-store run 20589: all six refusal cases pass; all four duplicate-positive cases fail.
- After production change, `core-test/20261110T125829Z-24016`: all 51 focused Rust checks pass, including both completion orders, the ten new checks, existing R1 checks, J6 feed, completeness, floor and identity coverage.
- Independent R2 review: clear at P0–P2. Similarity 25836 passes. Lint 25556 found one nested-if style error in the test; its correction and final check are pending.

This follow-up deletes only the three specified source comment paragraphs and replaces the review prose. Executable source, assertions, test names, gates, limits and fixtures are preserved. A single source-only byte comparison verifies the exact deletions; the 51 tests need no repeat for these comment-only changes. Root will rerun privacy; the normal integration gate remains due later. The native automatic rerun is pending. No signed, milestone or Unit completion is claimed.

## Native result and final delta

`swift-test/20261110T130342Z-31543` passes both real Core/Service variants,
including the earlier automatic ACK. Actual edit, original durable ACK and SDK
removal, peer fetch and WAL apply, data equality and owner refusals complete.
This does not establish signed CloudKit delivery.

R3 review is clear. The last executable delta collapses a nested test condition
without changing call order or waits. Both real consumer orders pass again in
`core-test/20261110T130602Z-33810`. All 51 focused checks passed before this
equivalent style change. The generic ledger CAS is unchanged.

Source privacy passes in 31209; root review prose passes in 32853.
Lint 25556 remains the retained style failure until the final check completes.
The original batch gate and signed multi-device work remain due.

Final lint `core-lint/20261110T130652Z-34191` passes all four checks.
Final similarity `20261110T130716Z-35993` and privacy
`20261110T130712Z-35728` pass. The earlier failed lint remains retained.

## An unchanged current feed during media preparation

Starting commit `6129e37c`. The earlier automatic save case in
`swift-test/20261110T223358Z-77710` reported `revisionMismatch` at `ledgerWrite`
in media preparation and stalled. Temporary tracing in
`swift-test/20261110T224119Z-84159` passes both save cases. It shows 79 feed
writes with unchanged ledger content, but does not reproduce the stall. The
temporary tracing was removed.

The new test holds root-read preparation after it reads the ledger. It consumes
the real empty feed on the same binding before releasing that preparation.
The unchanged feed increments the ledger revision on the starting code, and
preparation fails with `Tracking(RecoveryRequired)` in
`core-test/20261110T224349Z-85944`.

The existing no-write rule now accepts the current revision as well as a
greater valid revision. It still requires the same recorded feed and cursor,
full ledger equality after reference-age calculation, no file changes and no
apply commit. It preserves the generic revision check and all authority checks.
Preparation then issues a root read with valid authority. The focused run
`core-test/20261110T224432Z-86289` passes all seven tests.

The first wider run `core-test/20261110T224516Z-86944` passes 60 of 62. Both
failures expected the first empty consumer to increment the revision. Their
fixtures already prove the feed is current. Both now compare the first
completion to the full baseline, including the revision, then compare the
second completion to the first. The gates, deadlines, real consumer calls and
all other assertions stay in place. Run `core-test/20261110T224553Z-87263`
passes all 62 publication, media and store tests, including both orders and
the retained refusals for differing ledger contents and invalid cursors.

`swift-test/20261110T224612Z-87886` completes the case that previously stalled,
including both manual and automatic save variants. It passes 14 of 15 test
groups, including review and media adoption. The remaining group fails in both
variants while waiting for an actual newer edit to update the same SDK pending
name. Full pending reconciliation in `6129e37c` treats that edit like a repeat
of the original entry. The tests still require the newer pending revision and
the refusal of the older removal proof. This is a separate open regression.

Direct source review checked the no-write rule against the existing feed,
reference age, apply, cursor and authority checks. The combined app suite,
full batch gate and signed CloudKit round trip remain open.

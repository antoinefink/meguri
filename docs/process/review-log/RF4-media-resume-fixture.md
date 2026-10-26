# RF4 — media exclusion changes on resume

Supplemental start: `e36d5e1271b44cbb69fdba058138ca849e550aa6`, recorded
before work in `build/unit-starts/RF4-media-resume-fixture-20261026.json`.
Fixture implementation starts at `91fa8317`. The pending integration base stays
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Broad Rust run `035121Z-52222` fails the interrupted-exclusion test at its final
script assertion. Two excluded files are overwritten in place after close.
Those writes do not change the directory timestamp, so the persisted marker
fast path skips listing them on reopen. This behavior follows media tracking
T4.3 and sync Y3. The fixture did not model a directory-visible outside change.

Filtered fixer `s-20261025T162405Z-9dc656e4`, export
`cleanroom/20261026T042639Z-68834`, keeps both real edits and sets the fixture
directory timestamp to its previous value plus one second. It asserts the
observed timestamp and difference before the real reopen/resume. Independent
R2 review finds no P0–P2. No production scanner, reset, admission or transport
is changed; no pre-registration, forced scan, retry or sleep is introduced.

Real interruption, close/reopen, retained operation, consent, three uploads,
empty exclusion lists, receipts, hash checks and every scripted request remain
required. The final script assertion is unchanged. Passing the original method
with this sole precondition repair confirms the marker diagnosis; the original
failure did not print the exact remaining request names.

Shared focused receipts cover both fixture repairs in this batch.

- `core-test/20261026T043001Z-70870`: both original failing methods pass.
- `core-test/20261026T043120Z-71612`: all 57 nearby tests pass, zero skips.
- `core-lint/20261026T043322Z-73349`: PASS, all four checks.
- `private/20261026T043143Z-71986` and
  `similarity/20261026T043145Z-72225`: PASS for both fixture files.

The original failed Rust run and disk-cancelled gate `035002Z-43555` remain
retained. This fixes the fixture; it does not complete live media conformance,
the first-delivery acceptance checks or the original-base integration gate.

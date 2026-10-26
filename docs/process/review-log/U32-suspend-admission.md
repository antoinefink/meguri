# U32 — continued-job suspension fixture

Supplemental start: `e36d5e1271b44cbb69fdba058138ca849e550aa6`, recorded
before work in `build/unit-starts/U32-suspend-admission-20261026.json`.
Integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The broad gate `20261026T035002Z-43555` was cancelled when free disk fell below
40 GB. Completed child results remain retained. Rust `035121Z-52222` has
3,094 passed, two failed and ten skipped; native durability `041414Z-59768`
has 24 passed and this same suspension test failed. Remaining gate checks are
incomplete, not passing. Cleanup restored roughly 60 GB without removing exports,
results, logs, reports or prepared work.

## Diagnosis and contract clarification

The test starts a continued one-way job, then unwraps admission of a distinct
normal sync. Admission returns Busy naming the one-way holder. The test aborts
before calling suspend; dropping its completion sender causes the secondary
worker receive error. Those failures do not prove a suspension implementation bug.

A separate spec-writer audit found an unqualified promise in lifecycle L5 and
the matching U24 bullet. More specific transfer contracts M2/N15 and O4.6 exclude
independent sync overlap with one-way work. Session
`s-20261026T041939Z-a8f043ce` clarifies L5/L9, J3 and that exact plan bullet.
The change preserves Ready answers/edits, unrelated admissible jobs, no long-slot
ownership by normal/media sync and their ability to overlap each other. Internal
phases of one authorized transfer remain distinct from independent operations.

Independent review finds no P0–P2 in the narrow clarification. No Anki source,
reference, oracle or private material was read. The broader current NormalSync
refusal under every slot holder is not resolved or accepted by this clarification.

- `design-docs/20261026T042330Z-66704`: PASS.
- `static-checks/20261026T042330Z-66928`: PASS.
- `private/20261026T042331Z-67142`: PASS for the three public documents.

## Fixture repair and focused checks

Shared focused receipts below cover both fixture repairs in this batch.

Filtered fixer `s-20261025T162405Z-9dc656e4`, export
`cleanroom/20261026T042639Z-68834`, adds an exact Busy assertion and proves the
refused normal worker was discarded through its channel. The unrelated ordinary
worker is now Optimizer, with an entry handshake before suspension. Independent
R2 review finds no P0–P2. The production guard, both two-second suspend deadlines,
exact Deferred identity, continued-token state, ordinary cancellation, explicit
release, final Suspended and empty-thread assertions are retained.

- `goldens-check/20261026T042708Z-69384`: PASS, 703 golden checks, zero changes;
  1,525 schema files have no errors after the documentation clarification.
- `core-test/20261026T043001Z-70870`: both original failing methods pass.
- `core-test/20261026T043120Z-71612`: all 57 suspension/manual-transfer tests
  pass, with zero skips.
- `native-durability/20261026T043207Z-72624`: all 25 pass, with zero skips;
  this closes the native instance of the same suspension failure.
- `core-lint/20261026T043322Z-73349`: PASS, all four checks.
- `private/20261026T043143Z-71986` and
  `similarity/20261026T043145Z-72225`: PASS for both fixture files.

The media fixture has its separate record. The disk-stopped broad gate remains
incomplete and unaccepted. No unit or milestone acceptance is claimed here.

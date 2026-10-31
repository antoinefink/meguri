# Ordinary deletion repair

Source start: `8d7355e7a6d447737ca7a0ce97fd86dba39495e9`.
Continuation base: `6c3105f80abefc0d6fcec9bb305ec4a6be1ccdb2`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Session: `s-20261025T162405Z-9dc656e4` (fixer).

Authority: cloudkit-records K10, K12.3, K13.1–3, K17.1–3;
cloudkit-recovery V3.4; policy P7.1–2, P8.1.

Filtered exports: `20261030T152307Z-72104`, `20261031T134816Z-85811`,
`20261031T141202Z-97312`, `20261031T142709Z-5733`.

Changes:

- Note/Card removal uses committed row images and conditional server facts.
  Schema version 4 protects these stored facts from older readers.
- Own saves require the persisted envelope identity before any row mutation.
  Restored rows wait for rekey; no false ACK retires their flights.
- Recovered Undo uses durable engine evidence.
  Every changed key needs its own later observer image in that engine instance.
  A stored result counter alone proves no durability after reopen.
- A live Card removed by its Note in the same op becomes a pending tombstone
  against its real server version, including a newly received Card.
- Deletion time uses committed Col.mod even without a Col journal change.
- Stale or closed batch requests return empty before side effects (V3.4).
- Raw inbox storage retains exact opaque entries and cursor bytes.
- Review fixtures select the actual deck before their baseline. Strict pending
  and no-Modify assertions remain. SDKRig consumes actual ordered updates.

Independent review: R1 and R2 P1 findings repaired; R3 remaining mixed Undo/edit
P1 repaired in R4. R4 has no P0/P1. Native callback timing is still unproved:
SDKRig consumes builder updates immediately, Swift consumes them later.

R5 source-only continuation from `6c3105f8`, preserving the prepared seed:

- Whole scans retain explicit pending and clean ACKed Note/Card deletion facts.
  Missing live keys still block. Snapshot row counts exclude retained deletions;
  the coverage ledger records those facts separately. Existing guards remain.
- Added real pending-deletion reopen proof and ACKed-deletion rescan assertions.
  Lost Service replies now assert Undo refusal after handoff, absent rows and
  exact fetched-flight ACK. The envelope/revision mismatch case is unchanged.
- Held deletion uses automatic driving before ReviewSession end. Empty batches,
  pending facts and no-Modify assertions remain strict.
- Rephrased the indicated comments and shortened the held Undo test name.

Verification:

- `core-test/20261031T140722Z-93161`: compile error in the new reopen fixture;
  repaired using its real directory.
- `core-test/20261031T142300Z-4500`: 34 passed, 13 failed. This led to the
  committed-metadata, pending-update and stale-builder repairs.
- `core-test/20261031T143631Z-13872`: 44 passed, four failed, zero skipped.
  The original five failures pass; four new deletion cases still fail.
- R5: one source consistency pass; builds, tests, formatter, privacy, provenance,
  similarity and full gate unrun in the filtered runner. No network or commits.
  Prior log privacy:
  `privacy/20261031T143853Z-15164` passed before these source edits.
- Actual R5 `core-test/20261031T144839Z-19050`: 49 ran, 48 passed, one failed,
  zero skipped. Every original full-gate failure and the actual pending/ACKed
  deletion reopen, power-loss replay, own lost reply and valid held Undo cases
  pass. Only the new held actual-delete test fails strict no-Modify at line 1609.
  Independent review traced initial real Config/curDeck selection after baseline.
- `private/20261031T144834Z-18306`: 29 files scanned; only the two identified
  public test texts flagged. `similarity/20261031T144947Z-20775` passes R5 source.

R6 source-only from `6c3105f8`: begin/end/drain a no-answer setup session and
drive automatic saves before the held-delete baseline (P8.1/M2). Assert actual
Service Config/curDeck equals this Note's actual Card.did. The held interval and
all no-Modify, image, tombstone, ACK, Undo-refusal and reopen assertions remain.
Test behavior is unchanged by this label-only maintenance.

Completed deletion checkpoint verification (integrator receipts):

- `core-test/20261031T145700Z-26689`: 49/49 passed.
- `native-durability/20261031T145434Z-22677`: 95/95 passed.
- Native Swift: 36/36 passed. Independent R6 review clear.
- `private/20261031T145703Z-26959` flagged only the test label and its
  review-note sentence; both are replaced in this export. Privacy has not yet
  passed for this export; the integrator must run it.
- One scoped source consistency check; no network, build, test, formatter or
  commit in this filtered runner. Provenance/similarity and the original-base
  gate remain for integration; no new behavioral review or fix round.

Original-base gate and broader absence/ACK/cursor, schema ownership, Host/FFI
and native durability checks remain required. Rekey, unknown identities,
structural generations and native assets remain open. Core pa is unfinished
(four FFI tests RED); its separate implementer's changes are absent from this seed.
Adoption, retries and lifecycle gaps remain open as recorded in the native log.
No Unit, P24, Q6, F5 or full-workflow acceptance.

Integrator checks after R6:

- `core-test/20261031T145700Z-26689`: 49 passed, zero skipped.
- `core-test/20261031T150304Z-29966`: 215 related engine, Host and cloud tests
  passed, zero skipped.
- `native-durability/20261031T145434Z-22677`: 95 passed, zero skipped.
- `core-lint/20261031T150503Z-31452`: pass after two test borrowing corrections.
- `private/20261031T150508Z-31715`: all 29 selected files clear.
- Filtered label cleanup changed no test behavior. Its review note preserves the
  failure history. The full original-base gate remains pending.

Final label/borrowing checks: `core-test/20261031T150649Z-32562` passed the
four selected cases; 23 other cases were outside that filter and remain covered
by the full 49-case receipt above. Final similarity:
`similarity/20261031T150653Z-32828`, pass. Final note privacy:
`private/20261031T150701Z-33224`, pass before this receipt addition.

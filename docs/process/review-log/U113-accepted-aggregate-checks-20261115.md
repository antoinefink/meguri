# Incoming aggregate guard checks

Prepared work starts at `e0729f4ed`. The full pending batch keeps base `1416a520542a8c5daa38a23bfe398da704c3d6c9`. This record accepts no Unit or milestone.

The approved S2.5 exception now has a scoped implementation. The helper writes the final accepted aggregate itself. The executor checks its actual source. Extra writes, removals and note-type moves retain their guards. The exception supplies no map or identity authority. Savepoints restore its facts; rollback discards them.

Independent review found that an unchanged-name replacement could keep an older native map. Three new executor tests reproduced that defect. The repair records applied IDs regardless of semantic similarity, drops affected maps and supersedes open tokens. It keeps outside-write invalidation. All other prepared framework findings remain open.

Author: `s-20261110T214025Z-20fa15fd`. Independent reviewer: `s-20261109T210639Z-47d06d5b`.

| Review | Result and repair |
|---|---|
| `204054-58423` | P1: same-name replacement retained map authority. Test author `204709-63159`; actual three-case RED `205211-64852`; journal-only repair `205328-65724`. |
| `205600-67485` | Production repair clear of P0/P1. Three P2 findings in new host fixtures: unused fault hooks, missing exact authority, and an unsupported immediate-flush expectation. |
| `211453-76246` | Corrected host fixtures clear of P0–P2. A valid unrelated native report reaches actual read/flush boundaries; the incoming target gains no authority. |
| `212124-81250` | Source comparison cleared the scan helper, but actual run `212130-81703` failed its drain predicate. That failure prevailed over the review. |
| `212530-84928` | Final helper and uses clear of P0–P2. It checks live full-scan and classification coverage, matching snapshot/WAL facts, and whole entity equality. |

The old plain-save fixture renamed a field while keeping its old template reference. A one-line reference correction preserves every assertion and reaches the intended guard. New recovery fixtures also needed corrections: refusal need not flush; metadata similarity is not identity authority; an intent can retain its before digest without another before-body; provider checks follow actual error precedence. No production mechanism was added to make a test fault fire.

Successful scans refresh provenance. The two final comparisons derive open, sequence and generation from independent stored/snapshot facts, assert current-open WAL coverage, update only those three expected fields, then compare the whole entity. Data, origin, base, ACK, revisions, marks, flights and server metadata stay checked. A drained binding is distinct from a completed live scan.

Verification:

- `core-test/20261115T202731Z-42184`: original local production regression RED, wrapped as LocalStorage; its exact inner error was not observed.
- `204121-59718`: 29/30; original regression and eleven new guard cases pass. The old invalid fixture fails.
- `204605-61494`: 70/70 after the fixture correction.
- `205211-64852`: three actual map-authority failures, before the journal repair.
- `205625-68047`: 76/83; all 73 source cases pass, with seven new host-fixture failures. Its no-environment worker is not behavior evidence.
- `211519-76827`: 81/83 after fixture repair, excluding the direct worker. Its parent executes the actual intent fault. All source cases and eight host cases pass.
- `212130-81703`: two cases fail the incorrect drain predicate.
- `212552-85527`: both remaining cases pass after the test-only correction. The other 81 cases and production bytes are unchanged. These are combined focused receipts, not one 83/83 run.

Host checks cover live refusal, read/flush errors before separate validation, whole callback loss, reopen, exact admitted authority, WAL-gated handoff, retained intent/inbox/base/ACK/flights, and provider error classification. They use the real CoreHost writer and observer with synthetic fixtures and a controlled Service. They are not Apple SDK or physical-peer proof.

Final prepared artifact: `build/cloud-resume-diagnosis/structure-report-prepared-incoming-20261115.patch`, SHA-256 `3b2059814fa369cee81861b74f3d962468eea0cb1c51c6a86e2942c26dd9c678` (23 files). It preserves earlier prepared work; older pinned artifacts remain unchanged.

Privacy `212619-86690` passes all 23 files with the guard unchanged. Similarity `210535-74031` passes 22 scanned files, zero flags/allowlists and one excluded file. Only the host test module changed afterward; its final scan `212637-87192` passes one file with zero flags/allowlists. The integrator read only filtered implementer views. Editorial corrections changed comments, a test name, local names and a synthetic label; executable production bytes stayed unchanged.

Core lint `212749-88539` passes three required checks. The integrator restored clean master after verifying the exact prepared source. Code remains prepared: cloze composition, new-type admission, nested token ownership, other fixture migrations, and full generation integration are still open. The full gate, U113/U114 acceptance and real Mac–iPhone bidirectional review proof remain outstanding.

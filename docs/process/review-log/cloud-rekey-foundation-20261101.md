# Cloud re-key foundation

Component start: `0d519bd4562953cb6a69380ab46f68fb72793cde`.
Original integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
U113 and U114 remain unaccepted.

The initial baseline parked collisions and own landed tombstones with live Undo rows.
K14.2 requires identity and reference writes, a persisted intent, a covered WAL
sequence and one tracking transaction. K19 generations and media-name conflict
paths remain unaccepted.

Tests-first export: `cleanroom/20261101T002821Z-54292`.

- Note: different GUID and origin, unacked local identity, one tracking-only
  reserved successor and one occupied row. The moved identity must use the
  first available gap; its Cards must reference that new identity.
- Card: different note reference and origin, with history from actual reviews.
  Re-key must move the local Card and its Revlog references.
- Tombstone: a production-built deletion lands through Service and its reply
  is lost. Fixture rows are recreated with the child stopped; native deletion
  and Undo then restore them. The old tombstone stays final and the live rows
  must move to a fresh pending identity.
- Controls cover same-GUID origin adoption and refused acked-origin mismatch.

`core-test/20261101T003614Z-55706` compiles and runs six tests: three controls
or harness cases pass; the three behavior tests fail at their missing re-key
outcomes. Their prerequisite assertions pass. This is expected-red evidence,
not unit acceptance.

Independent tests-first review finds no P0–P2 issues. The test packet does not
cover all six kinds, failure/status handling, held reviews, undo effects or the
crash matrix. Those remain required implementation and verification work.

Packet cleanup `cleanroom/20261101T003927Z-57315` changes one test name,
one diagnostic and a comment. Test inputs and assertion conditions are fixed.
The final packet passes `private/20261101T004339Z-58786` and
`similarity/20261101T004340Z-59012`; Rust formatting is applied.
The production re-key path and its remaining matrix are the next work.

First implementation export: `cleanroom/20261101T004422Z-59522`.
It adds journaled identity/reference writers, paired re-key/apply intents and
WAL-qualified tracking movement. Workflow format 2 fences older readers; the
collection schema is unchanged. This export is explicitly partial.

`core-test/20261101T010831Z-69813` compiles ten tests: four pass and six fail.
The controls and cap case pass. Note/Card moves and restart tests fail with
the storage error family; cancellation is marked failed instead of ready.
The new save-conflict test also compares facts from before its helper builds
the handed flight, so its receipt-boundary expectation needs inspection.

Independent component review R1 requires:

- P1: align enabled Config tombstone handoff with persisted-flight validation;
  a lost reply must not make the sidecar unreadable at reopen.
- P1: remove simultaneous hydrated old, moved and cloned reference images;
  prove the existing heap bound with large Note and dependent Card payloads.
- P2: a latest-Revlog identity move must also journal and dirty the Card whose
  derived last-review identity changes, preserving its scheduling fields.

Full six-kind and same-object coverage, mixed-digest reconciliation,
re-creation beneath final tombstones without a new inbox entry, remaining
tombstone paths, review holds, undo effects and fault/concurrency cases remain
acceptance blockers. K19 and media-name work is not closed by this packet.

Repair export: `cleanroom/20261101T012322Z-72400`; source base `36945dd6`.
Only the export's changes from its input patch were applied. SQL paging,
tracking headers, latest-Revlog Card journals and Config flight validation
changed in this repair.

`core-format/20261101T013130Z-74854` passes.
`core-test/20261101T013147Z-75244` compiles thirteen tests: ten pass and three
fail, with no skips. The original Note/Card, save-conflict and own-tombstone
cases now pass, as do cancellation/rollback and the latest-Revlog case.

Open failures:

- Config rename coverage fails during its initial fixture sync, before the
  collision, with `RuntimeStaleIncarnation`; reopen coverage is not proven.
- The crash-boundary case fails to find the server Note after reopening.
- The large Note/Card case preserves its asserted rows and ACK state but
  reaches an absolute child Rust heap peak of 421,246,794 bytes, exceeding
  the unchanged 300,000,000-byte limit.

R2 review and focused repair remain required. No unit or integration-gate
acceptance follows from this result.

Independent R2 keeps two P1 findings open. Config validation uses current
revision/deletion facts for an older handed tombstone, so a supported newer
Config set can persist state that reopen rejects. Historical flight proof
must survive that local edit. The measured heap failure also remains open;
discarded payload encoding and buffered digest encoding still overlap full
row images. Exact codec bytes and validation rules must be preserved.
The latest-Revlog repair passes its focused case and is clear in source review.

The next filtered repair receives both findings and all three actual failures.
Its tests must retain the original payload sizes, absolute heap limit and
row/reference/ACK assertions. Crash and Config setup causes remain unproven.
`doctor/20261101T013357Z-76235` reports eleven passes, three warnings and no
failures. The required one-time evidence remains open.

Second repair export: `cleanroom/20261101T013652Z-77125`.
Test receipt: `core-test/20261101T014458Z-79622`; 12 pass, 2 fail, 0 skipped.
Config deletion and subsequent LocalApi-edit reopen cases both pass.
Config is in the fixture before publication. Adding Config after adoption
remains an open requirement; that earlier failure still needs its own test.

Crash diagnostics locate the failure at `ApplyAfterIntent`. Reopen has one
paired intent and one Ready inbox entry; after driving, the intent is gone
but that Ready entry remains, with no retry count or ACK progress.
The heap peak is unchanged at 421,246,794 bytes. Stage readings are needed
before another memory repair; the same payload sizes and cap remain enabled.

`core-test/20261101T014554Z-80084` passes all seventeen selected codec,
journal and tracking tests. Seventy-seven tests are outside that selection.
These include all-kind canonical byte/hash/count and error equivalence,
spill encoding/checksum, and historical Config flight validation.
Neither focused result closes the unit or integration gate.

R3 review clears the codec/journal and historical Config repairs. Two P1
findings remain: all-before re-key replay clears its intent without continuing
the original Ready entry through the writer, and the measured heap cap fails.
Keep new-Config-after-adoption coverage enabled as well. The next repair must
use fixed stage samples and retain the cumulative absolute peak.

Third repair: `cleanroom/20261101T014855Z-82262`, from `7450721c`.
It continues the original inbox entries after identity-intent replay and
admits journal-proven new Config rows through ordinary observer capture.
`core-test/20261101T015208Z-83635`: 14 pass, 1 fail, 0 skipped.
All three crash boundaries now pass. The enabled new-Config-after-adoption
case passes capture, Service ACK, row/identity preservation and reopen.

Heap diagnostics retain the original reset, sizes, cap and proof assertions.
After drain the peak is 421,246,794; the proof checks do not increase it.
The first `ApplyAfterIntent` reading is 253,481,445, with 1,813,663 live bytes.
The first `ApplyAfterCommit` reading is already 421,246,794. Inspect that
writer/commit/observer interval before further memory changes.

R4 clears restart continuation and new Config capture. It finds a P1 in
observer capture: protected intent keys are checked only after journal and
committed row images have been read. Use metadata-only keys first, then retain
the existing image reads and validation for unprotected keys. The unchanged
stress must establish whether that repair is sufficient.

Fourth repair: `cleanroom/20261101T015502Z-85028`.
`core-test/20261101T015759Z-86310`: 14 pass, 1 fail; heap peak unchanged.
`core-test/20261101T015930Z-86896`: both selected metadata-key tests pass.
R5 finds no new P0/P1 in metadata-first capture; the memory failure stays open.

Source attribution locates another allocation in `changes::prepare`: it holds
raw Card before/after images while decoding both complete Card models only
to read note identity. Scalar validation can be shared without parsing data;
the full record reader must retain its existing behavior and error order.

`core-test/20261101T020116Z-87489`: 54 pass, 1 fail. The first-read asset
conflict case ends with test IPC EOF at the shared save-conflict helper.
That helper now requests complete EntityFacts after handoff; the earlier
API returned unit and did not issue that diagnostic read. Restore bounded
normal helper behavior while keeping the small re-key fixture's exact proof.

Fifth repair: `cleanroom/20261101T020348Z-88138`.
The full Card reader and the compact identity projection share scalar checks.
Commit preparation no longer parses Card data solely to obtain note identity.
The ordinary asset-conflict helper returns unit without diagnostic hydration;
only the small re-key fixture opts into its pre-handler handoff snapshot.

Final component checks:

- `core-test/20261101T020659Z-89415`: 20/20 pass. This includes all 15 re-key
  cases, the unchanged 80 MiB Note plus 80 MiB Card absolute heap cap, and
  five scalar-decoding equivalence/error-order cases.
- `core-test/20261101T020800Z-89913`: 55/55 pass, including the original
  large-asset first-read conflict regression and incoming reviews/recovery.
- `core-test/20261101T020831Z-90299`: 21/21 engine changes/observer tests pass,
  including malformed-card rollback before commit and event publication.

R6 independent component review finds no remaining P0/P1 code findings.
The subsequent actual stress run closes the measured Rust heap failure;
native process RSS is separate evidence and remains required.
No assertions, payload sizes, heap limit, retry rules or IPC cap were relaxed.

This is a partial implementation checkpoint. Full six-kind/same-object work,
mixed-digest recovery, remaining tombstone paths, structural generations,
media-name conflicts, the complete concurrency/fault matrix and the original
integration/milestone gates remain open. U113 and U114 have no acceptance here.

Final source checks pass: `core-format/20261101T020649Z-89120`,
`core-lint/20261101T020926Z-90686`, `private/20261101T021011Z-91111` and
`similarity/20261101T021025Z-91385`. Similarity reports 9 files, 0 flags and
0 allowlist matches. These focused receipts do not replace the pending batch
and milestone gates.

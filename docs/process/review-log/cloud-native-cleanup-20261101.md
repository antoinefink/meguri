# Native RunCleanup integration

Original session: s-20261025T134357Z-235af9a2 (implementer).
Fixer session: s-20261025T162405Z-9dc656e4.
Fixer base: a808acfcdbaee4dbb2ff43523ec277954dd68aaf.
Seed source base: 6363dda2226652919d5705cbdf87fd7072c888a5.
Original U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Original batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Specs: sync/policy P4, P10.3; sync/cloudkit-recovery V26.1/V26.2; Q11.

Source: cleanroom/20261101T083531Z-90780, 22 files.
Coverage: RunCleanup, FetchAllZones, CloudCleanupExecutor, and the Service wire.

This fix removes the unreachable default from the exhaustive Swift action switch.
The CLI refusal test now checks Purged and EncryptedDataReset; its other refusal,
framing, size, privacy and mutation checks remain enabled. The stdio inventory
case now covers normal Deleted, another account, zone creation after deletion,
and durable request ids, caller/own-operation facts and original outcomes.
Production DeleteZone authorization and Modify semantics are unchanged.
The reported comment, test name and this log were rewritten as whole blocks.

Root-reported results before this fix:
- core-format 082946: PASS.
- core-test 083020Z-87979: compiled; 30/31 PASS. All 7 new FFI cleanup tests
  passed, including actual Driver qualified-cleanup invocation. The Service
  FetchAllZones and stdio inventory tests passed. The sole failure was the stale
  CLI refusal expectation corrected here.
- swift-test 083217Z-88785: Rust and bindings built; Swift compilation failed
  on the unreachable default at CloudPolicyExecution.swift:298.
- The prior 6363 run passed 25 Swift CloudMedia tests. That result does not prove
  this Swift cleanup path or the full AppModel roundtrip.
- Independent native_bounds_review R1: CLEAR, with no P0-P2 source findings.

One bounded source consistency check covered this delta. No builds or tests ran
in this filtered fixer session. The earlier media-helper result and the now-passed
actual Driver cleanup test are separate evidence. The broader GC harness still
needs actual adapter callbacks; that work belongs to the parallel fixer.

The runner exports the complete seed plus this delta. Root owns integration,
MG checks, binding regeneration and review.
Positive Swift cleanup execution and the full actual AppModel roundtrip after
this build remain required, including the existing card, review, media, reopen
and fresh-adoption assertions and budgets. Privacy, provenance, similarity,
batch/milestone gates, integration checks and final device/manual proof remain
pending. No unit is accepted by this source handoff.

Integration verification on a808 with this source applied:
- core-format/20261101T084019Z-92375: PASS.
- core-test/20261101T084043Z-92832: 31/31 PASS, including seven native FFI
  cleanup tests and the actual scoped stdio inventory/deletion checks.
- swift-test/20261101T084047Z-93102: 11/11 PASS with zero build warnings.
  Four cleanup cases exercise the real Core pass, original terminal callback,
  cancellation, close and queue rejection; seven cover instrumentation names.
- Independent native_bounds_review R2: no P0-P2 findings. The CLI refusal
  checks retain their guard coverage and positively exercise supported deletion.
- similarity/20261101T084122Z-94992: PASS.
- private/20261101T084119Z-94670: source files passed. The log passed after
  rewrite in private/20261101T084218Z-97257.
- core-lint/20261101T084157Z-96799: FAIL, collapsible_if in the existing
  media_driver.rs upload branch. Formatting, layering and the service guard pass.
- swift-test/20261101T084139Z-95447: 1/2 PASS. The actual own-inventory case
  passes. The full review/media case reports generationChanged and then
  staleCommand for recordQuietPoint; its assertions remain enabled. Diagnosis
  is in progress. No full workflow, unit or release acceptance is claimed.

Bounded rejection-diagnostic handoff, fixer s-20261025T162405Z-9dc656e4:
- Canonical clean base: bb0a24f4. Original U113 and batch bases above remain.
- The full test's earlier generationChanged diagnostic is lifetime evidence;
  it does not prove the later recordQuietPoint rejection's cause.
- Harness diagnostics now retain a per-backend sequence across the existing
  16-entry buffer. Successful adoption/reopen records a sequence baseline;
  exchange failure text uses only later retained entries on its original backend.
  Exact event text, original command errors and exchange assertions remain.
- Driver test-hooks stderr marks actual rejection under the existing lock:
  validate.policy-guard, validate.original-owner, validate.backend-guard,
  validate.automatic-setting, validate.generation-binding, or
  execute.quiet-preconditions (all prefixed cloud-policy-rejection stage=).
  Output is fixed tags plus equality/precondition booleans and queue counts;
  no command, identity, timestamp, binding or payload values are emitted.
- Error propagation, quiet qualification, scheduling and workflow behavior are
  unchanged. Overlapping command batches remain an unproven candidate.
- One bounded source consistency check covers this delta. No builds, tests,
  Git, history or network operations ran in this filtered fixer session. Root
  owns the next MG full Swift test, focused Rust checks and authoritative gates.
  Runtime rejection cause and full workflow acceptance remain unproven.

Diagnostic integration results:
- native_bounds_review: no P0-P2 source findings.
- private/20261101T084942Z-6957: PASS for these three files.
- similarity/20261101T084934Z-6590: PASS for the pending source.
- swift-test/20261101T084816Z-1955: compiled with zero warnings; FAIL at
  the existing original-media-command timeout. No rejection tag appeared.
  This different failure does not prove that the earlier quiet-point failure
  is repaired. Both remain open; no retry or budget change was added.

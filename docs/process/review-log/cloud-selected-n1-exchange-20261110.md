# Selected note ACK and apply — WIP

Starting commit: `1b2651b5`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer session: `s-20261109T210639Z-47d06d5b`.
Spec: conformance/cloudkit-agent J3.2.

The filtered export adds an observation to the original-role handle, an ACK
join for the retained edit, and peer readback using apply/WAL receipts. The
selected runtime uses its existing single manual exchange. Smoke review checks
and the Missing close/recovery paths stay in place. No production policy or
storage schema changes are included.

The author ran one source check and no builds or tests. Root applied the patch
with the parallel startup work kept separate.

Executed checks:

- `core-test/20261110T010814Z-13342`: eight original-role tests passed.
- `swift-test/20261110T010905Z-13645`: seven registry tests and one real bridge
  test passed. The bridge case checks edits and unbound/successor refusals.
- `build-app/20261110T011108Z-14943`: Mac CloudAgent compilation passed.
- `similarity/20261110T011148Z-16133`: passed.

Required R1 fixes:

- Recheck file, account, publication and Runtime authority after the tracking
  read, before returning positive observation facts. Provider reads must remain
  outside tracking locks.
- SDK callbacks can advance the ledger on another actor after the sampled
  attempt boundary but before the manual request. That sample cannot prove an
  ACK happened after the request. Keep the original ACK chain and report only
  timing established by the recorded events.
- Add real Service positives for bound ACK and settled peer apply, plus the
  required interruption and wrong-owner cases. The existing ledger and unbound
  tests do not cover successful cloud composition. Root will use a separate
  AppModelTests file with current Service support; no user step is pending.

This code remains unaccepted. The executed tests do not prove an ACK or peer
apply; selected close, restart recovery and signed round trip are incomplete.
The held AgentGeneratedBase patch remains outside this commit.

Rust formatting and `core-lint/20261110T011355Z-17332` passed. Tracked source
paths passed privacy scan `16366`; the rewritten log passed `17059`. The new
bridge test file is scanned separately before commit. Required R1 findings
remain open despite these focused checks.

R1 fixer source repair (clean base `b36a5559`; original batch base unchanged):

- The original note reader now captures the actual binding, open evidence and
  publication decision at its tracking read. After that read it rereads account,
  decision and native file outside State/tracking ownership, then rechecks the
  same binding, stored file/open counter, generation, incarnation, invalidation
  and current collection before returning facts. It does not compare global
  scan/feed/ACK bookkeeping. Initial and final provider errors propagate.
  FFI's original-Host and retained-source checks are preserved.
- A post-sample ACK is labelled
  `n1-original-ack-after-attempt-boundary-timing-unresolved`. Earlier automatic
  ACKs retain their own original batch/revision chain and earlier attribution.
  The synchronous prepare/request closures and one-exchange fence are unchanged.
- Added `CloudSelectedOriginalExchangeTests.swift`, reusing the existing real
  Service fixture and shared source/publication helpers. The existing bootstrap
  tests, Native assertions, startup case, 10-second wait and diagnostics remain.
  The two positive variants require actual Saved/envelope/system-field joins,
  a held pre-ACK refusal, one manual exchange, and peer ordinary fetched/apply
  after a pre-edit adoption baseline. Peer proof must include the actual settled
  Incoming/WAL producer and exact unmasked full canonical note digest. The
  independent wrong-Host capability still refuses after peer bytes match.
- Actual Service quota failure and a later ordinary rewrite while Saved is held
  exercise ACK refusals without replacing SDK replies or Core results. Focused
  Rust cases hold actual Incoming COMMIT and flush stages, inspect real intents,
  inbox and WAL receipts, then require settled proof and reject attributing the
  old apply to a later local rewrite. A separate outside-lock read hook exercises
  file/account/account-error/publication-decision/publication-binding/unavailable/Runtime-guard drift without
  changing Core generation or seq. Unrelated pending delivery/automatic settings
  may advance without invalidating the read.

Authored test names (execution pending with root):

- `selectedOriginalRevisionACKAndSettledPeerApplyUseRealService` (manual and
  earlier automatic ACK variants).
- `selectedOriginalHeldFailedAndMismatchedACKRefuseRealServiceProof` (quota and
  later-rewrite variants).
- `selectedAckBetweenAttemptSampleAndRequestHasUnresolvedManualTiming` (controlled
  concurrent SDK callback during synchronous prepare, before actual request).
- `original_observation_rechecks_file_account_publication_and_runtime_guard_after_actual_read`.
- `incoming_read_uses_durable_receipt`.

The fixer performed one source consistency check only; no builds, tests, Git,
network, raw-log or forbidden/private-source access. Root owns compilation,
execution, independent review and provenance/privacy/similarity receipts. These
new tests are authored evidence requirements, not passing receipts. Parallel
ServiceSDK wake/cancellation and publication-token policy repairs are untouched.
Selected close/recovery/restore remain Missing; J5 and final acceptance remain
unfinished. No Unit/full-gate, signed/real-account or release claim is made.

Source consistency check completed across the eight authorized files. Its
checker-only assertion initially counted the add-result ending as an updater
ending; the corrected remaining assertion requires the two actual updater
calls. Whitespace, shared helper visibility, five retained bootstrap tests,
timing labels, authority recheck dependencies, real stage names and positive
receipt/canonical joins were checked. No compiler or test runner was invoked.

## R2 execution

`core-test/20261110T014541Z-33047` compiled both new cases. Both failed in
common setup at reconcile_binding with Tracking(InvalidData), before their
observation checks. The active binding has only a note baseline; full reconcile
also requires its original structural images. The fixture needs repair.
The new binary now reserves the test pool for its bounded commit/flush checks.
The new Service tests also need explicit 32-record pages. No passing ACK/apply
or signed exchange is claimed.

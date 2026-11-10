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

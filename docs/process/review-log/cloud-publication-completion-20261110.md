# Publication completion — WIP

Start: `f1a0d33a`. Batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Session: `s-20261103T133750Z-f276f58e`.
Contracts: sync/cloudkit-recovery V15, V26.1; sync/policy P4, P6;
engine/jobs J2.1.1; conformance/cloudkit-agent J3.2.

The prior goal turn made progress: committed ordering and canonical-root repairs,
with four passing ordering tests and the real Host alias regression. Native
publication completion remained failed. No unit or signed sync was accepted.

New fixed diagnostics retain the original test assertions and expose no content,
paths, account values, identifiers or error payloads.

- `swift-test/20261110T004104Z-3825` failed the one matching-conflict case:
  operation in progress; unavailable backend replacement diagnostic.
- `swift-test/20261110T004352Z-5296` scoped that result to this startup:
  before start, operationFailed false and no diagnostic; after start, true and
  backendReplacement/bridge.unavailable, though the backend then attached.
  Publication's last work result is sendEntities at nextPublicationWork.
- The engine emits Ready while the FFI owner is still Opening. A forwarded
  lifecycle can therefore attach sync before HostState::Open exists. The
  completeOpenedHost method already runs after the open job returns; initial
  attachment belongs there. Later collection changes must still replace sync.

The completion contract is distinct: confirmation accepts a durable operation.
At zones the normally admitted engine must send the entities and media. Actual
ACKs allow sent, Complete, then the Active switch/done commit. Input barriers and
empty task maps do not establish completion. Tests must await the original
terminal commit, then retain every current ownership/control assertion.

Next: repair initial attachment ordering, add its deterministic regression, and
use original buffered commit observations for the five native cases. No retry
loop, new timeout, cleared failure state or weaker assertion is authorized.
The full native workflow, selected exchange ACK/peer/close/recovery and signed
Mac/iPhone round trip remain incomplete. No unit, gate or release acceptance.

Fixer source pass, clean base `1b2651b5`; original pending batch base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`. Same recorded fixer role; own source
only. Earlier failed evidence, including native9 `58704` (4 pass, 5 fail,
8 issues), matching `60861`, and scoped traces `3825`/`5296`, remains valid
failed evidence, not superseded by this unverified repair.

- SyncCoordinator now records retained-host admission only in
  completeOpenedHost. Early Ready/replacement inputs still track actual
  generations but cannot construct the first backend. Successful drain resets
  admission for reopen. Real attachment errors remain sticky; admitted
  replacement, stale-generation rejection and same-generation retry remain.
- The regression delivers Ready while the open result is held, then checks
  that backend creation has not run. Returning the retained Host admits one
  backend. It also checks the next generation and a later open after close.
- All five selected native cases install buffered own-reply observations before
  backend construction. Successful publishers wait for original doneActive
  committed delivery; the invalid Complete-conflict case waits for that
  original conflict's Core result instead. Existing barriers then join the
  original work. Failure/refusal/drain ends observation through the existing
  missing-reply path; task cancellation and the existing ten-second bound are
  preserved. Exact final inspection and requireOriginalCommit remain
  authoritative, including in the common helper and held-Status case.
- Required first-engine scan inspection, P4 machinery, real Host alias repair,
  content-free diagnostics and the held 99-line AgentGeneratedBase patch are
  unchanged. No selected exchange ACK/peer/close/recovery or signing work.

Verification: one source consistency check only in the filtered runner. Native
compilation, same9 focused tests, initial/retained/adoption lifecycle checks,
independent review and provenance/privacy/similarity checks are unrun here and
belong to root. This source handoff makes no Unit/full-gate/signed-sync/release
claim and does not prove the absence of further publication defects.

First repair verification:

- Independent source review is clear for the retained-open admission and
  original terminal-event wait. The former fails the old early-Ready behavior.
- `swift-test/20261110T005331Z-8387` compiled and ran ten cases: five passed,
  five failed. Ready-before-open-return and all four prior ordering tests pass.
  Actual startup now reports operationFailed false before and after opening.
  Each Native publication case still times out at the original ten-second bound.
- The timeout observer reads existing snapshots and prints numbers and status
  labels. It rethrows the same error after those reads.
  `swift-test/20261110T005822Z-10036` ran the matching case and failed: recovery
  at zones; 40 rows, four marked/acked-behind, zero in flight or inbox; four
  sendable, no held rows and no media backlog; scan proof true, no diagnostic.
- Independent source diagnosis finds the next gap in ServiceSDK: it ignores
  configuration.automaticallySync and sends at most one batch per zone. P6
  enables scheduling at zones despite the device setting, and relies on the
  SDK to send subsequent eligible revisions. The production native SDK already
  receives that setting. Do not change production send policy to mask this
  missing fixture behavior.

Next: model scheduling in ServiceSDK with actual Core batches and Service ACKs,
retaining callback bookkeeping, cancellation and original task lifetimes. Keep
all existing bounds, assertions and failed receipts. No unit or sync acceptance.

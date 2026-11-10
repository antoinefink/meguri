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

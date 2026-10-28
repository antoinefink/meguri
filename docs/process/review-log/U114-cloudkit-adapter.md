# U114 CloudKit adapter: tests first

Status: incomplete; no Unit trailer or P24 acceptance.
Starting commit: `218ab2e2543a2c4822b79c1ac3520230ca3c1998`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Enabled Swift contracts precede production adapter work. The fake supplies SDK
events and pending-list behavior; persisted facts and review results require
the real CoreBridge and Rust core. Fixture preparation is test-only.

Filtered author: `cleanroom/20261028T105325Z-31354`.
R1 found P1 failure propagation in a rendezvous and P2 clock origin drift.
Repair `cleanroom/20261028T111324Z-50463` is independently clear. It also adds
failure/success-before-arrival regression coverage and checks cancellation
order. Clock and answer timestamps use the required 2025 origin.

Compiler and privacy deltas:

- `cleanroom/20261028T160551Z-93357`: consistent test-target bindings import
  and comment edits; independent equivalence review clear.
- `cleanroom/20261028T161049Z-97354`: removes two redundant paragraphs;
  executable source unchanged. P2 spec mapping noted.
- `cleanroom/20261028T161614Z-2928`: throwing task-group syntax and restored
  `P21; V23.2` mapping; independent review clear. Assertions remain enabled.

Observed compile results:

- `swift-test/20261028T160108Z-89205`: import mismatch, not useful RED.
- `swift-test/20261028T161309Z-99614`: missing APIs plus task-group error.
- `swift-test/20261028T161747Z-3936`: expected RED, only missing production
  `CoreCloudWorkflowService`, `CloudKitSyncAdapter`, `CloudRecord`,
  `CloudEngineEvent` and generated fixture hook. Zero tests execute; no pass.
- `private/20261028T161752Z-5093`: final three files pass.
- `similarity/20261028T161750Z-4753`: final three files pass.

Next: implement typed guarded FFI and the native SDK adapter against these
contracts. Do not replace the real persistence assertions with fake outcomes.
Publication/adoption, incoming reconciliation, media, remaining P24 tests,
signed-device proof and integration gates remain required. The existing Rust
stable-binding workflow is partial evidence, not complete iCloud sync.

## Native adapter WIP checkpoint

Production phase starts at `e2ed929e43651281516fd92daa4f94bb7fcc06a3`, after
the enabled tests and observed missing-API failure. Filtered producer
`cleanroom/20261028T162353Z-7865` adds typed FFI, lane-backed service,
SDK ownership, native wrapper and durable event tracking over the Rust Runtime.

R3 found five P1s: deleted-root confirmation, zone encoding, incomplete snapshot
comparison, exchange termination and stale identity reads. Filtered repair
`cleanroom/20261028T165416Z-18859` addresses them with additive regressions.
R4 cleared four repairs and found two identity races: stop during startup and
failed later reads. It also noted atomic callback-origin validation.
`cleanroom/20261028T172007Z-30160` repairs these; R5 is clear of P0-P1.

R6 checks the native SDK correction in
`cleanroom/20261028T173800Z-38230`: routing uses each record's full zone ID,
with a single SDK callback boundary. R6 is clear of P0-P1. The four local SDK
tests are additive. Compiler-only corrections `43653` and `47768` qualify DTO
names and hoist two expressions; independent equivalence checks are clear.
All assertions and limits remain enabled.

Verification:

- `core-lint/20261028T173412Z-34973`: all four checks pass. Later deltas change
  Swift/test source only; Rust input is unchanged.
- `core-test/20261028T171602Z-28376`: base32 literal-vector test passes.
- `swift-test/20261028T175820Z-49152`: 32 of 33 tests pass. The remaining
  hold-back case checks its mark after acceptance, before asynchronous row
  application. Independent diagnosis confirms the existing production flush
  provides the missing barrier while keeping the session active. Preserve
  the failed receipt; add that barrier before repeating focused verification.
- `swift-test/20261028T180137Z-51836`: all 11 routing/registry tests pass.
- Final source/privacy coverage combines the complete SDK export and the
  subsequent fixture and assertion-file checks; their scoped receipts remain
  in MG. The pre-commit privacy hook also checks this checkpoint.

P2-REG: registration rollback (R5). P2-OWNER: private-zone ownership (R6).
Fault and signed checks remain open.

This is WIP, without a Unit trailer. Publication/adoption, incoming application,
absence processing, K19, assets/media, full result classification, large-data
budgets, signed integration and full gates remain required. Tests use generated
collections; no live cloud workflow or complete product acceptance is claimed.

### Hold-back verification precondition

`cleanroom/20261028T180526Z-53999` adds the existing production flush after
answer acceptance. Independent diagnosis and exact-delta review are clear.
The test keeps its active session and every assertion.
`swift-test/20261028T180831Z-55452`: the failed case now passes.
`private/20261028T180838Z-56351`: pass. The corresponding scoped similarity
receipt is retained in MG. The full focused selection follows on clean master.

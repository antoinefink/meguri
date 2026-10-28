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

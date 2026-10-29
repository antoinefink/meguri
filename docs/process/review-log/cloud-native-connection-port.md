# Native connection and own-operation source handoff

Implementer starting base: `40dc9650`. Original integration base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Scope: typed retained Rust connection/confirmation/work/request handles,
MaintenanceLane CoreBridge facade, independent native CKDatabase own-operation
executor. No AppModel owner, UI, entitlements, generated bindings, frozen server
work, media/reset/structural implementation or schema changes.

Authority: sync/cloudkit-records K2–K5, K12.6, K18; cloudkit-recovery V13/V14/V15/V26;
sync/policy account/gate/event ordering and applicable P24 items; conformance/
cloudkit-acceptance Q11. Existing publication/adoption proofs remain required.
No P24 item is closed, and this handoff is not U113/U114 or app acceptance.

Production changes retain actual Preview/PublicationWork/OwnRequest/AdoptionWork.
Swift receives counts with manifest dates, transport inputs and operation
metadata. Account/file/decision/key authority remains in Rust. Every own enqueue
calls Runtime::submit_publication_request and synchronously acknowledges the
already built CKDatabaseOperation. Original replies drain after Rust returns.
Zone creation is one real operation for all three zones; record saves are
conditional; native record identity and opaque system fields are validated.
Adoption records/pages/deletions/tokens return to CoreHost. Unsupported assets
fail instead of becoming an empty or completed page.

Minimal admission fixes: exact preview cancellation id; retained-host active
check for connection/adoption calls; exact Runtime incarnation/instance/generation
checks on own replies; unbound account observations and persistent accepted
identity cache (stale completions cannot replace it). No lock crosses queue add.

Tests/support to extract before production where applicable:

- Rust tests: `core/crates/meguri-ffi/src/cloud_connection_tests.rs`, included as
  a child of existing `sync_tests.rs` to reuse its actual Host fixture. The
  cfg(test)-only `retained_for_test` accessor in cloud_connection.rs lets the
  existing real Service/coordinator helper drive publication prefixes. The
  existing dev dependency is unchanged; production gains no testkit dependency.
- Swift support: `CloudConnectionTestSupport.swift` reuses the existing bootstrap
  and serialized EventTests; `NativeOwnCapturingQueue` in the native test file
  captures actual CKDatabaseOperation objects without a live account.
- Swift tests: `CloudConnectionTests.swift`, `NativeCloudOwnTransportTests.swift`.
  Includes real Host preview/confirmation, stale/wrong-owner/close guards,
  zero/one guarded queue admission, original-reply cancellation drain, root-read
  classifications, conditional identity checks, three-zone operation, and
  refusing incomplete/asset pages. The 37-code transport classification test
  covers only that boundary; it does not satisfy the full P24 reset scenario.
- Instrumentation exact registry expectation updated for all 16 new exports.
- Existing Host adoption guards and actual publication/adoption/durability tests
  remain enabled and unchanged; the deleted-card failure is not addressed.

Review/fix log: source implementation only. The single source consistency pass
checked the 16 export/registry/lane mappings, the nine new source/support/test
files, error mappings and scoped Host/Runtime forwarding; no source mismatch
was reported. Added 11 Rust cases and 16 Swift test declarations (parameterized
code cases expand at execution). Native compilation, binding generation, focused MG execution,
independent review and provenance/privacy/similarity gates belong to integration.
No build, test, formatter, history, network, credential access or commit was run.

Integration must verify Swift/SDK signatures, generated UniFFI names, Rust/FFI
and exact instrumentation tests through MG, retaining all earlier failures.
AppModel lifecycle/controls, rendered evidence, signed-account acceptance and
broader plan/milestone gates remain subsequent work. Use the same private
database via NativeCloudKitEngineFactory.makeOwnExecutor; no second SyncEngine
or pending database operations are introduced.

## Integrated source checkpoint

Phase starts at `40dc9650`; original batch base remains unchanged.

- `core-format/20261029T184813Z-76784`: pass.
- `core-test/20261029T184949Z-77988`: all 106 FFI library tests pass.
- `private/20261029T184954Z-78498`: all 22 paths pass.
- `similarity/20261029T184952Z-78255`: source changes pass.
- `swift-test/20261029T185110Z-79335`: compile fails at the nested switch
  expression in CloudConnectionModels line 150. No Swift tests ran.

Independent R1 requires a P1 identity-cache repair: a rejected retired SDK's
account event must not clear current accepted identity. Its P2 enqueue repair
must distinguish a queue that accepted nothing from a genuinely uncertain
earlier submission. Both require real regressions before acceptance.

This is compiled Rust WIP. Native Swift execution and the review repairs are
pending. P24, app connection, media and the original gate remain open.

## R1 source repair export (2026-10-29)

Fixer starting base: `0677c9af`. Original unit/phase base `40dc9650` and
original integration/batch base `1416a520542a8c5daa38a23bfe398da704c3d6c9`
remain unchanged. Earlier 106-test Rust pass, privacy/similarity passes and
Swift compile failure above remain the integration evidence; none is replaced
by this source export.

Repairs:

- Bind the local-kind switch to a typed local before constructing Swift's
  publication-work case; no switch expression remains in argument position.
- Update the accepted identity cache only after adapter_account succeeds.
  Regression installs that same cache in the actual CoreHost, accepts the
  current owner, rejects retired SDK SignedOut/ReadFailed events, and exercises
  current-owner preview, confirmation and guarded own-operation admission.
- Route native synchronous Boolean queue acknowledgment through a separate
  Runtime entry point, preserving the existing generic Rust transport API.
  Reserve durable evidence before enqueue; on actual false restore the prior
  switch_sent/active_attempt, release the exact running request, and wait for
  a new trigger. Swift sends no fabricated Failed reply on queue refusal.
  Never overwrite a successor if lifecycle invalidation raced acknowledgment;
  that stale case retains conservative recovery evidence. No lock crosses the
  foreign enqueue callback. Real Service publication prefixes feed SaveActive
  refusal and prior-uncertain-attempt controls; the original lost-reply test's
  assertions and 64-step prefix bound remain.
- Add an explicit RetryAfter duration variant through Swift/FFI/OwnResult.
  Existing Failed.retry_after_ms remains an absolute deadline. Native
  publication reads, saves and zone creation extract CKError seconds, round
  upward to milliseconds, and retain the longest reported partial/operation
  delay. Runtime rejects stale replies before reading its actual Host clock,
  rechecks all authority, and persists the absolute deadline after validation.
  No Swift date, timer, retry loop or constructed DTO authority is introduced.

Test source was started before production edits. Four Rust regressions and two
Swift test declarations were added; parameterization covers both documented
CloudKit retry error codes and original per-item, terminal and partial-error
callbacks for publication reads/saves/zones. The retry regression delivers a
120-second duration at the reply clock, proves +5 seconds is Idle, restarts the
actual Host and checks its durable deadline, rejects the stale reply, and admits
the first actual trigger after 120 seconds. Existing tests remain enabled.

Shared-file hunks are limited to sync.rs account/cache handling; sync_tests.rs
fixture identity injection and an explicitly stepped Host clock (unchanged until
stepped); publication.rs's additive duration variant; and publication_runtime.rs
submission acknowledgment and validated duration conversion. No media,
incoming-runtime, general Host-open, policy, UI, entitlements or server module
was edited. No new FFI method export or BridgeCall was needed; existing export
and exact registry counts remain unchanged. Generated bindings must be rebuilt
for the additive result enum variant.

One scoped source consistency pass was made. No build, test, formatter,
network/credentials, forbidden source/history access or commit was performed.
Compilation, actual test results (including all original 106 Rust/16 Swift
cases), MG guards, independent R2 and commits belong to the integrator. Native
SDK signatures and generated enum spelling remain unverified here. Ordinary
preview/adoption read failures still follow their existing failed-read path;
this persisted V26.1 retry repair concerns publication own-operation replies.
No acceptance or P24/app/milestone closure is claimed.

## R2/R3 integration checkpoint (2026-10-29)

Independent R2 closed the identity-cache, rejected-enqueue and persisted retry
findings, requiring only two enum spelling corrections. R3 cleared the narrow
Swift actor-capture repair; all 15 calls retain the same immutable raw handle
before the MaintenanceLane crossing. No new behavior finding in that delta.

- Rust FFI library: core-test/20261029T191345Z-92404, all 110 pass.
- Rust format: core-format/20261029T191334Z-92066, pass.
- Privacy: private/20261029T191825Z-97382, all 12 paths pass after rewriting
  the flagged test name and removing one comment.
- Similarity: similarity/20261029T191828Z-97738, pass.
- Swift: swift-test/20261029T191823Z-96650 builds successfully but fails its
  95 cases (62 pass, 33 fail) with 48 issues: two parameterized native retry cases and
  46 EventTests account-verification issues. This is an unaccepted WIP
  checkpoint. Next: repair the failed cases.

No Unit closure. U113/U114 and the batch gate remain pending.

## Native callback checkpoint

- swift-test/20261029T192751Z-8067: EventTests pass; retry test fails.
- swift-test/20261029T193806Z-16771: callback diagnostics fail.
- swift-test/20261029T194523Z-23237: all 17 selected checks pass.
- private/20261029T194527Z-24039: all five source paths pass.
- similarity/20261029T194529Z-24374: pass.

Each fixture now configures its platform before open. Negative account tests
remain enabled. The retry matrix retains both codes and every 120000-ms check.
Per-item failures and aggregate success now follow the modern SDK contract;
direct nested NSError checks remain separate. Authority and Rust deadlines
are unchanged. Independent review is clear for the final source delta.

The callback correction follows [Apple WWDC21, 6:35](https://developer.apple.com/videos/play/wwdc2021/10086/).
The earlier EventTests evidence covers unchanged cases; the full batch gate
still needs its final integrated run. No Unit closure.

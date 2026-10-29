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

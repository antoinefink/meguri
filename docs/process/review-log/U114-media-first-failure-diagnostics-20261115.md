# U114 media first-failure diagnostics

Starting commit: `4ecd1d453ecb914b4a5e96b2897cc41885d1e3dd`. Original pending batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

This change records fixed media subphase tags at the first throw. Debug sends retain collection/media phase context across the existing detached admission callbacks. Rust test hooks name the original refused predicate and the fixed Incomplete variant. They record no payload, account, address, record name or native identifier. The first-error latch, guard conditions, short circuit, locks, queries, ACKs, return values and cancellation rules stay the same. Healthy operations emit no new output.

The earlier failure remains unexplained: `swift-test/20261115T091429Z-72438` passed 579/580 methods, with `connection.incomplete` from ResumeMedia in `selectedPublicationRebindWaitsForOriginalStatusCoreAcknowledgement`. The original status ACK had joined. That does not prove readiness through later asynchronous media work. The broad mediaWorker label did not identify the failing operation. Ordinary CloudMedia errors and policy connection errors use different mappings; the earlier error cannot be assigned to ordinary media I/O from its category alone.

Filtered author: `cleanroom/20261115T093117Z-87644`. Independent review: `cleanroom/20261115T093830Z-92881`, CLEAR with no P0–P2 findings in the complete seven-file candidate and immediate callers. Root read the whole patch. Two temporary test-trace files were then restored exactly to their starting contents. The five remaining source changes are unchanged from the reviewed candidate. No reference source was read.

Evidence:

- Docker Rust: `core-test/20261115T093843Z-93878`, 45 policy/media/publication tests passed, zero skipped.
- Debug focused capture: `swift-test/20261115T094134Z-96231`, the one publication method passed in 2.047 seconds. The failure did not reproduce.
- Full affected AppModel context: `swift-test/20261115T094606Z-868`, 580/580 methods passed in 354.590 seconds. The publication method passed in 2.165 seconds. No failing media gate was captured. This does not establish a repair for the earlier failure.
- Extra Kit Profile attempt: `swift-test/20261115T095515Z-5187` failed during compilation, before test execution. AppModel tests reference Debug-only `observeCausalBoundary`, including unchanged test files. This failed check is retained; it is not a pass or a waiver. Broader Profile test coverage remains open.
- Core Profile: `swift-test/20261115T100346Z-12635`, both media preparation/cancellation methods passed, zero failed or skipped. This verifies the changed bridge without Swift Debug symbols; it does not repair the separate Kit test compile errors.
- After removing the temporary test tracing: `swift-test/20261115T100558Z-15176`, the original publication test passed in 2.081 seconds. Its assertions, holds, releases and limits match the starting source.
- Privacy: `private/20261115T094134Z-96173`, seven candidate files passed. Similarity: `similarity/20261115T094135Z-96811`, seven files, zero flags or allowlist entries. Both cover the unchanged five source files retained here. Root read only the implementer similarity report.

Rust process logs can include readiness probes from other commands. Log proximity alone is not command correlation. No behavior correction, root-cause claim, Unit trailer, P24 closure, milestone, full-gate pass or release acceptance is claimed. Native activation/replay, real peer evidence, Profile coverage and remaining U113/U114 work stay open. The failed full gate and original batch base are retained.

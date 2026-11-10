# Cloud callback worker compile fix — 2026-11-10

- Role: recorded fixer. Starting commit supplied by root: `8e8e1ec2`.
- Scope: only `selectedAckBetweenAttemptSampleAndRequestHasUnresolvedManualTiming` in `app/MeguriSnapshotTests/TestHooks/CloudKitSmokeObservationTests.swift` and this new log.
- Reported failure: ActualMacSnapshot `mac-hosted-test63952`, line 517, rejects `DispatchSemaphore.wait` inside the asynchronous `Task.detached` body. No raw logs were read.
- Fix: run the original callback interval on a synchronous global GCD worker. An `AsyncStream<Bool>` finishes on every worker exit and is drained asynchronously on both exchange success and error before the test returns or rethrows.
- Preserved: synchronous MainActor prepare and request; both five-second semaphore bounds and prepare refusal; original sent/Saved and later state callback events, ordinals and phases; exact record envelope and pending-removal join; one request; unresolved after-attempt-boundary and earlier-original-ACK assertions. Other cases and production, Runtime and Evidence source are unchanged.
- Consistency check: one source comparison passed. It confirmed unchanged source outside the named test, identical callback payload and prepare/request interval, both five-second waits, preserved ACK assertions and success/error stream joins. This is source inspection only; native compilation, execution and independent review belong to root.
- No builds, tests, git commands or network access performed. No forbidden/private/Anki sources or build artifacts read. No new framework, Unit selection or signed-success claim.

Independent source review: no P0–P2 findings. Source privacy `68602` and
similarity `68844` passed. The named callback case passed in the dedicated VM
via `mac-hosted-test/20261110T063035Z-76738`, one executed case, zero skips.

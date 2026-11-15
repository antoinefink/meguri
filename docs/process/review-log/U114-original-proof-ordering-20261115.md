# U114 original proof error ordering

Starting commit: `805ee6a1`; source candidate prepared at `7b561614`. The intervening commit changes only test configuration and its record. Pending full-gate base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The account-read path now calls `sealFailureBeforeOutputs` before `consume`. It passes the original error to the original native latch. This prevents that read's own CancelExchange and StopEngine from selecting cancellation first. A cancellation already selected still wins. The proof remains pending until outputs return. Existing checks and account-stop rules remain. The adapter also has a nil-default Debug test hook.

Filtered source author: `cleanroom/20261115T055954Z-71656`. Independent source review: `cleanroom/20261115T061931Z-93214`, no required P0–P2 findings in the candidate. Root inspected the complete patch and verified the original provider-outcome test was unchanged. No reference source was read.

Evidence:

- RED: `swift-test/20261115T063637Z-13711` reproduced the unchanged provider-outcome test's three assertions for outcome 3 before the coordinator change. Its other diagnostics do not establish a complete suite pass.
- GREEN: `swift-test/20261115T070703Z-36552`, 2 methods passed, 0 failed or skipped. All five original provider outcomes and both close/late-result controls ran.
- App controls: `swift-test/20261115T070939Z-42405`, PASS; 4 passed, 0 failed, 0 skipped. Original methods and parameter values ran.
- Core controls: `swift-test/20261115T071158Z-45082`, 7 passed, 0 failed or skipped. Covers earlier cancellation, original failure, revocation, unknown-epoch preparation and stale guards.
- Privacy: `private/20261115T070942Z-43565`, 3 source files passed.
- Similarity: `similarity/20261115T070947Z-44419`, 3 source files, zero flags or allowlist entries. Root read only the implementer report.

The additional stress-test candidate is not committed here. Its fixture timed out waiting for an edit's pending-update ACK before reaching the intended race (`swift-test/20261115T070103Z-31081`). Its patch and failed evidence remain under ignored build output for repair; no assertion or limit in the existing tests was changed. A deterministic held-propagation test remains unfinished.

The full gate remains incomplete after `gate/20261115T042009Z-87341` was cancelled with known failures. Whole-Kit lifecycle failures, the native AX hosting problem, the real peer checks and the required full gate remain open. No Unit trailer, milestone, P24 or release acceptance is claimed.

# Native bridge first-failure discriminator — 2026-11-10

- Role: recorded fixer; root owns execution and independent review.
- Clean starting commit supplied for this invocation: `28591ca4`.
- Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Scope: `CloudServiceAppHarness.swift` backend error capture and its retained first-failure field only, plus this log.

`capture` now distinguishes every public `BridgeError` case with fixed tags from
the owned `DebugCloudDiagnostics.failureTag` taxonomy. Policy failures retain
the existing `cloudPolicy` prefix through explicit fixed cases. No associated
strings, identifiers, paths, settings or record contents are formatted.

The backend retains its first observed failure separately from the bounded
ring and emits that snapshot once through the existing trace path, marked
`firstFailure`. Stage, kind, optional action, task cancellation, close phase and
owned-shutdown cancellation context are captured at observation time. Later
failures retain their ordinary capture and cannot replace the first snapshot.
The returned current diagnostic, `Issue.record`, failure state and owned
cancellation mapping are unchanged. No callbacks or bookkeeping were added.

One source consistency check passed: all 51 fixed bridge/policy/connection tags
match the owned taxonomy (retaining the existing policy prefix); no description
formatting or generic bridge flattening remains; the first snapshot has one
assignment and one emission site; current return and observed context remain.
No git, network,
builds, tests, raw logs or forbidden/private/reference sources were accessed.
Native compilation, execution, independent review and integration checks remain
with root; no unit, full-gate or signed acceptance claim is made.

Root's bounded execution: run only
`CloudSelectedOriginalExchangeTests.selectedOriginalRevisionACKAndSettledPeerApplyUseRealService`
once with its existing two `earlierAutomaticACK` argument variants. Do not add
Sync Now calls or retries. Inspect the first fixed discriminator before adding
any further diagnostic facts.

Actual root run `swift-test/20261110T102848Z-32328` failed its two variants.
The manual case reached durable original ACK but lacked its expected exact
savedPendingRemoved event. The automatic case's first send error is now
connection.authorityChanged. These are observations, not a repaired workflow.
Independent review found no required diagnostic-code finding.

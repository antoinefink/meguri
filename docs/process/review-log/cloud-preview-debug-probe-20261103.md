# Temporary preview observation

Status: WIP diagnostic; no unit or sync acceptance.
Start:34941ed98cf8983f76ff40d014d17e19dd849ebe.
Fixer:s-20261103T133750Z-f276f58e.
Batch base:1416a520542a8c5daa38a23bfe398da704c3d6c9.

Phone86370 reaches an idle connection form with no preview. Its generic error
does not retain the original error. Existing telemetry access times out. A
temporary Debug-only probe observes the existing own-preview boundary. It keeps
one attempt and a fixed tag; only known public error families and Int32 SDK codes
can enter that tag. No descriptions, userInfo, identities or SDK objects are
stored. The existing error leaf exposes the tag to the normal UI capture.

Export92927 preserves guards, awaits, cancellation, slot cleanup and rethrow.
Review asks for explicit SDK value handling. Export93807 adds exact known SDK
types and tests while rejecting arbitrary Swift errors. Review is clear.
The expected RED does not reproduce: SDK checks94443 pass on the first classifier;
all nine checks95786 pass there too. Keep that evidence. The final explicit
handling is not a demonstrated host bug fix. Its cross-platform behavior still
requires the actual phone observation.

Private96841 and Similarity97052 pass. Swift97467 passes all nine probe checks
and the existing cancellation/drain/reuse regression, with no skips. The two
fallback leaf identifiers also address the real phone's unidentified rows and
retry action.

Phone98749 on a481b4af passes all six steps with zero hierarchy issues and no
skips. Its final preview tag is `finished`, and the initial-adoption row is
present. Preview succeeded; adoption was admitted. The row is not evidence that
the download completed or is still advancing: its marker can survive a retained
failure. The historical generic error is not current-preview failure evidence.
The rendered progress label exposes a missing localization entry; Export2191
adds the exact approved T12 copy and a localized-value check. Independent review
is clear. This copy change has no operation or layout changes.

Private3181 and Similarity4199 pass the two copy/test files. Swift4531 passes
all eight CloudSyncPresentationTests, with no skips. The first sandboxed
Similarity3480 and Swift3479 requests could not reach Docker; they ran no checks.
The host-access runs retain those failed setup receipts. The signed phone render
of the corrected label is still pending.

The later inspection1629 is queued because the selected phone is unavailable.
It runs no test and provides no new adoption outcome. Keep that distinction.

Native Resume/registration ownership and the U114 repair cap are unchanged.
Remove the temporary observation after diagnosis, or review any permanent
diagnostic separately. No underlying sync repair is inferred from this probe.

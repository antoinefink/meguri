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

Host-access inspection10397 then passes two steps with no hierarchy issues. The
phone is reachable and still has no collection after normal relaunch. Do not
treat the sandboxed1629 preflight as proof that user setup is missing.
Phone12461 on57c4808a renders the corrected progress label. Six steps pass, then
the required disappearance of the adoption row times out after30 seconds. It has
zero hierarchy issues and no skips, but its receipt fails. The preview tag is
again `finished`. The collection download and round trip remain unproven; the
spinner alone cannot distinguish active work from a retained interruption.

From6e5ceec8, Export15382 adds one bounded own-work instance to the same temporary
probe. It records only fixed progress cases or the existing safe error category.
The existing runNext boundary preserves its admission, task, cancellation, result
and original error. Native Observation updates the Debug-only value on the
existing progress leaf even when the coordinator's old failure flag is unchanged.
Observation callbacks run outside the probe mutex; a synchronous re-read test
checks that boundary. No native ownership or coordinator code changes.

Independent review is clear. Swift16706 passes all14 focused cases, including
the existing cancellation/drain/reuse check. Private16097 flags four regions in
one file. Export18040 changes only four private property references; tests and
the view are byte-identical. Mechanical review is clear. Private18647 and
Similarity18904 pass the final three files. Swift19238 passes all14 cases on the
final source, with no skips. Signed phone observation remains pending; no work
outcome is inferred from source review.

Phone20467 on22e813db passes six steps with zero hierarchy issues. Its final
value is `preview:finished;work:running`. Phone22826 retains the same value in
the actual after-wait capture at the thirty-second deadline. Its receipt fails
the disappearance condition; zero hierarchy issues do not make it a sync pass.
Both workers are still inside runNext at their captured points. Each drive's
normal teardown terminates the app afterward. These captures establish neither
a terminal error nor completed adoption. The driver already captures failed
waits; no additional capture mechanism is needed.

Export24810 adds fixed, Debug-only last-entered await tags to that same probe.
It retains the original attempt UUID after idle admission and clears it by a
matching defer. No guard, task, await, cancellation, error, activation ordering
or Release behavior changes. Independent review is clear. Private26865 and
Similarity27114 pass both files. Swift27385 passes all16 focused cases without
skips. These tags identify an entered boundary; by themselves they do not prove
that its work is still blocked there. No further probe extension is planned.

Native Resume/registration ownership and the U114 repair cap are unchanged.
Remove the temporary observation after diagnosis, or review any permanent
diagnostic separately. No underlying sync repair is inferred from this probe.

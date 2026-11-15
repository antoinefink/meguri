# Receipt tests and review deadlines

Starting commit: `38235917e6a025daa57625c582e90d253aae62e3`. Pending full-gate base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The receipt suite ran file and JSON checks on MainActor. Its binding, private file authority and serialized suite now use explicit nonisolated declarations. Each caller owns its non-Sendable authority. Runtime callers stay on MainActor. The shared header remains the same immutable Data. App code, tests and their limits are unchanged.

Filtered author: `cleanroom/20261115T090737Z-60555`; header correction: `cleanroom/20261115T091109Z-67043`. Independent two-file review: `cleanroom/20261115T090958Z-64002`, CLEAR. Final header-delta review: `cleanroom/20261115T091323Z-69159`, CLEAR. Both found no P0–P2 findings. Root read the full patch. No reference source was read.

Attribution and verification:

- Whole Kit before this correction: `swift-test/20261115T085504Z-42132`, FAIL, 955/960 methods passed. Temporary fixed-label tracing retained the first boundary before cleanup. The original provider-outcome method passed all five cases. Four review methods failed their deadline; native replay failed separately. Root removed all temporary tracing afterwards.
- Review root only: `swift-test/20261115T090444Z-52987`, 52 methods passed in 0.217 seconds.
- Review plus receipt root: `swift-test/20261115T090504Z-54632`, 68 methods passed in 1.515 seconds.
- Review plus the Agent-prefix roots: `swift-test/20261115T090535Z-56728`, 155 methods passed in 2.966 seconds.
- Review plus all agent roots: `swift-test/20261115T090624Z-58342`, 192 methods failed in 3.966 seconds. The 68-method comparison above passed; the wider selection reproduced the deadlines.
- Committed-source RED, without tracing: `swift-test/20261115T090834Z-61664`, the same 192-method selection failed in 4.051 seconds with 23 issues.
- Initial compile: `swift-test/20261115T091013Z-64563` failed on the shared header's actor annotation. The final candidate fixes that one declaration; the failed run is retained.
- GREEN: `swift-test/20261115T091343Z-70111`, the unchanged 192-method selection passed in 2.731 seconds. Original review deadlines remain.
- Complete AppModel context: `swift-test/20261115T091429Z-72438`, FAIL, 579/580 methods passed. All four review deadline methods and the new Resume regression passed. The remaining method, `selectedPublicationRebindWaitsForOriginalStatusCoreAcknowledgement`, reported `connection.incomplete` from `resumeMedia`. That media failure remains open; its cause is not established here.
- Shared Mac Snapshot support compiled in `mac-hosted-test/20261115T092040Z-77257`. Its selected `productionReviewHostStartupLifecycle` method failed at native host activation after 3.133 seconds, before completing its six scenarios. This is compile evidence, not a native workflow pass.
- Privacy: `private/20261115T091348Z-71453`, three files passed. Similarity: `similarity/20261115T091352Z-71939`, three files, zero flags or allowlist entries. Root read only the implementer report.

The media handoff failure, native activation and replay checks, real peer evidence and the required full gate remain open. No Unit trailer, P24 closure, milestone or release acceptance is claimed. The original batch base is retained.

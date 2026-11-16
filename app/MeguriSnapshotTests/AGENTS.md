# MeguriSnapshotTests

- `NativeReplayAccessibilityTests.native_generated_replay_controls_are_named_and_identified_in_webkit_hierarchy` is mandatory Mac public AX replay proof in the app-hosted Snapshot VM lane. Its synthetic runtime fixture guards VM creation/publication, uses a real styled window, and closes/detaches pages and handlers on failure. AppKit launch belongs to the app host; no bootstrap, host fallback, activation, key window or VoiceOver.
- Normal Mac snapshot and package fixtures remain offscreen. `mac-unit-only` excludes Snapshot and cannot verify this proof; the existing `mac-hosted-test` exact selector can run the method above.
- Replay proof, both AppVisibility tests and each NativeReviewStartup `exerciseStartup` share `SnapshotAppLifecycle.shared` across suspension and all deferred restoration. Nested owned tasks do not reacquire it; unrelated fixtures remain independent.

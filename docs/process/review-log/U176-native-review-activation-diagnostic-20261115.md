# Native review host: activation diagnostic

Diagnostic start: `20cddcab4b269f06b3cae68219755406f8398bb3`. Run on `29f251f2926b533a6aa98d32415361bec62caafc` plus the test-only diagnostic. The original pending full-gate base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Author `cleanroom/20261115T141332Z-53532` adds a bounded recorder to `NativeReviewStartupTests.swift`. It records fixed labels, booleans and monotonic elapsed times. AppKit reads stay on MainActor; notification callbacks use locked storage. It captures state before failure cleanup and reports after owned task joins. Activation, assertions, three-second deadline, cancellation, observer lifetimes and production code are unchanged.

Independent review `cleanroom/20261115T141943Z-61762`: no P0–P2 findings. The integrator read the full patch. `cleanroom/20261115T142942Z-78692` rewords only one comment rejected by the privacy scan; all other bytes match the reviewed and tested export. The guard is unchanged.

`mac-hosted-test/20261115T142636Z-73174` built Debug Snapshot with zero errors and warnings, then executed the exact method in the dedicated VM. It FAILED at the same activation deadline before the six review scenarios ran. The trace is complete and untruncated:

- Policy changes from accessory to regular; the request returns true.
- The app is not hidden. Its own window becomes visible and unoccluded, with a screen present.
- The app remains inactive and the window is not key. No didBecomeActive notification arrives.
- Failure unblocking and the cancelled waiter are recorded before the owned window is removed.

This rules out a rejected policy change or hidden window in this run. It does not establish why activation was not granted, or prove a production review defect. The installed Apple SDK's NSApplication header explicitly does not guarantee that `activate()` will activate an app. No repeated unchanged test was started. That diagnostic run remains failed. The subsequent repair is recorded below; no acceptance claim or Unit trailer.


## Foreground request repair

Author `cleanroom/20261115T144313Z-93065` changes only the test host's request to `NSApp.activate(ignoringOtherApps: true)`. The installed SDK still documents that NSApplication entry point; it differs from the deprecated NSRunningApplication option. The fixture requires actual foreground activation of its own window. `mac-hosted-test` disables host fallback, so execution stays in the dedicated VM. Production behavior is unchanged.

Independent review `cleanroom/20261115T144421Z-94336`: no P0–P2 findings. The integrator read the exact one-line patch. The three-second deadlines, active predicate, assertions, six startup scenarios, observers, cancellation and task joins are unchanged.

- `mac-hosted-test/20261115T144436Z-94815`: PASS, exactly one executed method, no skips or failures; all six startup scenarios complete. Debug Snapshot builds with zero errors and warnings.
- `mac-hosted-test/20261115T144624Z-98395`: PASS, exactly one executed method, no skips or failures; accepted-answer preparation does not block Pause or Close.
- `similarity/20261115T144629Z-99606`: PASS, one file, zero flags; filtered implementer view read. The final source privacy scan also passes.

This establishes the native test-host correction and these two methods. It does not prove an Apple CloudKit peer exchange, physical-device behavior, visual approval or full-unit acceptance. The existing tests and this review record retain the lesson; no duplicate learning document is needed.

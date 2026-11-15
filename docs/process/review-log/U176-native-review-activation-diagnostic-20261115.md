# Native review host: activation diagnostic

Diagnostic start: `20cddcab4b269f06b3cae68219755406f8398bb3`. Run on `29f251f2926b533a6aa98d32415361bec62caafc` plus the test-only diagnostic. The original pending full-gate base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Author `cleanroom/20261115T141332Z-53532` adds a bounded recorder to `NativeReviewStartupTests.swift`. It records fixed labels, booleans and monotonic elapsed times. AppKit reads stay on MainActor; notification callbacks use locked storage. It captures state before failure cleanup and reports after owned task joins. Activation, assertions, three-second deadline, cancellation, observer lifetimes and production code are unchanged.

Independent review `cleanroom/20261115T141943Z-61762`: no P0–P2 findings. The integrator read the full patch. `cleanroom/20261115T142942Z-78692` rewords only one comment rejected by the privacy scan; all other bytes match the reviewed and tested export. The guard is unchanged.

`mac-hosted-test/20261115T142636Z-73174` built Debug Snapshot with zero errors and warnings, then executed the exact method in the dedicated VM. It FAILED at the same activation deadline before the six review scenarios ran. The trace is complete and untruncated:

- Policy changes from accessory to regular; the request returns true.
- The app is not hidden. Its own window becomes visible and unoccluded, with a screen present.
- The app remains inactive and the window is not key. No didBecomeActive notification arrives.
- Failure unblocking and the cancelled waiter are recorded before the owned window is removed.

This rules out a rejected policy change or hidden window in this run. It does not establish why activation was not granted, or prove a production review defect. The installed Apple SDK's NSApplication header explicitly does not guarantee that `activate()` will activate an app. No repeated unchanged test was started. The native lifecycle check remains failed; no render or acceptance claim, no Unit trailer.

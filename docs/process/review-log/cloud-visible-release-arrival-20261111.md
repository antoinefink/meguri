# Review arrival in the visible Release preview — 2026-11-11

A fresh phone review reached the normal signed Mac Release preview. Both devices then showed 49 reviews and 17 cards to review in the tested deck. No replacement, reset or hide-and-return cycle was used.

The phone baseline was 48 after its update (`drive/20261111T165554Z-65067`). The Debug Mac preview quit normally, and its exact process was absent before the new review. A post-Quit AX read had reopened the previous app; that invalid setup was discarded before recording the new review.

Phone source: `ba58e895`, preparation `run/20261111T165437Z-63294`. In `drive/20261111T170106Z-69052`, one Good committed, the session ended, and Settings requested Sync Now. All 22 interactions completed. The immediately captured status was old, so it was not treated as completion. A separate status observation (`170323Z-71929`) showed a newer successful completion with pending, held-back, in-flight and incoming-waiting counts zero. The later read-only summary (`170503Z-73503`) showed 49 reviews. Cancel left the collection unchanged.

Mac source: `9852d66d`, product `build-app/20261111T164104Z-48706`. Its executable still matched the receipt. The verified personal preview uses the development identity and cloud environment, team signing, schema configuration enabled, normal visible launch and no test hooks. It differs from the phone source only in the mobile Settings identifier addition and documentation.

The native launch helper was unavailable. Computer use opened the exact product in the background and raised its window; Settings was displayed, then Sync Now was requested. The status advanced to a new completion at 18:09 local time. The read-only summary showed 49 local reviews. Cancel dismissed it. The deck overview showed 17 remaining reviews. Exactly one Release process and no Debug process remained.

The tool started the exact product out of focus; Raise displayed its window. Settings offered Sync Now. After that action, Last Synced showed 18:09 local time. This Device showed 49 reviews. Cancel dismissed the read-only sheet. The deck showed 17 remaining reviews. One Release process remained; Debug was absent.

Focused batch static checks pass: `static-checks/20261111T171125Z-79691`, base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The subsequent `CloudServiceProcessTests` run (`swift-test/20261111T171404Z-28634`) passed its asset byte, cancellation, concurrent-frame and saved-receipt assertions. It did not finish the pagination/reopen test. A stack sample placed the idle runner in `CloudServiceProcessWorker.finish` at `Process.waitUntilExit()`, after stdout EOF, with no remaining child process. The exact MG owner was cancelled through SIGTERM; its cancelled receipt is retained. The suite is not counted as passed. A separate, bounded test-helper repair follows; no production sync change is justified by this wait.

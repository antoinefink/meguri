# Quit flush timing observation

Starting commit: `2daf590c`.
Pending batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Role: fixer; session `s-20261110T214025Z-20fa15fd`.
Authority: sync/policy P4 and P11.4, engine/jobs J2.1.1.

Read-only diagnosis `cleanroom/20261113T150256Z-3041` found no established
production ordering bypass. The timeout path removed observers before cleanup;
later callbacks could therefore inspect stale test state.

The temporary trace came from `cleanroom/20261113T151210Z-12068`.
Review `cleanroom/20261113T151940Z-13383` found one P2 and no P0 or P1.
The hook must verify a drained owner before it stops recording. This test's
owner closed successfully. All six changed files now match the original source.
The patch and its evidence remain under the ignored build directory.

`swift-test/20261113T152019Z-14155` compiled and ran the unchanged method with
both arguments. It failed with five issues. In the deadline row, the waiter
started with about 9,998 ms remaining and returned at elapsed 10,375 ms.
The test's separate ten-second signal wait timed out during that cancellation.
The trace then shows ordered revoke acknowledgement and join before SDK and
Core cancellation entry. It shows no ordering bypass in this run. It does not
explain the delayed wake or prove a timing repair.

The other row retains two cancellation errors and its successor-state failure.
The separate predecessor test remains unresolved. Assertions, limits and the
original absolute deadline are unchanged. No error was suppressed.

Temporary source privacy `private/20261113T152039Z-15864` and static checks
`static-checks/20261113T152038Z-15612` passed. The first test invocation,
`swift-test/20261113T151957Z-13884`, rejected its argument format before execution.
No similarity check, full gate, Unit closure or release acceptance is claimed.

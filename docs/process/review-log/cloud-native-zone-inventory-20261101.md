# Native zone inventory checkpoint

U113/U114 remain unaccepted.
Starting commit:2f0e9751004834a9581d98455859cb5425f166e6.
Original integration base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Contracts: sync/policy P4 and P10.3; sync/cloudkit-recovery V26.1.

The own transport prepares a real all-zone CloudKit operation on its existing
private database queue. It collects validated per-zone results and completes at
the original terminal callback. An error prevents successful inventory. A true
empty response remains distinct from failure. Errors retain their original
objects and metadata. Cancellation joins the original callback. No retry or
replacement request is added.

Initial source: cleanroom/20261101T062123Z-23542,
implementer session s-20261025T134357Z-235af9a2.
Round 1 review by native_bounds_review found missing exhaustive test cases and
fixed-yield waiting checks. The first build also exposed the wrong SDK callback
signature. The installed 27.2 interface has per-zone results and a Void terminal.

Fixer cleanroom/20261101T063207Z-31986, session
s-20261025T162405Z-9dc656e4, corrected those paths. Test-only unsupported branches
throw explicitly. Waiter admission is observed inside the actual detached join.
Round 2 found no P0-P2 issue. The next run passed all tests but failed its warning
check. cleanroom/20261101T063921Z-40557 adds only an explicit Void? annotation;
the integrator compared all seven source files to confirm that exact delta.

| Run | Result |
| --- | --- |
| swift-test/20261101T062900Z-28420 | Compile failure |
| swift-test/20261101T063634Z-35515 | 32 tests pass; one warning, job fails |
| swift-test/20261101T063744Z-37605 | Pass, 18 app tests |
| private/20261101T063649Z-36796 | Pass, seven source files |
| similarity/20261101T063654Z-37056 | Pass |

The earlier privacy scan identified comments and a custom fixture string. Their
whole regions changed; no exception was added. Actual Core inventory authority
is a separate committed component. RunCleanup admission, its retained executor,
full cleanup and the enabled app round trip still require integration. Signed
CloudKit evidence, platform checks and the original batch gate remain required.
No Unit trailer is added.

Final focused run swift-test/20261101T064220Z-43569 passes all 32 tests
with zero warnings. private/20261101T064252Z-44700 passes the final test file
and these notes before this result was appended.

similarity/20261101T064307Z-45050 passes the last annotation delta.

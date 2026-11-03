# Review startup activity

Status: WIP; no unit acceptance.
Start:e7f662fd; source:4b4b3c5c.
Batch:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer:s-20261103T122937Z-e1c92493.

A new model is visible before start returns its session ID. An inactive/active
pair can pause it synchronously, while queued resumes return before that ID
arrives. The old startup completion only handled inactive state. A question
could remain paused after activity recovered.

Completion now queues reconciliation after existing lifecycle work. Owner,
generation, closing state and ended phase are checked around the wait. Existing
pause/resume methods decide input admission. Go-to-Deck keeps its earlier path.
DEBUG activity logs contain fixed stage labels and Boolean facts only.

| Receipt | Result |
| --- | --- |
| Export68369 | Fix and three owner regressions |
| Swift70674 | Tests-only RED:2 passed; recovery failed3 assertions |
| Swift72269 | Compile failed: resolver named a nonoptional local |
| Export74430 | Qualify the current optional self.resolver |
| Swift75068 |22 selected tests passed |
| Private73352 / similarity73480 |3 files passed |
| Private77045 / similarity77192 |Final production file passed |

R1 and the one-line R2 review are clear of P0–P2 findings. Tests retain inactive
refusal, replaced-owner isolation and an actual reveal through the test surface.
The final run includes existing coordinator tests, owner close/reopen and real
cloud adoption followed by a committed review. UI lint and zero-warning build
pass within that job. No assertions or limits were removed.

Live reproduction of the exact callback order remains unproved. Preserve the
older native Resume failure, its repair cap and the open batch; no native cloud
code changed.

Signed Build78316 passes from6359c47f. Executable SHA256:
1ddb599fdfb9bab7bf9a469a57c455c9ceb6569c27a2b0143e77c4e9cfb2af40.
First and second live entries reveal answers during media sync. Both fixed
activity traces show the current, active, unpaused model. These observations
do not reproduce the earlier inactive/active ordering.

A later native rating reaches the accepted Core result, advances to the next
question and enables Undo. A normal quit finishes. Reopening shows one recorded
answer and the reduced due count. The bounded debugger reads only phase and
input-state flags, with fixed entry and acceptance markers; it is terminated
and joined, and the app is verified running afterward. An earlier rating with
ambiguous admission is not counted as an accepted answer.

A subsequent accessibility observation shows no decks while the app is no
longer running. Selecting the exact signed product launches one process and
shows the decks and recorded answer again. No collection loss is established.
The fresh process continues media sync. No two-device round trip or complete
media publication is claimed. This is live diagnostic evidence, not a passing
automated gate or unit acceptance.

## Physical phone preparation

UI97339 builds the signed development app with zero errors and warnings. The
test waits for device unlock, then is explicitly interrupted and joined. Its
failed receipt remains: no test passed in that run.

After Antoine reports the phone unlocked, UI2519 reuses the unchanged Core
products and passes TestHooksTests/testSampleScreenRoute. The test supplies no
fixture, changes no collection and leaves the app installed after termination.
Its build has zero errors and warnings. This proves signed installation and
launch, not phone review persistence or iCloud exchange. Xcode MCP cannot select
the physical phone for interactive inspection; no simulator fallback is used.

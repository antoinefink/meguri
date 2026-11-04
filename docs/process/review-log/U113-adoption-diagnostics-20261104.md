# Exact-job adoption diagnostics

Status: WIP; no unit acceptance.
Unit start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start: 11db2bcd.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

Export47369 records finite task/stage and separate primary/cleanup categories
on the exact registered result object. Host only forwards the recorder. Swift
reads that object after drain and uses the existing DEBUG logger. Returned
errors and cleanup precedence are unchanged. No content or identifiers enter it.

Review finds pre-entry failures incorrectly captured. Export64985 starts
capture after claim and corrects the test teardown through public cancellation.
Independent review clears both changes. Core89112 passes the diagnostic cases;
Core58796's teardown abort is retained as the pre-repair result. Private89953
still flags one comment in the test file. That file remains outside this WIP
source commit until its filtered comment repair. Native checks remain pending.

Export93777 removes only that flagged comment. Private98488 and the commit hook
pass. Core96685 passes all 52 selected checks, including the diagnostic cases.
Native97307 compiles the new bridge and runs its selected diagnostics without
failure; two other native cases remain red. The full batch gate is still open.

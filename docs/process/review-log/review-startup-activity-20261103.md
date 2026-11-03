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

Live reproduction of the exact callback order remains unproved. Signed first
and second entry checks remain pending. Preserve the older native Resume
failure, its repair cap and the open batch; no native cloud code changed.

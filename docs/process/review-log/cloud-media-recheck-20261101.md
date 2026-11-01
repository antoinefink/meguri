Media recheck review and checks, 2026-11-01.

U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Media integration start: 9d88dddf78698eed3b463705ebe705fe1472d7e4.
Fixer session: s-20261025T162405Z-9dc656e4.
Final source export: cleanroom/20261101T194447Z-69833.

Independent source review found a P1 key error: blob names used the collection key.
They now use the acknowledged shared media key. Missing media authority no longer rejects a valid collection ACK.
The ACK transaction retains deferred content metadata and its original time.
Later root acceptance creates the correct Unknown blob state and clears deferred facts in the same transaction.

Spec review confirmed that both qualified collection drain flags count for media rechecks.
The first drain and the first drain at least 24 hours after the ACK invalidate cached Present.
Current SDK, account and pause checks remain; absence completion still requires fetched:true.
Unacked-root and duplicate-content ACK edges received source review, without dedicated executed cases.

Native-parity review found that the rig reused its earlier media round after the exchange.
One post-collection MediaResume now uses the same remaining budget.
The hosted regressions moved to Host; no cyclic dependency was added.
Fixture callbacks now return the original handler before saved-name removal, checking the returned request ID.

Retained core-test failures:

| Receipt | Result | Finding |
| --- | --- | --- |
| 20261101T183707Z-93567 | FAIL 2/2 | Read check failed; an unrelated active apply blocked the Format builder. |
| 20261101T184529Z-95436 | FAIL 1/2 | Format passed; a name added again did not cause the expected read. |
| 20261101T185451Z-99819 | FAIL 2/2 | The wrong key created another Unknown blob and changed selection. |
| 20261101T185940Z-7623 | FAIL 2/2 | The post-send drain invalidated Present; the rig reused its old read round. |
| 20261101T192755Z-53774 | FAIL 2/4 | Both Sync cases passed; both Host fixtures used the wrong handler order. |

core-test/20261101T193641Z-63923 PASS: 545 selected tests, zero failures.
Counts: Host 112, Package 106, Sync 327; 387 tests were unselected.
Coverage includes both hosted drain cases, stale Present reuse, Format inbox protection, ZIP memory, reviews and crash/reopen workflows.
Inputs were the reviewed working tree on 9d88dddf, not a clean-head receipt.

The final cleanup removed two comments and renamed one test; production and assertions stayed identical.
private/20261101T194629Z-73016 PASS on the 15 source files.
core-test/20261101T194657Z-73655 PASS 3 selected cases after cleanup.
similarity/20261101T194259Z-66574 PASS before the textual cleanup.
The unchanged behavior retains the 545-test evidence; no full-suite rerun is claimed.

No direct mediaMissing status witness exists. In-flight warming of Present is not separately isolated.
These checks do not prove current native rendering or signed multi-device CloudKit behavior.
No Unit trailer, broader P24 closure, milestone, integration gate or release acceptance is claimed.

Final lint first failed only two cloned-reference slices in Host tests (core-lint/20261101T195405Z-79393). The filtered cleanup uses std::slice::from_ref with identical data and ordering; no lint allow was added. Final lint PASS: core-lint/20261101T200024Z-8739.

core-test/20261101T200156Z-9924 PASS2/2 on the final borrowed-slice form.

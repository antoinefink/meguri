# U19 catch-up result reuse — in progress

Start: `1a2466a56096d31155f9c6afde5259b1e5ec9b01`.
Session: `s-20261018T131028Z-b9f766d3` (Codex, infra).

Antoine approved avoiding duplicate verification on 2026-10-18.
U31's full gate passed Xcode check in 945,205 ms. Its following catch-up
then reran the same matrix in 835,003 ms on the same clean commit, checkout
and arguments. Reuse avoids that second run.
Snapshot and Xcode catch-up both passed, clearing the old queue.

Catch-up now checks the latest completed result before rerunning a job.
Reuse requires a real pass for the exact clean HEAD, checkout and arguments,
with no failures, queued parts or missing required steps in that result.
Resolve the latest pointer once so a concurrent run cannot switch evidence.
A matching result settles an entry even if its former machine block returned.
Otherwise the existing doctor, rerun, queue and escalation paths apply.
The queue and report preserve the original result path and mark reuse.

Normal gates and direct job calls still execute fresh. There is no general
build cache, worker scheduler, VM change or new dependency.

Regression evidence: the positive reuse case failed on the old code because
the entry stayed queued despite matching passing evidence. Guard tests cover
changed commits, arguments, newer failures, dirty/dry/queued results, wrong
checkout and malformed metadata. A macOS bash 3.2 integration case exercises
reuse with an argument containing spaces and a now-locked host.

Round 1 (Codex, frozen `22b4a771`): two P2 findings. The proof predicate
accepted missing standard metadata, and it did not recheck HEAD and
cleanliness after validation. Both received failing regression tests, then
fixes: validate the required standard fields and their shapes with jq, and
recheck the sampled clean HEAD before returning proof. The regression
injects both a commit and a tracked edit during proof validation. All three
focused cases pass, including 22 invalid-evidence variants.

Before those fixes, all 442 Bats tests and all 27 macOS host tests passed
(`selftest/20261018T131550Z-28657` and
`selftest-host/20261018T131551Z-28856`; exact paths in orchestration output).
Round 2 (Codex, frozen `7f17f4dc`): one P2 follow-through finding. A
checkout that became dirty while validating evidence still fell through to
a fresh run. The revised regression failed before the fix. Catch-up now
rechecks the checkout before fallback, including untracked files, and stops
without running or settling the queue if it changed. A clean concurrent
commit remains eligible for a fresh run. All 14 catch-up tests pass,
including commit, tracked-edit and untracked-file races.

Round 3 and the required full gate remain pending.

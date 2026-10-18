# U19 catch-up result reuse — in progress

Start: `1a2466a56096d31155f9c6afde5259b1e5ec9b01`.
Session: `s-20261018T131028Z-b9f766d3` (Codex, infra).

Antoine approved avoiding duplicate verification on 2026-10-18.
U31's full gate passed Xcode check in 945,205 ms. Its following catch-up
then reran the same matrix on the same clean commit and arguments.
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

Focused tests, full tooling checks, review and the full gate remain pending.

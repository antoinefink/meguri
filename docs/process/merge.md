# Merging a unit

## Current workflow: direct commits on master

Antoine changed the workflow on 2026-10-18. Work directly on `master` in the main checkout; create no branches or worktrees. Inspect an existing unit's status and log before bringing its prepared work onto `master`. Preserve its spec and implementation history and any uncommitted work.

Record the unit's starting commit before bringing in its work. Keep the spec, implementation, review and fix steps, the clean-room restrictions, and the review log. Commit WIP freely with co-author attribution. Before accepting a completed unit, commit it with its `Unit:` trailer and run `tools/mg gate --base <unit-start-commit>`. Fix failures before starting the next unit. Only push `master`, without force. `tools/mg merge` is not required.

The branch-merge procedure below remains available for historical receipts and existing tooling. Queued runs remain incomplete verification: run `tools/mg catch-up` between units and require passing results at milestone gates.

A full, clean main-checkout gate with an explicit base saves its queued checks
for catch-up when the tip commit carries `Unit:`. Local, partial and dirty gates
do not save acceptance work. Keep `Unit:` on the completed unit's tip commit.

## Earlier workflow: role branches

`tools/mg merge <branch>` checks and fast-forwards the integration branch in the main checkout (`master`). The origin remote uses `master` as its default branch. The orchestrator runs it in the main checkout. A green receipt merges without human review (R1991).

## What it does

1. Refuses when the unit has an open escalation in the main checkout's ledger joined with the branch tip's (an escalation added or resolved in the unit's worktree counts; a ledger that does not parse is refused), when the branch does not contain the integration head (merge the integration branch into the unit branch first), or when the branch's worktree is not clean at the branch tip.
2. Runs `tools/mg gate --base <integration head>` in the branch's worktree, with the branch's own tiers and tools: the worktree-safe T0 and T1 runs (docs/process/tiers.md).
3. Runs what the worktree left, in the plan's order and under the gate's rules (nothing after a failed canary; T1 only when T0 has nothing blocking):
   - the `where: main` runs from the main checkout against the branch's tree: `static-checks --rev <tip>` (which also reads `oracle/`) and `similarity --rev <tip>` (which needs `refs/`);
   - the `deferred-tip` and `needs-oracle-tree` runs in a full checkout of the tip at `build/mg/merge-tip`: the branch's own tools and `oracle/`, and the main checkout's similarity corpus (`MG_SIMILARITY_REFS`). That checkout is removed when the merge ends; their results are copied to `build/mg/merge/<run>/tip/`. One merge at a time: an existing `build/mg/merge-tip` is refused (`git worktree remove --force` it once no merge runs).
4. Checks that the worktree is still clean at the tip and that no run changed the main checkout or the tip checkout.
5. Evaluates one receipt for the tip with the branch's gate (`build/mg/merge/<run>/receipt.json`), keeping the worktree receipt's own blockers (`tree-changed`).
6. Fast-forwards the integration branch to the tip only when that receipt passes, is complete and is for a clean tree, and only when neither branch moved meanwhile. Otherwise the integration branch does not move and the result says why. A queued run does not block (docs/process/tiers.md, "Queued runs").
7. After the fast-forward, appends each queued run of the receipt to `build/mg/catch-up/queue.jsonl` in the main checkout: its job, the arguments its result ran with, the tip commit, the user steps it waits for, the parts that did not run, the unit and the branch.

## Catching up queued runs

`tools/mg catch-up` runs in the main checkout, on a clean tree; the orchestrator's watcher runs it on its schedule. One catch-up at a time works the queue: it holds `build/mg/catch-up/lock` from reading the queue to writing the last state line, and a catch-up started meanwhile does nothing and passes (a dead holder's lock is taken over). Under that lock it first commits escalation lines an earlier run appended to the ledger but could not commit, then checks that nothing else is uncommitted (`main-dirty`); a ledger it cannot settle that way stops it (`error`, `escalation-ledger`). A queue line that does not parse as an entry stops it (`error`, `queue-unreadable`) before anything runs. It then records the escalations an earlier catch-up could not record (below), then reads the queue, runs `tools/mg doctor`, and leaves every entry whose user step doctor still lists as missing (`host-session` while the Mac is locked, `mac-input` while the Mac UI runner still has the macOS and Xcode builds that dropped input, `mac-vm` without the VM and while it does not answer over SSH in a logged-in session or has fewer free processes than a job needs); while a step holds, it runs nothing for it and passes. Every other entry is run again once on the integration head, one run per job and arguments, and gets a line with its new state:

- the run passes: `done`;
- it fails: `failed`, and a `blocked-red` escalation for each unit whose merge queued it, with the run's directory. Units that do not depend on that unit go on: it is not a pause. When an escalation cannot be recorded (the ledgers' lock stays busy, the commit fails), that unit's entries are `escalation-pending` with the failed run's result and catch-up ends `error`; the next catch-up records the escalation from that result, without running the job again, and marks them `failed`. An escalation counts only once committed: one appended but not committed is committed first, and reused;
- it is queued again (another state now holds it): it stays `queued`, waiting for the steps the new result names;
- it errs: the entries stay as they were, and catch-up ends `error`.

The queue is append-only; the lines of an entry's id, merged in order, are its state. A run's lines are appended as soon as it ends.

Before rerunning a queued job, catch-up checks its latest result. A real,
successful result for the same clean commit, checkout and exact arguments
settles the entry without another run, even when the machine is now locked.
Dirty, dry, queued, warning, failed, malformed and older-commit results are
never reused. The result path and `reused: true` remain in the queue and the
catch-up report. Catch-up does not search past a newer failed result.
Full unit and milestone gates still execute their checks normally. Running
the job directly always requests fresh verification of code or setup.

## Oracle runs

From a sparse role worktree, an oracle-image run uses the main checkout's `oracle/` when that is exactly the branch's (same tree id, nothing changed under it). When the branch changes `oracle/` (a spec side's driver change or an oracle bump), the worktree's gate leaves the run to the merge (`deferred-tip`), and with it goldens-check and harvest-report, which compare the branch with the lock and the harvest inventory under `oracle/` (`oracle_files`); a job that refuses its tree anyway (`oracle test` always does without its own `oracle/`) reports `needs-oracle-tree`. The merge runs both kinds in its tip checkout, against the branch's exact tree.

## What reaches the implementer, and escalations

- A similarity flag: the merge copies the implementer view (`implementer.md`, `implementer.json`) to `<worktree>/build/mg/merge-feedback/<run>/` and records an approval escalation. The reviewer view stays in the main checkout. The implementer rewrites the region from the spec, or Antoine approves an allowlist entry (R1957).
- A changed approved signature baseline (`needs-human-review` from `tools/mg snapshots`): the merge records an approval escalation; Antoine reviews it in the gallery (R1991).

`docs/process/provenance/escalations.jsonl` is the list of what waits for Antoine. Each entry has a `kind`:

| Kind | When |
|---|---|
| `blocked-red` | a unit has no green gate after six review rounds, or `tools/mg catch-up` found a run its merge queued failing |
| `approval` | a change needs a similarity-allowlist, quarantine or approved-baseline entry |
| `behavior-gap` | a spec or the oracle cannot settle a behavior |
| `stop-condition` | a Goal Capsule stop condition |

`tools/mg escalation add --kind <kind> --unit <id> --summary <text> [--detail <JSON>]` records one; `tools/mg escalation resolve <id> --by <name> --resolution <text>` settles it after Antoine decides; `tools/mg escalation list [--open] [--unit <id>]` reads them. Lines are only appended and committed on the checkout's current branch (union merge); `list` reads that checkout's ledger only. The orchestrator records `blocked-red` after the sixth red round. A unit with an open escalation stays unmerged; units that do not depend on it go on. This is not a pause.

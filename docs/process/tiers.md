# Test tiers, quarantine and scheduled runs

Every verification job belongs to a tier (R1950). `tools/gate/tiers.yaml` is the one list: each run is a `tools/mg` job with its arguments, tier, the paths that trigger it, where its work happens (R1952) and a time budget. The gate refuses that file when a registered job has neither a run nor a line under `utilities`, so a new job must be placed in a tier in the change that adds it.

| Tier | When | What | Command |
|---|---|---|---|
| T0 | every change | static checks, then canaries, then for the changed paths: committed goldens on macOS and Linux, Rust and Swift unit tests, `lint-ui`, `core-lint`, `core-deny`, the notices check (U16), the similarity scan, snapshot and accessibility tests for touched UI, the simulator perf smoke for touched hot paths (R1863) | `tools/mg gate` |
| T1 | every change to `core/` (and to scenarios or goldens) | live oracle suites for the touched areas, the sync smoke, oracle-checked fixtures and anonymizer | `tools/mg gate` |
| T2 | nightly; weekly runs on Sundays | cargo-fuzz and the differential fuzzer with one logged seed (R1937), JPlag, Meguri-versus-Anki timing, benchmarks, the full sync, media and fault matrix, long simulations; weekly: the gallery, the soak, sync fuzzing | `tools/mg nightly [--weekly]` |
| T3 | milestone and release | the private collection suite, the macOS-native oracle, Mac benchmarks, harvest coverage (R1880), the release audit | `tools/mg milestone <M0..M5\|release>` |
| T4 | milestone and release | device performance, run by Antoine (U256) | listed by `tools/mg milestone` as awaiting him |

## The gate

`tools/mg gate [--local] [--base <rev>] [--all] [--only <run>,...] [--plan]` measures the change from `--base` (default: where the branch left the main checkout's branch) to `HEAD`, working-tree changes included, and runs in this order:

1. Static stage: `tools/mg static-checks`, every time. It fails when a test, script or scenario names an AnkiWeb host (R1876; only the U116 and U255 binary-scan scripts may be allowed, in `tools/gate/static-checks/ankiweb-allow.txt`), when a commit of the branch lacks its provenance trailers (the commit-msg hook's rules, so a `--no-verify` commit is caught), or when a test plan, nextest configuration or script retries failing tests (R1953).
2. Canary stage: the pin canary (`core-test --canaries`) and the canary scenarios (the 10,000-pair fuzz table, pending until U45; its smoke subset; the unicase collation scenarios). A canary whose failure blocks stops the gate; nothing else starts (AE82: a pin bump fails before anything else). A quarantined canary failure does not block, so it stops nothing.
3. T0 runs whose triggers match a changed path.
4. T1 runs, only when T0 has nothing blocking. The live-oracle run gets the scenario areas of the changed crates (`filters` in tiers.yaml); a change to the runner, the CLI, the lock file or the scenarios selects every area.

Each run is a nested `tools/mg` call: its result is final. Nothing is retried. The gate writes `build/mg/gate/<run>/receipt.json`: the commit, whether the tree was dirty, the base, every run with its outcome, the quarantine state, the receipt-level blockers and the blocking failures. A job that changes the working tree fails the gate (a receipt is for a commit): a receipt-level `tree-changed` blocker, which `tools/mg merge` carries over. A run over its budget is reported, not failed.

Outcomes: `pass`, `warn`, `queued` (a VM job without the VM: recorded as queued, root AGENTS.md), `quarantined`, `fail`, `error`, `needs-oracle-tree`, and for runs that did not start `not-triggered`, `pending` (the job lands with a later unit), `disabled` (the job exists, its run waits for the named unit), `unresolved` (an argument could not be filled in), `skipped-local`, `deferred-main`, `deferred-tip`, `user-step`, `skipped-after-canary`, `skipped-after-t0`. `fail`, `error`, `unresolved` and `needs-oracle-tree` block. A receipt with a `skipped-*` or `deferred-*` run is incomplete and never merges.

- `--local` skips the similarity scan and the oracle-image runs: an implementer worktree has no `refs/` or `oracle/` (U2). Implementers run it during review rounds. Its receipt never merges.
- In a role worktree, `where: main` runs (the similarity scan, the static checks over the whole tree) are `deferred-main`: `tools/mg merge` runs them from the main checkout. `where: tip` runs (the similarity self-test: the branch's scanner with the main checkout's corpus) are `deferred-tip`, and so are oracle-image runs and the runs that read `oracle/` files (`oracle_files`: goldens-check's lock pin, harvest-report's inventory; `--local` included) when the main checkout's `oracle/` is not exactly this branch's: `tools/mg merge` runs them in a full checkout of the tip.
- An oracle job that refuses the `oracle/` it would use (`oracle-tree-differs`, `no-oracle-tree`: `conformance --live` fails with it, `oracle test` errs) is `needs-oracle-tree`; `tools/mg merge` runs it again in that full checkout.
- An argument `{artifact:<run>:<file>}` is a file an earlier run of the same gate wrote: harvest coverage reads the live oracle run's T1 report (`t1-live.json`), so live chains that pass live count as passing.
- `--plan` prints the plan and runs nothing.

## Quarantine

A flaky test fails the gate until it is fixed or quarantined (R1953). `tools/gate/quarantine.yaml` lists quarantined tests: the tiers run and the failure id its job reports, a class, an owner, a reason, a deadline, `approved_by: antoine` and evidence. A quarantined test still runs and is reported; its failure does not block until the deadline. After the deadline the gate fails, whether the test failed or not.

1. Run the gate twice on one clean commit: the test fails in one run and its run passes in the other. These receipts are the evidence.
2. Record an approval escalation (`tools/mg escalation add --kind approval ...`) and ask Antoine in chat.
3. Only after his approval: `tools/mg quarantine add --run <run> --test <id> --class <class> --owner <owner> --reason <text> --deadline <date> --failed <gate run> --passed <gate run> --approved-by antoine [--escalation <id>]`. It checks the receipts and writes the entry. Commit `tools/gate/quarantine.yaml`.

An entry without the approval or the evidence makes the gate fail. A test of a conformance, sync or data-safety run keeps that class. Every milestone run fails while any conformance, sync or data-safety test is quarantined. `tools/mg quarantine check` validates the file.

## Nightly and weekly

`tools/mg nightly [--weekly] [--seed N]` runs T2 and writes one summary result. It draws one seed per night (or takes `--seed`), passes it to every run that takes `{seed}`, and logs it in the summary, so `tools/mg nightly --seed N` replays a night (R1937). The runs' budgets are in tiers.yaml; the differential fuzzer (U247) runs 30 to 60 minutes.

There is no git remote, so nights run on this Mac. `tools/mg nightly --print-schedule` writes two launchd entries (nightly at 02:30, weekly on Sundays at 04:00). Installing them is Antoine's step `nightly-scheduler` (docs/process/one-time-setup.md); agents never install them. A future cloud runner may run only Docker jobs without private data (R1939).

## Milestones and release

`tools/mg milestone <M0..M5|release>` runs in the main checkout on a clean tree. It runs every T0 and T1 run in full (no path triggers), then T3, and lists T4 as awaiting Antoine. It writes one summary result. It fails when a run fails, while a conformance, sync or data-safety test is quarantined, and on the R1880 rules of its harvest-coverage run: no unclassified harvest row at any milestone, and at release no in-scope or preservation-only scenario that does not pass. Harvest coverage takes the statuses of live chains from the full live-oracle run of the same milestone run; without that report (T1 did not run) it is `unresolved`.

`tools/mg milestone-gate <M0..M5>` (U240) then decides whether the milestone closes. `docs/process/milestones.yaml` lists, per milestone, its units, its suites (tiers runs of T0, T1 or T3, with scopes and pass rules), the budgets its device runs cover, Antoine's user steps with their evidence and validators, approved deferrals, and the conditions every gate checks. Gates are cumulative: a milestone's suites, units and deferrals include every earlier milestone's. The gate:

- reads the latest result of each suite's run with the milestone's arguments from `build/mg`; every result must be from one clean commit, the gate commit, and a missing result fails;
- accepts later commits that touch only evidence paths (`milestone-records/`, `conformance/private/`, the user steps' evidence) and reads the evidence from HEAD's commit, never the working tree;
- needs every unit merged at or before the gate commit (a `Unit:` trailer from an implementing role) with no open escalation;
- fails while a conformance, sync or data-safety test is quarantined or any quarantine entry is invalid or expired.

The outcome is `pass`, `fail`, or `awaiting_user` (status warn) when only Antoine's evidence is missing. A pass writes `docs/process/milestone-records/<M>.json` (commit, per-suite counts, user-step results, open quarantine entries with owners, deferrals); commit it. `tools/mg milestone-gate --check`, a T0 run, keeps the manifest equal to the plan's Delivery Milestones table and Unit Index and its suites on runs that `tools/mg milestone` executes.

## Changing tiers.yaml

- A job that lands later is listed with `pending: <unit>`. Once the job is registered, the gate refuses the file until that unit wires the run (removes `pending`).
- `disabled: <reason>` holds back a run whose job exists (the critic waits for U254, because a critic run writes `docs/design/screen-status.yaml`).
- `needs` names what a run uses: oracle, sync server, image diff and frame extraction run in Docker; simulators, devices and Mac UI on the host or the test VM (R1952). The gate refuses a placement that breaks this.
- A new conformance area adds its crate to the live-oracle `filters`.
- A run whose job reads files under `oracle/` without starting the oracle (`mg_conf__oracle_file`) sets `oracle_files: true`; otherwise a branch that changes `oracle/` fails it against the main checkout's tree.

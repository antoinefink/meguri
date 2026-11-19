# Hosted Core timing checks and suite selection

Starting commit `93d9e7fa5ab0446c132b148f53a2209c131e6f0d`. Infrastructure
change; no test, bound or production code changed.

GitHub run `44502568215` (job `135486336749`) on `93d9e7fa5` finished Core with
5,615 passed, two failed and 15 skipped:

- `meguri-engine::wal_durability twenty_commits_are_full_synced_within_one_second_with_a_snapshot_open`
  took 5.06 s and saw `Seq(15)` covered only by a sync through `Seq(21)` more
  than one second later. Locally it takes 1.56 s. It already ran alone
  (`threads-required`). It started as a 99 s 150,000-note test ended, and
  trivial FFI tests that followed took over 2 s each, so the runner was
  I/O-stalled rather than CPU-contended.
- `meguri-ffi sync::tests::connection::jobs::adoption_transport_failure_during_account_unavailability_still_requires_explicit_resume`
  failed with `SuspendDeadline { phase: "derived connection close" }` after
  14.29 s, while it shared the runner with other tests.

Repair: a nextest wrapper script runs `sync` before each `wal_durability` test,
and the adoption test now runs alone. Every timing bound is unchanged.
Focused `core-test/20261119T110842Z-37445` passed all 21 selected cases with
the new configuration, including both original failures. The hosted result
on the pull request is the first runner evidence. One pass there does not
establish that the hosted failure is resolved.

The workflow now selects suites per change. A web-only push no longer starts
Core, and per-suite concurrency groups keep it from cancelling a Core run.
Replaying the selection on the five web commits before `93d9e7fa5` and on
two Core commits gave the expected suites.

Independent reviewer `hosted_core_selection_review` reviewed the diff. It
found one blocking defect: under `pipefail`, `grep -q` closing its pipe early
skipped Core on large changes. It reproduced the defect and verified the fix
under `bash -e -o pipefail`. It also read the complete final workflow file
without opening private inputs and classified it as public CI configuration.
Decision: admit.

| File | SHA-256 |
| --- | --- |
| `.github/workflows/meguri-verification.yml` | `17c70e069e904e69a3f53844e493bc3cf2155dae627ba36c7160a9e7555908c0` |

`static-checks/20261119T113010Z-10478` passed. Similarity cannot run in this
worktree (`no-corpus`: no `refs/anki`). The change touches only configuration
and documentation.

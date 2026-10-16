# Merging a unit

`tools/mg merge <branch>` is the only path to the integration branch: the branch checked out in the main checkout (`feat/meguri-v1` today). There is no git remote, so this job, not a push hook, enforces the receipt (R1951). The orchestrator runs it in the main checkout. A green receipt merges without human review (R1991).

## What it does

1. Refuses when the unit has an open escalation, when the branch does not contain the integration head (merge the integration branch into the unit branch first), or when the branch's worktree is not clean at the branch tip.
2. Runs `tools/mg gate --base <integration head>` in the branch's worktree, with the branch's own tiers and tools: the worktree-safe T0 and T1 runs (docs/process/tiers.md).
3. Runs the `where: main` runs from the main checkout against the branch's tree: `static-checks --rev <tip>` (which also reads `oracle/`) and `similarity --rev <tip>` (which needs `refs/`).
4. Evaluates one receipt for the tip with the branch's gate (`build/mg/merge/<run>/receipt.json`).
5. Fast-forwards the integration branch to the tip only when that receipt passes, is complete and is for a clean tree, and only when neither branch moved meanwhile. Otherwise the integration branch does not move and the result says why.

## Oracle runs and `--receipt`

An oracle-image run uses the main checkout's `oracle/` from an implementer worktree. When the branch changes `oracle/` (a spec side's driver change), that tree is not the branch's: the run reports `needs-oracle-tree`. Run `tools/mg gate --base <integration head>` in a checkout of the same tip that has `oracle/` (a full worktree of the branch), then pass its receipt: `tools/mg merge <branch> --receipt <that receipt.json>`. Such a receipt only supplies runs that could not run in the worktree; it never replaces a result the worktree's gate already has.

## What reaches the implementer, and escalations

- A similarity flag: the merge copies the implementer view (`implementer.md`, `implementer.json`) to `<worktree>/build/mg/merge-feedback/<run>/` and records an approval escalation. The reviewer view stays in the main checkout. The implementer rewrites the region from the spec, or Antoine approves an allowlist entry (R1957).
- A changed approved signature baseline (`needs-human-review` from `tools/mg snapshots`): the merge records an approval escalation; Antoine reviews it in the gallery (R1991).

`docs/process/provenance/escalations.jsonl` is the list of what waits for Antoine. Each entry has a `kind`:

| Kind | When |
|---|---|
| `blocked-red` | a unit has no green gate after six review rounds |
| `approval` | a change needs a similarity-allowlist, quarantine or approved-baseline entry |
| `behavior-gap` | a spec or the oracle cannot settle a behavior |
| `stop-condition` | a Goal Capsule stop condition |

`tools/mg escalation add --kind <kind> --unit <id> --summary <text> [--detail <JSON>]` records one; `tools/mg escalation resolve <id> --by <name> --resolution <text>` settles it after Antoine decides; `tools/mg escalation list [--open] [--unit <id>]` reads them. Lines are only appended and committed on the checkout's current branch (union merge). The orchestrator records `blocked-red` after the sixth red round. A unit with an open escalation stays unmerged; units that do not depend on it go on. This is not a pause.

# Private-token scan consistency: review log, 2026-11-21

Branch `unit/privacy-scan-consistency` (PR #94), infra session `s-20261121T190806Z-09bc10bd`.

## Cause

The hook refused a merge of `master` in a worktree with an old base, for lines `master` already had. Worktrees with a recent base merged the same lines. A whole-file `tools/mg private scan-outputs` run then reported 14 files and 41 lines.

Findings, from paths, line numbers, counts, hashes and commit metadata only. No token file was opened and no flagged line was printed.

- Every worktree reads one token set (the main checkout's) through one set of hook shims. Neither differs between worktrees.
- All 14 files have a whole-file pin in `PUBLIC_ARTIFACTS`, and every pin is stale: later commits changed other lines. The hook scans added lines only, so those commits passed. The reviewed lines are flagged again where they are seen as added: a whole-file scan, or a merge from a base older than them.
- 40 of the 41 lines are unchanged members of bytes whose digest was pinned after a complete-content public review. The 41st, a line of the session archive, was admitted by digest on its unit branch; a rebase changed the archive's bytes and the pin on `master` was never refreshed.
- On `master` at `f17019af5`, 79 of 288 pinned files are stale.
- The hook exited 0 when the token set was missing or unreadable.

## What changed

See the PR description. No pin, exemption or matching rule changed. The real token set was not refreshed: that is the owner's decision (`docs/process/provenance.md#one-answer-in-every-checkout`).

## Independent review

Reviewer: an independent Claude subagent, non-author, reading commit `2ccb50391` only. It was barred from `local-data/`, `build/private/`, the scanner, the hooks and the private jobs. It found no path that prints or stores token text and no blocker. Findings and decisions:

- **Should fix: the "expected" key was set only by `private copy`.** A set built earlier had no key, so a session that could see neither the set nor the collection folder still passed unscanned. Fixed: `hooks install` (run by every `worktree new`) and `refresh-public` set the key when they see a set.
- **Should fix: `private status` in a worktree reported the worktree's own set.** Fixed: it reports the main checkout's.
- **Minor: an empty set beside a full one passed the emptiness check.** Fixed: each set must hold a token.
- **Minor: a malformed `baseline.json` broke the set's facts.** Fixed: anything but one object with a commit id and a hash is no baseline.
- **Minor: missing vendor pins were not counted.** Fixed: `pins` reports their number (their paths are stored as hashes).
- **Minor: an empty baseline commit id printed as blank.** Fixed: doctor treats it as unknown.
- **Minor: docs overstated "every worktree".** Fixed: the doc says a branch older than the rule keeps its old hook until it merges `master`, and what an emptied set does.
- **Not changed: `core.hooksPath` fails `private-hooks`.** With it set, git does not run the shims; doctor cannot tell whether that directory calls the hooks.
- **Not changed: no test of `--shared-main` writing the baseline.** The branch is one line; the python refresh behind it has its own tests.

## Checks

- `tools/mg selftest tools/tests/private.bats`: 25 pass.
- `tools/mg selftest tools/tests/mg_selftest.bats --filter "doctor|hooks"`: 21 pass.
- `tools/mg doctor` on this Mac: `private-hooks` pass; `private-scan` warn (82726 tokens, baseline unknown until the next refresh).
- Not run locally: the remaining Linux suites (left to GitHub Actions), `selftest-host`.

## CodeRabbit

- **An empty `core.hooksPath` was read as unset.** Fixed: `mg_hooks_check` uses the exit status of `git config --get`.
- **A set of blank lines was counted as tokens.** Fixed: the count is of non-empty lines, as the scanner reads them, so doctor fails on such a set.
- **`private status` in a linked worktree.** Already fixed in `927ac7ab1`.

- **The token files were read twice, once to check and once to scan.** Fixed: each is read once and the matcher is built from what was checked.

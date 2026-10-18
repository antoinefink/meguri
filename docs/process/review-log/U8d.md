# U8d review log

Unit: U8d, the Mac app stays out of sight on the host (no Dock icon, no activation as a test host or a hidden tools/mg launch).

## Integration (2026-10-18)

Integrator, in `meguri-wt/U8d` (unit/U8d).

### Rounds

Two review rounds ran. Each had findings that were closed.

- Round 1 (implementer s-20261017T192011Z-a1e21289, bfc8bed7). `tools/AGENTS.md` named only `mg_app_mac_guard`, so a new job could launch the Debug app with a plain `open -g -j` and put it back in the Dock. Fixed: the notes name the four helpers such a job uses (Debug only, the guard, `mg_app_open_mac`, the lsappinfo check).
- Round 2 (fixer s-20261017T215543Z-ba8ce4f2, 69c7f8f0). The out-of-sight check in `run`, `screenshot` and `xcode-check` failed only on a Foreground type or "(in front)"; a failed or empty lsappinfo query, an unknown type or a missing pid passed silently. Fixed: the check requires UIElement or BackgroundOnly for a known pid and reports anything else as a no-evidence failure, with self-tests for each case.

### Open after round 2

None.

### Merge

- No spec/U8d branch exists.
- unit/U8d already contained feat/meguri-v1, so `git merge feat/meguri-v1` was a no-op.

## Resumed integration (2026-10-18)

Antoine changed the execution policy to Codex only, with implementation,
review and fixes performed directly. The earlier completed review rounds above
remain the evidence for the original change; no Claude or Opus review was
started for this resumption.

- Incorporated `feat/meguri-v1` at `c2e8e834`. Only `AGENTS.md` and
  `tools/AGENTS.md` conflicted: retained U8d's hidden-launch checks together
  with U19c's VM-unreachable queueing rules.
- Rechecked the combined launch-policy and LaunchServices checks, the Debug-only
  configuration, and the shared TestHooks test. The macOS native `clickOrTap()`
  repair is preserved. No new P0-P2 finding remains from this integration review.
- The merge gate supplies fresh verification for the combined tree. The existing
  host-session checks remain queued while Antoine's host is locked.

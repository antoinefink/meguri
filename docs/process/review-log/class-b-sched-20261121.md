# Review: class (b) rewrites in sched scenarios (PR #87), 2026-11-21

- Branch `unit/class-b-sched`, author spec-writer session `s-20261121T180626Z-6fb8c9f6`.
- Reviewer: a separate Opus agent. It did not write the rewrites, ran no jobs, and read Anki only at the pinned tag (`refs/anki-26.09.3`). It quotes no Anki text.
- Context: `similarity-test-data-dispositions-20261121.md` (class b, rewrite decided by the coordinator).

## Scope

20 scenarios. The reviewer read the 10 that change more than a column list, and 4 of the 10 plain column-list drops (`loadbalancer/crafted-days`, `options/missing-preset`, `revlog/kept-rows`, `memory/lrt-repair`). A script checked that every changed insert is a plain column-list drop. The other changes:

- `answer/state-kinds`, `memory/{crafted-rows,no-history,recompute,reset,sm2-history}`, `options/remap-steps`: card selectors with 8 or more ids now list odd ids first, then even ids.
- `compat/enable-fsrs`, `compat/re-enable`: the nine-column read-back is now `SELECT * FROM revlog ORDER BY id`.
- `loadbalancer/siblings`: the `IN` subquery is now a correlated `EXISTS` over notes joined to notetypes.

## Findings

- Same behaviour: yes.
  - `card`, `revlog` and `next_states` output maps keyed by `#cN`, and goldens store the keys in sorted order, so the order of ids in a selector cannot change them.
  - `db_exec` goldens record only rows, and `SELECT *` returns the same columns in the same order.
  - The `EXISTS` form matches the old one because note type names have a unique index. `cards.nid` refers to the table being updated, and a missing name updates nothing in both forms.
- Goldens: only `scenario_sha256` and `driver_sha256` changed, in all 20.
- New overlap: none. The tag has no positional insert into the review-log table and no `EXISTS` join on notes and notetypes. Two `SELECT * FROM revlog` statements in Anki share at most 6 or 7 words with the new read-back, which is below the 8-word run.
- Comments: none is stale. The comments in `compat/enable-fsrs`, `memory/reset` and `options/remap-steps` describe the VALUES rows or the setup and still hold.
- Non-blocking: positional inserts and `SELECT *` depend on the table's column order, which schema 18 fixes on both sides.

Verdict: approve.

# Review: class (b) rewrites in stats scenarios (PR #86), 2026-11-21

- Branch `unit/class-b-stats`, author spec-writer session `s-20261121T180133Z-9326df1d`.
- Reviewer: a separate Opus agent. It did not write the rewrites, ran no jobs, and read Anki only at the pinned tag (`refs/anki-26.09.3`). It quotes no Anki text.
- Context: `similarity-test-data-dispositions-20261121.md` (class b, rewrite decided by the coordinator).

## Scope

63 scenarios. In each one, the only change is that `INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type)` becomes `INSERT INTO revlog` (VALUES or SELECT). A script checked that in all 63 files. The reviewer read 16 of them in full: card-info (params, zone-dst), browse-queries/calendar-cells, graphs (buttons, tz-matrix, calendar, hourly, retention, reviews, scope), studied-today/time-units, today (search-scopes, deck-subtree) and widget/day-start-spring-forward-r2-early-row.

## Findings

- Same behaviour: yes. The review-log table has the same nine columns in the same order in Meguri's schema-18 DDL and in Anki's schema at the tag, so a positional insert fills the same columns. That includes non-integer and out-of-range values (`graphs/scope/odd-row-values`).
- Goldens: once the hash lines are filtered out, nothing else is left. Only `scenario_sha256` and `driver_sha256` changed, in all 63.
- New overlap: none. The tag has no positional insert into the review-log table.
- Comments: none is stale.
- Non-blocking: positional inserts depend on the table's column order, which schema 18 fixes on both sides.

Verdict: approve.

# Round 11 approval and two spec records (2026-11-22)

Branch `unit/stats-spec-approval`, spec-writer session `s-20261122T113058Z-6bd475a0`. Decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs.

## Decisions

1. **Round 11 approved.** Every item of the Statistics plan's §8 (`docs/plans/2026-11-22-1200-feat-statistics-page-implementation-plan.md`) is approved 2026-11-22 (R1357), after PR #113's reviews (`statistics-round11-20261122.md`); Antoine approved the design in chat the same day. The Status lines of the eleven stats and search files and N25's sentence say so; `docs/design/decisions.md` has the row.
2. **The structure upload texts.** PR #112 (U269) built `sync.needsYou.meguri.structureUpload` ("Upload this collection to finish your note type changes.") and `sync.needsYou.meguri.action.upload` ("Upload…") from `sync/meguri-provider#MP7` step 5, with no row in `sync/status-strings`. T19.5 now has the row, placed and conditioned as `SyncKinds.meguriRow` decides: after the connection rows, before the setup stages; not while an MP7 job runs; a refused sign-in shows the sign-in row first. MP7 step 5's pointer said T19.4 (iCloud's table); it says T19.5.
3. **The heatmap day of Show in Browse.** `browser/workspace` V15 said a heatmap day's cards are found by the heatmap day's own bounds (`stats/day-buckets` B4), while the heatmap it serves counts by Anki's day (`stats/activity-heatmap` AH1, AH3; B3), as round 11's Activity and Q9 do. At an offset change the click listed other cards than the square counted. V15 now states AH2's rows in AH4's scope on AH3's day against the result's `N`, with the bounds written out; `search/filter-tokens` T3's row and `browser/entry-points` E3's round 8 line follow it. The plan's open point §7.3 recommended this.

## Review

Reviewer: an independent Claude (Opus) subagent, It was given the diff, the plan's §8, the format rule, and the V15 question with both readings. It also read the plan's §7.3, and based its V15 view on the earlier versions of AH1 and on the click rule of `stats/heatmap-addon` A9. Read-only; it read no `oracle/`, `refs/`, `local-data/` or Anki source.

First review: nothing blocking. Should fix, fixed: this log was cited before it existed; the T19.5 row's attribution moved out of the cell into a paragraph after the resume-fix one, as the file records coordinator decisions. Nits, fixed: N25's "approved" twice (now "In force: approved …", as N23 and N24); the sign-in row named; the running job's case stated in the condition; the row's test file named. Its judgement on V15: a plain inconsistency, not a question for Antoine (no decision picks B4 for a click, the built heatmap counts by B3, and the add-on heatmap's own click uses Anki's days); fix V15, T3's row and E3 together, with AH2's rows and AH4's scope.

Re-review: nothing blocking; one should fix, fixed: the T19.5 sign-in clause read as a first-match exclusion, which would fall through to "setup required"; it is now an override of this row's text and action, as built. Nits taken.

CodeRabbit: three threads, fixed. The decisions row quotes Antoine's permission of 2026-11-20. V15 says what a collection-scope day whose rows all belong to deleted cards does (it stays a link and opens nothing). The T19.5 row shows acceptance unknown before Upload…, as MP7 step 5 says. As built, a round's acceptance unknown with an upload pending shows Upload… (`SyncKinds.meguriRow` excepts only a refused sign-in); for the coordinator to check whether that state can occur.

Left for the plan owner: the plan's §0 gate, §7.4 and §8 still say "awaiting approval".

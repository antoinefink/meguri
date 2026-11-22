# Round 11 Statistics: spec and plan reviews (2026-11-22)

Scope: `specs/stats/statistics-page.md`, `specs/stats/history-reads.md`, the round 11 lines in `stats/graph-presentation` (P24), `graphs-common`, `graphs-revlog`, `graphs-cards`, `browse-queries` (Q9), `studied-today`, `activity-heatmap`, `pdf-export` and `search/filter-tokens`; `docs/design/native-interface-contract.md` N25; and the plan `docs/plans/2026-11-22-1200-feat-statistics-page-implementation-plan.md`. Written for Antoine's round 11 decision (`decisions.md`, the 2026-11-22 `stats.overview` rows) and the coordinator's loading decision of the same day. PR #113.

Reviewers: two independent Claude (Opus) subagents, one for the specs and one for the plan, each given the files and the decision, not the author's conclusions. Read-only; neither read `oracle/`, `refs/`, `local-data/` or Anki source.

## Spec review: 3 blocking, 21 should fix, 15 nits

Blocking, all fixed:

- Today's point of Knowledge Over Time was C9.4's stored-state sum over other cards, another formula and another decay, so the line could drop on its last day. Now the same replay at now over the same cards; the difference from Retrievability's Estimated total knowledge is stated (H3.1, S7.1, S7.12), and the no-estimate case has its line.
- A sync after a rollover brings rows older than today: the deltas and Started now go on the rows' own days and the card's samples after its earliest new row are recomputed (H14.3).
- Rollover and days away: every missing day-end and month-end sample is added, replaying the cards with later rows (H14.3).

Should fix, fixed: the day-end cut by whole second (H1, H3.1); a card with no row is in its current state; buried cards stay in Cards Over Time and Knowledge by state (only suspended cards are left out, with a Card Counts extra `suspended_by_state` so the last day still equals Card Counts, H2, H8); the preset cutoff applies only from its date and "studied" counts cards with an estimate (H3.1); the Card Info claim limited to kept rows and the I3 rule 1 case; the strip's studied numbers read stored values as T12 (H12); every chart folds rows after today into today (S4.1); the strip's tier-2 figures (S10.1); the two-refresh All sequence (S10.3); unreadable and scope checks before the first part, and unreadable rows outside the window failing only the all-history parts (H15); the derived-store details (no catch-up, a `deferred until` hold released by `request_refresh`, captures reading rows by `cid`, the phase in a record row, D7's stop conditions, H14); replays for card add, remove and state changes; the day read on a new snapshot (H13); Reviews Due's All capped at 24 months with a Later bar (S4.2); All's first month for every chart (S4.1); Knowledge re-bins in Swift (S3.2, S19); R1838's below-fold clause superseded (S18); the filter-tokens string row's marker; N25.7's sentence; "An open point".

Nits: taken into H3, H3.4, H9 to H11, H14.3, Q5, Q9, S3.1, S7, S10 and N25.6. Kept as is: R1357 in the Status lines (precedent of R1232 for round 8).

## Plan review: 6 blocking, about 30 should fix, 6 nits

Blocking, all fixed: the gate against the first three (U98 split into its in-force part, now, and U293, the round 11 request shape after approval; the first three are U98, then U99 and U101, all in force); U289's dependency on U98's derived-store items (D4's 2026-11-17 items are not built; U98 builds them) and its Card Counts test (direct SQL counts; U104 adds the check against U101); U290 no longer needs C9.4 (today's point is replayed); section units depend on the FFI units; U304 then U305 in sequence; the catalog (R1838's below-fold case removed with its supersession note, R1837 and R1838's intervals moved to the new names by U307, `StatsPerfTests` created by U296).

Should fix, fixed: the model and view split and each overlap it named; owners for S2, S15, S16, N25.9, N25.12, Q1 to Q9, P4.5, P10, P12, P18, P19 and the H9, H15 to H17 tests; U104 owns the graph observation and the scenario activation; U103's parent items (`stats_home_idle` stays, `setStatsPreference` dropped, heatmap settings and the widget left to their units); U291 needs U99; U303 waits for U271; U298 extends HeatmapKit; U307 reorders the sidebar and tabs; one test file per unit; one extension point per section; at most two Swift units at once; real `tools/mg` jobs (`snapshots`, `ui-test` classes, `gallery --components`, `hierarchy-check` with a fold check U305 adds); the `StatsRollout` flag and test routes; milestones.yaml for the plan owner; G9.2's idle build of `stats_columns` stated beside "no idle-time precomputation"; "What already exists" corrected (no Swift Charts yet, U271 not started, the testkit's `card_stats` executor, no registered derived component, the heatmap's 53-week window).

Nits: U293 now used; new requirement ids from R2067; benches in `meguri-testkit/benches`; PR #111 still open; the skipped Calendar scenarios recorded by U99; U302 after every section unit.

Decision: the spec lines stay `Under review (R1357)` until the coordinator approves them under Antoine's permission of 2026-11-20 to update specs.

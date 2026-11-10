# Batch B7-stats-graphs-cardinfo: U92, U99, U101, U103, U104

Implementer-view brief. Facts only, each with its source. Base read: `5cae7aef` (HEAD = origin/master; the plan is `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md`, cited as "plan"). `cf03979b` ("Correct six unimplemented-unit specs after an adversarial audit") committed `specs/stats/card-info.md`, `graphs-cards.md`, `graphs-revlog.md` and `studied-today.md`; every text cited from them below is on master. "WT" marks text that exists only as an uncommitted revision in this worktree and is not on master until committed: `specs/conformance/stats-ops.md` (H4, H11), `specs/stats/heatmap-addon.md` (A16) and `conformance/schema/*.json` (the `heatmap_totals` and `heatmap_report` shapes).

## 1. Batch

| Unit | Title (plan) | Milestone (`docs/process/milestones.yaml`) |
|---|---|---|
| U92 | Card Info and Previous Card Info data (plan l.7975) | M1 |
| U99 | Review-log graphs: Calendar, Reviews, Retention, Hourly, Answer Buttons (l.8234) | M1 |
| U101 | Card-based graphs: Future Due, Card Counts, Intervals, Stability, Difficulty, Retrievability, Added (l.8331) | M1 |
| U103 | Stats FFI surface and live-refresh events (l.8385) | M1 |
| U104 | Stats conformance matrix and performance gates (l.8433) | M3 |

- **Why grouped.** All five are `meguri-stats` consumers of the same `stats` and `card_stats` observation kinds (`specs/conformance/stats-ops.md` H9, H10). U103 exports U92/U99/U101 to Swift. U104 gates all of them.
- **Milestone boundary.** The five units cross one. U92, U99, U101 and U103 are M1 (`milestones.yaml` l.381); U104 is M3 (l.603). `docs/process/tiers.md` "Current verification cadence": "Use milestone order and dependency order within each milestone", and a full gate runs "at every milestone boundary". So U104 cannot share the M1 batch or its gate. It starts only after the M1 boundary gate, in M3's order.
- **Suggested order.** M1 batch: U92, then U99 and U101 (independent once U98 lands), then U103. U103 depends on U92, U99 and U101. Later, in M3: U104, which depends on U103 (plan Dependencies).
- **Priority.** `docs/process/review-first-delivery.md` (2026-10-29): reviews and iCloud sync first (U113, U114), then editing, search and statistics. This batch is last in that order. The milestone gates are still required.
- **Gate.** One full gate for the M1 batch (U92, U99, U101, U103), `tools/mg gate --base <batch-start-commit>`, and the M1 milestone gate at that boundary. U104 gets its own gate in an M3 batch. Each unit keeps its own focused checks, review log and commit (`docs/process/tiers.md` "Current verification cadence"; `AGENTS.md`).

## 2. Dependency status

"Merged" means the unit has `docs/process/review-log/<unit>.md` and a commit on HEAD with a role other than spec-writer or oracle-maintainer (implementer, fixer or integrator) carries its `Unit:` trailer. A `Unit:` trailer alone is not enough: spec-writer commits carry them too (`10913245`, Role: spec-writer, carries `Unit: U51`, and U51 has no review log and no implementation commit).

| Unit | Merged deps | NOT merged (blockers) |
|---|---|---|
| U92 | U3, U4, U23, U28, U29, U40, U46 | **U85** (needs U81, U82, U84: none merged; only U85a is merged), **U95**, **U97** |
| U99 | none | **U98**, **U97** |
| U101 | U45, U46, U82a | **U98**, **U82**, **U47** |
| U103 | U6, U24 | **U92, U95, U96, U97, U98, U99, U100, U101** |
| U104 | U4, U11, U12 | **U95, U96, U97, U98, U99, U100, U101, U103** |

Upstream of those blockers (plan Dependencies lines):
- U98: needs U95 and U82, which are not merged. U80, U23, U24, U26, U40, U3 and U4 are merged.
- U97: needs U95, which is not merged. U80 and U40 are merged.
- U96: needs U95 and U88, which are not merged.
- U84: needs U82, which is not merged. U80, U83, U85a, U23, U28, U29, U60 and U20 are merged. U85 needs U81, U82 and U84.
- U47's core deps (U46, U42, U43, U49, U80, U24, U26, U12) are all merged. Its app-side leg needs U51, U113, U114, U151, U154, U160 and U163, which are not merged.
- U82: needs U81, which is not merged. U80, U82a, U23, U28, U29, U30 and U40 are merged.

Root blockers. Three units on the critical path can start now; every listed dep of each is merged:
- **U95** (U3, U4, U5, U11, U22–U26, U38, U40) → U97, U98, U96.
- **U81** (U80, U20, U22, U29, U61, U82a) → U82 → U98 and U101; U82 → U84 → U85 → U92.
- **U88** (U80, U83, U28, U29, U30) → U96 (with U95) → U103 and U104.

**Result: none of the five units can start now.** U95, U81 and U88 must land first, then U97, U98, U82 and U96; U101 also needs U47; U92 also needs U85 (via U84).

The Meguri side does not exist yet:
- `core/crates/meguri-stats/src/lib.rs` holds only a doc comment.
- There is no `meguri-ffi/src/stats.rs` and no `CoreBridge/StatsBridge.swift`.
- `core/crates/meguri-host/src/wiring/pending.rs` lists U95 and U100 as pending providers (`docs/process/review-log/U38.md`).

## 3. Per unit

Spec id form: `area/topic#id` (`docs/process/provenance.md`, `Spec:` row).

### U92 Card Info data

- **Spec:** `stats/card-info#C1`–`#C12` (C13 is the oracle-maintainer's; C14 lists the tests). Rules it reads:
  - `model/row-records#R4`, `#R5`, `#R10`; `model/card-data-json#C3`;
  - `sched/memory-state#M3`; `sched/fsrs-items#I5`, `#I1.3`; `sched/presets#V1`, `#V6`, `#V7`;
  - `search/card-extractors#X5`; `stats/studied-today#T5.2`; `engine/change-stream#S1`;
  - `conformance/stats-ops#H9`.
- **Requirements:** R1248–R1257, R1259–R1264, R1423–R1430, R1448 and R1841 (500 revlog rows in 100 ms). Source: `specs/stats/card-info.md` header.
- **Meguri design docs to write:** none. No plan Files line is marked "(Meguri)". The Meguri-design rules are already spec sections: C7.5 curve points and C8 dump shape.
- **Committed scenarios** (`conformance/scenarios/stats/card-info/`, 12, each with a golden in `conformance/goldens/stats/card-info/`): `ae46-no-state`, `cloze-type`, `custom-data`, `due-estimate-night`, `filtered-card`, `intraday-learning`, `mixed-history`, `new-card`, `no-lrt-elapsed`, `reset-position`, `review-card`, `times`.
  - **Active:** none. There is no `conformance/scenarios/stats/area.yaml`; an area without one has no active scenarios (`specs/conformance/dsl.md` D9.1). U92 creates it and lists the 12 as active in the change that lands the `card_stats` adapter (`card-info.md` C13.2; `conformance/AGENTS.md`).
  - **Deviations:** `card-info-no-lrt-backfill` and `card-info-computed-retrievability` are on master (`conformance/deviations.yaml` l.1293–1306).
- **Not in tree:**
  - `damaged`: run errors, so no golden is possible (C14.1). U92 covers it with unit tests.
  - C14.3 pending coverage: `corpus/*.json` (50 cards × the R1899 zones × 3 clocks), `params`, `no-item`, `data-no-keys`, extra cards for `times` and `no-lrt-elapsed`, and more `damaged` cases. The oracle-maintainer adds these.
- **Plan test → scenario:**

  | Plan test scenario | Covered by |
  |---|---|
  | AE46: no lrt and no state → computed state and R; card row byte-identical; `col.mod` and WAL unchanged | `ae46-no-state`: raw values, card before/after, `SELECT mod FROM col`. Byte-identical row and WAL frames: Meguri-only (C14.2) |
  | New card: Position, no Due/S/D/R; reset card keeps its position | `new-card`; `reset-position` |
  | Filtered: Due from odue; Deck "(home)" | `filtered-card` (raw). The text `<deck> (<home>)` is Meguri-only (C4.3) |
  | Intraday learning: Due is the exact time | `intraday-learning` (raw `due_date` = stored due). The row text is a date (C4.1 Due-row note, decided 2026-11-10) |
  | Stability 45 d text; Difficulty 5.5 → 50% | Meguri-only (C4.2, C4.3 tables) |
  | Total time 0: no Avg/Total rows; Custom Data `k=v` | `times`, `custom-data` (raw). Row texts are Meguri-only (C4.1, C5.5) |
  | Mixed history `250%` / `D:xx%` | `mixed-history` (raw `ease`). Text is Meguri-only (C6.4) |
  | Manual/Rescheduled/cram types; curve skips them and resets; C7.3 ranges | `mixed-history`, `reset-position` (`review_kind`, per-entry states). Curve and ranges: Meguri-only (C7.2, C7.3, C7.5) |
  | 19 params → decay 0.5; empty → 0.1542 | Meguri-only (C7.4). Proposed scenario `params` (C14.3) |
  | Raw values at 3 clocks, 50 generated cards, TZ matrix | Pending C14.3 corpus (not in tree) |
  | Debug JSON round-trip | Meguri-only, with exact bytes (C8) |
  | Verification: no-write across 1,000 calls | Meguri-only (`core/crates/meguri-stats/tests/card_info.rs`) |

### U99 Review-log graphs

- **Spec:** `stats/graphs-revlog#V1`–`#V9`; `stats/graphs-common#G1`–`#G8`, `#G14`; `stats/browse-queries#Q4.2`; `stats/day-buckets#B3`; `conformance/stats-ops#H10`.
- **Requirements:** R1271, R1277, R1367, R1379–R1389, R1405–R1408, R1410–R1414, R1446 (`graphs-revlog.md` header).
- **Meguri design docs to write:** none.
- **Committed scenarios with goldens** (all inactive; no stats `area.yaml`):
  - `stats/graphs/reviews/{kinds-split, day-edges, window-start}`
  - `calendar/{ae58-paris-spring-forward, history-years}`
  - `retention/{row-filter, periods, ae59-history-parts}`
  - `hourly/{row-kinds, current-offset, kathmandu-edges, near-epoch, range-edges}`
  - `buttons/{groups, scope-parts}`, `buttons/tz-matrix/{kathmandu, lord-howe-dst, other-creation-zone, st-johns}`
  - U98-owned scope scenarios that also observe U99 parts (G15): `scope/{deleted-card-rows, history-window, moved-card, odd-row-values, unreadable-row, read-only, scope-follows-retag, filtered-deck-card}`
  - `stats/browse-queries/calendar-cells` (Q4.2; tags `needs-deck`, `needs-history`)
- **Proposed, not in tree:** `reviews/time-wrap` (V9, for deviation `stats-review-time-no-wrap`; there is no entry in `deviations.yaml` yet).
- **Held:** the heatmap side of AE58 (`heatmap_totals`) waits for the add-on image (`stats-ops.md` H4).
- **Plan test → scenario:**

  | Plan test scenario | Covered by |
  |---|---|
  | AE58 Calendar side | `calendar/ae58-paris-spring-forward` (day index). The Calendar date is a Meguri test of V3.2 |
  | AE59 | `retention/ae59-history-parts` |
  | Calendar back to 2019, never 2027; max count | `calendar/history-years` (count keys). `max_count` and the dates are Meguri tests. Year-navigation bounds are U226's (P7). The "2019" test is superseded by R1380's bound of 2000 (`graphs-revlog.md` intro, decided 2026-11-10) |
  | Reviews young/mature split, time sum; "29 of 30" | `reviews/kinds-split`. "Days studied" is U226's (`graphs-revlog.md` intro) |
  | Retention: cram excluded, type 0 long ivl, pass/fail, period edges ±1 ms | `retention/row-filter`, `retention/periods` |
  | Hourly +02:00 winter row → hour 10; Kathmandu edges | `hourly/current-offset`, `hourly/kathmandu-edges` |
  | Buttons: ease 5 skipped; learning-group percentages | `buttons/groups` (counts). Percentages are U226's |
  | Each graph in deck/collection/search across the TZ matrix | `buttons/tz-matrix/*` (Collection only) + `buttons/scope-parts` (Paris Deck/Search). Full coverage is pending (V10 note) |
  | Meguri-only (V10 list) | In `core/crates/meguri-stats/tests/graphs_revlog.rs` (V10): `calendar_shared_date_sum`, `calendar_dates_from_request_clock`, `review_time_sums_no_wrap`, `hourly_epoch_rows`, the V3.2 table rows 1–11, the V3.4 record tests, `last12Months` without all-time members |
  | G6 size test (20 years of daily rows ≤ 262,144 bytes; rows from 1970 with 4,000,000,000 ms times succeed) | `stats/graphs-common` G6 "Test (U99 with U103)". Not in V10's list; G6 names no test file |

### U101 Card graphs

- **Spec:** `stats/graphs-cards#C1`–`#C12`; `stats/graphs-common#G1`–`#G8`, `#G14`; `stats/browse-queries#Q4.1`; `search/card-extractors#X5` (the `fsrs` call only, C9.2); `conformance/stats-ops#H10`.
- **Requirements:** R1285, R1293, R1294, R1373, R1374, R1375, R1378, R1390–R1394, R1397–R1403, R1415, R1416, R1446 (`graphs-cards.md` header). The plan's U101 line differs: it has R1373–R1378 and R1390–R1404 (so also R1376, R1377, R1395, R1396, R1404) and not R1293.
- **Meguri design docs to write:** none. The Meguri extras are spec sections C3.6 (due tomorrow), C4.2 (notes count), C4.3 (label texts) and C6.2 (left out).
- **Committed scenarios with goldens** (inactive):
  - `stats/graphs/future-due/{ae57-buried-backlog, due-kinds, filtered-odue, types-and-queues, daily-load}`
  - `card-counts/{states, filtered-deck}`
  - `intervals/types-and-queues`
  - `fsrs/{stability-rounding, difficulty-bins, retrievability-elapsed, retrievability-sums, retrievability-now, fsrs-off}`
  - `added/day-edges`
  - U98-owned, observing U101 parts: `scope/{card-graphs-ignore-history, filtered-deck-card, subdeck-wildcards, read-only}`
  - `stats/browse-queries/{ae60-young, card-counts-labels, deck-scope-escaping, search-scope-or}` (tags `needs-deck`, `needs-state`, `needs-prop`, `needs-tag` → U82)
- **Proposed, not in tree (C13):** `fsrs/retrievability-decay`, `fsrs/retrievability-orphan`, `added/dst-edges`, `future-due/tz-matrix/*`, and more cards for `retrievability-elapsed`, `difficulty-bins`, `due-kinds` and `added/day-edges`.
- **Plan test → scenario:**

  | Plan test scenario | Covered by |
  |---|---|
  | AE57 buried/backlog | `future-due/ae57-buried-backlog` |
  | AE60 Young query | `browse-queries/ae60-young`, `card-counts-labels`, plus a Meguri test through U80 (C13 "AE60, core part") |
  | No lrt, 400 days: same R for both histories and `prop:r` | Meguri-only (C13 "Graph against `prop:r`", `advance` 430d) |
  | Intraday learning N+30 min → 0, +25 h → 1 | `future-due/due-kinds` |
  | Filtered card uses odue | `future-due/filtered-odue` |
  | Buried card by setting; 2-decimal percentages | `card-counts/states`. Percentages are U226's (held `presentation/card-counts-table`) |
  | D = 10 → bin 99 | `fsrs/difficulty-bins` |
  | R differs 1 h apart; decay 0.5 default; left-out count; 0 in note mean | `fsrs/retrievability-now` (uses 10 days), `retrievability-elapsed`, `retrievability-sums`. Left-out count is Meguri-only (C6.2) |
  | Added: 1 ms before N → day 0; "1 month" = −30…0 | 1 ms case: proposed `added/day-edges` cards (not in the committed scenario). The range is U226's |
  | TZ matrix, all scopes; deleted cards never appear | Proposed `future-due/tz-matrix/*`; C1 (existing cards only) |
  | Writes nothing | `scope/read-only` |
  | Meguri-only list (C13) | due tomorrow; notes count; label texts; left out; f32 sum order; `lrt` −2^63 with no panic; `due` −2^31; difficulty median from distinct-value counts; R recomputed after a clock change with warm caches |

### U103 Stats FFI and events

- **Spec:**
  - `stats/graphs-common#G6` (encoding: zigzag LEB128 runs, the 262,144-byte limit), `#G7`, `#G8`, `#G9.5`, `#G10`;
  - `stats/graphs-revlog#V3.4` (Calendar record, encoded by day number);
  - `stats/card-info#C10` (`card_info_affected`, refresh triggers);
  - `stats/studied-today#T7`; `stats/widget-snapshot#W5`, `#W6`;
  - `editor/config-keys#K5`, `#K6`; `engine/change-stream#S1`, `#S3` (stats events are `Extension(payload)`).
- **Requirements:** R1274, R1291, R1300, R1351, R1356, R1375, R1381, R1391, R1429 (plan).
- **Scenarios:** none. Tests are Meguri-only: `core/crates/meguri-ffi/tests/stats_ffi.rs` and `app/Packages/MeguriCore/Tests/CoreBridgeTests/StatsBridgeTests.swift` (plan Files). Every plan test row maps to these two.
- **Meguri design docs to write:** none.

### U104 Matrix and performance gates

- **Spec:**
  - `stats/graphs-common#G9.5` (≤ 40 MB at 1.2M rows), `#G9.6` (property tests vs cold compute);
  - `stats/widget-snapshot#W5` (≤ 10 ms build);
  - `stats/day-buckets#B5`, `#B7`; `stats/card-info#C14.3`;
  - `conformance/stats-ops#H4`; `conformance/dsl#D5`; `conformance/harvest-followups#H1` ("U104 owns the final Stats matrix").
- **Milestone:** M3. Not part of the M1 batch (section 1).
- **Requirements:** R1271, R1290, R1292, R1446–R1448; feeds R1837–R1840 and R1851 (plan).
- **Scenarios:**
  - `conformance/scenarios/stats/matrix.yaml` (plan Files) does not exist.
  - Prepared, held until the U95/U96 adapters land and the add-on image records goldens (untracked, not on master; none has a golden; `stats-ops.md` H4): `stats/day-buckets/*` (17), `heatmap-totals/*` (15), `heatmap/*` (16), `streaks/*` (11), `heatmap-config/writer-output`, and `widget-today-count/limdecks-vs-studied` (observes `heatmap_totals` and `studied_today_raw`; `stats/widget-families` F5, `counting-rules` N3.3, N6).
  - On master, T0 and `tools/mg goldens-check` report the heatmap-kind files `invalid` until the schema shapes land (H4). The shapes exist only in the WT `conformance/schema/observation.schema.json`.
  - Also held: `stats/presentation/*` (16) and `goldens/stats/presentation/preferences-read.json`. Of these, `preferences-read` needs `graph_preferences` (H13) and the rest need `stats_presentation` (H14, graphs-page image; owned by U98/U226).
- **Plan test → source:** AE58 matrix → `calendar/ae58-paris-spring-forward` plus the held `day-buckets/paris-spring-forward`. Kathmandu 03:59:59/04:00:00/04:00:01 → held `day-buckets/kathmandu-day-start` plus `today/day-start-kathmandu` (committed, U97). The fuzz (`core/crates/meguri-stats/tests/fuzz_aggregates.rs`) is not Meguri-only: plan U104 step 2 checks U95 resident state and U98 caches against a cold rebuild and against oracle observations at checkpoints. The private suite (`meguri-cli` `stats_private.rs`, T3, local Mac) compares heatmap totals and the four streak values with reference values recorded once from the add-on op (plan U104 step 3, R1447); they live in `local-data/goldens/stats/`.

## 4. Testkit and adapter work

The testkit model (`core/crates/meguri-testkit/src/scenario/`) already parses both kinds these units need:
- `card_stats` is `ObservationKind::CardStats` with `CardStatsRequest` (`prepared.rs`).
- `stats` is `ObservationKind::Stats` with `StatsRequest`, `StatsScope` and `StatsPart` (12 parts).
- Tolerances exist: `observe/mod.rs` sets 1e-5 for card_stats memory_state and R, and `STATS_TOLERANCE` 1e-6.

The **executor** registers neither kind. `exec/meguri.rs` l.479 returns `unsupported_observation`; there is no `registry.observation("card_stats"|"stats", …)` and no `observe/stats.rs`.

| Missing | Needed by | Defined in |
|---|---|---|
| `card_stats` executor adapter: C3/C6.1 raw values under the H9 names, typed C11 errors → run error | U92 | `stats-ops.md` H9; `card-info.md` C13.2 |
| `stats` executor adapter: G8 request mapped into the H10 part shapes, scalar-zero omission (V8), `{error}` per S3, `unknown_ref` for an unknown deck name | U99 (reviews, true_retention, hours, buttons), U101 (future_due, card_counts, intervals, stability, difficulty, retrievability, added), `context` (G14) | `stats-ops.md` H10; `graphs-common.md` G14; `graphs-revlog.md` V8; `graphs-cards.md` C12 |
| `conformance/scenarios/stats/area.yaml` (new) | U92, U99, U101 activation | `dsl.md` D9.1 |
| `graph_preferences` kind: in the schema (HEAD `observation.schema.json`), not in the testkit model | only the held `presentation/preferences-read`; not this batch | `stats-ops.md` H13 |

Ops and setup keys the committed card-info and graph scenarios use (`db_exec`, `answer`, `advance`, `forget`, `reopen`, `set_clock`, `set_config`, `add_note_tags`, `remove_note_tags`; setup `config`, `decks`, `presets`, `notetypes`, `notes`; observations `card`, `timing`, `db_dump`, `search`, `canonical_search`) are all registered (`exec/*.rs` `register`).

## 5. Open questions and decisions

| Item | Source | Blocking? |
|---|---|---|
| Card-info page and graphs page headless in a quarantined image (display-string goldens) | plan Open Questions l.3911; `graphs-common.md` G12.2 | No. Strings are tested from the spec tables (`card-info.md` C14.2) |
| Card Info strings shown verbatim in every locale (R1448 vs R191/R192) | plan Open Questions l.3934 | No (default) |
| `card-info-computed-retrievability` value 0.9710832: "U92 confirms the value, or adds an ignore transform" | `card-info.md` C13.4 | Yes, for activating `ae46-no-state` |
| `card_info_affected` takes the snapshot's dependencies, not `card_id` | `card-info.md` C10 (decided 2026-11-10) | No. Use the spec signature |
| Curve builder is Card Info only (no Stats consumer); shared time-span rule is unit thresholds only (not U45's formatter) | `card-info.md` C7.5, C5.2 (decided 2026-11-10) | No |
| Debug dump `stats` = raw values, not row texts | `card-info.md` C8 | No |
| Plan's U92 C7.3 example corrected (10-day history offers Week and Month) | plan U92 tests; `card-info.md` C7.3 | No |
| U99 split: year bounds, retention variants and "N/A", percentages and "Days studied" move to U226; "back to 2019" becomes 2000 | `graphs-revlog.md` intro (decided 2026-11-10); "plan's owner updates those lines" | No |
| R1271/R1384/U99 "only at DST edges" wording is too narrow; Product Contract correction needs Antoine's approval; V3.2 stands meanwhile | `graphs-revlog.md` V3.2 (raised 2026-11-10) | No |
| Calendar zone id must join G1's context and G8.3's token; graphs-common does not list it yet | `graphs-revlog.md` V3.2 "The zone" | No (the bullet is the rule) |
| `stats-review-time-no-wrap` deviation pending until the `reviews/time-wrap` scenario exists | `graphs-revlog.md` V9 | No, for U99's other scenarios |
| U104 `matrix.yaml` vs DSL: a scenario runs in exactly one zone; zone variants are separate scenarios or a generated corpus (precedent `scenarios/sched/day/matrix/*.json` from `corpora/sched`) | `dsl.md` D5; `conformance/AGENTS.md` (generated scenarios) | Yes, for U104's design |
| U104 private suite reads `local-data/`, which implementers may never read | `AGENTS.md` Roles; plan U104 step 3 | Yes, for U104 evidence: who runs T3 |
| `heatmap-addon` user step: master's H4 "Add-on copy" says `docs/process/one-time-setup.md` lists it, and that file does not | `stats-ops.md` H4 (master) | Pending in an uncommitted revision: the WT H4 "Add-on source" installs the add-on's public source v1.0.1 pinned by hash, "needs no copy from the user and no user step", and says the `MEGURI_HEATMAP_ADDON`/`heatmap-addon` step "was never set up". Once committed, the item is moot. U104 is M3 and later than this either way |
| Spec-ahead logs for stats | `docs/process/review-log/` holds only `spec-ahead-servers-open-questions-20261110.md` | None found |

## 6. Traps

- **Two day rules, never derived from each other.** Anki's day (B2, B3) applies to graphs, Today and searches. The heatmap day (B4) applies to the heatmap, streaks and widget. Card Info dates are plain local dates with no rollover shift (`specs/stats/AGENTS.md`).
- **Day-0 edges differ.** Graph day indices truncate toward zero. Added uses ⌈(c − N)/86,400⌉, so a card 1 ms before N is day 0 (`graphs-cards.md` C10). Future Due timestamp days truncate (C3.2).
- **Retrievability differs between the graph and `prop:r`.**
  - The graph elapsed rule is C9.1, not X5's. It differs from `prop:r` for the cards in C9.6.
  - Home due H = `odue` iff `odid` > 0 (C2), not the `odue ≠ 0` rule of `search/properties` P4.1.
  - R is never cached across requests (C9.5, R1294).
- **Card Info decay.**
  - For a computed state, the stored V6 decay is used unclamped: with 21 values ending 0.05, R uses 0.05 (C3.5 step 4).
  - The curve uses V6's decay for both stored and computed states (C7.4).
  - The `lrt` fallback reads the stored `type` integer: a type-259 row counts (C3.5 step 1).
- **Card data `cd` rule.** A `cd` that is not a string makes the whole data read as no keys (C1).
- **No writes.** Card Info never writes, though Anki backfills `lrt` (C13.3). Graph requests never write (G11). A dump comparison must observe `timing` first, because Anki writes `localOffset` (`stats-ops.md` H3).
- **Wrapping arithmetic.** `e` is `now − L` with wrapping i64, then the low 32 bits. It never traps (`card-info.md` C3.5; `graphs-cards.md` C9.1).
- **No ease row.** There is never an Ease row, and no FSRS-off branch (C4.1, C15).
- **Two number writers.**
  - The Home line rounds binary32 values and never groups digits.
  - Stats/Card Info text uses the shortest binary64 decimal with `,` grouping (T5.2).
  - C5.4 percents round the exact binary value: 1.005 → `1.00` (`card-info.md` C5.4).
  - Texts never carry U+2068/U+2069 (C12).
- **Graph `time` and Today.** Graph `time` reads 0 outside `u32` and sums in 64 bits (V9). The Today `time_ms` sum wraps at 32 bits on purpose (`studied-today.md` T3.3, decided 2026-10-27).
- **Browse join.** It is `(<scope>) AND <item>`, never written by the canonical writer or U88's `join` (`specs/stats/AGENTS.md`; `browse-queries.md` Q1). Suspended and Buried labels never get X appended (Q4.1).
- **Observation form.**
  - Use the protobuf field names (`newCards`, `including_inactive`) and decimal-string map keys.
  - U99 parts omit only scalar zeros. An empty `hours` entry is `{}` (V8). With `year`, the `all_time` members are left out.
  - f32 members are written with a fraction (26.0) (C12).
- **Scope semantics.**
  - Deleted-card rows count only in Collection scope (V1, G2).
  - A Deck scope matches `did` or `odid` in the subtree (G2.1).
  - Scenarios avoid the deck names `current`, `filtered` and `_*` (G14).
- **Search-scope scenarios need U82 terms.** These graph scenarios use search scopes but carry no `needs-*` tags: `buttons/scope-parts` (`deck:Lang`), `scope/deleted-card-rows` (`deck:*`), `scope/scope-follows-retag` (`tag:verbs`). They still need U82 terms (`search-ops.md` "needs-…" list).
- **Oracle panic case.** No oracle scenario may store a row with `t + offset ≤ −3,600`; Anki panics and the run dies (V10).
- **Filtered home deck.** On the oracle it is `worker_died`; Meguri returns `filtered_home_deck` (`stats-ops.md` H9).
- **Widget flush shape.** The record is `{changed, refreshedOnly, writeSeq, failed}` (`widget-snapshot.md` W6). The plan's U103 lists only three fields.
- **Graph preferences.** A write changes only the changed key, never all three (G13; `editor/config-keys` K6).
- **Goldens.** Never edit or bless a golden. Re-record with `tools/mg oracle record` (`conformance/AGENTS.md`).
- **Activation.** Mark scenarios active in the same change that lands the adapter (`conformance/AGENTS.md`).

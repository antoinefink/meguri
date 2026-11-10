# B6-stats-core: unit brief (implementer view)

Base read: `cf03979b` (origin/master = HEAD), 2026-11-10. `cf03979b` ("Correct six unimplemented-unit specs after an adversarial audit") put the revised `specs/stats/studied-today.md`, `graphs-revlog.md`, `graphs-cards.md` and `card-info.md` on master. It has no `Unit:` trailer. Implementer-view sources only: no `oracle/`, `refs/`, `local-data/`, add-on or Anki source was read. "Held" = present in the worktree, absent from `git ls-files`. "Uncommitted" = a tracked file modified in the worktree; cite the HEAD text.

## 1. Batch

- Units: U95 (day buckets, resident aggregates), U96 (heatmap grid, streaks, `heatmap` writer), U97 (studied-today line, Stats Today), U98 (graph engine: caches, scopes, aggregates, browse scope text), U100 (widget snapshot builder). Plan §U95–U100 (`docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md` l. 8029–8330).
- Why grouped: all are `meguri-stats` core units of milestone M1 (`docs/process/milestones.yaml` l. 381). U96, U97, U98 and U100 build on U95's day mapping, resident tier and observer registration. U100 consumes U95, U96 and U97 query APIs. U99, U101, U103 and U104 are excluded because they need U98 or FFI.
- Gate cadence: Rust only (`meguri-stats`, `meguri-testkit`, `meguri-host/src/wiring/`). No Swift or FFI work: the FFI is U103. One full gate at the batch end with the recorded batch base. Each unit keeps focused checks, independent review, its own commit and its review log (`docs/process/tiers.md`, "Current verification cadence").
- Suggested order: U95 → U97 → U96 → U100 → U98.
  - U95 and U97 have no unmerged dependency.
  - U96 needs U88 only for A9's deck-page search text.
  - U100 needs U96 and U97.
  - U98 needs U82 for the search scopes of its scenarios. It can move to a later batch.
- Priority: finish reviews and iCloud sync first. U113, U114 and their dependencies come before the remaining editing, search and statistics features (`docs/process/review-first-delivery.md`, "Next feature priority — 2026-10-27", delivery priority 2026-10-29). The source sets no order among editing, search and statistics. U113 and U114 have no `Unit:` trailer yet, so this batch ranks below them. The Anki-server freeze does not touch it.
- Milestone obligations:
  - The M1 gate requires `core/crates/meguri-host/src/wiring/pending.rs` to be empty (`docs/process/milestones.yaml`, M1 gate text). That file lists `ServicePort::Statistics` (provider U95) and `ServicePort::Widgets` (provider U100). Each unit removes its entry when its real wiring lands.
  - U95 must close U43's open item "Actual stats-cache/aggregate build concurrent with review; answer p95 <=5 ms" (`docs/process/review-log/U43.md` l. 73).

## 2. Dependency status

"Merged" means a `Unit: <id>` trailer exists in `git log` at `cf03979b`.

| Unit | Depends on (plan) | Merged | Not merged |
|---|---|---|---|
| U95 | U3, U4, U5, U11, U22, U23, U24, U25, U26, U38, U40 | all | none |
| U96 | U95, U23, U24, U28, U40, U88 | U23, U24, U28, U40 | **U95** (batch), **U88** |
| U97 | U95, U80, U40 | U80, U40 | **U95** (batch) |
| U98 | U95, U80, U82, U23, U24, U26, U40, U3, U4 | all but U95, U82 | **U95** (batch), **U82** (only U82a is merged) |
| U100 | U95, U96, U97, U24, U38, U40 | U24, U38, U40 | **U95, U96, U97** (batch) |

- U88 is needed only for `stats/heatmap-addon#A9`: deck-page cell searches are written by U88's builder in canonical form (`search/join-and-replace` J3). Home-scope terms (`prop:rated=-N`, `prop:due=N`) need no builder.
- U82 provides the `deck:`, `tag:`, `is:` and `prop:` terms. The scenarios that need them are: U98's `graphs/scope/deleted-card-rows` (`deck:*`), `scope-follows-retag` (`tag:verbs`) and `read-only` (`deck:Lang`), every `browse-queries/*` scenario tagged `needs-*`, and U97's `today/search-scopes` (tag `needs-deck`; `specs/conformance/stats-ops.md` H8).
- Existing seams:
  - `meguri_engine::ports::ChangeObserver` with `observe_with_rows` (`core/crates/meguri-engine/src/ports.rs:16`).
  - `CollectionReplaced`, `RolledOver`, `TimeZoneChanged` and `NextDayAtChanged` events (`core/crates/meguri-engine/src/changes/stream.rs`).
  - `Clock::zone_id()` (`core/crates/meguri-model/src/clock.rs:22`).
  - The pure D3/D4 day math `meguri_sched::timing::day::sched_day(&DayInputs)` (`core/crates/meguri-sched/src/timing/day.rs:43`), which U100's e(D) may call (`stats/widget-snapshot#W4`).
  - The App Group snapshot path, already a `HostConfig::new` parameter `app_group_snapshot` (`core/crates/meguri-host/src/config.rs:33`).
  - jiff 0.2.38 with `tzdb-bundle-always` is declared in `[workspace.dependencies]` (`core/Cargo.toml:108`). Only `meguri-testkit` uses it, so U95 adds `jiff.workspace = true` to `meguri-stats`.
  - `meguri-stats` is an empty crate: `Cargo.toml` and a 2-line `src/lib.rs`, no dependencies yet. No crate depends on it yet (it appears only in `core/Cargo.toml` l. 44).
  - `core/crates/meguri-host/src/wiring/` holds only `media.rs`, `mod.rs` and `pending.rs`. The `wiring/stats.rs` of the plan's U100 Files line does not exist yet.
- Layering (`core/tools/layering-check/edges.toml`): `meguri-stats` is row 8. It may depend on rows ≤ 7 (engine, sched, search, template, storage, model, text), not on `meguri-package` (sibling). `meguri-storage` (row 3) is "the only crate that issues SQL against collection.anki2".

## 3. Per unit

Spec-id form: `area/topic#id` (`docs/process/roles/implementer.md`, Commits). No unit has a "(Meguri)" design doc still to write: `specs/stats/widget-snapshot.md` (Meguri-authored, U100) is committed. No `conformance/scenarios/stats/area.yaml` exists, so no stats scenario is active. Mark scenarios active in the change that lands their adapter (`conformance/AGENTS.md`).

Hold reason for the heatmap scenarios: the commit message of `07e66d43` says "Heatmap and presentation observations still need their quarantined oracle images; their scenarios are held back". `specs/conformance/stats-ops.md` H4 adds that they have no goldens until the add-on copy is present, its hash matches the pin and the driver registers `heatmap_totals` and `heatmap_report`. The committed spec texts say "as of 2026-11-10 ... in the tree" (`day-buckets` B7, `counting-rules` N9, `heatmap-addon` A16, `stats-ops` H4). On master these files are held, not committed.

### U95

- Spec: `stats/day-buckets#B1`–`#B6`, `stats/counting-rules#N1`–`#N8`, `stats/heatmap-config#K1`, `#K3`, `#K4`, `conformance/stats-ops#H1`–`#H4`, `storage/derived-store#D4`–`#D8`, `engine/change-stream#S2`.
- Committed scenarios: none of its own. Committed scenarios that observe U95's `studied_today_raw` (all inactive, all with goldens): `stats/today/*` (6 `day-start-*`, `empty`, `odd-values`), `stats/studied-today/time-units`, `zero-line`, and `stats/widget/day-start-*` (4).
- Prepared, held until the add-on oracle image, schema shapes and goldens exist (H4). None has a golden.
  - `stats/day-buckets/` (17): `ease-zero-row`, `future-row`, `kathmandu-day-start`, `lord-howe-dst-end`, `lord-howe-dst-start`, `other-creation-zone`, `paris-fall-back`, `paris-spring-forward`, `rollover-23`, `rollover-change`, `rollover-shift`, `rollover-while-open`, `rollover-zero`, `st-johns-dst-start`, `st-johns-rollover-edge`, `travel-new-york`, `travel-paris`.
  - `stats/heatmap-totals/` (15): `ae53-deleted-card`, `ae6-manual-rows`, `answer-undo`, `answers-and-deletes`, `deck-names`, `deck-scope-move`, `filtered-deck-card`, `limcdel`, `limdecks-deleted-card`, `limdecks-rename`, `limdecks-subtree`, `partial-config`, `read-only`, `row-kinds`, `suspended-buried-rows`.
  - Expected values: `day-buckets#B7` and `counting-rules#N9` tables.
  - `stats/widget-today-count/limdecks-vs-studied` (spec `stats/widget-families#F5`, owned by U165; both files untracked): prepared, held until the add-on oracle image and goldens exist (stats-ops H4). It observes `heatmap_totals` and `studied_today_raw`, so it needs U95's adapter. Expected (Meguri, in its header): Home {2026-10-12: 5} and studied {cards 5, time_ms 10,000}; with `limdecks` [Lang], Home {2026-10-12: 2} and studied unchanged.
- Plan test → scenario:

| Plan test | Covered by |
|---|---|
| AE52 Set Due Date row, `limresched` true/false | `day-buckets/ease-zero-row` |
| AE5 02:30 / 05:00 | `day-buckets/rollover-shift` |
| AE58 Paris 03:30 CEST, both goldens in one scenario | `day-buckets/paris-spring-forward` (`heatmap_totals` + `timing`); Calendar side is U99's committed `graphs/calendar/ae58-paris-spring-forward` |
| Offset-instant cases (04:30 CEST, fall-back, Lord Howe) | `paris-spring-forward`, `paris-fall-back`, `lord-howe-dst-start`, `lord-howe-dst-end` |
| TZ matrix (Kathmandu, Lord Howe, St Johns, other creation zone) | `kathmandu-day-start`, `lord-howe-*`, `st-johns-dst-start`, `st-johns-rollover-edge`, `other-creation-zone` |
| Travel Paris → New York rebuild | `travel-paris` + `travel-new-york` (same instants, H3 "Travel"); the rebuild on `TimeZoneChanged` is Meguri-only |
| Row 3 h in the future | `day-buckets/future-row` (deviation `heatmap-future-rows-today`, held) |
| AE53 Home part | `heatmap-totals/ae53-deleted-card` |
| AE6 count part | `heatmap-totals/ae6-manual-rows` |
| Studied today excludes types 4/5, includes 3 | `heatmap-totals/row-kinds` |
| Incremental-equivalence property test (10k cases) | Meguri-only `tests/aggregates_incremental.rs`; must include a filtered-deck card (N7 item 2). Scenario support: `answer-undo`, `answers-and-deletes`, `limdecks-rename`, `deck-scope-move` |
| Persist day N, open day N+1 | Meguri-only (persist); related `day-buckets/rollover-while-open` |
| Kill after unapplied commit; tampered key; external move while closed | Meguri-only |
| Cold open 150k / 1.2M, no scan (query plan) | Meguri-only |
| `RolledOver` reset; `CollectionReplaced` rebuild | `rollover-while-open`; replacement Meguri-only |
| Read only (R1295) | `heatmap-totals/read-only` |

- Scenarios beyond the plan: `rollover-zero`, `rollover-23`, `rollover-change`, `partial-config`, `limcdel`, `limdecks-*`, `deck-scope-move`, `filtered-deck-card`, `suspended-buried-rows`, `deck-names`.
- Deviations to add with the goldens (held, H4): `heatmap-future-rows-today`, `heatmap-drawn-day-shift` (`lord-howe-dst-start`), `heatmap-deck-home-deck` (`filtered-deck-card`).

### U96

- Spec: `stats/heatmap-addon#A1`–`#A13`, `stats/heatmap-config#K2`, `#K6`, `conformance/stats-ops#H11`, `storage/config-table#C4`, `#C5`, `#C6`, `search/join-and-replace#J3` (A9, deck pages).
- Committed scenarios: none.
- Prepared, held (same reason as U95, H4/H11). No goldens.
  - `stats/heatmap/` (16): `forecast-cap`, `forecast-filtered`, `forecast-filtered-day0`, `forecast-limfcst`, `forecast-overdue-only`, `forecast-queues`, `forecast-scope`, `limdate-creation`, `limdate-fraction`, `limdate-unit`, `limhist-and-limdate`, `limhist-edge`, `limits-odd`, `no-history-forecast`, `today-cell`, `today-dst-shift`.
  - `stats/streaks/` (11): `ae5-yesterday`, `ae51-current-streak`, `deleted-card`, `empty`, `future-row`, `gaps`, `limdecks-home-only`, `limhist-span`, `lord-howe-dst-start`, `rounding-half`, `single-day`.
  - `stats/heatmap-config/writer-output`.
  - Expected values: `heatmap-addon#A16`.
- Plan test → scenario:

| Plan test | Covered by |
|---|---|
| AE55 levels | Meguri unit tests `levels-history`, `levels-forecast` (A16; no scenario file, H11) |
| AE51 Home part | `streaks/ae51-current-streak` |
| AE5 streak part | `streaks/ae5-yesterday` |
| AE54 today cell | `heatmap/today-cell` |
| AE6 deck part | `streaks/deleted-card` |
| Filtered card on home deck page; `limdecks` [Parent] | `heatmap/forecast-filtered`, `forecast-filtered-day0`, `forecast-scope`, `streaks/limdecks-home-only` (+ U95 `heatmap-totals/filtered-deck-card`) |
| Days learned 100 on one day; .5 rounding | `streaks/single-day`, `streaks/rounding-half` |
| Corrupt due 10⁹; `limfcst` 30 | `heatmap/forecast-cap`, `forecast-limfcst` |
| `limdate` = creation; `limhist` + `limdate` | `heatmap/limdate-creation`, `limhist-and-limdate` |
| Months / year periods; navigation bounds | Meguri-only `tests/heatmap_grid.rs` (A5, A6: "no scenario") |
| Empty collection | `streaks/empty` (+ `heatmap/no-history-forecast`) |
| AE56 writer value preservation; no key → no write | Meguri-only `tests/heatmap_config_roundtrip.rs`, one test per `heatmap-config#K6.3` row + K6.4 extras; add-on acceptance `heatmap-config/writer-output` |
| `limdecks` integers, `limdate` number type | K6.3 rows (unit) |
| — | tints: Meguri unit test `tints` (A16) |

- Scenarios beyond the plan: `forecast-queues`, `forecast-overdue-only`, `limhist-edge`, `limdate-unit`, `limdate-fraction`, `limits-odd`, `today-dst-shift`, `streaks/gaps`, `limhist-span`, `future-row`, `lord-howe-dst-start`.
- A9, A10 and A11 shapes and A2.2 saturation (`limhist` 2,147,483,647; `limdate` 253,402,300,800, `1e300`) are unit tests only (A16, last paragraph).

### U97

- Spec: `stats/studied-today#T2`–`#T7` (incl. `#T3.5`, `#T6.2`), `#T11`, `stats/counting-rules#N6`, `stats/graphs-common#G3`, `#G4`, `#G8.2`, `model/row-records#R10`, `conformance/stats-ops#H5`–`#H8`.
- Today query (HEAD `studied-today#T6.2`, `#T3.5`):
  - The Today section is the Today graph of the Stats graph request. Every scope, Collection included, reads the review log and the scope's cards from one snapshot. It never reads the resident values of `counting-rules#N7`; those hold only the Home line's numbers.
  - Decided 2026-11-10: U97's Today query takes a scope and a history (the G3 window). `today_stats` (H7) reads it with `last12Months`. U98 later serves it unchanged as the Today graph; only Context changed is absent from U97's results.
  - Results: Data; the scope error (a search error, or `unknown_deck`); `unreadable_row`; Cancelled.
  - Rows and cards are read with G4's tolerances (T3.5). A row or card that R10 calls unreadable gives `unreadable_row` and no values: an `ease` outside 0–255 or not an INTEGER, a `factor` outside u32, a `usn` outside i32, an `ivl` or `lastIvl` that is NULL or outside i32 after integer conversion, or an unreadable card row of the scope.
  - Only rows of the scope in the history window are read, so the history control decides whether an old bad row fails the section (`allHistory` fails on a bad row from 400 days ago, `last12Months` does not). The counted rows are the same for both. The Home line reads only `id`, `type` and `time`, so such a row still counts there (T4.5).
  - T11: Meguri's `unknown_deck` differs from Anki's `deck:current` → `Default` fallback. Test it in `core/crates/meguri-stats/src/today.rs`.
- Committed scenarios (all inactive, all with goldens at the current pin):
  - `stats/studied-today/` (4): `format-table`, `refresh`, `time-units`, `zero-line`.
  - `stats/today/` (13): `ae53-deleted-card`, `day-start-{chatham,kathmandu,los-angeles-dst,paris-dst,st-johns,utc}`, `deck-subtree`, `empty`, `lines-mix`, `no-mature`, `odd-values`, `search-scopes` (needs U82).
- Prepared/held: none. HEAD's T3.3, T9 and T10 call `today/time-wrap` "proposed and not yet written". No scenario file or golden exists.
- Plan test → scenario:

| Plan test | Covered by |
|---|---|
| AE53 (Home 50, Collection 50, deck 40) | `today/ae53-deleted-card` |
| No answers: zero line, "No cards have been studied today." | `studied-today/zero-line`, `today/empty` (text: unit test) |
| Time-unit boundaries, byte-exact | `studied-today/time-units`, `format-table` (T2.4's 36 lines) |
| Today lines mix / no mature | `today/lines-mix`, `today/no-mature` (values via `today_stats`; lines via `tests/studied_today_text.rs`, T6.4) |
| Types 4/5 excluded; row at S − 1 s / at S | `today/day-start-*` (6 zones), `today/empty`, `today/odd-values` |
| Stored `time` above limit used as stored | `today/odd-values`, `format-table` |
| Home line O(1), zero reader queries | Meguri-only |
| — | `studied-today/refresh` (T7); `today/deck-subtree`, `today/search-scopes` (T3.2) |

### U98

- Spec: `stats/graphs-common#G1`–`#G11`, `#G13`, `#G14`, `stats/browse-queries#Q1`, `#Q2`, `#Q4`, `#Q6`, `conformance/stats-ops#H10`, `#H13`, `storage/derived-store#D4`–`#D8`, `#D10`.
- Committed scenarios (inactive, with goldens).
  - `stats/graphs/scope/` (11): `card-graphs-ignore-history`, `deleted-card-rows`, `filtered-deck-card`, `history-window`, `invalid-search`, `moved-card`, `odd-row-values`, `read-only`, `scope-follows-retag`, `subdeck-wildcards`, `unreadable-row`.
  - `stats/browse-queries/` (7): `ae60-young`, `calendar-cells`, `card-counts-labels`, `deck-scope-escaping`, `parse-all`, `search-scope-or`, `special-deck-names`.
  - Parts used: G14 gives U98 the parts `reviews`, `added`, `card_counts` and `context`. Two scenarios also need parts from other units: `odd-row-values` needs U99's `buttons`, `hours` and `true_retention`, and `read-only` needs every part (U99 and U101). Neither can pass with U98 alone.
- Prepared, held: `stats/presentation/preferences-read` with its golden (G15, kind `graph_preferences` H13). It is held with the `presentation/` set under `07e66d43`'s hold.
- Plan test → scenario:

| Plan test | Covered by |
|---|---|
| Deleted rows in Collection only; moved card attributed to B | `graphs/scope/deleted-card-rows`, `moved-card` |
| Filtered-deck card in home and filtered deck | `graphs/scope/filtered-deck-card` |
| Invalid search returns error, memo untouched | `graphs/scope/invalid-search` + Meguri-only memo test (G8.3) |
| Window starts at `N − 366 d`, ±1 ms | `graphs/scope/history-window` |
| Cache sync property test | Meguri-only `tests/graphs_cache_sync.rs` (G9.6 op mix incl. `CloudRekey`, unreadable rows, warning with non-empty overlay) |
| Close/reopen stamp; Check Database rebuild | Meguri-only |
| No-`lrt` card, same value both histories | Meguri-only (G9.1) |
| Release and rebuild; U95 totals unaffected | Meguri-only (G9.5) |
| Aggregates schema, sums, 256 KB, Added range | Meguri-only `tests/aggregates.rs` (G6 encoding is U103's) |
| Background build after 2 s idle; persisted file without scan | Meguri-only (G9.2) |
| AE59 availability | G3 unit test; graph side is U99's committed `graphs/retention/ae59-history-parts` |
| Fixed-item queries parse and round-trip | `browse-queries/parse-all` + Meguri-only `tests/browse_queries.rs` (Q6, Q3 table byte-exact) |
| All history without scan on the request path | Meguri-only |
| Read only | `graphs/scope/read-only` (needs U99/U101 parts) |

- Meguri-only, listed in G15 and Q8: special deck names and `_*`; `did:` form and `unavailable`; context token and Context changed; one unreadable card failing a revlog-only request.

### U100

- Spec: `stats/widget-snapshot#W1`–`#W8`, `conformance/stats-ops#H12`, `sched/day-clock#D3`, `#D4` (e(D)).
- Committed scenarios (inactive, with goldens): the Anki-oracle e(D) checks `stats/widget/day-start-summer`, `day-start-spring-forward-r2`, `day-start-spring-forward-r4` and `day-start-fall-back`. They observe `timing`, `studied_today_raw` and `studied_today_text` only. They are not the "Meguri goldens" of the plan's Files line.
- Prepared, held: `stats/widget-today-count/limdecks-vs-studied` (§3 U95). It pins the Home count against studied today under `limdecks`, which the snapshot's today count copies (`widget-families#F5`, U165).
- Not written (W9): the snapshot scenarios `ae51-widget`, `no-history`, `year-end`, `gap-rollover`, `fall-back`, `limhist-slide`, `limdecks`, `week-start` and `no-content`. They wait for the `widget_snapshot` kind and for a way to record Meguri goldens, which `dsl.md` D11 forbids today (H12).
- Plan test → scenario:

| Plan test | Covered by |
|---|---|
| AE51 widget part | W9 `ae51-widget` (unwritten) + Meguri-only `tests/widget_snapshot.rs` |
| 50 answers → one rebuild; `flush` cancels debounce | Meguri-only |
| Unchanged → `changed=false`, same mtime; 13 h → `refreshedOnly` | Meguri-only |
| Deterministic bytes; atomic temp+rename | Meguri-only |
| `limdecks`, `heatmap`, `firstDayOfWeek`, deck delete, Set Due Date, zone change trigger rebuild | W9 `limdecks`, `week-start` (unwritten) + Meguri-only trigger tests |
| Year end; 32 KB; gap rule for `nextRolloverAt` | W9 `year-end`, `gap-rollover` (unwritten); e(D) side: committed `widget/day-start-spring-forward-r2`/`-r4`, `day-start-fall-back`, `day-start-summer` |
| No deck name, field or card id | W9 `no-content` (unwritten) + W8 golden scan |
| After `CollectionReplaced` only new values | Meguri-only |

## 4. Testkit and adapter work

The Meguri executor has no `stats` area: `MeguriExecutor::AREAS` in `core/crates/meguri-testkit/src/exec/meguri.rs:311` has 18 areas and none is `stats`. Adapters live in `exec/<area>.rs` and register through `register(&mut Registry)`. `src/observe/` holds only `mod.rs` and `overlay.rs`, although the plan names `src/observe/stats.rs`.

Observation kinds used by this batch's scenarios:

| Kind | Parsed by `scenario/model.rs` | In HEAD `conformance/schema/` | Meguri adapter | Defined in | Owner |
|---|---|---|---|---|---|
| `heatmap_totals` | no | no | no | `stats-ops#H1` | U95 |
| `studied_today_raw` | yes | yes | no | `#H2` | U95 |
| `studied_today_text` | yes | yes | no | `#H5` | U97 |
| `studied_today_format` | yes | yes | no | `#H6` | U97 |
| `today_stats` | yes | yes | no | `#H7` | U97 |
| `heatmap_report` | no | no | no | `#H11` | U96 |
| `stats` | yes (`StatsRequest`, `prepared.rs:200`) | yes | no | `#H10`, `graphs-common#G14` | U98 (parts `reviews`, `added`, `card_counts`, `context`) |
| `graph_preferences` | no | yes | no | `#H13` | U98 (G7) |
| `widget_snapshot` | no | no | no | `#H12` | U100 |
| `timing`, `db_dump`, `search`, `canonical_search` | yes | yes | yes | — | existing |

- Ops used: `set_config`, `db_exec`, `set_clock`, `advance`, `answer`, `delete_notes`, `undo`, `redo`, `rename_deck`, `reparent_decks`, `reopen`, `add_note_tags`, `remove_note_tags`. All are registered.
- Setup sections used: `config`, `decks`, `notetypes`, `notes`. All are registered.
- Top-level keys used: `clock`, `tags`.
- At HEAD, schema shapes for `heatmap_totals`, `heatmap_report` and `widget_snapshot` are missing. Held scenarios using them report `invalid` (H4). `conformance/AGENTS.md` asks for the spec, the schema, the testkit model and the oracle driver to change together. The `heatmap_*` driver ops belong to the oracle-maintainer (H4, H11). Implementers do not touch `oracle/`.
  - Prepared, held until the add-on oracle image and driver land: uncommitted shapes for `heatmap_totals` and `heatmap_report` in `conformance/schema/observation.schema.json` (defs `heatmap_days`, `heatmap_report`) and `scenario.schema.json` (def `heatmap_decks` and the request patterns). U95 and U96 must not write conflicting shapes. None exists for `widget_snapshot`.
  - Uncommitted revisions of `stats-ops#H4`/`#H11` and `heatmap-addon#A16` (prepared, not on master) say the add-on image installs the add-on's public source (no user step), its driver has both kinds and the shapes are in `conformance/schema/`. Goldens still wait for `tools/mg` to run these scenarios on that image.
- Adapter rules:
  - `heatmap_totals`, `heatmap_report` and `studied_today_raw` read the resident values. They wait while those values are `building` and fail the step after 60 s (`counting-rules#N7`, "Reads while the values are not ready").
  - The `studied_today_text` adapter (H5) waits the same way, with the same 60-second failure, so an observation right after `set_config rollover` or `set_clock` never reads `building` (`studied-today#T6.1`).
  - A separate revlog scan does not count as an implementation (H1, H11).
  - `studied_today_raw` and `studied_today_text` run no database query (H2, H5).
  - Deck names in requests are full names with `::`. An unknown name is `unknown_ref`.
- Activation needs per scenario: `stats-ops#H4` (U95/U96 lists) and `#H8` (U97).

## 5. Open questions and decisions

| # | Item | Source | Blocking |
|---|---|---|---|
| 1 | The add-on oracle image and the one-time user step `heatmap-addon` (`MEGURI_HEATMAP_ADDON`) are not set up. `docs/process/one-time-setup.md` and `tools/` do not mention them. An uncommitted `stats-ops#H4` revision (prepared, not on master) drops the user step: the image installs the add-on's public source. Goldens and deviation entries for every held heatmap scenario wait for them. | `stats-ops#H4` | Blocks U95/U96 scenario verification and activation. Does not block test-first work against the B7/N9/A16 tables. |
| 2 | Schema shapes and oracle driver ops for `heatmap_totals` and `heatmap_report` are missing. | `stats-ops#H4`, `#H11` | Blocks activation (oracle-maintainer). |
| 3 | No decided way to record Meguri goldens (`dsl.md` D11). W9 snapshot scenarios cannot exist yet. | `stats-ops#H12`, `widget-snapshot#W9` | Blocks U100's scenario verification ("goldens stable, shared with U165"). Unit tests proceed. |
| 4 | `storage/derived-store#D4` has no deferred-component registration (a refresh held until host signal `homeIdle` or a consumer's wait). The G9.2 text says "its owner adds it". It is still absent from D4. | `graphs-common#G9.2` | Blocks U98's build-start rule (spec update first). |
| 5 | U88 is unmerged. Deck-page cell searches need its builder in canonical form. | `heatmap-addon#A9` | Blocks completion of U96's A9 deck part. |
| 6 | U82 is unmerged. `deck:`, `tag:`, `is:` and `prop:` terms are missing. | §2 | Blocks U98 completion and U97's `today/search-scopes`. |
| 7 | B4.6 drawn-day shift is provisional. If a later web-view reading shows no shift, B4.6, H1 and `heatmap-drawn-day-shift` change together. | `day-buckets#B4.6`, `stats-ops#H1` | No |
| 8 | A15 items are provisional until goldens: `limdate` unit (item 1; K6.1 moves to local noon if the add-on reads a date), levels and tints maybe only in the web view (item 14), streaks across the drawn shift (13), filtered-deck forecast due (10). A disagreeing golden wins. | `heatmap-addon#A15` | No |
| 9 | `today/time-wrap` is "proposed and not yet written" in HEAD `studied-today#T3.3`, `#T9` and `#T10`. No scenario file or golden exists. | `specs/stats/studied-today.md` | No (T3.3 covered by unit test) |
| 10 | The revisions of `studied-today.md` (T3.5 unreadable rows fail the Today section; T3.2 deck-scope escaping and the `deck:current`/`Default` note; T6.2 query shape; T11 row), `graphs-revlog.md` and `graphs-cards.md` landed on master in `cf03979b`. U97's Today reading rules have changed to match (§3 U97). `git diff HEAD -- specs/stats/studied-today.md` is empty. | `cf03979b` | No |
| 11 | Week start: R149 (`firstDayOfWeek`), not R1302's Monday. Decided default. | plan Open Questions l. ~3932 | No |
| 12 | Zero studied-today line is `Studied 0 cards in 0 seconds today (0s/card)`. Decided 2026-11-10; R44's text is never shown. | `studied-today#T2.3` | No |
| 13 | Graph-preference writes go through U103's config op. U98 only reads them. | `graphs-common#G7` | No |
| 14 | No `spec-ahead-*` review log covers these units. The only stats mention is a U227 follow-up (`heatmap-presentation` M17). | `docs/process/review-log/spec-ahead-servers-open-questions-20261110.md` l. 303 | No |

## 6. Traps

Cross-unit:
- Two day rules, never derived from each other. Anki's day (B2/B3) is used for studied today, the Stats screen and searches. The heatmap day (B4.1) is the local date of (instant − R h), read with the offset at the moved instant, not at the review instant, and is used for heatmaps, streaks and the widget (`specs/stats/AGENTS.md`).
- Day-0 edges differ: studied today counts `id > S`, Stats Today counts `id ≥ S`, graph day indices truncate toward zero (`specs/stats/AGENTS.md`, B2, B3).
- The studied-today window has no upper end; never cap it at N. Heatmap rows after T count on T (B2 decided 2026-10-27, B4.3).
- There is no system clock or zone lookup. Use the bundled tzdb. An unknown zone falls back to the current fixed offset, logged once per open (B1, B5).
- The resident aggregates are never released, even on a memory warning (N7). Only U98's column caches are released (G9.5).
- `meguri-stats` never writes `collection.anki2`. Reading an absent `heatmap` key never creates it (N8, K3 rule 5). Read-only scenarios take their first dump after `timing`, because Anki writes `localOffset` (H3).

U95:
- N7 item 2 overrides the plan's key (day, home deck, rated, type). Counts must be addressable by current deck (`did`, for N3.3 on Home) and by home deck (N4 deck pages).
- N3.3 judges Home exclusion by the current deck (`cards.did`), even for filtered cards. A non-empty `limdecks` drops every deleted card's row, even when no listed id names a deck. Descendants are matched by name prefix + `::`, read at query time.
- Studied today (N6) counts every `type` except 4 and 5 (not "0–3": −1, 6 and 260 count on Home). Any `ease` counts, and time is summed as stored.
- The validity key also includes the bundled tzdb version, the fallback offset for unknown zones, and N for the studied-today values (N7 "Never stale", decided 2026-10-27).
- Not ready means `building`, never zeros or a partial map. Hosts re-ask on derived-store `Ready`/`DerivedApplied` (N7).
- Queries return `T` and `today_changes_at`. No event marks a move of T that differs from the move of N (B6, N7).
- Home `limdecks` totals are rebuilt when a deck rename, reparent or delete changes which decks an entry covers (R304; plan step 3).

U96:
- Forecast counts cards, history counts rows.
- Forecast eligibility: queue 2/3 only. A filtered card counts only when it would count at home (`type` 2, or 1/3 with D ≤ 10⁹). Membership is judged by `odid`, not `odue`. D = `odue` when not 0, else `due` (A3).
- Today's cell (A4): `future` with the forecast count until the first review, then `today`. Overdue cards never count today (A15 item 9).
- Levels use `a = max(20, rounded daily average)` over the whole filtered history. Threshold tests are exact, and every threshold is "at most" (A7).
- Rounding at .5 is to even (A8.1).
- The current streak holds through T = Y + 1 (A8.1).
- The writer (K6.2) has 5 steps in one transaction.
  - Drop unchanged patch members first. If none remain, write nothing.
  - A write starts from defaults when the stored value is not an object.
  - Every write adds missing K2 members with defaults, except `version`. This goes beyond R1338's "only changed fields".
  - Output is the compact form with sorted members. Integers stay integers.
  - The writer is not an undo step.
  - Invalid patches are refused with `invalid_heatmap_setting`.
- A9 (heatmap) uses U88's canonical builder. The Stats-screen browse link (`browse-queries`) never does. Do not share the two.
- Stats-screen code never reads the heatmap module (A12, R1343).

U97:
- Two number writers, never shared (`studied-today#T5`). The Home line uses binary32 and no grouping. Today lines use the shortest binary64 decimal with `,` grouping. `8855 ms` gives `8.85` on the Home line and `8.86` in Today.
- Today `time_ms` wraps mod 2³² and reads 0 for a `time` outside u32. The Home line sums as stored (T3.3).
- The kinds line omits `, Filtered: <d>` when d is 0. The core does this, not the UI (T4.2 item 3 overrides the plan's U97 note). Assert it in `tests/studied_today_text.rs` (T11).
- No U+2068/U+2069 and no line breaks in any line (T6.3).
- The Deck scope matches `did` or `odid` in the subtree, with deck names compared under simple case folding. Heatmap deck pages use the home deck only.
- An empty or White_Space-only search is the Collection scope (deleted rows included). `deck:*` is a search.
- An unknown Deck id gives `unknown_deck` (T3.2).

U98:
- `lrt` comes from card data only. The revlog is never read to fill it (G9.1, G12.3).
- The browse join is `(<scope>) AND <item>`, never canonical, never U88's `join` (`browse-queries` intro, supersedes the plan's approach 5).
  - The core writes only the scope text (`stats_browse_scope`). It falls back to `did:` lists for unmatchable names, `deck:"Current"`/`"Filtered"` for the decks named `current`/`filtered`, and `unavailable` for negative ids.
  - A test-only join may exist only in test code (Q6).
- G4 row reading:
  - A `type` whose low 8 bits are 0–5 reads as that kind. Anything else reads as Learn, and REAL is never truncated.
  - A `time` outside u32 reads 0.
  - A row that R10 marks unreadable fails the whole request (`unreadable_row`), but only when that row is in scope and window.
- History window: `id ≥ (N − 366×86,400)×1,000`. Negative ids are excluded from both histories (G3).
- No request reads or writes `curDeck` for a Deck scope (G2, R1353).
- The core takes `Search(text)` as given; the U+001C–U+001F replacement is U225's (G2).
- Context token and memo (G8.3):
  - Two requests per refresh share one token, and nothing is held open across FFI.
  - A different generation or seq gives Context changed.
  - The core keeps at most one memo.
- Overlay entries are versioned by seq. A release first merges a non-empty overlay (G9.3, G9.5).
- Column files are content only through a record table (G9.3, derived-store D1).

U100:
- The snapshot is compact JSON with sorted members, integers only, instants in ms (W1).
- `todayCell.count` is the review count, so it is 0 when `kind` is `future`. This differs from the heatmap query's A4, where count is the forecast count (W3).
- The W6 report is `{changed, refreshedOnly, writeSeq, failed}`. The plan omits `failed`. With no path passed at open, the core writes nothing.
- Triggers also include `NextDayAtChanged`, `firstDayOfWeek`, deck renames and moves, and external file changes (W5). The plan lists fewer.
- The rollover is a one-shot deadline at `nextRolloverAt` and a `SuspendParticipant`. It never polls (W5).
- e(D) comes from pure D3/D4 math. Never use the day clock's cached `today` or `nextDayAt`, and never read `localOffset` (W4).
- `upcoming` entries keep the build's history window and `a`, and `no-history` uses W3's short member set (W3, W4).
- Size ≤ 32,768 bytes. Never include deck names or ids, card, note or revlog ids, or due counts (W7, W8).
- The core never creates the App Group directory and never resolves containers (W1, W6 item 5).

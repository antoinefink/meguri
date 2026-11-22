# Statistics page: implementation plan

Date: 2026-11-22. Author: spec-writer session `s-20261122T080130Z-a8d3941b` (branch `unit/stats-contract-specs`, PR #113).
Decisions: `docs/design/decisions.md`, the four 2026-11-22 rows `stats.overview` (round 11: 1A 2A 3A 4A 5A 6A 7A 8A 9A; All as the first period; Forgotten orange; tiered loading with kept results and per-box progress).
Specs: `stats/statistics-page` (behaviour), `stats/history-reads` (the new core reads), the "Changed 2026-11-22 (round 11)" lines in `stats/graph-presentation` (P24), `stats/graphs-common`, `stats/graphs-revlog`, `stats/graphs-cards`, `stats/browse-queries` (Q9), `stats/studied-today`, `stats/activity-heatmap`, `stats/pdf-export` and `search/filter-tokens`; layout `docs/design/native-interface-contract.md` N25; screen contract `stats.overview`.
Parent plan: `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md`. Its units U98, U99, U101, U103 and U104 are reused below with their specs in force; U225, U226, U230, U231 and U236 are superseded by this plan's Swift units (§5).

**Gate.** Every new spec line is `Under review (R1357)`. No unit implements a line under review until the coordinator approves it, under Antoine's permission of 2026-11-20 to update specs, after the independent reviews (`docs/process/review-log/statistics-round11-20261122.md`). The parent units' sections that are in force (G1 to G11, V1 to V7, C1 to C11) can start now.

## 1. What already exists (master, 2026-11-22)

From a read of `core/` and `app/` (no code changed by this plan):

- **Core.** `meguri-stats` holds Card Info only (`card_info/`, `text/`; U92, PR #82). The graph engine is specified (`stats/graphs-*`) and not built: no `stats_columns` component, no graph request, no records, no `stats` testkit adapter. Built Meguri reads: the activity heatmap (`meguri-engine/src/activity.rs`, AH5, with FFI) and Today's numbers (`studied.rs`, T12, with FFI). The derived store and its host exist (`meguri-storage/src/derived`, `meguri-engine/src/derived_host.rs`), with change-stream application.
- **FSRS.** Memory items and per-entry states for one card (Card Info, `sched/fsrs-items` I3 to I5, C6.3) are built; nothing replays every card.
- **App.** `Features/Stats/Stats.swift` is a one-line placeholder; the section is not in `availableSections`. The heatmap grid (HeatmapKit's `ActivityGrid`) is built with N24.6's squares and levels. Card Info's forgetting curve uses Swift Charts (U274). Browse's shell (U270) has landed; its table (U270a, PR #106) and search field (U271) are in progress.
- **Recent defect class.** Sections opened before a collection existed (first run) and failed (PR #111, "Note Types and Browse wait for a collection"). Every screen unit below checks first run and no collection explicitly.

## 2. Conventions for every unit

- One implementer, one PR against `master`, opened early and ready for review; its own commits with the trailers of `docs/process/provenance.md` (`Role:`, `Spec:` with the spec ids it implements, `Session:`), the privacy and similarity checks, an independent review.
- Implementers build from the specs named here, never from the round 11 HTML page; layout is N25, then the native build is looked at (`docs/design/workflow.md`).
- Core integration tests go through each crate's `tests/main.rs` (one `mod` line per new file).
- Verify with the smallest `tools/mg` jobs: core `tools/mg core-test -- <filter>` and `tools/mg core-lint`; benchmarks `tools/mg bench --sizes 150k --bench <target>` (and `bench-mac` for the Mac numbers); Swift models `tools/mg swift-test --package MeguriKit -- <filter>` (CoreBridge: `--package MeguriCore`); app `tools/mg build-app --platform mac` and `--platform ios-sim`; screenshots, UI and accessibility only in the macOS test VM (`tools/mg screenshot`, `tools/mg mac-ui`, `tools/mg a11y`), one VM job at a time; UI performance `tools/mg perf-ui --suite StatsPerfTests`; design docs `tools/mg design-docs`. Hosted CI runs the Linux suites.
- **Required check for every screen unit (U296 to U307):** the first-run state (no collection on the device: the welcome screen, then Statistics reached from the sidebar or tab) and the no-collection states of `stats/statistics-page` S12.1, light and dark, before any other state. A unit that does not show both in its VM captures is not done.
- At most three implementers at once. Core units run in Docker and can run three wide; Swift units that need the VM queue behind each other for their UI checks.

## 3. Speed (the 150k fixture: 150,000 cards, 1,200,000 review-log rows)

Antoine decided on 2026-11-22 that this page may take longer than others ("it's understood for users that this page can take a bit longer to generate"), as long as it is performant. The figures below are targets to measure, and no unit changes the design to meet one: the tiers, the kept results and the progress shown in each box (`stats/statistics-page` S10) keep the page readable while a read runs. The rows come from `stats/history-reads` H19 (core) and `stats/statistics-page` S19 (app). Every figure is an estimate until the owning unit measures it and records the result in its PR and in the catalog.

| Read or interaction | Target | Owner | Measured by |
|---|---|---|---|
| Present tier (strip, Card Counts, Reviews Due, Memory boxes) | well under 100 ms | U98, U101, U288 | `bench stats_present`; `stats.presentTier` |
| Review-log tier, 1M to 1Y, caches current | under 300 ms core, 500 ms to the last box (R1837, R1838) | U98, U99, U288, U291 | `bench stats_log_year`; `stats.logTier` |
| Review-log tier, All | under 1 s core | the same | `bench stats_log_all` |
| History parts warm (Cards Over Time, Started, Knowledge) | under 50 ms | U289, U290 | `bench stats_history_warm` |
| Cold history build (one pass over 1.2 million rows) | a few seconds on an iPhone; target 2 s (under 1 s on the Mac) | U289 | `bench stats_history_build`; device run |
| Cold knowledge replay (FSRS for every card, about 400 samples) | a few seconds on an iPhone; target 4 s, parallel | U290 | `bench stats_knowledge_build`; device run |
| Incremental update: an answer or undo (≤ 10 cards); a sync of 1,000 rows | under 5 ms; under 100 ms | U289, U290 | `bench stats_history_apply` |
| Cards behind a day (Show in Browse) | under 50 ms | U288 | `bench stats_day_cards` |
| Cancellation of a superseded request | under 50 ms | U98, U289 | core tests |
| Page with a kept result | every box filled in the first frame | U294, U296 | `stats.keptOpen` |
| Scrolling and controls during any read | no hitch above 5 ms/s; a control responds in one frame | U296, U304 | `stats.scrollDuringRead` |
| Deck or period change, caches current | under 300 ms to the last tier-2 box (R1838) | U294 | `stats.scopeChange` |
| `stats_history` memory | under 32 MB on disk and resident | U289, U290 | bench peak memory |

How they are met: one request per refresh with parts returned by tier (H15); counts per day or bar, never rows (G6, S8.6); G9's mapped column caches for the review-log parts; the `stats_history` component kept per deck key, so a deck filter is a sum and a sync reads only its new rows (H14); every read off the main thread, cancellable (H16); the page's kept results on the device (S10.2). No idle-time precomputation is required (S10.6); starting the builds once Today is idle is an optional later optimisation.

The catalog (`tools/perf/budgets.yaml`) holds one entry per Product Contract budget id. The new rows need ids: §7, point 1.

## 4. Units in dependency order

Ids U98, U99, U101, U103 and U104 are the parent plan's. U288 onwards are new. "Core" runs in Docker, "Swift" on the host with VM checks. ∥ marks what can run beside the previous units of its lane.

### Lane A: core reads

| Unit | Scope | Spec sections | Side | Verify | Must not touch |
|---|---|---|---|---|---|
| **U98** | The graph engine: request context, scopes (Collection, Deck by id), history window, row reading with its tolerances, records and their encoding, the `stats_columns` component (build, mapping, overlay, updates, release), staleness, read only; the round 11 request shape: one request per refresh whose parts return by tier with progress (H15), cancellation (H16), the Search scope deferred (H1) | `stats/graphs-common` G1–G11 (in force), its round 11 lines; `stats/history-reads` H5, H15, H16, H17 | core | `core-test graphs_common`; G9.6's property test; `bench stats_present` | Swift |
| **U99** | Review-log records: Reviews, Retention, Hourly, Answer Buttons (the Calendar record is not needed by the page; build it only if U99 finds it free) | `stats/graphs-revlog` V1, V2, V4–V7 (in force) | core | `core-test graphs_revlog`; activate `stats/graphs/{reviews,retention,hourly,buttons}/*` once the `stats` adapter lands (V8) | history parts |
| **U101** ∥ | Card records: Future Due, Card Counts, Intervals, Stability, Difficulty, Retrievability, Added | `stats/graphs-cards` C1–C11 (in force) | core | `core-test graphs_cards`; activate `stats/graphs/{future-due,card-counts,intervals,fsrs,added}/*` | |
| **U288** | Round 11 parts computed per request: `retention_by_day` (H4), the extras By Answer (H6), `six_months` and `retention_by_hour` (H7), `future_due_young` (H8), `oldest_day` (H9), `activity_days` (H11), scoped `studied_today` (H12), `stats_day_cards` (H13) | `stats/history-reads` H1, H4, H6–H9, H11–H13; the round 11 lines of `graphs-revlog` V2.2, V4.3, V5.3, V6.2 and `graphs-cards` C3.6, C11 | core | `core-test history_reads` (H20's H4, H6–H8, H11–H13 tests); `bench stats_log_year`, `stats_day_cards` | Anki members of the records |
| **U289** ∥ | The `stats_history` derived component and the state replay: Cards Over Time and Started per deck key, built on first need with progress, incremental updates for every change of H14.3, phase and generation invalidation, persist and release | `stats/history-reads` H2, H14, H15 (progress of the build), H16 (the build continues after a cancel), H18 | core | `core-test history_reads` (H2 tests, H14's equivalence property test); `bench stats_history_build`, `stats_history_apply`, `stats_history_warm` | FSRS replay |
| **U290** | Knowledge Over Time: the causal FSRS replay of every card (H3.1), the samples (H3.2), today's stored point, preset-change recomputation, kept sums per deck key | `stats/history-reads` H3, H14 (knowledge members) | core | `core-test history_reads` (H3 tests: against Card Info C6.3 and C7.4, causality, bounds); `bench stats_knowledge_build` | the state replay's results |
| **U291** ∥ | By Deck: the home-deck partition, rows and per-window values | `stats/history-reads` H10 | core | `core-test history_reads` (H10 tests); `bench stats_log_year` | |
| **U103** | Stats FFI and CoreBridge, rescoped for round 11: the page request with its parts streamed by tier, progress messages, cancel tokens, `unknown_deck`, `unreadable_row`, `unsupported_scope`; the change events (`StatsChanged`, rollover, `CollectionReplaced`); `release_stats_caches()`; the Swift async sequence of part messages in `MeguriCore` | `stats/graphs-common` G8, G9.5, G10 (with round 11 lines); `stats/history-reads` H15, H16, H18 | core + CoreBridge Swift | `core-test`; `swift-test --package MeguriCore` | MeguriKit UI |
| **U292** | FFI and CoreBridge for the history parts and the day read: H2, H3, H10 parts in the stream, `stats_day_cards` | `stats/history-reads` H2.4, H3.3, H10.2, H13 | core + CoreBridge Swift | `core-test`; `swift-test --package MeguriCore` | |
| **U104** | Stats conformance and performance, round 11 additions: the `stats` testkit adapter and activation of the in-force graph scenarios; the H19 benchmarks at 150k and 500k; the core rows of §3 in the catalog once their ids exist | `stats/graphs-common` G14, G15; `stats/history-reads` H19, H20's benchmarks | core + tooling | `tools/mg conformance --lane docker stats/`; `tools/mg bench --sizes 150k,500k`; `tools/mg perf-gate --check` | |

### Lane B: the page (Swift)

| Unit | Scope | Spec sections | Side | Verify | Must not touch |
|---|---|---|---|---|---|
| **U294** | The page model (MeguriKit, no UI): deck and period state per window and device (first time All Decks and All), box switches; bins of S4 (days, weeks from the system's first weekday, months, Reviews Due ahead, the windows of S4.3); series for every box from the records; streaks; the refresh: one request, tier filling, the All-first-year sequence (S10.3), kept results in memory and on the device (S10.2), the subtitle status (S10.4), cancellation on change (S10.5), live refresh after half a second (S11). Starts against a CoreBridge protocol and fixture records; switches to U103's types when they land | `stats/statistics-page` S3, S4, S7 (data and figures), S10, S11, S17; S20's model tests | Swift | `swift-test --package MeguriKit -- Statistics` | views |
| **U295** ∥ | The chart kit: the box (header button with chevron, figure and sentence, plot, legend, figures, footnote), the two colour families on system colours and indigo, one axis, the selection rule and callout (hover and click pin on the Mac; hold and drag on touch; vertical swipe still scrolls), S8.5's empty and sparse rules, the per-box progress indicator (S10.3), the accessibility base (label, Audio Graph, per-bar elements in runs of ten, Differentiate Without Colour lines), Copy Image and Copy Values; a component gallery page | `stats/statistics-page` S8, S10.3, S15; N25.4 | Swift | model tests; VM screenshots light and dark, Increase Contrast, the largest text size; `tools/mg a11y` | page layout |
| **U296** | The Mac and iPad page shell: the sidebar row (second, `chart.bar.xaxis`) and View › Statistics ⇧⌘S, still hidden from `availableSections`; the toolbar's deck pop-up and period control; the strip; the section headings and View › Go to Section; the grid (1, 2, 3 columns); every state of S12 with the boxes as placeholders; the subtitle status; keyboard order | `stats/statistics-page` S3, S5, S6, S10.4, S12, S13 (sidebar, ⇧⌘S), S14; N25.1–N25.3, N25.10, N25.11 | Swift | **first run and no collection**; VM screenshots at 560, 1,250 and 1,600 points, iPad landscape; `mac-ui` for ⇧⌘S and Tab order | box internals |
| **U297** | Knowledge section: Knowledge Over Time, Retention (with its switch and its open points for thin bars), Forgotten (orange) | `stats/statistics-page` S7.1–S7.3 | Swift | **first run and no collection**; model tests of the figures; VM screenshots with the 150k fixture, a three-day collection and an empty one | |
| **U298** ∥ | Reviews section: Reviews Over Time (By Type and By Answer, Count and Time), Activity with every year and the streak figures | `stats/statistics-page` S7.4, S7.5; N25.8 | Swift (reuses HeatmapKit's grid) | **first run and no collection**; model tests of streaks; VM screenshots | Today's heatmap |
| **U299** ∥ | Cards section: Cards Over Time, Card Counts (segmented bar and table), Added and Started | `stats/statistics-page` S7.6–S7.8 | Swift | **first run and no collection**; a by-construction test that the last bar equals Card Counts; VM screenshots | |
| **U300** | Coming Up and Answering sections: Reviews Due (overdue bar, Total by then), Answers, Time of Day (two panels) | `stats/statistics-page` S7.9–S7.11 | Swift | **first run and no collection**; VM screenshots in 12- and 24-hour settings | |
| **U301** ∥ | Memory and Decks sections: Retrievability, Stability (doubling bins and the median interval), Difficulty, By Deck (sortable table; a row filters the page) | `stats/statistics-page` S7.12–S7.15 | Swift | **first run and no collection**; model tests of the bins' edges; VM screenshots | |
| **U302** | A chart's own page: the full chart, every row, About This Chart, the setting link, By Period on Retention's page, ⌘[ and Back keeping the scroll position; adds `stats.chartPage` to the screen inventory | `stats/statistics-page` S9; `stats/graph-presentation` P12; N25.5 | Swift | **first run and no collection** (the page cannot be reached then; check the route refuses); VM UI round trip | |
| **U303** | Show in Browse: the item texts of Q9, the Studied On tokens with their two labels, the Mac double-click, the touch callout button, Back to the same scroll position and selection | `stats/browse-queries` Q9; `search/filter-tokens` T3's round 11 line; `stats/statistics-page` S8.4 | Swift (+ U292's day read) | **first run and no collection**; model tests of every Q9 row; VM UI from three boxes and back | Browse internals |
| **U304** | iPhone, the folded Duo and a narrow iPad: the one column, four strip tiles, the pinned period, the Jump menu, the Statistics tab between Decks and Browse (hidden until U307), Count and Time in Reviews' menu, the swiping year grid, the Duo held tall with small boxes two abreast | `stats/statistics-page` S6, S13 (tab); N25.6 | Swift | **first run and no collection**; simulator jobs (iPhone, foldable folded and tall, iPad in Slide Over) | Mac layout |
| **U305** | The Duo opened: a section per screenful, the lead box left of the fold and the other boxes right, the two controls on either half, nothing on the fold, the side tab bar's item | N25.7 | Swift | **first run and no collection**; simulator job (foldable opened) with the fold overlay check | |
| **U306** ∥ | Deck Statistics: the deck context menu item, Deck › Deck Statistics, the touch More menu, the deck page's Activity header as a button, T on Today and a deck page | `stats/statistics-page` S13; N25.1, N20.2's new row | Swift | **first run and no collection** (the items are disabled or absent with no collection); `mac-ui` for the menu and T | the deck menu's other items |
| **U307** | Show Statistics: add it to `availableSections` on every platform in the final order (sidebar Today, Statistics, Browse, Note Types; tabs Decks, Statistics, Browse, Settings); the UI performance intervals of S19 in `StatsPerfTests`; the app rows of §3 | `stats/statistics-page` S13, S19, S20 | Swift | **first run and no collection** on every platform; `perf-ui --suite StatsPerfTests` on Mac and iPhone; a11y sweep | |

### Dependencies and parallel work

- Lane A: U98 first (everything reads through it). U99 and U101 need U98's request and columns. U288 needs U99 and U101. U289 needs only the derived store and the change stream to build and update; it plugs into U98's request when U98 lands, so it can start beside U98. U290 needs U289. U291 needs U98. U103 needs U98, U99, U101 and U288; U292 needs U103, U289, U290 and U291 (and U288's H13). U104 follows U99 and U101 for the adapter, and each core unit adds its own benchmark rows as it lands.
- Lane B: U295 needs nothing new and starts first. U294 starts against a protocol and fixture records, and integrates when U103 lands. U296 needs U294 and U295. The section units U297 to U301 need U296 and their data: U297 needs U288 and U290 (Knowledge can show its per-box progress until U290 lands, so U297 may land the two other boxes first); U298, U300 need U288; U299 needs U289; U301 needs U291. U302 needs U296 and one section unit. U303 needs U296, U292 and Browse's search field (U271). U304 needs U297 to U301. U305 needs U304. U306 needs U296. U307 needs U297 to U306.
- Up to three at a time, for example: {U98, U289, U295} → {U99, U101, U294} → {U288, U290, U291} → {U103, U296, U104} → {U292, U297, U298} → {U299, U300, U301} → {U302, U303, U306} → {U304, U305} → U307, with Swift units two at a time when they wait on the VM.

### Recommended first three

1. **U98**: the graph engine is "specified, not built" and every part, old or new, goes through its request and caches.
2. **U289**: the history component and the state replay are the largest new core risk (its incremental contract and the cold-pass speed decide whether the page's lead charts work at 150k); it needs only the derived store and can start beside U98.
3. **U295**: the chart kit has no core dependency, fixes the visual system (colour families, selection, progress, accessibility) that all eleven later Swift units reuse, and can be shown to Antoine early.

U294 is the fourth, as soon as a slot opens.

## 5. Requirements this plan supersedes

Each carries a note in the parent plan ("Superseded 2026-11-22 …"), with the decision rows as authority, for the plan owner to add: R1350 (one deck pop-up; no Collection or Search scope in this version), R1352 (default All), R1354 (kept deck and period), R1355 (one request per refresh, parts by tier), R1357 to R1359 (sections and the parts of a box, the grid), R1361 (S8.5's lines), R1362 (the page's own bins), R1364 (no second axis), R1366, R1367 (one period), R1369 to R1372 (the strip), R1375 to R1377 (Reviews Due), R1379 to R1383 (Activity replaces the Calendar box), R1386 to R1389 (Reviews Over Time), R1391, R1392 (Card Counts without the donut and the toggle), R1394 to R1396 (no Review Intervals box), R1399 to R1401 (Stability and Difficulty bins and colours), R1410, R1412 (Time of Day), R1414 (Answers), R1415, R1416 (Added and Started), R1421 (later, 9A). R1363's colours change as `stats/statistics-page` S8.1 says. Units U225, U226, U230, U231 and U236 of the parent plan are superseded by U294 to U307 (U236 deferred with 9A); U227's full heatmap component and U228's streak stats on Home are not built by this plan.

## 6. Decisions and defaults (each an assumption the specs make; the owner can change any)

1. **One deck rule.** Every part follows `stats/graphs-common` G2.1 (`did` or `odid` in the subtree), so Show in Browse's `deck:` token finds the same cards, except By Deck, which partitions cards by home deck so its rows add up (H10.1). A filtered deck shows the cards it holds now with their whole history (S12.6).
2. **Days.** Anki's day (B3) folded at 0 for every new read, with dates by AH3's `T0`; weeks start on the system's first weekday, not `firstDayOfWeek` (S3.4).
3. **Anki's three synced display keys** are read, never written; their effects are fixed (always separate suspended and buried, always show overdue).
4. **"Remembered"** is the retention table's rule (V4.1) everywhere on the page, Time of Day included (a Meguri extra beside Anki's `correct`).
5. **Cards Over Time** leaves out cards suspended or buried today and deleted cards; a card without a log enters on its creation day in its current state; the last day equals Card Counts by construction.
6. **Knowledge** replays each card with today's parameters, using only the rows up to each sample (a later review never changes an earlier sample); today's point is the sum over the stored memory states, so it equals Retrievability's Estimated total knowledge. Each card adds between 0 and 1.
7. **By Answer** leaves out unrated rows (other clients' `ease` 0); the chart's page says how many.
8. **Windows without a time axis** (Answers, Time of Day, By Deck) are 30, 90, 182 or 365 days or all, not week-aligned; 6M's 182 days is Meguri's.
9. **Streak**: a day not yet studied today does not break the current streak.
10. **Kept results** persist on the device per (collection, deck, period) and are dropped when the collection is replaced.
11. **Activity** ignores the period and shows every year.
12. **Show in Browse for a day** uses Browse's Studied On token, labelled "Forgotten on <date>" for Forgotten.

## 7. Open points for the owner

1. **Product Contract ids for the new speed rows** (§3) so `tools/perf/budgets.yaml` can hold them. Recommendation: the plan owner adds them as one new requirement group after R1841, with `pending:` cases owned by U104 and U307.
2. **Two of the design page's words** (its name for the page's parts and for one of them) are refused by the repository's private-token scan, so the specs say "sections" and "Answering". The app's strings face the same scan. Recommendation: keep "Sections" ("View › Go to Section") and "Answering" in the app, and show Antoine the two names in U296's first screenshot.
3. **`browser/workspace` V15** says a heatmap day's Studied On token uses the heatmap day (B4) bounds, while `stats/activity-heatmap` AH3 uses Anki's day (B3). Recommendation: V15 follows AH3 (B3), as the page's day items do (Q9); a small spec fix after this PR.
4. **Spec lines awaiting approval** (§8): the coordinator approves them under Antoine's permission of 2026-11-20 after the reviews.

## 8. Spec lines awaiting approval

New files: `specs/stats/statistics-page.md`, `specs/stats/history-reads.md`. New sections: `docs/design/native-interface-contract.md` N25; `stats/browse-queries` Q9; `stats/graph-presentation` P24. Changed lines (each beginning "Changed 2026-11-22 (round 11)", each file's Status line naming them): `stats/graph-presentation` P2, P3, P5 to P19, P22; `stats/graphs-common` G3, G5, G7, G8.3, G10; `stats/graphs-revlog` V2.2, V4.3, V5.3, V6.2; `stats/graphs-cards` C3.6, C11; `stats/browse-queries` Q5; `stats/studied-today` T3, T4; `stats/activity-heatmap` AH1; `stats/pdf-export` X1; `search/filter-tokens` T3, T15. The inventory entries `stats.overview`, `home.deckMenu`, `home.deckPage` and `shell.root` point to these specs.

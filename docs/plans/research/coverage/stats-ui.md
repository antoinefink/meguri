# Coverage note: stats-ui


Coverage map (unit → requirements)
- U225: R1291, R1294 (refresh on open), R1295, R1349–R1356, R1357, R1358, R1359, R1360, R1361, R1366 (persistence hook), R1368, R1417, R1419, R1420; AE53 (Stats), AE59.
- U226: R1290 (≤16 ms re-bin), R1293 (UI quantiles), R1362–R1367, R1418, R1420, R1439, R1440, R1443 (charts), R1444.
- U227: R289 (one component), R294, R297–R301, R1301, R1302–R1308, R1313–R1314 (display), R1315, R1316, R1317–R1325, R1441, R1443, R1445; AE54, AE55 (display).
- U228: R289 (scopes), R290–R293 (display of core values), R302, R303, R304, R305, R1296–R1300, R1309, R1322 (query prefixes), R1323, R1326–R1335, R1342 (honoured), R1344–R1348, R1442, R1997; AE5, AE6, AE51, AE53 (Home).
- U229: R1325, R1336–R1343 (UI side), R323/R324 (UI never writes unchanged/local settings); AE56, AE6 (toggle).
- U230: R1369–R1393, R1415, R1416; AE57 (UI), AE60.
- U231: R1394–R1414; AE59 (Retention).
- U232: R719 (name surfaced), R730–R738, R752, R753 (editor side).
- U233: R736 (Essentials), R739–R742, R748 (placement), R765 (via core).
- U234: R743–R753.
- U222 (browser-ui file, `Features/CardInfo/`): R1422–R1430 (display), R1448 (display), R1841 (UI); AE46. The stats-UI area's Card Info view was merged into it.
- U236: R1421.
- U237: R1837, R1838, R1839, R1841, R1290/R1291 (observable), R1439–R1445 (audit), R1997, R305, R1335.

R-IDs in my range owned elsewhere
- R290–R293, R304 data rules, R1310–R1312 (forecast/history limits), R1313/R1314 level computation, R1332 streak levels, R1336–R1338 config read/patch semantics, R1340, R1341: stats core (U95; heatmap/aggregates). UI only displays.
- R1373–R1374, R1378 (daily load), R1384 (Anki-day → civil date), R1402–R1403 (R/knowledge), R1405–R1408 (retention counts), R1411 (hour attribution), R1446, R1447: stats core graph data (U98), oracle-checked there.
- R1448 text formatting (studied-today, card-info values, revlog columns) and R1425 no-write: stats core (U97 for studied-today, U92 for Card Info); UI displays verbatim.
- R1431–R1438 widgets and R1840 widget perf: U100 (snapshot builder) + U165 (widget extension).
- R737 save steps, R744 re-sort, R753 read-side replacement, R762–R765 preservation/format: deck options core (U49) over decks/deck-config ops (U28).
- R754–R759 timer/auto-advance/audio/timebox behaviour during review: Review UI (U175) with scheduling (U43/U44).
- R760, R761 global scheduling preferences: Settings (U160) + core (U40/U41).
- R766–R768 deck-tap/deck-page/shortcut platform adaptations: Home (U152), Review UI (U175), Browser UI (U210), menus U150.

Cross-area contracts assumed (orchestrator to reconcile)
- U98 `graphs(search, history, cancel)`: one call returning raw, unbinned data for all 13 graphs (per-day per-kind counts/times; due-day counts incl. negatives + daily load + overdue flag; Anki-day calendar entries keyed by civil date; card-count categories for both separate modes + distinct notes; interval/stability histograms; 100 difficulty and retrievability bins + missing-memory-state count + average R + estimated knowledge; retention table counts; hourly and button buckets for each of the four ranges; Anki-formatted Today studied line), plus scope-search error; get/set for the synced graph preferences `futureDueShowBacklog`, `firstDayOfWeek`, `cardCountsSeparateInactive`.
- U95 heatmap payload per scope (collection minus `limdecks` / deck by home deck): sparse civil-date counts with core-computed levels (history and forecast), today, first/last bounds, four streak values with levels; studied-today payload with Anki-formatted text; heatmap config read and per-field patch.
- U92 card-info call (exported by U103): ordered Anki-formatted rows, formatted revlog rows, forgetting-curve samples per available range + default range, desired retention, debug JSON; never writes. Consumed by the shared U222 view.
- U49 deck-options snapshot over U28 records (presets + use counts, deck overrides incl. Today-only day, globals, defaults and valid ranges) and one save request executing R737 as an undoable "Deck Options" op that reports a one-way-sync requirement on preset removal (U49 save op over U28 preset ops); U49 learning-step parse/format.
- U41 deck queue counts plus buried difference for the deck page.
- U150: routes `.stats(scope:)`, `.deckOptions(deckId)`, `openBrowser(search:)` preserving section state; a leave-guard hook for unsaved deck options; File menu item "Export Stats as PDF…" (absent from R280's list) routed by FocusedValue; `T` key on Home/deck pages.
- U151/U40: time-zone-change notification forwarded to the core (R1273) and `RolledOver`/`CollectionReplaced`/ExclusiveBusy observed by these screens.
- U3 oracle extension: a headless driver of Anki 26.9.3's compiled graphs page (from the pinned aqt wheel, in headless Chromium) that records bins, tooltip strings, table rows and click queries for a given graph payload and control state → `conformance/goldens/stats/presentation/`. Without it, R1362/R1365 parity goldens would have to be spec-writer-authored, which the Key Decision "expected results always come from running the pinned Anki release" forbids.
- Swift target layout assumed: sources `app/Packages/MeguriKit/Sources/Features/Stats/` and `.../Features/DeckOptions/`, test targets `app/Packages/MeguriKit/Tests/StatsTests/` and `.../DeckOptionsTests/`; heatmap/activity views live in the Stats target and are imported by Features/Home (U152) and Settings (U160); the heatmap palette and grid layout live in the core-free `HeatmapKit` target shared with the U165 widget.

Conflicts / open questions
- R295/R296 (five add-on themes and palettes) and AE8 ("switches the theme to ice") conflict with R1315 (one palette, themes dropped, `colors` ignored). Units follow R1315; AE8's theme step cannot be executed (AE56 already covers the write-only-changed-field rule via Calendar mode).
- R298 (⌥-click jumps to bounds) vs R1319 (⇧-click on Mac): U227 accepts both modifiers on Mac.
- R300 (tapping a day opens Browse) vs R1321/R1322 (touch tap selects and shows a callout with "Show in Browser"): U227 follows R1321/R1322 on touch; Mac click opens directly.
- R289 ("matched on the card's current deck, not its original deck") vs R1284 (deck-page heatmap uses home deck `odid` else `did`) and R1283 (Stats deck scope uses `deck:` semantics): units consume whatever U95/U98 decide; recommend R1284 for the heatmap and R1283 for Stats, and amending R289.
- R1319 (horizontal swipe pages) vs R1321 (drag across the grid scrubs): resolved in U227 as fast flick = page, hold-then-drag = scrub, tap = select; needs user confirmation on device.
- R1355 (one core call recomputes all graphs) vs R1838 (graphs below the fold computed only when scrolled into view): interpreted as one core call for data (≤50 ms per R1290) with lazy binning/rendering of below-fold cards.
- R1366/R1367: behaviour when history returns to "last 12 months" while a graph had "all" is unspecified; U226 falls back to the graph default unless the spec writer finds Anki does otherwise.
- R742 "stronger styling below 70%" is unreachable given the 0.70–0.99 editable range and R753's default-on-read; harmless.
- R736 vs R748: "Don't play audio automatically" appears once, in Essentials, not repeated in Advanced ▸ Audio.
- R1448 vs String Catalogs: Anki-matched texts are formatted in the core (English only per R191) so they stay oracle-testable headless; Swift-owned labels use String Catalogs.

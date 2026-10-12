# Coverage note: stats


R-ID coverage map (unit → requirement IDs)
- U95: R1269–R1276, R1278–R1281, R1286–R1288, R1292 (per-day part), R1295, R290–R292, R304
- U96: R1302–R1305 (data), R1309–R1314, R1317–R1318 (data), R1320 (data), R1322–R1323, R1326–R1332, R1336–R1341, R1343, R1346 (data), R1441 (data), R289, R293–R295 (data), R297–R300 (data), R302–R303
- U97: R1277–R1279, R1296–R1298, R1300 (data), R1369–R1372, R1448 (studied-today)
- U98: R1282–R1285, R1289, R1290 (engine), R1291 (data side), R1292 (cache release), R1293, R1294, R1350–R1353 (data), R1355, R1362, R1364 (data), R1365 (data), R1367, R1417 (composition), R1420
- U99: R1379–R1389, R1405–R1408, R1410–R1414, R1446 (part)
- U100: R1274 (widget), R1431 (data), R1433–R1436, R1438 (data), R1840 (snapshot side), R312, R315–R318
- U101: R1285, R1294, R1373–R1378, R1390–R1404, R1415–R1416, R1446 (part)
- U92 (search area file, `meguri-stats/src/card_info/`): R1423–R1428, R1430, R1448 (Card Info rows), data side of R1248–R1261. The stats area's Card Info data unit was merged into it.
- U103: R1274, R1291, R1300, R1356, R1375/R1381/R1391 (config write path), R1429, R1351 (error surface)
- U104: R1271, R1290, R1292 (memory), R1446–R1448 (suite)

R-IDs in my ranges owned by other areas (UI/presentation or other cores)
- Stats UI area (U225+): R1299 (line placement, with Home U152), R1301, R1306–R1308, R1315 (palette rendering), R1316, R1319, R1321, R1324, R1325, R1333–R1335, R1347, R1349, R1354, R1357–R1361, R1363, R1366, R1368, R1382 (colour scale rendering; max supplied by U99), R1386 (toggle UI), R1409, R1412 (labels), R1418, R1419, R1421 (PDF export), R1439, R1440, R1442–R1445; the UI halves of R1302–R1305, R1317–R1322, R1346, R1350–R1351, R1365, R1372, R1381, R1383, R1427–R1428; R296 (rendering), R297–R301 (UI parts), R305.
- App shell: U160 Settings owns R1342 and the Heatmap settings UI of R1339 (data writer in U96). U152 Home owns R1299 and the R1301 placement. U165 widget extension owns R1432, R1437, R306–R311, R313, R314 and the UI of R1435, R1436 and R1438. U151 calls the U100 flush and WidgetCenter reloads.
- Scheduling U41 (queue counts and the buried "+N" difference) with Home/deck page UI U152: R1344, R1345, R1348.
- Shared Card Info view U222 (hosted by Review UI U183 and Browser UI U221): R1422 entry points, R1262–R1264 (previous-card info).
- Performance area (U240+): R1837–R1840 and R1851 as gates. U104 supplies the stats benchmarks.

Cross-area contracts assumed (orchestrator to reconcile)
1. U24/U25: in-core derived-state observers run on the writer thread after commit and before the ChangeSet is published. They receive row payloads:
   - revlog rows added and removed, with removed rows taken from undo before-images, because revlog deletions leave no graves;
   - compact card before/after: deck, odid, queue, due, type, ivl, memory state, existence;
   - config keys changed.
   - The C2 change stream to Swift stays id-based. U26 likely needs the same hook.
2. U40: the clock injection carries the IANA zone id along with `now_ms` and the current offset. U40 emits `RolledOver` and a `TimeZoneChanged` event and exposes `next_day_at` and days_elapsed for stats reuse (R1272).
3. Time-zone database: R1270 needs per-instant DST rules for historical reviews. The rust-stack decision says "no tz database". Proposed amendment: `jiff` with a bundled tzdb, used only for heatmap/widget civil-day mapping, keyed by the zone id from Swift. Anki-day math keeps injected fixed offsets. Alternative if rejected: Swift supplies an offset-transition table over the history range.
4. U3 oracle (oracle-maintainer role, `oracle/` only) needs these driver ops:
   - `stats.graphs` (raw graph data for search + history window);
   - `stats.card_stats`;
   - `stats.studied_today`;
   - `heatmap_addon.activity`, which runs the Review Heatmap add-on v1.0.1 activity computation headless with a stub of the desktop main window; this is the oracle for R1447 and the AE5/AE6/AE51/AE52/AE55 goldens.
5. U80: search → card-id set on a reader connection; typed parse errors; the canonical writer for round-tripping generated queries; `prop:rated`, `prop:due`, `prop:ivl`, `prop:s`, `prop:d`, `prop:r` and `added:` supported exactly as Anki.
6. U46 (sched): memory-state-from-history replay for one card (Card Info, no write), and FSRS retrievability through the pinned crate. The time-span formatter used by U45's answer-button labels is shared with Card Info. If the scheduling area does not own one, U97's `text.rs` provides it.
7. U23: a generic typed config-write op that preserves value types and unknown fields, used for `futureDueShowBacklog`, `cardCountsSeparateInactive` and `firstDayOfWeek`. The `heatmap` key patch writer is in U96.
8. U151/U165:
   - Swift passes the App Group snapshot path (`widget-snapshot.json`) at open;
   - U151 calls `flushWidgetSnapshot` at session end, on going to the background and in C8;
   - U151 calls WidgetCenter reload only on `WidgetSnapshotWritten{changed:true}`, and may throttle reloads during long foreground sessions;
   - U165 decodes the schema in `specs/stats/widget-snapshot.md` and uses `conformance/goldens/stats/widget/*.json` as fixtures;
   - U165 builds timeline entries from the precomputed `upcoming[]` list (7 rollovers, an extension of KTD16's "after next rollover").
9. U225 Stats UI:
   - the core returns every range and toggle variant pre-binned, so per-graph controls make no core call (R1290, R1355);
   - the UI requests above-the-fold graphs first and then the rest (R1838), with shared scope work memoized in the core. This is how "one core call" in R1355 and "below-the-fold computed only when scrolled" in R1838 are reconciled;
   - the UI formats tooltip text and day labels from bin bounds, except the oracle-tested strings (studied-today, Today lines, Card Info values), which the core supplies and the UI shows verbatim. That is an exception to the "Strings via String Catalogs" convention.

Conflicts and open questions
- R289 ("matched on the card's current deck, not its original deck") conflicts with R1284 (deck-page heatmap uses the home deck, `odid` else `did`). The units follow R1284, the later and more specific deviation. The orchestrator should correct R289.
- R296 (five add-on themes with exact palettes) and AE8 ("switches the theme to ice") conflict with R1315 (one Meguri palette, `colors` kept but ignored, no theme picker). The units keep `colors` byte-for-byte and never write it. AE8's "theme" step needs rewording, for example to the Calendar mode, as in AE56.
- R298 (⌥-click jumps to bounds) conflicts with R1319 (Shift-click on Mac). This is UI only. The orchestrator should pick R1319.
- R307–R310 (widget family contents, e.g. Small = 8-week heatmap) conflict with R1432 (Small = streak + today count). The U100 schema covers both, so U165 picks one family spec.
- R317 (36 h staleness, "Open Meguri to set up") conflicts with R1436/R1438 (one day, "Open Meguri to get started"). KTD16 sets 24 h and one string. The units assume R1438's string.
- R315 lists "due today", "forecast for today and the next day" and "theme", while R312/R318 forbid due counts. U100 carries the forecast only as cell levels and carries no theme.
- R1287 (deck heatmaps filtered "at query time" through card → home-deck) is implemented literally (resident row index plus card table) in U95/U96. Home totals with `limdecks` are also maintained incrementally. Card moves across an excluded-deck boundary adjust totals via the row index, so no rebuild is needed.
- Binning goldens: Anki computes graph bins and tables in its TypeScript front end. The Python backend oracle returns only raw graph data. Open question for U3: either run Anki's shipped graphs bundle from the `aqt` wheel headless (Node or headless Chromium) inside the oracle container to produce binned goldens, or the spec writer authors binning goldens from the spec and a critic reviews them. The default proposed in U98 is the former, if the oracle-maintainer finds it practical.
- `limdecks` with deleted cards: the add-on's deck restriction may implicitly drop deleted cards' reviews from Home when exclusions are set. R1341 implies `limcdel` alone controls this. The U96 spec writer must state the observed add-on behavior from the oracle, and R1447 parity wins.
- Studied-today, Today and Card Info strings come from the core (English, oracle-matched), which deviates from the String Catalog convention (R191 is English-only, so there is no localization loss). The orchestrator should confirm.

# Coverage note: browser-ui


R-ID coverage map (unit → requirements)
- U210: R1106, R1109, R1110, R1111, R1112, R1113, R1114, R1232, R1233 (scaffold), R1824–R1827; UI side of R1084; Browse entry side of R48, R98, R216, R300, R319, R807, R1418, R1733.
- U211: R1118–R1132 (display), R1147–R1151, R1155, R1156, R1241, R1242, R1243, R1268 (rows), R1829; UI side of R1792. AE44.
- U212: R1152, R1153, R1154, R1215, R1225, R1229 (navigation), R261 (⇧⌘L), R262, R263.
- U213: R1115, R1116, R1117, R1133–R1142, R1146 (model), R1244, R1265, R1266, R1267, R264, R1226 (mode toggle), R1828, R1830 (mode switch); UI side of R1143–R1145. AE50.
- U214: R1086–R1093, R1107, R1108, R1109 (display), R260 (⌘F), R1229 (Find). AE41 (display), AE42.
- U215: R1094–R1105.
- U216: R1157–R1170, R1268 (sidebar), R260 (⇧⌘F), R1226 (Toggle Sidebar), R1229 (Sidebar Filter). AE43.
- U217: R1171–R1180, R274.
- U218: R1181–R1186, R1190–R1195, R1212, R1225–R1231 (catalog), R1245, R1831, R265, R268, R269, R270, R271, R273 (registration). AE45, AE10 (Browse clause).
- U219: R1187, R1189, R1196, R1198, R1199, R1213, R1214, R1117, R1268 (chooser), R265, R267, R273 (export, copy); UI side of R1188, R1197.
- U220: R1200, R1201, R1206, R1208, R1209, R1210, R266, R1832; UI side of R1202–R1205, R1207, R1211. AE47, AE48 (UI side).
- U221: R1216–R1224, R261 (⌥⌘L), R273 (preview), R275, R1830 (row → editor).
- U222: R1247–R1250, R1253–R1260, R1262–R1264, R272, R1422–R1430 (display), R1448 (display), R1841 (UI); UI side of R1251, R1252, R1261. AE46 (UI side). Shared Card Info view, merged from the stats-UI area.
- U223: R1234–R1240, R1146 (menu), R1133 (Display menu), R1233 (compact), R1151 (iPhone); Browse part of R190.
- Every R-ID in R1086–R1268 and R260–R274 is covered above; R1824–R1832 are acceptance targets in U210/U211/U213/U218/U220/U221/U223 BrowsePerformanceTests.

R-IDs in my ranges whose rule is owned elsewhere (UI only invokes or displays)
- R1118–R1130 cell contents, R1143–R1145 sort orders, R1080, R1084 (core cancel): U84.
- R1188 (memory-state recompute on move), R1197 (FSRS Set Due Date interval): core bulk ops U86 / Scheduling area.
- R1202–R1205, R1207, R1211 (find/replace and batch-edit semantics and save path): U86.
- R1251, R1252, R1261 (display-only memory state, no backfill, oracle parity of card stats): U92 (Card Info data).
- R1189 mapping/saving (R944–R948): Notes UI U200.
- R1213 export sheet and options: Packages export U122 plus the export sheet in the App shell area.
- R1246 (CI benchmarks failing on regression): performance gate units U240+; Browse supplies BrowsePerformanceTests cases.
- R1792 overall: Mac servers area (U135–U147); Browse implements the visible-row refresh and "Results changed — Refresh" banner (U211).
- R1267 lives inside Settings (U160); Browse provides `BrowseSettingsSection` for embedding.

Cross-area contracts assumed (orchestrator to reconcile)
1. U80 must expose: parse/validate returning either the canonical text plus an AST with source spans per node (kind, key family, negated, AND/OR join, group) or an error with category, offending range and message; a node-edit/rewrite API (negate, change value, switch join, remove) returning canonical text; canonical text for a selected set of nodes; the combine API implementing R1079 (replace, AND, OR, negate, replace same kind) for sidebar/chip/token actions; node builders with name escaping for sidebar items (R1078); a pure search-escaping helper for autocomplete insertion (sub-millisecond, callable on the main thread as an R1857 exception — confirm).
2. U84 must expose: a cancellable search returning the ordered id list in two phases (first page early, then complete list and count) for R1826; paged row fetch (about 200 ids per call) for a given column set returning cell strings, row state (flag, marked, suspended, buried, leech, filtered), note id, browser font hint and RTL flag, with Question/Answer rendered only when requested; a column catalog (key, labels per mode, tooltip, sortable per mode, default direction, alignment); selection mapping between cards and notes modes; a search-vocabulary snapshot (decks with filtered and browser-collapsed flags, tag tree with collapsed flags, notetypes with templates and fields, presets, saved searches, flag labels).
3. U86 must expose: bulk ops (flag, mark, suspend, bury, tags add/remove, clear unused tags, change deck, set due date with input validation, reset, grade now, delete notes) with CancelToken, ~10 Hz progress, a commit-phase signal (to stop offering Cancel), one transaction and a named undo entry; find-and-replace dry-run count and regex validation; field names for a note set with per-field lacking counts; batch-edit apply.
4. Core config API: typed read/write of the R1265 keys writing only on change and preserving raw arrays/unknown keys (R1137, R1138, R1266). Assumed in U23 (config table); `savedFilters`/`flagLabels` edits are whole-key rewrites preserving other entries.
5. Change stream C2 (U24) must carry note ids for card-only changes or Browse needs a card→note index; Browse keeps a note-id index from fetched rows. Browse also needs the event `source` to decide on the "Results changed" banner.
6. Template rendering (U60/U64) must offer a preview render mode that drops type-in boxes (R1220); U67 must allow a single non-review CardWebView instance with the shared scheme handler and audio player.
7. Review session (U43/U175) must publish the previous-answered card id that tracks undo (R1262–R1264) and host the Card Info panel from U222.
8. U92 card-info snapshot (R1248–R1257, R1259), exported by U103: ordered applicable rows, history with optional stability, forgetting-curve points with decay and desired retention, debug JSON; must be read-only.
9. Shell (U150): menu bar contribution via focused values, a shortcut catalog that per-context Browse bindings register into for the R200 duplicate test, and `AppNavigation.open(search:)`/`openBrowse(fromReviewCard:)` used by other screens.
10. U190 must provide reusable pieces: tag entry with registry autocomplete, a single-field rich editor with HTML source toggle (Edit Field…), and the editor's focused-field index for R1224; U195 accepts a prefill (Create Copy) and a deck preset (sidebar Add Note).
11. Card Info lives in its own shared target `Features/CardInfo` (U222), imported by Browse, Review, the editor and the session summary.
12. One deck chooser component (U219 `DeckChooserSheet`) should be shared with the Add screen's deck picker (U195) to avoid two implementations.

Conflicts and open questions
- Keyboard clash ⌃⌘S: R217/R20 make ⌃⌘S the app sidebar toggle; R1226 makes it Browse's Toggle Sidebar. Proposal: inside Browse ⌃⌘S toggles the filter sidebar column; the app sidebar stays reachable from its toolbar button and View menu without a shortcut while Browse is focused. Needs shell (U150) agreement.
- Keyboard clash ⌥⌘A: R1153/R1225 say Select All ⌥⌘A "anywhere" in Browse; R252 makes ⌥⌘A attach media in the editor, which is embedded in Browse's inspector. Proposal: when an editor field has focus, the editor's ⌥⌘A wins and Browse Select All is disabled (consistent with the text-field rule R174/R197).
- R1229 says Go › Note editor focus stays unbound, but R261 binds ⌥⌘L to focus the editor pane. Proposal: follow R261 (bind ⌥⌘L); keep Go › Sidebar unbound.
- ⌥⌘I means Card Info toggle in Browse (R272/R1226) and Previous Card Info in review (R242/R1262); contexts never overlap, so R200 holds — noted only.
- Menu bar: R280–R287 list no "Notes" or "Go" menu, yet R1231 requires every Browse shortcut in the menu bar. Proposal: Browse adds "Notes" and "Go" menus (items disabled outside Browse); shell area to confirm.
- Deferred question "which app-level shortcuts must yield to ⌘1–⌘7": the Product Contract defines no app-level ⌘-number shortcuts (sections use D/B/T), so ⌘1–⌘7 are free in Browse and also flag in review (R233). Proposed resolution; the U218 catalog test enforces it.
- Deferred question on accepting `rid:<startMs>:<endMs>`: not adopted here; R1032 makes `rid:` a field search. If adopted, it is a U80 grammar change, and autocomplete would need a hint.
- R1219 "iOS respects the silent switch" vs R136 "Play card audio in Silent Mode" (default on). Proposal: Preview uses the same R136 audio-session setting as review; flag for product confirmation.
- Budgets: sort change has R1083 (first page <100 ms, core), R1244 (reverse/columns/mode ≤100 ms, UI) and R1828 (re-sort ≤200 ms, UI). Tests assert R1083 in core benches, R1828 end-to-end in the UI and R1244 for reverse/columns/mode toggle.
- R1792 scope: the requirement covers server changes only; spec-flow asks whether sync-incoming or other-screen changes also raise the banner. Proposal: any change not originating from Browse's own actions refreshes visible rows in place and raises "Results changed — Refresh"; deletions always remove rows (R1156).
- Table backend: SwiftUI `Table` may not meet R1829/R1152 at 150k rows with a distinct current row; U211 spike B1 decides between it and NSTableView/UICollectionView representables (permitted by "AppKit/UIKit only where SwiftUI falls short").
- Foldable layouts (R1233 "unfolded foldable") rely on size classes only until the 27.1 SDK (swiftui-arch §0.3); fold-specific work belongs to the shell's deferred foldable unit.

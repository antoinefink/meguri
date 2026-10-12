# Coverage note: search


R-ID coverage map
- U80: R1021–R1032, R1075–R1077, R1080
- U81: R1033–R1049
- U82: R1050–R1074, R1268 (search semantics)
- U83: R1115 (key), R1134, R1135 (order storage), R1137, R1138, R1141, R1265, R1266, R1267 (synced keys), R1038 (key), R1196/R1198 (remembered-input keys)
- U84: R1118–R1131, R1139, R1140, R1143–R1145, R1268 (display)
- U85: R1080, R1084, R1116, R1117, R1142, R1241, R1242, R1244 (with U90); API for R1776–R1778
- U86: R1181–R1185 (core), R1200 (core), R1202–R1211, R1245 (with U90)
- U87: R1117, R1187–R1199, R1212–R1215 (core), R1268 (actions)
- U88: R1078, R1079
- U90: R1081–R1083, R1244–R1246
- U91: R1085
- U92: R1248–R1257, R1259–R1264 (data), R1250, R1423–R1428, R1430, R1448 (Card Info rows); merged from the stats area's former Card Info data unit

R-IDs in my assigned ranges owned elsewhere (UI parts)
- Browser UI (U210+): R1132, R1133, R1136, R1146, R1186, R1201 (per-device history), R1243, R1247 (placement, U222), R1258 (U222).
- Settings UI (U160): the R1267 screen.
- Review UI (U175): the R1262/R1263 menu and switch UI.
- My units also provide core support for these R-IDs outside my ranges: R1086–R1114, R1147–R1156, R1168–R1170 (Browser UI); R1776–R1778 (Servers, U141); R1824–R1832 (Performance area); R1916–R1920 (Conformance area).

Cross-area contracts assumed
1. U22 provides a connection-init hook. The engine uses it to register `meguri-search` SQL functions on every reader and writer connection; storage stays free of any search dependency.
2. `meguri-search` depends directly on the pinned `fsrs =6.6.2` crate (not on `meguri-sched`) for `prop:r` and the retrievability sort and cell. Day and time values arrive through `SearchContext`, filled by the engine from U40. The orchestrator should confirm this exception in the KTD2 layering.
3. Cloze-markup stripping (`sc:`) and strip-HTML-keep-media-names (`dupe:`, sort field) live in `meguri-text` and are shared with U61, U20 and U27. Search must not depend on `meguri-template`. Question/Answer cells call U60 through the engine (U84's `qa.rs` is invoked from the engine layer). Alternatively the orchestrator can move `browser/qa.rs` into `meguri-engine`.
4. U48 exposes the manual-scheduling ops (Set Due Date with grammar parsing, Forget/Reset, suspend/unsuspend, bury/unbury, Grade Now over U44's answer application). U49 exposes the memory-state recompute plus step remap on deck change (R645). U87 calls them.
5. U35 owns the change-notetype op and its mapping (R944–R948); U29 owns the schema-change gate and the R949 warning classification; U87 only validates and invokes. U30 owns tag ops (R894–R896) and `flagLabels` (R899). U27 owns the note-save path and delete. U122 accepts a note-id set for export.
6. U43 keeps the answered-card history with undo awareness for Previous Card Info. U26 provides freshness watermarks and the change-stream-driven update path for U90's folded-text table.
7. U85 is the API contract for U210:
   - non-blocking `start_search` with seq-tagged callback events (FirstPage, Progress, Complete, Failed, Cancelled)
   - `rows` and `qa_cells` pages
   - `reverse` and `resort`
   - `map_selection`, `resolve_selection`, `index_of`
   - `invalidated_rows` with a `removed` list
   - `StaleGeneration` after replacement

   Row records carry typed cell values plus row state flags. Swift does the formatting (shared time-span and date formatters owned by U210/DesignSystem).
8. U88's builder is the only way other areas (Stats "Show in Browser", Home "Browse", the editor's "Show duplicates", AnkiConnect `guiBrowse`) produce search text.
9. U3/U4 must support the observations `search` (ordered ids or parse_error, both modes, with a sort), `canonical` (normalized text) and `card_info`. The oracle driver must expose Anki's search normalization, join and replace for golden generation.
10. U141 calls U85's `search_ids(text, mode, IdAscending)` and reuses the U80 error message text.

Conflicts and open questions
1. AE49 contradicts R1036 and the Key Decision "Search ignores letter case for every alphabet". AE49 says `über` must not match "Über" in a plain search. U81 tests the R1036 reading: it matches in product mode and does not match only in AsciiCompat mode. AE49 needs rewording.
2. R1776 lists `rid:` as a supported AnkiConnect search, while R1032 makes `rid:1:2` a field search, and the Outstanding Questions leave open whether to accept the add-on's `rid:` range. The units follow R1032 (no `rid:` keyword), which keeps R1085 parity. Adding `rid:` would be a deliberate deviation that needs a decision and a deviations entry.
3. Budget mismatch: R1083 says a sort change shows the first page in under 100 ms; R1828 says re-sort within 200 ms. U90 targets the stricter 100 ms.
4. R1145 requires id tie-breaking, but R1920 requires ties sorted "identically to the oracle". Anki's tie order beyond its documented keys is SQLite-plan-dependent. U84 compares ties as sets within equal-key groups unless the oracle's order is deterministic. The conformance area should confirm.
5. Case-folding definition: the requirements do not name the Unicode folding variant for plain and field search. U81 proposes simple (1:1) case folding, consistent with `re:`/`w:` and length-preserving for `_`. Name searches keep unicase full folding (ß = ss). Confirm.
6. R1916's "difference applied to the oracle's expected results" is implemented as AsciiCompat-mode exact equality plus a superset-with-explanation check (U91). The `test-hooks` feature gates AsciiCompat; it must also be enabled in conformance builds.
7. R1057 (current deck updated when opening a deck page or starting review): U82 covers resolution and fallback. The write op belongs to U28, and the triggers to Home (U152) and Review (U175). The orchestrator should assign that trigger.
8. Bulk ops on 150k rows hold the single writer (C1) for the commit phase, so a review answer during a large bulk commit waits for it. This is acceptable only if the commit phase stays short (computation happens in the cancellable read phase). U90 measures it; R1807 must not regress during bulk ops.

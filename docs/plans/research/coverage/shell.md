# Coverage note: shell


R-ID coverage map (unit → R ranges)
- U150: R1 (scene side), R2–R8, R11–R13, R17–R23, R25, R110, R319–R321
- U151: R14, R15, R45 (triggers), R87 (day clock), R88, R107, R114 (core-call side), R123, R124, R126, R127 (logic), R324
- U152: R15, R16 (implementation), R24 (Home "+"), R28–R45, R50, R53 (display), R74, R75, R182
- U153: R108, R164 (pattern), R165–R167, R169–R172
- U154: R14 (assertion), R127 (iOS background time), R1455/R1552–R1554/R1630/R1656 platform side
- U155: R70–R73, R76–R84, R91 (last-session link), R222, R224
- U156: R26, R27, R46–R49, R53–R69, R225, R288 (deck tree)
- U157: R21–R23 (with U150), R76, R77, R85–R101, R208
- U158: R196–R202, R219, R221–R226
- U159: R24 (File menu), R173–R175, R203–R218, R220, R278–R287, R288 (routing)
- U160: R18 (Note Types entry), R109–R116, R128–R149, R155, R322–R324
- U161: R89, R102–R106, R117–R127, R168
- U162: R50 (import entry), R51, R52, R212, R213, R280
- U163: R150–R152, R170 (maintenance ops), R285
- U164: R11 (flag), R153, R154, R285 (Migaku item)
- U165: R306–R318, R1431–R1438 (UI side)
- U166: R157–R163
- U167: R156, R164, R176–R182, R186–R195, R190 (shell screens)
- U168: R9, R10 (blocked on Xcode 27.1)

R-IDs in my range owned elsewhere
- R183 (heatmap VoiceOver) → Stats UI (U225 area).
- R184, R185 (review Show Answer button, answer button speech) → Review UI (U175 area).
- R190 audits for review, Browse and Stats screens → their areas; cross-screen gate → U240+.
- R227–R277 (review, editor, browse, preview, template editor, image occlusion keys) → Review UI (U175), Notes UI (U190/U195/U200/U205), Browser UI (U210); they register into the U158 engine/catalog.
- R289–R305 heatmap/streak component → Stats UI (U225+); data → Stats core (U95+). Home (U152) and deck page (U155) only host the component.
- R16 performance measurement → U240+ gate (implementation support in U151/U152).
- R1 target creation → U7. R322 deck-record preservation and R323 config preservation are enforced in core (U28, U23); shell only avoids writes.
- Behaviour of settings listed in my pages: R133, R136, R137 → Review (U175); R138–R140 → Notes UI (U190/U195); R141 → Browser (U210); R153 server behaviour → Integrations (U135+).

Cross-area contracts assumed (orchestrator to reconcile)
- U41: `deckStudyState(deckId)` record (tree vs queue counts, next-learning due time, learning-later-today, limit-reached flags with remaining counts, buried counts split manual/sibling, tomorrow's due count, empty flag); deck tree query already applies R31, R34, R39.
- U43: core session summary (answers excluding undone, per-button counts, pass share, new introduced, duration, missed cards in first-miss order with miss counts, plain-text question excerpt, deck name).
- U175: review session model owned at AppNavigation level (not by a view) to survive layout switches; hooks for foreground (queue head refresh) and background (pause timer, stop audio, persist session record); "resume after undo" entry for the summary; answer-bar safe-area inset published for toast placement; web-input focus reported to the keyboard engine; R162 tap-zone tip display.
- U190/U195: editor publishes `hasUnsavedEdits` to the sync coordinator (R1550); Add/edit drafts held at AppNavigation level; Add accepts a forced deck; close-with-save/discard API for the exclusive phase.
- U20: sanitizer + Markdown renderer returning display-safe HTML for deck descriptions and server messages.
- U28: non-undoable op for `curDeck`/`activeDecks` that writes only on change; ops return the final deck name after suffixing; refusal reasons for invalid reparent.
- U23 / U95: typed config API with write-if-different; `heatmap` accessor that patches sub-keys and preserves unknown ones.
- U100: snapshot builder takes the App Group file path, debounces on the change stream, exposes an immediate flush, reports `changed`, and precomputes states for the next two rollovers; a golden snapshot JSON committed under `conformance/goldens/stats/widget-snapshot/`.
- U227: heatmap palettes, level thresholds and grid layout live in the core-free `HeatmapKit` target so the widget can reuse them.
- U22/U29: "create empty collection" (Default deck + stock note types) for Start Empty; protection class inherited from the directory the app creates.
- U6: generated UniFFI protocols usable as a `FakeCore` in AppModel tests.
- U7: Info.plist/entitlement files exist; this area adds keys (scenes, URL scheme, document types, BG task ids, local network usage string, App Group ids per platform).
- U210: Browse accepts an initial search and a `cid:` list; passes a selection to the export sheet. U222: shared Card Info view for the summary preview.
- New MeguriKit target `HeatmapKit` (no MeguriCore dependency) is an addition to the planning-contract Output Structure. Session summary, deck page, deck management and Go to Deck are placed in `Features/Home`; sync, import/export, backups/maintenance and integrations pages in `Features/Settings` subfolders; root layout views and menus in the app target (`app/Meguri/Shell/`, `app/Meguri/Commands/`).

Conflicts / open questions
1. R13 vs R455: R13 says relaunch never resumes mid-review; R455 (review area) reopens review after termination during a session on the same day. Proposed rule: R455 applies only when an unfinished session record exists for today (system termination); Mac ⌘Q ends the session, so R13 applies there.
2. Widget specs disagree. Small widget content differs (R307: 8-week heatmap + streak + today; R1432: streak + today only); planned per R307. Staleness 36 h (R317) vs one day (R1436): KTD16's 24 h used. Empty-state copy "Open Meguri to set up" (R317) vs "Open Meguri to get started" (R1438): planned "Open Meguri to get started"; orchestrator to confirm. Field lists R315 vs R1433: KTD16 union.
3. Resolved in KTD12: backups in `Documents/Backups` on iOS and iPadOS (Files access enabled by U7) and in the private container on the Mac; everything else stays in Application Support. U151 passes the backups path to the core at open.
4. R1630 "checked every 5 minutes" vs R1854 "no polling timers": planned an event-armed one-shot timer (U154).
5. Shortcut collision: ⇧⌘= is both superscript (R249, editor) and ⌘+ card zoom in (R218) on US layouts, and card zoom also applies to the editor preview (R143). R200 forbids both being active. Proposal: in the editor context the zoom items stay in the View menu without shortcuts (same pattern as R238); editor area to confirm.
6. R14 wording ("shared container") is superseded by KTD12 (collection in app-private storage); implemented as commit-on-background via C8.
7. AE68 vs R88/AE9 (sync during review) belongs to the sync area; the coordinator here implements R88/AE9 per KTD6.
8. The Migaku "Recently received" view (R153, R285) is assumed to be built by the integrations area in `Features/Integrations`; this area only links to it.
9. R1435 asks for an entry at "each upcoming rollover"; the snapshot precomputes two, enough for the streak to reach its terminal value. Further entries only shift dates; confirm with U100.

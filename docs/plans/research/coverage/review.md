# Coverage note: review


Coverage map (unit → R-IDs)
- U175: R325, R326 (UI call; write by core startSession), R327, R330, R331, R332, R336, R337, R338, R340, R359–R363, R414, R1817
- U176: R329, R350, R443–R451, R1807, R1808, R1813, R1814, R1815 (targets; suites in U240+)
- U177: R333, R334, R335, R341 (display; counts from U41), R342–R345, R364
- U178: R346–R349, R351, R352, R354–R358, R1809, R1810, R1981, R1982 (display of R353)
- U179: R365–R371, R1811 (and the review side of R1264)
- U180: R401–R413, R415, R416, R1816
- U181: R227–R244, R339, R373, R417, R418, R419–R428
- U182: R374–R383, R387, R392, R393, R1812
- U183: R384–R386, R388–R391 (review side of R241, R1262, R1263, R1422)
- U184: R394–R400
- U185: R429–R433
- U186: R328, R372, R434–R442
- U187: R452–R458
- U188: R459–R464
Every R-ID in R325–R464, R227–R244 and R1807–R1817, plus R1981 and R1982, is covered except as listed below.

R-IDs in my range owned elsewhere
- R353 (interval strings byte-identical to Anki, oracle-verified): formatting is owned by the Scheduling area (U45 next-state labels, delivered in U43 head items; scenarios under conformance/scenarios/sched/labels/). U178 only displays the core strings verbatim and tests that it does not reformat them.
- R341's count rule (v3 reviewer counts, learning counted in cards) is computed by U41; U177 owns the display formula.
- R326's write (curDeck as a synced change) happens inside the core startSession op (U43); U175 only calls it.

Rendering-environment R-IDs (Templates area) that are implemented in Review UI files. The orchestrator should either reassign them to Review or confirm "consume":
- R571 (menu item and ⌘+/−/0 wiring only; zoom mechanics stay in U67), R573 (review chrome Dynamic Type), R574/R575 (card view frame between the bars), R576–R579 (layout continuity, one-row buttons, keyboard and rotation changes no state), R580 (button labels and VoiceOver focus after reveal), R581 (toast behaviour). The units cite them as "consumed"; they do not claim them.
- Shell-owned rules that review depends on: R22, R90, R91 (summary rules), R101, R208, R284 (shell's Card menu, which binds to U181's `ReviewActions`), R88/R89/R1549 (sync cadence and the session-end sync in U151), R215/R173 (global undo menu).

Cross-area contracts assumed (to reconcile)
1. C5 (U43), beyond the spec-flow table:
   - `startSession(deckId, resumeCardId?)` sets curDeck, cancels a running optimizer and returns a session id, head(3), counts by kind, `validUntil` and `timeLim`.
   - Head items carry ids, kind, `stateToken`, four next states with label strings plus interval seconds, predicted next card id per rating, question/answer HTML, AV tags, media refs, flag/marked, and per-card home-preset values (autoplay, replayq, timer show/stop, max answer secs, leech action, auto-advance values).
   - `answer(...)` returns an outcome (accepted{leech, suspended} | stale), head diff, counts and rolledOver.
   - `refreshCard(sessionId, cardId)` returns a fresh package.
   - Card ops issued with a session id patch the queue in place and return head diffs (R1812).
   - `endSession(sessionId)` is called at summary dismissal.
   - Events: `QueueHeadChanged`, `CurrentCardChanged{eligible|removed|memoryOnly}`, `CommitFailed`.
2. U41: congrats info for a deck (next learning due + count, review/new limit reached, buried kinds, description + md) and unbury by mode scoped to a deck tree.
3. U48 card scheduling ops: bury/suspend card and note, forget (writes `restorePositionReviewer`/`resetCountsReviewer`), set due date (validator returning error text; writes `setDueReviewer`). U30: flag (with `flagLabels`), mark. U27: delete note.
4. U25 (C3): `peek(sessionId)` with disabled reasons; `undo(sessionId)` scoped to session steps; undo/redo results return the affected card id; core-side 30-step depth and the no-uploaded-step invariant.
5. U67:
   - two persistent page slots with side update (question/answer), z-order present in one CATransaction with completion, and a double-rAF painted confirmation;
   - preload into a slot without touching `#qa`; typed-text read/restore; scroll get/set;
   - `_drawFlag`/`_drawMark` calls; night-mode and zoom application;
   - process-termination callback per page; script-error capture per card;
   - an extension point to install review-host.js in a dedicated WKContentWorld with an allowlisted message handler (R2038);
   - a context-menu customization hook (Mac right-click, iPad secondary click);
   - audio player queue with completion callbacks.
   U66 owns the page-world runtime in `app/Packages/MeguriKit/Sources/CardRendering/Resources/CardRuntime/`. review-host.js lives in the same directory but is owned by Review (U176/U180/U181).
6. U151/U150:
   - `ReviewCoordinator` lives under `AppModel/Review/`;
   - U151 owns the single `nextDayAt` timer and the time-zone notifications, and reads `ReviewCoordinator`'s session-open flag for the hold-back, one-way deferral (R458) and session-end sync;
   - U150 launch routing reads `SessionSnapshot` (R455), provides AppNavigation sidebar collapse/restore and tab-bar hiding, and drops New Deck's ⌥⌘N during review/Browse (R238).
7. U160 Settings exposes the device-local Double Tap Prevention (0–1000 ms, default 250), "Space and Return answer Good" (default on) and the answer-key bindings with conflict rejection.
8. The App shell's summary screen consumes `SessionRecord`, calls `ReviewCoordinator.reopenAfterUndo()` (R99/R371) and `dismissSummary()`, and uses `ReadOnlyCardPlayer` for missed-card previews (R97/R442).
9. Embeddable views: U222's shared Card Info view with a Current/Previous switch (data U92); the U232 deck options view for a given preset with a deck-named title; the U190 editor component; the U195 Add editor prefilled for Create Copy; U152's sanitized deck-description view.
10. U9: `rating.again`/`rating.good` tokens shared by the answer buttons and the zone tint.
11. Spec writer: `specs/review/reviewer-strings.md` (Anki wording for toasts, leech, timebox, congrats notices, unbury options, the external-change notice) — the only Anki-derived input to the Review UI units (KTD1).
12. Package layout assumption: Swift target `Review` at `app/Packages/MeguriKit/Sources/Features/Review/`, tests in `app/Packages/MeguriKit/Tests/ReviewTests/` (same pattern as Browse's U210).

Conflicts and open questions
1. Tap path vs R1807: R407 requires the reveal/answer decision after the card's own handlers, in web content. That puts one web-content→UI-process hop on every tap answer. At 120 Hz this may cost the earliest frame. U180 starts with a device spike. The fallback options (stage the swap at native touch-up and commit on confirmation; or decide natively from the hit map and accept missing `preventDefault` handling) each change a requirement, so a product decision is needed if the spike fails.
2. R1816 vs R403: same-frame tint at touch-down needs a native, synchronous exempt check. U180 uses a hit map that the isolated script publishes. A stale map, for example during an image load or a script-driven DOM change, can briefly tint an exempt target before it is corrected. A strict "never" may be impossible together with same-frame tint.
3. R348/R1810: native buttons and web content paint in different processes. Showing the buttons on the page's "presented" message may land one frame apart from the web paint. The spike S1 measurements in U67 should include this; otherwise R1810 needs a tolerance.
4. R724 says redo is in the review ⋯ menu on iPhone, but R374's ordered menu list has no Redo item. Proposal: add "Redo <op>" after Auto Advance on iPhone only. This needs a decision; U182 currently follows R374.
5. R365 literally re-shows the affected card on its question side after undoing flag or mark on the current card, which forces a re-render. U179 follows the literal text; confirm whether an in-place revert is preferred for flag and mark.
6. R398 literally re-renders on edit-panel close even when nothing changed. U184 follows it; confirm.
7. R461 with congrats shown at rollover: U188 recomputes congrats and offers Continue when new-day cards exist, rather than auto-resuming. This reads R437's "never resumes on its own" as applying here too; confirm.
8. R359 cap: applied once, in the UI (U175). The scheduling area must not apply a different cap in the core.
9. R340 on Mac is read as "display sleep"; Macs have no auto-lock apart from screen saver and display sleep.
10. Spec-flow gap 4 (AE68 vs R88) is not in this area, but U179's AE9 handling assumes R88's 60-second cadence during review.
11. R325's exit list does not mention D/B/T (R90), although R90 ends the session. U175 adds `navigationKey` as an end reason; the two lists should be aligned in the requirements.

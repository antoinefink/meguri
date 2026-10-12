# Coverage note: sched


R-ID coverage map (unit → requirements):
- U40: R592–R599, R760 (read side), R464 (core side)
- U42: R584–R591, R629 (stored fields), R637, R647, R648, R653, R753, R762, R765; data for R739–R751, R754–R759 (preset and config fields, `timeLim`)
- U41: R602–R620, R623 (classification), R624–R626, R628 (data), R630, R631, R707 (exclusion), R767 (core counts)
- U45: R587 (deltas), R601, R636 (minute semantics), R638–R643, R646, R649, R651, R652, R654–R664, R670, R671 (predicate), R656
- U46: R649 (revlog fallback), R650, R685–R690 (R688 rule, R690 count), R697 (no rescheduling)
- U44: R587 (write), R589, R591, R632, R633, R665–R669 (behavior), R671–R678, R681, R683, R684, R721 (data side), R764
- U43: R600, R620 (presentation), R621, R622, R623 (execution), R627 (data), R680, R720 (session side), R721 (queue side), R722, R726 (session tagging), R727–R729
- U48: R634, R635, R682, R683, R706, R707, R708–R718, R719 (names), R768 (core API)
- U49: R636 (parse and validate), R644, R645, R665 (stored toggle), R686 (triggers), R690 (validation), R731–R735 (data and ops), R737, R739–R753, R760, R761
- U47: R691–R700, R701 (data, Optimize now)
- U50: R702–R705, R763, R764 (verification)
- U51: R597 (app triggers), R698 (platform scheduling), R701 (view), R702/R703 (sheet), R704 (copy)

R-IDs in my range owned elsewhere (the core API they call is listed in parentheses):
- R627 progress bar UI, R673 leech notice UI, R679 answer-timer pausing, R680 UI rollback, R754–R758 timer/auto-advance/audio behavior, R759 timebox prompt → U175 Review UI (C5 from U43; leech flags from U44; display options from U43 head; `timeLim` from U42).
- R628 finished-state UI, R766 iPhone tap-to-review, R767 deck page UI → U152 Home/deck page (U41 counts and finished facts, U43 session).
- R768 menus and keyboard shortcuts → U175 (review ⋯ menu), U210 (browser), U150 (Mac menu bar) (U48 ops).
- R719 undo depth (30, memory only), R720 undo/redo mechanics, R725 history clearing → U25 undo manager. I supply op names, the answer undo metadata, and clear calls (rollover from U40).
- R723 review undo button, R724 ⌘Z/⇧⌘Z and menus → U175 / U150.
- R726 sync cadence and hold-back → U107 (uses the U43 session id tagging and `endSession` scope).
- R730, R736, R738 screen placement, sections and layout; R731–R735 UI controls and badge → U232 (U49 snapshot, validation and save).
- R760 settings rows for rollover and learn-ahead, and the R665 toggle placement → U160 Settings and U232 (U49 prefs op).
- R688 Check Database invocation → U128 (U46 lrt repair rule).
- R590 card creation positions → U27 (U42 allocator).
- R645 trigger on deck change → U27 set-deck op (U49 hook).

Cross-area contracts assumed:
- C5 is defined in U43: startSession, head(n) with stateToken, answer, endSession, and the events QueueHeadChanged, CurrentCardChanged{eligible|removed|memoryOnly}, CommitFailed, WaitingUntil, Finished, PresetUnsupported. Labels for prefetched heads can change after each answer (load balancer), so U175 must use the latest head event's labels. The `Accepted` result carries them synchronously.
- C6 is defined in U40: today, nextDayAt, checkRollover(), and RolledOver{oldDay,newDay} as a C2 change-stream variant. U107 calls checkRollover after each sync; U51/U151 call it on foreground and at the timer.
- U24 (C1/C2): op sources include `Rollover` and `Optimizer`; a test-hook fault injection for commit failure; a ChangeSet flag for memory-state-only card changes.
- U25 (C3): undo entries carry a session id and op-specific metadata (answer: card id, from-queue kind, load balancer entry, learning cutoff); a `ClearedByRollover` reason; undo and redo results are observable by the engine session.
- U23: a raw queue update without an mtime/usn bump (rollover unbury, R599); typed config access.
- U22: registers the scheduler SQL functions from `meguri-storage/src/sqlfns/sched.rs` (fnvhash, FSRS extractors) on every connection; U80 reuses them for `prop:` searches. U22 open calls U40 for new-collection timing and U50 for the v1 refusal and v2→v3 switch.
- U27: card generation uses the U42 position allocator; the set-deck op calls the U49 preset-change hook.
- U28: deck and deck-config record storage; U49 orchestrates the save and U42 patches fields.
- U29: the schema-change confirmation gate for preset removal (R733).
- U30: tag add inside the answer transaction (leech).
- U80: executes `preset:"<name>" -is:suspended` and `weightSearch` for U47/U46.
- U107: a sync result event carrying a server-state marker, a meta-only probe for the optimizer gate, and a sync-2 request API; hold-back uses U43 session tags.
- U26: an optimizer diagnostics table, and an optional deck-count cache keyed by (day, col mod).
- U12/U13/U3/U4: bench entries (memory recompute, optimizer, queue build), the two-client sync rig, oracle record/compare, and the scenario runner. The aggregate matrices R1890–R1900 (U240+) are built from the scenario dirs named here.

Conflicts and open questions:
1. Random-number crate version. R660 and R2027 say rand 0.10.1. The oracle research (oracle-harness finding 2) found that at tag 26.09.3, Anki's scheduler fuzz uses rand 0.9.4 (0.10.1 is pulled in by fsrs 6.6.2 and by later `main` commits). U45 pins the scheduler's version at the tag through a renamed workspace dependency, separate from fsrs's own rand. The 10k-pair table decides. R660 and R2027 should say "the version Anki 26.09.3's scheduler uses".
2. Apple libm vs glibc f32 results (oracle-harness finding 1). About 0.6 % of powf/expf calls differ by 1 ulp, so R1890's exact match can fail on rare boundary cards on macOS and iOS. This affects U45, U46 and U41 (retrievability orders). The plan runs T0 natively on macOS from week 1. The fallback is a fsrs-rs fork that calls a vendored port of ARM optimized-routines math, or R1889 deviation entries. The orchestrator should choose a default.
3. R699 vs KTD8. R699 says to abandon without writing if either sync fails, and to start "only right after a successful sync". U47 follows KTD8: after sync 1 is a fresh meta gate; if sync 2 fails, the write stays. R699 needs the amendment.
4. R725 ("a sync that applies changes clears the whole undo history") vs KTD7 (keeps the held-back run). U43 and U44 assume KTD7; U25/U107 own the rule.
5. R645 says a deck change "recomputes its FSRS memory state". The research suggests Anki clears the memory state on a deck change and recomputes it lazily. The spec writer must check this against the oracle: if Anki clears it, the card rows after set-deck differ from the goldens unless R645 is recorded as a deviation (R1889) or reworded.
6. R727 strictness on memory-state-only changes (spec-flow minor gap 18). Followed literally: after another device's optimization, the user sees "Card changed, please answer again" even when the labels are identical. A possible refinement is to accept the answer when the fresh labels equal the displayed ones. This is not adopted; the orchestrator should decide.
7. R594 (writing `localOffset`) vs R725 ("any action that changes data outside undo tracking clears undo"). U40 assumes the offset write does not clear undo unless the day number changes. Otherwise every time zone change would empty the back button. The spec writer should confirm what Anki does.
8. Non-deterministic draws in Anki (set due date `N-M`, Random insertion-order reshuffle) cannot match bit for bit. Their scenarios use range or set comparison (`compare: set`), as oracle-harness §4 allows. Strictly, this is an exception to R1890/R1896.
9. R700 health results and R701 diagnostics are device-local (U26 derived store, KTD12), so another device's optimization shows only through the synced `lastFsrsOptimize`. The diagnostics view should say "on this device".
10. Rayon pool. KTD8 calls for the global pool at utility QoS, because the fsrs health check uses the global pool. bridge-build.md suggested a bounded 2–4-thread pool. U47 follows KTD8.

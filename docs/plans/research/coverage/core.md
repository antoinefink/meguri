# Coverage note: core


R-ID coverage map (unit → requirements)
- U20: R771 (encoding), R773, R774, R779, R979 (pure), R987, R988, R1457 (computation); helpers for R891, R1451, R989.
- U21: R322 (model), R903–R904 (model), R1456, R1459–R1465, R1484–R1486.
- U22: R1449, R1451, R1452, R1454, R1478–R1480, R1482, R2013, R2016.
- U23: R323, R324 (core side), R772, R776/R906 (storage no-op), R1456, R1466, R1468–R1474.
- U24: R171 (core), R727 (mechanism), R1490, R1494, R1495, R1658 (core), R2002.
- U25: R719, R720, R721, R725 (per KTD7), R1795, R1796 (per KTD7), R2020.
- U26: R1487–R1489, R2016.
- U32: R1450 (core), R1453, R1454, R1491–R1493, R2002, R2004, R2006 (core), R2007, R2011, R2012, R2014, R2015.
- U33: R1481.
- U29: R903–R913, R949–R951, R1475–R1477, R1485, R1586, R1590; core side of R916, R917, R921, R942, R943.
- U30: R770, R891–R899, R901, R1467.
- U27: R769–R795, R1457, R1458; core side of R801, R807, R808.
- U28: R322, R913 (bootstrap), R1465, R1474, R1475, R1486; core side of R54, R55, R60, R62, R63, R65–R69, R161, R732.
- U31: R978–R988, R1587, R2018; core side of R1582, R1702, R1855.
- U34: R789, R914, R949; core side of R918, R920, R922–R924, R926, R927, R932–R934.
- U35: R944–R948, R952.
- U36: R2017, R322–R324, R582, R583, R1483–R1486, R1586, R1587, R1590, R2016 (verification); AE66, AE83.

R-IDs in my ranges owned (wholly or for their non-core part) by other areas
- R722, R728, R729 → U43 (review session); R723 → U175; R724 → U150/U175; R726 → U107 (uses U25 hold-back API); R727 answer-specific precondition → U43/U44 (mechanism in U24).
- R582 implemented by U43, R583 by U60; U36 verifies both.
- R898 shortcuts, R900 → Browse/Review UI (U210, U175); R902 → U80 + U175.
- R915–R943 screens → Notes UI (U200); only their core ops are here (U29, U34).
- R989 → U130 Check Media (U20 provides the reference scan; templates deliberately not scanned). Empty Cards maintenance (R1652–R1654) → U131 on top of U35.
- R1455 → U151 (Swift creates the data directory with the protection class; U32 keeps every file inside it).
- R1588 → U111 media sync (U31 journal supports resumable dirty tracking).
- R2003 → U240+ fault-injection suite (U32 provides failpoints).
- R2005 → U109 (uses U32 integrity check and atomic replace).
- R2008, R2010 → U127 (U25 catalog + pre-op backup hook called by U28, U34, U131, U128, U109, U124); R2009 → U126.
- R2019 → U107/U109.
- R2011/R2012 scheduling and UI → U151 and KTD15 background tasks (core primitives in U32).

Cross-area contracts assumed (orchestrator to reconcile)
1. U60 exposes the parsed template AST publicly, with replacement nodes (field name, filters), conditionals (normal/negated) and source spans, so U27 evaluates card existence and U34 rewrites references without re-implementing parsing. U61 exposes cloze-number scanning over field text.
2. U21 Clock: `now_ms`, current UTC offset and IANA zone id injected from Swift (and from the scenario clock); U40 builds the day clock on it; the tz-database question raised by the stats area stays with U40/U95.
3. U6 exports: op submission returning `OpResult`, change-stream callback (id-based, `seq`-ordered, with `source` and card→note ids), lifecycle state and waits, CancelToken, progress (with phase), undo `peek()`/undo/redo, suspend/resume, free-space provider callback, integrity check.
4. In-core observers (U24) run after commit and before publishing, with row payloads; consumers U26, U41, U95, U100 (matches the stats area's ask).
5. U43/U44: `stateToken` is a hash of the card row; the answer op carries it as a precondition (R727); U43 registers undo restorers for in-memory state (R721) and keeps answers in its overlay when the executor reports a long write phase.
6. U107: consumes `held_back_run` (touched set + before-images), reports sync start/commit so U25 marks uploaded entries and clears per KTD7, commits staged incoming as one SyncIncoming op, checks `Failed(Corrupt)` before uploads, and registers as a C8 participant (cancel on suspend).
7. U40 calls U25's rollover clear and emits `RolledOver` on the C2 stream; U32's resume order calls U40's rollover check after the external-change check.
8. U100 registers as a C8 suspend participant and a C2 observer; U151 supplies the App Group path and calls flush at session end.
9. U127 provides the pre-op backup hook required by R2008 ops (deck delete with cards, notetype delete, Empty Cards deletion, Check Database, one-way sync, `.colpkg` import).
10. U3 oracle-maintainer produces committed fixtures at schemas 14–17 using historical `anki` releases, plus refused-version fixtures and the preservation kitchen-sink fixtures.
11. U22 connection-init hook lets U80/U81/U41 register SQL functions (matches the search area's ask).
12. Derived components (U41, U80/U90, U84, U95, U130) register with U26 and keep a fallback path while `Building`.
13. U137/U139/U141 add notes with source LocalApi (no Add-screen defaults, R801 via spec-flow Q13), use U31 staging for `/receive` atomicity and `ensure_static_media` for fonts, and use U24 waiters with their timeouts. Composite ops (create + delete) are one undo entry (servers area ask) — supported by U24/U25, though LocalApi ops are not recorded in undo per R1795; the servers area's "move" is a user action (UserUndoable) and is recorded.
14. U195 flags Add-screen adds so U27 writes R801 defaults; U190 consumes the field-check op (R807).
15. Change Note Type lives in U35; U87 (search) and U203 (Notes UI) depend on U35. Empty Cards maintenance (report, delete op wiring, backup) is U131, which depends on U35.
16. U31's media journal format and `MediaArrived` event are consumed by U111; the media lock is shared by U111 and U130.

Conflicts and open questions
- R1451 says "Unicode simple case folding"; Anki's pinned `unicase 2.6.0` does full folding (`ß` = `ss`). Meguri follows the crate and the oracle; the requirement wording should be corrected.
- R1495 / AE68: KTD6 commits answers immediately during a sync (no queue). AE68's "committed right after the sync finishes" should read "committed immediately and uploaded on the next sync"; its second clause contradicts R88/R726 (spec-flow critical gap 4).
- R725 vs R1520/R365: resolved by KTD7 (sync clears all but the held-back run); R1796's "top entry" becomes truncation at the deepest intersecting entry (KTD7). Requirement text should be patched.
- R1490 (one transaction per user op, including import) vs R2006/R1817 (answer durable within 1 s): a long single-transaction write (100k-note `.apkg` import) delays answer commits. U24's plan/commit keeps most write phases short; for declared long writes the answer stays in U43's overlay and becomes durable right after. If that is not acceptable, imports must be split into multiple transactions, which breaks R1490 literally.
- R14 says "shared container"; KTD12 keeps the collection in app-private storage. R14 wording should be corrected.
- R781: undo of a note deletion restores rows and removes graves; the deleted-notes log is append-only and keeps the entry. The spec writer confirms Anki's behavior; if Anki removes nothing either, no conflict.
- R1470: the exact cap rule when the clock jumps backward is defined by the spec writer from the oracle; the requirement does not say what happens if `max(id)+1` would exceed now + 1 day.
- R1482 (refuse v1 scheduler) must also apply to a downloaded collection; U109 should call U22's probe on the temp file.
- R2014 vs R1452: implemented with an advisory lock file plus a busy probe; it cannot detect a non-locking external SQLite writer (unsupported per Scope Boundaries; R1453 detects its changes afterwards).
- Schema 14–17 upgrade fixtures depend on older `anki` wheels still installing in the oracle image. If they do not, R1478 for those versions can only be verified with spec-writer-built fixtures, a weaker oracle.
- R913 adds Image Occlusion to new collections, unlike Anki's default set; the bootstrap golden comparison therefore excludes stock template text and expects one extra notetype.

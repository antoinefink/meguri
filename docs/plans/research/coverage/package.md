# Coverage note: package


R-ID coverage map (unit → requirements)
- U120: R1595 (core), R1596 (shared with U124), R1597–R1604, R1607–R1616, R2004 (helpers), R2015 (preflight helper); AE67; test-enforces R1846, R1852.
- U121: R1605, R1606; import-merge cases of R1475, R1476, R2008.
- U122: R1617, R1618 (core), R1619 (latest), R1620–R1623, R1624 (core output); test-enforces R1922, R1936.
- U123: R1481, R1619 (legacy), R1625 (legacy option).
- U124: R1596, R1625–R1628, R2004, R2005 (replace gate), R1489 (trigger), R1548 (flag only).
- U126: R1629, R1633, R1634, R1635, R2009; AE69; test-enforces R1848.
- U127: R1630, R1631, R1632, R1637 (core), R1638, R2008, R2010, R2020 (backup side), R1662 (backup side), R1539 (provider).
- U128: R1639, R1640, R1641, R1642 (record parts), R1643 (record parts), R688 (co); test-enforces R1924, R1925, R1847.
- U129: R1642 (note and tag parts), R1643 (note and notetype parts), R1644; R780 co-covered.
- U130: R1645–R1651, R2018; AE79 co-covered.
- U131: R1652, R1653, R1654; core for R952, R1015.
- U132: R1655, R1656, R1657 (data), R2011, R2012; R2003 co-covered.
- U133: R1616 (contract), R1624 (handoff), R2015 (provider); co-covered R170, R171, R1494, R1660, R1661.

R-IDs in my range owned elsewhere
- R1600: the sheet UI is U160; the preview data comes from U120.
- R1617 entry points: deck menu U152, browser U210, collection export U160. R1618 "remember last choices": U160 (device-local UI state).
- R1624 share sheet and save panel: U160, using U133's handoff.
- R1636 settings note: U160. R1637 restore list UI: U160. R1657 Maintenance section UI: U160.
- R1658: U24 (op executor, sources) and U135/U137/U141. R1659: U24 (executor queueing during sync) and U135.
- R1660, R1661 platform placement: U150/U160.
- R1662 sync-on-quit: U151. Its backup side is U127.

Cross-area contracts assumed (to reconcile)
1. Layering: `meguri-engine` defines a `BackupService` hook trait. `meguri-package` implements it, and `meguri-ffi` registers it at open. KTD2 puts package above engine, so engine ops (U28 delete deck, U34 delete notetype, U29 schema-change confirmation, U109 one-way sync) request safety backups only through this hook.
2. Op descriptor flag "safety backup before" in U24's executor (R2008). U28, U29, U34, U109, U121, U124, U128, U130 and U131 set it.
3. Verified collection replacement (`lifecycle/replace.rs` with `ReplaceReason`) is provided by U124. U109's full download consumes it (R1543/R2005). If U24 already owns a replace primitive, U124 moves into it.
4. Device-local maintenance sidecar in the app-private collection folder (KTD12), never in the collection. It holds `oneWayChoicePending`, last-backup `mod`/time, last check/optimize/quick-check results and the unclean-shutdown marker. U151 and U107 read `oneWayChoicePending` to pause auto-sync (R1548/R2010).
5. U24 long-operation slot (R171) is separate from lifecycle `ExclusiveBusy` (KTD17):
   - The slot serializes one-way sync, restore, `.colpkg` import, Check Database and `.apkg` apply.
   - `ExclusiveBusy` (UI disabled, collection replaced) covers only the replacement ops.
   - Local servers (U135) wait on either one, which matches R1706 and R1797 (both list Check Database).
6. U4 scenario format additions: `media_files` setup, ops `check_media`, `empty_cards`, `backup_force` and `retention`, observations `media_dir`, `import_log`, `package_listing` and `backup_files`. Oracle-made fixture packages go in `conformance/fixtures/package/`.
7. U25 must accept large import undo entries (10k+ rows). An undone import leaves copied media on disk (U31 agrees: orphans are reported by Check Media).
8. Temp and staging naming (`Staging/` directory plus a fixed temp suffix) is shared with U22/U24's R1493 launch cleanup.
9. U107/U109 check the lifecycle `Failed(Corrupt)` state before `meta` and never upload a corrupt collection (R2012). U109 calls the U132 optimize primitive and the U127 forced backup before one-way transfers.
10. U31 provides the trash primitive. Every media-removing path (U111 server deletions, U141 `deleteMediaFile`, U130) routes through it (R2018). U111 honors the C9 media lock held by Check Media.
11. U151 sends maintenance triggers (open, background, quit, BGProcessingTask) to `run_due_maintenance` and wraps user-started imports in `BGContinuedProcessingTask`. U7/U150 declare the `.apkg`/`.colpkg` UTTypes.
12. The fault-injection suite harness (R2003) belongs to U240+. Kill-point failpoints for package and maintenance ops (`test-hooks` feature) are added in U120, U124, U126, U128 and U132.

Conflicts and open questions
- A. Resolved in KTD12: backups live in `Documents/Backups` on iOS and iPadOS (Files-visible, R1629) and in the private container on the Mac with a Show Backups action. The core takes the folder path as a parameter (U126); U151 supplies it, U7 enables Files access, U163 shows it.
- B. R1630 ("checked every 5 minutes") conflicts with R1854 ("no polling timers"). U127 resolves it with an event-armed one-shot deadline. The observable outcome matches R1630.
- C. R171 lists Check Database and import as exclusive operations. KTD17's exclusive phase (UI disabled) covers only replacement operations. Resolved as contract 5 (slot vs `ExclusiveBusy`); the orchestrator should confirm.
- D. Answer durability within 1 s (R2006, R1491) is at risk while a long single-transaction maintenance write holds the writer: Check Database vacuum and repairs (up to 5 s, R1847), and possibly large `.apkg` applies. U120 keeps its write phase under 1 s by preparing everything outside the transaction, but VACUUM cannot be split. U24 must define the behavior. Options: answers wait in the in-memory overlay with a sub-second durable journal, or vacuum runs only when no session is active (Check Database is user-started, so a "pauses study for a few seconds" note may be acceptable). Not resolved here.
- E. Resolved: Empty Cards is U131 (maintenance report and ops) on top of U35 (generation-side scan); every Empty Cards reference now cites U131.
- F. R2008 interpretation:
  - "any operation that forces a full sync" is read as the op at the R1476 confirmation point (the first schema change since the last sync), not every later schema change.
  - A failed safety backup refuses the op (`SafetyBackupFailed`) rather than proceeding. Product authority may prefer an "Proceed without backup?" confirmation.
- G. The behavior of R2008's backup before Delete Unused media is unclear, because a collection backup does not contain media. It is kept as required. The media itself is protected by the trash (R2018).
- H. Details deliberately left to the oracle and recorded by spec writers: cloze/standard merge abort scope (R1606), colpkg same-name-different-media handling (R1627), the kept card for "Keep notes with no valid cards" (R1653 vs R952 "first card"), whether import option choices are written back to config, and undo of a merge import.

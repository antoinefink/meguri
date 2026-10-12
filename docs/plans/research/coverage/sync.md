# Coverage note: sync


R-ID coverage map (unit → requirements)
- U105: R1496 (core), R1497 (Rust), R1498 (core), R1499 (core), R1500, R1501 (core), R1502 (core holds no endpoint), R1504, R1505–R1512, R2030, R2031–R2032 (Rust); AE70 (half).
- U106: R1513–R1518, R1559 (computation), R1561 (classification), R1499 (kind), R1516 (pass-through), R1562 (core), R1464 (sync use), R1523 (shapes); AE63.
- U107: R1519–R1533, R1535, R88, R726, R1520, R1495, R1845, R1554 (normal), R2019 (scm on mismatch), R2006 (not delayed by sync); AE61, AE64, AE68 (clause 1), AE9 (core), AE27 (core).
- U109: R1518, R1536–R1548, R1563, R1564, R2019, R158, R1593, R1554 (one-way), R1655 (upload compaction), R2015 (full sync); AE62, AE86 (sync).
- U110: R1567 (sync state), R1573, R1574, R1582, R1583 (check), R1502 (media reset), R1855.
- U111: R1534, R1546, R1565 (upload exclusion), R1572, R1575–R1581, R1583 (progress), R1584 (core), R1585, R1588, R1592 (core), R1554 (media), R1844; AE65.
- U113: R1549, R1550, R1555, R1556, R1558, R1559 (emission), R1560 (data), R1548, R1534 (trigger), R88/R726 (cadence), R89, R458, R123, R124, R127 (decision); AE9 (cadence).
- U114: contract C4, R1494, R1857, R1535/R1545 delivery, R1558–R1560 data, R1562, R1496 list, R1861.
- U116: R1497, R2030 (check), R2031, R2032; AE89.
- U117: R1566, R1589, R1590, R1586 (guard), R1926–R1931, R1933, R2017 (sync); AE66, AE83.
- U118: R1932, R1554, R2003/R2015 (sync slices), R1842, R1844, R1852 (sync), R1936 (sync idempotence); AE86.

R-IDs in R1496–R1594 owned (fully or in part) by other areas
- R1503 (one collection per account wording): App Shell, U160.
- R1501 (Local Network permission prompt and `NSLocalNetworkUsageDescription`): U7 (Info.plist) and U160 (explanation). The core only allows plain HTTP.
- R1496, R1498 (Keychain storage), R1499 (clearing the key and re-login UI), R1502 (clearing key and endpoint on server change or logout), R1504 (setting UI): U151/U160 using U114. The core parts are listed above.
- R1516 (displaying the server message as sanitised rich text, R168): U151.
- R1551 (Sync button and Y key): U150/U160 (R102, R207); U113 supplies "manual allowed".
- R1552, R1553, R1557 (beginBackgroundTask, BGAppRefresh/BGProcessing scheduling, Mac quit sheet with "Quit now"): U151 (KTD15), executing U113 actions with U107/U111 cancel semantics.
- R1558, R1560 (glyph and status view UI): U151/U150/U160 consume the U113/U114 model.
- R1568, R1569, R1570, R1571 (media filename normalisation, dedupe, case handling, no rewrite of unchanged files): U31 (media store). The R1567 index is U31's store; U110 adds only sync state. The orchestrator should confirm U31 lists these.
- R1565 (telling the user when an oversized or empty file is added, and in Check Media): U31/U190 (R856) and U130. U111 only excludes such files from upload.
- R1585 (UI note where media is deleted): U130 and the editor. U111 implements the behaviour.
- R1587 (bundled `_` fonts added only if absent or different): U137, via U31 dedupe. U117 asserts no churn (AE66).
- R1586 (no rewrites on launch or timer): implemented by every writer (U137, U141, U29). U117 owns the guard test.
- R1591 (first-launch three paths) and R1592 (UI side): App Shell onboarding (U150 range, R157). U109 provides the empty-local download; U111 provides background media.
- R1594 (`deleted.txt` log): U27 (note deletion op; same behaviour as R781).
- R2033 (store text): release area (U240+).

Cross-area contracts assumed (please reconcile)
- U24 (engine):
  - a dedicated read-snapshot reader that can hold one read transaction for a sync job;
  - op source SyncIncoming, with ChangeSet emission;
  - attaching a scratch staging file to the writer during one commit transaction;
  - ExclusiveBusy, CollectionReplaced and generation for one-way syncs;
  - CancelToken and throttled progress callback types;
  - C8 `prepareForSuspend` cancels a running normal sync.
- U25 (undo):
  - full row before-images for every row an entry touches, including deck rows (today counters) and config keys;
  - a query for the held-back run (top-of-stack entries of the active session down to and including the latest answer);
  - per-entry upload-pending and uploaded marks; disabled reasons Syncing and AlreadyUploaded;
  - KTD7 clearing that keeps the held-back run;
  - the invariant that no uploaded step is undoable.
- U43 (session):
  - an active-session registry queried by the sync job;
  - session end defined as summary or congrats dismissal (KTD6);
  - session started and ended events for U113;
  - a background queue refresh on SyncIncoming that keeps the current card and unchanged (cardId, stateToken) pre-renders.
- U40: the C6 rollover check and unbury, called by the sync job before `start`; rollover runs before sync on foreground return.
- U22: `VACUUM INTO`, integrity check, temp-verify-rename helper; the launch sweep also removes `sync-stage` scratch files and one-way temp downloads.
- U26: incremental update from SyncIncoming ChangeSets; revalidation (not a full rebuild) after the upload swap, where ids are unchanged.
- U31: write journal, NFC normaliser, trash, media lock shared with U130 and U111, MediaArrived(names), and a pending-download lookup for the media scheme handler (U67).
- U130: exclude pending-download names from missing-media reports and tagging.
- U127: a forced-backup API (R1539, R2008).
- U126:
  - a place in the backups folder for the server-copy `.colpkg` (R1540) that Restore lists and retention thinning never deletes.
- U124: a `.colpkg` writer usable on a downloaded raw collection file.
- U47: a meta probe for the KTD8 write gate; U113 emits "run optimizer check" only after a successful incremental sync with no session (R699).
- U6: XCFramework variants built with and without the `ankiweb` feature (U116).
- U7/U160/U150: AnkiWeb strings in a separate, excludable String Catalog.
- U4: the scenario `sync` op gains `client` and `kind`, and multi-client observations.
- U13:
  - a fresh server user per scenario;
  - oracle clients with desktop-like and AnkiMobile-like version strings;
  - the server port published on 127.0.0.1 for host Swift tests;
  - the toxiproxy service in the `sync-rig` profile (U13 provides it).
- U151: Keychain and device-state persistence (endpoint, redirected endpoint, auto-sync pause flag, preferences); translating platform events into U113 inputs; the one-way choice sheet from U109 facts; closing editors before ExclusiveBusy (KTD17).

Conflicts and open questions
1. AE68, second clause ("given a review session is active, no automatic sync starts until it ends") contradicts R88, R726, R1550 and AE9. This plan follows R88 and KTD6 and tests only clause 1. The requirement text should be amended (spec-flow Q4).
2. R725 ("a sync that applies changes clears the whole undo history") and R1520 ("discards cached queues") conflict with R365, R457 and R1807. They are resolved by KTD7 (keep the held-back run; refresh queues in the background, keeping the head). The requirement text should be amended.
3. R1519 ("inside one local transaction") and R1495 ("answers queued until the sync finishes") are replaced in mechanism by KTD6. Incoming changes still commit atomically in one transaction, and answers commit immediately instead of being queued, which is stronger than R1495's intent. The requirement wording should follow.
4. R1530 ("no row still has `usn = -1`") cannot hold on the live database with hold-back. U107 checks it against the virtual outgoing state, and the held-back rows and rows written during the sync legitimately stay pending.
5. R2019 asks for a comparison including "last review" on both sides. The server's last review time cannot be observed without downloading its collection. The facts show server `mod` plus the server counts from the `bad` sanity reply, and local last review.
6. R1549 says sync on any return to the foreground; R123 says only after at least 5 minutes away. U113 uses R123's threshold outside sessions; R88 already covers leaving during a session.
7. R1584 limits placeholders to the first media download; U111/U67 apply them to any not-yet-downloaded file (spec-flow Q14). This is a small extension.
8. Incoming media names that differ only by case collide on a case-insensitive Mac volume, and R1570 covers only local adds. Default proposed: save the second file under a unique name and report it in Check Media (R1648); notes are never rewritten by sync. Needs a product decision.
9. Risk: AnkiWeb may reject an honest non-Anki `c`/`cv` (R1509, R2030). This cannot be tested automatically (R1566); it needs a manual personal-build check, not CI.
10. R1589 ("current AnkiMobile"): AnkiMobile cannot be automated. Oracle clients stand in for it (same backend), and a manual device check against the self-hosted server is proposed once per milestone.
11. R1540's server-copy backup naming and retention exemption extends U126's backup model (R1629, R1633). U126 must agree.
12. The upload path swaps the transformed `VACUUM INTO` file in as the live collection after `OK` (local equals server; R1655 compaction). This differs from Anki, which mutates the live file before uploading. Observable state is identical, and a failed upload leaves the live file untouched.

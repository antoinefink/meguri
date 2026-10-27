# Optimize and integrity monitoring

Spec: maintenance/optimize-integrity
Requirements: R1655, R1656, R1657 (M10, the record), R2011, R2012; supports R169 (M7.1), R1641 (M3), R2003 (M8.4), R2056 (M7)
Anki: 26.09.3
Scenarios: conformance/scenarios/maintenance/optimize/

What Anki does to keep a collection file compact and to notice a damaged one, and what Meguri keeps from it: the statements of the automatic optimize (M1) and what they leave in the file (M2), the other places that optimize (M3), the 14-day cadence (M4) and the integrity verdict (M6). When Meguri runs these, the quick check's cadence (M5.1), the move to `Failed{Corrupt}` (`engine/lifecycle` L1), the remedies (M7) and the status record (M10) are Meguri's own design (plan, U132); M5 and M7 say where that design departs from Anki and why. M8 lists SQLite facts the implementation depends on, measured in the oracle image.

## M1. The automatic optimize

- Two SQL statements on the collection's read-write connection, in this order: `VACUUM`, then `ANALYZE`. Each runs alone, with no transaction open on that connection (SQLite refuses `VACUUM` inside one, and Anki runs both between operations).
- Anki sends nothing else: no `REINDEX` (that is M3's variant), no checkpoint, no row written, no stamp. Anki optimizes only while closing the collection (M5), and the close checkpoints. Meguri adds only the trailing checkpoint of M8.3.
- Both statements name no schema. On Meguri's writer they reach the collection file only, because the writer attaches nothing (`storage/derived-store` D9).
- Meguri runs each statement as a maintenance task on the writer (`engine/op-model` O8, catalog entries `vacuum` and `analyze` in O9).

## M2. What the optimize leaves

| Observable | After the optimize |
|---|---|
| Rows of the twelve tables (`storage/schema-18` S2) | every row unchanged, none added or removed |
| `col.mod`, `col.scm`, `col.usn`, `col.ls` | unchanged; the optimize alone never makes the collection look changed to an Anki sync status check (`sync/meta-status`, manual transfers only, KTD26). It changes no row, so it gives routine iCloud sync nothing to send either |
| Keys | unchanged: seven tables key their rows by an integer `id` that is the rowid, and the other five are WITHOUT ROWID tables, so no key moves. A foreign table (an add-on's) without an integer primary key may get new implicit rowids, in Anki as in Meguri: that is `VACUUM`'s own behavior |
| Free pages | none: `PRAGMA freelist_count` reads 0 |
| `PRAGMA integrity_check` | `ok` |
| Schema objects | the same objects with the same definitions; `sqlite_stat1` and `sqlite_stat4` exist (created if missing) and describe the contents at the time of `ANALYZE` (`storage/schema-18` S5, S6.2) |
| Page size, journal mode | unchanged (a WAL-mode file stays in WAL mode, and its page size cannot change in WAL mode) |

- Statistics example (scenario `maintenance/optimize/statistics`): three notes of one note type with distinct first fields give `sqlite_stat1` the rows `notes`/`idx_notes_mid` `3 3`, `notes`/`ix_notes_csum` `3 1` and `notes`/`ix_notes_usn` `3 3`. The text is SQLite's: the row count, then the average number of rows per distinct value of each index column prefix. Scenarios read only such rows of tables they fill, the one exception to `storage/schema-18` S5's rule that statistics rows are never compared; `sqlite_stat4` (sampled keys) and `page_count` depend on the SQLite release and are never compared.
- Undo and study queues. Anki empties its undo history and its study queues after any raw statement that is not a `select`, the optimize's included. Nobody sees it, because Anki optimizes only while it closes the collection. Meguri optimizes while the collection stays open: the optimize changes no row, so the undo and redo histories stay as they are (`engine/undo` M2, an op that changed nothing), and nothing derived is invalidated.

## M3. The other places that optimize

| Where | Statements, in order | Owner |
|---|---|---|
| Automatic optimize | `VACUUM`, `ANALYZE` | U132 (this spec) |
| Check Database | `VACUUM`, `REINDEX`, `ANALYZE`, after its integrity stage passed and before any repair (`conformance/collection-check` K3 step 7) | U128 (R1641) |
| Before a one-way upload | `VACUUM`, `REINDEX`, `ANALYZE`, after the transaction that prepares the file for upload | U109 (R1655's pre-upload part), on the copy it uploads, never on the live file |

- The one-way upload row is manual-transfer compatibility only: Meguri uploads a whole collection file only in an explicit Export to Anki through a server (R2060, KTD26). Routine iCloud sync never uploads the SQLite file (R2044), so it has no pre-upload optimize. U109 is frozen; this row adds no work to it.

`REINDEX` rebuilds every index with the connection's collations, `unicase` included (`storage/schema-18` S4); a connection without it cannot run it. The three rows share M8's SQLite constraints.

## M4. The 14-day cadence

- The interval is 14 days of wall-clock time: 1,209,600 seconds, compared in whole Unix seconds.
- The time of the last automatic optimize is device-local. Anki keeps it in the user's profile, outside the collection, so it never syncs and each device has its own. Meguri keeps it as `lastOptimize` in the device state of U32's sidecar (`engine/collection-replace` R6.1), in epoch milliseconds. The rule below compares whole seconds: `last` is `lastOptimize` (in Meguri, M4.1's `last`) divided by 1,000, rounded down, and `now` is the current time in whole Unix seconds.
- Due rule: the optimize is due when no last time is recorded (Meguri: M4.1), or when `now - last >= 1,209,600`. It is not due when `now - last < 1,209,600`, which includes a clock that moved back past `last`: the optimize then waits until the clock passes `last + 1,209,600`.
- Recording: when the optimize completes, the last time becomes the time it completed. A failed or cancelled optimize records nothing, so it stays due.
- A new record: when Anki creates a profile it records the current time as the last optimize, so the first automatic optimize comes 14 days later.
- Anki only evaluates the rule while the collection closes; a check that finds nothing due costs nothing.

| Last recorded | Now | Due |
|---|---|---|
| none | any | Anki: yes. Meguri: no; M4.1 records `now` |
| 1,894,701,600 | 1,895,911,199 (13 days, 23 h 59 min 59 s later) | no |
| 1,894,701,600 | 1,895,911,200 (14 days later) | yes |
| 1,894,701,600 | 1,894,698,000 (one hour earlier) | no |

### M4.1 Meguri's first evaluation

- Meguri's device states are created without `lastOptimize` (U124a). Anki's "none is due" would therefore optimize every existing collection at the first idle trigger. Instead, when Meguri evaluates the rule and the device state holds neither `lastOptimize` nor `optimizeCadenceStart`, it writes the current time as `optimizeCadenceStart` and does not optimize. The first automatic optimize then comes 14 days later, as after Anki's new profile.
- `optimizeCadenceStart` is a `deviceState` member of U32's sidecar beside `lastOptimize`: signed 64-bit epoch milliseconds, read and written under `engine/collection-replace` R6.1's rules. Only a completed optimize writes `lastOptimize`, so the status record (M10) never shows an optimize that did not run.
- Meguri's `last` is the later of the two recorded values; when only one is recorded, that one.
- A `lastOptimize` or `optimizeCadenceStart` that cannot be read (malformed, which R6.1 makes an error, never absence) is never replaced. The optimize is not due, and the status record shows the error (M10). No path repairs such a record yet; until one exists, the automatic optimize stays off on that device.

## M5. When Meguri runs it, and how that differs from Anki

- Anki, at every collection close (closing a profile, quitting): the automatic optimize when M4 says it is due, then the quick integrity check (M6), then the backup made on close, which is skipped when the check found the file corrupt. The user waits behind a progress window.
- Meguri (plan U132, R1655, R1656): the optimize and the quick check are evaluated in `run_due_maintenance` on the `Opened`, `Background` and `BackgroundProcessing` triggers, only when idle (no review session, no long operation), never through a U127 deadline and never on `Quit`. A session start or `suspend` cancels a running optimize (M8.4); it records nothing and is due again at the next idle trigger.
- Order: when several are due on one trigger, Meguri runs the quick check (M5.1), then U127's backup (`package/backup-triggers` T3.4), then the optimize. A corrupt verdict moves the lifecycle to `Failed{Corrupt}`, so no backup runs (T3.4 runs periodic backups only in `Ready`) and a damaged file never enters the backup rotation, as in Anki. The optimize runs only after a sound verdict, or when no check was due; a check with no verdict (M6) skips it on that trigger. Anki optimizes before its check. `VACUUM` rewrites the whole file, and R2012 forbids writing a damaged one; in `Failed{Corrupt}` the engine admits no write and no maintenance task (`engine/lifecycle` L4).
- `run_due_maintenance` returns once the quick check and the backup it started have ended (T3.4). It does not wait for the optimize, which goes on as maintenance tasks until it completes or a cancel ends it (M8.4); so the optimize never uses the time T3.4 gives the backup on `Background`.
- A failed optimize is not a corruption verdict. In Anki any error the optimize raises while closing counts as a damaged file for that close: the corrupt warning shows and no backup is made. In Meguri a `VACUUM` or `ANALYZE` that fails with `SQLITE_CORRUPT` (any extended code) or `SQLITE_NOTADB` is a corrupt verdict, as in M6. Any other failure (`SQLITE_FULL`, `SQLITE_IOERR`, `SQLITE_NOMEM`, `SQLITE_INTERRUPT` from a cancel) leaves the collection as it was and `Ready`, records nothing, and the optimize stays due.

### M5.1 When the quick check is due

Meguri design (plan U132, R1656, R2011). `now` and `last` are whole Unix seconds as in M4; `last` is `lastQuickCheck.instant` divided by 1,000, rounded down.

- On an idle trigger (M5) the check is due when the device state has no `lastQuickCheck`, when `now - last >= 86,400`, or when `now < last` (the clock moved back). Otherwise it is not due, so two idle triggers within 24 hours give one check.
- After an unclean shutdown (U32's clean-shutdown flag: the open records that the previous opening did not end clean, `storage/durability-and-files`), the check is due on `Opened`, whatever `lastQuickCheck` says. It stays due for this opening until it gives a verdict: a cancelled or check with no verdict (M6) runs again at the next idle trigger.
- A recorded corrupt verdict: when the collection opens and `lastQuickCheck.passed` is false, the engine reads it before the open steps (`engine/lifecycle` L2 step 3) and the open ends in `Failed{Corrupt}` instead of running them, so no write reaches the file. Anki keeps no such state (M7). The way out is a remedy (M7.1).
- An unreadable `lastQuickCheck` (an R6.1 error) counts as absent for the due rule, at most once per opening. Its verdict is acted on but not recorded (R6.1 forbids replacing the record), and the status record shows the error (M10).
- Recording: a sound or corrupt verdict writes `lastQuickCheck` with the instant the check ended and `passed`. A corrupt verdict from a failed `VACUUM` or `ANALYZE` (M5) writes `passed: false` too. An check with no verdict writes nothing.
- A replacement (`CollectionReplaced`: restore, `.colpkg` import, the iCloud replace) installs only a file that passed `integrity_check` (`engine/collection-replace` R3 step 2, R4 step 4). The engine then writes `lastQuickCheck` with that instant and `passed: true`, so a restored collection does not open in `Failed{Corrupt}` again.

## M6. The integrity verdict

- Anki's quick check is `PRAGMA quick_check` on the collection's connection. The file is sound when the first row's only value is exactly the text `ok`. Any other first row (SQLite then lists one problem per row, up to 100) means corrupt, and so does an error while the statement runs.
- Meguri's check is U32's `Reader::quick_check` (`storage/durability-and-files`): `PRAGMA main.quick_check` on the maintenance reader, never the writer. A cancel interrupts that reader.
- The full check `PRAGMA integrity_check` (which also checks index contents against their tables) has the same verdict rule. Anki runs it on a downloaded collection and on an imported `.colpkg` before either replaces the live file, never at open. In Meguri every replacement of the live file runs it on the staged file (`engine/collection-replace` R3 step 2, R4 step 4).
- Where Anki runs the quick check: at every collection close (M5; skipped in its developer mode), and as Check Database's first stage, where a corrupt verdict ends Check Database with its corrupt error before the optimize and before any repair (`conformance/collection-check` K10, rejection `corrupt`).
- Meguri's verdict, which narrows "any error":

  | Outcome of the check | Verdict |
  |---|---|
  | first row `ok` | sound |
  | first row anything else | corrupt |
  | fails with `SQLITE_CORRUPT` (any extended code) or `SQLITE_NOTADB` | corrupt |
  | fails with anything else: `SQLITE_INTERRUPT` (a cancel), `SQLITE_IOERR`, `SQLITE_NOMEM`, `SQLITE_BUSY` | no verdict: no result recorded, the check stays due |

  Meguri cancels checks (`suspend`, a session start), and an interrupted check says nothing about the file. Treating it as corrupt would stop all writes on a sound collection.

## M7. A damaged file

- Anki, when the check at close finds the file corrupt: a warning says the collection file appears corrupt and suggests restarting the computer, then opening an automatic backup; no backup is made on that close, so a damaged file never enters the backup rotation. Anki keeps no other state: the next open opens the file as usual.
- Meguri (plan U132, R2012): the lifecycle moves to `Failed{Corrupt}` (`engine/lifecycle` L1). Writes, routine iCloud sync, Export to Anki and media upload are refused, so nothing sends the damaged collection or its changes anywhere. This replaces Anki's close-time warning (R1656 deviation: Anki checks on every close, Meguri at most once a day in the background, M5.1).

### M7.1 Remedies

The engine offers the remedies as an ordered list, in R169's order. An entry is in the list only when the Offered column holds.

| Order | Remedy | Owner | Offered |
|---|---|---|---|
| 1 | Check Database (`conformance/collection-check`) | U128 | once U128 lands (M7.2); it attaches itself at the head of the list |
| 2 | Restore (`package/backup-triggers` T8) of the most recent good backup | U127 | when the backups listing (`package/backups` B7) holds at least one `backup` or `server_copy` entry |
| 3 | reserved: replace the local collection with the collection in iCloud, plan U113 (`specs/sync/cloudkit-recovery.md`, not yet written) | U113 | never, until that spec defines it and its conditions |

- Restore: choosing the entry opens Restore on the `backup` and `server_copy` entries, newest first (B7's order), and the user picks one. When T8 refuses the file (step 4's inspect or step 8's integrity check), the state stays `Failed{Corrupt}` and the list is unchanged; the user may pick an older entry. So no restore installs a damaged backup.
- Before U128 lands (M1R), the list is restore only. No recovery is lost meanwhile: on a file that fails `quick_check`, Check Database stops at its integrity stage with the corrupt outcome (`conformance/collection-check` K10), so it would not recover the file either.
- The iCloud replace never uploads. It installs the cloud collection only through the verified replace (`engine/collection-replace`), with R2056's preview, backups and confirmation. Its conditions, steps and errors will be `sync/cloudkit-recovery`'s; this spec only reserves its place in the list.
- No remedy downloads from an Anki server. R169's third remedy, Download from Server, would be the one-way download of frozen U109. An Anki server stays a manual Settings transfer (R2060, KTD26), never a remedy this list offers.
- U127's safety backup in this state is a byte copy of the damaged file. The listing leaves such copies out (`package/backup-triggers` T7.2), so the restore entry never offers one.
- Which jobs may leave `Failed{Corrupt}` is `engine/lifecycle` L4 and L5: restore, Check Database (M7.2) and, once it exists, the iCloud replace.

### M7.2 Check Database from `Failed{Corrupt}`

Applies once U128 lands.

- Check Database takes the long-operation slot in `Failed{Corrupt}` (`engine/lifecycle` L5) and runs its integrity stage first (`conformance/collection-check` K3, K10), on the maintenance reader. Nothing else is admitted meanwhile.
- A corrupt verdict ends Check Database with K10's `corrupt`. The state stays `Failed{Corrupt}`, and `lastQuickCheck` records `passed: false` (M5.1).
- A sound verdict writes `lastQuickCheck` with `passed: true`, and the state returns to `Ready` with the same generation (no file was replaced). Check Database then goes on as from `Ready`.

## M8. SQLite facts the implementation depends on

Measured in the oracle image with SQLite 3.46.1 (Python's module) and 3.49.2 (Anki's), on files in WAL mode.

### M8.1 Temporary storage

An in-place `VACUUM` builds the new file in a temporary database, which SQLite keeps where `temp_store` says. The writer runs with `temp_store=MEMORY` (`storage/open-and-connections` C4), so a `VACUUM` there would hold a copy of the whole collection in memory: on a 49 MB file under a 20 MB heap limit it fails with `MEMORY` and succeeds with `FILE`. Every in-place `VACUUM` Meguri runs on the writer (M1, M3) therefore sets `temp_store=FILE` first and `MEMORY` again after it, whether it succeeded or not. Changing `temp_store` drops the connection's TEMP tables, so this happens only between ops, while the writer holds none. SQLite writes the temporary file in the directory named by `SQLITE_TMPDIR` or `TMPDIR`; on iOS that is the app's own temporary directory. `VACUUM INTO` (backups, `package/backups` B4) writes straight to its target and needs no temporary database.

### M8.2 Space

`VACUUM` needs free disk space for the temporary copy and for the rebuilt pages in the `-wal`: up to about twice the file's size. On a full disk it fails with `SQLITE_FULL` and changes nothing (M5: not corruption).

### M8.3 The WAL, readers and file size

- `VACUUM` writes the rebuilt pages into the `-wal`; the main file shrinks only when a checkpoint copies the whole log back. The `-wal` meanwhile grows to about the size of the compacted file.
- `VACUUM` succeeds while another connection holds a read transaction, and that reader keeps seeing its snapshot.
- A checkpoint cannot pass a reader's snapshot: with a read transaction open, `PRAGMA wal_checkpoint(TRUNCATE)` reports busy (first value 1) and the file keeps its size (82 MB in the probe); once the reader ends, the same checkpoint completes, the file shrinks (41 MB) and the `-wal` is emptied.
- So the optimize ends with a `wal_checkpoint(TRUNCATE)` maintenance task (`engine/op-model` O9 lists it for U132). A busy result is not a failure: a later checkpoint finishes the job.

### M8.4 Cancel and kill

- Interrupting the writer's connection (SQLite's interrupt call) while `VACUUM` runs ends it with `SQLITE_INTERRUPT`: nothing changed (the free page count is the same), no transaction is left open and `quick_check` is `ok`. A cancel interrupts the writer only while the optimize's own statement runs, never another op's.
- `VACUUM` also fails while the same connection has a statement that has not finished; every cached statement must be reset first.
- A process killed during `VACUUM` leaves the last committed file: the rebuilt pages sit in an uncommitted part of the `-wal`, which the next open ignores. Kill tests (R2003 subset) reopen and require `integrity_check` `ok` and every row committed before the kill.

## M9. Scenarios and tests

| What | Where |
|---|---|
| Optimize removes free pages, passes the integrity check, keeps rows and `col` stamps (M1, M2) | `maintenance/optimize/compact-no-stamps` |
| `ANALYZE` refreshes the statistics of what is stored (M2) | `maintenance/optimize/statistics` |
| A file Meguri optimized opens in Anki, passes its integrity check and gives Check Database nothing to repair (T1 live) | `maintenance/optimize/anki-opens-optimized` |
| Cadence: not due at 1,209,599 s; due at 1,209,600 s; not due when the clock moved back; a failed or cancelled run records nothing (M4) | `core/crates/meguri-engine/tests/integrity.rs` (Meguri only) |
| First evaluation: neither record → `optimizeCadenceStart` records `now`, no optimize, `lastOptimize` and the status's Last optimize stay none, due 14 days later; `last` is the later of the two records; a malformed `lastOptimize` or `optimizeCadenceStart` stays as it is, no optimize, the status shows the error (M4.1) | `tests/integrity.rs` |
| Order on one trigger: quick check, backup, optimize; a corrupt verdict → no backup, no optimize; a check with no verdict → backup, no optimize; `run_due_maintenance` returns before the optimize ends; no optimize in `Failed{Corrupt}` (M5) | `tests/integrity.rs` |
| Quick check cadence: none recorded → due; due at 86,400 s, not at 86,399 s; two idle triggers 2 hours apart → one check, a trigger 24 hours after the check → a second; due when the clock moved back; none arms a U127 deadline (M5.1) | `tests/integrity.rs` |
| Unclean shutdown → check due on `Opened` with a sound check recorded an hour before; a cancelled one runs again at the next idle trigger; the first interactive read is not delayed beyond R1857 (M5.1) | `tests/integrity.rs` |
| Recorded `passed: false` → relaunch opens in `Failed{Corrupt}`, no open step runs, the collection file's hash unchanged; a restore then records `passed: true` and the next open is `Ready`; a corrupt `VACUUM` records `passed: false` (M5.1) | `tests/integrity.rs` |
| Verdict table, an interrupted check gives no verdict (M6) | `tests/integrity.rs` |
| `VACUUM` holds no in-memory copy: it succeeds under a heap limit below the file size, and `temp_store` reads `MEMORY` (2) afterwards (M8.1) | `tests/integrity.rs` |
| Cancel during `VACUUM`: nothing changed, no open transaction, due again (M8.4) | `tests/integrity.rs` |
| Kill during `VACUUM` (failpoint): reopen passes `integrity_check`, committed rows present (M8.4, R2003 subset) | `tests/integrity.rs` |
| Remedies in `Failed{Corrupt}` before U128: restore only while `sync/cloudkit-recovery` defines no replace; no Anki server entry; no restore entry when the listing holds no `backup` or `server_copy` entry (empty, or only unverified server copies, damaged-file copies present or not); a restore of a damaged backup refused → still `Failed{Corrupt}`, same list (M7.1) | `tests/integrity.rs` |
| Check Database from `Failed{Corrupt}` (with U128): corrupt verdict → `corrupt`, state unchanged; sound verdict → `Ready` with the same generation, `lastQuickCheck` `passed: true` (M7.2) | `tests/integrity.rs` (U128 adds it) |
| Status record: after a backup, its fields name that backup; last Check Database none; after a media registration, sizes equal the folder sums; one unreadable device-state field leaves the others readable; reading changes nothing; `CollectionUnavailable` in `NoCollection`, `Opening` and `ExclusiveBusy` (M10) | `tests/integrity.rs` |

The scenarios use the scenario op `optimize` (`specs/conformance/maintenance-ops.md` M1). Their goldens in `conformance/goldens/maintenance/optimize/` were recorded with the oracle driver's `optimize` op (M1.4 says when they count as current). Until Meguri's executor has its adapter they are pending and not active; the change that lands the adapter lists `optimize/*` in `conformance/scenarios/maintenance/area.yaml`.

## M10. The maintenance status record (R1657)

Meguri design (plan U132); Anki keeps no such record. The engine returns it on request. U133a passes it to Swift, and the Maintenance section of Settings shows it (U160).

| Field | Value | Source |
|---|---|---|
| Last backup | instant and file name of the last backup that succeeded, or none | `lastBackup` of the device state (`engine/collection-replace` R6.1); a failed backup leaves it (`package/backup-triggers` T3.4) |
| Last Check Database | instant and result, or none; none until U128 records its runs | `lastCheckDatabase` (R6.1) |
| Last optimize | instant of the last completed optimize, or none; M4.1's cadence start is never shown here | `lastOptimize` (R6.1, M4); unreadable also when `optimizeCadenceStart` is (M4.1) |
| Last quick check | instant and sound or corrupt, or none; a check with no verdict (M6) records nothing | `lastQuickCheck` (R6.1) |
| Collection size | bytes of the collection file plus its `-wal` file; a missing `-wal` counts 0 | the file system, when the record is read |
| Media size | sum of the sizes of the tracked files (`media/media-files` M7.1), as of the last registration (`media/media-files` M7.3); reading the record runs no change check | the media journal (`media/media-journal`) |
| Media trash size | sum of the sizes of the trash entries (`media/media-files` M10) | the media journal |
| Remedies | empty; in `Failed{Corrupt}`, the M7.1 list in order | the engine |

- Each device-state field is a value, none (the record is absent) or unreadable (a read error under R6.1). An unreadable field never hides the other fields.
- Sizes are unsigned 64-bit byte counts.
- Reading the record changes nothing: no M4.1 write, no repair of a malformed record. It runs off the main thread and never waits for a maintenance task.
- The record exists in `Ready` and `Failed{Corrupt}`. In any other state the call fails with `CollectionUnavailable{reason}` (`engine/lifecycle` L4; U133a).

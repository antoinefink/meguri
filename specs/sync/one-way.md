# One-way sync: full upload and full download

Spec: sync/one-way
Requirements: R1518 (directions in the flow), R1536 (core: no direction chosen), R1537, R1538 (core: the facts the warnings use), R1539, R1540, R1541, R1542, R1543, R1544, R1545, R1546, R1547, R1548 (fresh cloud decision after manual Import), R1563, R1564, R2019, R1655 (compaction before upload), R2015 (full sync), R2060, R2061, R2062, R2063, R2064; AE62, AE86 (sync part)
Anki: 26.09.3 for O2, O6.2, O6.4, O7.4 and O9 (what Anki's client and the official server do); every other section is Meguri design from the Product Contract and the plan (U109)
Scenarios: conformance/scenarios/sync/oneway/ (prepared; integration and activation remain deferred)

Manual-transfer compatibility (KTD26; frozen with U109). Connecting to iCloud (R158, R1593) is `sync/cloudkit-recovery#V13`, and background expiry (R1554) is `sync/policy#P11.2`. Where this file cites R158, R1593 or R1554, it means only the manual transfer.

`sync/meguri-provider#MP4` (2026-11-16) reuses replacement, backup and uncertainty rules for Meguri's own server. Full replacements need explicit consent; MP5 requires media completion. Legacy Anki work stays frozen.

A one-way sync replaces one side's collection with the other's, whole. An upload sends the device's collection, prepared as O6.2 says, and the server keeps it in place of its own; a download fetches the server's collection and swaps it in for the device's. This file says when one is needed and which directions are possible (O2), the facts the choice shows (O3), how a job starts (O4), the safety backup (O5), each direction (O6, O7), what follows a success (O8), the AnkiWeb limits (O9), the free-space check (O10), progress (O11), cancellation and kills (O12) and the errors (O13).

Manual admission and recovery are `sync/anki-transfers` M1–M8. This spec retains prepared protocol facts;
its old automatic dispatch is superseded. References to U107 normal/hold-back and U110/U111 media are deferred
integration dependencies, not claims that those contracts, code or scenarios are already present.

Other files: the HTTP exchange is `transport.md`; the `upload` and `download` bodies are `wire-format.md` W14; the status check that asks for a one-way sync is `meta-status.md`; error kinds are `errors.md`; the swap is `engine/collection-replace`; backups, server-copy names and the listing are `package/backups`; the safety backup is `package/backup-triggers` T6; the exclusive phase is `engine/lifecycle` L5 to L8.

## O1. Jobs, inputs and results

- Two jobs: the **upload job** and the **download job**. Each is one sync job in the sense of `transport.md` (one session key for all its requests) and one replacement op in the sense of `engine/lifecycle#L9` (it takes the long-operation slot and the exclusive phase).
- Admit each job from the explicit confirmed preview of `sync/anki-transfers#M2`, not a preceding normal-sync job. Required meta checks belong to explicit preparation/admission. A status answer never dispatches transfer without confirmation.
- Inputs, besides those every job gets (`login-endpoints.md` L4: base, server kind, host key, stall timeout, proxy, cancel token):

  | Input | Used for |
  |---|---|
  | confirmed manual operation identity, direction/scopes, endpoint/account and generation/sequence guards | admission and recovery under `sync/anki-transfers` M2–M4 |
  | fresh remote facts, including `empty` | preview only; no empty-card exemption from server-copy preservation |
  | the local UTC offset at the job's start (host-provided, as `package/backups#B3`) | O6.1: the server copy's name |
  | a progress callback | O11 |

  The backups folder is the one the host passed at open (`package/backups#B1`).
- Result on success: the direction, the new generation (`engine/lifecycle#L8`), the safety backup's file name (none when there was no collection file, O5), the server copy's file name and listing kind when one was made (O6.1), and the bytes transferred.
- Result on failure: one error value of `errors.md` E1 (O13).

## O2. When a one-way sync is needed, and its directions (R1517, R1518, R158, R1593, R1564, R1547)

Different modification and schema stamps give the protocol's one-way result (`meta-status#M3`).
The user must still choose and confirm a direction in Settings under `sync/anki-transfers#M2`, including when
one collection has zero cards. A status result never launches download. Validate the server candidate for Import;
Export needs a valid local collection and durable server copy. Zero cards does not establish that other data
is absent or may be discarded. Preserve historical direction hints as protocol facts without using them to
dispatch the UI. The local collection has cards exactly when `cards` contains a row.

- **Server-forced one-way sync (R1564).** When the server's collection file is larger than its limit (the official server: 300 MiB with its default settings, O9), the server answers every `meta` with `scm` set to its current time. Every status check then finds the schema times different and reports a one-way sync, which Meguri presents as a protocol fact within the explicit transfer preview. Nothing in the answer tells this case apart, and Meguri does not try: after a download the next check asks again, as it does in Anki, until the server's collection is smaller.
- **Force a one-way Anki transfer (R1547).** A choice made for one transfer inside Settings Anki Transfers, never a standing setting, and never in the iCloud sync popover; it changes nothing in CloudKit (`sync/policy#P16`). The choice runs the `force_one_way` op of `notetypes/schema-changes` G2: it sets `scm` to now (and `mod` with it, G5), without a confirmation, even when `scm` is already past `ls`. The offline status then says one-way sync needed (`meta-status.md` M5), and the next explicitly requested check reports it (M3); a transfer still requires manual direction confirmation. Anki's desktop option does the same (it sets `scm` whether or not it was already past `ls`).

## O3. The choice facts (R1537, R1538, R2019)

Read the local facts in one consistent read and combine them with explicitly requested server observations
for `sync/anki-transfers#M2`. No U107 exchange is needed to build the preview. Refresh the final Export
preview after O6.1's server-copy download changes server stamps, before destructive-upload confirmation.

| Fact | Value |
|---|---|
| `local_mod` | `col.mod` (epoch ms): the last change on this device |
| `server_mod` | the answer's `mod`: the last change on the server |
| `last_sync` | `col.ls` (epoch ms); 0 means never synced |
| `local_scm`, `server_scm` | `col.scm` and the answer's `scm`. `local_scm > last_sync` means a change on this device requires the one-way sync |
| `local_cards`, `local_notes`, `local_reviews` | the number of rows of `cards`, `notes` and `revlog` |
| `unsynced_reviews` | the number of `revlog` rows with `usn` −1: reviews not on the server (AE62: "120 reviews not on the server") |
| `unsynced_rows` | per table, the number of rows with `usn` −1 in `notes`, `cards`, `decks`, `deck_config`, `notetypes`, `tags`, `config` and `graves`: the unsynced edits a download discards. A review leaves its card pending too, so reviewed cards count in `cards` |
| `last_review` | the largest `revlog.id` (the instant of the newest review, epoch ms); none when `revlog` is empty |
| `server_empty` | the answer's `empty` |
| `upload_allowed`, `download_allowed` | manual direction eligibility under `sync/anki-transfers#M2`; O2 status hints do not dispatch requests |
| `sanity_counts` | when the sidecar holds the counts of a failed sanity check (`engine/collection-replace#R6.1`, written by U107): the server's counts tuple `s` (`wire-format.md` W12.1: cards, notes, review rows, note types, decks, presets) and the instant it was stored; absent otherwise |

- `sanity_counts` comes from the sidecar, so it is there after a relaunch too (R2019). U107 writes it when `sanityCheck2` answers `bad` with an `s` tuple (`errors.md`, `sanity_check_failed`); O8 clears it. Together with the local counts it gives R2019's comparison.
- Counting writes nothing.
- Obtain preview facts through explicitly requested consistent reads; a status result alone may omit them.
- Anki shows no facts: its desktop dialog only offers the three buttons. The facts are Meguri's (R1537).

## O4. Starting a job (R458, R171, R2012, R2015)

Before write-bearing preparation, run these checks in order. A refusal performs no transfer mutation; an earlier explicit preview may already have queried meta. Release any acquired long-operation slot. Repeat eligibility/identity checks before upload or local replacement as `sync/anki-transfers#M2` requires.

1. **Lifecycle state.** The upload job needs `Ready`: in `Failed{Corrupt}` it is refused with `collection_corrupt` (R2012: a damaged collection is never uploaded), in any other state with `local_storage`. The download job runs from `Ready`, from `Failed{Corrupt}` (U132's recovery download, `engine/lifecycle#L4`) and from `NoCollection` (a first download; O5 says which backup, if any); any other state is `local_storage`.
2. **Review session (R458).** While a review session is open (until its summary is dismissed, KTD6, U43), the job is refused with `session_active`. Review ending does not start it again; retry requires explicit Start/Resume and renewed admission checks.
3. **Long-operation slot (R171).** Take it (`engine/lifecycle#L5`). When another holder has it, the job is refused with `exclusive_busy`, naming the running operation.
4. **AnkiWeb precheck** (upload job, server kind `ankiweb`, feature `ankiweb` only): O9.1, `upload_too_large`.
5. **Free space (R2015)**: O10, `disk_full`.
6. **Media sync.** If a media sync job is running, cancel it and wait until it has ended (it keeps every batch it completed, R1554). Until this job ends, a media sync that is started is refused with `exclusive_busy` (U111). Media sync and a one-way sync never overlap (R1546).
7. **Exclusive phase.** Enter it (`engine/lifecycle#L7`): participants flush, queued writes drain, then the phase is `Working`. The session check of step 2 is made again as part of the switch (U24, U43): if a session opened since step 2, the phase is dropped and the job ends with `session_active`.

- The app starts preparation only after the user explicitly authorized its disclosed effects and open editors were closed. Destructive upload additionally requires the post-copy final confirmation of `sync/anki-transfers#M2`; the core enforces the supplied authority but does not display a prompt.
- Every one-way job registers with U32 as a continued-processing job, so `suspend` defers to it (U32's `Deferred`); on iOS the app runs a user-started one under `BGContinuedProcessingTask` with O11's progress (KTD15, U151).

## O5. The safety backup (R1539, R2008, R2019)

- Every job takes the forced safety backup of `package/backup-triggers` T6 (through the engine's `BackupService` port), during its `Working` phase:
  - **upload**: right after the drain (O4 step 7), before the server-copy download or upload; explicit preview meta requests may precede it;
  - **download**: after the staged download has passed O7.2's checks and before the swap, so a refused download leaves the backups folder as it was.
- From `Failed{Corrupt}`, and from `NoCollection` with a collection file at the collection path, it is the byte copy of a damaged collection (`package/backup-triggers` T7). With no collection file there (a first download into an empty directory), there is none and the job goes on.
- The backup is protected from retention until the job ends (T6.2). It is published before the swap, so its instant precedes the replacement (AE62, R2019).
- If the backup fails, the job ends: `disk_full` when the cause is a lack of space (`InsufficientSpace`, `DiskFull`), `safety_backup_failed` otherwise. For an upload no server-copy download or upload was sent; for a download the staged file is removed and the local collection is unchanged.
- Anki's desktop backs up before a download only (`package/backup-triggers` T4.1); Meguri also backs up before an upload (R2008). No scenario observes backups in sync scenarios.

## O6. Upload

Order: O4, O5, O6.1, then fresh post-copy facts and final confirmation (`sync/anki-transfers#M2`), O6.2, O6.3 (feature-gated only), O6.4 and O6.5. No destructive upload occurs before that final confirmation. If local work changed while previewing, repeat admission/preparation and any stale safety backup; never send the old candidate under new authority.

### O6.1 The server copy (R1540; self-hosted servers only)

Before an upload replaces a self-hosted server's collection, Meguri keeps a copy of it in the backups folder. Anki keeps none (a deviation of R1540).

- **When.** Every self-hosted Export preserves the server download, including `empty=true`: zero cards can still leave other collection data. Failed acquisition or durable preservation prevents upload; no HTTP error proves absence. The separate feature-gated server kind keeps its existing no-server-copy rule; this delivery contacts only self-hosted servers.
- **Steps.**
  1. Request `download` (`wire-format.md` W14) with the job's session key. When the answer's head arrives, run O10's check with its declared size. Stream the body into `collection.anki2` in a fresh staging directory in the collection directory (`package/container#P9`), never whole in memory.
  2. Verify the file, writing nothing next to it: the open gate's probe and refusals only (`storage/open-and-connections#C2.1`, `#C2.2`; no upgrade, no write), then `PRAGMA integrity_check` on a read-only connection opened with `mode=ro&immutable=1` and `unicase` registered.
  3. **Verified** (the probe and the refusals pass, and the integrity check gives the single row `ok`): write a collection-only package of the file with `package/container#P11` (standalone source, temporary purpose `export`, finalisation `Replace`), named `server-copy-YYYY-MM-DD-HH.MM.SS.colpkg` (`package/backups#B2.2`) in the backups folder. The listing labels it `server_copy`, and Restore offers it like a backup (U127).
  4. **Not verified** (the gate refused it, for its version, its scheduler, as not a collection or as corrupt, or the integrity check found anything): publish its bytes unchanged under `server-copy-YYYY-MM-DD-HH.MM.SS.unverified.anki2` in the backups folder, through U32's atomic-file sequence (temporary file in the backups folder, full sync, size and SHA-1 checked against the staged file, rename, full sync of the folder). The listing labels it `server_copy_unverified`. The upload goes on.
  5. Remove the staging directory.
- **Name.** The date and time are the local wall time of the instant the copy is published, from the job's UTC offset (`package/backups#B3`). Retention never deletes a server copy (`package/backups#B5.1`).
- **Failures.** Any failure of the `download` request (transport, status, size, timeout) ends the job with its kind (O13). A failure to write the copy ends it with `disk_full` (lack of space) or `local_storage`. Then no `upload` has been sent, the local collection is unchanged and the safety backup stays.
- The official server raises its collection's `usn` and sets its `mod` when it serves a download (O7.4). The copy therefore holds those two values, and a job that ends after the copy leaves the server with them raised: its content is unchanged, and the next status check still finds a one-way sync needed.
- After preservation obtain fresh meta facts for final confirmation. The copy download changes mod/usn; its post-copy observations become the baseline. Validate explained changes where served-file facts permit; do not attribute arbitrary changes to this download. An unverified copy has unknown content/counts. Another client may change the server between copy, preview and upload; the backup is the served download, not a transaction protecting the later overwrite.

### O6.2 Preparing the file (R1541, R1655)

What Anki's client does to its collection before it sends it, which Meguri does to a copy:

1. **Copy.** `VACUUM INTO` a new `collection.anki2` in a fresh staging directory in the collection directory, from the sync reader (KTD3), after the drain: a compacted, consistent copy of everything committed. The live collection is never written by this section.
2. **Transform** the copy, in one transaction, with T the job's instant (epoch ms, from the core's clock seam). Storage writes these values as given (U23's caller-supplied stamping): nothing else is stamped.

   | Table or column | Change |
   |---|---|
   | `graves` | every row deleted |
   | `notes`, `cards`, `revlog` | `usn` −1 becomes 0; every other `usn` is kept |
   | `decks`, `deck_config`, `notetypes`, `tags` | every `usn` that is not 0 becomes 0, synced rows included |
   | `col.usn` | plus 1 |
   | `col.scm`, `col.ls`, `col.mod` | T, all three |
   | everything else | unchanged: `templates.usn` and `fields`, every `config` entry (one pending at `usn` −1 stays −1), every `mod` and `mtime_secs` of a row, `col.crt`, `col.ver` and the other `col` columns |

3. **Compact.** On the copy, after the transformation: `VACUUM`, then `ANALYZE`. The pages the deleted graves freed are reclaimed (`PRAGMA freelist_count` is 0) and `sqlite_stat1` describes the transformed content. Anki also runs `REINDEX` between the two; a `VACUUM` rebuilds every index with the registered `unicase` collation, so a `REINDEX` after it changes nothing and Meguri may omit it. A second `VACUUM INTO` of the copy, taking the place of its in-place `VACUUM`, gives the same result.
4. **Finalise** the copy as `engine/collection-replace#R4` does (checkpoint, rollback-journal mode, no `-wal`, `-shm` or `-journal` next to it, integrity check, full sync). It is now the **prepared file**: one self-contained file, sent as is and later swapped in as is.

- The values in the table are Anki's (oracle). Under a frozen clock `mod`, `scm` and `ls` are equal; Anki writes `scm` and `ls` first and `mod` when its transaction commits, so on a real clock its `mod` may be a millisecond or two later. Meguri writes one instant for all three.
- Example (`sync/oneway/upload-stamps`, the uploader's rows counted by `usn` before and after its upload at 10:06 on 2030-01-15, UTC):

  | Table | Before (`usn`: rows) | After |
  |---|---|---|
  | `cards` | −1: 2, 0: 1 | 0: 3 |
  | `notes` | −1: 2, 2: 1 | 0: 2, 2: 1 |
  | `revlog` | −1: 1 | 0: 1 |
  | `graves` | −1: 2 | none |
  | `decks` | 0: 1, 2: 1 | 0: 2 |
  | `deck_config` | 2: 1 | 0: 1 |
  | `notetypes` | 0: 6, 2: 1 | 0: 7 |
  | `tags` | −1: 1, 0: 1, 3: 1 | 0: 3 |
  | `config` | −1: 2, 3: 18 | −1: 2, 3: 18 |
  | `templates` | 0: 9 | 0: 9 |
  | `col`: `mod`, `scm`, `ls`, `usn` | 10:05, 10:01, 10:04, 4 | 10:06, 10:06, 10:06, 5 |

  Free pages before: some; after: none. A client that downloads the file at 10:07 gets the same rows, with the `col` row 10:07, 10:06, 10:07, 6 (O7.4).
- Since `scm` = `ls` afterwards, the next schema change asks for confirmation again (`notetypes/schema-changes` G1, R951).
- **Anki differs here** without a scenario showing it: Anki transforms and compacts the live collection itself before sending it, so a failed or refused upload leaves its graves deleted, its `usn` values reset and its `scm` moved. Meguri changes the live collection only after the server accepted the upload (O6.5), so on any failure the live collection is byte for byte what it was (R1544's guarantee, extended to uploads).
- The prepared file is in rollback-journal mode (header bytes 18 and 19 are 1); Anki sends its file in WAL mode (2). The official server opens and converts either; no dump shows the difference.

### O6.3 AnkiWeb limits (feature `ankiweb` only)

O9.2, on the prepared file, before any request of the upload.

### O6.4 Sending, and the answer (R1542)

- Request `upload` (`wire-format.md` W14): the body is the zstd compression of the prepared file's bytes, streamed from the file (`transport.md` T5), or, with server kind `ankiweb`, the bytes O9.2 compressed beforehand.
- Statuses, redirects and size checks are the transport's (`transport.md` T6, T7): a 4xx or 5xx is its `errors.md` E4 kind (a server or proxy body limit typically answers 413, `unexpected_status`).
- A 2xx answer is text. The upload succeeded only when the decompressed answer is exactly the two bytes `OK`. Any other answer, the empty one included, is a refusal: the job ends with `server_message`, whose `text` is the answer read as UTF-8 with invalid bytes replaced by U+FFFD, not trimmed, at most its first 16,384 bytes (bytes past them are read and dropped, never held). The core never shows, sanitizes or translates it (U151 shows it sanitized, R168).
- Refusals the official server of 26.9.3 sends (examples; any server may send any text):

  | Text | When |
  |---|---|
  | `Your upload was corrupt. Please use Check Database, or restore from backup.` | the server could not open the uploaded file as a collection, or its integrity check failed |
  | `collection exceeds size limit` | the decompressed body is at or above the server's limit (O9) |

- After a refusal or any failure the live collection is unchanged (O6.2). A failure after the whole body was sent may still leave the server holding the upload (O12).

### O6.5 After `OK`: the swap

- After exact OK is durably recorded, the retained prepared file replaces the live collection through verified `FullUpload` with no additional caller transform. This identifies the local prepared file, not server-byte equality: server schema processing and later download stamps follow O7.2/O7.4.
- Nothing else can have changed the live collection since the copy: in the exclusive phase only the job's own ops commit (`engine/lifecycle#L4`), and the job commits none.
- The sidecar's last-optimize time becomes T (`engine/collection-replace#R6.1`), so U132's periodic optimize counts its 14 days from the upload.
- Then O8.
- If local activation fails after OK, retain both file identities, backups and the operation record. Report remote accepted and the actual local activation/recovery state under `engine/collection-replace#R9`; an error after rename does not prove the old local file remains active. Do not resend the accepted upload.

## O7. Download (R1543, R1544, R1545)

Order: O4, O7.1, O7.2, O5, O7.3.

### O7.1 Receiving

- Request `download` (`wire-format.md` W14) with the job's session key.
- When the answer's head arrives, run O10's check with its declared size (`anki-original-size`). If it fails, close the connection; nothing has been written.
- Stream the body into `collection.anki2` in a fresh staging directory in the collection directory (`package/container#P9`), never whole in memory (R1543). The transport checks that the decompressed length equals the declared size (`transport.md` T6.2).
- Progress reports the bytes received against the declared size, then `checking` (O11).

### O7.2 Checking the staged file (R1543, R2005)

The checks of `engine/collection-replace#R3` steps 1 and 2, run by the job on the staged file before the backup:

1. The open gate, every phase (`storage/open-and-connections#C2`): the refusals of R1479, R1482 and R2013, the upgrade of schemas 11 and 14 to 17 on the staged file only (R1478), and the expected-object check (R2016).
2. `PRAGMA integrity_check`: the single row `ok`, else refused.

A refused file ends the job with `download_invalid` and its `reason`:

| Gate or check result | `reason` | Also |
|---|---|---|
| `TooOld` | `too_old` | `version` found |
| `TooNew` (12, 13 or above 18) | `too_new` | `version` found |
| `V1Scheduler` | `v1_scheduler` | |
| `NotACollection` | `not_a_collection` | |
| `UpgradeFailed` | `upgrade_failed` | `version` found |
| `Corrupt`, or an integrity check that is not `ok` | `corrupt` | |

- Anki's client refuses the same versions and a failed integrity check, and upgrades older files the same way. It accepts a v1-scheduler collection, which Meguri refuses (deviation `storage-v1-scheduler-refused`, R1482; tested against the stub, no scenario).
- The official server stores only uploads it could open, upgraded to schema 18, so the version refusals come from other servers, and `corrupt` and `not_a_collection` from a damaged server file or a proxy that answered in the server's place. It accepts a v1-scheduler upload, so `v1_scheduler` can come from any server.

### O7.3 Backup and swap

1. The safety backup (O5).
2. `engine/collection-replace` with reason `FullDownload` and the caller's change: `UPDATE col SET ls = mod` (R1543). The primitive checks the file again (its R3), finalises it and swaps it in.
3. Then O8.

### O7.4 What the downloaded collection holds (Anki 26.9.3; oracle)

- The official server serves its collection after raising its `col.usn` by one and setting its `col.mod` to the instant it serves it. Its `mod` therefore moves at every download (`meta-status.md` M3).
- The downloader keeps every row and column as served, except `col.ls`, which becomes the file's `col.mod`. So `ls` = `mod`, `scm` is the server's, `usn` is the server's raised value, and every `config` entry, `usn` and `mtime_secs` included, is as the server stored it.
- Example (`sync/meta/outcomes`, a download at 10:02 of a collection uploaded at 10:01): the `col` row reads `mod` 1894701720000, `scm` 1894701660000, `ls` 1894701720000, `usn` 2.
- Every local change not on the server is gone, pending reviews included (`sync/oneway/choice-download`); the safety backup (O5) keeps them.

## O8. After a successful job (R725, R1489, R1546, R1548)

- Publish one `CollectionReplaced{generation}` after clearing undo/redo with `ClearedByOneWay` and restarting the day clock, as `engine/lifecycle#L8` requires.
- The derived store rebuilds, or revalidates after upload, under `storage/derived-store` (R1489, U26).
- Clear only the successful manual transfer's legacy sanity/reconciliation state, and persist its actual stage outcome under `sync/anki-transfers#M4`. Any remembered no-changes status is observational and schedules no request.
- Manual Import creates a fresh pending publication generation using current `engine/collection-replace#R6` and `sync/anki-transfers#M7`, even if the old cloud decision was clear. Export never clears an existing cloud fence. A pendingScm/timestamp comparison is not cloud recovery evidence.
- Collection transfer leaves media unchanged. Only a selected and separately authorized M5 media stage may follow; neither a successful result nor lifecycle event starts media automatically. Selected-media delivery is frozen reference (plan, "Anki server work: frozen"), not a current requirement.

## O9. AnkiWeb size limits (R1563; feature `ankiweb` only)

Without the feature this section's code and constants do not exist (`login-endpoints.md` L3). For server kind `self_hosted` nothing here applies: the client checks no size, and the server answers for itself (O6.4).

| Limit | Bytes |
|---|---|
| uncompressed: the collection file | 314,572,800 (300 MiB) |
| compressed: the `upload` body as sent | 104,857,600 (100 MiB) |

These are the official server's defaults, which R1563 sets for AnkiWeb too. The server refuses a decompressed upload at or above 300 MiB (`collection exceeds size limit`) and a body above 100 MiB (HTTP 413).

### O9.1 The precheck

- Runs at the start of every collection sync job with server kind `ankiweb`: before the status check of a normal sync (U107 calls it; `meta-status.md` M2) and at O4 step 4 of an upload job. Not before a download job, not before a media sync.
- L = `PRAGMA page_count` × `PRAGMA page_size` of the live collection, read on a reader: the size the collection file has once checkpointed, without reading the file.
- L above 314,572,800: `upload_too_large` with `size_bytes` L, `limit_bytes` 314572800, `measure` `uncompressed`. The job sends nothing.
- Anki's client runs the same check before its normal sync's status check, on the size of the collection file on disk; a WAL that is not checkpointed can make that size smaller than L. Meguri's L never misses growth that sits in the WAL.

### O9.2 The upload check

On the prepared file (O6.2), before any request:

1. U = the prepared file's size. U at or above 314,572,800: `upload_too_large`, `measure` `uncompressed` (Anki's client compares the same way).
2. Compress it with the transport's zstd settings (`transport.md` T5) into a temporary file in the same staging directory. C = its size. C above 104,857,600: `upload_too_large`, `size_bytes` C, `limit_bytes` 104857600, `measure` `compressed`.
3. Otherwise O6.4 sends exactly those compressed bytes as the body.

- Anki's client checks only the uncompressed size and lets the server refuse a body that is too large; Meguri checks the compressed size first, so a collection that compresses to 120 MB is refused before any transfer.

## O10. Free space (R2015)

Before any transfer, and again when a `download` answer's head gives the real size, the job checks free space with U32's free-space provider. A shortfall ends the job with `disk_full` (`needed_bytes`, `available_bytes`) before anything is written.

- S is the size of `collection.anki2` plus its `-wal` (0 when absent). B is the safety backup's need: 2 × S + 4 MiB (`package/container#P10`, a package of the live collection), S + 4 MiB for the byte copy of a damaged collection (`package/backup-triggers` T7.2's preflight), 0 with no collection file. MiB is 1,048,576 bytes.
- At O4 step 5:

  | Job | Collection directory | Backups folder |
  |---|---|---|
  | upload | 3 × S (the prepared file and the room its compaction needs), + S with server kind `ankiweb` (the compressed body), + S when a server copy will be made (its download, estimated at S) | B, + 2 × S + 4 MiB when a server copy will be made (its package) |
  | download | 2 × S (the download, estimated at S, and the pre-upgrade copy the gate may make) | B |

- When the answer's head arrives, with declared size D:

  | Request | Collection directory | Backups folder |
  |---|---|---|
  | `download` for the server copy (O6.1) | D | 2 × D + 4 MiB |
  | `download` for the download job (O7.1) | 2 × D | B |

- When the collection directory and the backups folder are on one file system (the same device, as on iOS and the Mac), the two figures add up into one check.
- A write that still fails for lack of space (another app filled the disk) ends the job with `disk_full`, with the same cleanup as any failure (O12).

## O11. Progress (R1545)

The job reports a stage and, while bytes move, the bytes done and the total, through the job's progress callback at `engine/jobs` J5's throttling.

| Stage | Job | Bytes |
|---|---|---|
| `backup` | both (O5) | none |
| `server_copy` | upload (O6.1) | received of declared |
| `preparing` | upload (O6.2, O9.2) | none |
| `uploading` | upload (O6.4) | sent of total: uncompressed bytes as the transport compresses them (`transport.md` T15), or compressed bytes of the compressed body (`ankiweb`) |
| `downloading` | download (O7.1) | received of declared, after decompression |
| `checking` | upload: the body is sent, the answer has not come; download: O7.2 | none |
| `replacing` | both: the swap and the reopen | none |

- Order: upload `backup`, `server_copy` (when made), `preparing`, `uploading`, `checking`, `replacing`; download `downloading`, `checking`, `backup`, `replacing`.
- The words shown ("Checking…" for `checking`) are the app's (U151, U160). Anki's client switches its label to its "Checking…" text when the bytes done reach the total, in both directions.

## O12. Cancellation, interruption and kills (R1544, R1554, AE86)

- **Cancel.** Subject to `sync/anki-transfers#M4`'s unknown-upload guard, honoured until the upload's `OK` answer, and until the download's swap begins (`engine/collection-replace#R5`). A cancel ends the job with `cancelled`, or `interrupted` when the system ended it (background time ran out, R1554), within `transport.md` T9's 200 ms for the network part. Then: disposable staging is removed, but required manual-operation evidence is retained outside U32 sweep namespaces; the exclusive phase is dropped (the state is `Ready` again, or `Failed{Corrupt}` for a recovery download, with the same generation), and the slot is released. Before local activation, collection content and publication decision remain unchanged; the manual-operation journal still records actual partial/unknown outcomes. A safety backup or server copy already published stays.
- **After the point of no return** (the `OK` answer, or the start of the download's swap) a cancel is ignored and the job completes the swap: it is local and short.
- **Upload accepted but not swapped.** When the server stored the upload and the device never swapped (the `OK` answer was lost to a network failure, the job was cancelled after the body was sent, or the process was killed after `OK`), the server's `scm` is the upload's and the device keeps its old collection. The next status check asks for a one-way sync again, and both collections still exist, one on each side.
- **Kill.** Before the swap's rename, the next launch opens the old collection unchanged; it passes the integrity check and holds every change committed before the job started (AE86). U32's launch sweep removes disposable staging only (R1493); M4-required prepared artifacts, intent, acknowledgements and recovery files remain outside those sweep namespaces. After the rename, the next launch opens the new collection (`engine/collection-replace#R8`).
- An interrupted download starts again from the first byte next time: nothing of a partial download is reused. The transport seam (KTD14) keeps a later URLSession background transfer possible; it is not part of this unit.
- **Unknown upload.** A possible remote upload with no durable OK retains the operation overlap guard across cancellation, kill, account/endpoint changes and recovery dismissal. Local job exit and matching stamps prove neither acceptance nor remote completion. After durable OK, resume guarded local activation without re-uploading. `sync/anki-transfers#M4` owns explicit recovery.
- **Fault points** (U21's `faultpoint!`, cataloged by U250, driven by U118's kill matrix), besides `engine/collection-replace#R8`'s: `oneway.after_exclusive` (after the drain), `oneway.after_backup`, `oneway.upload.after_server_copy`, `oneway.upload.after_prepare` (the prepared file finalised), `oneway.upload.after_ok` (before the swap), `oneway.download.after_body` (all bytes on disk, before O7.2), `oneway.download.after_check` (before the backup). U118 also kills by transfer progress: at 10%, 50% and 90% of the download's bytes and during `checking`.

## O13. Errors

The kinds of `errors.md` E3 a one-way job ends with:

| Where | Kind |
|---|---|
| O4 step 1 | `collection_corrupt` (upload in `Failed{Corrupt}`), `local_storage` (any other state the direction does not run from) |
| O4 steps 2 and 7 | `session_active` |
| O4 step 3 | `exclusive_busy`, with `running_op` |
| O9 | `upload_too_large` |
| O10, and any write that lacks space | `disk_full` |
| O5 | `disk_full` (lack of space) or `safety_backup_failed` |
| O6.1 and O7.1: the `download` request; O6.4: the `upload` request | the transport's and the status's kinds (`errors.md` E4, E5): `network`, `offline`, `timeout`, `auth_failed`, `server_error`, `bad_response` and the rest |
| O6.4: a 2xx answer that is not `OK` | `server_message` |
| O7.2 | `download_invalid`, with `reason` |
| the swap (`engine/collection-replace#R9`) or any other local file failure | `local_storage` (`disk_full` for lack of space) |
| cancel; the system ended the job | `cancelled`; `interrupted` |
| a broken invariant | `internal` |

Keep device collection data unchanged on failures preceding activation. For a failure during activation,
replacement R6/R9 supplies the recovery decision: identify the active file, generation and publication fence.
Do not derive those facts from the error alone. O6.5 and manual-transfer M4 also require recording whether
server acceptance is known or unresolved; a local failure cannot erase that outcome.

## O14. Scenarios and tests

### O14.1 Scenarios

The following prepared scenario/evidence inventory is retained for later integration. No scenario is activated or acceptance claimed by importing this neutral contract; current manual-policy behavior is checked under `sync/anki-transfers#M8`.

Prepared actor scenarios under `conformance/scenarios/sync/oneway/` (`specs/conformance/sync-ops.md`), with goldens recorded by the oracle (each recorded once and confirmed by `tools/mg oracle compare`). They are live and stay `pending` until Meguri's executor plays actors and declares `sync` with the `normal`, `full_upload` and `full_download` kinds (sync-ops.md S10.2); the implementer marks them active in `conformance/scenarios/sync/area.yaml` with the adapter. The Meguri actor's own steps use only `add_note`, `edit_note`, `delete_notes`, `set_config`, `db_exec` and `sync`; the oracle actors' steps need nothing from Meguri.

| Scenario | Shows |
|---|---|
| `upload-stamps` | O6.2 on the Meguri actor: every grave deleted, pending notes, cards and reviews at 0 while synced ones keep their `usn`, every deck, preset, note type and tag row at 0, `templates` and `config` untouched (a pending config entry stays −1), `col.usn` + 1 and `mod` = `scm` = `ls` = the upload's instant, free pages reclaimed; the uploader's next normal sync finds no changes; a fresh client's download gets the file with the server's `usn` one higher and `mod` the download's instant (O7.4); another client's next sync asks for a one-way sync (R1538: an upload replaces the server collection for every device); all converge; Anki's Check Database on a downloader of the Meguri-written file |
| `choice-download` | AE62: another client adds a field and uploads while the Meguri actor holds three unsynced reviews; the Meguri actor's normal sync answers `full_sync` and changes nothing; its download gives `ls` = `mod`, the server's `usn`, no review left and the added field; its next sync finds no changes; both converge |
| `choice-upload` | the same conflict resolved by an upload: the other client's next sync answers `full_sync`, and its download brings the three reviews and drops its added field; all converge; Anki's Check Database on it |

- In every scenario where the Meguri actor uploads, its collection first came from an oracle actor's upload, so the server never holds Meguri's own stock note types (sync-ops.md S12) and the goldens apply as they are.
- The plan's T1 check that an oracle client's download of the uploaded file holds no graves and no `usn` −1 row holds for every table but `config`. In `upload-stamps`, client_c's download has no graves and no pending row in `notes`, `cards`, `revlog`, `decks`, `deck_config`, `notetypes`, `tags` or `templates`, while the two `config` entries pending before the upload are still at −1, as Anki leaves them (O6.2). A test asserts the first and never "no −1 anywhere".
- The reviews are written with `db_exec`, as in `sync/wire/records-up`, so the scenarios do not depend on the scheduler.
- Directions (O2) are shown by `sync/meta/outcomes`: upload only against an empty server, download only for a collection without cards, a choice when both have cards. `smoke/sync/meguri-downloads` has the Meguri actor download, and `smoke/sync/reset-during-download` shows a failed download leaving the collection as it was.

### O14.2 Tests in `core/crates/meguri-sync/tests/one_way.rs`

Against the scripted stub (`transport.md` T13) and the engine with a test clock; expected values from this spec, never from the code under test.

| Test | Expect |
|---|---|
| AE62 facts: a stub `meta` with another `scm`, 120 local `revlog` rows at `usn` −1 | a one-way outcome with `upload_allowed` and `download_allowed`, both `mod` values, `unsynced_reviews` 120, no direction chosen |
| Download chosen | the safety backup's publication precedes the rename (op log and file times); the local dump equals the stub's file with `ls` = `mod` |
| Upload over a non-empty self-hosted stub | the stub sees `download` then `upload`; `server-copy-*.colpkg` in the backups folder, listed as `server_copy`; inspected and restored (U127), its dump equals the file the stub served |
| Upload with kind `ankiweb` (features `ankiweb` and `test-hooks`, the test AnkiWeb suffix), stub not empty | no `download` request, no server copy |
| Upload, stub serves a truncated file or a file at `ver` 19 for the copy | a `server-copy-*.unverified.anki2` holding the served bytes, listed as `server_copy_unverified`; the upload goes on |
| Upload, the copy's `download` reset by the stub | `network`; no `upload` request; live collection hash unchanged |
| The stub answers `upload` with `Your upload was corrupt. Please use Check Database, or restore from backup.` | `server_message` with exactly that text; live collection hash unchanged; graves and `usn` values as before |
| The stub answers `upload` with `OK\n`, or with 20,000 bytes of text | `server_message`; the text is `OK\n`, or its first 16,384 bytes |
| Prepared file, inspected before streaming (fault point `oneway.upload.after_prepare`) | O6.2's table holds; `freelist_count` 0; `sqlite_stat1` present and written after the transformation; no `-wal` next to it |
| A collection with no cards, the stub's `meta` not `empty` | the outcome allows only a download |
| The stub's `meta` `empty` | only upload allowed |
| `ankiweb` build, a collection whose prepared file compresses to 120 MB | `upload_too_large`, `measure` `compressed`, no request sent; the same collection with kind `self_hosted`: no limit check, the upload is sent |
| `ankiweb` precheck as a function of page count and page size: 76,801 pages of 4,096 bytes; 76,800 pages (exactly 314,572,800 bytes) | `upload_too_large` with `size_bytes` 314,576,896; the second passes. A normal sync job of an `ankiweb` build refused by it sends no request, `meta` included |
| Download, the stub serves a truncated SQLite file | `download_invalid` `corrupt`; local hash, generation and backups unchanged; no staging left |
| Download, `ver` 19; a v1-scheduler file | `download_invalid` `too_new` (version 19); `v1_scheduler`; nothing changed, no backup made |
| Download of a schema-11 file | upgraded on the staged copy; the swapped-in collection is schema 18 with `ls` = `mod` |
| One-way job while a review session is open | `session_active`; no request; nothing changed |
| Sanity counts stored by U107, then a relaunch | the facts carry `sanity_counts`; after a successful one-way job they are gone and the backup preceded the swap |
| Injected low free space | `disk_full` with needed and available bytes, before any request |
| A media sync running when the job starts | it is cancelled and has ended before the first request; the media folder's hash is the same before and after; no media request during the job |
| A manual transfer after restore or Import | successful transfer may clear legacy manual bookkeeping only; the cloud fence remains. Manual Import from either prior-clear or prior-pending creates a fresh pending publication generation; no automatic-start path exists |
| Cancel in each stage before the point of no return | `cancelled`; local hash and generation unchanged; disposable staging removed and state `Ready`; required manual-operation evidence and unknown-upload guard retained |

### O14.3 Kills (U118, U250, AE86)

`meguri-cli` downloads a 150,000-card collection from the U13 rig and is killed at 10%, 50% and 90% of the bytes and during `checking`, and at each fault point of O12: the next launch opens the previous collection unchanged, it passes the integrity check and holds every review committed before the download began, and no staging directory or temporary file is left after the launch sweep. Killed after the rename: the new collection opens and passes the integrity check.

# Media sync

Spec: sync/media-sync
Requirements: R1534, R1546, R1565 (upload exclusion), R1575, R1576, R1577, R1578, R1579, R1580, R1581, R1583 (progress and estimate), R1584 (core part), R1585, R1588, R2060, R2061, R2062, R2063; AE65
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/media/ (prepared; integration/activation remain pending)

2026-11-16: `sync/meguri-provider` MP1–MP5 governs admission and scheduling for Meguri's own server, replacing only this file's legacy entry rules, including Y11/Y14/Y15. Protocol, limits, file handling and durable batches stay unchanged. MP5 governs required media completion and exclusions. Legacy Anki transfers stay frozen.

Manual-transfer compatibility (KTD26; frozen with U111): this job runs only inside an explicit manual Anki transfer. Routine media sync is CloudKit: R1572 and R2057 are `sync/cloudkit-records#K9` and `sync/policy#P13`, R1592 is `sync/policy#P13` and `sync/cloudkit-recovery`, R1554 is `sync/policy#P11.2`. The CloudKit halves of R1844 (resumable transfer of the large media fixture), R1583 (the first full download's counts, bytes and estimate), R1584 (the placeholder while the first download runs) and R1588 (a mass change resumes in the background) are `sync/cloudkit-records#K9.3` and `sync/policy#P13`. Where this file cites R1554, R1572, R1583, R1584, R1588, R1592 or R1844, it means only the manual job.

How Meguri's media sync job brings the media folder and the server's media into agreement: which changes it fetches, what it decides for each one, how it downloads, saves and uploads files, the sanity check that ends it, and how it reports progress, stops and resumes. It builds on:

- `sync/media-tracking`: the journal's per-name state (`live`, `deleted`, `awaiting`, `none`, the pending flag, the change stamp), the store-wide media USN, the change check, files awaiting download (T9) and reset (T11);
- `media/media-files`: the name transform's use, the add path (M3), lookup (M5), exact-name writes (M6), ignored names (M8), writes into the folder (M9) and the trash (M10);
- `sync/transport`: URLs (`msync/<method>`), the `anki-sync` header, the session key, compressed and streamed bodies, cancellation;
- `sync/wire-format` W15–W17: the media envelope, the five media methods and both zips;
- `sync/errors`: the error kinds a failed job returns.

The job's scenario op is `conformance/media-sync-ops#V3`. The op `server_media` puts on the server what no client would send; `conformance/media-sync-ops` does not define it yet, and it lands with U111's driver and schema contract (V4).

Follow `sync/anki-transfers` M1/M5/M6. Ask separately before media changes either side, including before any request
or page is applied. Never start from a timer or app event. Collection-only does no media work.
Scenarios and delivery checks are pending; this spec records no pass.

## Y1. Terms

- **Server change**: one element of a `mediaChanges` answer, `[name, usn, sha1]` (`wire-format#W16.1`): the server holds a file with that SHA-1 under that name (a **server file**), or, when `sha1` is `""`, it deleted the name (a **server deletion**).
- **Page**: one `mediaChanges` answer.
- **Local media USN**: the journal's media USN (`media-tracking#T3`). Every server change at or below it has been applied locally.
- **Server media USN**: the server's media update sequence number, from `meta` or `begin` (Y4).
- **Download list**: the server files this job must fetch, with the USN and SHA-1 each was listed with (Y5, Y6).
- **Batch**: one `downloadFiles` request with its answer, or one `uploadChanges` zip with its answer.
- **Present file**, **tracked file**, **syncable file**, **case variant**, **normalized**: as `media/media-files` M2 and M7.1.
- **Register a name**: run the change check's rule for that one name (`media-tracking#T4.3`), hashing its file whatever its size and time say, under the media lock.

## Y2. Constants

| Constant | Value |
|---|---|
| Names per `downloadFiles` request | at most 25 |
| Changes per `uploadChanges` zip | at most 25 |
| Zip fill limit | 2,621,440 bytes (2.5 MiB) of file bytes, Y10.2 |
| Syncable file size | 1 to 104,857,600 bytes (100 MiB) |
| Official server, `mediaChanges` page | at most 1,000 changes, in ascending USN order, every USN above the `lastUsn` asked for |
| Official server, `downloadFiles` | packs the asked names in order and stops after the file that takes its packed file bytes above 2,621,440; refuses more than 25 names with 400; answers 409 when an asked name is not one of its files |
| Official server, `uploadChanges` | refuses (400) more than 25 changes, a `_meta` over 50 KiB, a name over 255 bytes, not in NFC or containing `/`, and a file of 0 bytes or over 104,857,600 bytes; an addition identical to its file and a deletion of a name it does not hold change nothing and do not raise its USN, yet count as processed |
| Official server, `mediaSanity` | compares the client's count with its number of non-deleted files |

The server rows were checked against the official server (oracle black box): five 1 MiB files asked in one request come back as the first three; an unknown name, alone or after a known one, gives 409; 26 names give 400; an empty request gives a zip with no file; a page of 1,000 then a page of 5 for 1,005 changes.

## Y3. The job

A media sync is a job (`engine/jobs` J1) that takes no long-operation slot, so it runs beside reviewing, editing and a normal sync (KTD9, R1572). Its steps, in order:

1. **Change check** (`media-tracking#T4`), holding the media lock, so that local changes become pending entries.
2. **Server media USN** (Y4).
3. **Pull**, when the server media USN differs from the local media USN: list every server change above the local media USN (Y5), applying each page's decisions as it arrives (Y6, Y7); then download the download list (Y8, Y9).
4. **Push**, while any entry is pending: upload batches (Y10).
5. **Sanity check**, when step 3 or step 4 made a request (Y11). A mismatch resets the state and runs steps 1 to 5 once more.

Nothing else happens: when the USNs are equal and nothing is pending, the job makes no request except a `begin` it needed in step 2.

A collection transfer leaves the media folder/journal unchanged. This same algorithm may follow only as an explicitly selected and separately authorized media stage under `sync/anki-transfers#M5`. A successful collection result never grants media permission; collection-only schedules no media work.

## Y4. Server media USN (R1534, R1575)

- The caller may pass the server media USN. After a normal or one-way sync, it is the `media_usn` of that sync's `meta` answer (`meta-status#M2`), and no `begin` is sent (R1534).
- Otherwise the job sends `begin` with `{"v": <client identity>}` (`transport#T3`) and uses the answer's `usn` (`wire-format#W16`).
- The value passed may be older than the server's current one, when another device synced media since that `meta`. Nothing depends on it being current: a pull lists until a page is empty, and Y10.4 notices the gap after an upload.
- Y11's bounded re-run within the authorized operation always uses `begin`.
- Media requests need only the host key: they never belong to a collection sync's session, and the server accepts them while a normal sync runs.

## Y5. Listing server changes (R1575, R1577)

- The job asks `mediaChanges` with `lastUsn` set to the local media USN, then, after each non-empty page, with `lastUsn` set to the highest USN of that page, until a page is empty.
- A page whose changes are not in strictly ascending USN order, or that holds a USN at or below the `lastUsn` asked for, is a decode failure (`bad_response`, method `mediaChanges`): applying it could skip changes without notice. A change whose name is the empty string is skipped (it counts as applied).
- The job lists every page before it downloads anything, so that the whole download list is known early: every name it will fetch is `awaiting` (Y12) seconds after the job starts, even for a first download of 30,000 files (R1584). The official server's pages hold 1,000 changes, so 29 requests list 28,800 files.
- A name the official server lists appears once in the whole listing. If a listing names one twice, the later change replaces the earlier one's decision.
- **Resume point.** The local media USN only grows, and only to a USN R such that every change listed at or below R has been applied: its decision made and recorded, and its file saved when it needed a download. After each page (Y7) and after each download batch (Y8.5), the job sets the local media USN to the highest such R: the page's highest USN when the download list holds nothing unsaved, otherwise one less than the lowest USN of an unsaved download-list entry, but never lower than its value before. An interrupted job therefore loses no applied change, and the next job lists again from the first change it had not finished (R1577).
- **Stale `awaiting` entries.** When the listing reaches its empty page, every `awaiting` entry whose name this job did not list (a leftover of an interrupted job, for a file the server no longer holds) is removed. The listing starts below every `awaiting` entry's USN, so the server lists again every name it still holds.

## Y6. Decisions (R1576, R1585, AE65)

For each server change, the job compares it with the journal entry of the same name (exact string; the server's names are in NFC):

| Local entry | Server file, SHA-1 S | Server deletion |
|---|---|---|
| no entry, a `none` entry, or an `awaiting` entry | download | nothing; an `awaiting` entry is removed |
| `live`, clean, hash L | L = S: nothing. L ≠ S: download | the file goes to the trash and the entry is removed |
| `live`, pending, hash L | L = S: the entry becomes clean, nothing is sent. L ≠ S: download (the server's version wins) | nothing: the local file is uploaded by the push |
| `deleted`, clean | download | the entry is removed |
| `deleted`, pending | download, and the deletion is never sent (R1585, AE65) | the entry is removed |

- **Download** adds the change to the download list with its USN and S. When no file is present under the name, the entry becomes `awaiting` (`media-tracking#T9`), remembering S and the USN.
- A `none` entry is a tracked file that never syncs (empty, or over 100 MiB). It counts as no entry: a server file replaces it (to the trash first, Y9), a server deletion leaves it alone.
- The server's version wins over a pending local change of the same name, as in Anki; the local version goes to the trash, so it can be restored (R2018, Y9).
- **R1585.** A file deleted locally while the server still lists it above the local media USN comes back: a pending deletion meets a server file and the row says download. This is what happens after a reset (`media-tracking#T11`) and to files that arrived before the first media sync (an import, a restore): a deletion counts only once the deleted file had been confirmed in sync. AE65: `a.mp3` is on the server, the user deletes the local `a.mp3` before the first media sync finishes, the media sync downloads `a.mp3` again and sends no deletion. The UI note where media is deleted is U130's.

## Y7. Applying a page

All decisions of a page are applied together, holding the media lock:

1. Files of server deletions with a clean `live` entry move to the trash (`media/media-files#M10.1`), each name on its own: a name whose file is already gone does not stop the others. The trash entry is recorded as usual; the journal records no deletion for these names.
2. In one journal transaction: the entries of server deletions are removed (the `live` ones above, `deleted` ones, `awaiting` ones); clean marks are made; `awaiting` entries are created or updated with their USN and SHA-1; every write takes a new change stamp (`media-tracking#T8`); and the local media USN advances to the resume point (Y5).

The decisions are made against the journal as it is when the page is applied. The store's change set lists the trashed names (`media/media-files#M12`).

## Y8. Downloading (R1577, R1844)

### Y8.1 Requests

- The download list is fetched in ascending USN order. Each request names, in that order, up to 25 names not yet received: first the names the previous answer left out, then the next ones.
- Each name is sent exactly as the server listed it, normalized or not.
- One request is in flight at a time, and the next is sent as soon as the previous answer is saved: no delay between requests, so the transfer is limited only by the network and the server (R1844).

### Y8.2 The answer

The answer is a zip (`wire-format#W17.1`): file entries named `0`, `1`, … and `_meta`, a JSON object from entry name to the real name. On top of W17.1, Meguri requires, or the answer is a decode failure (`bad_response`, method `downloadFiles`) and nothing of it is saved:

- every real name is one of the names asked for in this request, and no real name appears twice;
- the zip holds at least one file when at least one name was asked for (without this a server could make the job repeat a request forever);
- every file entry holds at least 1 byte and at most 104,857,600 bytes: the reader stops at the upper limit plus one byte and refuses the answer. A zero-byte file entry is not a deletion.

Reject the complete zip as `bad_response` (`downloadFiles`) if any file member has length 0. Do this before any publication or batch commit. This extra Meguri check leaves `media/media-files#M7.1`'s local empty-file rules intact; `wire-format#W17.2` already disallows empty uploads.

Zip entry names and real names are never used as paths: a file reaches the folder only under a name chosen by Y9.

Names asked for and not received are asked again in the next request. The official server sends a prefix of the names asked for, in order, so a request of large files returns fewer files (five 1 MiB files: three, then two).

### Y8.3 A name the server no longer has

The official server answers 409 to a request that names a file it does not hold or whose file is missing from its disk, and in the second case forgets the file without raising its USN. The job fails with `conflict` (`sync/errors#E3`). Its resume point is below that name's USN, so the next media sync lists again: the server no longer lists the name (or lists its deletion), and Y5 removes the stale `awaiting` entry.

### Y8.4 Saving

Each received file is saved by Y9. When the entry's state changed since its page was applied (a file appeared under the name meanwhile), the decision is made again against the current entry with the server file's actual SHA-1 (the column "Server file" of Y6), then applied.

### Y8.5 Recording

After the files of one answer are in the folder, one journal transaction, holding the media lock, records them (Y9), ends their `awaiting` state and advances the local media USN to the resume point. Then the job emits `MediaArrived` with the names whose file appeared or changed (Y12). A crash between the files and the transaction leaves files the journal does not know: the next change check registers them as pending, and the next listing, which lists their changes again, finds the same SHA-1 and marks them clean without transferring anything.

## Y9. Saving a downloaded file: names (R1578)

Let N be the real name, B the bytes and H their SHA-1. N is **usable** when it is normalized (`text/media-filenames#F1` leaves it unchanged), not ignored (`media/media-files#M8`), and either a regular file with exactly the spelling N is present or no present file is a case variant of N with a different spelling.

For this rule, use the directory entry's spelling. Merely opening N on a case-insensitive volume does not prove exact spelling. An existing exact regular N is the sole comparison/replacement target. Other variants keep both their bytes and journal state, and N is not tombstoned. Without an exact regular N, use the case-collision add path as before. This tie choice is Meguri policy.

| N | Folder | Journal |
|---|---|---|
| usable | As an exact-name write (`media/media-files#M6`): a file already named N with hash H stays as it is; a file named N with other content goes to the trash, then B is written as N; otherwise B is written as N | N `live`, clean, with the file facts of the file now under N and a new change stamp |
| not usable, and the transform of N is empty or an ignored name, or N is an ignored name | nothing is kept | N becomes a pending deletion, when N is in NFC |
| not usable otherwise | B goes through the add path (`media/media-files#M3`) with desired name N: an existing file with the same content is reused (row a), or B is written under the lowercase candidate or a hash name (rows b, c); call the stored name S | as the add path records S (pending when it wrote a file); N becomes a pending deletion, when N is in NFC |

- So a server file whose name Meguri cannot keep is renamed everywhere: the next push sends the deletion of N and, when the add wrote a file, the upload of S, and other clients receive S (R1578). Fields that reference N are never rewritten by sync; Check Media (U130) reports their references.
- Examples: `clip:1.mp3` is saved as `clip1.mp3`; `notes.` as `notes._`; `Thumbs.db` and `???` are not kept. Each sends the deletion of its server name.
- A server name not in NFC cannot be recorded (`media-tracking#T2`), and the official server refuses a deletion of it (400). Meguri saves the file as above but records no deletion for such a name. The official server never holds one; on a server that does, the sanity check then disagrees on every sync (Y11).
- When the exact incoming spelling is absent but another case spelling is present, Meguri keeps the existing file and renames the incoming one (M3 row a or c). This includes two server names that differ only in case, which a case-insensitive volume cannot hold side by side. Anki on a case-insensitive volume writes the second file over the first, then uploads the merged state; on a case-sensitive one it keeps both (Y17). This is a default chosen without a product decision (open point of `docs/plans/research/coverage/sync.md`, gap 8).
- Bytes are written by rename from a temporary file (`media/media-files#M9`). A file a download replaces always goes to the trash first (R2018); Anki overwrites it.
- `downloaded_files` counts every file received, whichever row applies.

## Y10. Uploading (R1579, R1580, R1565)

### Y10.1 Choosing a batch

Pending entries are taken in ascending change-stamp order (oldest change first), at most 25. A deletion written before a file under another name therefore travels first, which keeps a rename by case (`Photo.jpg` deleted, `photo.jpg` added) safe on a server whose disk ignores case.

### Y10.2 Fill limit

Before taking each further entry (a file or a deletion), the job adds up the sizes of the files already taken; when the sum is above 2,621,440 bytes, the batch ends. A batch thus always holds at least one entry, and may end with one large file: five 1 MiB files go as three, then two; a 100 MiB file after two small ones goes in their batch, and closes it.

The official server refuses a request body over 100 MiB as sent (`transport#T5`). A file close to 104,857,600 bytes that does not compress (most audio and video), alone or after small files up to the fill limit, can make its batch's compressed body pass that limit; the server then refuses that batch (a 413, `unexpected_status`) at every sync. Anki batches the same way and meets the same refusal; Meguri does not work around it (an open point of U111).

### Y10.3 Building the zip

- A `deleted` entry becomes a deletion, `[name, null]` in `_meta` (`wire-format#W17.2`).
- A `live` entry is opened by its exact name. When it is missing, not a regular file, empty or over 104,857,600 bytes, the job registers that name (Y1) and builds the batch again from Y10.1: the entry turns into a pending deletion, or into whatever the folder now shows. Files over 100 MiB and empty files are never put in a zip (R1565); the change check never makes them pending in the first place (`media-tracking#T4.3`).
- File bytes are read when the zip is sent and streamed (`transport#T5`): stored entries, real names exactly as the journal holds them, entry names by position, `_meta` as W17.2. The SHA-1 of each file is computed while it is read. An entry whose bytes do not hash to its recorded hash (the file changed after the change check) is not marked clean after the batch, and its name is registered again before the next batch.

### Y10.4 The answer

The answer is `[processed, usn]` (`wire-format#W16`). In one journal transaction, holding the media lock:

1. The first `processed` changes of the batch, in batch order, are accepted (all of them when `processed` is larger). An answer that accepts none of a non-empty batch is a decode failure (`bad_response`): the job would otherwise send the same batch forever.
2. Each accepted change is marked clean only if its entry's change stamp still equals the stamp it had when the batch was built (`media-tracking#T8`). An entry written meanwhile, by the store or by a download, stays pending and goes in a later batch with its new content. A change the server processed is never sent twice unless the entry changed.
3. If `usn` equals the local media USN plus the number of accepted changes, the local media USN becomes `usn`. Otherwise it stays: another device changed the server's media meanwhile, or the server treated some changes as no-ops (an identical file, a deletion of a name it does not hold). The next pull lists from the old value, and the decisions find the job's own changes already applied (R1580).

`uploaded_files` and `uploaded_deletions` count the accepted changes by kind.

### Y10.5 Loop

The push repeats Y10.1 to Y10.4 until no entry is pending, each batch committed before the next is built. 19,000 changed files go as about 760 batches of 25, each saved as it is accepted (R1588).

## Y11. Sanity check and bounded re-run (R1581)

- After any pull or push request, the job sends `mediaSanity` with `local`: the number of `live` entries. It counts them in the same journal read that finds no pending entry; if an entry became pending since the push ended (a file added while the job ran), the push runs again first. So a file added during the job never makes the counts disagree.
- `"OK"`: the job succeeds.
- `"mediaSanity"` (the counts disagree): the job resets the state (`media-tracking#T11`) and runs Y3 steps 1 to 5 once more, starting with `begin` (Y4). The re-run lists every server change from USN 0, downloads only what is missing and uploads only what the server lacks; its counts add to the job's.
- If the re-run's sanity check disagrees too, the job resets the state again, so that the next media sync starts from USN 0 as well, and fails with `media_resync_failed` (`sync/errors#E3`). The error is shown as a status, never as a blocking dialog (R1562, U151).
- Anki instead resets after the first mismatch and asks the user to sync again (Y17).
- A server whose media USN fell below the local one (its media was replaced) lists nothing; the sanity check then disagrees, and the re-run brings both sides together.
- This re-run remains inside the same authorized manual operation and disclosed media scope. It cannot schedule a later transfer or bypass cancel/identity guards. A failure leaves selected media incomplete; `sync/anki-transfers` M4–M6 retains the collection result and governs explicit recovery and custom-server eligibility.

## Y12. Files still arriving (R1584 core part, R1592)

- **Pending downloads.** A name is pending download when lookup (`media/media-files#M5`) finds no file for it and the journal holds an `awaiting` entry for it or for one of its case variants. The core answers this per name and lists every such name, through the `awaiting` index (`media-tracking#T9`, under 1 ms per name).
- The card media handler (U67) answers a pending name with its quiet placeholder instead of a missing-file response. Check Media (U130) leaves pending names out of its missing-media report and never tags notes for them.
- **`MediaArrived(names)`**: after each download batch's transaction (Y8.5), one event on the change stream (`engine/change-stream`) with the names whose file appeared or whose content changed in that batch, renamed files (Y9) under their stored names. Prepared and visible cards that use one of them render again (U67, U175).
- Study does not wait for media: the job never takes the collection writer, so a first launch can review as soon as the collection is downloaded while media keeps arriving (R1592).

## Y13. Progress and result (R1583)

### Y13.1 Progress

The job reports through its progress sink (`engine/jobs#J5`) with these phases and values:

| Phase | Reported values |
|---|---|
| `checking` | entries examined by the change check |
| `listing` | server changes listed so far |
| `downloading` | files in the download list, files received, bytes received, files left, estimate |
| `uploading` | changes accepted so far, by kind, and bytes sent |
| `confirming` | none (the sanity check) |
| `resyncing` | the re-run's phases follow, reported again from `checking` |

- `checked` (the status view's count, R1560) is the sum of entries examined by change checks and server changes listed; `added` and `removed` are the files and deletions moved in each direction.
- **Estimate.** In `downloading`, once at least three batches are saved: files left × (seconds taken by the last 20 saved batches ÷ files received in them). This moving average follows changes in throughput better than the phase average of `engine/jobs#J5`, which applies to the other phases.

### Y13.2 Result

A finished job returns:

| Field | Meaning |
|---|---|
| `uploaded_files`, `uploaded_deletions`, `downloaded_files`, `downloaded_deletions` | as `conformance/media-sync-ops#V3`, across the re-run when there was one |
| `checked` | as Y13.1 |
| `bytes_downloaded`, `bytes_uploaded` | file bytes, not wire bytes |
| `resynced` | whether the bounded internal re-run of Y11 ran |
| `oversized` | the names of tracked files over 104,857,600 bytes, each once, sorted by UTF-8 bytes: they never sync, and the status view and Check Media show them (R1565) |
| `awaiting` | the number of `awaiting` entries left (0 after a complete job) |

These counters describe the media stage only. Retain the completed collection result if media is waiting,
cancelled, interrupted or failed; show the whole selected operation as partial. Completion and exclusions
follow `sync/anki-transfers#M5`. Media-only success never clears a CloudKit fence. Whole-operation success
requires durable completion of all selected work and its receipt under M6. Custom servers impose no subsequent wait.

`downloaded_deletions` counts the server deletions that removed a local file or a local deletion record (Y7); a server deletion met by a pending local change, or by no entry, is not counted.

## Y14. Cancellation, pause, interruption and resumption (R1572, R1588, R1554, R1844)

- The job honours its cancel token (`engine/jobs#J4`) between requests, between files while it saves an answer, and during a request (`transport#T9` aborts it). A cancelled job ends `cancelled`; a job the system stops (background time over, the app suspending, C8) ends `interrupted` (`sync/errors#E3`).
- Pausing is cancelling: every committed page, download batch and upload batch is kept, and the next job resumes from the local media USN, the pending entries and the `awaiting` entries (Y5). A half-saved answer is never recorded; its files are found again as Y8.5 says.
- A media sync interrupted at any point, even across a relaunch, never transfers again a file it already recorded, and never loses a change: a 60-file download stopped after its second batch fetches only the last 10 files when it resumes; a 19,000-file upload stopped halfway sends only the rest (R1588, R1844).

Resume requires an explicit action for the same durable operation, endpoint/account, media scope and expected
local generation (`sync/anki-transfers#M5`). Resume only this media stage after collection completion; never
resend or replace the collection. Preserve completed pages/batches and recorded partial effects. Credentials
or identity changes do not erase operation results, retained deadline history or an unknown-upload guard. Relaunch, foreground,
review end, timers and background events start no Anki media work. An authorized job may continue within its
lifecycle limits, and Y11 may perform its internal bounded re-run.

## Y15. Concurrency and ordering (R1572, R1592, R2006)

- The job never takes the collection writer and never reads the collection. Its writes go to the media journal (`media/media-journal`), a separate database, so answering cards during a 28,800-file download commits within R2006's budget.
- It holds the media lock (`media/media-files`, U31) only while it changes the folder or the journal: the change check, each page's application, each download batch's saving and recording, each upload batch's building and its answer. It never holds the lock during a request. Check Media waits for the lock and may run between two batches; every decision is made against the journal as it is when applied, and Y10.4's stamp rule protects uploads in flight.
- Only Y11 resets inside an active media job. Logout/server change first cancels and drains that job, then applies `media-tracking#T11`. Preserve the manual operation's durable outcome and identity guards; a reset is not permission to resume or contact another account.
- Reject a second media job for the same collection with `exclusive_busy`. A normal protocol round can run beside it only with its own disclosed effects and explicit authorization. Permission for media alone cannot start that round.
- Collection replacement cancels and drains an active media job before swapping. Later media requires the operation's selected scope and separate M5 permission, with the resulting generation checked. It reuses the folder/journal and existing stamp/USN resume rules; collection completion itself starts nothing.

## Y16. Errors

Every failure is one value of `sync/errors#E3`. Media-specific cases:

| Failure | Kind |
|---|---|
| a media answer holding only `err` (`wire-format#W15`) | `server_message` |
| a `mediaChanges` page out of order (Y5), a malformed or oversized download zip (Y8.2), an upload answer accepting nothing (Y10.4), any other decode failure | `bad_response`, with the method |
| 409 on `downloadFiles` (Y8.3) | `conflict` |
| the sanity check disagreeing after the re-run (Y11) | `media_resync_failed` |
| a file or journal write failure; a full disk | `local_storage`; `disk_full` |
| the change check failing on a file (`media-tracking#T7`) | that failure's kind |
| cancel; system stop | `cancelled`; `interrupted` |
| HTTP statuses, transport failures | `sync/errors` E4, E5 |

A failed job keeps everything it committed; nothing rolls back past the last batch. No failure panics or blocks study (R1562).

## Y17. Differences from Anki

| Situation | Anki 26.09.3 | Meguri | Golden |
|---|---|---|---|
| Order of listing and downloading | lists a page, downloads its files, records the page, lists the next | lists every page first (Y5), then downloads | none: the counts and the folder are the same |
| Resume point | the whole page of up to 1,000 changes, recorded after all its downloads | each download batch (Y5, Y8.5) | none |
| A file a download replaces | overwritten | moved to the trash first (Y9, R2018) | deviation `media-sync-replaced-to-trash` |
| Server deletions of a missing file and of a present file in one page | stops moving files at the first missing one; the others stay in the folder unrecorded and are uploaded again later | each name on its own (Y7) | none: scenarios keep present files first |
| Download answer with fewer files than asked | assumes a prefix and skips by count; an empty answer repeats forever | matches by name; an empty answer is `bad_response` (Y8.2) | none |
| A non-normalized server name with upper-case letters | records the transformed name while the file is written lowercased | records the stored name (Y9) | none: scenarios use lower-case names |
| An ignored server name (`Thumbs.db`) | downloads it, keeps it until a later folder listing turns it into a deletion | keeps nothing, sends the deletion at once (Y9) | none |
| Two server names that differ only in case | case-sensitive volume: both kept; case-insensitive volume: the second overwrites the first | the second is renamed (Y9) | none |
| An upload entry whose file is missing, empty or too large | removes the entry, so no deletion is ever sent | registers the name, which usually sends a deletion (Y10.3) | none |
| An upload entry changed while its zip is in flight | marked clean, the change found at the next check | stays pending by its stamp (Y10.4) | none |
| Sanity check mismatch | resets and fails, asking the user to sync again | resets and runs once more; fails only if that disagrees too (Y11) | none |
| A non-NFC name in its records, on Apple | drops it at upload | never holds one (`media-tracking#T5`) | none |

## Y18. Scenarios and tests

This is the prepared evidence inventory. No scenario is activated here and no recorded compatibility result substitutes for current manual-consent/native/recovery acceptance under `sync/anki-transfers#M8`.

Neutral actor scenarios under `conformance/scenarios/sync/media/`, in the vocabulary of `conformance/media-ops` and `conformance/media-sync-ops`:

| Scenario | What it shows |
|---|---|
| `roundtrip` | Y6, Y7, Y10: files added from outside and through the store go up and down; an edited and a removed file reach the other client; removed files go to the trash; repeated media syncs move nothing |
| `decisions` | every row of Y6 in one pull: new, unchanged, changed on the server, pending with the same and with other content, pending deletion met by a server file, server deletions met by a clean file, a pending change, a pending deletion and no entry |
| `ae65` | AE65 and R1585 |
| `server-wins` | Y6, Y9: the server's version replaces a clean and a pending local version; Meguri keeps both old versions in the trash (deviation) |
| `server-names` | Y9, R1578: names a client cannot keep, put on the server with `server_media`, are saved under normalized names and renamed on the server; another client receives the new names; no churn afterwards |
| `batches` | Y8, Y10: 60 small files down, five 1 MiB files and 30 small files up and down; its recording shows the request sizes (25, 25, 10; three then two; 25 then 5) |
| `size-limit` | R1565: a file over 100 MiB and an empty file never reach the server or the other client |
| `nfd-upload` | an NFD-named file reaches the server and another client under its NFC name, and later media syncs move nothing (deviations: the Linux oracle skips the file, `media-tracking#T5`; Anki on macOS sends and delivers the NFC name as Meguri does) |

The goldens are the Linux oracle's. The native macOS oracle agrees with them for every scenario but `nfd-upload`, where it shows the transfers Meguri's deviations hold, and `server-names`, which it cannot run. Driver and executor status: `conformance/media-sync-ops#V4`.

No client can put an NFD name on the server: the official server refuses a name not in NFC (Y2). An NFD name therefore only ever starts in a client's own folder, which is what `nfd-upload` covers; a non-NFC name from a server that holds one anyway is a stub test (Y9, below).

Recordings: `tools/mg oracle recordings sync/media/batches` keeps the exchanges of the oracle playing every actor (`conformance/sync-ops#S9`). Download requests show their name lists; upload answers show `processed`; zip bodies are recorded by size and hash only. The job records only scenarios T0 lists as live or pending, so this recording is made once Meguri's testkit model reads the scenario (`conformance/media-sync-ops#V4`); until then the request sizes are checked by the stub tests below.

Meguri-only behavior is tested in `core/crates/meguri-sync/tests/media_sync.rs` against the scripted stub (`transport#T13`) and the local server rig (U13), where a scenario cannot reach:

- batch sizes, from the stub's request log: 60 server files are asked for as 25, 25 and 10 names; 30 small pending files go up as 25 then 5; five 1 MiB files as three then two (Y8.1, Y10.1, Y10.2); a download answer that holds a prefix of the names asked for leads to a request for the rest (Y8.2);
- interruption: a job killed after the second of three download batches resumes with one request for the last 10 names (stub request log); cancelling mid-answer records nothing of it; a crash between files and transaction (Y8.5); resumption after `interrupted`; a 19,000-file upload cancelled halfway and resumed (R1588), with normal syncs succeeding meanwhile;
- the stamp rule: `clip.mp3` replaced through the store while its zip is in flight stays pending, the next batch sends the new bytes, and an oracle client on the rig receives them (Y10.4); bytes that do not match the recorded hash (Y10.3);
- the USN gap: a media sync started with a stale `meta` USN while another client uploaded: the USN does not advance, the next pull fetches the gap (Y4, Y10.4);
- the sanity check: a stub mismatch leads to a reset and one re-run; a second mismatch gives `media_resync_failed` with no state left half-reset; a pending entry appearing before the check (Y11);
- the stub's decode cases: pages out of order, a zip with an unknown or repeated real name, an empty zip, an oversized entry, `processed` 0 and larger than the batch, the `err` envelope, 409 (Y8.3) and the stale `awaiting` cleanup that follows;
- names: ignored and transform-empty server names, two server names differing by case, a non-NFC server name (Y9);
- placeholders: during a first download, a render request for an `awaiting` name gets the placeholder answer and Check Media neither lists it as missing nor tags its notes; `MediaArrived` per batch (Y12);
- progress: the phases, `checked`, the estimate after three batches (Y13);
- concurrency: reviewing during a synthetic 28,800-file download commits answers within the R2006 budget, and the job never takes the collection writer (Y15);
- the `oversized` result for a 101 MB file, listed once however many syncs run (Y13.2).

`core/fuzz/fuzz_targets/media_zip.rs` feeds arbitrary bytes to the download zip reader (Y8.2): it saves nothing and returns a decode failure or the files, never panicking, aborting or allocating beyond the bytes it read; it runs nightly (U19).

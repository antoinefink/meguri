# Media sync state and change tracking

Spec: sync/media-tracking
Requirements: R1567, R1573, R1574, R1582, R1583, R1502, R1581, R1855, R2060, R2061, R2062, R2063; supports R1584, R1588
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/media/tracking/ (prepared; integration/activation remain pending)

Manual-transfer compatibility (KTD26): the state below serves manual Anki media transfers only. CloudKit media state is separate (`sync/cloudkit-records#K9.4`, K10; `media/media-files#M16`) and reads the journal only through the file-fact feed (`media/media-journal#J6`). Nothing in this file, T11 included, changes CloudKit state.

The per-file state that media sync (U111, `specs/sync/media-sync.md`) builds on, the change check that brings it up to date before each media sync, and the reset that makes the next media sync compare every file with the server. Which directory entries count as media, how a registration hashes files and what makes an entry pending are `media/media-files#M7` and M11; this file states the sync side of them and what media sync needs on top. The scenario vocabulary for these behaviors is `conformance/media-sync-ops`.

Anki keeps the equivalent state in its own media database next to the media folder. Meguri never reads or writes that file (`media/media-files#M1`): the state below lives in Meguri's media journal database (U31, `media/media-journal`), so nothing is ever added to `collection.anki2` (KTD3, R2016). It is device-only state and never syncs (KTD12).

Manual policy is `sync/anki-transfers` M1/M5/M6. These tracking rules grant no permission to send requests.
Only an explicitly selected media stage with separate both-side consent may use them for server reconciliation.
Local media-store operations can still register files without starting a transfer. Scenario/evidence references
below describe prepared work, not activated conformance or completed delivery.

## T1. Terms

- **Store**, **journal**, **registration**, **change marker**, **tracked file**, **syncable file**: as `media/media-files` M2 and M7. A syncable file is a tracked file of 1 to 104,857,600 bytes.
- **Check**: one run of the change check (T4), whoever starts it. **Full comparison**: a check that lists the folder whatever the change marker says (`media/media-files#M7.3`).
- **Media sync**: U111's job. **Media USN**: the server's update sequence number for media.
- **Reference Mac**: the Mac the perf report uses for budgets (U12).

## T2. Entry state (R1567)

The journal holds at most one entry per name, and every entry name is in NFC. An entry has two independent parts.

**File facts**, present when the entry describes a regular file in the folder: its size, its modification time and its content hash (SHA-1 of the bytes, 40 lower-case hex digits). Size and time are kept at the full precision the file system reports (nanoseconds on APFS) and are only compared for equality. Lookup (`media/media-files#M5`) and the content hash of M2 use the file facts whatever the sync state is.

**Sync state**, what media sync knows about the name:

| Sync state | Meaning | File facts |
|---|---|---|
| `live` | a syncable file that media sync sends or has sent | present, syncable size |
| `deleted` | a deletion record (the "tombstone" of R1582): a file media sync sent or would have sent is gone | absent, or the facts of a file that is no longer syncable (emptied, or grown past 104,857,600 bytes) |
| `awaiting` | the server holds a file under this name that is not in the folder yet (T9) | absent |
| `none` | the journal knows the file, but it is not part of media sync: a tracked file that is not syncable, or any file not yet confirmed since a reset (T11) | present |

Each entry also carries:

- **pending**: whether the next media sync must send the entry, as a file (`live`) or as a deletion (`deleted`). Entries in `awaiting` and `none` are never pending.
- **change stamp** (T8): changes on every write to the entry except media sync marking it clean.

Where `media/media-files` says the journal records a file as live, this file says the entry has file facts; its sync state is then `live` when the file is syncable, and `none`, or `deleted` once it stopped being syncable, when it is not.

The media sanity check after a media sync (U111, R1581) counts the `live` entries: Anki's client sends the number of files it has on record, and its record never includes empty or oversized files.

For comparison: Anki's media database has one row per name with a checksum (absent for a deletion), a modification time in whole seconds (0 for a deletion) and a pending flag. Its rows are Meguri's `live` and `deleted` entries; it keeps nothing like file facts outside media sync, change stamps or `awaiting` entries.

## T3. Store-wide state

The journal also holds, once per media folder:

| Value | Meaning |
|---|---|
| media USN | integer, initially 0: every server change up to this USN has been applied locally. Media sync advances it and resumes from it (R1577); reset sets it to 0 (T11). |
| change marker | the folder's own modification time at full precision, or unset (initially, and after a reset). T4 and `media/media-files#M7.3` set it. |
| last check report | the report of the most recent check (T6), kept for diagnostics and tests. |

Persist the media baseline's owner with its USN and sync state, using the opaque configured-endpoint/account
reference of `sync/anki-transfers#M6`. Do not store credentials or a raw username here. Equality follows M6's
case-sensitive logical identity and its alias limits; redirects or key rotation do not create another owner.
An absent or different owner makes the old cursor and clean/awaiting state unusable for the selected account,
even if the USN values happen to match. Apply T11 before reconciliation under that account.

## T4. The change check (R1573, R1855)

### T4.1 When it runs

- As the first step of every media sync.
- Before the store operations that decide on present files (`media/media-files#M7.3`: adds, exact-name writes, trash and restore).
- As a full comparison only on request: Check Media (U130), the first check after a reset (T11, because the reset unsets the marker), and the conformance observation `media_pending`.
- Never when the collection opens, the app launches or comes to the foreground, and never on a timer (R1855). Server media work starts only under `sync/anki-transfers#M5` authorization. A lifecycle event, timer or review end never starts or resumes it.

### T4.2 Steps

1. Read the folder's modification time, F.
2. If the change marker is set and equals F, the check ends: no listing, no stat of any file, no hashing and no journal write other than the report (T6). This is the whole cost of a check when nothing in the folder was created, removed or renamed since the last one.
3. Otherwise list the folder. Each entry is skipped or tracked by `media/media-files#M7.1` and M8, except that T5 replaces M7.1's rule for names that are not in NFC, on every platform. A tracked file whose size and modification time both equal its entry's file facts is not opened or hashed, and its entry is written only when T4.3 changes its sync state. Any other tracked file is hashed, and its entry changes by T4.3.
4. After the whole listing, every entry with file facts whose name the listing did not yield as a tracked file changes by T4.3, in the same transaction that sets the change marker to F, the value read in step 1. A time read after the listing is never used: a change made while the check runs moves the folder's time past F, so the next check lists the folder again.

A full comparison runs steps 1, 3 and 4.

### T4.3 Sync state after a check

Restating `media/media-files#M7.2` for each name, with "same" meaning same size and modification time as the entry's file facts:

| The listing yields | Entry before | Entry after |
|---|---|---|
| a syncable file, same | `live` | unchanged |
| a syncable file, not same | `live` | facts updated; pending if the hash changed, otherwise the pending flag stays as it was |
| a syncable file | `none`, `deleted`, `awaiting` or no entry | facts recorded (the old hash reused, without hashing, only when the old facts were the same); `live`, pending |
| a tracked file that is not syncable | `live` | facts updated; `deleted`, pending |
| a tracked file that is not syncable | `none`, `deleted` or no entry | facts recorded or updated; the sync state stays (`none` for a new entry) and the pending flag too |
| a tracked file that is not syncable | `awaiting` | unchanged |
| nothing (absent, skipped, or not a regular file) | `live` | facts dropped; `deleted`, pending |
| nothing | `deleted` with facts | facts dropped; still `deleted`, pending flag unchanged |
| nothing | `none` | entry removed |
| nothing | `awaiting`, or `deleted` without facts | unchanged |

Consequences the scenarios pin:

- A file written into the folder from outside (AE79's `clip.mp3`) moves the folder's time, so the next check lists the folder, hashes the new file and records it `live` and pending (`sync/media/tracking/outside-add`).
- A file rewritten in place moves only its own time, never the folder's, so a check whose marker matches does not see it. A later check that lists the folder for another reason, or a full comparison, finds it when its size or time differs from the facts. A file whose size and time still equal its facts is never hashed again, even when the folder is listed (R1567; `sync/media/tracking/quick-check`).
- Empty files, files over 104,857,600 bytes and ignored names never become pending `live` entries. A `live` file that is emptied or grows past the limit becomes a pending deletion, so other devices delete their copy, and a pending `live` entry again once it is syncable (`sync/media/tracking/skip-transitions`). Anki does the same.

## T5. Names that are not in NFC (R1574)

APFS keeps the form in which a name was created, so a file another program wrote can have a decomposed (NFD) name. For each regular file whose name is valid UTF-8 and not in NFC, let C be its NFC form:

1. If C is not normalized (`text/media-filenames#F1` would change it) or is ignored (`media/media-files#M8`), the file is skipped like any such name.
2. If the entry for C has file facts with the file's size and modification time, the file is left as it is (on an Apple volume it is the file registered under C).
3. Otherwise the check renames the file to C with a rename that never replaces an existing entry (`media/media-files#M9`), then registers it under C like any tracked file (T4.3). The rename does not change the file's modification time.
4. If the rename fails because another directory entry already holds C (only on a volume that tells NFC and NFD names apart), the non-NFC file is skipped and C is registered on its own. Check Media (U130) reports the skipped file.
5. If the rename fails for another reason, the file is registered under C without renaming when opening C reaches it (any Apple volume), and skipped otherwise.

So no entry ever holds a non-NFC name, and no deletion of a non-NFC name is ever recorded or sent. A rename moves the folder's time, so the next check lists the folder once more and finds nothing new. Each rename counts in the report (T6, `renamed`).

Anki on Apple platforms registers such a file under C and leaves the name on disk as it is; on other platforms it skips the file. The native macOS oracle shows the Apple half: in `sync/media/tracking/nfd-name` it registers C with the file's hash at both checks, records nothing for the NFD name, and the folder still lists the NFD name. Meguri renames on every platform, so its results are the same on macOS and in the Linux container. The goldens are the Linux oracle's: `conformance/deviations.yaml` holds `media-nfd-renamed-check` and `media-nfd-renamed-dir` for `sync/media/tracking/nfd-name`.

## T6. Check report

Every check returns a report, and the journal keeps the last one:

| Field | Meaning |
|---|---|
| `started_at`, `finished_at` | wall-clock instants |
| `completed` | false when the check was cancelled or failed (T7) |
| `listed` | whether the folder was listed (false when T4.2 step 2 ended the check) |
| `seen` | directory entries examined |
| `hashed` | files opened and hashed |
| `added` | entries that became pending `live` from `none`, `deleted`, `awaiting` or no entry |
| `changed` | `live` entries whose hash changed |
| `removed` | `live` entries that became pending deletions |
| `renamed` | non-NFC names renamed to NFC (T5) |
| `skipped` | directory entries not tracked: ignored names, names that are not normalized or not UTF-8, entries that are not regular files, and non-NFC files left out by T5 |

Tests count hashes with it (the 30,000-file test asserts `hashed` = 0), and the sync status view (U151) uses it to explain a slow check.

## T7. Locking, concurrency and cancellation

- A check that media sync or Check Media starts runs while that job holds U31's media lock, which keeps media sync and Check Media apart. The store's own checks before an operation (`media/media-files#M7.3`) do not wait for that lock, so two checks, or a check and a store write, can overlap. The two rules below keep every outcome correct; an overlap costs at most one extra listing.
- A check never overwrites a newer write. It applies its result for a name only if the entry's change stamp (T8) is still the one it read before it looked at the file; otherwise it leaves the entry alone, and the folder's time has moved, so the next check looks at that file again. The deletions of T4.2 step 4 follow the same rule, using the stamps read before the listing.
- A check commits its registrations in batches, so that hashes already computed survive a cancellation or an interruption (R1588: re-encoding 19,000 files must not start over after each interruption). Changes that need the whole listing (T4.2 step 4) and the change marker are written last, in one transaction.
- A check honours its cancel token (`engine/jobs#J4`) between files and while hashing a large file. A cancelled check keeps what it committed, leaves the change marker as it was and reports `completed: false`; the next check lists the folder again and skips every file already registered.
- A directory entry that disappears between the listing and its stat or hash counts as absent at this check.
- Any other I/O error on one entry ends the check with an error naming the file and the system error, after committing what was done; the change marker stays as it was, and the media sync that started the check fails with it (U106 maps the kind).
- A check never takes the collection writer and never reads the collection.

## T8. Change stamps (for media sync)

- A store-wide counter, persisted in the journal, only ever increases. Every write to an entry takes its next value as the entry's change stamp: store writes (add, exact-name write, trash, restore), every write a check makes (T4.3, including a rehash that finds the same hash), the `awaiting` bookkeeping (T9), recorded downloads (U111) and reset (T11).
- The one write that keeps the stamp is media sync marking an entry clean after the server accepted it (U111). It clears the flag only if the stamp still equals the stamp the entry had when media sync put it into the upload batch, checked in the same transaction. An entry written meanwhile stays pending and goes in a later batch.
- The counter is store-wide, so a stamp never repeats for a name, even after the entry was removed and created again.

A change made from outside while an upload is in flight reaches the journal only at the next check, which finds the file's size or time different from the facts, rehashes it and marks it pending again. So clearing the flag of an entry whose file changed from outside after it was zipped never loses that change.

## T9. Files awaiting download (supports R1584)

- Media sync (U111) puts a name in `awaiting` when it learns that the server holds a file the folder does not have, and records the downloaded file as `live` and clean. When it does this is `specs/sync/media-sync.md`'s.
- A check never turns an `awaiting` entry into a deletion: no file of it was ever in the folder (T4.3). A syncable file that appears under that name becomes a pending `live` entry like any new file, and media sync decides what to do with the server's version.
- The journal answers whether one name is `awaiting` and lists every `awaiting` name, each through an index that holds only `awaiting` entries, so that the card media handler (U67) can show its quiet placeholder and Check Media (U130) can leave these names out of its missing-media report and tagging (R1584).

## T10. Deletion records (R1582)

- A `deleted` entry stays after media sync has sent it, as a clean deletion record, until a file with that name is registered again (T4.3), media sync applies the server's own deletion of that name (U111), or a reset removes it (T11).
- Pending entries are found through an index that holds only pending entries, and `awaiting` entries through one that holds only those. Clean entries of every state are in neither, so these queries cost the same with 20,000 deletion records as with none.
- Budget on the reference Mac, with 30,000 `live` entries, 20,000 clean deletion records and up to 25 pending entries: listing up to 25 pending entries, and asking whether any entry is pending, each take under 5 ms; asking whether one name is `awaiting` takes under 1 ms.

## T11. Reset (R1502, R1581)

Reset makes the next media sync compare every file with the server from the start, without transferring files the server already holds with the same content.

- It runs after cancelling/draining media on logout or server change, and on the authorized job's sanity mismatch (R1581, U111). A mismatch permits only Y11's bounded re-run within that operation. Reset itself starts no request. Keep manual-operation receipts, stage results, retained deadline history and unknown-upload guards outside this reset; new endpoint/account authority requires explicit admission. If reset invalidates a media baseline, an explicit resume must re-list against the new baseline rather than trust old clean marks.
- Reset takes the media lock, so it waits while a media sync or Check Media holds it; a caller that must not wait cancels the media sync first. Holding the lock, in one journal transaction, reset:
  1. removes every entry without file facts (deletion records and `awaiting` entries);
  2. puts every other entry in `none`, not pending, with a new change stamp, keeping its file facts;
  3. sets the media USN to 0;
  4. unsets the change marker.
- Before reconciling an unknown or changed baseline owner, require the separate M5 consent for the selected
  endpoint/account and both-side effects. Cancel and drain prior media work. Under the media lock, perform the
  T11 reset and persist the new opaque owner in the same journal transaction; a crash cannot expose a new owner
  with an old cursor. Keep file facts, media files, manual history, receipts, retained deadline history and unresolved-operation
  guards. Reset alone starts no requests. Logout invalidates the active binding; an unbound baseline must be
  initialized this way before later use. A same-owner sanity reset may retain its binding within the authorized job.
- Reset leaves CloudKit alone (R1572, R2052, R2057): it never changes the CloudKit tracking store, and it keeps every file-fact feed item above the feed floor, the floor and the feed identity (`media/media-journal#J6`). An entry that step 1 removes leaves a removal item when the floor needs one. So a deletion or a change that CloudKit has not yet consumed still reaches CloudKit after an Anki logout, a server change or a sanity reset.
- The next check is therefore a full comparison. By T4.3 it makes every syncable file a pending `live` entry, hashing only files whose size or time changed since they were last registered, and removes the entries of files that are gone, without recording a deletion for them.
- Media sync then receives the server's whole change list from USN 0 and applies U111's decision rules (R1576): a pending file whose content equals the server's becomes clean without a transfer; a file the server holds and the folder lacks is downloaded; a pending file the server does not hold is uploaded.
- A deletion that was pending at reset time is forgotten: the server still holds the file, so the next media sync downloads it again (as R1585 says for deletions made before media is in sync).

Anki's reset deletes its whole media database; its next registration hashes every file again and records each syncable file as pending, with no deletion records and a media USN of 0. After Meguri's next check the journal holds the same pending set and media USN, so every media sync outcome is the same; only the hashing work differs. `sync/media/tracking/reset-identical` (nothing transferred) and `sync/media/tracking/reset-pending` (a pending deletion comes back, a pending add is uploaded) pin this.

## T12. Cost budgets (R1583, R1855)

On the reference Mac, with a media folder of 30,000 files and 1.5 GB (U11's synthetic media), none changed since the last check, and a warm file-system cache:

- A check with an unchanged folder time lists and hashes nothing (`listed` false, `seen` 0, `hashed` 0) and takes well under 0.5 s.
- A full comparison of the same folder takes under 0.5 s and hashes nothing (`hashed` 0).

The perf report records both; the iPhone figures come from U245 (simulator) and U256 (device).

## T13. Differences from Anki

| Situation | Anki 26.09.3 | Meguri | Golden |
|---|---|---|---|
| A listed file's time is unchanged but its size changed | not rehashed: it compares whole-second times only | rehashed | none: scenarios keep sizes equal or move the clock (`conformance/media-ops#F12`) |
| Two writes to a file within one second, same size | the second is missed | seen when the nanosecond times differ | none |
| A non-NFC file name | Apple: registered under its NFC form, name on disk unchanged; elsewhere: skipped | renamed to NFC on every platform (T5) | deviations `media-nfd-renamed-check`, `media-nfd-renamed-dir` |
| Reset | deletes its media database; every file is hashed again | keeps file facts (T11) | none: the state after the next check is the same |
| Cancelling a check | the whole check is rolled back | committed batches are kept (T7) | none |
| A file disappears during a check | the check fails | the file counts as absent | none |

## T14. Scenarios

Prepared inventory only: these scenarios are not activated by integrating this spec. New manual consent, identity, partial-result and explicit-resume checks remain required under `sync/anki-transfers#M8`.

Neutral scenarios under `conformance/scenarios/sync/media/tracking/` use `conformance/media-ops` and `conformance/media-sync-ops`:

| Scenario | What it shows |
|---|---|
| `quick-check` | T4: a matching marker ends the check, so a same-size rewrite in place goes unseen; a new file makes the check list the folder; a file whose size and time match is not hashed again |
| `outside-add` | T4.3, AE79 slice: a file written from outside is one pending add at the next check |
| `skip-transitions` | T4.3: ignored and empty files never become pending; a file that is emptied or grows past 100 MiB becomes a pending deletion, and pending `live` again once syncable |
| `nfd-name` | T5 (deviation): an NFD name is renamed and registered under its NFC form; no deletion of the NFD name |
| `reset-identical` | T11: reset, then media sync against a server holding the same files: nothing transferred, nothing pending |
| `reset-pending` | T11: a pending deletion is forgotten and its file downloaded again; a pending add is uploaded and reaches an Anki client |

The two reset scenarios are actor scenarios: the live tier (`tools/mg conformance --live`) reports them `pending` until Meguri's actor can run media sync (U111).

Meguri-only behavior has no golden and is tested in `core/crates/meguri-sync/tests/media_tracker.rs` (U110): the T6 counts; T7 (a store write during a check, overlapping checks, cancellation and resumption, a file vanishing, an I/O error); T8 stamps and clean marking; T9 `awaiting` entries surviving checks; the T10 and T12 budgets (30,000 files with `hashed` = 0, 20,000 deletion records); non-UTF-8 names on Linux (APFS refuses them, so no scenario can create one); the T5 rename collision on Linux; and that the full comparison after a reset hashes nothing when no file changed.

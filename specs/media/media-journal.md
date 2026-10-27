# Media journal and staged operations

Spec: media/media-journal
Requirements: R986, R1582, R1702, R1855, R2018; supports R1572, R2057
Anki: none (Meguri design; no Anki source or database layout)
Scenarios: none (Meguri's own store tests, U31 and U114; J6 lists the feed's)

## J1. Ownership and files

One media store belongs to each open collection, under the engine's collection lock.
Clones share its journal connection and a short mutation mutex.
The store owns `meguri-media.db`, beside `collection.media`, `media.trash` and `media.staging`.
It never attaches its database to the collection or adds collection schema objects.
Directory roots must be real directories; symlinks are refused.

The journal uses SQLite WAL and synchronous FULL. Each local record holds its exact name,
case key, SHA-1, size, modification time, live state, dirty flag and change counter.
A global counter orders changes. Metadata holds the folder marker and last media USN.
Partial indexes cover dirty records and pending downloads. Clean tombstones do not affect pending queries.
A dirty non-syncable record represents a deletion, even when an empty or oversized local file remains.
Acknowledgement clears a dirty flag only when the change counter still matches.
The dirty flag, the sync state of `sync/media-tracking#T2`, the last media USN and the baseline owner are manual
Anki media-sync state (KTD26, R1572). CloudKit media state is never stored here; it lives in the CloudKit
tracking store and reads this journal only through the file-fact feed (J6).
The journal identifies its own format with SQLite application id `0x4d47524d`
and a schema version. The current version is `2` (U31 on master; open migrates a version `1` journal);
the file-fact feed of J6 raises it to `3` (migration in J6). Open refuses foreign formats and unsupported versions
before making any writes; it never silently replaces their contents.

## J2. Registration and writes

A change check compares only the directory modification time. Matching markers require no listing or hashes.
A full comparison lists entries, then registers them individually, yielding between entries.
Hashing occurs only when size or modification time changed. Cancellation leaves completed records valid and the folder marker unset.
A scan does not delete records created after its starting change counter.
It records the final folder marker only when the directory stayed unchanged during the scan.

Every publication uses U32's file helper and refuses existing destinations.
A replacement first preserves the old bytes in the trash. A failed publication restores the old file when possible.
A journal failure after publication leaves intact bytes for the next registration.
Lookup never scans the directory or hashes files.

## J3. Trash

Each trash entry has an independent identity and retains its original name and bytes.
Its durable path carries the identity and original name, so deleting the media index cannot destroy restore information.
The identity is a parent directory; its child keeps the complete original basename,
including outside names that already use the filesystem's component limit.
Recovery also accepts the earlier flat identity-prefixed filenames.
Trash metadata records the supplied collection time. Restore processes newer identities first.
It follows `media/media-files#M10`, and never overwrites an existing file.
Only an explicit empty-trash call permanently removes trash bytes.

## J4. Staging and recovery

A staged operation records its files and expected note post-images in the journal before the collection commit.
Each reference holds a note id, post-op modification time and field digest.
The engine reads these from the final stored rows. Every witness must change
its identity, modification time or field digest in the operation, and the
witnesses must reference every staged name. An unchanged pre-existing row
cannot prove that an operation committed. Attachments without a note change
use the immediate-add path.
Staged names participate in subsequent naming decisions until the operation ends.
Stage bytes are synchronized before their manifest becomes usable.
An ordinary rollback removes that operation's files and manifest; it never publishes them.

The engine binds a staged operation to the collection operation. Its publication runs after the collection transaction commits,
before observers receive the change set. Publication failure retains the manifest and reports a typed committed-state error.
Callers must not repeat a committed note operation after that error.
A crash needs no separate committed flag: recovery compares every expected note post-image against the open collection.
Matching rows publish remaining staged files; nonmatching rows move their bytes into the trash.
A destination already holding the expected bytes completes an interrupted publication without rewriting it.
Unknown staged files are preserved in the trash. Recovery is idempotent and never permanently deletes media bytes.
Recovery skips stages still owned by a live operation in this process. A missing
manifest is an error when reading names, binding references or publishing; it
cannot turn an attachment-bearing operation into an empty successful stage.
The engine pins its collection snapshot after the store captures inactive
recovery candidates, so every selected operation's commit is visible.

## J5. Engine coordination

The service offers one exclusive media-job lease shared by media sync and Check Media.
Adds do not acquire that lease. Journal mutations remain serialized through the store.
Long scans run away from the collection writer, with progress and cancellation.
Collection ops report added and removed names in their change sets.
Downloads additionally emit `MediaArrived(names)` after publication. CloudKit blob installs do the same.
CloudKit media work (U114) does not take the media-job lease. Each blob install is one exact-name write
(`media/media-files#M6`), serialized with other journal mutations. A manual Anki media transfer or Check Media
holding the lease neither pauses nor is paused by CloudKit media work.
So a `CloudIncoming` apply can add references while Check Media runs. Check Media's destructive actions (Delete
Unused and its fixes, U130) check each name again when the action runs, not only when the report was made: the
name's references in the current collection and its file facts in the current journal. A name that is referenced
now, or whose file facts changed since the report, is skipped and reported as skipped.

## J6. File-fact feed for CloudKit (R1572, R2057)

CloudKit tracking (`sync/cloudkit-records#K9.4`, K11.1) learns local media changes from this feed.
The feed depends only on file facts, never on the Anki media-sync state of J1.

- A name is **present** when its record has file facts (name, SHA-1, size, modification time;
  `sync/media-tracking#T2`). A **feed item** is a name with a change-counter value and either its file facts
  or "absent" with its absence cause (below).
- `feed(after C)` returns, in counter order, every record whose change counter is above C, with its file facts
  or "absent" and its cause, and every retained removal item (below) whose counter is above C. It may return a name whose facts did not change;
  the consumer compares digests. It must return every change of presence or facts made after C.
- **Absence cause.** Decided 2026-10-27: every write that makes a present name not present records why, on the
  name's record or on its removal item.
  - `removed`: an explicit Meguri removal. That is `trash(names)` (`media/media-files#M10.1`) leaving the name
    absent, whoever calls it (Delete Unused, Check Media fixes, a staged operation's recovery, a deletion that a
    manual Anki media transfer applies), and a Check Media rename, for the old name.
  - `missing`: every other write. A registration that no longer finds the file (`media/media-files#M7.2`,
    `sync/media-tracking#T4.3`, the "nothing" rows: the file was deleted, moved or evicted outside Meguri, or a
    restore left it out), the T4.3 removal of a `none` entry, an import that clears the journal, and the
    migration below.
  - An absent record keeps its cause until the name is present again. A record deleted while already absent
    (`sync/media-tracking#T11` step 1) gives its stored cause to its removal item.
  - Only `removed` is a deletion signal for CloudKit (`sync/cloudkit-records#K9.4`). M10.1 keeps a `deleted`
    record for manual Anki transfers (`sync/media-tracking#T2`), so whether the record survives does not decide;
    the cause does. An item with the cause `missing` is never a deletion signal, whether it is a record's
    item or a removal item for dropped facts (facts dropped by a registration while the record stays, or a
    fact-less record deleted later): CloudKit tracking treats the name as a missing local file, a download
    candidate from CloudKit, and reports it as missing media when CloudKit holds no asset for it. A file that leaves the syncable range keeps its file
    facts, so it stays present; tracking marks it unsyncable (K9.4) and makes no tombstone.
  - The reconcile scan (`sync/cloudkit-records#K11.4`) reads the same causes. A name with no record and no
    retained removal item counts as `missing`.
- **Removal items.** A write that makes a present name not present (its facts dropped, or its record deleted)
  takes the next global counter value for that name. Every deletion of a record, whatever the caller
  (`sync/media-tracking#T4.3` "entry removed", T11 step 1, an import that clears the journal, any other path),
  writes a removal item (name, counter and absence cause) in the same journal transaction as the deletion, except in an
  operation that sets a new `feed_id` (Feed identity, below). The item's counter
  is the counter that deletion takes when the record had file facts, and the record's last counter when it had
  none (it was already absent). No item is written while the floor is unset, or when its counter is at or below
  the floor. A removal item is deleted when the floor reaches its counter, or when the name is present again.
  Callers need no code of their own for this, so the frozen U110 reset needs no change.
- **Floor.** Metadata holds `cloud_feed_floor`: a counter value, or unset. Only CloudKit tracking writes it.
  Unset means no consumer: the journal keeps no removal items. The journal may have several consumers: builds
  of separate containers or environments share the journal and one tracking file, each with its own active
  binding (`sync/cloudkit-records#K10.2`). The rule for every floor write is `sync/cloudkit-records#K10.3`
  ("Feed floor"); in short:
  - The floor is never above the lowest consumed counter, for the current `feed_id`, of any active binding in
    the tracking file, whatever its container or environment. Isolated bindings do not count. So one
    consumer's progress never deletes a removal item another consumer has not consumed yet.
  - A new consumer (a binding with no consumed counter for the current `feed_id`) sets the floor to the
    journal's current counter only when no other active binding has a consumed counter for that `feed_id`;
    otherwise it leaves the floor as it is. Then it takes the snapshot of its reconcile scan
    (`sync/cloudkit-records#K11.4`) and records the snapshot's counter as its consumed counter, so that
    counter is at least the floor. A record deleted after the floor write leaves a removal item above it, and
    the feed returns it.
  - After that, a floor write comes only after the tracking commit that stores a consumed counter, and sets
    the floor to the lowest consumed counter over the active bindings, never above it.
  - Tracking unsets the floor when the tracking file holds no binding any more (`sync/cloudkit-records#K10.2`).
    While a binding is kept, paused or not, the floor stays.
- **Feed identity.** Metadata holds a random 128-bit `feed_id`, created with the journal. A journal created
  again (a missing or replaced file) gets a new one. An operation that rewrites the journal's records as a
  whole (an import or restore that replaces the media folder) may set a new `feed_id` instead of writing
  removal items. Tracking records `feed_id` with its consumed counter; a different `feed_id`, or a floor
  above tracking's consumed counter, makes tracking run a reconcile scan before it consumes the feed.
- **Complete registration.** Metadata records, for the current `feed_id`, whether the feed is **complete**: a
  full comparison (J2) has finished and recorded the folder marker under that `feed_id`. The flag is set in the
  transaction that records that folder marker and stays set for that `feed_id`. A new `feed_id` starts not
  complete, unless the operation that sets it records the folder marker in the same transaction. The feed and
  the journal's status report `feed_id` and the flag. While the flag is not set, a journal's lack of a record
  is not evidence of a deletion or of a missing file (a new journal is empty and filled by registration, which
  can be cancelled, J2): CloudKit tracking makes no media-name tombstone, from the feed or from a reconcile scan,
  until the flag is set (`sync/cloudkit-records#K11.4`, "absent | live" row; K9.4). Decided 2026-10-27: until
  then it also neither downloads nor reports as missing a name the journal has no record of, so an unregistered
  local file is never replaced by a download.
- **Migration to version 3.** It adds the removal items, `cloud_feed_floor` (unset), a new random `feed_id` and
  the complete flag, set only when the folder marker is already recorded. Every record without file facts gets
  the cause `missing`. The first consumer then starts as a new consumer (Floor, above).
- **Independence from Anki state.** Clearing a dirty flag, a sync-state change that keeps the file facts,
  the `awaiting` bookkeeping, a media USN or change-marker write, a baseline-owner change and the reset of
  `sync/media-tracking#T11` never delete a feed item above the floor, change the floor, change `feed_id` or
  change an absence cause.
  Writes they make to records may appear in the feed as items with unchanged facts.
- Reading the feed takes no lease and never blocks a journal write for longer than one read transaction.
- Tests (U31, extended when U114 lands): a deletion, a T11 reset and an Anki logout between two feed reads
  still yield the removal item; a removal item survives until the floor passes it; a recreated journal
  changes `feed_id`; a dirty-flag clear changes no facts in the feed; a record deleted between a new
  consumer's floor write and its scan snapshot is returned as a removal item; with two active consumers, a
  removal item stays until both have consumed past its counter, and a second consumer that starts after the
  item was written does not move the floor past it; the deletion of a fact-less
  record above the floor yields a removal item with the record's last counter; a version 2 journal migrates to
  version 3 with the floor unset, a new `feed_id` and the cause `missing` on every fact-less record; Delete
  Unused and a Check Media rename yield `removed`, a file deleted outside Meguri and the T4.3 removal of a
  `none` entry yield `missing`, and a T11 reset keeps each cause. U114: delete the journal, reopen, interrupt
  the first registration, and check that the feed reports not complete and that tracking produces no
  media-name tombstone and no download; during that first registration, Delete Unused on an acked name makes
  no download and no missing report, and its tombstone is sent once the feed is complete, also after a relaunch
  in between; after a full registration finishes, the flag is set, a file deleted
  outside Meguri makes no tombstone and is downloaded again (or reported missing when CloudKit has no asset),
  and a file that Delete Unused moved to the trash makes one.

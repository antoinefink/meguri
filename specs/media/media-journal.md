# Media journal and staged operations

Spec: media/media-journal
Requirements: R986, R1582, R1702, R1855, R2018
Design: Meguri; no Anki source or database layout

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
Its durable file name carries the identity and original name, so deleting the media index cannot destroy restore information.
Trash metadata records the supplied collection time. Restore processes newer identities first.
It follows `media/media-files#M10`, and never overwrites an existing file.
Only an explicit empty-trash call permanently removes trash bytes.

## J4. Staging and recovery

A staged operation records its files and expected note post-images in the journal before the collection commit.
Each reference holds a note id, post-op modification time and field digest.
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

## J5. Engine coordination

The service offers one exclusive media-job lease shared by media sync and Check Media.
Adds do not acquire that lease. Journal mutations remain serialized through the store.
Long scans run away from the collection writer, with progress and cancellation.
Collection ops report added and removed names in their change sets.
Downloads additionally emit `MediaArrived(names)` after publication.

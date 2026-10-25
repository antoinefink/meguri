# Bounded operation journal

Spec: storage/journal
Requirements: R1490, R1851, R1852, R1860, R2002
Anki: none (Meguri-owned transaction and observer storage)
Scenarios: none; focused storage and engine tests

This specifies the storage behind the journal in `engine/op-model` O3 and
the observer payload in `engine/change-stream` S2. It changes no collection
format or sync protocol.

## J1. Image storage limit

Each changed table/key retains its first before-image and final after-image,
using the rows actually stored after SQL and stamping. Absence is a separate
state. Preserve the existing note after-image digest and changed-column rules;
this contract does not add note contents to observer payloads.

The O3 default of 8 MiB bounds retained full-image payload bytes for the
whole journal, counting before-images, after-images and images retained for
open savepoints together. Both sides spill to journal-owned unnamed temporary
storage in the collection directory. A large non-note after-image must not
remain permanently resident merely because it is an after-image.

Track keys, indexes, digests, column sets and compact ChangeSet data separately;
their size depends on the number of changed keys. Encoding and decoding may
also hold the current row or a fixed-size page. If one row exceeds 8 MiB,
store it in the spill file instead of increasing the in-memory limit. This
exclusion cannot cover an encode buffer for the entire journal, a cache of
decoded rows or another vector containing every full image.
Report retained image bytes, spilled bytes, key count and measured heap
separately when proving the bound.

## J2. Reads and ownership

Journal image access is fallible and on demand, for an entry's before or
after side. It distinguishes an absent row from I/O, truncated/corrupt image
or typed-row decode failure. It never logs a read error and returns absence,
silently skips an entry or substitutes a default row/key.

The immutable committed journal supports independent repeated iteration and
lookup without consuming shared cursor state. An access decodes only the row
requested; merely enumerating keys or cloning an owned journal handle reads
no image. Concurrent immutable readers must not race a shared file position.
The returned image belongs to this journal, never a later read of the live
collection. No collection reader transaction is held to keep these images
available.

A shared owned journal handle retains its backing storage until every undo,
observer and queued consumer handle is dropped. Discarding a SyncIncoming
journal from undo, truncating undo, closing the collection or committing a
later edit cannot change or invalidate an already retained commit view.
The temporary backing file disappears after its last owner drops.

## J3. Savepoints, composition and commit

Nested rollback restores the exact earlier after-image and corresponding
payload accounting; releasing an inner savepoint preserves what an outer
rollback still needs. Spill extents referenced by any surviving image cannot
be truncated or reused. First-before/final-after coalescing and the removal
of absent-before/absent-after entries remain unchanged.

Journal composition for undo merging preserves first-before/last-after and
the existing digest, changed-column and unchanged-row checks. The result
either owns the referenced backing stores or copies their images through
bounded storage. It must survive dropping either source journal. Savepoint
replacement and composition use bounded reads and keep accurate image byte
counts. Count images held through shared backing owners in the composed
journal's total too; ownership links do not increase J1's storage limit.

Before SQLite COMMIT, secure every compact fact needed for the authoritative
ChangeSet, including card note IDs, memory-state-only classification, schema
classification and collection-row changes. These facts describe the final
stored rows. Capture them during journaling or prepare them with bounded
reads while rollback remains possible. A failure there aborts the transaction.
After COMMIT, the ChangeSet uses only validated compact metadata. Do not read
spill data again or replace read errors with defaults. Follow
`engine/op-model` O4 for any error after successful COMMIT: the write stands
and cannot be repeated as though it had rolled back. S2 governs later reads
by observers.

## J4. Focused proof

Retain the existing 150k-note journal allocator regression and its 48 MiB
provisional journal-share assertion. That test uses note digests; it proves
neither large card after-images nor observer/queued-review memory behavior.

Add isolated allocator-measured coverage of 150k changed cards with substantial
variable row data, from before the write through commit derivation, real
observer traversal and active review capture/delivery. Exercise both image
sides beyond the threshold. Keep undo or a queued observer view alive, make
a later conflicting edit, then reread the earlier view twice and verify its
exact images. Also verify undo/redo, nested savepoint rollback/release and
composed journal reads after dropping the source owners.

Compare the same keys at small and large row widths, preparing fixtures before
the measured interval. Assert the configured held-image limit and bounded
additional payload heap, accounting explicitly for the implementation's
maximum live rows/page and codec buffers. Key-count metadata may remain
O(changed keys); increasing row width must not create O(total image bytes)
resident storage in the journal or any downstream consumer. Set the heap
assertion from the actual threshold and maximum row/page buffers. Do not
replace it with a higher process-memory budget. Count image reads, or provide
equivalent focused evidence, to show that taking or queueing a view does not
first read all images.

Use a dedicated counting-allocator binary, such as the existing PeakAlloc
harness, with no unrelated tests sharing its measurement. It measures Rust
heap only. The existing 150k production normal-merge and suspend tests remain
required timing/durability checks; without allocation measurement they are
not memory proof. Process memory remains the declared perf metric: Apple
phys_footprint or Linux RSS via the existing memory sampler. R1851 requires
at most 150 MB on iPhone in normal use at 150k and 500k cards; R1852 requires
at most 300 MB during sync at 500k cards. No separate numeric U107 150k heap
budget replaces those requirements. Keep device/process-footprint acceptance
distinct from this focused Rust regression.

# Collection file export

Spec: package/file-export
Requirements: R1619, R1624, R1625, R1635, R1852, R2004, R2015, R2046, R2059
Anki: none (Meguri app flow over package/container P5/P10/P11/P15)
Scenarios: none (focused Rust/Swift tests and native VM journeys in E6)

This contract covers RF3's owned file-export flow and RF4's media-inclusive extension,
a partial U124/U162 delivery. Collection-only internal writers remain
`package/container#P10` and `#P11`; user file export adds `#P15` and E7.

## E1. Scope and entry

Offer “Export Collection” from Settings on every platform.
Produce a latest-format whole-collection .colpkg including media by default (E7).
Show that media is included before creation; no separate option is needed in this delivery.
No deck/selection scope, legacy format or scheduling switch is implied.
R1625’s complete option set remains part of U124/U162; this extension does not accept those units.
That option set and `.apkg` deck and selection export are `package/apkg-export` (A14, A1–A13).
Local file export needs no Apple account, server, network, sync permission or cooldown.
FSRS admission and unsupported presets cannot block a faithful collection snapshot.
Require an open Ready collection; explain NoCollection, failed and exclusive-busy states.
File export neither replaces the collection nor resolves its cloud publication fence.
It never updates backup reference/retention metadata or starts an Anki request.

## E2. Snapshot and job

Use the existing live snapshot path and the E7 media capture on the bounded background job pool.
Apply `package/container#P10` plus `#P15` disk preflight before writing.
Capture source generation and admission sequence from the admitted collection handle.
Keep the engine and Maintenance reader leases through the existing live-writer call, including compression and verification.
Use VACUUM INTO; never copy live database bytes or hold the writer lock for compression.
Drop those leases before Ready delivery. Replacement must drain/cancel the job before closing its reader.
Perform all storage SQL in meguri-storage; do not factor the writer merely to release its reader earlier.
Include every committed review preceding the snapshot's first read; later commits may follow it.
On the bounded export worker, run review.flush_for_background() before snapshot acquisition.
Its wait may take five seconds. The short FFI starter must return without waiting for this barrier.
Do not replay answers, end the session, freeze later input or advertise pending answers as durable.
Record snapshot_modified from the closed captured copy, not from the later live collection.
Admission sequence identifies the request; it is not an exact snapshot revision or proof of currentness.
If an exact snapshot sequence is exposed later, obtain it from the same snapshot, never guess it.
Report queued/snapshot/write/verify/ready progress honestly; indeterminate stages may be indeterminate.
Cancellation requests wait for the real terminal outcome, including during non-interruptible snapshot work.
After draining/dropping leases, recheck generation before Ready; replacement makes unfinished output stale and disposable.

## E3. Owned output artifact

The Rust job writes a unique, app-private completed artifact using Purpose::Export and ExclusiveCreate.
Keep Ready files in an operation-owned output directory outside the collection/backups directories
and outside U32's .meguri-stage-* and .meguri-tmp-* sweep namespaces.
Snapshot/write staging follows `package/container#P9` and U32; its crash cleanup remains unconditional.
Apply the existing streaming writer, frame/container verification and durable publication rules.
Expose an opaque owned artifact plus bounded metadata, not package bytes across FFI.
Metadata: operation id, source generation, admission sequence, snapshot_modified, byte length,
fixed/sanitized suggested .colpkg name, and streaming SHA-256 identity of the completed package.
An artifact may provide a read-only file URL for native handoff; callers cannot choose its private path.
The handle owns that immutable file independently of the source reader/collection lifetime.
Keep at most one active export and one completed artifact per export flow; refuse duplicate starts.
The result owner retains ownership through job tracking, including setup failure and cancellation.
Reuse Jobs.trackDraining/terminal-result patterns; never delete while a worker still writes or reads it.
Provide idempotent discard; close handles before removing only the operation's files.
A released/stale handle cannot be reused. Retry destination selection may reuse a still-owned Ready artifact.
Once Ready, later ordinary commits do not change its contents; it is a point-in-time export.
A collection replacement before destination confirmation invalidates this flow; require a fresh export.

## E4. Native destination and lifetime

Mac: use NSSavePanel with .colpkg and native replace confirmation.
iPhone/iPad: use URL-based Files export with a platform-owned copy.
Support compact and regular native presentations on iPad. Sharing is deferred from the initial implementation.
Never construct Swift Data, an in-memory FileWrapper, or a FileDocument snapshot holding the package.
Copy through bounded streams or a proven file-to-file platform API off the main actor.
Keep destination security scope and synchronous file-coordination access alive until all I/O ends.
Use the coordinator's returned URL, not the initially selected URL.
For app-controlled save, preflight space on the destination volume using an authorized location.
Stage output on that volume in an authorized temporary sibling or Foundation item replacement directory
chosen for the coordinated destination. A selected-file grant does not imply access to its parent directory.
Verify staged length/digest and sync before publication; validate both staging and final destination paths and aliases,
even when the Rust writer receives only the staging path.
Use safe atomic publication: create a new destination without overwriting a concurrently created item,
or replace an existing destination through the platform's safe replacement operation.
Foundation replacement may use its same-volume staging directory; preserve any original-item recovery location on error.
Keep the E5 cancellation boundary and uncertain-publication result; an error during replacement does not prove the old path is unchanged.
Never truncate/delete an existing destination before the replacement is ready.
Reject any destination aliasing the collection, SQLite sidecars or the private export artifact.
If a provider cannot safely replace an existing file, offer a new copy or refuse replacement.
Do not fall back to an in-place overwrite. Retain the artifact so the user can choose another destination.
For Files export, hand off a copy; never transfer ownership of the app's only completed artifact.
Keep the artifact lease until the platform has its own independent copy or every provider read has ended.
Sheet dismissal alone is not proof that an asynchronous provider finished reading.
Report “Saved” only after destination completion. This does not prove a provider completed its cloud upload.
The user may cancel the destination panel and retry while the Ready artifact remains owned.

## E5. Cancel, failure and restart

Before destination publication, cancellation/error preserves any existing destination unchanged.
After atomic publication, a cancellation request cannot relabel a completed save as cancelled.
After rename, a sync or completion error requires an uncertain-publication result,
not “old file unchanged”; resolve through the recorded destination identity where access permits.
Export failures never mutate source rows, generation, backup records or publication decisions.
Report destination-copy errors apart from package-generation errors. Reuse a valid snapshot for a destination retry.
On background, use the existing job/lifecycle policy; never promise an unbounded background task.
On restart, do not silently export again or restore expired security scope.
U32 still unconditionally sweeps its temporary snapshot/write staging at launch. Ready output is never stored there.
The existing artifact owner handles its own output directory; add no general cleanup framework.
Remove completed output only after provider ownership ends or an independent provider copy is confirmed.
If that cannot be proven after process death, retain the Ready output outside the swept paths.
Keep user files and backups. Limit new exports while owned outputs await safe removal.

## E6. Reuse and focused acceptance

Existing Rust: write_collection_package_from_live, CollectionPackageWriteContext,
LiveCollectionPackageWritten.snapshot_modified, Finalisation::ExclusiveCreate, AtomicFile and staging_directory.
Existing engine: Collection.generation/last_seq/reader(ReaderRole::Maintenance); review flush barrier.
Existing FFI/Swift: jobs::start/JobHandle, Jobs.trackDraining, CoreEventStream, lane signposts,
CollectionEntryService's owned-result/cancellation pattern and CoordinatedSourceOwner's scope-lifetime pattern.
New seams only: typed export job/result/artifact DTOs, native service, export flow and destination adapter.

Focused checks: inspect/restore exported bytes and compare the captured snapshot; answers continue during compression;
flush failure prevents a false Ready; concurrent writes do not falsify receipt identity; replacement rejects stale work;
large/sparse fixture copy has bounded RSS and zero whole-package Swift allocations; destination aliases refused;
existing destination preserved under copy/disk-full/pre-rename cancellation; uncertain post-rename failure is truthful;
job-tracking failure drains before cleanup; delayed provider read outlives sheet dismissal; retry/discard/restart cleanup.
VM journeys: Mac Save/replace/cancel; iPhone Files; iPad native presentation.
Retain RF2 re-import and durable review for empty-media, collection-only exports only.
Media-bearing exports require E7.4’s independent archive and compatible-client import proof.
Offline/missing-account/quota status never hides local export.
Full U124/U162 scope remains open: sharing, media installation during file import, legacy, deck/selection export, complete options, remembered settings,
all file intake routes/menus and broader package interoperability/performance matrices.


## E7. Media-inclusive user export

This section applies only to the explicit user file-export flow. Backups,
retained server copies and damaged-collection copies keep their existing
collection-only contracts. Never change their defaults to include media.

### E7.1 Inventory and names

List the files directly from the admitted generation's `collection.media`
folder. Include every regular media file: externally added, unreferenced,
`_`-prefixed, zero-byte and larger than 100 MiB. The journal and the references
in notes, templates or CSS must not restrict this list. Read no other media
root, trash, staging area, child folder or symbolic-link destination.
This limits payload selection. The protected metadata checks in E7.2 add no
files to the archive.

Apply `media/media-files#M8` only to exclude operating-system metadata. Flat
subdirectories are not media (`#M1`); never recurse into them. A symbolic link,
other nonregular nondirectory entry, unreadable media file or name that cannot
be represented safely must refuse the whole export with an actionable error.
Do not silently skip that file or follow the link. Refuse a media directory
that is itself a link or is replaced during capture.

Use UTF-8 media names that satisfy `package/container#P6`. On Apple platforms,
represent the filesystem name in NFC as `media/media-files#M7.1` permits;
retain the original directory-entry spelling for source access. This conversion
must not rename or rewrite the source. On other platforms a non-NFC name is
refused. Refuse invalid UTF-8, unsafe names and duplicate archive names after
NFC conversion. Distinct safe case variants remain distinct map entries with
their own numbered payloads. Never lowercase, deduplicate by hash, rename files
or rewrite collection references during this whole-collection export.

Order entries by the archive name's UTF-8 bytes for a deterministic manifest.
An absent media directory means zero media files. A present path that cannot be
safely inventoried is an error, not an empty directory. Validate name, count,
individual length and aggregate limits before producing a Ready artifact.

### E7.2 Capture and concurrent changes

Capture media as immutable operation-owned data with bounded streaming I/O;
reuse U32 staging and E3 ownership. Never build whole media payloads in memory
or pass them to Swift. Open each exact directory entry relative to the
validated media root without following links, require a regular file and keep
its descriptor while copying. Reject any alias of the collection, its SQLite
sidecars, operation output or internal staging.

P16/R6 may leave directories and plans inside `media.staging`; permit that tree
as protected state. Freeze the retained trees present at export preflight.
A nested directory is not by itself an export error.
Visit its files and directories through directory descriptors, with finite
traversal bounds and no symbolic-link resolution. For every entry, save its
native identity, name and containing-directory binding. On entering a directory,
prove that its opened identity matches the recorded parent entry. Repeat that
binding check when leaving. Keep the full inventory and check it after capture
and after the final package digest, before Ready. Any addition, removal, rename,
replacement, metadata change or ancestor rebinding fails the attempt wherever
it occurs. Checking only the root timestamp cannot establish this proof.

Retain the ceiling of 400,000 protected-entry visits, including repeated inode
identities. Use finite limits for depth, memory and simultaneous descriptors.
Observe cancellation while walking and while checking the saved inventory.
An unreadable entry, link, special file, changed binding or media hard link to
any protected descendant requires refusal. Cancellation or any exceeded limit
also stops export. No branch may be omitted to obtain a successful result.
Use the current invalid-source, changed-source, cancellation and limit results;
public host/FFI behavior stays the same. Plans, journals, recovery data, staging
and trash must never become archive payloads. Leave retained plans and saved
media untouched. P16/R6 continue to define storage, retention and recovery rules;
export adds neither another layout nor a cleanup pass.

Do not apply this frozen-tree rule to the current export's U32 operation stage
or output. Its owner may create these after preflight, write during capture or
package creation, and remove staging before delivery. Keep ownership, binding
and alias checks for every such item. Accept its owner's writes and stage
cleanup under E3. Refuse a different owner, changed authority or replaced
binding. Validate the finished artifact through E3; the writable output is
not part of the retained source inventory.

Record file identity, length and high-resolution modification/change metadata.
Check them around each capture and again before Ready. Rescan the root's exact
inventory before Ready to detect additions, removals, renames and replacements.
The directory timestamp alone cannot detect an in-place rewrite. A changed,
vanished or replaced source invalidates this attempt; discard unpublished work
and ask for an explicit fresh export. No automatic restart loop or partial
success is permitted. Tests must exercise an in-place content rewrite while
copying, including a same-length rewrite, and a directory-entry replacement.

The package contains each captured file's exact bytes and the consistent
collection snapshot from E2. These have separate capture boundaries; there is
no cross-filesystem transaction and no claim of a single atomic database/media
instant. Completed capture data is immutable even if the live file later changes.
Do not acquire the collection writer for enumeration, copying or compression,
and do not block review commits on a long media-store or engine lock. Drain or
cancel through the existing export/replacement leases; generation checks apply
to the entire package. Export must not register media, update journal/pending
state, change names or mutate source content.

### E7.3 Container and limits

Use `package/container#P15`: every captured media file has one numbered zip
entry and one P5 map record with its exact decoded length and SHA-1. Zero bytes
is valid and has SHA-1 of the empty byte sequence. The protobuf uint32 length
admits at most 4,294,967,295 bytes per media file. Refuse a larger file before
writing; never wrap, truncate, omit it or substitute the server's sync limit.
Keep the existing P8 limits, including 400,000 zip entries, 64 MiB decoded map
and 16 GiB total decoded package data. Report a resource/format-limit refusal
as such, not as media successfully excluded. This is the existing package
safety envelope, not a CloudKit or server quota.

Reread the finished archive without relying on the original media paths.
Check the complete set of numbered entries and map records, each name and
length, zstd frame completion, frame checksums and streamed SHA-1 digests.
Check the captured collection's length and SHA-256 as before. Reject duplicate,
absent or unexpected numbered payloads. Enforce the same decoded-data bounds
in this check and ordinary reads. Release every verification handle before
publishing durably. Continue to apply E3–E5 to destination handling, artifact
ownership, cancellation and any uncertain publication outcome.

### E7.4 Focused acceptance and explicit intake boundary

Use generated media only. Verify a package with referenced, unused, underscore,
empty, Unicode, case-variant and greater-than-100-MiB files; decode its map and
payloads with an independent validator and compare every filename, byte length
and digest. A black-box compatible-client import must recover the collection's
actual saved reviews and every included media file. Preserve source database,
media bytes/names and journal/pending state throughout.

Extend every applicable E6 focused check to the complete E7 and P15 contract,
including each refusal and failure outcome and all safety invariants.
Import generated media through the actual public host, close and reopen without
reseeding, then export through that host. Keep R6 plans and any saved old media
present; compare their identities and bytes before/after. Independently check
the archive against fixed media/review expectations and exclude all internal
state. A directly assembled export fixture cannot prove this sequence.
Cover nested hard-link aliases, symlinks and special files. Change nested names,
contents, directory bindings and ancestors between inventory, capture and Ready;
require safe refusal. Exercise cancellation and each traversal bound while
proving that retained recovery evidence was not changed.

Native VM checks on Mac, iPhone and iPad must save a media-inclusive artifact
through the actual destination flow and validate the resulting bytes. Replace
all user-file-export “Without Media” copy only with this implementation; keep
collection-only safety-copy descriptions accurate.

Media installation in local file import follows `package/container` P16, a
separate U122/U124 leg. Until that complete path is implemented and verified,
explicitly refuse nonempty media maps before replacement with a clear message.
Do not silently import only the collection or advertise a media roundtrip from
export proof alone. Container validation and user intake policy remain separate;
adding the export verifier must not enable an incomplete import. E7.4's
independent archive/client proof and fixed expected-source rules remain required.

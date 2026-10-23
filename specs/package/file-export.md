# Collection-only file export

Spec: package/file-export
Requirements: R1619, R1624, R1635, R1852, R2004, R2015, R2046, R2059
Anki: none (Meguri app flow over package/container P10/P11)
Scenarios: none (focused Rust/Swift tests and native VM journeys in E6)

This contract covers RF3's latest-format collection-only export, a partial U124/U162 delivery.
Package format and verification remain `package/container#P10` and `#P11`.

## E1. Scope and entry

Offer “Export Collection — Without Media” from Settings on every platform.
Make the omission visible before export; produce latest-format whole-collection .colpkg only.
No deck/selection scope, legacy format, scheduling switch or media option is implied.
Local file export needs no Apple account, server, network, sync permission or cooldown.
FSRS admission and unsupported presets cannot block a faithful collection snapshot.
Require an open Ready collection; explain NoCollection, failed and exclusive-busy states.
File export neither replaces the collection nor resolves its cloud publication fence.
It never updates backup reference/retention metadata or starts an Anki request.

## E2. Snapshot and job

Use the existing live writer and `package/container#P10` disk preflight on the bounded background job pool.
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
For app-controlled save, preflight, write a temporary sibling, verify length/digest, sync, then atomically publish.
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
VM journeys: Mac Save/replace/cancel; iPhone Files; iPad native presentation; real RF2 re-import
and durable review of the exported file. Offline/missing-account/quota status never hides local export.
Full U124/U162 scope remains open: sharing, media, legacy, deck/selection export, complete options, remembered settings,
all file intake routes/menus and broader package interoperability/performance matrices.

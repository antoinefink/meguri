# RF2: safe local collection entry

Anki: none for this app flow; package facts come from the prepared neutral specs.
Scope: parts of U124a, U126, U127 and U166. No unit completion is claimed.

## RF2.1. Supported input and ownership

Accept latest-format, collection-only `.colpkg` through a user file picker.
Show the supported format before selection. No Apple account or network is required.
Reject legacy/deck packages and packages with declared or carried media files explicitly.
Never import only the database while silently dropping package media.
Do not accept raw databases as a shortcut. Preserve the selected source without modification.
Keep unavailable sync, backup-management and advanced import UI outside this flow.

## RF2.2. Source access and inspection

Own security-scoped access for the complete coordinated read on every platform.
Copy the selected source into operation-owned app-private staging in bounded chunks.
Preflight space for that copy, inspection, required upgrade and safety backup before writes.
Keep the copied package stable for the job; release source access on success, failure or cancellation.
Remove only this operation's unpublished files. Never delete or modify the selected source.
Use `package/container#P8–P12`: bounded decoding, safe names, existing open gate,
copy-only upgrades, integrity check and an owned inspect handle.
A bad or unsupported package cannot close or change the current collection.
Present inspected note/card/review counts and the destination before confirmation.
Show that existing local content will be replaced; require an explicit confirmation.
Refuse activation while a review session is open. Recheck that guard at exclusive entry.

## RF2.3. Protect and activate

Use the existing lifecycle long-operation slot and exclusive drain.
If a collection file exists, take a real forced safety backup before replacement.
Reuse `package/container#P11`, `package/backups#B2–B4` and `package/backup-triggers#T6–T7`.
An empty but existing collection still requires backup. Only no file permits no backup.
Register the real package backup provider through the composition root.
A missing provider or failed backup refuses replacement; no success stub or fixture witness.
Keep the published backup recoverable through the operation; failure never removes it.
U166 owns the ColpkgImport request and its `scm`/`mod` change. Apply the existing R7
package stamps exactly once through verified replace, using one injected instant.
Do not add a second timestamp write in the picker or bypass storage-owned SQL.
`engine/collection-replace#R3–R6` checks again, finalises, journals, swaps and reopens.
After activation, old handles expire and one CollectionReplaced event resets app state.
Use the new identity and pending publication fence even when CloudKit is unavailable.
A failure after activation reports that the new generation remains active; never promise rollback.

## RF2.4. App integration and continuation

Expose inspection and import as cancellable off-main-thread jobs with typed results/errors.
Inspection owns its staging handle; do not expose an arbitrary path for later replacement.
Bind activation to that handle, selected source and confirmed destination generation.
Reject stale confirmations after replacement or new local work; refresh the preview.
Show progress for copy, inspect, safety backup and activation; block conflicting input.
Permit cancellation before the commit boundary. After activation, finish recovery instead.
On success, dismiss entry, refresh the actual deck tree and allow RF1's real review path.
Restart loads the imported collection and retains its durable recorded answers.
No fixture launch hook is required for a user's import.

## RF2.5. Acceptance and retained scope

Prove source bytes unchanged; supported import preserves expected collection contents.
Prove malformed, unsupported, media-bearing, cancelled and low-space cases leave old content intact before activation.
Prove a recoverable pre-import backup contains every committed old answer.
Exercise safety-backup failure, session race, stale confirmation and each R6 crash window.
Import through the picker on iPhone, iPad and Mac; review, background and restart.
Use generated/committed synthetic inputs only. UI checks run in the test VM through tools/mg.
Focused checks, independent review and the RF2 full integration gate remain required.
The prepared package scenarios remain inactive until their own adapters/evidence pass.
RF2 does not require full periodic backup scheduling, retention UI, CloudKit or full package support.
Keep those unit requirements open. Retain prepared scenarios and provenance for their later implementation.

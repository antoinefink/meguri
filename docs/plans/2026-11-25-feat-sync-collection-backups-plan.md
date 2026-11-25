# Scheduled collection backups on Meguri Sync

## What the account owner can do

On the signed-in Collection page, browse dated recovery points and restore a chosen point to the server. A recovery point covers the collection database, including review history and scheduling, and the media associated with it. Each ready account gets its own history, including accounts that never open the web page.

## Implementation

1. Capture once per UTC day while serving. Run a bounded background sweep on startup and periodically afterwards. Catch up with today's collection following downtime. Use committed SQLite views and account-owned durable media under the server's existing publication and reset rules. Publish only complete recovery points and isolate failures between accounts. Keep one point per day for seven days, then one per week for four weeks, then one per month for twelve months. Promote existing points and avoid duplicate payloads where possible.
2. Expose a session-authenticated history and same-origin restore for the signed-in account. Use opaque backup IDs. Verify the selected point and preserve the pre-restore state before replacing current data. Recovery must survive process interruption and fence stale writes. Clients must be able to download the restored database and reconcile added, changed and removed media. Account deletion and permanent data reset remove online history. Offline operator recovery remains a separate workflow.
3. Add a backup history section to the existing Collection page with date, retention tier, size and a Restore action. Confirm the date, effects and device recovery instructions before submitting. Support loading, empty, failure, cancellation and completion states at narrow and wide widths. Preserve the existing storage display and keyboard navigation.
4. Exercise daily idempotence, tier boundaries, missed time, cross-account isolation, interrupted or invalid backups, restore synchronization and cleanup. Use synthetic study data to make a backup, change the collection and recover it. Capture the rendered history and restore confirmation from the MG browser fixture. Attach both images to the pull request using GitHub CLI.

The proposed defaults beyond seven days are four weeks and twelve months. Backups live on the server's persistent volume and protect against collection changes. Protection against losing the volume still requires an operator backup.

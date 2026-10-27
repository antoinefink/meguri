# CloudKit recovery

Spec: sync/cloudkit-recovery
Requirements: R2051, R2054, R2055, R2056, R1548, R1593 (connecting: V13), R2066 (recovery contract); supports R89, R117, R125, R158, R169, R458, R1553, R1555, R1558, R1559, R1561, R1562, R1592, R2010, R2012, R2043, R2046, R2049, R2057, R2059
Anki: none (Meguri design)
Scenarios: none yet
Status: Approved 2026-10-27 (R2066). Implement U113 and U114 against this contract; first settle the open items of `sync/policy#P24`.

How Meguri stays correct when something goes wrong around CloudKit sync: the crash windows of every step of sending and receiving, the durability rules for the cursor and the outbox, account and service conditions, conflict copies, the publication fence after a restore or an import, the first connection, the two replace directions, and what the damaged-collection screen may offer. Record formats, merges, tombstones, epochs and structural generations are `sync/cloudkit-records` (the K ids below). Eligibility, hold-back, the Sync Now boundary and status computation are `sync/policy` (the P ids below).

Every failure here has a defined outcome: what is durable, what the device does next, and what the user sees (V11). No failure loses a committed local change, sends data the user did not choose to send, or replaces a collection without the user's confirmation.

Manual Anki transfers (KTD26, frozen) are not a recovery path. No remedy, choice or retry in this file contacts an Anki server.

Lines marked **Assumption** state Apple behavior this design depends on and has not verified. V24 lists them as A11 to A19, after the A1 to A10 of `sync/cloudkit-records` K23.

## V1. Terms

- **Binding**, **usable**, **pending**, **acked**, **flushed**, **epoch**, **structural generation**: K1, K10.2.
- **Cursor**: the sync engine's state serialization, which holds its database and zone change tokens. Tracking stores it in `engine_state` (K10.3). Meguri never reads tokens out of it.
- **Outbox**: tracking's pending entities (`entities`) and blob upload states (`blobs`, K10.3). The engine's own pending list is a copy of it (K12.4).
- **Engine instance**: one CKSyncEngine object, created for one usable binding with that binding's saved cursor or none. It has a random instance id, and every fact the Swift actor passes to the core carries it (V3.4).
- **Staging fetcher**: the fetch that fills a staged collection for a `prefetch` or `adopt` op (V13.2, V14.2 step 4). It is not an engine instance and belongs to no binding (V3.4).
- **Isolated binding**: a binding in state `isolated` (K10.2), with an isolation reason (V2.2). Its pending rows, intents and conflict copies are kept. Its inbox is kept too, except when it is isolated as `zone_deleted_by_user` or `encrypted_data_reset`: then K12.5 deletes, with the isolation, what tracking holds only as a copy of the server (the inbox entries that no apply intent names, their staged files, and its `refused` rows). Nothing runs against it, and the tracking observer records no dirty mark for it (V2.2, "Restoring a binding").
- **Publication fence**: `cloudPublicationPending` true in the sidecar's `deviceState` (`engine/collection-replace#R6.1`).
- **Recovery decision**: a question only the user can answer: the fence (V12), a first connection (V13), an isolated binding that needs a choice (V9, V10), or an incomplete generation (V18). Decisions are computed from durable state at every start, so they survive relaunch.
- **Empty collection**: a local collection with no row in `notes`, `cards` or `revlog`, no deck other than id 1, and no tracked media file (`media/media-files#M7.1`). Note types, presets and config do not count.
- **Recovery op**: one of the user-started operations of V13.2 (prefetch), V14, V15 and V18. Each has a durable record in tracking (V2.3).
- **Withheld**, **superseded**: the two states of a local media file left from the collection that an adoption replaced (V14.4).
- **Sent after the replacement**: the changes of a binding acknowledged after its confirmation mark, a counter value taken before the server read of its last confirmation that `MGActive` named its epoch, once `MGActive` names another one (V10.2).

## V2. Durable state

### V2.1 Where each fact lives

| Fact | Store | Writer |
|---|---|---|
| collection rows | `collection.anki2`; durable once flushed (K1) | ops |
| publication identity, fence, prior decision, replacement intent | sidecar `deviceState` (`engine/collection-replace#R6`) | the replace primitive; U113 for the decisions of V12, V13, V14, V15 |
| bindings, cursors, entities, inbox, intents, generations, blobs, conflict copies, refused records, recovery ops | `meguri-cloud.db` (K10), durable commits (K10.1) | the core, with facts from the Swift actor |
| staged incoming assets, blob downloads, blob upload clones | the cloud staging directory on the media folder's volume (K9.3) | the core; swept as V6 says |
| a staged collection for a prefetch or an adoption | a collection staging directory (`package/container#P9`) | V13.2, V14 |
| safety backups | the backups folder (`package/backups#B1`) | `package/backup-triggers` T6, T7 |
| previous epochs | their collection zones in CloudKit, 30 days (K18.5) | the publisher creates them; any device bound to the active epoch deletes them (K18.5) |
| iCloud enabled, paused (V16), automatic sync | device settings (U151 `DeviceSettings`), never in the collection | Settings |

The collection file and the tracking file are separate SQLite files, and no transaction spans both. Every crash window of V4 to V7 is safe by one of three rules:

1. Tracking records only facts about flushed commits (K10.1).
2. A collection change made from server data has a durable intent written before its commit (K13.2, K14.2, V2.3).
3. Anything else is rebuilt by the reconcile scan (K11.4).

### V2.2 Isolation reasons

A binding becomes isolated only for one of these reasons, which tracking stores with it (V25):

| Reason | Set by |
|---|---|
| `signed_out` | V9.1 |
| `account_changed` | V9.1 |
| `environment_changed` | derived, never stored: a binding of another container or environment, as the running build reads it (V9.1) |
| `epoch_switched` | another device made a new epoch active (K18.4, V10.2) |
| `zone_missing` | V10.1 |
| `zone_deleted_by_user` | V10.1 |
| `encrypted_data_reset` | V10.1 |
| `active_missing` | V10.2 |
| `replaced_locally` | a restore, `.colpkg` import or manual Anki import swapped the collection (V12.1), or V14 replaced it |
| `file_mismatch` | V8.2 |
| `stopped` | the user stopped iCloud on this device (V16) |
| `republished` | this device replaced the iCloud collection; the binding of the replaced epoch, when it was active (V15.2 step 2) |
| `tracking_damaged` | the salvage binding that holds conflict copies read from a damaged tracking file (V8.1) |

- A binding stores one reason. The account reasons (`signed_out`, `account_changed`) replace a service reason (`epoch_switched`, `zone_missing`, `zone_deleted_by_user`, `encrypted_data_reset`, `active_missing`) when their event comes later, and a later account event replaces an earlier account reason. Nothing replaces `replaced_locally`, `file_mismatch`, `stopped`, `republished` or `tracking_damaged`.
- `environment_changed` is never written (decided 2026-10-27, V9.1). No event replaces it and it replaces nothing: the binding keeps the state and reason that the build of its own container and environment stored.
- The stored reason does not limit adoption: V14 is offered from a fresh read of `MGActive`, whatever the reason (V10.1, V14.1).
- Before it isolates for a service reason, the actor reads the account status and identity again (V9.1). When they differ from the binding's, the V9.1 outcome applies instead.
- An isolated binding never becomes active again by itself. Only a resume (V9.2), the Resume of a waiting `publish` op whose new binding an account event isolated (V15.3), or a cancelled recovery op that restores its recorded state (V14.3, V15.3) makes it active again, each after an explicit user action. A recovery op that replaces it (V14, V15) leaves it isolated.
- **Restoring a binding** (decided 2026-10-27). Nothing runs against an isolated binding (K10.2), so the tracking observer records no dirty mark for it, and edits committed while it was isolated are not in its tracking (during a publication they are tracked under the new binding, K18.3). Every path that makes an isolated binding active again (V9.2's Resume; V15.3's Resume of a `publish` op's new binding; V14.3's and V15.3's return of the binding before to a recorded state `active`) does so in one tracking commit that also clears the binding's `scan_state` drained flag (K11.4), and sets the scan-required flag (K11.1) in the running process. Then, before the scan, the binding's re-key intents, apply intents and media apply intents are settled as V2.4 step 8 does (K14.2, K13.2, K9.3; decided 2026-10-27): a settle that came due while the binding was isolated did not run, and until it runs K12.1 rule 8 keeps the intent's entities unsendable. The reconcile scan (K11.4) then runs before any engine instance of that binding starts, so those edits become pending and are sent to its epoch. After a crash, the cleared drained flag makes the next open run the scan (K11.4). A return to a recorded state `isolated` needs no scan now: a later resume runs it.
- **The base of those edits** (decided 2026-10-27). The observer captured no base for them (K11.2), and the binding's fetch then brings the other devices' versions of the same entities. Tracking therefore keeps the acked payload of every acked entity, not only of media names, and a pending entity that has no base merges with its acked payload as base (`sync/cloudkit-records#K10.3`, `#K11.2`, `#K15.1`; V25). For an entity that was clean when the binding was isolated, the acked payload is exactly the content its edits started from, so the merge takes from the local version only the parts those edits changed: a tag another device removed meanwhile stays removed, and another device's flag, deck name or field edit is kept. A merge without a base (K15.1) is left only for an entity that was never acked.

### V2.3 Recovery op records

Tracking table `recovery_ops` (V25), one row per recovery op:

| Column | Meaning |
|---|---|
| op id | 16 random bytes; for an adoption it is also the `engine/collection-replace#R6.2` operation identity |
| container and environment | those of the build that created the row (decided 2026-10-27, review round 4; below) |
| kind | `prefetch`, `adopt`, `publish` or `abandon_generation` |
| step | the last step made durable (V13.2, V14.2, V15.2, V18) |
| binding before | the binding the op replaces or acts on, with its state and isolation reason when the op started; or none |
| binding after | the binding the op creates, or none |
| epoch | the epoch id the op stages, adopts or publishes |
| source | `adopt` only: `active` or `previous` (V14.1) |
| candidate identity | `adopt` only: the staged collection's publication identity |
| active change tag | `publish` only: the change tag of the `MGActive` version the user confirmed replacing, or none |
| active system fields | `publish` only: the encoded system fields of that `MGActive` version, taken from the preview read's root facts (V26.2 `publish`), or none |
| active epoch id | `publish` only (decided 2026-10-27, review round 4): the `epoch_id` that `MGActive` version names, taken from the same root facts as the two columns above and set with them (V15.2 step 2 from step 1's preview, V15.2 step 5's Reuse from the last root facts), or none when there was no version. It becomes the `previous_epoch_id` of the `MGActive` this op saves (K18.3 step 5, V26.1) |
| epoch system fields | `publish` only: the encoded system fields of the acked `MGEpoch` `publishing` version, written by the commit that accepts that save's result (V26.1); none until then |
| switch sent | `publish` only: set true in a commit made before the core first reports the `MGActive` save due (V26.1); never cleared. From then on that save may have reached the server, with its result lost (V15.3) |
| switch conflict | `publish` only: the change tag of the `MGActive` version that V15.2 step 5's conflict returned, set in the commit that records that conflict (V26.1), cleared by `republish` (V26.2); or none. While it is set, no op operation of the op is due (V26.1), and the op waits for a choice of V15.2 step 5 |
| generation id, manifest change tag | `abandon_generation` only (V18) |
| manifest system fields | `abandon_generation` only: the encoded system fields of the manifest version that V18 step 2's read returned |
| fence before | whether the fence was pending when the op started and, when it was, the complete prior publication decision as the sidecar held it, its `pendingDecisionReason` included or absent (`engine/collection-replace#R6.1`, `#R6.7`; decided 2026-10-27), so a Cancel restores exactly that decision (V15.3) |
| staging | the staging directory, while one exists |
| started at | epoch milliseconds |
| retry after | epoch milliseconds before which the core reports no own operation of the op due again (V26.1), from the retry-after of its last failure; or none |

- **Rows of another build** (decided 2026-10-27, review round 4). Builds of separate containers share one tracking file (K10.2), and a row names its build's bindings, prior sidecar decision and account. A build sees only the rows of its own container and environment: the limits below, V2.4 step 3, `Busy` (V21), `recovery_state` and every recovery call (V26.2), the status's `recoveryOps` (`sync/policy#P3`) and the own operations of V26.1 count, settle, cancel or run only those rows. A build never settles, resumes or cancels another build's row, and never writes the bindings it names; that row waits for its own build. The staging directories and staged files such a row names are kept (V6).
- At most one `adopt` or `publish` row exists at a time per container and environment; `adopt`, `keep_local_only` and the new-epoch branch of `republish` first cancel a `publish` op whose switch conflict is set (V15.2 step 5, V26.2). At most one `prefetch` row exists per container and environment, and it does not count for that limit or for `Busy` (V21).
- A row is removed in the same transaction that makes its last step durable.
- Rows are settled at start, in V2.4 step 3.
- Whether an attempt of the op's own operation runs is kept in memory only, with the row (V26.1). A start begins with none running.
- Decided 2026-10-27: a conditional save needs the encoded system fields of the version it replaces; a change tag alone cannot be set on a record. So every own save that replaces a version (V26.1) takes them from these columns, and a change tag column only identifies the version the user confirmed.

### V2.4 Start sequence

At every launch, these run in this order. An engine instance is created only at step 10.

| # | Step | May change |
|---|---|---|
| 1 | `engine/collection-replace#R6.3` settles an unfinished replacement intent, before the collection opens | sidecar publication identity, fence, decision, intent |
| 2 | V8.1: the tracking file opens and passes its checks, or is replaced and salvaged | the tracking file; the salvage binding |
| 3 | recovery op rows of the running build's container and environment settle (V2.3, "Rows of another build"): V14.3 (`prefetch`, `adopt`), V15.3 (`publish`), V18 (`abandon_generation`) | bindings, op rows, staging directories, `scan_state` identity |
| 4 | V12.1: the open collection's publication identity is compared with the active binding's; then the crt check compares the open collection's `col.crt` with the binding's epoch `crt` (`sync/policy#P6`) | binding state; the `crtMismatch` summary flag |
| 5 | V8.2: the collection file identity is compared for the binding step 4 left active | binding state, `scan_state` identity |
| 6 | V6's sweep of the cloud staging directory | staged files |
| 7 | V9.1: the account status and identity are read; steps 8 to 10 wait for a binding until its identity is verified | binding state |
| 8 | K14.2's re-key intents, then K13.2's apply intents and K9.3's media apply intents, of the binding that is still usable. Decided 2026-10-27 (review round 4, K15.9 "One resolution, one intent"): the intent of a K15.9 resolution, which holds both names and the keys of its note-rewrite op, is settled as one unit, never as two intents: first the op's keys as K13.2 says at open (all at their before digests while the hash name holds the moved content: the op runs again), then the names as K9.3 says, continuing in order; one tracking transaction settles the whole apply | tracking entities and inbox; collection rows of replayed applies; media files of replayed media applies |
| 9 | the reconcile scan, when K11.4 requires it | tracking entities |
| 10 | the engine instance starts (`sync/policy#P10.1`, `Lifecycle{Ready}`) | none |

- Steps 1 to 6 need no network. Steps 8 and 9 need the collection `Ready` (`engine/lifecycle#L1`).
- After step 6 the core also deletes resolved conflict copies 30 days after their resolution, of every binding in the tracking file (`sync/policy#P10.3` item 4, decided 2026-10-27). This is a tracking deletion only and needs no network, gate or binding, so it runs at every collection open, also on a local-only device and for a salvage binding; the staged file a media copy holds is deleted after that commit (V6, V17.3).
- After an in-process replacement, the replacement's own R6.3 activation stands for step 1. Then V14.2 step 9 (an adoption) or V12.1 (any other replacement) runs before any instance.
- Study does not wait for steps 7 to 10 (R2046).

## V3. Ordering rules

### V3.1 The cursor (R2051)

1. A cursor is saved only in tracking, in the `engine_state` row of the binding whose engine instance reported it.
2. A cursor is saved only after every fetched record and record deletion the engine delivered before it is durably in the inbox (an entry of kind `record` or `deletion`, K13.1 step 2) or in `refused` (K13.1 steps 1 and 2; A7). A deletion of a record name that no entity and no inbox entry of the binding holds is dropped in that commit, as K13.1 step 2 says. The actor handles engine events one at a time, in delivery order, and awaits each durable commit before it takes the next event.
3. A cursor is never saved after a durable commit failed for an earlier event of the same engine instance. Such a failure stops the instance (V3.3).
4. Tracking never edits a cursor, never moves one to another binding, and never saves one for an isolated binding. A staging fetcher saves none (V3.4).
5. Losing a cursor is safe. A new instance that Meguri creates without one fetches the zone from the start, and that is the only fetch from no cursor Meguri relies on (A9a, V10.3); every apply is idempotent (K13.2), and that fetch also runs K17.4's absence check.

### V3.2 The outbox (R2051)

1. Tracking is the only outbox. A local change is pending when its row digest differs from the acked one, whether or not a dirty mark exists (K11.4, K13.2).
2. A dirty mark may be lost; the reconcile scan restores it (K11.4). A sent mark and an in-flight entry are durable before the engine gets the record (K12.1).
3. An acknowledgement is durable before the engine state that follows it is saved (A6).
4. A pending entity becomes clean only when its current row digest equals its acked row digest (K11.4, K12.3, K13.2, K15), when K17.1 drops it before any send, or when K14.2 moves it. A failed save, an account change, a zone loss, quota, throttling and a cancelled exchange leave it pending (R2054).
5. The engine's pending list is rebuilt from tracking at each instance start and after each reconcile scan (K12.4).

### V3.3 Event handling

- The actor never calls the engine's send or fetch from inside event handling (U114).
- When a durable tracking commit fails during event handling (an I/O error, a full disk, a constraint error, a panic in the core), the actor stops the engine instance without saving its state and drops the events still queued for it. The status gets reason `local_storage_full` or `tracking_error` (V11).
- Meguri holds no retry timer for this (U113). At the next trigger (`VolumeSpaceAvailable`, a foreground return, Sync Now or an account event, `sync/policy#P3`), the core tries one tracking write. When it succeeds, a new instance starts from the last saved cursor (`sync/policy#P15`).
- The new instance fetches again what the old one delivered. Inbox inserts are idempotent (V5).

### V3.4 Engine instances and late callbacks

- An engine instance exists only for a usable binding (K10.2) whose account identity was verified in this process (V9.1), and not while sync is paused (V16). When the binding stops being usable (V9, V10, V12, V16, a recovery op), the actor stops the instance first, then the core records the change.
- Every fact passed to the core (save results, fetched records, deletions, state serializations, account events) carries the instance id. The instance id travels with the guard of `sync/policy#P4`. The core accepts a fact only when the id is that of the current instance of a usable binding. Any other fact is the `StaleGuard` outcome: no tracking change, and it is logged. This covers callbacks that arrive after an account switch (U114).
- A dropped save result leaves the entity pending with its in-flight entry. If the binding is resumed later, the next save conflict or fetch settles it (K12.3, second row).
- Own operations (V26.1) are not engine instances: their results carry the op id (a preview read's carry the account identity instead) and follow V26.1.
- The staging fetcher is the one exception to the first bullet. It runs without a binding, only while its op row is at step `prefetch` or `staging`. Its facts carry the op id, not an instance id; the core writes them only into that op's staging directory and drops a fact whose op id is not that of a current row at one of those steps. It saves no cursor. It is a `CKFetchRecordZoneChangesOperation` with no previous change token, on the source epoch's zone only, run outside the sync engine (A14). It is never a CKSyncEngine object.
- At most one CKSyncEngine object exists per process, the current engine instance (decided 2026-10-27). A staging fetch, a prefetch and a recovery op never create a second one on the private database.
- Stopping an instance (decided 2026-10-27, `sync/policy#P4` `StopEngine`): the engine has no stop call, so the actor waits until the running handler has persisted, calls the engine's `cancelOperations` and awaits its completion handler, then releases the object and reports `EngineStopped`. No new instance is created before the old one's `EngineStopped`, also when `ConfigureAutomatic` recreates the engine. An operation that completes after the cancel carries the old instance id and is refused (above). Decided 2026-10-27 (review round 4): Apple does not document that releasing the object ends it. Its own running work may keep a reference to it, so the old object may outlive the release, and with automatic scheduling it may still fetch, handle a push or ask for a batch while the new instance runs. Only the per-instance callback object below keeps that harmless: every fact and batch request of the old object is refused, so it writes nothing to tracking and sends no record. Signed runs check that the old object ends within a bounded time after the release and delivers no event or batch request after it (V25).
- **A stale instance never gets a batch** (decided 2026-10-27). The cancel is asynchronous, and an operation of the old engine may still run, or ask for a batch, after it. So each instance has its own callback object (the object the engine's configuration names to receive its events and batch requests), created with the instance and holding its instance id; no callback object serves two instances. Every callback passes the id to the core: a request for the next batch (K12.1) included, not only the facts above. When the id is not that of the current instance of a usable binding, the core builds no batch and writes nothing (no `sent` mark, no in-flight entry), the callback returns no batch, and an event is dropped and logged (`StaleGuard`). So a late batch request of the old engine never sends a record while the new engine also sends (`sync/policy#P4` `StopEngine`, `ConfigureAutomatic`; V25).
- Development and daily-driver builds use separate containers (K2), so a binding of one never matches the other (V9.1).

## V4. Crash windows: sending

The steps of one local change, and what a crash (process kill, OS crash or power loss) after each step leaves. "Restart" means the next launch or the next instance start.

| # | Step done | At restart |
|---|---|---|
| S1 | the op commits (WAL) | after a process kill the commit stands and S2 applies. After a power loss before the flush the commit is lost, and nothing in tracking depends on it (K10.1) |
| S2 | the writer's flush covers it | the sidecar shows no clean shutdown, so the reconcile scan runs (K11.4) and finds the change |
| S3 | the tracking observer's dirty mark commits | the mark may be lost on power loss (K10.1); the scan of S2 covers it |
| S4 | the actor adds the record name to the engine's pending list | the instance start adds every sendable name again (K12.4) |
| S5 | the core builds the batch; `sent` and the in-flight entry commit durably (K12.1) | the entity is still pending; its record is built again from current rows |
| S6 | assets are staged (`pa`, `ma`) and uploads start | staged files of a record not saved are swept (V6) and built again at the next send. Bytes uploaded for a record never saved are not visible to any device (A11) |
| S7 | the server saves the record | the result never arrived. The next save gets "server record changed" with a server version whose `writer` is this binding and whose payload digest is an in-flight one, and counts as saved (K12.3), or a fetch delivers the version and K13.2 records it as saved (`sync/cloudkit-records#K13.2`, "Own landed saves", with the same `writer` test) and never applies it. Another device's version with the same payload is never taken for this one: its `writer` differs, and it is merged as usual |
| S8 | the save result reaches the actor | as S7 |
| S9 | tracking records the acknowledgement | the engine may list the name again or report the result again. A name that is not sendable is removed (K12.4); a result for a revision already acked changes nothing |
| S10 | the engine state after the results is saved | nothing to recover |

- A local edit made between S5 and S9 stays pending after the acknowledgement (K12.3, "Saved"); an ack never clears a newer revision (K21 invariant 4).
- A local deletion made between S7 and S9 makes a pending tombstone of an entity tracking holds as sent; K17.1 settles it against the server version.

A save whose result is "server record changed" with other content (K12.3, the same-origin row, and the other-origin row for a name-keyed kind once the entity takes the server origin) settles through the inbox (decided 2026-10-27, K22.2):

| # | Step done | At restart |
|---|---|---|
| C1 | the transaction that records the result: the server version S is checked as a fetched record (K20) and is in the inbox with source `save_conflict`; the entity keeps its acked system fields, `erev`, digests and base B | the entry is applied as R4 to R7 say: the op merges B, the current local row L and S again (K13.2). When S's payload is in an asset, the entry has reason `asset`: a crash before K12.3's first read by record ID leaves it to `RetryAssetEntries` (K13.1) |
| C2 | the apply intent is durable | as R4 |
| C3 | the `CloudIncoming` op commits | as R5. After a power loss before the flush the before digests match, and the merge of B, L and S runs again; the other device's version is never lost |
| C4 | the writer's flush covers the op | as R6 |
| C5 | tracking takes S's acked fields, `erev` and digests, the base becomes S, the entry is deleted and the intent removed | nothing to recover. An entity left pending (R not equal to S) is sent again under K12.1 |

- Until C5 the entity is not sendable (K12.1 rule 8), so no save of the merged row runs ahead of the apply.

## V5. Crash windows: receiving

| # | Step done | At restart |
|---|---|---|
| R1 | the engine fetched a batch (memory only) | the cursor is older; the batch is fetched again |
| R2 | the records are checked and put in the inbox with their staged assets, and the batch's record deletions as entries of kind `deletion`, in one durable commit (K13.1) | the cursor is older; the batch is fetched again. Decided 2026-10-27 (review round 4, K13.1): CloudKit does not promise the order of fetched changes, so the entries of one record name are ordered by `erev`, never by fetch order. A fetched record of the entity's origin whose `erev` is at or below the entity's acked server `erev`, or at or below that of an entry of that name and origin already held, is stale (the same version again, or an older page after a newer one): it is not inserted, it is logged, and its `seen_pass` is still set (K17.4). A version with a higher `erev`, or a deletion, replaces an entry that no apply intent names; otherwise it is queued after that entry, and the entries of one record name apply in `erev` order |
| R3 | the cursor covering the batch is saved | ready entries are applied before the instance sends anything |
| R4 | the apply intent is durable (K13.2) | the intent is settled by its digests (K13.2): finish, apply again, or scan and apply again |
| R5 | the `CloudIncoming` op commits | after a process kill the after digests match, and the tracking update finishes. After a power loss before the flush the before digests match, and the entries are applied again |
| R6 | the writer's flush covers the op | the after digests match; the tracking update finishes |
| R7 | tracking marks the entries applied, updates their entities and removes the intent | nothing to recover |

- The day clock's rollover check runs before every apply, a replay at restart included (K13.2).
- A replay never duplicates: applying a version equal to the local row changes nothing (K13.2), revlog rows are keyed by id, and media installs check content (K9.3).
- A `deletion` entry (K13.1, K17.4) passes R3 to R7 like a record. For a tombstoned entity it has no op and no intent: one tracking transaction drops the entity and deletes the entry, and a crash before it leaves the entry to apply again. For a live entity it is a deletion under K17.3, through a `CloudIncoming` op and its apply intent (R4 to R7). A replay finds the entity already dropped or its rows already absent, and changes nothing.
- A refused record (K20) is in `refused` before the cursor passes it. It is never applied; V11 shows it.

## V6. Crash windows: media

| Window | At restart |
|---|---|
| a blob upload: tracking marks the blob `uploading`, and the core makes an APFS clone of the media file in the cloud staging directory and checks its SHA-256, before the engine gets it | a blob still `uploading` is sent again from a new clone. The clone keeps the bytes stable while the media file changes or moves to the trash (A16) |
| the blob record is saved, its result lost | the next save of the same name counts as success (K9.3) |
| a download in progress into the cloud staging directory | the partial file is deleted; the blob is fetched again |
| a download complete, not installed | the staged file is checked again (SHA-256 and size) and installed, or deleted when it fails |
| an install in progress (the exact-name write, `media/media-files#M6`) | the existing valid file is intact (R2057); the install runs again |

- At start (V2.4 step 6) the core deletes every file in the cloud staging directory that no inbox row, blob row, `conflicts` row or recovery op names. A `conflicts` row names the file K16 downloaded for a media copy whose bytes were not on the device ("Media copies whose bytes are not on the device"). Decided 2026-10-27: that file stays while the `conflicts` row exists, resolved or not, so `unresolve` (V17.3) still finds the bytes; it is deleted after the commit that deletes the row (30 days after resolution, V17.3), and the sweep deletes it if a crash comes between.
- Collection recovery never waits for media. A media-name entity can apply before its blob (K9.3).

## V7. Crash windows: other operations

| Operation | Durable state that makes it safe | At restart |
|---|---|---|
| re-key | the re-key intent | K14.2 |
| generation claim | `generations` state `claiming`; the claim batch is atomic (A8), so the server holds all of it or none | the generation stays `claiming` and its claim batch is sent again (K19.7). Decided 2026-10-27: the manifest `g<generation id>` decides, never an anchor's `struct_gen` or origin (two devices that change one notetype reach the same `struct_gen`, K19.2, and the origin belongs to the entity, not the device). When the manifest's save, a new record, meets server record changed with a server manifest of this generation id, the earlier claim landed with its result lost: in state `claimed` the generation becomes `claimed` and takes that manifest's system fields as the acked claimed manifest (K10.3 `generations`), and each anchor's result follows K12.3 (its in-flight digest row when the server version is this claim's); in state `abandoned` it is lost (V18, K19.6). A fetch that delivers that manifest while the generation is `claiming` has the same outcome. Only when the batch shows no such manifest (the manifest's own result is batch request failed) do the anchors' results decide by K19.4, its first row included |
| generation adoption | the apply intent of its one `CloudIncoming` op | K19.5, K13.2 |
| prefetch for a decision | the `prefetch` recovery op | V14.3 |
| publication | the `publish` recovery op | V15.3 |
| adoption | the `adopt` recovery op and the R6.2 replacement intent | V14.3 |
| abandoning a generation | the `abandon_generation` recovery op | V18 |

## V8. The tracking file

### V8.1 Damage, loss and a newer schema

| Condition | Outcome | User sees |
|---|---|---|
| `meguri-cloud.db` missing | a new tracking file is made; no usable binding exists. The local collection and its fence stay as the sidecar holds them. Nothing is sent | when iCloud is enabled on this device: needs attention, choose the collection (V13) |
| it fails to open, fails `PRAGMA integrity_check`, or a required table cannot be read | the file and its `-wal` are moved aside as `meguri-cloud.db.damaged-YYYY-MM-DD-HH.MM.SS.salvaging` (local time, `package/backups#B3`; decided 2026-10-27, below) and never read by sync again, except by the salvage below. A new tracking file is made, the salvage runs, and then as "missing" | as "missing"; salvaged conflict copies stay listed (V17.2) |
| its schema is newer than this build | it is not opened for writing and not moved. Nothing is sent or applied | needs attention: unsupported schema, update Meguri (R1561) |

The salvage (the core, V2.4 step 2):

- The core opens the moved-aside file read-only and copies every `conflicts` row it can read, resolved or not, into the new file. They go under one salvage binding: state `isolated`, reason `tracking_damaged`, with the old binding's environment, container, account and epoch when they can be read. The salvage binding never resumes and nothing runs against it.
- When every `conflicts` row was read, the core deletes the moved-aside file (with its `-wal`) after the salvage commits. Otherwise, after the salvage commits, a rename drops `.salvaging` from both names, and that file is the kept one: the core never deletes it, Needs attention lists "some conflict copies could not be read", and Copy Debug Info names it.
- **A salvage that did not finish** (decided 2026-10-27, data safety; `conformance/cloudkit-faults#F8`). The `.salvaging` suffix marks a moved-aside file whose salvage has not ended with that delete or rename; a file without it is a kept file and is never salvaged again. A start that finds a `.salvaging` file in the tracking directory runs the salvage from it before anything else at V2.4 step 2 (when `meguri-cloud.db` is missing, the new tracking file is made first), into the current tracking file, oldest name first when there are several, and then the delete or rename above. That salvage skips every row whose copy id is already in the current tracking file. Decided 2026-10-27: every salvage, the first one included, makes no salvage binding when it copies no row, so a kill does not change whether one is made. So a kill after the move aside, or after the salvage committed, neither hides nor doubles a conflict copy.
- **Order of the delete and the rename** (decided 2026-10-27, data safety). With `<name>` = `meguri-cloud.db.damaged-YYYY-MM-DD-HH.MM.SS`, the moved-aside main file `<name>.salvaging` and its `<name>.salvaging-wal` are two files, so the core orders the steps so that a start can always tell how far they got. Both begin only after the salvage commits. The delete removes the main file first and the `-wal` last: a start that finds `<name>.salvaging-wal` with no `<name>.salvaging` beside it deletes it, since no salvage can read a WAL without its main file and every row was copied. The rename renames the `-wal` first and the main file last, so the main file's suffix stays the marker until both are done: a start that finds `<name>.salvaging` beside `<name>-wal` renames the main file and does not salvage again (the salvage had committed). So a rerun salvage always reads the main file with the WAL it was moved aside with, and never reaches another decision than the first.
- Nothing else is salvaged: no acknowledgement, cursor or pending row (K10.1, K21 invariant 12).
- Tracking never infers acknowledgements to rebuild a lost file. A device without tracking that connects to its old epoch goes through V13 and never merges into it. Its unsent changes stay in its local collection only, and V13's preview counts them as local content.

### V8.2 Collection file identity

- The identity is the file system's device id, the inode number and the creation time (`st_birthtime`) of `collection.anki2`. `scan_state` holds it next to `col.mod` and `col.scm` (K11.4).
- Tracking records it at each drained close and suspend (K11.4), in the commit that creates a binding for the open file (V15.2 step 2), in the commit of a resume (V9.2), and in the commit that completes an adoption (V14.2 step 9). Every Meguri path that puts a new file at `collection.anki2` while a binding stays or becomes usable records the new identity before its op row or intent is removed. Today that is V14 only: the other replacements isolate the binding (V12.1).
- At start (V2.4 step 5), for the active binding whose publication identity equals the open collection's: when no identity is recorded, the current one is recorded and nothing is checked. When the recorded identity differs, the file was replaced outside Meguri: for example a device restored from an Apple backup, or a container copied by hand. Tracking may then describe another state of the collection, and a reconcile scan would send stale rows as edits.
- So the binding becomes isolated with reason `file_mismatch`, and V13's choices apply. Resume (V9.2) is not offered.
- A changed `col.mod` or `col.scm` under the same file identity is an external change (`storage/durability-and-files#F3`): the reconcile scan runs and the binding stays usable.

## V9. Account and environment

### V9.1 Events

The actor reads the account status and the account's user record id, an opaque identity, at launch, at each foreground return and on every account-change event of the engine. A binding's account identity is that id (K10.2, A12). Reading the id needs the network (A19).

- **More read points** (decided 2026-10-27). With no engine instance there are no engine account events, so the actor also reads both at each `CKAccountChanged` notification, which U114 observes while the app runs, and, while the status is not `available` or the identity is not yet verified in this process, once at each `Network{reachable: true}` that follows `false`. Neither is a timer, and neither polls: each is one read per notification or per transition. So an offline launch, or a `temporarilyUnavailable` status that clears, starts the instance while the app stays in front.

| Event | Outcome | Isolation | Status (V11) |
|---|---|---|---|
| status `available`, identity read and equal to the binding's | normal | none | `sync/policy#P14` |
| status `available`, identity not yet read in this process (offline at launch, or the read failed) | no instance runs and V2.4 steps 8 to 10 wait until a read succeeds | none | offline when not reachable; else iCloud unavailable, temporary |
| status `available`, a read fails later in the same process | the running instance continues; the rows below apply on an engine account-change event or a later read that differs | none | unchanged |
| status `temporarilyUnavailable` or `couldNotDetermine` | no instance runs | none | iCloud unavailable, class as `sync/policy#P15` gives |
| status `noAccount` (signed out) | the instance stops | `signed_out` | iCloud unavailable: no iCloud account |
| status `restricted` | no instance runs | none | iCloud unavailable: restricted |
| status `configuration` (not an Apple status: U114 derives it when the account status read itself, an engine operation or a record-ID read fails with `missingEntitlement`, `badContainer`, `permissionFailure`, `managedAccountRestricted` or `badDatabase`, `sync/policy#P3`) | no instance runs, until a later account status read (at one of the read points above) returns another status | none | iCloud unavailable: setup needed (V10.3) |
| status `available`, another identity (an account switch, or a sign-in to another account) | the instance stops; nothing is ever sent to the new account from this binding | `account_changed` | needs attention: account changed; V13 for the new account |
| the engine reports a sign-in whose identity equals the binding's (an instance created with no saved state may report one at once: V9.2, V14.2 step 9, V3.1 rule 5) | normal: not an account change. The instance keeps running, the token is unchanged and the event is logged (`sync/policy#P3` `Account`, `#P4`; A12). Decided 2026-10-27 (review round 3): the engine may have reset its state at that event, its change tokens and pending list included (A12), so the event is a no-state point (`sync/policy#P3`): the full update it causes adds the pending names back, the next `fetchChanges` call gets the no-state zone check (V26.1, `sync/cloudkit-records#K12.5`), and no absence pass starts (K17.4) | none | unchanged |
| the engine reports a sign-out or an account switch during an operation | as the rows above; late facts are dropped (V3.4) | as above | as above |
| the build's container or environment differs from the binding's | the binding is not usable in this build (K10.2). This build runs nothing against it and never writes its state, isolation reason, cursor or rows; the build of its own container and environment uses it unchanged | none stored; this build reads the binding as `environment_changed` (V2.2) | local only, not connected (V11), unless this build has a binding of its own |

- The other rows apply only to bindings of the running build's container and environment. Decided 2026-10-27: a build never isolates a binding it cannot use, so a debug build that shares the app's data directory with the daily-driver build cannot leave that build's binding stuck and its pending changes. At most one binding is active per container and environment (V25).
- Local study, editing, backups and file export continue in every row (R2046).
- An account event never deletes local data, tracking or conflict copies (R2055).
- A recovery op that runs during an account event stops at its next step; V14.3 and V15.3 say what remains.

### V9.2 Resume

A binding isolated as `signed_out`, `account_changed` or `stopped` can be resumed through an explicit Resume action, offered only when all hold:

- the account status is `available` and its identity, read in this process, equals the binding's;
- the open collection's publication identity equals the binding's, and the fence is clear;
- `MGActive` names the binding's epoch, and its collection zone exists;
- V8.2 found no file mismatch since the binding was isolated;
- the binding is not the binding before of an `adopt` or `publish` row (V2.3): that op's Cancel returns it to its recorded state (V14.3, V15.3), and a resume meanwhile would make two bindings of one container and environment active (decided 2026-10-27).

The new binding of a waiting `publish` op is resumed by V15.3, not here. Resume, in one tracking commit, makes the binding active, deletes its `engine_state` row, records the open collection's file identity (V8.2) and clears its `scan_state` drained flag (V2.2, "Restoring a binding"). The engine's state is reset by an account change (A12), so no cursor is kept. Then the reconcile scan runs (K11.4), so edits made while the binding was isolated are found, and only then does the instance start without a cursor. Its fetch from the start reconciles the epoch with tracking (V3.1 rule 5); pending local changes then send normally. When a condition fails, Resume is not offered and V13 applies.

## V10. Service conditions

### V10.1 Zones

| Condition | Outcome | Status | Choices |
|---|---|---|---|
| the bound collection zone is not found (K12.3, K12.5) | sending stops; isolate `zone_missing`; this binding never creates the zone again (K2) | needs attention: the iCloud collection is missing | V15; V16 local only; V14 when `MGActive` names a complete epoch other than the bound one |
| the zone was deleted by the user (system settings, iCloud storage) | isolate `zone_deleted_by_user` | needs attention: Meguri's iCloud data was deleted; the local collection and its unsent changes are intact | as above |
| `meguri-media` missing or deleted while the collection zone exists | collection sync continues. Media sends and downloads stop for this binding for good: it never recreates the zone (K2), and never resumes media on a zone another device recreated | needs attention, media | V15, whose publisher creates the zone and a new `MGMediaRoot` (V25) |
| encrypted data reset (the account's encrypted values can no longer be read) | CloudKit deletes every zone that holds encrypted values, and every Meguri zone does (K3): the collection zones, `meguri-root` and `meguri-media`. The engine reports each zone deletion with the reason encrypted data reset (A18), and a zone-not-found result carries the same reason. Isolate `encrypted_data_reset`. Rows already applied stay; nothing new is read | needs attention: the iCloud copy can no longer be read | as the first row. The V15 publisher creates `meguri-root`, `meguri-media` and a new `MGMediaRoot` with a new media key (V15.2 step 3). No previous epoch is left to keep (V22 invariant 6). When a Meguri zone survives the reset but cannot be read (A18 false), the publisher deletes a surviving collection zone after its `MGActive` switch, without the 30-day retention, through `DeleteZones` (V26.1), and deletes and recreates a surviving `meguri-media`, because its key cannot be read. Decided 2026-10-27 (review round 4): Meguri decrypts nothing itself, so "cannot be read" is only CloudKit's own sign (K2): a record returned with its encrypted field absent (`e` of `root`), or a read that fails for that record with CloudKit's error that an encrypted value cannot be decrypted; the signed runs record which of the two CloudKit gives (A18) |

- Before it isolates for any row here, the actor reads the account status and identity again (V2.2). A collection zone exists only in its publisher's account (K2), so a zone that is missing after an account switch is an account change, not a lost zone.
- A fetched zone deletion and a send error reach the same row, by zone and cause, as K12.5 maps them. With `zone_deleted_by_user` and `encrypted_data_reset`, the binding's copies of server data are deleted with the isolation (V1, K12.5); its pending rows, intents and conflict copies stay.
- V14 is offered from a fresh read of `MGActive` (V13.2 step 2), whatever the stored isolation reason: a binding keeps its first service reason (V2.2), but another device's new epoch can be adopted from any of these rows.
- The isolated binding never creates a zone (K2). A V15 `publish` op creates `meguri-root` and `meguri-media` under its new binding, before its `MGActive` switch; it always asks for both, which is not an error when they exist, so a missing one is created without a read that finds it missing (V15.2 step 3, V25). Decided 2026-10-27: this is how a publication completes after a reset or after a deletion in system settings (A15), which leave no root zone to save `MGActive` into.
- Every device bound to a lost zone reaches the same state. When one of them publishes again, the others see a new epoch (V10.2) and are offered adoption.
- Every new binding (V14.2 step 9, V15.2 step 3) reads `MGMediaRoot` and holds it acked before any blob work (K9.2). Its `blobs` rows start with server state `unknown`. A binding never uses a media key that another binding read.
- When A10 is false, every zone loss is handled as `zone_deleted_by_user`.

### V10.2 The root zone and `MGActive`

| Condition | Outcome |
|---|---|
| `meguri-root` or `MGActive` missing, for a bound device (K12.5: a fetched deletion or a send error; a purge or an encrypted data reset maps to V10.1's reasons instead) | isolate `active_missing`, except the binding a `publish` op created while the op's step is before `done`: it saves `MGActive` itself (V15.2 step 4, V26.1). This binding never creates them (K2, K18.2); a V15 `publish` op does, under its new binding (V10.1). Choices as in V10.1 |
| `MGActive` names another epoch | isolate `epoch_switched` (K18.4), except the binding a `publish` op created, while that op's step is before `done` (K2, K18.4). Choices: V14 (use the new iCloud collection), V15, V16 local only |
| `MGActive` cannot be read, or its epoch has no complete `MGEpoch` | nothing is adopted or published automatically; needs attention: the attention item `rootUnreadable` (`sync/policy#P14.4`; decided 2026-10-27). V15 may replace it after confirmation; its switch is conditional on the change tag of the version that could not be read |

- **Sending after a confirmed root** (decided 2026-10-27). An engine instance sends to its collection zone only after the bound epoch is confirmed for that instance, in one of three ways (K2 "Confirmed root", K21 invariant 20): a fetch of `meguri-root` by that instance has drained and found `MGActive` naming the bound epoch; a `root_read` result accepted for that instance has recorded an `MGActive` naming it (V26.1, "The bound binding's `MGActive` read"); or the publisher's accepted `MGActive` save, in its `done` commit during that instance, confirmed it (V26.1, the "saved" row). Until then the core builds no collection batch for it (K12.1, K12.4), whatever the engine schedules. The binding a `publish` op created is exempt while the op's step is before `done` (K2). So a device that comes back online learns of an epoch switch before it sends, unless the switch happens between its root fetch and a send. An incremental root fetch delivers no `MGActive` while it is unchanged, so the drain confirms by the binding's last applied `MGActive` (K2 "Confirmed root", K10.3 `bindings`), also when it fetched nothing (`sync/policy#P3` `FetchDrained{root, fetched: false}`); a binding that records none reads `MGActive` by record ID first (`sync/policy#P10.1`). An instance created with no saved state gets no `fetched: false` input for a zone its first fetch did not list, so it never confirms an epoch from a root zone that no longer exists. Decided 2026-10-27: such a zone is only inferred to be gone. Before anything isolates on that inference, U114 reads its record by record ID (`active` for `meguri-root`, `epoch` for the bound collection zone; V26.1, "The bound binding's `MGActive` read"). Only a zone-not-found result reports the zone not found, and K12.5's plain `zoneNotFound` row then applies (`sync/cloudkit-records#K12.5`, "A zone missing from a first fetch"). Decided 2026-10-27 (review round 4, as `sync/policy#P3`): unknown item on `epoch` is a plain `zoneNotFound` too (the zone exists and holds no `MGEpoch`); unknown item on `active` is K12.5's `meguri-root` row (`active_missing`). A read of `epoch` that finds `MGEpoch` isolates nothing and is logged; a read of `active` that finds `MGActive` is recorded and decides as that bullet says. Either found record proves the zone exists, so the call that did not fetch it then gets `FetchDrained{zone, fetched: false}`: it completes no absence pass, and for `meguri-root` it confirms from the `MGActive` just recorded. A read that fails otherwise isolates nothing, gives no drain, and the read is made again at the instance's next `fetchChanges` call that does not fetch the zone (V26.1). The binding of a `publish` op before `done` gets no such read (V26.1).
- **Confirmation mark.** Every confirmation of the bound epoch records with the binding a value of its acknowledgement sequence: a per-binding counter that each acknowledgement takes, from a save result or from an applied fetched version (V25). When `MGActive` is later found naming another epoch, the binding's entities acknowledged after the mark are **sent after the replacement**: they may be only in the replaced epoch's zone. V13.4, V14.2 step 1 and V15.2 step 1 show their count.
- **Which value the mark records** (decided 2026-10-27, review round 4). A confirmation rests on a server read of the root state, and the instance's sends keep being acked while that read and the rest of its call run: a `RequestFetch{root, collection}` call reads `meguri-root` first and may then fetch the collection zone for minutes, and its `FetchDrained{root}` comes only after the call returns (`sync/policy#P3`, `#P4`). An ack in that time may come after a switch that the read did not see. So the mark is never the counter's value at the drain: it is a value taken before the server read could start, so that every ack after the read is above it:
  - U114 makes at most one `fetchChanges` call of an instance at a time, and none while a record-ID read of that instance runs (V26.1); a call asked for meanwhile starts when the one before has returned. The first will-fetch-changes event that U114 handles after a call began is passed to the core as a fact with the instance id (V3.4). The core then takes the counter's value and keeps it in memory with the instance as the value of that call, until the next call's first event replaces it;
  - a drained root fetch records the value of its own call (A7, `sync/policy#P3`);
  - a `root_read` result records the value of the instance's last call, which returned before the read began;
  - the publisher's `done` commit records the value when the core reported due the attempt of the `MGActive` save whose result that commit accepts (V26.1, "Attempts").

  The count may include changes acknowledged just before the switch; it never misses one acknowledged after the switch, also when the switch comes during a long fetch call.

### V10.3 Transient and storage conditions

| Condition | Outcome | Status (R1555, R1561) |
|---|---|---|
| network unavailable | the engine waits; work stays pending | offline |
| rate limited, zone busy, service unavailable, with or without a retry-after time | the engine retries on Apple's schedule (R1553); Meguri adds no timer. Sync Now during the wait reports waiting with the retry time and does not bypass it | quiet: changes pending, with the retry time in the popover |
| change token expired, when the engine surfaces it | the engine fetches from the start; tracking keeps its acknowledgements. Meguri cannot tell that fetch from an incremental one (A9b), so it is not a fetch from no cursor and runs no K17.4 absence check | syncing collection, when the engine reports the fetch |
| quota exceeded | records stay pending in tracking and fetching continues. The engine does not retry these saves: it drops each name whose save failed with quota exceeded (A13 is false, `sync/cloudkit-records#K12.4`). The next full update adds the names back at an event trigger: a local change, a foreground return, an account change, Sync Now, or a state change the engine reports (`sync/policy#P2`). Meguri adds no pause and no timer (U113). Study, editing, backups and file export continue (R2059) | needs attention: iCloud storage is full, until a later save succeeds |
| per-record failures, partial batch results | K12.3 per record. Completion is not reported while any selected record is unaccounted for (R2049, `sync/policy#P12.4`) | changes pending, or needs attention |
| not authenticated, or account temporarily unavailable, from an engine send or fetch while the account status read stays `available` with the binding's identity | the instance keeps running and the engine retries the work itself; nothing is isolated. The actor reads the account status and identity again; V9.1 applies only when the read differs | iCloud unavailable, temporary, class `noAccount`, until a send or fetch succeeds (`sync/policy#P15`, `#P14.2` row 2) |
| account status `configuration`; permission failure, missing entitlement or container | no instance runs; nothing is isolated; Meguri performs no setup (R2065) | iCloud unavailable: setup needed, class `noAccount` (`sync/policy#P15`, `configuration`) |
| a record format newer than this build (K5.4) | the record waits in the inbox | needs attention: unsupported schema, update Meguri |
| a full disk during an inbox or tracking commit | V3.3 | needs attention: local storage full |

**Assumption A13** (false for quota exceeded, by the SDK headers, 2026-10-27; `sync/cloudkit-records#K12.4`, `#K23`): the sync engine retries throttled, busy and quota-exceeded work by itself with no app timer, and reports quota exceeded per record or per batch, so status can show it. The engine keeps for retry only the failed saves K12.4 lists: the network, throttling and busy codes and the two transient authentication codes. It drops a save that failed with quota exceeded. K12.4's rule replaces that part: the names are added back at the next event trigger, with no timer and no pause. Quota exceeded is reported per record (K12.3).

- Decided 2026-10-27: an engine result of not authenticated or account temporarily unavailable stops nothing, because the engine retries both itself (K12.4). Only an account status read (V9.1) or a setup failure (the `configuration` row above) stops the instance. A stop on a transient code would leave the engine stopped while the account status stays `available`, because no account change would follow to start it again (`sync/policy#P15`).
- Decided 2026-10-27: no public event of the sync engine says that it reset an expired change token and started again from the beginning. Only an engine instance that Meguri itself creates with no saved cursor counts as a fetch from no cursor (V3.1 rule 5, V9.2, V14.2 step 9). Tombstone purging (K17.4) stays off until a signed device run shows that the engine's own restart is detectable (A9b, K23), so a device offline for longer than the retention never keeps a purged deletion live.

## V11. What the user sees

`sync/policy` computes the status from durable tracking (R1559), in the order of `sync/policy#P14.2`. This section fixes how each recovery condition maps to that order, and the reason and action it adds. The copy is `sync/status-strings` T9 to T13; quoted text here gives the meaning, not the copy.

- A pending fence, or a replacement intent that is unfinished or unproven, gives `paused{decisionRequired}` (`sync/policy#P17`). While it is pending, the `replaced_locally` isolation it caused adds no attention item. Decided 2026-10-27 (review round 4, `sync/policy#P17`, `engine/collection-replace#R6.6`): a `CloudAdopt` intent that R6.3 cannot prove is not a publication decision and never shows V12.2's sheet. While its `adopt` op row waits (V14.3, last row) it gives `needsAttention{recoveryOpWaiting}` with Cancel only; after that Cancel it gives `needsAttention{chooseCollection}`, whose action is V13, until a V13 choice for the collection that opens resolves it. Sync Now gives `recoveryRequired` in both.
- A binding isolated as `signed_out` gives `iCloudUnavailable{noAccount}` while the account status is `noAccount`. Once an account is available it gives `needsAttention{isolated}`: with Resume when V9.2 allows it (the same identity), or V13 for another identity (`sync/policy#P14.2` row 2, `#P14.4` `isolated`).
- `localOnly` applies only when the sidecar decision is local only, the binding is `stopped`, every binding is of another container or environment (`environment_changed`, V9.1; decided 2026-10-27: local only and not connected), or there is no binding and iCloud is not enabled on this device.
- **No binding** means no binding in any container or environment other than a V8.1 salvage binding (`tracking_damaged`). A salvage binding adds no isolation item: it only holds conflict copies (V17.2), which keep their own `conflicts` item.
- Every other isolation reason, and no binding while iCloud is enabled on this device (V8.1), gives `needsAttention`. No binding with iCloud enabled is the choose-the-collection case (V13), not an isolation.

| Status (R1558) | Reason | Error class (R1561) | Action |
|---|---|---|---|
| iCloud unavailable | no account, restricted, could not determine, temporary, identity not yet read; temporary also while an engine send or fetch last failed with not authenticated or account temporarily unavailable and none has succeeded since (V10.3) | as `sync/policy#P15` gives | system account guidance (R102) |
| iCloud unavailable | setup needed: account status `configuration`, permission, entitlement or container failures (V10.3) | `noAccount` (`sync/policy#P15`, `configuration`) | none; Meguri performs no setup (R2065) |
| local only | the user chose local only (V12, V13, V16), stopped, only bindings of another container or environment, no binding with iCloud not enabled | none | Connect to iCloud (V13); Resume for a stopped binding when V9.2 allows it |
| paused | the user paused sync (V16) | none | Resume |
| paused | the fence is pending (V12) | conflict/recovery needed | "Choose where to keep the restored collection" (or imported; "this collection" when no reason is recorded, `engine/collection-replace#R6.7`) |
| needs attention | account changed | account changed | Resume when V9.2 allows it; V13 |
| needs attention | signed out, and an account is available again | as `sync/policy#P15` gives | Resume when V9.2 allows it (the same identity); V13 |
| needs attention | epoch switched | conflict/recovery needed | V14, V15, V16 |
| needs attention | zone missing or deleted, encrypted data reset, root or `MGActive` missing | conflict/recovery needed | V15, V16; V14 when `MGActive` names a complete epoch other than the bound one (V10.1) |
| needs attention | `MGActive` cannot be read, or its epoch has no complete `MGEpoch` (`rootUnreadable`, V10.2 row 3; decided 2026-10-27) | conflict/recovery needed | V15 after confirmation, its switch conditional on the change tag of the version that could not be read; V16 |
| needs attention | media zone missing (V10.1) | conflict/recovery needed | V15 |
| needs attention | media missing: live media names whose file is not on this device and whose blob the media zone does not hold (K9.3, K9.4, K9.5; `sync/policy#P14.4` `mediaMissing`) | none: no sync operation failed | a details list of the names; Copy Debug Info |
| needs attention | tracking file damaged or missing while iCloud is enabled, file mismatch | conflict/recovery needed | V13 |
| needs attention | unresolved conflict copies (V17) | conflict/recovery needed | Review Conflicts (V17) |
| needs attention | some conflict copies could not be read (V8.1) | conflict/recovery needed | Copy Debug Info |
| needs attention | refused records; entries still waiting for a reference after a drained fetch; oversize, unreadable or unsyncable entities (K6.1, K7, K9.4) | conflict/recovery needed | a details list with kind and key; Copy Debug Info |
| needs attention | incoming changes this device could not apply after five attempts (`applyFailed`), a failed re-key (`rekeyFailed`) (K13.2, K14.2) | conflict/recovery needed | a details list with kind, key and error; Copy Debug Info. The entries are tried again at the next event trigger and once per collection open |
| needs attention | the open collection's `col.crt` differs from the binding's epoch `crt` (`crtMismatch`, `sync/policy#P6`, K20) | conflict/recovery needed | V14 or V15; nothing is sent or applied meanwhile, and nothing is isolated |
| needs attention | an incomplete generation (V18) | conflict/recovery needed | Abandon incomplete change when V18 offers it; otherwise the details list |
| needs attention | a recovery op waiting (V14.2 step 7, V14.3, V15.2 step 5, V15.3, V26.1) | conflict/recovery needed | Resume or Cancel, as V14.3, V15.3 and V26.2 `cancel_recovery_op` allow: for an adoption, Cancel until step 7's exclusive phase starts, also while the op waits for a review session to end, and Cancel only (no Resume) when R6.3 cannot prove either file; for a `publish`, Cancel at every step before `done` and hidden only while an attempt of its `MGActive` save runs; for a `publish` whose switch conflict is set (V2.3), V15.2 step 5's three choices in place of Resume |
| needs attention | quota full | quota full | Try Again; storage guidance |
| needs attention | local storage full, tracking error | local storage full | Try Again |
| needs attention | newer format, newer tracking schema | unsupported schema | update guidance |
| offline | network | offline | none |

- No reason shows a modal that blocks study (R1562). Failures of background work change only the indicator and the popover (R166).
- A decision is never answered by Sync Now, the automatic toggle, a relaunch or an Anki transfer. Sync Now with a pending decision starts no exchange (`sync/policy#P12.1` row 6) and shows the decision's action.

## V12. The publication fence (R1548)

### V12.1 Setting it

- A restore, a `.colpkg` import and a manual Anki import set the fence durably before their swap (`engine/collection-replace#R6.2`).
- At the open after the swap (V2.4 step 4, or the in-process reopen), before any scan or instance, tracking sees that the open collection's publication identity differs from the active binding's and isolates that binding with `replaced_locally`.
- While the fence is pending nothing is sent for any binding (K10.2). Nothing is fetched for the old binding either: its collection is gone. Local study is not blocked.
- The fence survives relaunch, account changes, Sync Now and manual Anki transfers (R2010, `engine/collection-replace#R6.3`).
- The replacement's reason (restore, `.colpkg` import, manual Anki import) is kept with the pending decision in `deviceState` (`pendingDecisionReason`, `engine/collection-replace#R6.7`) until the decision is resolved. It gives `MGEpoch.reason` in V12.2. A pending decision with no recorded reason (one recorded before the member existed, or one that R6.6's account check left) follows R6.7: it publishes as an unfenced publication would, and the status and sheet name it "this collection".

### V12.2 The decision

The status is paused with "Choose where to keep the restored collection" (or the imported one; with no recorded reason, "this collection", `engine/collection-replace#R6.7`). The choice sheet shows the direction of each choice, the restored or imported collection's counts (V13.4), the iCloud collection's counts when the account has an active epoch, the old binding's unsent changes with a note that they exist only in the safety backup taken before the swap, and the unresolved conflict copies, which stay.

| Choice | Effect |
|---|---|
| Replace iCloud collection | V15 with this collection; `MGEpoch.reason` from `pendingDecisionReason`: 3 (restore), 4 (collection package import) or 5 (manual Anki import). With no recorded reason, 2 (replace cloud) when `MGActive` names an epoch, else 1 (initial) (`engine/collection-replace#R6.7`). The fence clears in V15.2 step 2 |
| Keep this collection on this device only | the sidecar decision becomes local only (unbound, `engine/collection-replace#R6.1`) and the fence clears. The old binding stays isolated. Status: local only |
| Not now | nothing changes; the fence stays |

- Without an available account, only the last two are offered.
- When the device has no binding and iCloud is not enabled, the fence stays, and the first connection (V13) is the decision. Each V13 choice except Cancel resolves it; Cancel keeps the fence.
- Using the iCloud collection instead, which discards the restored one, is V14, offered in Account & Sync as an advanced action, not in this sheet. It resolves the fence too (V14.2 step 8).

## V13. Connecting

### V13.1 When

The connect action (Welcome's Connect to iCloud, Account & Sync's Enable, or a V11 action) runs this flow when the device has no usable binding. It needs account status `available` with an identity read in this process, and it starts only outside a review session (V20).

### V13.2 What the device reads first

1. The account identity (V9.1).
2. `meguri-root` and `MGActive`, read only, by record ID (V26.1). Nothing is created yet.
3. When `MGActive` names an epoch: its `MGEpoch`, which must be `complete` (else `EpochIncomplete`). Then, when a local collection exists, a **prefetch**: tracking records a `recovery_ops` row `prefetch` (step `prefetch`, the epoch), and the staging fetcher (V3.4) fills a staged collection, records only, as V14.2 steps 4 and 5 say. So the decision shows exact counts, and "Use the iCloud collection" needs no second fetch.

- The prefetch does not take the long-operation slot and does not count for `Busy` (V2.3).
- The user can cancel the fetch. The decision then shows `MGEpoch.counts` labelled with `published_at_ms` (V13.4), and V14 fetches when chosen.
- Choosing "Use the iCloud collection" turns the row into V14's `adopt` row (V14.2 step 3).
- Any other choice, Cancel, closing the decision, a collection replacement (`engine/collection-replace`), an account change and an `MGActive` change each delete the staging and the `prefetch` row first, before any row of their own.
- A relaunch during a prefetch: V14.3.

### V13.3 Cases

| Local | `MGActive` | Flow |
|---|---|---|
| no collection | none | the user starts empty or imports a file (U166). A collection started empty is published at once: V15 from step 2, with no preview, reason 1. An imported one sets the fence, and V12 decides |
| no collection | an epoch | adopt it (V14) without a confirmation step: nothing local is replaced. Progress shows the collection, then media (R158, R1592) |
| a collection | none | confirm "Upload this collection to iCloud" with its counts, then V15 from step 2, with no preview of a replaced side (reason 1, or 3 to 5 when the fence is pending with a recorded reason; a pending fence with no recorded reason gives 1, since `MGActive` names no epoch, `engine/collection-replace#R6.7`) |
| an empty collection | an epoch | confirm "Use the iCloud collection on this device" with both sides' counts, then V14, safety backup included |
| a non-empty collection | an epoch | the two-collections decision (V13.5) |
| a collection whose isolated binding meets V9.2 | that binding's epoch | Resume (V9.2), confirmed as "Continue syncing this collection", with this device's unsent change count |

A first publication is a `publish` op like any other, so a crash follows V15.3, and another device that publishes first leads to V15.2 step 5.

### V13.4 Counts

- A side's counts: notes, cards, review entries, decks, note types, media files, the newest review instant and the last change instant.
- Local counts come from the collection. iCloud counts come from the staged collection (V13.2), from tracking's acked live entities when the device is bound to that epoch, or else from `MGEpoch.counts` with their date. With `MGEpoch.counts`, the newest review instant and the last change instant show as unknown.
- Unsent work: this device's pending entities by kind, held-back answers, and each isolated binding's pending count. Other devices' unsent changes are unknown, and the preview says so.
- Sent after the replacement: for a binding whose epoch `MGActive` no longer names, its changes acknowledged after its confirmation mark (V10.2), by kind, shown next to the unsent work. The preview says that they may be only in the replaced iCloud collection: after V14 they exist on this device only in the safety backup, and in iCloud only in the previous epoch's zone for its 30 days (V14.1); V15 publishes them with the rest of the local collection. What other devices sent to the replaced collection is unknown, and the preview says so.
- Fetched, not applied (decided 2026-10-27, review round 4; V15.2 step 1): the inbox entries (K10.3) of the binding a `publish` op would replace, by reason (`ready`, `generation`, `reference`, `structure`, `format`, `asset`, `apply_failed`). They are other devices' changes, already acked in the replaced epoch, that this device fetched and has not applied: entries that wait for a structural generation, a reference, a newer format or an asset, failed applies, and ready entries not yet applied. The local collection does not hold them, so the new collection will not include them; that binding's inbox is deleted at step `done` (V15.2 step 4), and the changes stay in the previous epoch's zone for its 30 days (V14.1). A remote generation `waiting` with a `claimed` manifest is named with its note type. The sending devices may not count these as sent after the replacement (V10.2: acks before their last confirmation are not counted), so this device's preview is where they are reported.

### V13.5 The two-collections decision (R158, R2055)

Two non-empty collections are never merged. The decision offers:

| Choice | Effect |
|---|---|
| Use the iCloud collection on this device | V14: this device's collection is backed up, then replaced |
| Replace the iCloud collection with this device's | V15, reason 2: the iCloud collection stays as the previous epoch for 30 days, and other devices are asked (V10.2) |
| Keep this device's collection without iCloud | local only; nothing is sent |
| Cancel | nothing changes; not connected; a pending fence stays (V12.2) |

## V14. Replace local collection from iCloud (R125, R2056)

### V14.1 When

- From V13 (adoption at connection), from V10 and V11 (an epoch switch, a lost zone while another epoch is active, a `crt` mismatch, `sync/policy#P6`), from V12.2's advanced action, from the damaged-collection screen (V19), and as an advanced action in Account & Sync.
- From an isolated binding, it is offered whenever a fresh read of `MGActive` names a complete epoch other than the bound one, whatever the stored isolation reason (V2.2, V10.1).
- The source is the active epoch, or the previous epoch: the one `MGActive.previous_epoch_id` names, offered as "Restore the previous iCloud collection from <date>" only while its zone exists and its `MGEpoch` is `complete`. Older epochs are not listed.
- Adopting the previous epoch gives this device that epoch's content; it does not change `MGActive`. That epoch is not active, so for this source step 6 creates no binding, step 8 records the sidecar decision local only, and step 9 starts no instance. The collection is local only until the user chooses V15.
- Not during a review session (V20). The long-operation slot must be free (`engine/lifecycle#L5`); else `Busy`.

### V14.2 Steps

1. **Preview**: the direction ("this device's collection will be replaced"), both sides' counts (V13.4), unsent and isolated work, the changes sent after the replacement (V13.4), and the unresolved conflict copies, which stay. A damaged local collection's counts show as "cannot be read". For the active source, also the number of local media files the iCloud collection does not name: they stay on this device and are not uploaded (V14.4); and the number it names with other content: the iCloud files replace them, and the local ones go to the media trash (V14.4). Media is not in the safety backup (`package/backup-triggers#T6.1`).
2. **Confirm.** Skipped only when there is no local collection (V13.3).
3. **Record**: take the long-operation slot (held: `Busy`), then record a `recovery_ops` row `adopt`, step `staging`, with the binding before (its state and reason), the source and the fence state. The binding before is the newest binding of this account and of the running build's container and environment, active or isolated, if any (as in V15.2 step 2); a salvage binding (`tracking_damaged`, V8.1) is never the binding before, and a binding of another account is not touched (R2055). The slot is held until step 9 or until the op ends, so restore, `.colpkg` import, manual Anki import and Check Database get `Busy` meanwhile. From a prefetch (V13.2), its row becomes this row in the same commit: when the prefetch finished, it keeps the staging, the step becomes `staged` and step 4 is skipped; otherwise its staging is deleted and step 4 runs.
4. **Stage**: the staging fetcher (V3.4) fetches the source epoch's zone from no cursor into a new staged collection (K18.4). After each fetched batch is written, staging stops with `InsufficientSpace` when the volume's free space is below 2 × the staging directory's size + 64 MiB. Study and edits continue meanwhile; they change the collection that is about to be replaced, which the preview said. When the fetch drains, the step is `staged`.
5. **Validate**: the open gate, `PRAGMA integrity_check` and K13.3's reference checks. A failure ends the op with `StagingRefused` or `StagingCorrupt`, and the local collection is unchanged.
6. **Step `ready`**, in one tracking commit: for the active source, the new binding (state `active`, the epoch, the candidate publication identity, no cursor, every fetched entity acked as K18.4 says); for the previous source, none. The binding before (step 3), if any: an active one is isolated with `replaced_locally`; an isolated one keeps its reason, since no reason replaces another except as V2.2 says (decided 2026-10-27). Its prior state and reason are in the op row either way, so V14.3 returns it to exactly that state. The new binding is not usable yet: its publication identity is not the open collection's.
7. **Exclusive phase**: one `package/container#P10` preflight for the safety backup (B4's need for the live collection, or T7's for a damaged one). Then the exclusive phase (`engine/lifecycle#L7`). The session check runs again as the phase is entered, as `package/backup-triggers#T8` step 6 does: when a session is open, the phase is dropped and the op waits at step `ready` until `SessionEnded`, then enters again (V11: a recovery op waiting). When the open collection's publication identity changed since step 3, the op is cancelled as V14.3's first `ready` row says. Then the safety backup of the local collection (`package/backup-triggers#T6.1`; T7's byte copy when it is damaged; none without a collection file).
8. **Replace** with reason `CloudAdopt` (`engine/collection-replace#R1`, `#R6.6`): R3 to R5, with an R6.2 intent whose operation identity is the op id. `CloudAdopt` sets no fence, because the cloud side is the source: it writes `cloudPublicationPending` false, with a decision bound to the new binding's account and environment (active source) or local only (previous source). This also resolves a fence that was pending before the op (V12.2).
9. **Step `done`**, after the reopen, in one tracking commit: for the active source, the new binding becomes usable, its `scan_state` records the new file's identity (V8.2), and its media adoption window opens (V14.4). The old binding's pending rows, inbox and intents are deleted and its conflict copies kept (V17.6). The op row is removed, then the slot is released. For the active source, the new binding's first reconcile scan then classifies the local media files (V14.4), and only after it does an instance start with no cursor: its fetch from the start finds every record equal to the staged one (K13.2) and runs K17.4's absence check (V3.1 rule 5). Media follows in the background (R1592), after the new binding holds an acked `MGMediaRoot` (V10.1). A local file that the epoch names with other content is **superseded**: K9.3's install replaces it and trashes it (`media/media-files#M9`), and it is never sent as a local change. A local file the epoch does not name is **withheld**: it stays on this device and is never uploaded unless the user chooses it (V14.4).

### V14.3 Crash, failure and cancel

| Point | At restart |
|---|---|
| a `prefetch` row | the staging is deleted and the row removed. The decision starts a new prefetch when it is shown again |
| step `staging` or `staged` | the staging directory is deleted and the op row removed; the collection and the bindings are unchanged. When the op came from a decision, the decision is shown again with Try Again |
| step `ready`, and the open collection's publication identity is not the candidate's (no R6.2 intent was recorded, or R6.3 settled the old file as live) | in one tracking commit, the new binding is deleted, the old binding returns to its recorded state and reason, and the op row is removed. When that state is `active`, the same commit clears the old binding's `scan_state` drained flag, and its reconcile scan runs before any instance of it starts (V2.2, "Restoring a binding"), so answers and edits committed while it was isolated since step 6 (a review session that step 7 waited for included) are sent to its epoch |
| step `ready`, and the open collection's publication identity is the candidate's (R6.3 settled the candidate as live, also when it already removed the intent) | step 9 |
| R6.3 cannot prove either file (its last row) | neither binding is usable and the op row stays; needs attention: recovery needed. Nothing is sent or applied. Cancel deletes the op row and the new binding; the old binding stays isolated; then V13 applies to the collection that now opens, and its choices resolve R6.3's block (U113, `engine/collection-replace#R6.1`). The R6 journal is kept for Copy Debug Info |

- A failure without a kill after step `ready` and before the R5 rename (`SafetyBackupFailed`, `InsufficientSpace`, `LiveWalNotEmpty`, `StagingNotFinalised`, `Io`) is handled at once as the first `ready` row, its scan included, and the error is shown with Try Again. A cancel at step `ready` is handled the same way.
- An account change, a zone loss or an `MGActive` change during steps 3 to 6 cancels the op as the `staging` row says, or as the first `ready` row says once step 6 is durable. Once step 7's exclusive phase has started the swap completes, and V9 and V10 then check the new binding like any other.
- The user can cancel until step 7's exclusive phase starts, also while the op waits for a review session to end.

### V14.4 Local media after an adoption

Decided 2026-10-27. An adoption replaces `collection.anki2`, not the media folder. Every media file on the device when it completes belongs to the replaced collection, so none is a local change of the new binding: none may win over the epoch's content or be uploaded unless the user chooses it afterwards. This section covers the active source; the previous source creates no binding (V14.1).

- **Adoption window.** The new binding has no `media_feed` row, so it is a new consumer of the media journal's feed and its first reconcile scan runs before its first instance (K10.3, feed floor; V14.2 step 9). The window opens in step 9's commit. It closes in the first tracking commit, at or after that scan, that consumes the feed while `media/media-journal#J6` reports it complete for the binding's `feed_id`; when the feed is complete at the scan, the scan's commit closes it. Tracking stores the open window with the binding (V25), so a crash or a relaunch keeps it open.
- **Classification.** While the window is open, every name the scan or the feed reports is classified by what the adopted epoch holds for its case key (K4.2), never compared as a local change:

  | Epoch | Local file | Entity |
  |---|---|---|
  | live, same content | present | clean |
  | live, other content | present | `superseded` |
  | live | absent | `file_missing` (K9.4): a download candidate, or missing media when the media zone has no blob |
  | none, or a tombstone | present | `withheld`: a new entity, or the epoch's acked tombstone entity with the flag |
  | none, or a tombstone | absent | nothing |

  A name first registered while the window is open (the journal's registration was still running or was cancelled at the adoption) is classified when it reaches the feed, as a file of the replaced collection. A file the user adds while the window is open is classified the same way; the lifts below release it. A `superseded` or `withheld` entity records the content digest it was classified with.
- **Superseded.** Not pending and not sendable, whatever its local digest: K11.4's scan and K13.2's apply never take its local file as the local version, and K15.9 never compares it. Its acked content is a download candidate, as for an entity applied from the server (K9.3). The install runs as a media apply under its intent; the exact-name write moves the old file to the media trash (`media/media-files#M9`), and the transaction that marks the apply done clears the flag. When the media zone holds no blob for the acked content, the name is reported as missing media (K9.3), and the flag and the local file stay. When the name's local content later changes to content other than the classified digest and the acked content, the user changed the file: the flag clears and the name compares as any local change (K11.4).
- **Withheld.** Never sent; its blob is never uploaded (V22 invariant 11). It holds back no note: K12.1 rule 6 does not wait for a withheld name, since it is never acked. It is released, and becomes pending like a new entity (or a pending re-creation over the epoch's tombstone, K11.4), when either holds:
  1. the name's local content changes to content other than the classified digest;
  2. a local op (not `CloudIncoming` or `CloudRekey`) writes a live note whose fields reference the name (`text/media-references`) while the note's before-image did not (`engine/change-stream#S2.4`): the user chose the file. Inserting an existing file through the editor is such a write, although the media journal records nothing for it (`media/media-files#M3`). K12.1 rule 6 then holds that note until the name is acked.
- A withheld name that becomes absent with the cause `removed` loses the flag: a new entity leaves tracking, as an entity never sent does (K17.1), and a tombstone entity stays the epoch's acked tombstone. Added again later, the name is a new local change and is sent. With the cause `missing` it stays withheld, so a file of the replaced collection that comes back unchanged is still not uploaded.
- A superseded name that becomes absent, whatever the cause, loses the flag and is a missing local file (K9.4): the bytes removed were the replaced collection's, not the epoch's, so it makes no tombstone.
- **Incoming versions** (K9.4, "Incoming versions over these flags"). An incoming version S, from a fetch or a save conflict, never merges with a withheld or superseded file:
  - superseded, live S: the entity takes S and keeps its flag, and S's content becomes the download candidate; when the local file already holds S's content, the entity is clean and the flag clears;
  - superseded, tombstone S: the entity takes the tombstone and becomes `withheld`, keeping its classified digest. The epoch no longer names the name, and the file still belongs to the replaced collection, so it stays on this device and is not trashed;
  - withheld, live S: S wins without a merge. Its install runs as a media apply (K9.3); the exact-name write moves the local file to the media trash (`media/media-files#M9`), and a conflict copy `media_content` names that trash entry (K16), so the user can still recover the file (V17.4). The flag clears when the apply is done. There is no move to a hash name, no note rewrite and no new entity (K15.9 never compares it);
  - withheld, tombstone S: the entity takes the tombstone and stays withheld; its file stays.
- These states never make a media-name tombstone. A withheld file is never sent and never deleted by sync: it leaves the media folder only when the user removes it, or when an incoming live version for its name is installed, and then always into the media trash with a `media_content` copy.

## V15. Replace iCloud collection (R125, R2056)

### V15.1 When

From V12 (the fence), V13 (a first upload: V13.3 rows 1 and 3; two collections), V10 (a lost zone), V11 (an epoch switch, a `crt` mismatch, `sync/policy#P6`) and Account & Sync. Account status `available` is required. A review session may stay open, since the local collection does not change; its held-back answer is sent only under `sync/policy#P8`, so the publication completes after the session ends.

### V15.2 Steps

1. **Preview**: the direction ("the iCloud collection will be replaced on all your devices"), both sides' counts, the count of media names that K18.3 step 2 carries over from the binding before whose files are not on this device (`sync/cloudkit-records#K9.4`, "Carried names": live names flagged `file_missing` or `superseded` there, or absent from the media journal with cause `missing`; decided 2026-10-27, review round 3), with the note that the new collection names them from the old one: they are downloaded when the media zone holds them, and otherwise shown as missing media until a device that holds the files uploads them (`sync/cloudkit-records#K9.3`), that changes other devices have not sent, or send before they learn of the replacement, are not included and those devices will be asked (each shows what it sent after the replacement, V13.4), the binding before's changes fetched and not applied, by reason, with the note that the new collection will not include these changes from other devices (V13.4; decided 2026-10-27, review round 4: the publication neither waits for them nor applies them first, since a waiting generation may never complete; the user's confirmation covers them, and the previous epoch keeps them), that the previous iCloud collection stays recoverable for 30 days (V14.1), and the bytes the new zone needs, against the iCloud space available when CloudKit reports it.
2. **Confirm.** Then, in this order: record a `recovery_ops` row `publish` (a new epoch id, step `confirmed`, the confirmed `MGActive` version's change tag, encoded system fields and the epoch id it names, from step 1's preview root facts, or none, the fence state with the prior decision and its `pendingDecisionReason` (V2.3 fence before), and the binding before: the binding of this account and environment that the op replaces, if any, with its state and reason); in one tracking commit, create the new binding with the open collection's file identity (V8.2), and isolate the binding before with `republished` when it is active; an isolated one keeps its reason, since no reason replaces another except as V2.2 says (decided 2026-10-27, as V14.2 step 6). Its prior state and reason are in the op row either way, for V15.3; then clear the fence when it was pending (U113 resolves `engine/collection-replace#R6` for the new binding's account).
3. **Publish**: first, under the new binding, the op creates `c-<epoch id>`, `meguri-root` and `meguri-media` in one zone creation, whether or not they exist (creating a zone that exists is not an error, K2), since no read tells it that `meguri-media` is missing (an encrypted data reset, a deletion in system settings, a lost zone; V10.1, V25). After that creation succeeded, it saves `MGMediaRoot` `root` with a new media key as a new record, whatever it found (except after an encrypted data reset, below), and `MGEpoch` `publishing` as a new record. When `root` exists, V26.1's "server record changed, `root`" row takes the server's key (K9.2), so the binding holds an acked `root`, read through that conflict or created, in every case (decided 2026-10-27). Decided 2026-10-27: outside the encrypted-data-reset path, a server `root` whose key this build cannot read (a newer `format`, a decode error, an `e` that CloudKit fails to read (a decryption error)) deletes nothing: the op waits at its step as V26.1's "any other error" row says, since that `meguri-media` may hold the only copy of other devices' files. After an encrypted data reset (the binding before is isolated `encrypted_data_reset`, V10.1) the op reads `root` by record ID after the zone creation and before any `root` save (K2, V26.1): a `root` whose `e` this build reads and decodes gives the binding its media key (K9.2), and nothing is deleted or saved; unknown item makes the new-record `root` save due; only CloudKit's own sign that the reset left the zone unreadable (K2, decided 2026-10-27, review round 4: the read returns the `root` record with its encrypted field `e` absent, or fails for that record with CloudKit's error that an encrypted value cannot be decrypted; A18 false) makes the op delete `meguri-media`, create it again and save a new `root`; a present `e` that fails Meguri's decode, or that decodes to a newer `format`, is V26.1's "any other error" and deletes nothing. Meguri decrypts nothing itself: the media key is plain bytes inside `e` (K5.2). These writes are K18.3 step 1; then K18.3 steps 3 and 4 follow. K18.3 step 2's binding exists from V15.2 step 2: its reconcile scan runs after K18.3 step 1, and its instance starts then. The new binding reads or creates `MGMediaRoot` before any blob work (V10.1). The op row records each step reached.
4. **Switch**: K18.3 step 5. On success, step `done`: the op row is removed. When the binding before is in the same account and environment, its work is deleted with its conflict copies kept (V17.6). A binding of another account keeps its work, isolated (R2055).
5. **Conflict** at step 4 (another device switched `MGActive`, or created it first): nothing is overwritten. Needs attention: "iCloud was replaced from another device while this device was uploading". Choices: use that collection (V14); replace it again (a new step 1 preview with the new change tag, then `republish`, V26.2); keep local only (V16). Decided 2026-10-27: the commit that records the conflict sets the op row's switch conflict (V2.3); the op stays at step `complete` with its new binding, its acknowledgements and its zone, and none of its op operations is due, so the `MGActive` save is not tried again at each event trigger. The binding's instance keeps running, exempt before `done` (K2), so edits made meanwhile still reach the uploaded zone. "Use that collection" (`adopt`) and "keep local only" (`keep_local_only`) first cancel the waiting op as V15.3 says for an op whose switch sent is true (no zone is deleted), in the same call, so neither returns `Busy` for it; then they run. Decided 2026-10-27 (K18.5): "replace it again" reuses the uploaded zone only while its `MGEpoch.published_at_ms` is less than 20 days before now and a record-ID read of its `epoch` (V26.1, a preview read of step 1) returns it; otherwise it starts a new epoch, with a new epoch id and zone, and the old zone is left to K18.5. Decided 2026-10-27: the uploaded zone is not deleted when the user decides. An earlier attempt of the switch whose result was lost may have landed before the other device's switch, so other devices may have adopted the zone or keep it as their previous epoch; K18.5's 30-day rule removes it (V15.3, "switch sent").

   **Replace it again** (decided 2026-10-27), `republish(op_id, active_change_tag)` (V26.2), after a new step 1 preview whose preview reads (V26.1) returned the new `MGActive` and the op's own `epoch`:
   - **Reuse** (K18.5's 20-day rule holds and the read returned the op's `MGEpoch`): one tracking commit keeps the op row, its epoch id, its new binding and that binding's acknowledgements, sets the row's active change tag, active system fields and active epoch id to the new `MGActive`'s from the last root facts (decided 2026-10-27, review round 4: so the saved `MGActive`'s `previous_epoch_id` names the epoch this switch retires, K18.3 step 5), and clears the switch conflict. The step stays `complete` and switch sent stays true, so only the pre-switch `epoch` read and the conditional `MGActive` save run again (K18.3 step 5), now against the version the user just confirmed.
   - **New epoch** (otherwise): in the same call, the waiting op is cancelled as V15.3 says for switch sent (the binding before returns to its recorded state, a fence step 2 cleared is restored with its prior decision, and the uploaded zone is left to K18.5); then `publish` runs from step 2 with a new epoch id, with the reason V12.2 and `engine/collection-replace#R6.7` give for the restored state: the restored `pendingDecisionReason` when the fence is pending, else 2, since `MGActive` names an epoch. Its new binding's reconcile scan (K18.3 step 2) makes every live entity pending, so edits made during the first publication are published with the rest.

- **Steps recorded** (decided 2026-10-27). The op row's step is `confirmed` (the row is written), `bound` (step 2's binding commit), `zones` (K18.3 step 1 done: step 3's zone creation succeeded, `root` is acked, read through a save conflict or created, and `MGEpoch` `publishing` is acked), `sent` (K18.3 step 3 done, below), `complete` (K18.3 step 4: `MGEpoch` `complete` acked), then `done` (step 4's switch), which removes the row; and `cancelling` while a Cancel completes (V15.3). Decided 2026-10-27: at `cancelling` the gate is closed for the new binding as at `confirmed` and `bound` (`sync/policy#P6`), and the exemption that K2 and K12.1 rule 3 give a step before `done` no longer applies, so the core builds no batch for the new binding. Zone, `MGEpoch`, `MGMediaRoot` and `MGActive` writes are U114's own operations, not engine sends (V26.1). K18.3 step 2's scan runs, and the new binding's instance starts, once the step is `zones`. Decided 2026-10-27: the core reports every step change as a `Binding` input with the step (`sync/policy#P3` `publishStep`). The changes to `zones`, `sent`, `complete` and `done` leave the policy token unchanged, and only the change to `cancelling` increases it (`sync/policy#P4`), so an exchange that waits or runs during the publication keeps a current guard. The change to `zones` opens the gate for the new binding (`sync/policy#P6`, "Publication"), and while the step is `zones` its fetches, sends and media start run as with auto on, whatever the setting, so the publication does not stall with auto off. Decided 2026-10-27 (review round 3, `sync/policy#P6`): so the new binding's engine runs with automatic scheduling on during `zones`, and the engine retries by itself the names it keeps after a network or throttling failure; the auto setting is applied at `sent`, whose `Binding` input emits `ConfigureAutomatic{off}` when auto is off (`sync/policy#P10.1`, the `Binding` row). The change to `done` asks for a send of what is pending (`sync/policy#P10.1`, the `Binding` row).
- **K18.3 step 3 is done** when one tracking commit of the new binding finds all of these hold: K18.3 step 2's reconcile scan has committed and no scan is required (K11.1); no entity of the new binding is sendable (K12.1) or in flight (`sync/policy#P1`: handed to the current engine instance, with no result yet, in a send that has not ended; a durable in-flight entry alone does not count, since a record handed and never saved is sendable again, `sync/cloudkit-records#K12.4`); no entity is held only by `sync/policy` eligibility (a held-back answer, R2047, `sync/policy#P7`), by an unclaimed local generation (K12.1 rule 7) or by a reference to an entity in one of these states (K12.1 rule 5); and, while media sync is on (`DeviceSettings.media`) and the new binding has no `media_zone_missing`, no blob of the new binding is ready to upload (K9.3) or uploading, and none is in server state `unknown` while a live media-name entity of the new binding names it with the local file holding that content and none of the flags that K9.3's upload rule excludes (decided 2026-10-27: a new binding's blobs start `unknown`, V10.1, and only a read round can make one ready, so an unread blob would otherwise let the epoch complete before its upload). Entities that stay pending only for a flag that needs the user (`oversize`, `unreadable`, `unsyncable`, `file_missing`, K12.1 rule 1) or for a reference to such an entity do not block: they stay pending and listed (V11), and step 4's counts are of the live entities acked in the new zone. With media sync off the epoch completes without its blobs, which upload when media is on, as for any binding. The core checks this in every tracking commit of the new binding while the step is `zones` (a save result, a blob acknowledgement, a blob read result, a scan result, a dirty mark); the commit that finds it true also sets the step `sent`, and the core then reports the `MGEpoch` `complete` write as due (V26.2). No input from the actor reports it.
- Local edits during steps 3 and 4 are tracked under the new binding (K18.3).
- Replace iCloud takes no local safety backup: the local collection does not change. The cloud side's recoverable copy is the previous epoch's zone (K18.3, K18.5).
- So when `meguri-media` is missing, step 3 creates `meguri-media` and a new `MGMediaRoot` (V10.1, V25). After an encrypted data reset the publisher reads `root` first (step 3): it takes a `root` whose `e` it reads and decodes (K9.2), since another device may have recreated the zone and its blobs are the active epoch's; it creates a missing zone and a new `root`; and it deletes and creates again only a `meguri-media` whose `root` read gives K2's sign of an unreadable zone (`e` absent, or CloudKit's decryption error for that record; K2, V10.1, V26.1).
- **Progress within step `bound`** (decided 2026-10-27). The op row records no step between `bound` and `zones`; the durable facts show which writes landed: the binding's `root` acked flag (K10.3 `bindings`) and the op row's epoch system fields (V2.3). The success of the zone creation is kept in memory with the row; a start begins without it. So a resumed op creates the zones again, then saves only what is not yet acked. Every write of step 3 is safe to run again: a new-record save whose earlier attempt landed meets "server record changed" and counts as saved or takes the server's key (V26.1). The commit that accepts the last of them sets the step `zones`.

### V15.3 Crash, failure and cancel

- At restart, the op resumes from its recorded step without asking again while its new binding is active and the account identity equals the new binding's. Otherwise it waits, and the status shows Resume or Cancel (V11).
- **Resume after an account event** (decided 2026-10-27). During a publication, V9.1's rows isolate the new binding as `signed_out` or `account_changed` like any binding. Resume (`resume_recovery_op`, V26.2) is then offered when the account status is `available` with an identity, read in this process, equal to the new binding's, the open collection's publication identity is the new binding's, and V8.2 found no file mismatch since the isolation. V9.2's `MGActive` condition does not apply: `MGActive` names the new epoch only after the switch. In one tracking commit Resume makes the new binding active, deletes its `engine_state` row (A12), records the open collection's file identity (V8.2) and clears its `scan_state` drained flag, then settles its intents and runs its scan as V2.2's "Restoring a binding" says; the op then continues from its recorded step, its own operations are reported due again (V26.1, "Due again"), and from step `zones` its instance starts with no cursor after the scan. For another identity Resume is not offered; Cancel is.
- Throttling or a network loss during step 3 leaves the op at that step, with V10.3's status. The engine retries its own sends; the op's own operations are reported due again at the first event trigger after the condition clears, `Network{reachable: true}` after `false` included (V26.1, "Due again").
- Quota exceeded during step 3 leaves the op at that step with V10.3's status and the reason "iCloud needs room for both collections while the previous one is kept for 30 days"; actions Cancel and storage guidance. It continues when a later save succeeds.
- Cancel is possible at every step before `done`, except while an attempt of the `MGActive` save runs (V2.3), and needs no network. In this order (decided 2026-10-27): the op row's step becomes `cancelling`, durably, and the core reports that step change as a `Binding` input (`sync/policy#P3`), so the policy closes the gate for the new binding and stops its instance (`sync/policy#P6`, `StopEngine`); from that commit the core builds no batch for the new binding (V15.2, "Steps recorded"), and its instance's late facts are refused once the binding is deleted (V3.4). Then, when step 2 cleared the fence, the sidecar's prior decision is restored exactly as the op row's fence before recorded it, its `pendingDecisionReason` included, through `engine/collection-replace#R6.1`'s sidecar write (decided 2026-10-27), so a later Replace iCloud still publishes with reason 3 to 5 and the status still names the restored or imported collection (R6.7); then one tracking commit deletes the new binding and its tracking, returns the binding before to its recorded state and reason, and removes the op row. When that state is `active`, the same commit clears that binding's `scan_state` drained flag, and its reconcile scan runs before any instance of it starts (V2.2, "Restoring a binding"). So local edits made during the publication, which were tracked under the new binding only (K18.3), are sent to the old epoch. At restart, an op at `cancelling` finishes these steps.
- **Switch sent** (decided 2026-10-27). Once the op row's switch sent is true (V2.3), an `MGActive` save naming the new epoch may have landed with its result lost (a network failure, a kill), so `MGActive` may name the new zone. A Cancel then deletes no zone: the new zone is left to K18.5's 30-day rule, which never deletes the zone `MGActive` names, and the cancel commit gives no binding a `zones` row with deletion due for it. When the binding before returns to `active`, its root fetch confirms its epoch before it sends anything (V10.2); when `MGActive` names the new epoch, that fetch isolates it `epoch_switched` with the edits pending, which V14, V15 or V16 then settle. Nothing is deleted that another device may be bound to.
- Otherwise the new zone, which `MGActive` never named, is deleted after that commit (K18.5), decided 2026-10-27: when the binding before returns to `active` in the same account and environment, the commit gives it a `zones` row for the new zone with deletion due, so its next `DeleteZones` deletes the zone (decided 2026-10-27, review round 4: an own operation, never engine work, V26.1), and the mark stays due until the deletion's result (`sync/policy#P10.3` item 3; with auto off, from a Sync Now fetch step's drain, `sync/policy#P12.3`); otherwise U114 deletes the zone once with its own operation, when the account identity read in this process is the one the new binding had (V26.1, "Zone deletion after a cancel": the commit leaves no op row, so this deletion has its own rule). A deletion that does not run leaves the zone to K18.5's 30-day rule. Deleting the zone only after the commit means a crash never leaves an op row whose zone is already gone.
- A crash inside step 2 is completed at restart: the op row proves the confirmation.

## V16. Pause, stop and local only

| Action | Binding | Effect |
|---|---|---|
| Pause iCloud Sync (Account & Sync, R117, R2046) | stays active | a device setting in U151's `DeviceSettings`, passed to the policy (`sync/policy#P3`, `#P6`). No instance runs; nothing is fetched or sent; pending work stays and dirty marks continue. Sync Now is unavailable while paused. Status: paused. Resume clears the setting and needs no decision |
| Automatic sync off (R123) | unchanged | only automatic scheduling stops; Sync Now still exchanges (`sync/policy#P6`, `#P12`) |
| Stop using iCloud on this device | isolated `stopped` | the instance stops; pending changes are kept; status local only; Resume follows V9.2 |
| local only (V12, V13) | none usable | as Stop; the sidecar decision is local only |

None of these deletes iCloud data. Meguri offers no action that deletes the user's iCloud data; system settings do, and Meguri then sees V10.1.

## V17. Conflict copies (R2054)

### V17.1 When

Only where `sync/cloudkit-records` K16 says. Cards get copies only together with their note (K16 `missing_notetype`, `unmapped_structure`); revlog, tags and config get none (K15). A copy commits in tracking before the op that replaces the losing content (K16).

### V17.2 Where they appear

- Review Conflicts, reached from the iCloud needs-attention popover and from Account & Sync (U161).
- The list holds every unresolved copy of every binding on this device, isolated and salvage bindings included, newest first, grouped per object (kind and key). A card's copy is listed in its note's group. Copies of an isolated or salvage binding are labelled "from an earlier iCloud connection".
- Unresolved copies keep the attention item `conflicts`, and so the state needs attention, until each is resolved (`sync/policy#P14.4`, `#P22`). Opening Review Conflicts does not clear it.
- Account & Sync shows the count of unresolved copies.

### V17.3 Operations

Core operations for U161. All run off the main thread and need no network.

| Operation | Result |
|---|---|
| `list_conflicts` | per copy: id, kind, key, reason, created at, binding label, object label (V17.5), and whether the object exists locally |
| `get_conflict(id)` | the saved version, decoded from its envelope, and the current local version or "deleted"; for a note or card, its fields and templates by name, from the names the copy recorded (V25) |
| `preview_recovery(id, selection)` | the normal edit that recovery would submit, validated as that edit is, and what it would change |
| `recover(id, selection)` | submits that edit as a normal op, undoable as that kind of op is. The copy stays unresolved |
| `resolve(id)` | sets resolved at; the copy leaves the list |
| `unresolve(id)` | clears resolved at, while the copy still exists |

- Resolved copies are deleted 30 days after resolution, by `RunCleanup` at a drained collection fetch and also at each collection open after V2.4 step 6 (`sync/policy#P10.3` item 4, V2.4), with the staged file a media copy holds (V6). Meguri never deletes an unresolved copy, whatever the state of its binding.
- A recovered edit is a local change and syncs like any other.

### V17.4 What recovery submits

| Reason | Selection | Edit |
|---|---|---|
| `field_conflict` | some fields of the saved version | update the note's fields with them (the editor's validated note update, U190). Fields are matched by name with the note's current note type; a saved field whose name the current note type lacks cannot be selected |
| `edit_vs_delete`, note | the saved version | add a new note with a new id and a new `guid`, with the saved fields and tags, in the deck the copy recorded when it still exists, else the current deck. Its cards are new: the tombstone is final (K17.3) |
| `edit_vs_delete`, deck or preset | the saved version | create a deck or preset with that name and settings; a name in use follows `decks/decks#D2` |
| `edit_vs_delete`, note type | the saved version | add it as a new note type with a new id |
| `edit_vs_delete`, media name | the saved version | restore the file from the media trash under its name, or under a new name when the name is in use, through the media store's normal operations (`media/media-files#M10.2`). For a copy whose bytes K16 downloaded into the cloud staging directory, add those bytes under its name through `media/media-files#M3`, which picks another name when that one holds other content; while the bytes are unavailable (K16), recovery fails with `RecoveryInvalid` |
| `blob_conflict` | the saved settings | the normal deck options or preset save |
| `notetype_conflict` | the name, the CSS, or one template's text | the normal note type save of those parts. A change to the field or template lists is a structural edit (K19; backup per T6.1) |
| `missing_notetype`, note | a target note type and a field mapping from the recorded field names | add a new note under it |
| `structure_rollback`, `unmapped_structure`, note | field contents, by the recorded field names | put the text into chosen fields of the current note (an update), or add a new note |
| `structure_rollback`, note type | the saved version | add it as a new note type with a new id |
| `missing_notetype`, `unmapped_structure`, card | none | none: shown read-only in its note's group. Restoring the note makes new cards; the card's scheduling is not restored |
| `media_content` | the saved file | restore it from the media trash, or rename it back, through the media store's normal operations |

- Recovery never replaces the collection and never writes rows outside a normal op (R2054).
- When the edit's validation fails (a field count mismatch, a missing note type), `preview_recovery` says why, and `recover` fails with `RecoveryInvalid` and the same reason.

### V17.5 Object labels

A note's label is the sort field of the saved version as plain text, at most 80 characters; a card's is its note's label and its template name; a deck's, preset's or note type's is the saved name; a media file's is its name. Labels are read from the saved envelope in tracking and are never sent.

### V17.6 Copies of isolated bindings

When a recovery op deletes a binding's work (V14.2 step 9; V15.2 step 4 for a binding of the same account and environment), the binding's conflict copies stay, and its binding row stays with them. The row is deleted once it holds no copy and no work.

## V18. Incomplete structural generations

- A remote generation in state `waiting` (K19.7) holds back the notes and cards of its notetypes on this device (K12.1 rule 8, K19.5).
- Tracking keeps, per waiting generation, its last progress instant: when a drained fetch (A7) last brought its manifest or one of its members (V25).
- Abandon is offered only when all hold: a drained fetch of the collection zone (`sync/policy#P3` `FetchDrained{collection}`, with `fetched` true or false: a drain that found nothing new counts) finished more than 7 days after the last progress instant; the generation is still `waiting`; and the manifest as last fetched has state `claimed`. The status then shows needs attention: "changes to note type <name> from another device are incomplete", with Abandon.
- When the manifest is `complete` and the generation still waits on this device (a member waiting for an asset, a reference or a newer format), Abandon is never offered. Needs attention lists the waiting members, and they are fetched again (K13.1).
- Abandon, after confirmation and outside a review session (V20):
  1. records a `recovery_ops` row `abandon_generation` with the generation id and the change tag of the manifest as last fetched, step `recorded`;
  2. fetches the manifest again, by record ID (V26.1). When it is `complete`, nothing is abandoned: the op ends, and the generation is adopted when it is complete for this device (K19.5). When it is `abandoned`, the op continues at step 3. When it is `claimed`, the manifest is saved with state `abandoned`, conditional on the version this read returned (the op row's manifest system fields, V2.3), as U114's own operation (V26.1); on a conflict, step 2 runs again. A save of `abandoned` is never made over a `complete` manifest;
  3. in the commit that records the manifest as `abandoned` (the save's accepted result, or step 2's read of an `abandoned` manifest), which sets the step `abandoned`: runs the rule "An abandoned manifest on every device" below, so every member already in this device's inbox is kept as a conflict copy and removed from the inbox;
  4. in one tracking commit, which removes the op row: publishes this device's current layout of each anchor notetype as a new local generation (K19.2), whose maps lead from the abandoned layout back to this device's, so every device ends on the layout from before the abandoned change. Decided 2026-10-27: step 3 gave each anchor the system fields and acked server `struct_gen` of its abandoned version; an anchor with no entry in the inbox takes the abandoned manifest's `to_struct_gen` as acked server `struct_gen` in this commit. So the new generation's value is above the abandoned one, and its claim does not meet K19.4's first row. Content of fields that only the abandoned layout has becomes `structure_rollback` copies on the devices that hold it.
- **Step values** (decided 2026-10-27): `recorded` resumes at step 2's read; `abandoned` resumes at step 4. Step 4's commit removes the row (V2.3), so a crash never repeats it.
- **An abandoned manifest on every device** (decided 2026-10-27). The tracking commit that records a manifest with state `abandoned` for a generation this device holds as remote `waiting` (a fetched version, or step 3 above on the device that abandons it) makes the generation `abandoned` (K19.7) and, in the same commit:
  - stores the manifest's maps in `generation_maps`, so a device still at the layout before the abandoned change has every step of the chain to the restoring generation of step 4, whose maps start at the abandoned layout (K19.5);
  - keeps every inbox entry that waits with reason `generation` for that id as a conflict copy and deletes the entry, so K12.1 rule 8 no longer holds its entity:
    - an anchor (a notetype version in the abandoned layout) becomes a `structure_rollback` copy (V17.4). Its entity, when this device tracks it, takes that version's system fields and `erev`, and its acked server `struct_gen` (`notetype_layouts`) takes that version's `struct_gen`; its rows and acked digests stay, and it is not sent again for this: the restoring generation of step 4 replaces that version on the server, conditional on it;
    - any other member becomes an `unmapped_structure` copy. Its entity, when this device tracks it, takes the copied version's system fields, `erev` and acked digests, keeps its earlier acked payload as its base (K11.2) and keeps its own rows, as a tombstone takes the server's change tag after deletion wins (K17.3). Its row digest then differs from the acked one, so it is pending and sent again (K12.1): its local row replaces that version, and no later save conflict meets it.

  A version whose `generation` names a generation this device holds as `abandoned` and that comes later, fetched or in a save conflict, is handled the same way in the commit that records it. A manifest `abandoned` for a generation this device has not seen yet is recorded `abandoned` with its maps stored. So no entry waits for an abandoned generation on any device, and Abandon need not be offered on each one.
- A device whose own generation is abandoned handles it as `sync/cloudkit-records#K19.8` says (decided 2026-10-27, review round 4): the generation becomes `rolled_back` with no K19.6 rollback, and its anchors and their notes and cards wait until this device adopts the restoring generation of step 4.
- After a crash the op resumes from its recorded step.

## V19. The damaged-collection screen (U153, U251)

`maintenance/optimize-integrity#M7.1` reserves remedy 3 for this file. It is **Replace with the iCloud collection**:

- **Offered** in `Failed{Corrupt}` when all hold: iCloud is enabled on this device; the last known account status is `available`; and this device holds a binding in that account and environment (active or isolated), or has read an `MGActive` naming a complete epoch there (A17).
- **What it runs**: V14 with the active epoch as source. The staging fetch only reads CloudKit and writes only the staging directory, so it may run in `Failed{Corrupt}`; sending stays refused (M7). The safety backup is T7's byte copy. `engine/lifecycle#L4` and `#L5` admit `CloudAdopt` from `Failed{Corrupt}`.
- **Preview**: the iCloud collection's counts; "the damaged collection cannot be read"; the binding's unsent change count from tracking, with the note that those changes exist only in the damaged file and its byte copy.
- Offline when selected: the screen shows the offline reason and keeps the entry.
- The screen lists the remedies in M7.1's order: Check Database, Restore from Backup, Replace with the iCloud collection; Copy Debug Info is always there (U153). R169's "Download from Server" is this entry for routine sync. No entry downloads from an Anki server (M7.1, KTD26).
- A Restore from Backup chosen here sets the fence (V12) and isolates the old binding.

## V20. Review sessions (R89, R458)

- Adoption (V14), abandoning a generation (V18) and the connect flow (V13) do not start while a review session is open (U43). A decision that needs them waits for the session's end; its status reason shows meanwhile, with no modal during review (R1562).
- An adoption whose exclusive phase finds a session open waits at step `ready` until the session ends (V14.2 step 7); its answers commit to the collection the preview said would be replaced.
- An epoch switch found during a session isolates the binding at once, so nothing more is sent to the old epoch; the choice waits.
- Replace iCloud (V15) and conflict review (V17) may run during a session. A recovered edit to a card under review reaches the session as another source's change (R457).

## V21. Errors

Errors of the recovery core calls (V26.2); U114 maps them to its FFI error set.

| Error | When |
|---|---|
| `SessionActive` | V20 |
| `Busy{running_op}` | V14.2 step 3: the long-operation slot is held; an `adopt` or `publish` op while another one of the running build's container and environment exists (V2.3); a row of another build never causes it. Not for a `publish` op whose switch conflict is set: `adopt` and `keep_local_only` cancel it first, and `republish` continues it (V15.2 step 5) |
| `AccountUnavailable{status}` | V13, V14, V15 without status `available` and an identity read in this process |
| `AccountMismatch` | a resume or a recovery op whose binding's account is not the current account |
| `EpochIncomplete` | `MGActive` names an epoch without a complete `MGEpoch` |
| `EpochChanged` | `MGActive` changed between the preview and the confirmation, or K18.3 step 5 conflicted (V15.2 step 5) |
| `InsufficientSpace{needed, available}` | V14.2 step 4, the preflight of V14.2 step 7, the replace |
| `NotOffered` | a choice that V12, V13, V14.1, V15.2 step 5, V15.3, V18 or V19 does not offer in the current state |
| `ConflictNotFound` | V17.3 with an unknown or deleted copy id |
| `RecoveryInvalid{reason}` | V17.4: the edit's validation failed, or the copy has no recovery edit |
| `Cancelled` | the user cancelled; the state is as V14.3 or V15.3 says |

The errors of `engine/collection-replace#R9`, `package/backup-triggers#T9` and `engine/lifecycle` pass through unchanged.

## V22. Invariants

1. No flushed local change is lost by a crash, an account event or a service condition: it stays in the collection and stays pending until content equal to it is acked (R2051, R2054).
2. No cursor is saved past a record or a record deletion that is not durably in the inbox, refused or applied (K21 invariant 5).
3. No fact from CloudKit, from a stopped engine instance or from a staging fetcher changes an isolated binding's rows. Only isolation itself (V2.2), the user operations of V9.2 and V14 to V18, and the salvage of V8.1 do.
4. Nothing is sent while the fence is pending, while sync is paused, from an isolated binding, or to an account other than the binding's (R1548, R2055). Decided 2026-10-27 (review round 4): this holds for zone deletions too. They are never engine work, which the engine would send by itself with no batch request; `DeleteZones` runs them as an own operation whose guard `zone_deletion_check` checks right before the deletion starts, so an input that closes the gate (Pause, an account change, a stop, a fence) makes it delete nothing (`sync/policy#P4`, `#P10.3`).
5. Nothing replaces a local or an iCloud collection without the user's confirmation, except V13.3's adoption into a device with no collection.
6. Every replacement of a local collection keeps a safety backup of it. Every replacement of an iCloud collection keeps the previous epoch for 30 days (R2056): Meguri never deletes it earlier, except a zone that survived an encrypted data reset unreadable (V10.1). A zone that CloudKit itself removed (an encrypted data reset, a deletion in system settings) leaves no previous epoch to keep.
7. A recovery decision survives relaunch and is answered only by the user.
8. Meguri never deletes an unresolved conflict copy.
9. Two non-empty collections are never merged.
10. Nothing in this file contacts an Anki server (KTD26).
11. An adoption never sends a local media file of the replaced collection: a file the adopted epoch does not name is never uploaded, and one it names with other content never wins over the epoch's, unless the user changes the file or adds a reference to it in a note afterwards (V14.4).
12. Abandon never overwrites a `complete` manifest (V18).
13. No account or service condition that one build sees isolates or changes a binding of another container or environment (V9.1).
14. An engine instance sends nothing to its collection zone before its bound epoch is confirmed for it: by a drained root fetch of its own, by a `root_read` result accepted for it, or by the publisher's accepted `MGActive` save in its `done` commit during that instance; except a publisher before its switch (V10.2, `sync/cloudkit-records#K21` invariant 20).
15. A binding that becomes active again after an isolation runs its reconcile scan before any instance of it starts, so no edit committed while it was isolated stays unsent (V2.2).

## V23. Test obligations

### V23.1 Fault points

Fault points for U21's `faultpoint!` and U250's catalog. The fault catalog is `conformance/cloudkit-faults#F5`: it holds every point below, with its expected recovery, and the points `conformance/cloudkit-faults#F7` adds for windows this list leaves without a name. With the CloudKit fake of K22.2, a kill at each point and a restart must end converged, with no lost change, no duplicate, and no record sent to a wrong binding or account.

| Fault point | Window |
|---|---|
| `cloud.send.after_commit` (with an OS crash, before the flush) | V4 S1 |
| `cloud.send.after_flush` | V4 S2 |
| `cloud.send.after_dirty_mark` | V4 S3 |
| `cloud.send.after_sent_mark` | V4 S5 |
| `cloud.send.after_asset_stage` | V4 S6 |
| `cloud.send.after_server_save` (the fake drops the result) | V4 S7 |
| `cloud.send.after_ack` | V4 S9 |
| `cloud.send.conflict_after_inbox`, `cloud.send.conflict_after_commit` (with an OS crash, before the flush), `cloud.send.conflict_after_flush` | V4 C1, C3, C4 |
| `cloud.fetch.after_inbox` | V5 R2 |
| `cloud.fetch.after_cursor` | V5 R3 |
| `cloud.fetch.deletion_after_cursor` (a batch that reports a record deletion, before its apply) | V5 R3, `deletion` entries |
| `cloud.apply.after_intent`, `cloud.apply.after_commit`, `cloud.apply.after_flush` | V5 R4 to R6 |
| `cloud.media.after_clone`, `cloud.media.after_download`, `cloud.media.during_install` | V6 |
| `cloud.rekey.after_commit` | V7 |
| `cloud.claim.after_send` | V7 |
| `cloud.tracking.after_move_aside`, `cloud.tracking.after_salvage` | V8.1 |
| `cloud.prefetch.during_stage` | V13.2, V14.3 |
| `cloud.adopt.after_record`, `cloud.adopt.during_stage`, `cloud.adopt.after_ready`, `cloud.adopt.after_activation` (R6.3 done, before step 9), `cloud.adopt.after_done` (before any close), and every `replace.*` point of `engine/collection-replace#R8` with reason `CloudAdopt` | V14.3, V8.2 |
| `cloud.publish.after_confirm`, `cloud.publish.after_binding`, `cloud.publish.after_fence_clear`, `cloud.publish.after_step_1`, `_3`, `_4`, `cloud.publish.after_switch` | V15.3 |
| `cloud.own.after_server_save` (the fake saves an own write of V26.1 and drops its result), for each write of V26.1 | V26.1 |
| `cloud.publish.after_cancelling`, `cloud.publish.after_cancel_fence`, `cloud.publish.after_cancel_commit` (before the new zone's deletion) | V15.3 |
| `cloud.abandon.after_record`, `cloud.abandon.after_manifest` | V18 |

- At `cloud.send.after_sent_mark`, the relaunch's full update hands the entity's name to the engine again and the change is sent: the durable in-flight entry does not hold it back (`sync/policy#P1`, `sync/cloudkit-records#K12.4`, "Handed, never saved").
- At `cloud.send.after_server_save` (V4 S7), the landed version counts as saved, through the next save conflict and, in a second run, through a fetch, only because its `writer` is this binding and its digest is an in-flight one (`sync/cloudkit-records#K13.2`, "Own landed saves"). In a third run this device's save does not land and another device saves the same payload by itself: its version, met through the save conflict or a fetch, is merged as usual and never taken for this device's own (`sync/cloudkit-records#K22.2`, "own versions by `writer`").

### V23.2 Without a kill

- The start sequence (V2.4): a kill in an adoption after R6.3 restored the old file isolates nothing as `replaced_locally` by mistake: the new binding is deleted and the old one returns.
- A durable tracking failure during a fetch: the instance stops, no cursor is saved, and the fetch again converges; the instance restarts only at a trigger after a tracking write succeeds, with no timer (V3.3).
- Sign-out; an account switch with an operation in flight and late callbacks from the old instance: none changes tracking. Resume after signing in to the same account runs the scan before the first send and converges; another account never receives a record.
- An offline launch: no instance runs until the identity is read; with the account switched while the app was not running, nothing is sent to the new account. With the app kept in front: the network's return, and separately a `CKAccountChanged` after a `temporarilyUnavailable` status clears, each cause one account read, the instance starts, and an answer given meanwhile is sent with no foreground return (V9.1).
- Network return (V26.1, "Due again"): an `MGEpoch` `complete` write that failed with network unavailable, in an idle app with nothing pending, is reported due again at `Network{reachable: true}` and the publication continues; a second `true` with no `false` between reports nothing.
- An account switch during suspension followed by zone not found: the binding is isolated as `account_changed`, never `zone_missing`.
- A zone deleted by the user, a missing zone, an encrypted data reset, `MGActive` removed: each isolates and never recreates; V15 publishes again after confirmation, and a second device is then offered adoption. A missing media zone recreated by another device is not used by the old binding; the adopting device reads the new `MGMediaRoot`.
- An encrypted data reset that removes every Meguri zone: V15 on one device creates `meguri-root`, `meguri-media` and a new `MGMediaRoot`, and its switch completes; a second device, still isolated as `encrypted_data_reset`, is offered V14 for that epoch and adopts it.
- An epoch switch while a device is offline with edits: on reconnect it sends nothing to the old zone before its root fetch drains, and its edits stay pending. With a switch injected between that root fetch and a send, the saves are acked in the old zone, and the V14 preview counts them as sent after the replacement, with the note that they are only in the safety backup and the previous epoch.
- A relaunch from a saved cursor with an unchanged root zone and offline edits: the root fetch drains with nothing fetched, confirms the epoch from the last applied `MGActive`, and the edits are sent (V10.2). An instance with no saved state (a Resume, V9.2) whose bound collection zone was deleted meanwhile: its first fetch does not list the zone, the binding is isolated by K12.5's plain `zoneNotFound` row, and no `deletion` entry and no local deletion is made.
- Publication writes (V26.1): during each V15.2 step, the zones, `MGEpoch`, `MGMediaRoot` and `MGActive` are written only as own operations, never through the engine's pending lists, and the new binding's engine instance starts only at step `zones`.
- A `crt` mismatch (`sync/policy#P6`): needs attention with V14 and V15 offered; nothing is isolated; a relaunch finds it again at V2.4 step 4.
- A build of another container or environment opens the collection, then the matching build does: the matching binding is usable with its pending changes, needs no Resume and no replace, and the other build stored nothing into it; the other build shows local only, not connected.
- Quota exceeded during a batch, throttling with a retry-after time, an expired change token when the fake surfaces it: pending work survives, completion is not reported, and no app timer runs. A fetch from the start that the engine runs on its own makes no K17.4 absence check.
- A tracking file deleted, damaged (its conflict copies are salvaged and stay listed; an unreadable `conflicts` table keeps the file), and newer than the build (V8.1); a collection file with a new inode under the old tracking file: V8.2 isolates and nothing is sent. A binding with no recorded identity records it and is not isolated.
- Restore, `.colpkg` import and manual Anki import while bound: the fence holds through relaunch, Sync Now and a manual Anki transfer; each V12.2 choice; adoption from a fence-pending start leaves the fence clear and the new binding usable.
- The six cases of V13.3 and the four choices of V13.5. Two devices that both upload a first collection: one switch wins, the other gets V15.2 step 5.
- A prefetch followed by Replace iCloud: the prefetch row is deleted and V15 is not refused with `Busy`.
- Restoring a binding (V2.2): Replace iCloud while bound, local edits and answers during step 3 (with quota exceeded holding it there), then Cancel and a clean close and reopen: the old binding scans before its instance starts, and every one of those edits is sent to the old epoch and reaches a second device. An adoption waiting at step `ready` for a review session, the session finished, then Cancel: the session's answers are sent to the old epoch. A kill after the cancel commit and before the scan: the next open scans and sends them.
- The base after a restore (V2.2): a binding signed out for a day while another device removes a tag from a note, flags a card and renames a deck; this device meanwhile edits another field of that note, moves the card and changes the deck's other settings, with later stamps. After Resume, and again after a cancelled publication and a cancelled adoption: the tag stays removed, the flag and the deck name are kept, the other device's field edit is kept, the local changes are kept, and no conflict copy is made for parts only one side changed.
- Own operations (V26.1): with sync paused, a tracking stop or another account, a due `MGEpoch` or `MGActive` write is not reported and nothing is sent; it is reported after Resume, after the stop clears or after the account matches again. A `zones` create that fails with a retry-after of 120 s: a local change 5 s later reports nothing, and the first event trigger after 120 s reports it again; a relaunch keeps the retry after. A preview read for one account whose result arrives after an account switch is dropped. A publication cancelled before switch sent whose binding before does not return to `active`: the zone deletion is reported once, U114 deletes the zone once for the new binding's account, and a deletion that did not run is left to K18.5. A binding with no last applied `MGActive`: the record-ID read's result is recorded, confirms the epoch and the send follows; one naming another epoch isolates `epoch_switched` after the account read.
- Engine instances (V3.4): after `ConfigureAutomatic` recreates the engine, a batch request from the old engine gets no batch, writes no `sent` mark or in-flight entry, and sends no record. With a fake that keeps the old engine object alive after its release and lets it fetch and ask for batches while the new instance runs, every fact of the old object is refused with `StaleGuard`, tracking is unchanged by it, and no record is sent twice.
- A switch during a long fetch call (V10.2, "Which value the mark records"): device D, confirmed and with auto on, runs `RequestFetch{root, collection}`; the fake returns `meguri-root` naming E0, then holds the collection fetch for 2 minutes, during which another device switches `MGActive` to E1 and 50 of D's answers are acked in E0. The drain records the mark from the call's first will-fetch-changes event; D's next root fetch isolates it `epoch_switched`, and the V14 preview counts all 50 as sent after the replacement. A second `fetchChanges` call asked for during the first starts only after the first returns.
- A waiting generation at Replace iCloud (V13.4, V15.2 step 1): devices A and B bound to E0; B adds a field to note type T with 3,000 notes, and A has fetched the manifest and 2,000 members, which wait with reason `generation`, plus 5 `apply_failed` entries. A's Replace iCloud preview shows 2,005 changes fetched and not applied, by reason, names T, and says the new collection will not include them; after `done` A's inbox for E0 is deleted, and B's claim and members are still in E0's zone, which K18.5 keeps for 30 days.
- Rows of another build (V2.3): a debug build's `publish` op waits at `zones` (another account identity) and, in a second run, is killed at `cancelling`; then the daily-driver build opens the same data directory. It settles, resumes and cancels nothing of that row, writes neither the debug build's bindings nor the sidecar decision for it, shows no recovery op waiting, and its own V14 and V15 actions get no `Busy` from that row; the debug build, launched again, settles its row as V15.3 says.
- Publication gate (V15.2, `sync/policy#P6`): after step 2 the gate is closed at steps `confirmed` and `bound`, the engine of the binding before is stopped and no instance of the new binding starts; the `Binding` input with step `zones` opens the gate and starts the new instance; with auto off, its fetches, sends and media start still run while the step is `zones`; Sync Now pressed at `bound` waits at its step 1 and continues on the new instance. At `done`, with auto on, an edit made after the switch is sent by the same instance with no further root fetch (K2), and the binding's last applied `MGActive` names the new epoch.
- Publication with no confirmed root (K2, V10.2, V26.1): a first publication on an account with no `MGActive`, and a republication while `MGActive` names the previous epoch, reach `done` with no isolation, no `RootReadDue` and no no-state read for the new binding's instance. Sync Now pressed at `zones` continues through the step changes to `sent`, `complete` and `done` with no `StaleGuard`, since those changes leave the policy token unchanged; a Cancel's change to `cancelling` ends it.
- The pre-switch `epoch` read (K18.3 step 5, V26.1): a publisher waiting at V15.2 step 5 whose never-named zone another device deleted after 30 days reads `epoch` before its `MGActive` save, gets zone not found (and, in a second run, unknown item), saves no `MGActive`, and is isolated by the bound collection zone's zone-not-found row with Cancel offered. "Replace it again" within 20 days of `published_at_ms`, with a read of `epoch` that returns it, reuses the zone; more than 20 days after it, or with a read that returns unknown item or zone not found, starts a new epoch and never re-runs V15.2 step 3 (K18.3 step 1) in the old zone (K18.5).
- The media zone after an encrypted data reset (K2, V15.2 step 3): a second publisher that finds a `meguri-media` with a `root` whose `e` it reads and decodes (another device recreated it) takes that key, deletes nothing and saves no `root`, and the active epoch's blobs stay readable on every device; one that finds unknown item saves a new `root`; one whose read returns `root` with `e` absent, and in a second run one whose read fails with CloudKit's decryption error for that record (K2, A18), deletes and creates `meguri-media` again with a new `root`; one that finds a present `e` that does not decode deletes nothing and waits with needs attention.
- Publication progress with a lost send (V15.2): a record of the new binding handed and never saved (a kill at `cloud.send.after_sent_mark`, or a send cancelled before its result) keeps the step at `zones` until it is sent again and acked; it never counts as done because it holds a durable in-flight entry.
- Bindings (`sync/cloudkit-records#K10.2`): bound to account A, then signed in to B and Replace iCloud collection there (a newer binding of B), then back to A with Resume: A's binding is the binding of `sync/policy#P1`, the gate opens, and A's edits are sent to A's epoch; nothing reaches B's epoch.
- No-state zone check (V10.2): an instance with no saved state whose first fetch does not fetch the bound collection zone reads `epoch` by record ID; zone not found isolates by K12.5's plain `zoneNotFound` row, and so does unknown item (review round 4); a read that finds `epoch` isolates nothing and deletes nothing, and the call then gets `FetchDrained{collection, fetched: false}`, which completes no absence pass.
- Resolved copies at open (V2.4, V17.3): a resolved copy 30 days old is deleted at the next open after step 6 on a local-only device with no engine, and its staged media file is deleted after the commit.
- Publication progress (V15.2): step 3 completes with an oversize entity and a card that references it still pending, and their status listed; a held-back answer during a session keeps it at `zones` until the session ends; with media sync off it completes without blobs. A lost result of the `MGActive` save, sent again, counts as saved and is not a V15.2 step 5 conflict (V26.1). With media sync on and blobs no read round has read yet, the step stays at `zones` until each is read and the absent ones are uploaded; a second device that adopts the epoch then finds every blob.
- Publication writes after a fence and a kill (V15.2): Replace iCloud from a pending fence reports the zone creation due when the fence clears, with no other trigger. A kill after the zone creation landed and before the `root` save (`cloud.own.after_server_save` on the zone creation): the resumed op creates the zones again, saves `root`, and reaches `zones` with an acked `root`; a `root` that already exists gives its key to the new binding; no device then sets `media_zone_missing`. A relaunch at `complete` sends the `MGEpoch` `complete` save with the stored system fields, and a lost result of it counts as saved.
- Resume after an account event (V15.3): Replace iCloud collection reaches `zones` with records sent; the user signs out and signs in again to the same account: Resume makes the new binding active, its scan runs, and the op reaches `done`. Signed in to another account: Resume is not offered, Cancel is, and nothing reaches the other account.
- Replace it again (V15.2 step 5): after a conflict, a local change and a foreground return report no `MGActive` save. `republish` within 20 days reuses the zone, keeps the new binding's acknowledgements, saves `MGActive` conditional on the newly confirmed version and reaches `done`; after 20 days it cancels and starts a new epoch, and edits made during the first publication reach the new epoch. "Use that collection" and "keep local only" from that state return no `Busy` and delete no zone.
- Unreadable `root` (V15.2 step 3, V26.1): with no reset recorded, a `root` save that meets a `root` of a newer `format`, one that does not decode, or one with `e` absent (K2's sign of an unreadable value) deletes nothing; the op waits with needs attention. After a reset, a `root` of a newer `format`, or with a present `e` that does not decode, also deletes nothing; only `e` absent or CloudKit's decryption error on the `root` read deletes and creates `meguri-media` again.
- Carried names in the preview (V15.2 step 1, `sync/cloudkit-records#K9.4`): a device bound to E0 whose 300 images were never downloaded, 20 of them flagged `file_missing` after an eviction, previews Replace iCloud collection with 320 carried names whose files are not on this device and the missing-media note; a name `withheld` in the binding before is not counted; after the switch the 320 names are live in E1, those whose blob the media zone holds are downloaded, and the others are listed under `mediaMissing` until another device uploads them.
- Cancel from a restore (V15.3): restore a backup, choose Replace iCloud collection, cancel at `zones`: the fence is pending again with `pendingDecisionReason` `restore`, the status names the restored collection, and a later Replace iCloud publishes with `MGEpoch.reason` 3.
- A binding before isolated `stopped` or `zone_missing` (V15.2 step 2): it keeps its reason during the publication, V9.2's Resume is not offered for it, and a Cancel returns it to that state and reason.
- Cancel after switch sent (V15.3): the `MGActive` save lands and its result is lost, then Cancel: no zone deletion is reported and no `zones` row gets deletion due; the binding before, back to `active`, sends nothing before its root fetch, which isolates it `epoch_switched`, and a second device that adopted the new epoch keeps its zone. Cancel at `zones`: the `Binding` input with step `cancelling` stops the new instance, and no batch is built for the new binding after that commit.
- Intents at a restore (V2.2): an apply whose WAL flush came while its binding was isolated by a publication, then Cancel: the intent is settled before the scan, and its entities are sendable with no relaunch.
- Save conflicts and record deletions (V4, V5): each passes through the inbox before its apply; a kill at each step of C1 to C5, and after a deletion's cursor save, converges with the other device's version kept.
- A zone deletion with reason `purged` and one with `encryptedDataReset`: the binding's inbox entries that no apply intent names, their staged files and its `refused` rows are deleted with the isolation, and its pending rows and conflict copies stay (V1, K12.5).
- A media copy whose bytes were downloaded for it (K16): a relaunch keeps the staged file (V6), Review Conflicts recovers it through V17.4, and the file is deleted only with the copy's row.
- Adoption with a review session started during staging: the op waits at step `ready` and swaps after the session ends.
- Adoption of the previous epoch: no binding, local only, nothing sent.
- Adoption over local media (V14.4):
  - files the epoch does not name are not uploaded; after the user inserts one into a note through the editor (the media journal records nothing), it is uploaded and then the note syncs; a note of the epoch that already references a withheld name syncs its edits; a file removed and added again is uploaded;
  - a same-named file with other content: the epoch's bytes win on every device, nothing is sent from the adopting device, and the local bytes are in its media trash; with no blob in the media zone the name is reported missing and the old file stays, unsent;
  - a name the epoch holds as a tombstone is not re-created from the local file;
  - incoming versions (V14.4): another device adds a file under a withheld name, and the adopting device installs it, moves its own file to the media trash and lists a `media_content` copy that recovers it; a tombstone for a superseded name makes it withheld and keeps the file; a tombstone for a withheld name keeps it withheld and keeps the file; none of the three uploads the local file;
  - an adoption while the media journal's first registration is running, or after it was cancelled: files registered after the adoption are withheld too, and none is uploaded; after the window closes, a newly added file is uploaded.
- Conflict copies: each K16 reason is listed after relaunch, inspected, recovered through V17.4's edit or shown read-only, resolved and unresolved; copies of an isolated binding remain listed; the state stays needs attention until every copy is resolved.
- Abandon: not offered for a manifest that is `complete` on the server while a member waits on this device; not offered on a device that has not fetched for 7 days; a manifest completed during the abandon is adopted.
- Abandon on three devices (V18): device A claims a field reorder on T and disappears; B and C hold A's members waiting. B abandons: C's fetch of the `abandoned` manifest turns C's waiting entries into conflict copies (`structure_rollback` for T, `unmapped_structure` for the notes), C's edits to those notes are sent, and C's pending T notes in the earlier layout apply through the stored maps when B's restoring generation arrives, with no `unmapped_structure` copy for them. B's restoring generation is claimed with no claim conflict. A kill at `cloud.abandon.after_record` and `cloud.abandon.after_manifest` resumes at step 2 and step 4.
- A claim whose result was lost (V7): devices A and B both add a field to T at acked `struct_gen` 5. B's claim lands first, then A's claim is handed and A is killed: at restart A's claim is sent again, its manifest's result shows no manifest, and K19.4's first row makes it `lost`. In a second run A's claim landed: the claim sent again meets A's own manifest, A's generation becomes `claimed`, and no device holds a `claimed` generation whose members never come.
- Pause: no instance, Sync Now unavailable, Resume sends what is pending.
- The damaged-collection screen shows the iCloud entry only under V19's conditions, and choosing it adopts.

Fake runs do not prove Apple's behavior (R2064). Signed multi-device runs in the development container check A11 to A19 and the account and zone rows of V9 and V10.

## V24. Assumptions and open questions

| Id | Assumption | If false |
|---|---|---|
| A11 | asset bytes uploaded for a record that is never saved are invisible and removed by CloudKit | staged uploads need their own cleanup |
| A12 | the account's user record id is a stable identity within the container, and the sync engine's state must be reset on sign-out and on an account switch. An engine sign-in event whose id equals the binding's is not an account change (V9.1, `sync/policy#P3` `Account`). Decided 2026-10-27 (review round 3): Apple documents that the engine resets its internal state, change tokens and pending changes included, when the account changes, and does not say whether a `signIn` with the same id does so; Meguri takes it that the engine may reset at every account-change event, `signIn` included, and treats each as a no-state point (`sync/policy#P3`). The signed runs check when the reset happens (`sync/policy#P21`) | another identity source; V9.2 may keep the cursor. If the engine never resets at a same-id `signIn`, the no-state point costs one full update and, decided 2026-10-27 (review round 4), one record-ID read per zone of the scope that the next call does not fetch (with an unchanged zone, every zone), each of which then gets a `fetched: false` drain for that call (`sync/policy#P3`); it isolates nothing and deletes nothing |
| A13 | V10.3: the engine retries throttling, busy zones and quota exceeded itself with no app timer; quota exceeded is reported per record or batch. False for quota exceeded by the SDK headers (2026-10-27): the engine keeps only the failed saves `sync/cloudkit-records#K12.4` lists, and drops quota-failed ones | K12.4's rule applies: quota-failed names are added back at the next event trigger, with no timer and no pause (V10.3, `sync/policy#P2`) |
| A14 | a zone fetch from no cursor can run outside the sync engine, for staging (V13.2, V14.2). Verified 2026-10-27 from the API: a `CKFetchRecordZoneChangesOperation` with no previous change token returns every record of the zones it names and needs no sync engine; signed runs still exercise it | none expected. No fallback creates a second sync engine (V3.4) |
| A15 | deleting Meguri's data in system settings deletes all its zones and reaches every device | V10.2 covers a partial deletion |
| A16 | an APFS clone of every media file can be made in the cloud staging directory (one volume) | uploads copy the file |
| A17 | the last known account status and identity can be used while the collection is `Failed{Corrupt}` | V19 reads them again before offering the entry |
| A18 | an encrypted data reset deletes every zone that holds encrypted values, every Meguri zone included, and the sync engine reports each deletion with the reason encrypted data reset; a deletion in system settings reports them purged (A15). Decided 2026-10-27 (review round 4, K2): when a zone survives, the only sign that it is unreadable is CloudKit's own, since Meguri decrypts nothing itself: a record-ID read returns the record with its encrypted field absent (`e` of `root`), or fails for that record with CloudKit's error that an encrypted value cannot be decrypted. The signed runs record which of the two CloudKit gives after a reset | V10.1's fallback: the V15 publisher deletes a surviving unreadable collection zone through `DeleteZones` (V26.1) and recreates a surviving `meguri-media` whose `root` read gives that sign |
| A19 | reading the account's user record id needs the network; it does not return a cached id offline after an earlier read on the device (V9.1). Signed runs check it | an offline launch reads the id from that cache: V9.1 uses the id it returns as a read, so the instance starts at launch and the engine waits for the network itself |

Questions for review (each settled; decided 2026-10-27 in the R2066 approval pass where no earlier date is given):

1. Decided 2026-10-27: conflict copies in a damaged tracking file are salvaged (V8.1). When the salvage cannot read every copy, the moved-aside file is kept for good: deleting it after a time could lose a copy the user never saw.
2. Decided 2026-10-27: the previous epoch kept for 30 days is the cloud-side backup of R2056; Replace iCloud writes no local copy of the replaced iCloud collection (V15.2, `package/backup-triggers#T6.1`).
3. Decided 2026-10-27: the two-collections decision never offers a combined collection (V13.5, R2055).
4. Decided 2026-10-27: staging is not resumable across a relaunch (V14.3), and the new binding fetches the zone once more from the start after an adoption (V14.2 step 9). The double fetch is accepted: it is what runs K17.4's absence check, and every apply of it writes nothing (K13.2).
5. Decided 2026-10-27: quota sets no app pause and no timer (V10.3, U113, `sync/policy#P15.1`); the engine drops quota-failed names and Meguri adds them back at event triggers (A13 false, `sync/cloudkit-records#K12.4`); a tracking failure is retried only at event triggers (V3.3, `sync/policy#P15.2`).
6. Decided 2026-10-27: Abandon after 7 days of drained fetches without progress, only while the manifest is `claimed`, restoring the earlier layout for every device (V18). A local-only abandon would leave this device on a layout no other device holds.
7. Decided 2026-10-27: resolved copies are kept 30 days; unresolved copies are kept forever, and they keep the state needs attention until each is resolved; opening Review Conflicts does not clear it (V17.2, `sync/policy#P14.4`, `#P22`).
8. Decided 2026-10-27: a device restored from an Apple backup gets a new file identity, so V8.2 asks the user to choose again; a stale tracking file must never send its rows as edits.
9. Decided 2026-10-27: on an epoch switch, the device's unsent changes are kept in its safety backup after adoption (V14.2), and the preview counts them (V13.4); they do not become conflict copies.
10. Decided 2026-10-27: Pause iCloud Sync makes Sync Now unavailable (V16); Pause means nothing reaches CloudKit.
11. Decided 2026-10-27: only the epoch named by `MGActive.previous_epoch_id` is offered for restore (V14.1); older retained epochs are not listed.
12. Decided 2026-10-27: after an adoption, local media files the iCloud collection does not name are withheld: kept on the device and uploaded only after the user changes one or inserts it into a note; same-named files with other content are replaced by the iCloud files and kept in the media trash (V14.4).

## V25. Changes other specs need

This file does not change them; their owners do.

No item is open (2026-10-27).

Landed in review round 4, and removed from the open list (2026-10-27):

- `sync/cloudkit-records`: K10.3 `recovery_ops` and K10.2's sentence on it (the column container and environment, and a build sees only its own rows, V2.3 "Rows of another build"); K10.3 `bindings` (the confirmation mark is the value V10.2 "Which value the mark records" gives, not the counter at the drain); K22.3's signed runs (after `cancelOperations` returns and the engine object is released, a weak reference to it becomes nil within a bounded time, and it delivers no event or batch request after that, V3.4).
- `sync/policy`: P3 (U114 passes the first will-fetch-changes event of each `fetchChanges` call to the core with the instance id, V10.2); P4 `RequestFetch` (at most one `fetchChanges` call of an instance at a time, and none while a record-ID read of it runs; a call asked for meanwhile starts when the one before returns, V10.2).

Landed in review round 3, and removed from the open list (2026-10-27):

- `sync/policy`: P2 and P3 `Account` (the account read at `CKAccountChanged` and at `Network{reachable: true}` after `false` while the identity is unverified, V9.1); P2 (due own operations are reported again also when the fence clears and at `Network{reachable: true}` after `false`, V26.1 "Due again"); P6 (step `cancelling` closes the gate for the new binding as `confirmed` and `bound` do, V15.2, V15.3); P10.1's `Network{reachable}` row (the account read and the due own operations; blob read rounds and the download queue after a network return are P13.2's to decide); P21's offline-launch test (V23.2).
- `sync/cloudkit-records`: K10.3 `recovery_ops` (the columns active system fields, epoch system fields, switch sent and manifest system fields, V2.3); K10.3 `generations` (the encoded system fields of the acked claimed manifest, which K19.3 step 3's `complete` save needs); K2 and K18.3 step 1 (the publisher always creates the three zones and always saves `root` as a new record, V15.2 step 3); K12.1 rule 3 and K2 (the exemption ends at `cancelling`); K18.5 (a publication cancelled after switch sent, or ended by a V15.2 step 5 conflict, leaves its zone to the 30-day rule, V15.3). Also (decided 2026-10-27, round 3): K19.4 and K19.7 `claiming` (a resent claim is decided by its manifest `g<generation id>` first, and K19.4's first row applies only when no such manifest exists, V7), with a K22.2 test of both cases; K2 (outside the encrypted-data-reset path a `root` this build cannot read deletes nothing, V15.2 step 3, V26.1); K10.3 `recovery_ops` (the columns switch conflict and the prior decision in fence before, V2.3); K19.5, K19.7 remote `abandoned` and K12.3's `generation` row (on every device an `abandoned` manifest stores its maps in `generation_maps`, turns its waiting entries into `unmapped_structure` copies, and a version of an abandoned generation is not held for its id, V18). Also K12.1 rule 3 (no record while iCloud is disabled or paused, or to a stopping instance), K12.4 (`RootConfirmed` in its list), K12.5 and K17.4 (the no-state point after an engine account event, with no absence pass), K9.3 (the completion transaction of a media apply consumes the feed first), and K23 A22.
- `sync/policy`: P3 and P14.4 (`recoveryOpWaiting` for a `publish` op whose switch conflict is set, with V15.2 step 5's choices; Resume of a `publish` op whose new binding an account event isolated, V15.3); P10.3 item 3 (the previous epoch's 30 days from the later of the switch and the `zones` row's first-seen instant); P21 (A20 and A21 in the signed runs).
- `engine/collection-replace` R6.7: V15.3's Cancel restores the prior decision with its `pendingDecisionReason` through R6.1.

Landed, and removed from this list (2026-10-27):

- `sync/cloudkit-records`: K10.3 `entities` keeps the acked payload of every acked entity, and `recovery_ops` has the retry after column (V2.3); K11.2 drops the media-only limit, and a pending entity with no captured base merges with its acked payload as base; K15.1's merge without a base applies only to an entity that was never acked (V2.2, "The base of those edits"); K12.5 and K17.4: the record-ID read of `active` or `epoch` before a zone missing from a no-state instance's first fetch is reported not found (V10.2, V26.1).
- `sync/policy`: P4 `StopEngine` and `ConfigureAutomatic` (each instance's own callback object; a stale instance never gets a batch, V3.4); P3 `RootConfirmed` and P10.1 "Confirmed root" (`RootReadDue` and `root_read` as the channel of the bound binding's record-ID read, and a confirmation by it runs the first-`FetchDrained{root}` row, V26.1); P2 (due own operations are reported again only after event triggers, V26.1 "Due again"); P1 "The binding" (V23.2's test of a Resume after a newer binding of another account); P10.1's `Binding` row for a `publish` op's step change and `done` (V15.2, V26.1).
- `media/media-files` M16: the feed hook after each committed media store operation or registration (`sync/cloudkit-records#K11.1`).

- `sync/cloudkit-records`: K11.1 and K11.4 (no dirty marks for an isolated binding; the scan when one becomes active again, V2.2); K16 (a media copy's staged file stays while its `conflicts` row exists, V6); K2, K9.2, K12.5, K12.6 and K18.3 (zone creation, `MGEpoch`, `MGMediaRoot` and the conditional `MGActive` save are U114's own operations, V26.1, never engine sends; K18.3 step 3 ends by V15.2's condition, with V15.2's step names); K18.5 (a cancelled publication's zone, V15.3); K24 (this file's items marked done). Decided 2026-10-27 (R2066 approval): where K24 had asked this file to treat these writes as control records, V26.1's own operations stand, and K12.6 keeps only the generation manifests the engine sends.
- `sync/policy` P10.3 item 4: deleting a resolved media copy also deletes its staged file, after the commit (V6, V17.3).
- `maintenance/optimize-integrity` M7.1: row 3 points to V19 and its conditions; the jobs that may leave `Failed{Corrupt}` include `CloudAdopt`; the M9 test row for remedies.

- `engine/collection-replace`: reason `CloudAdopt` in R1 and R2 (the caller's operation identity, candidate identity and decision), its intent with the fence false in R5 step 2, its startup table and fence rules in R6.6, and the reason of a pending decision (`pendingDecisionReason`) in R6.7.
- `engine/lifecycle`: `CloudAdopt` in L4 (`Failed{Corrupt}`), L5 (the slot from V14.2 step 3 to step 9), L6 (availability until its exclusive phase) and L9.
- `package/backup-triggers` T6.1: the rows for Replace local collection from iCloud and Replace iCloud collection.
- `sync/policy`: the Pause setting in P3 and P6; the instance id with the guard in P4; the `paused` row 4 of P12.1; P14.2 row 1; in P14.4, `pausedReason` `userPaused`, the items `conflictsUnreadable` (V8.1), `mediaZoneMissing` (V10.1) and `recoveryOpWaiting` (V14.2 step 7, V15.3), and no `isolated` item for `replaced_locally`; P15, P15.1 and P15.2 follow V3.3 and V10.3: no retry timer and no quota pause. Also: P3, a `superseded` entity is not pending for the policy; P12.1 row 3, P14.2 row 1 and P17 read `environment_changed` as every binding being of another container or environment, and no binding as none other than a salvage binding; P14.4's `isolated` item excludes `tracking_damaged`; P15's `changeTokenExpired` row applies only when the engine surfaces it and runs no absence check; P15's `quotaExceeded` row and P15.1 say the engine drops the names and the next full update hands them back at an event trigger (K12.4).
- `sync/cloudkit-records`: K2 (zone creation by a `publish` op only, one CKSyncEngine object per process, the confirmed root), K10.2 (isolation reasons and precedence, `environment_changed` derived, one active binding per container and environment), K10.3 (`recovery_ops`, the `conflicts` copy-time names, the `entities` flags `withheld` and `superseded` with their classified digest, the acknowledgement counter and confirmation mark, the adoption window, the last progress instant, `scan_state` file identity), K11.4 (V8.2's recording points, the adoption window and the `withheld` and `superseded` rules), K12.1 rules 1, 3 and 6, K9.3, K9.4 and K13.2 (`superseded` and `withheld`), K17.4 and A9 (only an instance Meguri created with no saved cursor counts as a fetch from no cursor; purging off), K18.3 (the binding before step 1), K18.4 (no cursor after adoption), K19.5 and K19.7 (abandoning is V18), and K24's T6.1 item.

## V26. Own operations and recovery calls (decided 2026-10-27)

### V26.1 Own operations

The engine's send path (K12.1, K12.4) carries only `MGEntity`, `MGGeneration` and `MGBlob` records and the record deletions that `RunCleanup` marks due (`sync/policy#P10.3`). Decided 2026-10-27 (review round 4): the zone deletions `RunCleanup` marks due are not engine work, since the engine sends a pending database change by itself with no batch request, so no gate, send scope or root confirmation could hold one back; they are `DeleteZones` own operations (`sync/policy#P4`, below). Every other CloudKit write or read that a recovery op or `DeleteZones` needs is an **own operation**: U114 runs it as its own CloudKit operation on the private database, outside the sync engine (it creates no second CKSyncEngine object, V3.4) and never from inside engine event handling (V3.3). One own operation runs per op at a time.

| Own operation | CloudKit operation | Condition | Used by |
|---|---|---|---|
| create `c-<epoch id>`, `meguri-root` and `meguri-media`, always all three | modify record zones | none: creating a zone that exists is not an error (K2) | V15.2 step 3, K18.3 step 1 |
| delete and create again a `meguri-media` whose `root` read gives K2's sign of an unreadable zone (`e` absent, or CloudKit's decryption error for that record), only after an encrypted data reset (the `root` read below) | modify record zones | none | V10.1, V15.2 step 3 |
| delete a cancelled publication's zone that no binding holds a `zones` row for | modify record zones | none | V15.3 |
| save `MGEpoch` with state `publishing`, then with state `complete` and its counts | modify records | `publishing` as a new record; `complete` with the system fields of the acked `publishing` version (the op row's epoch system fields) | K18.3 steps 1 and 4 |
| save `MGMediaRoot` `root` with a new media key, always; after an encrypted data reset only when the `root` read returns unknown item, or K2's sign of an unreadable zone after `meguri-media` was deleted and created again (K2) | modify records | a new record | K9.2, V15.2 step 3 |
| read `root` after an encrypted data reset, after the zone creation and before any `root` save | fetch records by record ID | none | K2, V15.2 step 3 |
| read the op's own `epoch`, right before each attempt of the `MGActive` save | fetch records by record ID | none | K18.3 step 5, V15.2 step 4 |
| save `MGActive` naming the new epoch, with K18.3 step 5's values (decided 2026-10-27, review round 4): `epoch_id` the op's epoch, `previous_epoch_id` the op row's active epoch id (absent when none), `activated_at_ms` the device clock when the attempt is built | modify records | the system fields of the version the user confirmed replacing (the op row's active system fields), or a new record when there was none | K18.3 step 5, V15.2 step 4 |
| save a manifest with state `abandoned` | modify records | the system fields of the manifest version V18 step 2 read (the op row's manifest system fields) | V18 step 2 |
| read `MGActive`, the `MGEpoch` of its epoch and of `MGActive.previous_epoch_id`, the `MGEpoch` of a waiting `publish` op's own epoch before "replace it again" reuses its zone (K18.5), or a manifest | fetch records by record ID | none | V13.2 step 2, V14.1, V15.2 steps 1 and 5, V18 step 2 |
| list the database's record zones, only while A20 is taken as false (`sync/cloudkit-records#K23`): before the zone creation of V15.2 step 3, which then creates only the zones the listing lacks, and again when that creation fails because another device created a listed-missing zone meanwhile | fetch all record zones | none | V15.2 step 3, K18.3 step 1, K23 A20 |
| list the database's record zones for the bound binding, only while A21 is taken as false (`sync/cloudkit-records#K23`): once after each `RunCleanup` (`sync/policy#P10.3`) | fetch all record zones | none | K10.3 `zones`, K18.5, K23 A21 |
| `DeleteZones` (decided 2026-10-27, review round 4): read `active` (`MGActive`) by record ID, then delete the `c-` zones that `zone_deletion_check` returns | fetch records by record ID, then modify record zones | the read names the bound epoch, and the policy guard is current at `zone_deletion_check` (`sync/policy#P4`) | `sync/policy#P10.3` item 3, K18.5 |

A surviving unreadable collection zone after an encrypted data reset (V10.1) is not an op operation. Decided 2026-10-27 (review round 4): after the switch, the new binding gives it a `zones` row with deletion due, and `DeleteZones` deletes it (`sync/policy#P4`, `#P10.3`); no update hands it to the engine (K12.4).

- Every save uses the save policy that fails when the server record changed (`ifServerRecordUnchanged`): a new record fails when a record of that name exists, and a save with system fields fails when the server holds a later version (K12.2, A5). Meguri never overwrites a record unconditionally.

Decided 2026-10-27: own operations fall in groups with their own rules: the **op operations** (every write of the table but the cancelled publication's zone deletion and `DeleteZones`, V18 step 2's manifest read, the `root` read after a reset, the pre-switch `epoch` read and A20's zone listing), the **preview reads** (the reads of V13.2 step 2, V14.1 and V15.2 step 1, which come before any op row or, for the `epoch` read before a reuse, before `republish`, V15.2 step 5), the **zone deletion after a cancel** (V15.3), decided 2026-10-27 (review round 3) **A21's zone listing**, which serves the bound binding and no op (below), and decided 2026-10-27 (review round 4) **`DeleteZones`**, which serves the bound binding too (below). The bound binding's own read of `MGActive` is not an own operation; it has its own rule below.

- **Op operations.** Each carries its op id. Its result reaches the core through `recovery_fact` (V26.2). The core accepts it only while a `recovery_ops` row with that id is at the step that made the operation due; it drops any other result and logs it, as V3.4 says for the staging fetcher. An accepted result, and the step it reaches, commit in one durable tracking transaction. K12.3's rows, which are written for entities, never apply to an own operation.
- **Start condition of an op operation.** U114 starts one only when the core reports it due (`RecoveryWriteDue`, V26.2). The core, not U114, decides that it is due. It reports it only while all hold:
  1. iCloud is enabled on this device and sync is not paused (V16; V22 invariant 4: Pause means nothing reaches CloudKit);
  2. no tracking stop is set (`sync/policy#P15.2`), so the result can commit;
  3. the op's binding is usable (K10.2: active, the open collection's, no pending fence, this build's container and environment): the new binding for a `publish`, the bound binding for an `abandon_generation`; and the account status is `available` with that binding's identity, verified in this process (V9.1, `sync/policy#P3` `Account`). So V22 invariant 4 holds for own operations too;
  4. no attempt of the op runs (V2.3, in memory), and the current instant is at or after the row's retry after (V2.3);
  5. for a `publish`, the row's switch conflict is not set (V2.3, V15.2 step 5).

  The lifecycle state does not gate it: an own operation reads and writes only CloudKit and tracking. While a condition fails, the operation stays due and the op waits (V15.3); nothing is sent. U114 also starts nothing when its own account read in this process differs from the op's identity, and reports that attempt as `not_started` (V26.2).
- **Attempts.** The core sets "attempt runs" when it reports the operation due. U114 reports exactly one `recovery_fact` for each reported attempt: its result, its error (a cancellation included) or `not_started`. That fact clears "attempt runs", and so do a process start and an account event that stops the op's binding. A failure with a retry-after writes the row's retry after in the same commit as the failure's status. Before it first reports a `publish` op's `MGActive` save due, the core commits the row's switch sent (V2.3, V15.3).
- **Due again.** An op operation stays due until its result commits. After each input that `sync/policy#P2` counts as an event trigger (a local change, a foreground return, an account change, Sync Now, an engine start, a drained fetch of the collection zone; decided 2026-10-27, review round 4: a `FetchDrained{root}` is not one, `sync/policy#P2`, P10.1), the core reports every due operation again for which the start condition now holds. It also does so after V2.4 step 7 verifies the account at a start, at `resume_recovery_op`, when a pause or a tracking stop clears, when the publication fence clears (V15.2 step 2: the commit that records `bound` comes before the fence clears, so its report found the binding not usable), and at each `Network{reachable: true}` that follows `false` (decided 2026-10-27: an own operation gets no engine retry, and with nothing pending no engine start or drained fetch may follow). Each of these is one report per transition, so none can loop. The policy receives all these inputs, so U114 needs no knowledge of local changes. No timer runs (U113): a retry after that passes with no trigger waits for the next one.
- **Preview reads.** U114 starts them when U161 shows the decision or preview that needs them, only while the account status is `available` with an identity read in this process (V13.1) and sync is not paused. They carry no op id. Their result reaches the core through `root_facts` (V26.2), with the account identity they were read for; the core drops root facts whose identity is not the one verified now. A failed preview read commits nothing: the decision shows the error with Try Again (V21).
- **Zone deletion after a cancel** (V15.3). The cancel commit removes the op row and the new binding, so no row can make this deletion due. It exists only for an op whose switch sent was false (V15.3, "Switch sent"). When V15.3 gives it to U114, the core reports `RecoveryWriteDue{op_id, zone deletion, zone, account}` once, after that commit. U114 runs it once, when conditions 1 and 2 hold and the identity verified in this process is the named account; it is not reported again. Its result is logged only and never committed (it calls no `recovery_fact`). A deletion that did not run, after a relaunch included, is left to K18.5's 30 days.
- **A21's zone listing** (decided 2026-10-27, review round 3; only while A21 is taken as false). The core reports it due once after each `RunCleanup` (at most once per 24 h, `sync/policy#P10.3`), with the guard of `sync/policy#P4`. U114 runs it once, when conditions 1 to 3 above hold for the bound binding. Its result reaches the core through `zone_listing(guard, result)` (V26.2), which a stale guard refuses with `StaleGuard`, as `sync/policy#P4`'s result calls are. An accepted listing commits, in one tracking transaction, a `zones` row with first seen at now for each `c-` zone other than the bound one that the binding has no row for (K10.3); it marks nothing due and deletes nothing, since the deletions stay `RunCleanup`'s (K18.5). A failed listing commits nothing and waits for the next `RunCleanup`; no timer runs.
- **`DeleteZones`** (decided 2026-10-27, review round 4; `sync/policy#P4`, `#P10.3` item 3). The policy emits it at a `FetchDrained{collection}` of an instance that has confirmed the bound epoch, with the guard of `sync/policy#P4`, and U114 runs it when conditions 1 to 3 above hold for the bound binding. Its steps, its results through `zone_deletion_check` and `zone_deletion_result`, and their effects on the `zones` rows are `sync/policy#P4` and P10.3's. It carries no op id and calls no `recovery_fact`; a stale guard deletes nothing. So the zone of a cancelled publication that the binding before holds a `zones` row for, a never-active zone, the previous epoch's zone and a surviving unreadable zone after a reset are all deleted this way, never by the engine.
- **The bound binding's `MGActive` read** (K2 "Confirmed root", K12.5, `sync/policy#P10.1`). This record-ID read is made for the current engine instance of a usable binding, not for a recovery op, in three uses: when a drained root fetch cannot confirm because the binding records no last applied `MGActive`; before K12.5's plain `zoneNotFound` row after a no-state point (`sync/policy#P3` `FetchDrained`, the sub-bullet below); and, decided 2026-10-27 (review round 3), when the bound collection zone is lost (K12.5's row for a fetched reason `deleted` or a plain `zoneNotFound`, "The `MGActive` read after a lost collection zone"). The core reports it due as `RootReadDue{instance}`. Decided 2026-10-27 (K2, K12.1 rule 3): it is never made for the binding of a `publish` op whose step is before `done`, which needs no confirmed root and is never isolated `active_missing` or `epoch_switched` before its switch: the core reports no `RootReadDue` for its instance, and U114 makes no no-state read (below) for it. The read carries the instance id, and its result reaches the core through `root_read` (V26.2), accepted only from the current instance (V3.4). While a read of an instance runs, this one or the no-state read below, U114 makes no `fetchChanges` call of that instance (V10.2, "Which value the mark records"). An `MGActive` read is recorded as the binding's last applied `MGActive` (K10.3) in one tracking commit; then it confirms the epoch (V10.2's confirmation mark included) or isolates `epoch_switched` (K18.4). Unknown item or zone not found is K12.5's `meguri-root` row (`active_missing`). Every isolation reads the account status and identity again first (V2.2, K12.5). Any other result confirms nothing; the read is made again at the instance's next drained root fetch. A confirmation by this read reaches the policy as `RootConfirmed{instance}` (`sync/policy#P3`), which runs P10.1's `RootConfirmed` row, so that row's `UpdatePending{full}` and send follow it. Decided 2026-10-27 (review round 3): every confirmation of the bound epoch for an instance is reported as `RootConfirmed`, once per instance, whether a drained root fetch or this read made it, except the `done` commit's (the "saved" row below), which the `Binding` input of step `done` reports (`sync/policy#P10.1`).
  - Decided 2026-10-27 (review round 3, K12.5): after a lost collection zone the outcomes differ, since the zone is gone whatever `MGActive` says. An `MGActive` naming another epoch is recorded as the binding's last applied `MGActive` and isolates `epoch_switched`; unknown item or zone not found is K12.5's `meguri-root` row (`active_missing`). An `MGActive` naming the bound epoch, a read that fails (offline, any error), and the end of the instance before its result is accepted isolate `zone_missing`: none of them is a confirmation, and none reports `RootConfirmed`.
  - Decided 2026-10-27: the same channel carries the no-state check of `sync/policy#P3` (V10.2, `sync/cloudkit-records#K12.5`). When the first `fetchChanges` call after a no-state point (the creation of an instance with no saved state, or, decided 2026-10-27 in review round 3, an engine account event of the current instance, `signIn` with the binding's identity included, V9.1, A12) did not fetch a zone of its scope, and no fetch of the instance since that point fetched it, U114 itself reads that zone's record by record ID with the instance id, with no `RootReadDue` (only U114 sees which zones the call fetched): `active` for `meguri-root`, `epoch` for the bound collection zone. The result reaches the core through `root_read`. For `active` it is handled as above. For `epoch` (decided 2026-10-27, review round 4, as `sync/policy#P3`): zone not found, and also unknown item (the zone exists and holds no `MGEpoch`, as at the pre-switch `epoch` read), is K12.5's plain `zoneNotFound` row for the bound collection zone, which reads `MGActive` again; a found `MGEpoch` means the inference failed and the zone exists: it isolates nothing and is logged; any other result isolates nothing and is logged. A read decides when it returns a record, unknown item or zone not found. When it returns the zone's record (`MGActive` or `MGEpoch`), U114 emits `FetchDrained{zone, fetched: false}` for the call that did not fetch the zone, once the core has handled that `root_read` (`sync/policy#P3`): a `fetched: false` drain completes no absence pass, and a root drain with `fetched` false confirms from the `MGActive` the read just recorded. Until a read decides or the zone drains with `fetched` true, U114 makes the same read again at each later `fetchChanges` call of that instance that counts as a read (with both events, `sync/policy#P3`) and did not fetch the zone (decided 2026-10-27, as `sync/policy#P3`).

| Result | Outcome |
|---|---|
| saved | the core records the saved system fields (for `MGEpoch` `publishing`, as the op row's epoch system fields; for `root`, as the binding's acked media key and `root` acked flag, K10.3 `bindings`) and, when it was the last write of its step, moves the op to its next step (V15.2, V18; step `bound` has several writes, V15.2 "Progress within step `bound`"). For `MGActive` that commit is the one that records step `done` (K2): in the same transaction it sets the binding's last applied `MGActive` (epoch id and the saved system fields, K10.3 `bindings`) and confirms the bound epoch for the current engine instance, recording V10.2's confirmation mark; the `Binding` input of that step then asks for the send (`sync/policy#P10.1`) |
| server record changed, and the server record is the one this op writes: the `MGEpoch` of this op's epoch with the same state, `MGActive` naming this op's epoch, a manifest already `abandoned` | as saved, with the server record's system fields: an earlier attempt landed and its result was lost |
| server record changed, `root` | K9.2: the binding takes the server's key and discards its own, and records it as acked, as saved. Decided 2026-10-27: when this build cannot read that `root` (a newer `format`, a decode error, or K2's sign of an unreadable value: `e` absent, or CloudKit's decryption error for that record), the row "any other error" applies: the op waits with needs attention and Copy Debug Info, and nothing is deleted. Only the `root` read after an encrypted data reset (below) may lead to deleting `meguri-media` |
| the pre-switch read of the op's own `epoch` (K18.3 step 5) returns this epoch's `MGEpoch` with state `complete` | the `MGActive` save becomes due (after the commit of switch sent, "Attempts" below). Any other state is handled as "any other error" |
| the `root` read after an encrypted data reset (K2) returns a `root` whose `e` this build reads and decodes | the binding takes its media key and records it as acked, as saved (K9.2); no `root` save is made and nothing is deleted. Decided 2026-10-27 (review round 4, K2): only CloudKit's own sign that the reset left the zone unreadable, a `root` returned with its encrypted field `e` absent or a read that fails for that record with CloudKit's error that an encrypted value cannot be decrypted (A18 false), makes the op delete and create `meguri-media` again and save `root` as a new record (V10.1). A present `e` that fails Meguri's decode, or that decodes to a newer `format`, is the row "any other error", and nothing is deleted |
| server record changed, `MGActive` naming another epoch, or created by another device first | V15.2 step 5; `EpochChanged`. The commit sets the op row's switch conflict (V2.3) to that version's change tag; from then no op operation of the op is due, and "Due again" reports none, until `republish` or a Cancel (V26.2) |
| server record changed, a manifest not `abandoned` | V18 step 2 runs again |
| zone not found or user deleted zone | by zone and cause, as K12.5 maps them for the binding the op writes for, with one exception: a plain zone not found for `meguri-root` at a `publish` op's `MGActive` save (an app deleted the root zone; no purge and no reset) makes the op create `meguri-root` again (K2 allows its publisher this until the switch) and save `MGActive` as a new record, since no version is left to replace. When K12.5 isolates the binding, the op stops at its step and waits with Cancel (V15.3); acknowledgements in a lost zone are never trusted again. An `abandon_generation` op ends with nothing abandoned. Decided 2026-10-27: on the pre-switch `epoch` read (K18.3 step 5), zone not found, and also unknown item (the zone holds no `epoch`), is this row for the bound collection zone, by its cause (a plain zone not found for unknown item); the `MGActive` save is not made |
| network failure or unavailable, rate limited, zone busy, service unavailable, not authenticated, account temporarily unavailable | the op stays at its step, with V10.3's status, and the commit records a reported retry-after as the row's retry after (V2.3). The core reports the operation due again at the next event trigger at or after that instant ("Due again" above). No timer runs (U113) |
| quota exceeded | as the row above, with V15.3's quota reason |
| unknown item, on a read | the record does not exist: no `MGActive` (V13.3), or a manifest gone, which ends an `abandon_generation` op with nothing abandoned. For the `root` read after a reset, the new-record `root` save becomes due (K2, K9.2). For the pre-switch `epoch` read, the zone-not-found row above |
| any other error | the op stays at its step: needs attention, a recovery op waiting (V11), with Resume, which runs the operation again, and Cancel where V14.3, V15.3 or V18 allow it. Copy Debug Info names the error |

### V26.2 Recovery calls

Core calls for U161 and U114. Each runs off the main thread, commits before it returns, and needs no network: U114 runs the network work as own operations (V26.1) or as the staging fetcher (V3.4). Errors are V21's.

| Call | Arguments | Result |
|---|---|---|
| `recovery_state()` | none | the open recovery decisions (V1), each with the choices it offers now (V12.2, V13.3, V13.5, V14.1, V15.1, V18, V19) and their previews (V13.4, V14.2 step 1, V15.2 step 1), from tracking, the staged collection and the last root facts; every `recovery_ops` row of the running build's container and environment (V2.3) with its kind, step and what it waits for, and for a `publish` row its switch sent flag (V2.3), which `sync/status-strings#T13.5` uses to pick its cancel message (decided 2026-10-27) |
| `root_facts(account, active, epochs)` | the result of a preview read (V26.1, "Preview reads"): the account identity it was read for; `MGActive` with its change tag and encoded system fields, or none; the `MGEpoch` records it names; for the `epoch` read before a reuse (K18.5), the waiting op's own `MGEpoch`, or none when the read returned unknown item or zone not found, in which case "replace it again" starts a new epoch | none. The core keeps them in memory for previews and for the checks of the calls below; it drops them when `account` is not the identity verified now |
| `root_read(instance_id, record, result)` | the result of the bound binding's record-ID read (V26.1): `record` is `active` or `epoch`; `result` is the record with its system fields, unknown item, zone not found, or an error with its CloudKit code | none. The core commits it as V26.1 says, or drops it when the instance is not current (V3.4) |
| `start_prefetch(epoch_id)` | the epoch the last root facts name | the op id of the `prefetch` row (V13.2 step 3); `NotOffered` |
| `adopt(source, epoch_id, active_change_tag)` | `active` or `previous`; the epoch and the `MGActive` change tag that the confirmed preview showed | the op id (V14.2 step 3); once the checks below pass, a `publish` op whose switch conflict is set is first cancelled as V15.3 says (V15.2 step 5), in the same call, so a refused `adopt` leaves it waiting. `SessionActive`, `Busy`, `AccountUnavailable`, `EpochIncomplete`, `EpochChanged` (the last root facts differ from the arguments), `NotOffered` |
| `publish(reason, active_change_tag)` | `MGEpoch.reason`; the `MGActive` change tag that the confirmed preview showed, or none | the op id (V15.2 step 2); the op row takes that version's encoded system fields and the epoch id it names from the last root facts (V2.3). `Busy`, `AccountUnavailable`, `EpochChanged` (the last root facts hold no `MGActive` with that change tag, or hold one when the argument is none), `NotOffered` |
| `resume_binding(binding_id)` | an isolated binding | V9.2's commit; `NotOffered` when a V9.2 condition fails; `AccountMismatch` |
| `resume_recovery_op(op_id)` | a waiting op | the op continues from its recorded step (V15.3, V26.1). For a `publish` op whose new binding V9.1 isolated as `signed_out` or `account_changed`, it first makes that binding active in V15.3's commit ("Resume after an account event"). `AccountMismatch` when the verified identity is not the op binding's; `NotOffered` when a V15.3 condition fails or the switch conflict is set |
| `republish(op_id, active_change_tag)` | a `publish` op whose switch conflict is set; the `MGActive` change tag that the new step 1 preview showed | V15.2 step 5's "Replace it again": the reuse commit, or the cancel and a new `publish`, whose op id it returns. `AccountUnavailable`, `EpochChanged` (the last root facts hold no `MGActive` with that change tag), `NotOffered` (no switch conflict) |
| `cancel_recovery_op(op_id)` | an op | V13.2's deletion for a `prefetch`, V14.3 for an `adopt`, V15.3 for a `publish`; `NotOffered` once V14.2 step 7's exclusive phase has started, and for a `publish` while an attempt of its `MGActive` save runs (V15.3) |
| `abandon_generation(generation_id)` | a `waiting` generation that V18 offers | the op id (V18 step 1); `SessionActive`, `NotOffered` |
| `keep_local_only()` | none | first cancels a `publish` op whose switch conflict is set, as V15.3 says (V15.2 step 5); then the sidecar decision becomes local only (V12.2, V13.5, V16 last row); a pending fence clears (V12.2) |
| `stop_using_icloud()` | none | V16: the binding is isolated `stopped` |
| `zone_listing(guard, result)` | the result of A21's zone listing (V26.1), with the guard of `sync/policy#P4` that the core reported it due with: the record zone names, or an error with its CloudKit code | none. The core commits it as V26.1 says, or fails with `StaleGuard` and changes nothing when the guard is not current |
| `recovery_fact(op_id, fact)` | a result of an own operation (V26.1) or of the staging fetcher (V3.4): a fetched batch, a drained fetch, a save result (with the server record when it changed), a zone result, a zone listing (A20's, V26.1), an error with its CloudKit code and retry-after, or `not_started` (V26.1, "Attempts") | none. The core commits it, or drops it when it is stale |

- After each commit that makes an op operation due, and each time V26.1's "Due again" applies, the core reports `RecoveryWriteDue{op_id, operation}` to U114 when V26.1's start condition holds, beside the `Binding` and `TrackingChanged` inputs that `sync/policy#P3` lists. The zone deletion after a cancel and `RootReadDue{instance}` are reported as V26.1 says. The `publish` step `sent` (V15.2, "K18.3 step 3 is done") is found by the core in a tracking commit of the new binding, so no call reports it.
- `Cancelled` (V21) is the result of a call that was running when the user cancelled its op.
- Decided 2026-10-27 (review round 3): U114's media reads and downloads are not recovery calls. Their results reach the core through `sync/policy#P4`'s result calls (`media_root_read`, `blob_reads`, `asset_entry_read`, `download_result`), each with the guard of its action. A call whose guard is not current fails with `StaleGuard` and changes nothing (V3.4), so a read started before an account change, a binding change, a replacement or a close records no media key, sets no `media_zone_missing` and no blob server state, and installs nothing. The bound binding's record-ID reads of `active` and `epoch` carry the instance id instead (`root_read` above), and A21's zone listing carries a guard as these do (`zone_listing` above).

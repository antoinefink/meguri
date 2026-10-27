# CloudKit sync policy

Spec: sync/policy
Requirements: R88, R89, R122, R123, R124, R126, R127, R726, R1549, R1550, R1551, R1552, R1553, R1554, R1555, R1556, R1557, R1558, R1559, R1560, R1561, R1562, R2046, R2047, R2048, R2049, R2050, R2059; supports R365, R1547, R1548, R1583 (CloudKit media progress: P14.4), R1584 (visible media first: P13.2; the placeholder is `sync/cloudkit-records#K9.3`), R1588 and R1844 (bounded, resumable media transfer: P13.2, P13.3, with `sync/cloudkit-recovery#V6`), R1592, R2051, R2052, R2053, R2054, R2055, R2056, R2057, R2060, R2066
Anki: none (Meguri design)
Scenarios: none (U113 table and property tests, U114 and U151 fake-cloud tests: P21)
Status: Under review (R2066). Do not implement U113 or U114 until approved.

When Meguri sends and fetches CloudKit records, which local changes may be sent, how an open review session holds back its newest answer, what Sync Now does, how lifecycle events become cloud work, how status and errors are computed, and how manual Anki transfers stay apart. U113 implements the policy in Rust; U151's `SyncCoordinator` executes its actions through U114's CloudKit adapter.

Records, tracking, acknowledgement, merges and epochs are `sync/cloudkit-records` (cited as K…). Crash windows, account changes, conflict copies, publication and replacement choices are `sync/cloudkit-recovery` (cited as V…). The Anki sync specs (`sync/normal-sync`, `sync/hold-back`, `sync/meta-status`, `sync/errors`, `sync/one-way`, `sync/media-sync`) describe manual Anki transfers only (KTD26). Nothing here reads their USN, `col.ls`, `col.scm`, `col.mod` or `meta` state.

## P1. Terms

- **Policy**: the state machine of this file. It takes inputs (P3) and returns actions (P4).
- **Gate**: the conditions under which cloud work may run against a binding (P6).
- **Barrier**: an op on the writer that runs between two ops and writes no collection row (P7.1).
- **Held run** (R): taken at a barrier for the active session. It is the contiguous run of steps at the top of the undo history that are neither uploaded nor upload-pending, down to and including the newest answer of that session among them. Steps of no session above that answer belong to R (R88: "dependent top-of-stack changes"). R is empty when no session is active or no such answer is reached. R is `engine/undo#M8.1`'s `cloud_held_run(session)`; when every step of the run is the session's, it equals M8's `held_back_run(session)`.
- **Detached hold**: the steps of held runs that an incoming change dropped from the undo history during the active session (P8.3). They are no longer undoable, and their rows stay held until the session ends.
- **Rows in R**: the rows of the journals of R's steps and of the detached hold's steps (M8's value). The notes that M1's touched set adds for changed cards are not rows in R.
- **Incoming-held set**: the entities that a `CloudIncoming` or `CloudRekey` op wrote during the active session while one of their rows was a row in R (the detached hold's included), and the new key of such an entity that a `CloudRekey` moved (P8.3). They stay deferred until `SessionEnded`, whatever later barriers find (P7.2).
- **Eligible image**: the image of an entity the policy allows to be sent (P7.2). An entity is **held** when its eligible image differs from its live rows or it is deferred because of R, the detached hold or the incoming-held set.
- **The binding**: K10.2's binding of the running build, the newest binding of its container and environment; a salvage binding is never it (No binding, below).
- **Image seq**: the collection seq an image represents: S for an image taken from the snapshot, and the seq just before the oldest step that changed one of its rows for a before-image (P7.2).
- **Cloud flush**: a request that the engine sends the collection zone now. Durability flushes of the collection file are "WAL flush" (`storage/open-and-connections#C9.2`); "flushed" has K1's meaning.
- **Exchange**: one Sync Now run (P12).
- **Device settings**: the device-local settings iCloud enabled, Pause (V16, R2046), Sync automatically (R123, "auto" below) and Sync audio and images (R122, "media" below), held in U151's `DeviceSettings` (V2.1).
- **Tracking summary**: the counts that the core computes from tracking after each tracking commit (P3 `TrackingChanged`).
- **Policy token**: a counter in the policy state (P4). It is not an epoch (K1).
- **No binding** (`sync/cloudkit-recovery#V11`): no binding in any container or environment other than a V8.1 salvage binding (reason `tracking_damaged`). A salvage binding only holds conflict copies (V17.2). It is never the binding of a P6, P12.1, P14 or P17 row, and it adds no `isolated` item.
- **Local change**: a tracking commit that holds dirty marks (K11.1) made for a local op, that is an op other than `CloudIncoming` or `CloudRekey`.
- **Parked**: an entity, a blob, a due control record (K12.6) or a due deletion (P10.3) whose last result from the current engine instance was an error after which the engine drops the name from its pending list (K12.4): `quotaExceeded`, an asset error (`pa`, `ma`), a `limitExceeded` for a batch of one record (K12.3), or any other error that K12.4 does not list as kept. A name that K12.4 adds back at once (`batchRequestFailed`, a `limitExceeded` of a larger batch, a `serverRecordChanged` result that left it sendable) and an unknown item (K17.4) are not parked. The core holds the parked marks in memory for the current engine instance; they are not tracking rows and a relaunch drops them. Only `UpdatePending{full}` hands a parked name back (P4).

## P2. Layering

- The policy runs in the core, in `core/crates/meguri-sync/src/cloud/policy/`. Its decision function is pure: `step(state, input) → (state, actions)`. It reads only its state (P5) and the input. The current instant is a field of every input. The policy reads no clock, no tracking row and no collection row, and holds no timer. What it needs from tracking reaches it as the tracking summary.
- The CloudKit policy is separate from the manual-transfer storage under `meguri-sync/src/policy/` (frozen, P16). They share no state.
- Session state comes from U43's session registry inside the core (`sched/review-session#C17`: `SessionStarted`, `SessionEnded`). No FFI call can supply or override it.
- The Swift CloudKit actor (U114) owns `CKSyncEngine`, Apple network operations, retry and backoff. The policy never sets a delivery deadline or a CloudKit retry time.
- App timers: U151 holds the only app timer, the flush timer for the foreground cloud flush target (P10.2). Decided 2026-10-27 (plan U113: "No Rust policy pretends it can enforce a delivery interval or CloudKit retry timer"): Meguri holds no retry timer and no quota pause, in the core or in U114. CKSyncEngine owns scheduling and backoff. After a quota or tracking failure the policy acts again only at event triggers: a local change, a foreground return, an account change, Sync Now, or a state change the engine reports (P15.1, P15.2; `sync/cloudkit-recovery#V3.3`, `#V10.3`). Decided 2026-10-27: the state changes the engine reports that count as triggers are an engine start (`StartEngine`'s update), a drained fetch (`FetchDrained`) and an engine account event (`Account`). An `EngineError` or an `EngineActivity` only recomputes the status, so a failure never hands its own names back at once and cannot start a send loop (P4 `UpdatePending`).
- Deadlines are not timers. Decided 2026-10-27: `SyncCoordinator` holds the deadline of a running operation: the 30 s gate wait of an exchange (P12.3 step 1) and the `until` of a background or quit flush (P11.2, P11.4). The core's waits for a covered seq are deadlines of one operation too: the builder's wait of at most 1 s for the WAL flush to cover S (P7.1) and the tracking update's wait for the seq of an apply (K13.2, K14.2), both `storage/open-and-connections#C9.2`'s wait with a timeout. A deadline only ends the operation it belongs to and ends with it; it never starts, repeats or delays cloud work. There is no other timed wait: an exchange never waits out a retry-after (P12.5), and a retry of a record read (an inbox entry with reason `asset`, which counts its attempts under K13.1, or a blob read, P13.2) runs only at an event trigger. Nothing polls (P3 `VolumeSpaceAvailable`).
- `SyncCoordinator` (U151) executes each action, then reports the outcome as an input. It calls `sendChanges` and `fetchChanges` only from its own tasks, never from inside the engine's event handling (R2048, KTD25).

## P3. Inputs

| Input | Source | Carries |
|---|---|---|
| `Lifecycle{state, generation}` | `engine/lifecycle` L1 events | `Ready`, `Opening`, `Upgrading`, `ExclusiveBusy`, `Failed`, `NoCollection` |
| `CollectionReplaced{generation}` | `engine/lifecycle` L8 | |
| `SessionStarted{session}`, `SessionEnded{session}` | U43, in the core | |
| `TrackingChanged{summary, cause}` | the core, after each tracking commit: dirty marks (K11.1), a scan's start and end (K11.4), a barrier's result, a save result or acknowledgement, an inbox insert, an apply, a blob read result (K12.3, K13.1, K13.2, P13.2), a tracking probe (P4 `ProbeTracking`) | the tracking summary below; `cause` is `local` for a local change (P1), `scan` for a scan's start or end, `probe` for a probe's commit, and `cloud` for every other commit. A commit that holds a local change is `local` |
| `TrackingWriteFailed{noSpace}` | the core, when a durable tracking or staging commit fails (V3.3), a tracking probe included | true for SQLite full or `ENOSPC`; false for an I/O error, a constraint error or a panic |
| `Binding{binding, state, reason}` | the core's tracking, after every K10.2 or V2.2 transition and every recovery op step that creates, isolates, resumes or deletes a binding (V9.2, V13 to V16) | the binding (P1) with `active` or `isolated` and the V2.2 reason. With no binding of the running build's container and environment but one of another (a salvage binding does not count, P1): no binding, with the derived reason `environment_changed` (V2.2, V9.1). None when no binding exists |
| `PublicationFence{state}` | `engine/collection-replace#R6.1`, `sync/cloudkit-recovery` | P17 |
| `Account{status, identity}` | U114: account status, and the engine's account-change events (V9.1) | Apple's account statuses `available`, `noAccount`, `restricted`, `temporarilyUnavailable`, `couldNotDetermine`; `configuration`, which U114 derives (below); changes `signIn`, `signOut`, `switch` |
| `Network{reachable}` | U114 | |
| `EngineStarted{instance}`, `EngineStopped{instance}` | U114, when an engine instance (V1) is created or dropped | V3.4's instance id |
| `EngineActivity{zone, kind, phase, outcome?, saved?}` | U114 | kind `send` or `fetch` for an engine operation, `read` for a round of record-ID reads (P13.2, P4 `RetryAssetEntries`); phase `started`, `ended`. At `ended` only: `outcome` `ok` or `failed`, and for a send `saved`, the number of records with a Saved result (K12.3 first two rows) in that send. `failed` when the operation failed or a record of it got a result that P15 maps to an error class; the per-record outcomes K12.3 resolves (server record changed, unknown item, batch request failed, limit exceeded, asset errors) leave it `ok`. U114 reports every `EngineError` of an operation before that operation's `ended` (decided 2026-10-27) |
| `EngineError{class, code, retryAfter?, scope}` | U114, mapped by P15 | `code` is the CloudKit error code, logged, and read by P15's `invalidArguments` row; `scope` is `{zone, kind, recordName?}`: kind `send` or `fetch` for an engine operation, `read` for a record-ID read (P13.2, P4 `RetryAssetEntries`); `recordName` only for a per-record outcome. Failing cycles count per (zone, kind) (P15) |
| `FetchDrained{zone, fetched}` | U114 (A7), after the core has handled every event of that fetch; when, below | `root` or `collection`; `fetched` true when the engine fetched changes of that zone in this fetch, false when it had none to fetch |
| `VolumeSpaceAvailable` | U151, read at existing triggers only (below) | |
| `DeviceSettings{enabled, paused, automatic, media}` | U151, at open and on change | |
| `Foreground` | U151's foreground-return sequence (P11.1) | |
| `Background{deadline}` | U151's background sequence (P11.2) | the instant the OS background time ends |
| `BackgroundExpiring` | U154's expiry handler | |
| `QuitRequested{}` | U151, Mac, after the session was ended | |
| `FlushTimerFired` | U151's flush timer | |
| `SyncNowRequested{source}` | Sync Now command, `Y` key (R1551), AnkiConnect `sync` (U146) | `command`, `key`, `ankiConnect` |
| `SyncNowCancelled{exchange}` | user | |
| `ExchangeEnded{exchange, result}` | `SyncCoordinator`, after P12.3 step 6, or when the exchange stops early | P12.6; every one emits `RecordExchange` (P12.7) |
| `VisibleMedia{names}` | U151, from the review head and open screens | P13.2 |

Removed (2026-10-27): the inputs `EngineRetryDue` and `QuotaRetryDue`. No timer reports a retry time (P2).

- `VolumeSpaceAvailable` (decided 2026-10-27): Apple platforms give no notification that free space came back, and Meguri never polls. U151 reads the free space of the volume that holds the collection, the tracking file and the cloud staging directory only just before it reports `Foreground`, `SyncNowRequested` or `FetchDrained`. When that free space is at or above P13.2's 512 MiB reserve, U151 reports `VolumeSpaceAvailable` first, then the input it was about to report. The policy acts on it only during a media download pause for free space (P13.2) or a tracking stop (P15.2); otherwise it returns no action.
- `configuration` (decided 2026-10-27) is not one of Apple's account statuses. U114 reports `Account{status: configuration}` when the account status read itself fails, or an engine operation or a record-ID read fails, with `missingEntitlement`, `badContainer`, `permissionFailure`, `managedAccountRestricted` or `badDatabase`. It holds until U114's next account status read (at launch, at a foreground return or at an engine account-change event, V9.1) succeeds with another status, so a setup that stays broken is tried again only at those event triggers, never in a loop.
- `FetchDrained` (decided 2026-10-27). The engine fetches a zone's changes only when the database changes report that zone changed, and reports no event for a zone it had nothing to fetch for. So U114 emits `FetchDrained{zone, fetched}` as follows, always after the core has handled every event of that fetch:
  - when a `fetchChanges` call of `RequestFetch` (P4, Sync Now steps 3 and 5) returns with no error: one input for each zone of the call's scope, root first, with `fetched` true when the engine fetched that zone and its zone fetch ended with no error, false when it had no changes to fetch for it;
  - when a fetch that the engine scheduled itself ends: one input with `fetched` true for each zone whose zone fetch ended with no error. U114 cannot tell that such a fetch read the database changes without error, so it emits no `fetched: false` input for it;
  - never for a zone whose zone fetch ended with an error.

  Exception, for data safety: an engine instance created with no saved state reads the database changes from no cursor, which lists every zone that exists (A7; the signed runs check it, P21). So when the first `fetchChanges` call of such an instance whose scope names a zone returns with no error and did not fetch that zone, and no earlier fetch of the instance fetched it, the zone does not exist. U114 emits no `FetchDrained` for it and reports it not found; K12.5's plain `zoneNotFound` row applies, after `MGActive` is read by record ID (P10.1, "Confirmed root"). This holds until that instance's first `FetchDrained` of the zone with `fetched` true. The binding of a `publish` op whose step is before `done` follows V15.2 for its own zone. Decided 2026-10-27: an absence pass (K17.4) completes only at a `FetchDrained{collection, fetched: true}`. A drain that fetched nothing proves no record absent; a pass that waits so completes at the next drain that fetches the zone.
- `Account` (decided 2026-10-27): an engine account-change event `signIn` whose user record id equals the binding's verified identity is not an account change. It isolates nothing, does not increase the policy token (P4) and is logged; like every engine account event it is an event trigger, so with the gate open it emits `UpdatePending{full}` and nothing else (P10.1). An engine instance created with no saved state (V9.2 resume, `sync/cloudkit-recovery#V14.2` step 9, a lost cursor under V3.1 rule 5) may report such a `signIn` at once. A `signIn` with another id is an account switch (P15, V9.1 `account_changed`). This rests on A12 and is confirmed in the signed multi-device runs (P21).

The tracking summary holds, for the active binding:

- `sendable`: collection entities that are pending, not in flight, not blocked, not named by an intent in progress, not parked (P1) and not deferred (P7.2). An entity whose deferral a full update cleared, or that was dirtied after the barrier that decided it, counts until the next barrier decides it.
- `parked`: parked collection entities, blobs and due deletions (P1). The next `UpdatePending{full}` hands them back, so a condition "sendable" that follows a full update in the same action list counts them too (P10.1).
- `held`: entities currently deferred or last built from a before-image (P7.2).
- `inFlight`, and `blocked` with its K12.1 cause (rule 1 flags, rule 5 waiting references, rule 7, rule 8).
- inbox entries: `ready`, and waiting entries per K10.3 reason.
- `applyInProgress`: an apply or re-key intent whose op has not yet been settled by the WAL flush (K13.2, K14.2). `unsettledIntents`: intents left by a crash or a failed run and not yet settled at open.
- `scan`: `none`, `required` or `running` (K11.1, K11.4).
- blobs: uploads and downloads waiting and running, with bytes. Downloads include the live media-name entities marked `file_missing` (K9.4) or `superseded` (`sync/cloudkit-recovery#V14.4`) whose blob is not known to be absent. `blobsUnknown`: blobs in server state `unknown` (K10.3) that a live media-name entity names (P13.2 read round).
- `mediaMissing`: live media-name entities whose content no local file holds (`file_missing`, `superseded`, or applied from the server and not yet installed) and whose blob is absent from the media zone: its `blobs` server state is `deleted` (K10.3), set by a record-ID read that reported unknown item (P13.2), whether the blob was never uploaded or another device deleted it (K9.5).
- `crtMismatch`: the last crt check (P6) found the open collection's `col.crt` different from the binding's epoch `crt`.
- the counts behind P14.4's attention items.
- **Not pending for the policy** (decided 2026-10-27): a media-name entity marked `file_missing` (K9.4), `withheld` (`sync/cloudkit-recovery#V14.2` step 9) or `superseded` (`sync/cloudkit-recovery#V14.4`). K1 would call it pending, because its local state differs from the acked one, but no local change is at risk: a `file_missing` or `superseded` name is a download candidate and never a deletion, and a withheld name is never sent. Such an entity is in no count above except the media downloads and `mediaMissing` (`file_missing` and `superseded` only); it is not in `sendable`, `parked`, `held` or `blocked`, not selected by an exchange (P12.2), never keeps the state at `changesPending` (P14.2 row 9) and never prevents a quiet point (P14.3). A `file_missing` entity still in flight (K9.4: `sent` true, waiting for its save result) counts in `inFlight` until that result arrives.

## P4. Actions

Every action carries a guard: the lifecycle generation, the binding id and the policy token. The token increases on every `StopEngine`, on every account change that changes the account status or identity (P3 `Account`), and on every `Binding` input, replacement and close. It does not increase on `StartEngine` or `ConfigureAutomatic` (decided 2026-10-27): starting an engine or changing its scheduling changes no binding and no account, so a running exchange and the pending update that follows `StartEngine` keep a current guard. Each core call that `SyncCoordinator` makes for an action passes the guard. A call whose guard is not current fails with `StaleGuard` and changes nothing.

Facts from an engine (save results, fetched records, state serializations, account events) also carry V3.4's instance id. The core accepts a fact only from the current instance in P5 (`EngineStarted`). Together, the guard and the instance id make a late callback of a stopped engine, of an engine recreated by `ConfigureAutomatic`, or of another account have no effect (U114 test). An exchange uses the engine instance that is current at each of its steps, not the one current at `RunExchange` (P12.3).

| Action | Executed by | What it does |
|---|---|---|
| `StartEngine{binding, automatic}` | `SyncCoordinator` | only when no engine runs for the binding and the previous instance, if any, has reported `EngineStopped`: creates the engine from the saved engine state (K10.3) with automatic scheduling on or off, reports `EngineStarted`, then runs `UpdatePending{full}` |
| `StopEngine{reason}` | `SyncCoordinator` | the engine has no stop call, so (decided 2026-10-27): after its running handler has persisted, calls the engine's `cancelOperations` and awaits its completion handler, then releases the engine object and reports `EngineStopped`. An operation that completes after the cancel reports facts with the old instance id, which the core refuses (the instance id above, V3.4). Reason `trackingFailed` stops it without saving its state and drops the events still queued (V3.3) |
| `ConfigureAutomatic{on}` | `SyncCoordinator` | turns the engine's automatic scheduling on or off. Automatic scheduling is fixed when the engine is created, so it may recreate the engine from the saved state, with a new instance id: it stops the old instance as `StopEngine` does and creates the new one only after the old one's `EngineStopped`, so two engines never target the private database at once (V3.4). That stop is not a `StopEngine` action and does not change the token. While an exchange runs, `SyncCoordinator` applies it only after that exchange's `ExchangeEnded`, so the exchange is never cut by a new instance (decided 2026-10-27) |
| `UpdatePending{full}` | `SyncCoordinator` | K12.4's full update. It first clears every deferral (P7.2) other than those of the incoming-held set (P8.3) and every parked mark (P1), then adds to the engine's pending changes the record names of every sendable entity, of every blob ready to upload (P13.3) and of every due control record (K12.6: the generation manifests), and the record deletions and zone deletions that `RunCleanup` marked due, and removes every other name. With media off, or with `mediaZoneMissing`, it adds no blob. It runs at the event triggers (P2) and where P9, P10.1, P11, P12.3 and P13.1 name it, never at an `EngineError`. When nothing is parked or deferred and the engine's list was kept by `UpdatePending{changes}` since the last full update, its result equals that list, and the adapter may skip the full comparison |
| `UpdatePending{changes}` | `SyncCoordinator` | the update between full updates (K12.4): adds the names of entities and blobs that became sendable or ready since the previous update, of control records newly due and of deletions newly marked due, and removes the names that stopped being sendable. It never adds a parked or deferred name, and never removes a control record while it is due (K12.6). Emitted at a `TrackingChanged` that is not a local change, so a save result, an acknowledgement or a barrier result never hands back a name that just failed (decided 2026-10-27) |
| `RequestFetch{zones}` | `SyncCoordinator` | calls `fetchChanges` for the zones, in the order given, and awaits each call. The zones are only `root` (`meguri-root`) and `collection` (the bound collection zone): the engine's fetch scope (P13.2) |
| `RequestSend{zones}` | `SyncCoordinator` | calls `sendChanges` for the zones: `collection`, or `media` (`meguri-media`, only from `ResumeMedia`, P13.1) |
| `ReadBlobs{scope}` | `SyncCoordinator` | one round of record-ID reads of blobs, `e` only (P13.2): scope `unknown` reads every blob in `blobsUnknown`; scope `missing` also reads the `deleted` blobs whose live names count in `mediaMissing`. At most 200 record IDs per read operation (decided 2026-10-27, under A1.1's 250 records per request). U114 reports the round as `EngineActivity{media, read}`. No round runs with `mediaZoneMissing` set |
| `RetryAssetEntries` | `SyncCoordinator` | U114 reads by record ID, with every field, each inbox entry with reason `asset` whose retry count is below 5. Each read adds 1 to the count (K13.1); a record whose assets now pass K20 goes through K13.1 steps 1 and 2 again. An entry at 5 stays and sets `assetUnavailable` (P14.4). This is not the first read of a save-conflict entry whose payload is in an asset: U114 makes that read once, right after the event handling of the send returns, with no action of the policy, and it does not count among the 5 (K12.3) |
| `RunCleanup` | core | P10.3 |
| `ArmFlushTimer{at}`, `CancelFlushTimer` | `SyncCoordinator` | holds at most one timer; arming replaces it |
| `RunExchange{exchange}`, `CancelExchange{exchange}` | `SyncCoordinator` | P12 |
| `BackgroundFlush{until}`, `CancelBackgroundFlush` | `SyncCoordinator` | P11.2 |
| `QuitFlush{until}`, `QuitNow` | `SyncCoordinator` | P11.4 |
| `PauseMedia`, `ResumeMedia`, `PrioritizeMedia{names}` | `SyncCoordinator` | P13.1, P13.2 |
| `ApplyReady{retryFailed}` | core | runs the day clock's `checkRollover()`, then submits `CloudIncoming` ops for the ready inbox entries (K13.2). With `retryFailed` it then applies the entries with reason `apply_failed`, one entry per op, after the ready ones (K13.2 "Failed applies"; after five failures, once per collection open). `retryFailed` is set only at an event trigger (P2), never at a `TrackingChanged` (decided 2026-10-27) |
| `RecordExchange{result}` | core | P12.7; also runs `checkRollover()` once (P12.7) |
| `RecordQuietPoint{at}` | core | P14.3 |
| `ProbeTracking` | core | during a tracking stop only: one durable tracking commit that writes the probe row (P19), then `TrackingChanged` when it commits or `TrackingWriteFailed` when it fails (V3.3, P15.2) |
| `StatusChanged{status}` | core | publishes the status (P14) to `SyncStatusModel` |

- Core actions run in the core after `step` returns and before the next input. `SyncCoordinator` executes the others in the order given, and starts each only after the one before it has returned. So a `RequestSend` that follows a `RequestFetch` in one action list starts only after that `fetchChanges` call has returned, which is after the core has handled the fetch's events (A7) and the root fetch has confirmed the epoch (V10.2).
- Decided 2026-10-27 (K12.4, P2): every `RequestSend` in an action list follows an `UpdatePending{full}` in the same list, or in `StartEngine`, except `FlushTimerFired`'s: the local change that armed the timer ran the full update (P10.2), so the timer never hands back a parked name.
- The action set has no action that starts, resumes or schedules a manual Anki transfer (P16).

## P5. Policy state

- In memory: lifecycle state and generation; the active session; device settings; account status and identity; the binding, its state and isolation reason; the fence; reachability; foreground or background; the engine instance that runs, if any; the latest tracking summary; the policy token; the flush timer instant; the running exchange and its capture (P12.2); the running background or quit flush; engine activity and the drained flags per zone; whether a `FetchDrained{root}` has arrived from the current engine instance (reset at `EngineStarted`, P10.1); whether a blob read round runs (P13.2); whether `quotaFull` is set (P15.1); the tracking stop and its kind (P15.2); the last error class and its retry-after; consecutive failing cycles per (zone, kind) (P15); the media download pause for free space (P13.2); whether a local change's `TrackingChanged` has cleared a tracking stop since the last `Foreground`, and a `ProbeTracking` in progress (P15.2); the instant of the last `RunCleanup` (P10.3).
- In memory in the core, beside the policy and not read by `step`: the detached hold, the incoming-held set (P8.3) and the deferrals (P7.2). They are kept with the barrier machinery on the writer and dropped as P8.4 and P9 say.
- Durable, in the tracking store (K10), per binding, written only by core actions: the last completed exchange and the last exchange attempt (P12.7), the last quiet point (P14.3) and the tracking probe (P15.2). P19 lists the rows.
- After a relaunch the in-memory state starts empty: no session, no exchange, no timer, no tracking stop. Pending work comes from tracking (K12.4).

## P6. The gate

Cloud work runs against the active binding only while every row of the first table holds. The second table says what runs when one does not.

| Condition | Source |
|---|---|
| iCloud enabled and not paused on this device | `DeviceSettings`, V16 |
| lifecycle `Ready` | `engine/lifecycle` L1 |
| binding usable: active, publication identity equal, no publication pending, same account and environment | K10.2, P17 |
| account `available` with the binding's identity | P3 `Account` |
| no reconcile scan running or required; no unsettled intent | K11.1, K11.4, K13.2, K14.2 |
| the open collection's `col.crt` equals the binding's epoch `crt` (the crt check below) | K20 |
| the engine is not stopped for a tracking failure | P15.2 |

| Failing condition | Engine | Batch builder (K12.1) | Fetched records | Apply |
|---|---|---|---|---|
| none | running | builds | inbox (K13.1) | yes |
| iCloud not enabled, or paused | not started; stopped if running | | | |
| `Opening`, `Upgrading`, `NoCollection` | not started | | | |
| `ExclusiveBusy` | running | returns no record | inbox | waits |
| `Failed` (any reason, `Corrupt` included) | stopped | | | |
| scan running or required, or an unsettled intent | running | returns no record | inbox | waits |
| `crt` mismatch | running | returns no record | inbox | waits |
| binding not usable | stopped (P17) | | | |
| account not available or not the binding's | stopped | | | |
| tracking failure | stopped (P15.2) | | | |

- An intent in progress (`applyInProgress`: committed, waiting for its WAL flush) does not close the gate. Entities it names are not sendable until it settles (P7.2).
- Auto and media are not gate conditions. With auto off the engine runs with automatic scheduling off: nothing is sent or fetched until Sync Now (P12). Dirty marks and the engine's pending list stay current.
- A replacement's exclusive phase makes the binding unusable once the replacement intent is recorded (`engine/collection-replace#R6.2`), which stops the engine.
- **The crt check** (decided 2026-10-27, K20's last row: "nothing sent or applied; needs attention"). Card due days count from `col.crt`, so a record exchanged between collections with different `crt` values would move every due date. The binding records its epoch's `crt`: from the `MGEpoch` it published or adopted, and then from each fetched `MGEpoch` of the bound epoch, in the K13.1 step 2 transaction that inserts it (K10.3 `bindings`, P23). The core compares the open collection's `col.crt` with that value:
  - at open, with V2.4 step 4; after every committed op that writes the `col` row (the tracking observer, K11.1); after a reconcile scan; and in the tracking transaction that records a fetched `MGEpoch`. These set or clear `crtMismatch` in the summary (P3);
  - at every barrier, from its snapshot, and on the writer before every `CloudIncoming` or `CloudRekey` op. A barrier that finds them different returns no record, and such an op does not run. So nothing is sent or applied while they differ, whatever the summary said last.

  Both values are durable, so a relaunch finds the mismatch again at open, with no flag of its own. While they differ, the gate is closed with the engine running (the row above), and status shows the attention item `crtMismatch` (P14.4). It clears when a check finds them equal again, for example after an undo of the op that changed `col.crt`. Otherwise the user chooses Replace local collection from iCloud (V14), whose staged collection takes `crt` from `MGEpoch` (K18.4), or Replace iCloud collection (V15), which publishes the local `crt`; both are offered from Account & Sync (V14.1, V15.1), and V11's row for this reason is P23's. The binding is not isolated.
- P10.1 says what happens when the gate opens or closes.

## P7. The eligibility frontier

### P7.1 The barrier

Every batch of collection-zone records is built from one barrier, whichever path asked for it: automatic engine scheduling, a cloud flush, an exchange, a background or quit flush, or a retry after a restart. The batch request names the record names the batch may hold. On the writer, before the next op is taken, the barrier:

1. begins a read transaction on the cloud reader and runs one read, fixing the snapshot. The builder leases the cloud reader off the writer and submits the barrier with the lease, so the writer never waits for it (`engine/lifecycle#L11`). The cloud reader is a reader role of its own, so a manual transfer's long snapshot on the sync reader never blocks it (P23);
2. records S, the seq of the last committed op (`engine/op-model` O4);
3. takes R for the active session (P1) and the detached hold (P8.3). This reads no row image;
4. flags undo steps as P8.2 says.

Then, off the writer, the builder waits until the WAL flush covers S (`storage/open-and-connections#C9.2`). If S is not covered within 1 s, the batch returns no record. The entities stay pending, and the next request builds again (K12.1 rule 4). So every image comes from flushed commits. Several batch requests made together may share one barrier.

### P7.2 Eligible images

For each pending entity (K1) named by the request, the builder computes its eligible image E row by row:

| The row at the snapshot | Its image in E |
|---|---|
| not a row in R | the snapshot's row |
| a row in R, with a before-image | the before-image from the oldest step (of R or the detached hold) that changed it |
| a row in R that R or the detached hold created | absent |

| E | Sendable (K12.1 rule 2) |
|---|---|
| every row from the snapshot | when E's row digest differs from the acked row digest. The image seq is S |
| some row from a before-image, or absent | held. When E has a row: sendable when E's row digest differs from the acked row digest, otherwise deferred. The image seq is the seq just before the oldest step that changed one of its rows |
| no row, because R or the detached hold created every row | held and deferred: no record, no tombstone |
| the rows R deleted, from their before-images | held; sent live as row 2, with no tombstone |

- "Deferred" keeps the entity pending and not handed to the engine. K12.4 removes it from the engine's pending changes.
- A deferral holds only until the next `UpdatePending{full}` (decided 2026-10-27), except a deferral of the incoming-held set, which holds until `SessionEnded` (below). A barrier decides only the names its request names, and the engine asks only for names it holds, so a deferred entity would otherwise never be decided again when R moves. R moves only through a local op (a new answer, an undo, an edit after the answer) or the session's end, and each runs a full update (P10.1, P9). The full update clears every deferral and hands the names back; the next barrier decides them with the R of that time: a card answered before the newest answer is then below R and is sent live. A deferral is held in memory by the core, with the barrier that set it; a relaunch drops it, as it drops R (P8.4). An entity whose deferral was cleared counts as `sendable` until a barrier decides it (P3); the cost is one extra barrier decision per full update.
- An entity named by an apply or re-key intent in progress (`applyInProgress`) is not sendable until the intent settles (K13.2, K14.2).
- An entity of the incoming-held set (P1, P8.3) is held and deferred at every barrier until `SessionEnded`, with no record and no tombstone, whatever its image digest and whatever the tables above give (decided 2026-10-27). Its images can no longer be taken from the journals: the incoming change made its live rows and its acked digest newer than every before-image of R or the detached hold, so such an image would differ from the acked one, count as sendable and overwrite the other device's version with a stale one; and after a re-key the journals name only the old key, so the new key's live rows would carry the held step's effect (P20 invariants 2 and 14). The full update does not clear this deferral (P4), so the engine is not asked for the entity; a barrier asked for it anyway defers it again. At `SessionEnded` the set is dropped and the entity is sent live (P9). The op's own local write always uses L = the live row (K13.2, K12.3); a held image is never L.
- An entity whose E references an entity that is deferred is held by K12.1 rule 5. Example: a card answered in R keeps its earlier image, and the review-log row R added is absent, so nothing references it.
- Media-name entities come from the media journal, not from collection rows, so R never holds them (K9.4). A note held by R that references a new media name holds nothing else back; the name is sent alone (K12.1 rule 6 applies only to the note).
- The builder records with each in-flight entry its image seq and whether the image is held (P12.4, P23).

| In the session, before the barrier | Sent from this barrier |
|---|---|
| 30 answers; R = the newest answer | 29 cards, 29 review rows, the deck as after answer 29 |
| answer of #c1, answer of #c2, then a flag on #c2; R = the two newest steps | #c1 and its review row; the deck as after answer #c1 |
| answer of #c1, then Delete Note of its note; R = both | nothing of the note, its cards or the review row |
| answer of #c1, then Add (over the review) of a note; R = both | nothing of #c1's answer, the new note or its cards |
| answer of #c1, then a Browse edit of note #n (Mac, no session); R = both | nothing of #c1's answer or of #n's edit |
| an edit made in Browse before the session, not yet sent | the edit (it is below R) |
| answer of #c1, then an incoming change to #c1 drops R (P8.3) | nothing of #c1; the other rows of the dropped run as their before-images, until the session ends |
| answer of #c1, a Browse edit of note #n, an incoming change to #n (R dropped), then answer of #c2 | nothing of #n until the session ends, although the answer of #c2 ran a full update (#n is in the incoming-held set) |
| no session | every pending entity, live |

## P8. Hold-back during a review session (R2047, R88, R726)

### P8.1 Scope

- While a session is active, every record built for the collection zone follows P7. No path skips the barrier: there is no "send live rows" route, also not for the background, quit or retry paths. A record name that the engine saved in its pending list before a restart is built with the R of the batch time (K12.4).
- Committed changes below R keep syncing during the session (R1550). A change made outside the session after its newest answer, such as a Browse edit on Mac, is part of R until the next answer of the session or the session's end. Only committed changes enter tracking; an open editor draft is not in the collection.
- Hold-back adds no latency to answers: answers commit through the executor and never wait for a barrier (R2006).

### P8.2 Undo steps and hand-off

- At the barrier, on the writer, every step with a seq at or below S that is not in R, is not yet flagged, and whose touched set (`engine/undo` M1) holds a row of an entity the batch request names, becomes `UploadPending`. No undo can run between the snapshot and the flags (M9.1's rule). The flags stay when the batch then returns no record or the save fails. Redo steps at or below S whose touched set holds a row of a named entity are dropped (`engine/undo#M9.6`).
- A flagged step becomes `Uploaded` once every entity of its touched set that the batch named is acked or clean.
- Neither state is undoable (M4.1), whatever the save result: a save whose result is lost may have landed (K12.3).
- R's steps are never flagged, so the newest answer stays undoable (R365). The detached hold's steps are no longer in the history (P8.3).
- For CloudKit this replaces M9.1's "every other step up to the snapshot" with the steps that meet the request, and replaces M9.2 and M9.3 (`finish` sent, failure before `finish`), which stay for manual transfers. `engine/undo#M9.6` and `#M9.7` give the flags and the acknowledgement.

### P8.3 Incoming changes and undo (R2047)

- A `CloudIncoming` or `CloudRekey` op reports its targets: every key its journal changed, a grave mapped to the key of the card, note or deck it deletes (as M9.4 does), `col` left out. When the op commits, on the writer:
  - the history is cut below the newest step whose touched set holds a target: that step and every older step are dropped, as in M7 (`engine/undo#M9.8`). Steps newer than it stay;
  - redo steps are cut the same way: the redo step nearest the top whose touched set holds a target, and every redo step below it (M9.8);
  - R is dropped whole when a target is in the touched set of any of its steps, and the cut then reaches every older step too. M9.4's first rule ("every step up to the snapshot is dropped") does not apply to CloudKit.
- A dropped R joins the detached hold until `SessionEnded`. Its steps are not undoable any more, but the rows of their journals stay rows in R, and the entities the op did not write keep the before-images P7.2 gives. M8's value keeps the journals while the policy holds it.
- Every target of the op that has a row in R or in the detached hold joins the incoming-held set (P1), whether or not this op dropped R, and so does the new key of such a target that a `CloudRekey` moved (decided 2026-10-27). The set stays until `SessionEnded`; P7.2 defers its entities at every barrier, and no full update hands them back. A target with no row in R or the detached hold (for example a note that only M1's touched set adds for a changed card) does not join it.
- The next barrier takes R again from the remaining history. Its rows and the detached hold's together decide P7.2's images; the oldest before-image of a row wins.
- Undo then reports what M3 gives for the remaining history; there is no new reason.

### P8.4 Lifetime

- R, the detached hold and the incoming-held set exist only while the session is active.
- Background with the session open keeps them: suspend keeps the undo history (`engine/suspend` S1), and the background flush sends under P7.
- Close, a crash, an OS termination and `CollectionReplaced` drop the history (`engine/undo` M6), the detached hold and the incoming-held set, and end the session. After a relaunch nothing earlier is held: the lost session's newest answer is sent like any change.
- On Mac quit the session is ended before the quit flush (P11.4, KTD6), so the newest answer is sent if the flush gets the time.

## P9. Release at session end (R1549)

`SessionEnded` (`sched/review-session#C16`) ends hold-back: the core drops the detached hold and the incoming-held set, the next barrier's R is empty, and every pending entity's live image is eligible. The full update below then clears every deferral (P7.2), so each deferred entity is handed back and sent live.

| Gate (P6) | Auto | Actions |
|---|---|---|
| open | on | `CancelFlushTimer`, `UpdatePending{full}`, `RequestSend{collection}` when an entity is sendable |
| open | off | `UpdatePending{full}` |
| closed | any | none |

`startSession` while a session is active ends the old session first (C1): `SessionEnded` then `SessionStarted` arrive in that order, and the row above applies between them.

## P10. Automatic triggers (R123, R1549, R88)

### P10.1 Decision table

"Open" and "closed" refer to the gate (P6). "Sendable" is the summary's `sendable` count above 0; after an `UpdatePending{full}` in the same action list (or in `StartEngine`) it is `sendable` plus `parked` above 0 (P3). Every input also recomputes the status and ends its actions with `StatusChanged` when the status changed (P14), and with `ApplyReady` when the gate is open, no apply is in progress, and a ready inbox entry exists or the input is an event trigger (P2) and an `apply_failed` entry waits. `retryFailed` is set when the input is an event trigger.

| Input | Condition | Actions |
|---|---|---|
| any input after which the gate opens | no engine runs | `StartEngine{automatic: auto}` (which runs `UpdatePending{full}`); when auto: `RequestFetch{root, collection}`, then `RequestSend{collection}` when sendable, and `ResumeMedia` when media is on |
| any input after which the gate opens | an engine runs | `UpdatePending{full}`; when auto: `RequestFetch{root, collection}`, then `RequestSend{collection}` when sendable |
| any input after which the gate closes | the P6 table says stopped | `CancelFlushTimer`, `CancelBackgroundFlush`, `CancelExchange`, `PauseMedia`, `StopEngine` |
| any input after which the gate closes | the P6 table says running | `CancelFlushTimer`. A running exchange waits as P12.3 step 1 says |
| `Lifecycle{Ready}` (launch, reopen, end of an exclusive phase) | the gate was already open | none |
| `CollectionReplaced` | | as a closing gate; the gate then opens only once the replacement's fence is known (P17) |
| `TrackingChanged{cause: local}` | open | `UpdatePending{full}`; when auto, foreground, sendable and no timer armed: `ArmFlushTimer{now + 60 s}`; when auto, media on, `MGMediaRoot` acked, `blobsUnknown` above 0 and no read round runs: `ReadBlobs{unknown}` |
| `TrackingChanged{cause: cloud, probe or scan}` | open | `UpdatePending{changes}` (a scan's start or end closes or opens the gate, and those rows apply). It never arms the flush timer, so a save result, an acknowledgement, a barrier result or a blob read result never starts a send by itself (decided 2026-10-27) |
| `TrackingChanged` | closed, during a tracking stop | P15.2 says whether the stop clears; when it does, as a gate that opens |
| `TrackingChanged` | closed, otherwise | none |
| `TrackingWriteFailed` | | P15.2 |
| `FlushTimerFired` | open, auto, foreground | `RequestSend{collection}` when sendable |
| `SessionStarted` | | none (R starts with the session's first answer) |
| `SessionEnded` | | P9 |
| `DeviceSettings{automatic: on}` | open | `ConfigureAutomatic{on}`, `UpdatePending{full}`, `RequestFetch{root, collection}`, `RequestSend{collection}` when sendable, `ResumeMedia` when media is on |
| `DeviceSettings{automatic: off}` | | `ConfigureAutomatic{off}`, `CancelFlushTimer`. A running exchange continues: the change takes effect after it ends (P4), and the token does not change |
| `DeviceSettings{media}` | | P13.1 |
| `DeviceSettings{enabled, paused}` | | as a gate that opens or closes |
| `Binding` | | the token increases; then as a gate that opens or closes |
| `Account` with a change | | P15; during a tracking stop also P15.2 |
| any `Account` input after which the gate is open and was open before (a `signIn` with the binding's identity, P3) | | `UpdatePending{full}` |
| `Network{reachable}` | | none. The engine keeps the names that failed for the network and retries them itself (K12.4) |
| `PublicationFence` | | P17 |
| `EngineStarted`, `EngineStopped` | | none; P5 holds the instance and resets its root-drained flag |
| `EngineActivity` | | P14.3; at `ended`, the status and the counts of P15 (failing cycles, `quotaFull`, the transient authentication state). No `UpdatePending` |
| `FetchDrained{root}` (either `fetched`) | open, the first from the current engine instance | `UpdatePending{full}`; when auto: `RequestSend{collection}` when sendable; then P14.3. The core refuses every collection batch of an instance until its root fetch has drained and confirmed the bound epoch (V10.2, "Confirmed root" below), so a send that the engine scheduled before that got no record; this row asks again (decided 2026-10-27) |
| `FetchDrained{collection}` (either `fetched`) | open | `RunCleanup` when P10.3 says it is due; `UpdatePending{full}`; `RetryAssetEntries` when an inbox entry with reason `asset` is below 5 attempts; when auto, media on, `MGMediaRoot` acked, `blobsUnknown` above 0 and no read round runs: `ReadBlobs{unknown}`; media downloads may start (P13.1); then P14.3 |
| `FetchDrained` | otherwise | P14.3 |
| `EngineError` | | P15. No `UpdatePending`: the names the error dropped stay parked until the next full update (P1) |
| `VolumeSpaceAvailable` | | P15.2 and P13.2 |
| `Foreground`, `Background`, `BackgroundExpiring`, `QuitRequested` | | P11 |
| `SyncNowRequested`, `SyncNowCancelled`, `ExchangeEnded` | | P12 |
| `VisibleMedia` | | P13.2 |

- `StartEngine` is never emitted while an engine runs for the binding (P20 invariant 7).
- No row starts an exchange; only `SyncNowRequested` does (P12).
- **Confirmed root** (decided 2026-10-27; K2, K12.1 rule 3, V10.2). The core confirms the bound epoch for the current instance at its first `FetchDrained{root}` that finds `MGActive` naming that epoch. With `fetched` true, it reads the `MGActive` that fetch delivered and applied. With `fetched` false, `meguri-root` has not changed since the instance's saved cursor, and tracking recorded the epoch of every fetched `MGActive` with the binding, in the K13.1 step 2 transaction that came before that cursor (K10.3 `bindings`, P23), so that record is current: it confirms the bound epoch when it names it. When the binding records none, U114 first reads `MGActive` by record ID: naming the bound epoch confirms it; naming another epoch isolates `epoch_switched` (K18.4); unknown item or zone not found is K12.5's `meguri-root` row (`active_missing`). An instance created with no saved state gets no `fetched: false` input for a zone its first fetch did not list (P3), so it never confirms an epoch from a root zone that is gone. Each confirmation records V10.2's confirmation mark. So after a relaunch with an unchanged root zone, which is the normal case, the root fetch of P10.1's opening rows confirms the epoch and the device sends.

### P10.2 The foreground cloud flush target

- R88's target: in the foreground with auto on, an eligible change waits at most about 60 s before the app asks the engine to send it. The engine may send it earlier on its own schedule.
- The timer is armed at the first `TrackingChanged{cause: local}` that finds a sendable entity while no timer is armed. When it fires, one `RequestSend` follows, with no `UpdatePending`; the next local change arms it again. It is not armed while nothing is sendable, and never by a commit that is not a local change. After `quotaExceeded` it is armed as usual: it follows a local change, one of P15.1's event triggers, and is not a retry timer. It never hands a parked name back: only the local change's own full update did (P4).
- The target applies to the whole foreground, not only during a session. During a session it sends what P7 makes eligible.
- It is a request time, not a delivery deadline (R88: no fixed completion deadline). The policy never repeats a send because the engine did not finish.

### P10.3 Cleanup (K9.5, K17.4, K18.5, V17.3)

Decided 2026-10-27: the time-based deletions of the CloudKit specs run from one core action, `RunCleanup`, at a drained fetch. No timer runs them (P2).

- **When.** At `FetchDrained{collection}` while the gate is open: the first such input after the gate opens in this process, and then the first one at least 24 h after the last `RunCleanup` (P5 holds that instant; the input carries now). At most once per drain. A drained fetch of the bound collection zone, with `fetched` true or false (P3), is the "completed a fetch" that K9.5 needs, and the gate open means the binding is usable and `MGActive` names its epoch (K18.4).
- **What it computes**, in one tracking transaction, from tracking only:
  1. tombstone purges (K17.4): acked tombstones whose `deleted_at_ms` is more than 180 days before now, only while A5 and A9b hold (K17.4, K23; A9b is taken as false, so purging stays off until the signed runs show both hold);
  2. blob deletions (K9.5): blobs that meet every K9.5 condition. None while media is off, while `mediaZoneMissing` is set, or while another epoch zone is retained;
  3. zone deletions (K18.5): the previous epoch's zone 30 days after the switch, and a never-active collection zone whose age passes K18.5's 30 days;
  4. resolved conflict copies 30 days after resolution (V17.3). This is a tracking deletion only; nothing goes to CloudKit. The staged file a media copy holds (K16) is deleted after that commit (V6).
- It marks items 1 to 3 due in tracking. The `UpdatePending{full}` that follows hands them to the engine as record deletions (items 1 and 2) and zone deletions (item 3), with the normal sends. A deletion result drops the tombstoned entity (K17.4), sets the blob's server state `deleted`, or clears the zone mark. A failed deletion stays due. When it failed with an error the engine keeps (K12.4), the engine retries it; otherwise it is parked (P1), and the next full update at an event trigger hands it back. A due mark whose condition no longer holds at an update (a new name references the blob, the binding changed) is cleared without a deletion.
- It runs no exchange step and does not change any exchange's result. Nothing in it deletes local data other than resolved conflict copies, and nothing recreates a zone.

## P11. Foreground, background and quit

### P11.1 Foreground return (U151, KTD15)

U151 runs, strictly in this order: `resume()` (`engine/suspend` S2) → the review queue refresh → the `Foreground` input → the actions it returns. During a tracking stop, `Foreground` first emits `ProbeTracking` (P15.2).

| Condition | Actions |
|---|---|
| gate open, auto on | `UpdatePending{full}`; `RequestFetch{root, collection}`; `RequestSend{collection}` when sendable; `ResumeMedia` when media is on (its read round, P13.2; the download queue starts after `FetchDrained{collection}`, P13.1) |
| gate open, auto off | `UpdatePending{full}`, which keeps the engine's list current and sends nothing (P6) |
| an engine runs, the gate is closed only for a scan or an unsettled intent, auto on | `RequestFetch{root, collection}`; fetched records go to the inbox (P6) |
| otherwise | none |

An external change found by `resume()` sets the scan-required flag (K11.4), so the gate closes until the scan ends. When the scan ends, the gate opens and P10.1's opening row sends.

### P11.2 Background (U151, KTD15, R1552, R1554)

U151 runs, in this order: review pause and session record → the editor's `flush()` → `flush_for_background()` (`engine/suspend` S1) → the `Background{deadline}` input and its actions, inside U154's background-task assertion → `suspend(deadline)`.

| Condition | Actions |
|---|---|
| an exchange runs | `CancelFlushTimer`, `BackgroundFlush{until}`. The exchange continues until `until`, then ends `cancelled` (P12.6) |
| gate open, auto on, and an entity is sendable, an apply is in progress or a ready inbox entry waits | `CancelFlushTimer`, `BackgroundFlush{until}` |
| otherwise | `CancelFlushTimer` |

- `until` is the earlier of `deadline` − 5 s and now + 25 s.
- The background flush: settle ready inbox entries and intents in progress (K13.2), run `UpdatePending{full}` (so names parked since the last trigger go too), then send the collection zone under P7 (hold-back applies when a session is open), await the results, and persist the engine state. It fetches nothing new and starts no media transfer. Media transfers already running continue under the engine's own scheduling.
- At `until`, or on `BackgroundExpiring`, the flush and any exchange are cancelled: records without a result stay pending and in flight (K12.3); acked records stay acked (R1554). Then U151 calls `suspend`.
- From `suspend` until the next `resume()`, the engine instance stays, but nothing reaches the suspended core: a batch request returns no record, and fetched records and engine state events wait in the actor, in order and unsaved. If the process ends first, the cursor is older and they are fetched again (V5 R1).
- Assets of a waiting event (decided 2026-10-27). The system may delete a fetched asset's file from its own staging area once the handler returns. So while it handles each fetched event, and before it returns, the actor moves (or clones) every asset file of the event (`pa`, `ma`) into the cloud staging directory, also while the core is suspended, and queues the event with the staged paths. This is a file operation, not a tracking fact: the core inserts the inbox rows at `resume()` (K13.1 step 2, with the file already in place), and the start sweep of V6 deletes a staged file that no committed inbox row names, for example after the process ended first. Cloud work that the system starts in the background (P11.3) reaches the core only after U151 has run `resume()`; U154 owns that wake path.
- The next `Foreground` or `Lifecycle{Ready}` sends what is left.

### P11.3 Opportunistic background work (R1553)

The policy registers no background task. The engine's automatic scheduling and remote notifications run cloud work when the system allows. U154 owns every registration; no background task starts an exchange or an Anki transfer.

### P11.4 Mac quit (R127, R1557)

U175 ends an open session first (`sched/review-session#C16`), then U151 sends `QuitRequested`.

| Condition | Actions |
|---|---|
| an exchange runs | `QuitFlush{until: now + 10 s}`, whatever auto says. The exchange continues until `until`, then ends `cancelled` |
| gate open, auto on, and an entity is sendable, an apply is in progress or a ready inbox entry waits | `QuitFlush{until: now + 10 s}` |
| otherwise | `QuitNow` |

- The quit flush works as the background flush (P11.2). U161's sheet shows it with "Quit Without Syncing", which cancels it and any exchange.
- At `until` or on cancel the app quits; pending work stays durable and is not called synced.

## P12. Sync Now (R2048, R2049, R2050, R89, R1551)

### P12.1 Availability

`syncNowAvailability()` returns `available` or `unavailable{reason}`. The first matching row decides.

| # | Condition | Reason | Explanation key and English text |
|---|---|---|---|
| 1 | no collection open, or lifecycle not `Ready` | `collectionUnavailable` | `sync.now.unavailable.collectionUnavailable`: "Sync is unavailable until the collection is open." |
| 2 | a review session is active | `reviewActive` | `sync.now.unavailable.reviewActive`: "iCloud sync continues automatically during review. End the session to sync now." |
| 3 | iCloud not enabled, no binding (P1), kept local, the binding isolated as `stopped`, or every binding other than a salvage binding of another container or environment (`environment_changed`, V9.1) | `notConnected` | `sync.now.unavailable.notConnected`: "Connect to iCloud in Settings to sync this collection." |
| 4 | the user paused sync (V16) | `paused` | `sync.now.unavailable.paused`: "iCloud sync is paused. Resume it in Settings to sync now." |
| 5 | account not available | `iCloudUnavailable` | `sync.now.unavailable.iCloudUnavailable`: "Sign in to iCloud to sync." |
| 6 | publication decision pending (P17) | `decisionRequired` | `sync.now.unavailable.decisionRequired`: "Choose whether this collection replaces the one in iCloud before syncing." |
| 7 | binding isolated (a salvage binding is never the binding, P1), account not the binding's, or a `crt` mismatch (P6) | `recoveryRequired` | `sync.now.unavailable.recoveryRequired`: "iCloud sync needs your attention. Open iCloud status to continue." |
| 8 | otherwise | | available |

- An available request emits `RunExchange`. A request while an exchange runs returns that exchange; it never starts a second one. Repeated presses add nothing. `SyncNowCancelled` emits `CancelExchange`.
- Reason 2 is R89: the Sync Now command and `Y` are disabled, and the Mac tooltip (or the explanation shown with the disabled control on iOS and iPadOS) is row 2's text. Automatic sync continues (P8).
- Offline, throttling, quota and a tracking stop do not disable Sync Now: the exchange runs and reports `waiting` or `failed` (P12.6). After `quotaExceeded` the exchange sends as any other: its step 4 full update hands the parked names back (P15.1). During a tracking stop it acts as V11's Try Again: `ProbeTracking` comes before `RunExchange` (P15.2), and step 1 waits for the gate that a successful probe opens; a failed probe ends the exchange `failed{localStorageFull}`. The `StartEngine` that the opened gate emits does not change the token (P4), so the exchange continues with the new instance.
- With a pending decision Sync Now starts no exchange; U161 shows the decision's action (V11).
- An AnkiConnect `sync` (U146) uses the same availability and the same exchange; U146 maps `unavailable` to its typed error.

### P12.2 The boundary

At P12.3 step 2:

- On the writer, a barrier op records S_c, the seq of the last committed op.
- The exchange waits until the tracking observer has processed every op up to S_c (K11.1).
- Then, in one tracking read, it reads B, the binding's `change_seq` counter value (K10.3), and selects every entity that is pending with `change_seq` ≤ B, and every pending entity with no `change_seq`. Entities that are not pending for the policy (P3: `file_missing`, `withheld`, `superseded`) are never selected; their media state is the result's `media` field.

So every change committed before the capture is selected, a change the reconcile scan of step 1 found included (R2048). No session is active at capture (P12.1 row 2), so nothing selected is held then. Changes after the capture are not selected. A selected entity is satisfied by the ack of any image with an image seq at or above S_c (P12.4), whether or not it was edited again.

### P12.3 Steps

All steps run from `SyncCoordinator`'s task, outside the engine's event handling. Each step awaits the durable persistence of the engine's event handler (K12.3, K13.1) before the next. Each step uses the engine instance current when the step starts (P4); an engine started or recreated while the exchange waits does not end it.

1. **Gate.** Wait until the gate (P6) is open and no reconcile scan or apply runs, at most 30 s (a deadline of the exchange, P2). A scan that ends, a probe that succeeds or an engine that starts during the wait lets the exchange go on. The exchange ends at once when the gate is closed for a reason other than `ExclusiveBusy`, a scan or an unsettled intent, and after 30 s otherwise. P12.6 gives the result.
2. **Capture** the boundary (P12.2).
3. **Fetch** the root zone, then the collection zone, until each drains. Wait for the record-ID reads that the drain started for entries with reason `asset` (P4 `RetryAssetEntries`). Apply every ready inbox entry (K13.2), and, since Sync Now is an event trigger, retry the entries with reason `apply_failed` once (`ApplyReady{retryFailed}`, P4); wait until the applies are flushed and tracking is updated.
4. **Send** the collection zone. After step 3's applies, run `UpdatePending{full}` (K12.4), so names the engine dropped after an earlier failure (parked, P1) and deferrals are handed back; then call `sendChanges`, and await its return, the persistence of its results, and U114's first read of each save-conflict entry of this send whose payload is in an asset (K12.3, P4 `RetryAssetEntries`). The engine sends its whole pending list. Entities changed after the capture may be sent with the selection; they are not selected, so the result does not wait for them, and those without an ack stay pending (R2048). A selected entity with no result when the exchange ends counts as outstanding (P12.4), whether or not its record was handed to the engine in a batch: the step waits only for `sendChanges` to return, never for an entity the engine was not handed (decided 2026-10-27). A server-record-changed result merges under K12.3; a merge result that differs from the server version is sent again in the next round.
5. **Fetch** the root and collection zones again and apply, as step 3. If this makes a selected entity pending again, run step 4 again, then step 5.
6. **Account** for the selection (P12.4). The core returns the result; `SyncCoordinator` reports `ExchangeEnded{result}`, and the policy emits `RecordExchange{result}` (P12.7, which also covers an exchange that stops before this step).
7. **Media**, when media is on: `ResumeMedia` (P13.1), whether or not media already runs: its read round finds blobs to upload and blobs another device uploaded, and its `RequestSend{media}` sends the blobs that are ready, also with auto off. Its uploads and downloads continue after the exchange ends, under P13.2 and P13.3. The exchange's collection result does not wait for them.

A session that starts during an exchange does not stop it. Every batch follows P7 with the new R. Incoming applies continue.

### P12.4 Accounting (R2049)

The first matching row decides for each selected entity at the end:

| A selected entity at the end | Counts as |
|---|---|
| an in-flight entry with an image seq at or above S_c was acked (K12.3 "Saved", or the row for an in-flight digest), tombstones included; or the entity is clean; or it was dropped unsent (K17.1) | done; reported as held back when that image was held |
| oversize, unreadable, unsyncable, format-blocked, its server version waiting in the inbox for `format` (K12.1 rule 8), waiting on a reference that is one of these, or in a generation that cannot be claimed (K7, K6, K9.4, K5.4, K12.1, K19.3) | blocked |
| anything else: no result for its in-flight record, a retryable error, conflict rounds used up, held with no sendable image (deferred, or K12.1 rule 5), waiting for this device's generation claim (rule 7) or for another device's generation (rule 8, `generation`) | outstanding |

Incoming entries fetched in steps 3 and 5:

| Inbox reason (K10.3) | Effect on the result |
|---|---|
| `ready` | must be applied durably |
| `generation`, `reference`, `structure` | waits for another device's work; does not prevent `succeeded`; counted in `incomingWaiting` |
| `format`, `asset` | prevents `succeeded`; counted as blocked incoming |
| `apply_failed` (K13.2, an entry fetched in these steps or before) | prevents `succeeded`; counted as blocked incoming. Step 3's apply retries it once, as an event trigger does (P4 `ApplyReady`) |

- The collection result is `succeeded` only when every selected entity is done, every ready entry is applied durably, no entry waits with reason `format`, `asset` or `apply_failed`, and the engine state is persisted.

### P12.5 Bounds

| Bound | Value | When reached |
|---|---|---|
| waiting for the gate (step 1) | 30 s | the exchange ends `waiting` |
| send rounds (step 4 runs) | 3 | the remaining selected entities count as outstanding |
| waits for a retry-after | Removed (2026-10-27) | a throttled result ends the exchange `waiting` at once |
| conflict merges of one entity | 3 | that entity is outstanding |

- Removed (2026-10-27): the exchange's waits for a retry-after. Decided 2026-10-27 (P2, V10.3): when a step gets `throttled{retryAfter}`, the exchange ends `waiting{throttled, retryAfter}` after the results in hand persist. It never sends again itself; the engine honours the retry-after, and status shows the retry time.
- There is no overall time limit. Every step either makes progress or reaches a bound, so the exchange cannot loop under continual edits or throttling (KTD25).

### P12.6 Results

| Result | When | Pending work |
|---|---|---|
| `succeeded{heldBack, newerPending, incomingWaiting, media}` | P12.4 | kept: entities edited after the capture, held entities, media |
| `incomplete{outstanding, blocked}` | some selected entity is outstanding or blocked, or an entry waits for `format`, `asset` or `apply_failed` (P12.4) | kept |
| `waiting{class, retryAfter?}` | offline, throttled (P12.5), a transient authentication error from an engine operation (P15), or the gate still closed after 30 s (step 1) | kept |
| `failed{class}` | account change, an account status of no account or setup needed, quota full, local storage full or a tracking error, zone deleted, encrypted data reset, an epoch switch, a `crt` mismatch (P6), unsupported schema (P15) | kept; isolation per `sync/cloudkit-recovery` |
| `cancelled` | `SyncNowCancelled`, close, `CollectionReplaced`, the background or quit `until`, a guard that became stale for another reason | kept; acked records stay acked |

- When several rows apply, the first of `failed`, `cancelled`, `waiting`, `incomplete`, `succeeded` is the result. A guard that became stale through an account or binding change gives `failed`.
- `media` is one of `done`, `pending{files, bytes}`, `paused` (R2050). A collection result never says media is done when it is not.
- Success means this device finished its requested exchange. The result text never says that other devices have the changes (R2050).
- Cancel stops after the engine operation in progress returns and its results persist.
- An `incomplete` result sets the attention item `exchangeIncomplete` (P14.4) until a later exchange succeeds or every entity it counted is done (AE91).

### P12.7 Recording

- Every `ExchangeEnded` input emits `RecordExchange{result}` once, whichever step the exchange ended at (decided 2026-10-27): step 6, or an early stop such as the step 1 gate wait, a cancel before the capture, a background or quit `until`, or a guard that became stale.
- `RecordExchange` writes a `succeeded` result as the binding's last completed exchange: its end instant, source and counts. Any other result is written as the last exchange attempt: its kind, instant and error class. U161 shows both (R1560).
- With `RecordExchange` the core runs the day clock's `checkRollover()` once, whatever the result (R597, `sched/day-clock#D8`); the check is idempotent, so it writes nothing when the day did not change.
- When the exchange ended because the collection closed or was replaced (`CollectionReplaced`), the action's guard names a lifecycle generation that is no longer current (P4): `RecordExchange` writes no row and runs no check. The first `checkRollover()` of the next open or of the new generation runs instead (`sched/day-clock#D8`).
- The exchange's capture and selection live in memory. A crash ends the exchange without a result; its pending work stays in tracking.

## P13. Media (R2057, R122, R124, R1592)

### P13.1 Order and the media setting

- Collection records go first: across requests, and within a batch request whose scope includes both zones, where the builder puts every sendable collection record before any blob record (K12.1 order). A request scoped to one zone holds only that zone's records; zones are never mixed to keep this order (K12.1, "Send scope"). Blob names join the engine's pending changes when the blob is ready to upload (P13.3); a collection entity that waits for the flush timer does not hold them back.
- Asset downloads start only after the collection zone's fetch in progress has drained (`FetchDrained{collection}`); with no collection fetch in progress they start at once.
- Media off (R122): `PauseMedia`. No blob is uploaded or downloaded, and `UpdatePending{full}` removes blob names. Media-name entities are collection records and keep syncing. Status shows media paused.
- Media on: `ResumeMedia`, then P13.2 and P13.3 when the gate is open. With auto off, `ResumeMedia` waits for the next Sync Now (P12.3 step 7), as other cloud work does (P6).
- `ResumeMedia` (decided 2026-10-27), executed by `SyncCoordinator` through U114, in this order, each step after the one before returns: (1) read `root` by record ID, with `e` (P13.2), before any blob work, every time (K9.2). When the binding holds no acked media key, record the key read as acked (K9.2, V10.1). When it holds one and the read returns another key, unknown item or zone not found, set `mediaZoneMissing` and stop: no later step runs, now or at a later `ResumeMedia` (K2, K9.2, K12.5). Unknown item or zone not found with no acked key sets it too (V10.1). A read that fails with any other error ends this `ResumeMedia` with no blob work; the next `ResumeMedia` reads again (P2). (2) `ReadBlobs{missing}` (P4, P13.2) unless a read round runs, and await it; (3) `UpdatePending{full}`, which now adds the names of blobs ready to upload (P13.3); (4) `RequestSend{media}` when a blob is ready to upload, with auto on or off; (5) start the download queue (P13.2), when it is not running, once the collection zone's fetch in progress has drained, as the second bullet says. It is idempotent: with media already running it repeats steps 1 to 4, so the `root` read of step 1 runs at every `ResumeMedia` (decided 2026-10-27). A device with `mediaZoneMissing` never creates the zone or the root (K2).
- `PauseMedia`, executed by `SyncCoordinator`: stop the download queue and the record-ID reads. A download in progress is cancelled after its staged bytes are left as V6 says, so only that blob restarts later. Then `UpdatePending{full}` removes blob names. Uploads the engine has already started end under its own scheduling, as in P11.2.
- After `quotaExceeded`, blob uploads stay pending in tracking, as collection records do: the engine drops the failed blob names, which are parked (P1) until the next full update at an event trigger. Downloads continue (P15.1).
- A missing local file is not a deletion (decided 2026-10-27, `sync/cloudkit-records#K9.4`, `media/media-journal#J6`). Only an explicit Meguri removal makes a media-name tombstone: a feed item whose absence cause is `removed` (`media/media-journal#J6`; K9.4 lists its callers, among them Delete Unused, a Check Media fix and the old name of a Check Media rename). Whether the journal record itself is deleted does not decide; the cause does. A name the J6 feed reports absent with the cause `missing` (its file facts were dropped: deleted or evicted outside Meguri, an incomplete restore; a removal item for dropped facts included) is `file_missing`: a download candidate (P13.2), and `mediaMissing` when the media zone holds no blob for it. A file that leaves the size range is `unsyncable`. Neither makes a tombstone. A `removed` item while J6 reports the feed not complete gives the mark `removal_held` (K9.4): the name is neither sent, downloaded nor reported missing, counts in `blocked`, and is sent as a tombstone once J6 reports the feed complete.

### P13.2 Downloads

- Mechanism (decided 2026-10-27). The engine's fetch options have no way to leave out asset fields, so every record the engine fetches arrives with its assets downloaded. A fetch of `meguri-media` through the engine would therefore download every blob at once, outside every bound below, and production allows only one engine per database. So:
  - The engine's fetch scope is exactly `meguri-root` and the bound collection zone. U114 sets that scope in the options the engine asks for before its own scheduled fetches and in every `RequestFetch` (P4). `meguri-media`, the previous epoch's zone and every other `c-` zone are excluded, which is how K20's first row ("not fetched after a switch") holds.
  - Media-zone records are never fetched as changes. U114 reads them by record ID with its own record fetch operation, naming the fields it wants: `MGMediaRoot` (`root`) with `e` at `ResumeMedia` (P13.1); a blob with `e` only to learn whether it exists and to check its envelope, before an upload decision (P13.3) and before a download is queued; a blob with `e` and `a` in the download queue below. The blob's record name comes from the content that a live media-name entity names (K4.2), so no listing of the zone is needed.
  - A read result sets the blob's server state (K10.3): present, or `deleted` for unknown item. A `deleted` blob is uploaded again when the local file holds its content (K9.5); otherwise its live names count in `mediaMissing` (P3, P14.4), and the blob is read again by each `ResumeMedia` read round (scope `missing`: each `Foreground` with auto on, each Sync Now media step, P12.3 step 7), so a blob another device uploads later is found. These reads are event triggered. Zone not found, or the engine's database changes reporting `meguri-media` deleted, sets `mediaZoneMissing` (V10.1).
  - **Read rounds** (decided 2026-10-27). A blob in state `unknown` that a live media-name entity names is read in a `ReadBlobs` round (P4), never one by one. A round runs at `ResumeMedia` (step 2), and, with auto on, media on and `MGMediaRoot` acked, at `FetchDrained{collection}` and at a `TrackingChanged{cause: local}` (a new picture, P10.1) while `blobsUnknown` is above 0. At most one round runs at a time; a trigger during a round adds nothing. With auto off a round runs only from Sync Now's media step, as other cloud work (P6). Each round is reported as `EngineActivity{media, read}`; a result that makes a blob ready to upload reaches the engine through the update that follows it (P10.1, `ResumeMedia` step 3).
  - A read that fails with a retryable error is tried again only at the next round (P2), and the round counts as a failing cycle of kind `read` (P15).
  - Blob uploads and the blob deletions of P10.3 still go through the engine's sends to `meguri-media` (K12.4); sending needs no fetch scope.
  - So Assumption A4 (K9.3, K23) is false for the engine's fetch and is relied on only for record-ID reads that name their fields. The signed runs check it (P21).
- Downloads and record-ID reads run only while the gate is open (P6) and media is on; an install waits while the gate is closed.
- Priority, highest first:
  1. blobs named in `VisibleMedia`: the current card and the review head (`head(3)`), and the cards and notes on screen;
  2. blobs named by cards due today;
  3. all others, in record-name order.
- A `VisibleMedia` input re-sorts the waiting downloads (`PrioritizeMedia`); it cancels no download in progress.
- At most 2 asset downloads run at once.
- Staged bytes (K9.3) never exceed 256 MiB, except that one file larger than that may be staged alone.
- A download starts only when the volume's free space after it stays at or above 512 MiB. Otherwise media downloads pause and status reports `localStorageFull` for media. The pause ends at `VolumeSpaceAvailable`, `Foreground` or `ResumeMedia` when the free-space rule holds again.
- Assets are streamed to files, never held whole in memory.

### P13.3 Uploads

- A blob is ready to upload when a live media-name entity names its content, the local file holds that content, and its server state is `deleted`: a record-ID read (P13.2) found it absent. A blob in state `unknown` is read first, in the next read round (P13.2), so bytes already in the media zone are never uploaded again (K9.1); a save that still meets an existing blob counts as success (K9.3). A media-name entity marked `file_missing`, `withheld` or `superseded` (P3) never makes its blob ready.
- Blob uploads go through the engine's sends to `meguri-media`. With auto on the engine schedules them once the names are pending; with auto off only `ResumeMedia`'s `RequestSend{media}` sends them (P13.1), so a picture added with auto off is uploaded at the next Sync Now.
- A media-zone batch holds at most 8 blobs and at most 128 MiB of asset bytes; one larger blob goes alone.
- The asset file is an APFS clone of the media file in the cloud staging directory, checked against the entity's SHA-256 after cloning. On a mismatch the clone is dropped and the file is hashed again (`media_hashes`, K9.1). When the file's SHA-256 still differs from the entity's, tracking durably sets the entity's mark `media_changed` (K9.3, K10.3): the blob is not uploaded and status lists the file under the attention item `mediaChanged`, also after a relaunch, until the next feed item of that name, or a reconcile scan that reads other facts for it, clears the mark and the name compares as any local change (K9.3, K9.4).
- A `present` blob can be stale (K9.3, "Stale `present`"): tracking makes it `unknown` again when a local change names its content anew and at the first drained collection fetch after that version's ack, so it counts in `blobsUnknown` and the next read round reads it before any upload decision.

## P14. Status (R1558, R1559, R1560)

### P14.1 Sources

Status is a function of the latest tracking summary (P3) and the policy state (P5): account, binding, fence, device settings, reachability, engine activity, error class and tracking stop. The last completed exchange and the last quiet point come from P19's rows.

It never reads `col.mod`, `col.scm`, `col.ls`, `usn` columns or graves, and never sends an Anki `meta` request (R1559). Anki transfer status is separate and appears only in Settings Anki Transfers (R1560).

### P14.2 State

The first matching row gives the state.

| # | State | When |
|---|---|---|
| 1 | `localOnly` | iCloud not enabled on this device (never connected, V16 Stop or local only), the sidecar decision is local only (K18.2), the binding is isolated with reason `stopped`, or every binding other than a salvage binding (P1) is of another container or environment (`environment_changed`, V9.1, V11) |
| 2 | `iCloudUnavailable` | account `noAccount`, `restricted`, `temporarilyUnavailable`, `couldNotDetermine` or `configuration`; or an engine operation failed with a transient authentication error (P15) and no send or fetch has ended with outcome `ok` since (P3 `EngineActivity`). A binding isolated `signed_out` while the account is `noAccount` matches here |
| 3 | `paused{decisionRequired}` | a publication decision pending (P17); the binding isolated `replaced_locally` by it does not count as `isolated` |
| 4 | `needsAttention` | any attention item (P14.4), a binding isolated for any other V2.2 reason included; a salvage binding (`tracking_damaged`, P1) adds no item of its own |
| 5 | `paused` | the user paused sync (`userPaused`), or auto off (`automaticOff`) |
| 6 | `offline` | not reachable |
| 7 | `syncingCollection` | an exchange runs, or the engine sends or fetches a collection zone (a fetch from the start after an expired change token included) |
| 8 | `syncingMedia` | a blob upload or download runs |
| 9 | `changesPending` | an entity is pending for the policy (P3: not `file_missing`, `withheld` or `superseded`) and not held, an entity is in flight, or a ready inbox entry waits |
| 10 | `upToDate{asOf}` | otherwise; `asOf` is the later of the last completed exchange's end and the last quiet point |

Held entities never prevent `upToDate`; they appear in `heldBack` (R1559).

### P14.3 Quiet point

At an input after which both zones' fetches have drained since they last started, every ready entry they fetched is applied durably (no ready entry, no apply in progress), and the summary shows no pending entity that is not held and nothing in flight, the policy emits `RecordQuietPoint{now}` once for that drain. "Both zones" are the engine's fetch scope, `meguri-root` and the bound collection zone (P13.2). Media, `file_missing`, `withheld` and `superseded` entities (P3) and held entities do not affect it.

### P14.4 Details

| Field | Meaning |
|---|---|
| `pending` | collection entities sendable now |
| `heldBack` | entities held by the session (P7.2) (R1559) |
| `inFlight` | records handed to the engine without a result |
| `incomingWaiting` | inbox entries waiting (K13.1) |
| `media` | files and bytes to upload and download, the `file_missing` and `superseded` names included (P3); `paused`; while downloads run, an estimate of the time left (R1583): the bytes left to download divided by the download throughput of the last 60 s, none before 60 s of downloading or while paused (decided 2026-10-27) |
| `conflicts` | unresolved conflict copies (K16, V17.2) |
| `attention` | items, each with a count (list below) |
| `error` | the R1561 class (P15) and its retry-after, when one applies |
| `unavailableReason` | state 2 only: `noAccount`, `restricted`, `temporary` or `setupNeeded` |
| `pausedReason` | states 3 and 5 only: `decisionRequired`, `userPaused` or `automaticOff` |
| `lastExchange` | P12.7 |
| `account` | status and whether it matches the binding |

Attention items:

| Item | When |
|---|---|
| `isolated` | no binding is active, and the newest binding other than a salvage binding is isolated for a V2.2 reason other than `stopped`, `environment_changed`, `replaced_locally` and `tracking_damaged` (V9, V10, V8.2) |
| `chooseCollection` | iCloud enabled and no binding (P1): the tracking file was missing or damaged (V8.1); a salvage binding does not prevent it (V11) |
| `conflicts` | unresolved conflict copies, until each is resolved (V11, V17.2). Opening Review Conflicts does not clear it (decided 2026-10-27) |
| `conflictsUnreadable` | a damaged tracking file's salvage could not read every conflict copy, while the moved-aside file is kept (V8.1) |
| `refused` | refused records (K20) |
| `referenceWaiting` | entries still waiting with reason `reference` after a drained fetch of their zone (V11) |
| `oversize`, `unreadable`, `unsyncable` | entities with that flag (K7, K6.1, K9.4) |
| `claimStuck` | a local generation still `local` after 5 failed claim attempts (K19.4) |
| `generationIncomplete` | a remote generation `waiting` with no new member for 7 days (V18) |
| `assetUnavailable` | entries with reason `asset` after 5 attempts (K13.1) |
| `applyFailed` | inbox entries with reason `apply_failed` and failure `apply` after their fifth failure, with kind and key (K13.2 "Failed applies") |
| `rekeyFailed` | a failed re-key: inbox entries with reason `apply_failed` and failure `rekey`, with kind and key (K14.2, K10.3) |
| `crtMismatch` | the crt check finds the open collection's `col.crt` different from the binding's epoch `crt` (P6, K20); it clears when a check finds them equal again, or when V14 or V15 replaces one side |
| `collectionCorrupt` | lifecycle `Failed{Corrupt}` |
| `mediaZoneMissing` | `meguri-media` missing or deleted while the collection zone exists (V10.1) |
| `mediaChanged` | P13.3 |
| `mediaMissing` | live media names whose file is missing locally and whose blob the media zone does not hold (P3 `mediaMissing`; K9.3, K9.4, K9.5), with a details list of the names and Copy Debug Info. It clears for a name when its file is present again, a later read finds its blob and the download installs it, or the name stops being live (decided 2026-10-27) |
| `recoveryOpWaiting` | a recovery op waiting for Resume or Cancel (V14.3, V15.3) |
| `exchangeIncomplete` | P12.6 |
| `quotaFull` | P15 |
| `localStorageFull`, `trackingError` | P15.2 |
| `unsupportedSchema` | a newer record format or tracking schema (K5.4, V8.1) |
| `cloudError` | P15, last row |

- A completed exchange can still show held, newer, waiting or media work; each is its own field (R1559).
- While a reconcile scan runs, counts are tracking's values from before the scan and are marked provisional.
- `StatusChanged` is emitted when the value changes. No status is a modal; none blocks study (R1562).

## P15. Error classes (R1561, R1555, R1556, R2059)

U114 maps each CloudKit result to a class (Apple's `CKError` codes and account statuses, and the derived status `configuration`, P3). Per-record outcomes that K12.3 resolves (server record changed, unknown item, batch request failed, limit exceeded, asset errors) are not error classes; K12.3's `invalidArguments` row is, below. The class names are R1561's; V11 gives the reason each row adds.

| CloudKit result | Class | State and attention | Pending work | Policy action |
|---|---|---|---|---|
| account `noAccount`; account change `signOut` | `noAccount` | `iCloudUnavailable{noAccount}` | kept; the binding is isolated `signed_out` (V9.1) | `StopEngine`, `CancelFlushTimer`, `CancelExchange` |
| account `restricted` | `noAccount` | `iCloudUnavailable{restricted}` | kept; nothing isolated | `StopEngine`, `CancelFlushTimer`, `CancelExchange` |
| account status `temporarilyUnavailable` or `couldNotDetermine` | `noAccount` | `iCloudUnavailable{temporary}` | kept; nothing isolated (V9.1) | `StopEngine`, `CancelFlushTimer`, `CancelExchange` |
| `notAuthenticated` or `accountTemporarilyUnavailable` from an engine send or fetch, while the account status read stays `available` with the binding's identity | `noAccount` | `iCloudUnavailable{temporary}` until a send or fetch ends with outcome `ok` (P14.2 row 2) | kept; nothing isolated | none; the engine keeps these names and retries them itself (K12.4). U114 reads the account status and identity again; an `Account` input follows only when they changed (decided 2026-10-27) |
| account change `signIn` whose identity equals the binding's verified identity (P3 `Account`) | none | unchanged | kept | `UpdatePending{full}` when the gate is open (P10.1); logged; the token does not change |
| account change `switch`, or `signIn` with another identity; an available account other than the binding's | `accountChanged` | `needsAttention{isolated}` | kept; isolated `account_changed`; never sent to the new account (R2055) | `StopEngine`, `CancelFlushTimer`, `CancelExchange` |
| account status `configuration` (P3), which U114 derives from two sources: a failed account status read, or a failed engine operation or record-ID read, with `missingEntitlement`, `badContainer`, `permissionFailure`, `managedAccountRestricted` or `badDatabase` | `noAccount` | `iCloudUnavailable{setupNeeded}` | kept; nothing isolated (V10.3) | `StopEngine`; the gate opens again only after an account status read with another status (P3) |
| `networkUnavailable`, `networkFailure` | `offline` | `offline` | kept | none; the engine keeps these names and retries (K12.4) |
| `requestRateLimited`, `zoneBusy`, `serviceUnavailable` | `throttled{retryAfter}` | unchanged state; quiet detail (R1555) | kept | none; the engine keeps these names and honours the retry-after (K12.4) |
| `changeTokenExpired`, only when the engine surfaces it (V10.3) | none | `syncingCollection` while the engine fetches from the start (V10.3) | kept; acknowledgements kept | none. That fetch is not a fetch from no cursor and runs no K17.4 absence check (V10.3, A9b) |
| `operationCancelled` after the app's own `StopEngine`, `ConfigureAutomatic` or cancel | none | unchanged | kept | none; not a failing cycle |
| `quotaExceeded` | `quotaFull` | `needsAttention{quotaFull}` until a later save succeeds: a send that ends with outcome `ok` and `saved` above 0 (R2059, P15.1) | kept; fetching continues; local data, backups and file export unaffected | none. The engine drops the failed names (K12.4); the core parks them (P1); the next `UpdatePending{full}` at an event trigger hands them back. The error emits no `UpdatePending` itself (P15.1) |
| a failed tracking or staging write (P15.2) | `localStorageFull` | `needsAttention{localStorageFull}` with no space, `needsAttention{trackingError}` otherwise | kept | P15.2 |
| `zoneNotFound`, a zone deleted by the user, an encrypted data reset, an epoch switch, a missing `meguri-root` or `MGActive`, a refused record, a re-key failure, a `crt` mismatch (P6) | `conflictRecovery` | `needsAttention` with the item | kept; isolation where K12.3, K18.4, V10 say | `StopEngine` when the binding is isolated |
| `incompatibleVersion`; a newer record format (K5.4) | `unsupportedSchema` | `needsAttention{unsupportedSchema}` | kept; the record waits | none |
| `invalidArguments` (`code`, P3): a batch with a change outside its send scope, or another malformed request (K12.3) | none | `needsAttention{cloudError}` at once, not after 2 failing cycles (decided 2026-10-27), until an `EngineActivity{ended, ok}` of the same (zone, kind) | kept; nothing changes in tracking | none. The engine drops the names (K12.4) and they are parked (P1): no name is handed back at once, and the error emits no `UpdatePending`; the next full update at an event trigger hands them back. U114 logs the request's scope and record names for Copy Debug Info (K12.3) |
| any other error | none | unchanged for the first 2 failing cycles of a (zone, kind); then `needsAttention{cloudError}` | kept | none. The engine drops the names (K12.4); they are parked and handed back by the next full update at an event trigger. A failed read round is tried again at the next round (P13.2) |

- A failing cycle is an engine send or fetch, or a round of U114's record-ID reads (P13.2, `RetryAssetEntries`), of one zone that ends with outcome `failed`: an `EngineActivity{zone, kind, ended, failed}` (P3). It counts per (zone, kind), never per record, and takes its class from the `EngineError`s of that operation, which U114 reports before the `ended`. An `EngineActivity{ended, ok}` of the same (zone, kind) resets the count.
- Decided 2026-10-27: the sync engine documents `notAuthenticated` and `accountTemporarilyUnavailable` as transient errors that it retries itself when system conditions allow (checked in the signed runs, P21), so an engine result with either code stops nothing. Only an account status read of `noAccount`, `restricted`, `temporarilyUnavailable` or `couldNotDetermine`, or the derived status `configuration` (P3), stops the instance. Stopping on a transient code would leave the engine stopped while the account status stays `available`, because no `Account` input would reopen the gate.
- Errors never discard pending work, never recreate a deleted zone, and never clear conflict copies (R2054). An HTTP 409 or any Anki error mapping never applies to CloudKit (R1556).

### P15.1 Quota exceeded (V10.3)

Removed (2026-10-27): the quota pause, its 1-hour delay and `QuotaRetryDue`. The rest of this section is in force. Decided 2026-10-27 (P2, `sync/cloudkit-recovery#V10.3`): quota exceeded sets no pause and no timer.

- At `quotaExceeded` the policy records the error class and sets the attention item `quotaFull`. It emits no other action. The engine drops every name that failed so (K12.4; `sync/cloudkit-recovery` A13 does not hold for quota exceeded, K23), and the core parks them (P1). The error emits no `UpdatePending`, and `UpdatePending{changes}` never hands a parked name back, so a failure cannot start a send loop: nothing is sent again until an event trigger.
- The full update hands every parked collection and blob name back at the event triggers (P2): a local change (`TrackingChanged{cause: local}`, P10.1), `Foreground` (P11.1), an `Account` input (P10.1, P15), Sync Now (P12.3 step 4), an engine start, and a drained fetch (`FetchDrained`, P10.1); also at `SessionEnded` (P9) and in the background and quit flushes (P11.2, P11.4). The engine's other reports (`EngineActivity`, `EngineError`) only recompute the status. There is no timer: the flush timer is armed only by a local change and hands nothing back (P10.2).
- The attention item `quotaFull` stays until a later save succeeds: an `EngineActivity{send, ended, ok}` with `saved` above 0 (P3). A send in which a record still fails with `quotaExceeded` ends `failed` and keeps the item.

### P15.2 Tracking failures (V3.3)

- `TrackingWriteFailed` sets the tracking stop: `StopEngine{trackingFailed}` (the actor stops the instance without saving its state, V3.3), `CancelFlushTimer`, and a running exchange ends `failed{localStorageFull}`. The attention item is `localStorageFull` when `noSpace`, else `trackingError`.
- No timer retries the stop (V3.3, P2). Its triggers are `TrackingChanged` (a local change whose dirty mark committed), `Foreground`, `SyncNowRequested`, an `Account` change and `VolumeSpaceAvailable`. No engine runs during the stop, so no engine report is a trigger. At `TrackingChanged` that commit is the successful write. At each other trigger the policy first emits `ProbeTracking` (P4), which reports `TrackingChanged` or `TrackingWriteFailed`.
- A `TrackingChanged` that is a local change clears the stop at most once between two `Foreground` inputs (P5 holds the flag; `Foreground` resets it). After that, during the same foreground period, only the `TrackingChanged` that answers a `ProbeTracking` at one of the other triggers clears it (P5 holds a probe in progress, so the policy tells the two apart). Decided 2026-10-27: a failure that repeats on every commit (a constraint error on an inbox insert, a nearly full disk) then costs one engine restart and refetch per foreground period, not one per answer.
- At a `TrackingChanged` that clears the stop, the attention item clears with it. A reconcile scan that the stop held (a scan whose commit failed, or a scan-required flag set during the stop, K11.4 "Failed commits") starts then, and runs before any engine: the gate stays closed for it (P6). When it ends and the rest of the gate holds, the gate opens (P10.1): `StartEngine` from the last saved engine state.
- A probe, or a commit of the new instance, that fails sets the stop again (`TrackingWriteFailed`). Nothing retries until the next trigger.
- So no cursor advances past an inbox write that failed (K13.1, V3.1).

## P16. Manual Anki transfers (KTD26, R2060, R1547, R89)

- No input maps to an Anki transfer: launch, foreground, background, timers, session end, Sync Now, AnkiConnect `sync`, optimizer adoption, network recovery and account events start no Anki request (R1549). P21 tests this for every input.
- Manual transfers start only from Settings Anki Transfers with an operation id and confirmation (`sync/anki-transfers` M1, M2). That work is frozen (plan, "Anki server work: frozen"); this file adds nothing to it.
- While a review session is active, `sync/anki-transfers` M2 refuses a collection transfer (R89). This file adds no reason or string for it.
- A manual transfer never reads or writes CloudKit tracking and never changes CloudKit status. It reaches CloudKit only through a collection replacement, which sets the publication fence (`engine/collection-replace#R6.1`, P17). A successful transfer never clears that fence.
- Force one-way exists only inside a manual transfer (R1547). The policy has no one-way state and no next-sync setting.
- File import and export are independent of both.

## P17. Publication fence and recovery decisions (R1548, R2055, R2056)

The fence is an input. The policy never clears it: only the explicit choices of `sync/cloudkit-recovery` do (`engine/collection-replace#R6.1`: only U113 or U114 after the explicit choice).

| Fence, binding or setting | Engine | Sends | Applies | State | Sync Now |
|---|---|---|---|---|---|
| clear, binding usable | P6 | P7 | yes | P14 | P12.1 |
| `cloudPublicationPending` (restore, `.colpkg` import, manual Import; the old binding isolated `replaced_locally`, V12.1) | stopped | none | none | `paused{decisionRequired}` | `decisionRequired` |
| replacement intent unfinished or unproven (`engine/collection-replace#R6.3`) | stopped | none | none | `paused{decisionRequired}` | `decisionRequired` |
| binding isolated for a V2.2 reason other than `stopped`, `environment_changed`, `replaced_locally` and `tracking_damaged` (a salvage binding is never the binding, P1) | stopped | none | none | `needsAttention{isolated}` | `recoveryRequired` |
| binding isolated `stopped`, every binding other than a salvage binding of another container or environment (`environment_changed`, V9.1), or kept local (K18.2) | none | none | none | `localOnly` | `notConnected` |
| iCloud enabled, no binding (P1: a salvage binding does not count; tracking file missing or damaged, V8.1) | none | none | none | `needsAttention{chooseCollection}` | `notConnected` |
| paused by the user (V16) | none | none | none | `paused{userPaused}` | `paused` |

- Sync Now, launch, relaunch, an Anki transfer and `scm` never clear a fence (U151 test).
- What the decision flow fetches to show counts, and how it starts a new epoch or adopts one, is `sync/cloudkit-recovery`. When the fence clears or a `Binding` input reports a usable binding, the gate opens and P10.1's opening row applies.
- Work queued for an old binding or generation is never sent for the new one: the binding and the guard change (P4, K10.2).

## P18. Optimizer (U47)

Added by U47: the optimizer action, its CloudKit reconciliation and revision gate (R699, KTD8).

## P19. Tracking rows this file adds

Logical content in `meguri-cloud.db` (K10), per binding:

| Row | Fields |
|---|---|
| last completed exchange | instant, source, selected count, held, newer pending, incoming waiting, media state |
| last exchange attempt | instant, result kind, error class |
| last quiet point | instant |
| tracking probe | instant of the last `ProbeTracking` commit (P15.2); nothing reads it |

## P20. Invariants

1. No action starts, resumes or schedules a manual Anki transfer.
2. While a session is active, no record handed to the engine contains the effect of a step of R or of the detached hold.
3. Every collection-zone record is built from a barrier, from commits covered by the WAL flush.
4. Nothing is sent or applied while the gate is closed or a fence is set.
5. An exchange reports `succeeded` only when P12.4 holds; it never reports media done when media is pending.
6. Status never reads `col.mod`, `col.scm`, `col.ls`, `usn` or graves, and never makes an Anki request.
7. At most one flush timer, one exchange, one background or quit flush, and one engine per binding exist at a time. `StartEngine` is never emitted while an engine runs, and no new engine object is created before the previous one has reported `EngineStopped` (P4). No other app timer exists; a deadline only ends its own operation (P2).
8. No `sendChanges` or `fetchChanges` call is made from inside the engine's event handling.
9. A call with a stale guard, and a fact from an instance that is not current, change nothing.
10. `step` is deterministic and reads only its state and the input.
11. Every undo step whose effect a handed record contains was flagged at that record's barrier, before the writer took another op. A flagged step is never undone.
12. Every `RequestSend` except `FlushTimerFired`'s follows an `UpdatePending{full}` in the same action list or in `StartEngine`. No `EngineError` and no `TrackingChanged` other than a local change hands a parked name back (P4).
13. No deferral outlives a full update, except a deferral of the incoming-held set, which ends at `SessionEnded`; so every other entity deferred under an R that has since moved is decided again by a later barrier (P7.2).
14. While a session is active, no record handed to the engine is built for an entity of the incoming-held set, so a held image never replaces a newer server version (P7.2, P8.3).
15. Nothing is sent or applied while the open collection's `col.crt` differs from the binding's epoch `crt` (P6).

## P21. Test obligations

Focused Rust tests in `core/crates/meguri-sync/tests/cloud_policy.rs`, with a fake engine, fake clock and fake account:

- Every row of P9, P10.1, P11, P12.1, P14.2, P15 and P17, as input-output tables.
- Property test: random sequences of every P3 input produce no Anki transfer action, no `StartEngine` while an engine runs, and keep invariants 2, 4, 7, 9, 10, 13, 14 and 15.
- Hold-back: every send path (automatic, cloud flush, exchange, background, quit, retry after restart) respects the newest answer after newer edits, an undo of the newest answer, a flag after the answer, a deletion in R, an Add over the review, a Browse edit after the answer, and a crash and restart (after which the lost session's answer is sent).
- Barrier flags: two answers A1, A2 (R = A2); a batch request; undo of A2 succeeds; undo of A1 is refused with `UploadPending` even when it is submitted before the in-flight marks commit; A1 becomes `Uploaded` at the ack; a lost result leaves it not undoable.
- Incoming change to a card of R: R is dropped and joins the detached hold; that card is deferred and the run's other rows are sent as before-images until `SessionEnded`; then all are sent live. An incoming change to an unrelated card leaves R.
- Incoming-held set (P8.3): answer #c1, a Browse edit of #n, then another device's edit of #n is applied (R dropped, #n in the set); then answer #c2, whose full update clears the other deferrals: the next barriers send nothing of #n, and no record of #n is built, until `SessionEnded`, after which #n is sent live with the other device's edit kept. The same with a second incoming change to #n while it is in the detached hold, and with a `CloudRekey` of an entity of R: neither its old nor its new key is sent before `SessionEnded`.
- Save conflict on a held before-image: the merge writes with L = the live row, so the local card keeps the answer; the card is deferred until the session ends.
- Flush timeout: S not covered within 1 s gives no record, and the next request sends.
- Sync Now: an edit committed just before the press, not yet seen by the observer, is selected; an entity the step 1 scan finds pending is selected; boundary with edits made during the exchange (they need no ack, the result is `succeeded`); an entity edited again after the capture is satisfied by the newer ack; a held image acked after a session started mid-exchange counts done and held back; partial save failure after a successful API return (`incomplete`, `exchangeIncomplete`, nothing shown as done, AE91); three conflict rounds; a throttled send with a retry-after of 10 s and of 120 s both end `waiting{throttled}` at once, with no app wait or resend; gate closed by a scan for more than 30 s (`waiting`); cancel mid-send; account switch mid-exchange (`failed{accountChanged}`, late callbacks refused); media paused (collection `succeeded`, media `paused`); result precedence. Token: Sync Now during a tracking stop whose probe succeeds continues on the new instance and ends `succeeded`; Sync Now pressed while the launch scan runs waits at step 1 and continues after the `StartEngine` that follows the scan; turning auto off mid-exchange lets it finish, and `ConfigureAutomatic` applies after `ExchangeEnded`; a fresh instance's `signIn` with the binding's identity changes no token and fails no exchange or pending update. A `file_missing`, `withheld` or `superseded` media name is not selected and does not make the result `incomplete`. Early stops (P12.7): an exchange that ends at step 1 after 30 s, and one cancelled before its capture, each emit `RecordExchange`, write the last attempt and run `checkRollover()` once; one ended by `CollectionReplaced` writes nothing and runs no check, and the new generation's first check runs.
- Quota: after `quotaExceeded` the status shows `quotaFull`; the failed collection and blob names are parked; the quota error emits no action of its own and no `UpdatePending`; 50 `TrackingChanged{cause: cloud}` inputs (save results, acknowledgements, barrier results) hand nothing back and arm no timer; one `Foreground`, one Sync Now (step 4), one `Account` input, one `FetchDrained`, one local change each hand every parked name back; Sync Now after the user freed storage sends the parked names and ends `succeeded`; fetches continue; no timer other than P10.2's flush timer is armed; a send that ends `ok` with `saved` above 0 clears the item, and a send that ends `failed` keeps it. A one-record `limitExceeded` and an asset error are parked the same way. A due deletion (P10.3) that failed so is handed back by the next full update.
- Deferrals: answer #c1 (A1), barrier with R = A1 (#c1 deferred), answer #c2 (A2), barrier with R = A2: the full update of A2's local change hands #c1 back and the second barrier sends #c1 and A1's review row during the session. An undo of A2 makes A1 the newest answer again, and #c1 is deferred again at the next barrier. At `SessionEnded` every deferred entity is sent live.
- Confirmed root: with auto on at launch, an engine-scheduled send before the root fetch drains gets no record; the `FetchDrained{root}` of that instance emits `UpdatePending{full}` and `RequestSend{collection}`, and the offline edits are sent. A `RequestSend` that follows a `RequestFetch` in one action list starts only after that fetch returns. Unchanged zones (P3 `FetchDrained`): after a relaunch from saved state, with `MGActive` unchanged for weeks and no change in the collection zone, the opening `RequestFetch{root, collection}` gives `FetchDrained{root, fetched: false}` and `FetchDrained{collection, fetched: false}`; the epoch is confirmed from the recorded `MGActive`, local reviews are sent, and `RetryAssetEntries`, `RunCleanup`, the read round and the quiet point run. The same after `ConfigureAutomatic` recreates the engine and after a tracking stop clears. A binding that records no `MGActive` reads it by record ID first. An instance with no saved state whose bound zone, or `meguri-root`, no longer exists gets no `FetchDrained` for it and reaches K12.5's row; no absence pass completes on a `fetched: false` drain, so nothing is deleted locally.
- Engine outcomes: `EngineError` before `EngineActivity{ended, failed}` of one operation counts one failing cycle; `ended{ok}` of the same (zone, kind) resets it; `ended{ok}` of a send or fetch clears `iCloudUnavailable{temporary}`; a `read` round's `ended` counts and resets only kind `read`.
- Tracking failure: the engine stops without saving its state; `Foreground`, `SyncNowRequested`, an account change and `VolumeSpaceAvailable` each emit `ProbeTracking`; a successful probe or a local change's `TrackingChanged` restarts it; a failed probe stops it again, and nothing else restarts it. With a failure that repeats on every commit, 200 answers in one foreground period restart the engine at most once from `TrackingChanged`. A full disk during a reconcile scan: the scan does not restart by itself; when the stop clears, it runs once before `StartEngine`.
- Errors: `notAuthenticated` and `accountTemporarilyUnavailable` from an engine fetch while the account stays `available` emit no `StopEngine` and show `iCloudUnavailable{temporary}` until a send or fetch ends `ok`; failing cycles count per (zone, kind), so two zones failing once each show no `cloudError`; one `invalidArguments` shows `cloudError` at once, emits no `UpdatePending`, parks its names, hands them back only at the next event trigger, and logs the request's scope and record names; the item clears at an `ended{ok}` of the same (zone, kind). `crt` check (P6): a local op that changes `col.crt`, and a fetched `MGEpoch` with another `crt`, each close the gate with `crtMismatch`, the engine running; no record is built and no apply runs, also when a barrier or an apply was already requested; a relaunch finds it again at open; an undo that restores `col.crt` clears it; Sync Now reports `recoveryRequired`.
- Cleanup: `RunCleanup` runs at the first `FetchDrained{collection}` with the gate open and then at most once per 24 h; it never runs with the gate closed; tombstones older than 180 days become record deletions only while A5 and A9b hold, so none does while A9b is taken as false; no blob deletion while media is off or another epoch zone is retained; a due mark whose condition stops holding is cleared without a deletion; a failed deletion stays due.
- Failed applies (K13.2): an entry that takes `apply_failed`; 50 `TrackingChanged` inputs emit no `ApplyReady{retryFailed}`; the next event trigger (a local change, `Foreground`, Sync Now step 3, a `FetchDrained`) retries it in an op of its own after the ready entries; Sync Now ends `incomplete` while it waits; after the fifth failure `applyFailed` is shown, and a failed re-key shows `rekeyFailed`.
- Stale blob state (K9.3): a blob cached `present` that another device deleted is read again after a local change names its content anew, found absent and uploaded, and no device ends with `mediaMissing`; `media_changed` survives a relaunch and clears at the next feed item of the name.
- Volume space: `VolumeSpaceAvailable` is reported only just before `Foreground`, `SyncNowRequested` or `FetchDrained`, never from a poll.
- Status: each P14.2 row and each attention item, `mediaMissing` included; a salvage binding (`tracking_damaged`) with iCloud enabled and no other binding gives `needsAttention{chooseCollection}`, no `isolated` item and Sync Now `notConnected`; a `superseded` media name is not pending for the policy and counts in `mediaMissing` when the media zone has no blob (a file deleted outside Meguri with no blob: `mediaMissing`, then `upToDate` apart from it; with a blob: a download, no tombstone); held-back and media reported apart from success; `upToDate` during a session with held entities; no read of `col` stamps (a test that changes `col.mod`, `col.scm` and `col.ls` alone changes no status).
- Media: collection-first order, blob uploads not held off by the flush timer, visible-card priority, the concurrency, staging and free-space bounds, the free-space resume, the clone mismatch. `RequestFetch` never names `meguri-media` or another epoch's zone; `ResumeMedia` reads `MGMediaRoot` before any blob work, also when media already runs; a `root` read that returns another key than the acked one, unknown item or zone not found sets `mediaZoneMissing`, and no blob is read, uploaded or downloaded afterwards, also at later `ResumeMedia` inputs; a blob is uploaded only after a read found it absent; with auto off, media starts only from Sync Now step 7; with auto off and media on, a new picture's blob is read and uploaded by the next Sync Now (step 7's read round and `RequestSend{media}`), also when media already runs; with auto on, a local change that adds a blob in state `unknown` starts one read round, and a second trigger during it adds none; a read round names at most 200 record IDs per operation.
- Background: the flush ends by `until`; expiry leaves acked records acked and the rest pending; a running exchange with auto off gets the flush and ends `cancelled` at `until`.

U114 `CloudKitSyncTests`, with a fake engine: a late callback with a stale guard or an old instance id is refused; the fetch scope of every engine fetch, scheduled or requested, is `meguri-root` and the bound collection zone; blob reads name `e` alone, or `e` and `a` in the download queue; no `sendChanges` or `fetchChanges` runs from event handling; a batch request while the core is suspended returns no record and fetched events wait unsaved; an asset file that the system deletes after the handler returned does not prevent a queued record from applying at `resume()`, because the actor staged it while handling the event (P11.2); at most 2 asset downloads run; staging stays at or below 256 MiB; a clone whose SHA-256 differs is dropped; the actor holds no retry or quota timer (V3.3, V10.3). Engine lifetime (P4): while the old instance's `cancelOperations` completion is pending, after a `StopEngine`, a `ConfigureAutomatic` that recreates the engine and a tracking-stop restart, no second engine object exists, and an old operation that completes later changes nothing. `FetchDrained` (P3): a `fetchChanges` call that returns with no error gives one input per scope zone with the right `fetched` flag; an engine-scheduled fetch gives inputs only for the zones it fetched; a zone fetch with an error gives none. `configuration` (P3): `missingEntitlement`, `badContainer`, `permissionFailure`, `managedAccountRestricted` and `badDatabase`, from the account status read and from an engine operation, each report `Account{configuration}`, and a later read that returns `available` reports `available`.

U151 `SyncCoordinatorTests` with `FakeCore` check the call orders of P11.1, P11.2 and P11.4, one adapter request per action, one timer owner, the quit flush with a running exchange and auto off, and that a fence or Pause makes Sync Now unavailable. Signed multi-device runs (R2064, R2065) prove Apple's delivery; fake runs alone do not. They also check: an engine fetch with a restricted scope delivers database changes for zones outside it (a deleted `meguri-media`); a record-ID read with named fields downloads no other asset (A4); a fresh instance's first account event (A12); that the engine retries `notAuthenticated` and `accountTemporarilyUnavailable` itself; and that a first fetch from no cursor fetches every existing zone of its scope and none that is gone, while a fetch from saved state with no change reports no zone event (P3 `FetchDrained`, A7).

## P22. Decisions for review

- R includes steps of no session above the session's newest answer (a Browse or editor edit during the session), so they wait for the next answer or the session's end. This follows R2047, R88 and R365; KTD6's wording ("the active session's contiguous top-of-stack steps") covers only the session's own steps. The alternative is that an outside edit ends hold-back, which publishes the newest answer.
- An incoming change that drops R moves it to the detached hold: its rows stay held until the session ends, and the entities the change wrote that have a row in R join the incoming-held set, deferred until the session ends. The answer is then no longer undoable (R2047: incoming conflicts invalidate affected undo entries). Decided 2026-10-27: an incoming change cuts the history below the newest step it touches (`engine/undo#M9.8`, as M7); safety comes before undo depth.
- Undo after such a drop reports the remaining history's reason; there is no new reason.
- Barrier flags cover only the steps that meet the batch request (P8.2): an undo step becomes non-undoable when a record built after it is handed, even if that save then fails.
- The foreground cloud flush target (60 s) covers the whole foreground, not only review sessions.
- Bounds: background flush ends at the earlier of the deadline − 5 s and 25 s; Mac quit flush 10 s; Sync Now waits at most 30 s for the gate and runs at most 3 send rounds. Decided 2026-10-27: Sync Now never waits out a retry-after; a throttled exchange ends `waiting` at once (P12.5, V10.3).
- Media: 2 concurrent downloads, 256 MiB staging, 512 MiB free-space reserve, upload batches of 8 blobs or 128 MiB.
- Sync Now stays available with auto off, while offline, while media is paused, after `quotaExceeded` and during a tracking stop (it probes tracking once, P15.2).
- Decided 2026-10-27: `checkRollover()` runs once after every Sync Now exchange, whatever its result, early stops included (P12.7); the check is idempotent.
- Decided 2026-10-27: a deferral holds only until the next full update, which every move of R causes (P7.2); the cost is one more barrier decision per full update.
- Decided 2026-10-27: an `EngineActivity` that ends carries its outcome and, for a send, the saved count; U114 reports an operation's errors before its end. The transient authentication state, the failing-cycle count and `quotaFull` clear on these fields (P3, P14.2, P15, P15.1).
- Decided 2026-10-27: a drained root fetch of a new engine instance asks for a send, because the core refuses collection batches until that fetch confirms the epoch (V10.2, P10.1); a `RequestSend` after a `RequestFetch` waits for the fetch to return (P4).
- Decided 2026-10-27: blobs in state `unknown` are read in rounds of at most 200 record IDs, at `ResumeMedia`, and with auto on at a drained collection fetch and a local change; Sync Now's media step always runs `ResumeMedia`, whose `RequestSend{media}` uploads ready blobs also with auto off (P13.1, P13.2).
- Decided 2026-10-27: the actor stages the asset files of a fetched event before its handler returns, also while the core is suspended (P11.2).
- Decided 2026-10-27: a V8.1 salvage binding is not a binding for the gate, status or Sync Now (P1, V11): iCloud enabled with only a salvage binding is `chooseCollection` and `notConnected`.
- Entities changed during an exchange may be sent by it; they are not selected.
- Blocked entities (oversize, unreadable and so on) and fetched entries waiting for `format` or `asset` make a Sync Now result `incomplete`. An `incomplete` result shows `needsAttention{exchangeIncomplete}` (AE91).
- Inbox entries that wait for another device's work (`generation`, `reference`, `structure`) do not prevent `succeeded`.
- The `conflicts` attention item follows V11 and V17.2: unresolved conflict copies keep it, and so needs attention, until each is resolved. Decided 2026-10-27: opening Review Conflicts does not clear it.
- Held entities do not prevent `upToDate`; `asOf` is the later of the last completed exchange and the last quiet point (R1558 says "as of a completed exchange").
- A pending publication decision is `paused`; an isolated binding is `needsAttention`; a binding the user stopped is `localOnly`. Decided 2026-10-27: when every binding other than a salvage binding is of another container or environment (`environment_changed`), the state is `localOnly` and Sync Now reports `notConnected` (P12.1 row 3, P14.2 row 1, P17).
- A step handed to CloudKit never becomes undoable again, even when its save fails.
- After a crash or OS termination with a session open, the newest answer is sent at the next launch: its undo is gone.
- The Mac quit flush and the background flush run with auto on, and also with auto off while an exchange runs.
- Decided 2026-10-27: Meguri holds no retry timer and no quota pause; CKSyncEngine owns scheduling and backoff. After a quota or tracking failure the policy acts again only at event triggers (P2, P15.1, P15.2). The engine drops names that failed with an error it does not keep (K12.4); they are parked and only `UpdatePending{full}` hands them back, at the event triggers, while the update after a save result or another non-local commit (`UpdatePending{changes}`) never does, so a failure cannot loop (P4). U151's flush timer stays the only app timer (plan U113: "A single app flush deadline covers the foreground target only"). The exchange's gate deadline and the background and quit `until` only end their own operation and are not timers in this sense; retries of record reads run at event triggers (P2).
- Decided 2026-10-27: the R1583 download estimate uses the throughput of the last 60 s (P14.4). A tracking stop is retried with one probe commit (`ProbeTracking`, P19) at each trigger, because the pure `step` cannot write.
- Decided 2026-10-27: a `file_missing`, `withheld` or `superseded` media name is not pending for the policy (P3): it never keeps `changesPending`, never makes an exchange `incomplete`, and a `file_missing` or `superseded` name with no blob shows `mediaMissing` (P14.4). Only a feed item with absence cause `removed` makes a media-name tombstone (P13.1, J6).
- Decided 2026-10-27: the policy token does not change on `StartEngine` or `ConfigureAutomatic`, nor on a `signIn` with the binding's identity; `ConfigureAutomatic` during an exchange applies after it (P4, P3 `Account`).
- Decided 2026-10-27: the engine fetches only `meguri-root` and the bound collection zone; media-zone records are read by record ID with named fields (P13.2), because an engine fetch downloads every asset.
- Decided 2026-10-27: time-based cleanups run from `RunCleanup` at a drained collection fetch, at most once per 24 h (P10.3).
- Decided 2026-10-27: transient authentication codes from an engine operation stop nothing (P15); a tracking stop is cleared by a local change at most once per foreground period (P15.2); `VolumeSpaceAvailable` is read at existing triggers, never polled (P3).
- Decided 2026-10-27: an entity that an incoming change or a re-key wrote while one of its rows was a row in R or the detached hold stays deferred until `SessionEnded`, through every full update, and so does a re-key's new key (P7.2, P8.3). Its before-images are older than the server version it now holds, so sending one would overwrite another device's change; holding it costs only latency until the session ends.
- Decided 2026-10-27: `ResumeMedia` reads `MGMediaRoot` at every run, also when media already runs, and another key, unknown item or zone not found stops media for the binding for good (P13.1, K9.2).
- Decided 2026-10-27: a `crt` mismatch closes the gate with the engine running, is found again from durable values at every open, and is checked again at every barrier and before every apply; it isolates nothing and clears when the values are equal again; otherwise V14 or V15 resolves it (P6).
- Decided 2026-10-27: `invalidArguments` is a programming error: `cloudError` at once, names parked, no immediate resend (P15, K12.3).
- Decided 2026-10-27: every `ExchangeEnded` records the exchange and runs `checkRollover()` once, early stops included; an exchange ended by a close or a replacement records nothing, and the next open's check runs (P12.7).
- Decided 2026-10-27: `FetchDrained` covers every zone of a returned `fetchChanges` call, also one with nothing to fetch (`fetched: false`), because the engine reports no event for an unchanged zone. An unchanged root zone confirms the epoch from the recorded `MGActive`. An instance with no saved state treats a zone its first fetch did not list as not found, and an absence pass completes only on a drain that fetched the zone, so a vanished zone never makes local deletions (P3, P10.1).
- Decided 2026-10-27: `StopEngine` awaits the engine's `cancelOperations` before it releases the object, and no new engine object is created before the old one's `EngineStopped`, because production allows one engine per database (P4, V3.4).
- Decided 2026-10-27: `configuration` is a status U114 derives from setup errors of the account read and of operations; it holds until a later account read returns another status (P3, P15).
- Decided 2026-10-27 (R2066 approval): an entry with reason `apply_failed` is retried only at an event trigger, through `ApplyReady{retryFailed}`, one entry per op, and keeps a Sync Now result from `succeeded` (P4, P12.4, K13.2); `applyFailed` shows after its fifth failure (P14.4).
- Decided 2026-10-27 (R2066 approval): the engine's send path carries only entities, generation manifests and blobs, and the deletions `RunCleanup` marks due. Zones, `MGEpoch`, `MGActive`, `MGMediaRoot` and an `abandoned` manifest are U114's own operations (V26.1), so the only control records the update forms keep are the manifests (K12.6).
- Explanation strings of P12.1 are draft wording.

## P23. Changes other specs need

This file does not change them; the owners do. Implemented units are named with the impact. Items marked **Done** have landed where they say (checked 2026-10-27, R2066 approval pass). No item is open.

- **Done.** `engine/undo` (U25, implemented): M8.1 (`cloud_held_run`) and M9.6 to M9.10 (flags, acknowledgement, targeted drop, tests). U25 adds them; the manual-transfer path is unchanged.
- **Done.** `sched/review-session` C15 (U43, implemented): the CloudKit hold-back reads the session id (P8). Wording only; no code change.
- **Done.** `sched/day-clock` D8 (U40, implemented) and `sched/app-coordination` A1, F1 (U51): `checkRollover()` runs before a `CloudIncoming` apply (K13.2, P4 `ApplyReady`) and once after every Sync Now exchange, whatever its result, early stops included (P12.7). U40 needs no code change until U113 and U114 land.
- **Done.** `engine/lifecycle` L9 (U24, implemented), with L4, L5, L6 and L11.
- **Done.** `storage/open-and-connections` C6 and C9.2 (U22, U24, U32): the `cloud` reader for barrier snapshots (P7.1), separate from the sync reader that manual transfers keep, and the covered seq with a wait for it.
- **Done.** `media/media-journal` J6: a `missing` item, a removal item for dropped facts included, is never a deletion signal for CloudKit (P13.1, K9.4).
- **Done.** `sync/hold-back`, `sync/meta-status`, `sync/errors`, `sync/transport`: their headers no longer claim the CloudKit requirements (R88, R726, R1554, R1558, R1559, R1561, R1555, R1556).
- **Done.** `engine/op-model` O1.1 and `engine/change-stream` S2.4: a `CloudIncoming` op that rolls back other than by a cancel gives its entries `apply_failed`, retried only at event triggers (P4 `ApplyReady{retryFailed}`); the core records the op's seq also for an empty change set, and the observer marks no key of the intent dirty.
- **Done.** `sync/cloudkit-records`: K9.3, K10.3 and K12.1 cite P13.2, P19 and P7; K10.3 and K12.1 keep each in-flight entry's image seq and held flag (P7.2, P12.4); K11.4, K12.3 and K13.2 take a new `change_seq` when they make an entity pending (P12.2); K9.3's header names R1583, R1584, R1588 and R1844. K9.2 reads `root` again at each media start (P13.1). K9.3 and K23 A4: false for the engine's fetch, relied on only for record-ID reads with named fields; blob existence from those reads (K9.3, K9.5). K10.3: the `withheld` flag, the due marks of P10.3, the `zones` rows with their first-seen instants, and the binding's epoch `crt` (P6). K12.4: the full update, its event triggers, the due deletions and the names added back at once or only at an event trigger. K12.3: the one-record `limitExceeded` row and the `invalidArguments` row (P15). K2, K12.1 rule 3 and K21 invariant 20: the confirmed root, by the `MGActive` a drain delivered or the binding's last applied one, with a record-ID read when the binding records none (P10.1). K13.1 step 2: the last applied `MGActive`, the epoch `crt`, and an asset the actor staged while the core was suspended (P11.2). K13.1 and K23 A7: `FetchDrained` per scope zone (P3). K13.1: an `asset` entry is read again only at `RetryAssetEntries`. K17.4: an absence pass completes only at `FetchDrained{collection, fetched: true}`, and a zone missing from a no-state instance's first fetch is K12.5's row. K17.4, K18.5, K9.5 cite P10.3. K17.4 and K23: A9 split into A9a and A9b, with purging gated on A5 and A9b (P10.3). K20 and K21 invariant 26: the crt check (P6). K20 row 1: the fetch scope. K22.2: unchanged zones after a relaunch, and a no-state instance whose bound zone is gone. K24: its `sync/policy` items are marked done.
- **Done.** `sync/cloudkit-recovery`: V3.3 and V10.3: no retry timer and no quota pause (P2, P15.1, P15.2); both stay as they are, apart from the A9b citations of V10.3. V24 A13: false for quota exceeded. V9.1: the engine `signIn` with the binding's identity, with A12, and the derived `configuration` status (P3). V10.3: the transient authentication row with no instance stop, and the throttling row (P12.5). V11: the transient authentication reason, `mediaMissing`, `applyFailed`, `rekeyFailed` and `crtMismatch` rows. V14.1 and V15.1: offered for a `crt` mismatch (P6); V2.4 step 4: the crt check at open. V3.4: `StopEngine` awaits `cancelOperations`, and no new instance exists before `EngineStopped` (P4). V10.2: the confirmed root by the last applied `MGActive`, and a zone missing from a no-state instance's first fetch (P3, P10.1). V18: a drain with `fetched` false counts. V3.1 rule 5: A9a. V17.3: cites P10.3. V23.2: the unchanged root, the vanished zone, own operations of a publication and the crt mismatch. V25: no item open. V25's `sync/policy` items: P3 `superseded`, P1 no binding, P12.1 row 3, P14.2 row 1 and P17 for `environment_changed`, P14.4 `isolated`, P15 `changeTokenExpired`, P10.3 item 4.
- Plan (not a spec): U113's Files name `core/crates/meguri-sync/src/policy/` and `tests/policy.rs`; on master `src/policy/` holds manual-transfer history. The CloudKit policy goes to `src/cloud/policy/` and `tests/cloud_policy.rs` (P2). KTD6's held-run wording (P22, first item).

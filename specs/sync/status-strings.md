# iCloud sync strings

> Availability changed 2026-11-27: `sync/meguri-provider#MP0` and
> `native-interface-contract#N28` supersede iCloud choices, copy, native
> controls, account and transport requirements. Those clauses are retired;
> Meguri Sync, Off and shared collection/media/hold-back requirements remain.

Spec: sync/status-strings
Requirements: R89, R102, R103, R104, R105, R106, R107, R117, R122, R123, R124, R125, R127, R158, R166, R1548, R1551, R1555, R1558, R1559, R1560, R1561, R1562, R1583, R1592, R2046, R2048, R2049, R2050, R2054, R2055, R2056, R2058, R2059
Anki: none (Meguri design)
Scenarios: none (Meguri's Swift tests: T16)

The user-visible text of routine iCloud sync: the sync button, the status popover, the Sync Now command and its results, the attention items, the error classes, the recovery choices and their confirmations, Account & Sync's iCloud rows and the Mac quit sheet. U151's `SyncStatusModel` and U161 take every such string only from this file (KTD1); U166's connect entry and U153's damaged-collection remedy reuse the T12 and T13 keys for the same flows (changed 2026-11-20: Welcome has no connect entry, `docs/design/native-interface-contract.md` N18.2; its Set Up Sync opens Account & Sync, whose connect control is T12's, until T19 ships and it opens the Sync section). It fixes the draft wording that `sync/policy#P24` left open: P12.1's explanation texts are replaced by T6 under the same keys.

The states, items, results and choices are those of `sync/policy` (P12, P14, P15) and `sync/cloudkit-recovery` (V9 to V21); this file adds no state and changes no condition. Manual Anki transfers (Settings Anki Transfers, `sync/anki-transfers`) are frozen and have their own text; none of it is here (R1560).

T18 gives the text of the second service, Meguri Sync (`sync/meguri-provider` MP2–MP5; `docs/design/native-interface-contract.md` N12.4), and of the Sync Service choice in Account & Sync. T2 to T15 stay the text of iCloud Sync, unchanged.

T19 (2026-11-19) gives the text of the Sync section that replaces Account & Sync: one Service control, setup without a Connect step, the sheet that asks which collection to keep, problems in three kinds, and Troubleshooting. It adds keys and lists the keys of T3 to T18 that become unused (T19.8); it changes the value of no existing key.

T20 (2026-11-21, round 10) gives the text of the status panel that the sync button opens, for both services (`docs/design/native-interface-contract.md` N23). It adds three keys and changes no state or existing value.

The lines and rows marked "changed 2026-11-21 (resume fix)" in T19.1, T19.2, T19.5, T19.7, T19.8 and T20.1 give the text of `sync/meguri-provider#MP6.5`'s paused setup media step and of the Sync section's one status row (`docs/design/native-interface-contract.md` N15.2). In force: decided by Claude (coordinator) under Antoine's delegation of 2026-11-21 and his permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/sync-media-resume-20261121.md`).

The T19.5 row marked "added 2026-11-22 (structure upload)" gives the two texts of `sync/meguri-provider#MP7` step 5 that U269 built in PR #112 (`MeguriStructureSaveTests` checks the row). In force: decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/stats-spec-approval-20261122.md`).

## T1. Keys, arguments and plurals

- Every string is an entry of the Sync String Catalog with the stable key given here. The English value is the one given; other languages are Meguri's own translations of the English. Keys never change once shipped; a changed meaning gets a new key.
- Decided 2026-10-27: the Sync String Catalog is the one file `app/Packages/MeguriKit/Sources/AppModel/Sync/Resources/Sync.xcstrings`, the table `Sync` of the AppModel target's resource bundle (U151 adds it; U161 adds the keys its views show). U161's views in the Features target read it through AppModel's bundle, which AppModel exposes publicly; no other target holds a copy of any key here.
- "Followed by" (T8, T11.2, T13.7) means a separate line directly under the entry, never one string made by joining the two. A kinds or reasons breakdown (T11.3) is one line of its own: the T11.3 count entries joined with the T1 list format.
- `%lld` is an integer argument, `%@` a text argument; with two arguments they are numbered (`%1$lld`, `%2$@`). A plural entry has the catalog's plural variations on its integer argument: English uses **one** for exactly 1 and **other** for every other value, 0 included; other languages follow their CLDR rules. The tables write a plural entry as `one` / `other`.
- A **Mac:** value is the catalog's Mac device variation of the entry; iPhone and iPad use the main value.
- Text arguments are formatted by the system, never by hand:
  - an instant: the relative date ("today", "yesterday") within a day of now, else the medium date, then the short time, with middle-of-sentence capitalization ("today at 10:42"), in the user's locale;
  - a date without time (V14.1's previous collection): the medium date;
  - a byte count: the system byte-count format, file style ("1.2 GB");
  - a duration: the system duration format, full unit names, one unit, rounded up (a retry-after of 70 s is "2 minutes", of 20 s "1 minute");
  - a list of counts (T11): the system list format ("3 notes, 2 cards and 1 deck").
- Decided 2026-10-27: control labels (buttons, menu items, toggles, sheet and alert titles, status titles) are in title case, as Apple's are; messages and detail lines are in sentence case, and end with a period when they are sentences. Apostrophes and quotation marks are typographic (’ “ ”). A label that opens another step before anything changes ends with "…".
- Decided 2026-10-27: places are named as the user sees them. Meguri's own settings section is "Account & Sync"; the system's are "Settings" (iPhone, iPad) and "System Settings" (Mac). Meguri never says "Settings" alone for its own screen, because on iPhone and iPad that is the system app.
- Words never used in this text: record, zone, epoch, binding, tombstone, entity, engine, CloudKit, schema, token, quota (say "iCloud storage"). Nothing says or implies that other devices have the changes (R2050, R107).
- VoiceOver reads the visible text unless the tables give an accessibility label. Status changes are not announced, except the end of a Sync Now the user started (T7) and the Mac quit sheet (T15); background failures change only the button and the popover (R166). No string here is shown in a modal that blocks study (R1562).

## T2. Where the text appears

| Surface | Text |
|---|---|
| sync button: Home toolbar or navigation bar (R102), and the review corner icon (`docs/design/native-interface-contract.md` N3, N11.1) | the symbol of the state (U161's screen contract, R104) with `sync.button.label` as its accessibility label and the T3 title as its accessibility value; on the Mac, the T3 title and detail as its help tag |
| status popover (R105, R1560), opened by the button | the T3 title and detail, then T4's lines, the running or last Sync Now (T7), the T9 attention rows with their actions, and Sync Now (T6) |
| media indicator (R106), quiet and secondary, beside the button while blobs transfer | `sync.media.filesLeft` |
| Sync Now: the menu command, the popover button, the `Y` key (R1551) | T6 |
| Account & Sync (R117) | T14, with the T3 title and detail and T4's lines |
| sheets and alerts of the recovery flows | T10 to T13 |
| Mac quit | T15 |

| Key | English |
|---|---|
| `sync.button.label` | iCloud Sync |

## T3. Status (R1558, R103, R104; `sync/policy#P14.2`)

One title and one detail per state, from `SyncStatus.state` and its reason fields (P14.4 `unavailableReason`, `pausedReason`).

| State | Title key, English | Detail key, English |
|---|---|---|
| `localOnly` | `sync.status.localOnly.title`: iCloud Off | `sync.status.localOnly.detail`: This collection is only on this device. |
| `iCloudUnavailable{noAccount}` | `sync.status.unavailable.title`: iCloud Unavailable | `sync.status.unavailable.noAccount`: Sign in to iCloud in Settings to sync this collection. **Mac:** Sign in to iCloud in System Settings to sync this collection. |
| `iCloudUnavailable{restricted}` | `sync.status.unavailable.title` | `sync.status.unavailable.restricted`: iCloud is restricted on this device, so this collection can’t sync. |
| `iCloudUnavailable{temporary}` | `sync.status.unavailable.title` | `sync.status.unavailable.temporary`: iCloud isn’t available right now. Syncing continues when it’s available again. |
| `iCloudUnavailable{setupNeeded}` | `sync.status.unavailable.title` | `sync.status.unavailable.setupNeeded`: Meguri can’t use iCloud on this device. |
| `paused{decisionRequired}` | `sync.status.paused.title`: Paused | the T10 prompt for the pending decision's reason: `sync.decision.title.restore`, `.import` or `.none` |
| `needsAttention` | `sync.status.needsAttention.title`: Needs Attention | one item: that item's T9 text. Several: `sync.status.needsAttention.count`: `%lld issue needs your attention.` / `%lld issues need your attention.` |
| `paused{userPaused}` | `sync.status.paused.title` | `sync.status.paused.user`: iCloud sync is paused. |
| `paused{automaticOff}` | `sync.status.automaticOff.title`: Automatic Sync Off | `sync.status.automaticOff.detail`: Use Sync Now to sync this collection. |
| `offline` | `sync.status.offline.title`: Offline | `sync.status.offline.detail`: Changes sync when you’re back online. |
| `syncingCollection` | `sync.status.syncing.title`: Syncing… | `sync.status.syncing.detail`: Syncing your collection with iCloud. |
| `syncingMedia` | `sync.status.syncingMedia.title`: Syncing Media… | `sync.media.filesLeft` (T5) |
| `changesPending` | `sync.status.pending.title`: Waiting to Sync | `sync.status.pending.detail`: Some changes have not synced yet. |
| `upToDate{asOf}` | `sync.status.upToDate.title`: Up to Date | `sync.status.upToDate.detail`: `Last synced %@.` with `asOf` (T1 instant). No detail when no instant is recorded |

- The accessibility value is the title without its "…". The `needsAttention` count counts attention items, not their entities.
- The states `iCloudUnavailable`, `paused` (every reason), `offline` and `needsAttention` add, in the popover only, `sync.status.keptLocally`: "Your changes are saved on this device." (R2046), unless an attention item `collectionCorrupt`, `localStorageFull` or `trackingError` is present, whatever the P14.2 state: then the collection may not be saving, and the line would not be true. Decided 2026-10-27: the exception does not depend on the state, because P14.2 rows 2 and 3 come before row 4, so these items can be present in `iCloudUnavailable` and `paused{decisionRequired}`.
- Decided 2026-10-27: `paused{automaticOff}` has its own title, because Sync Now still works then (`sync/policy#P12.1`) and "Paused" would say otherwise. It is the `paused` state for the symbol and the tests.
- Decided 2026-10-27: no state text says "synced" about the changes of other devices. `upToDate` names this device's last confirmed sync (R107, R2050); it is "Last synced", also when `asOf` is a quiet point (`sync/policy#P14.3`).
- Decided 2026-10-27: `iCloudUnavailable` offers no button. Apple gives no public way to open the account pane, so the guidance is the text (R102); Meguri never asks for an Apple password (R117).

## T4. Popover lines (R105, R1559; `sync/policy#P14.4`)

Shown under the title and detail, each only when its condition holds, in this order.

| Key | English | Shown when |
|---|---|---|
| `sync.details.toSend` | `%lld change to send` / `%lld changes to send` | `pending` > 0 |
| `sync.details.heldBack` | Your latest answer syncs when you finish reviewing, so you can still undo it. | `heldBack` > 0 (R1559) |
| `sync.details.incoming` | `%lld change from iCloud waiting to be applied` / `%lld changes from iCloud waiting to be applied` | `incomingWaiting` > 0 |
| `sync.details.provisional` | Updating counts… | a reconcile scan runs: the counts are provisional (P14.4) |
| `sync.details.lastSync` | `Last synced %@` | a last completed exchange is recorded (P12.7) and the state is not `upToDate` |
| `sync.details.lastAttempt` | `Last Sync Now: %1$@, %2$@` | the last exchange attempt (P12.7) is newer than the last completed exchange; `%1$@` is its result title (T7), `%2$@` its instant |
| `sync.details.error` | the T8 line of the current `error` class | `error` is set and no T9 row already says it |

- Counts here are of collection changes; media has its own lines (T5). A completed exchange still shows held, newer, waiting and media lines (R1559).

## T5. Media (R106, R122, R124, R1583; `sync/policy#P13`)

| Key | English | Shown when |
|---|---|---|
| `sync.media.filesLeft` | `Syncing media, %lld file left` / `Syncing media, %lld files left` | a blob upload or download runs; the count is the files waiting and running in both directions (R106) |
| `sync.media.sizeLeft` | `%@ left` | with the line above, bytes waiting and running (T1 byte count) |
| `sync.media.timeLeft` | `About %@ remaining` | downloads run and P14.4 has an estimate (R1583) |
| `sync.media.waiting` | `%lld media file waiting to sync` / `%lld media files waiting to sync` | media is on, files wait and none transfers (offline, paused, auto off) |
| `sync.media.off` | Audio and image sync is off. | the media setting is off (R122); no other media line shows |
| `sync.media.noSpace` | Media downloads are paused until this device has more free space. | downloads paused by P13.2's free-space rule (the media no-space flag, below) |

- The media indicator shows `sync.media.filesLeft` alone. Its accessibility label is that text; it is never announced.
- Decided 2026-10-27: the media free-space pause of P13.2 is not the `localStorageFull` attention item. Collection sync goes on, so it shows only `sync.media.noSpace`: it changes neither the T3 state nor T3's `sync.status.keptLocally` line, adds no T9 row and sets no T8 error. U151 reads it from a media no-space flag in P14.4's `media` field, which `sync/policy` must add; P13.2's "status reports `localStorageFull` for media" means this flag.

## T6. Sync Now (R89, R105, R2048; `sync/policy#P12.1`)

| Key | English | Use |
|---|---|---|
| `sync.now.command` | Sync Now | the menu command and the popover button, shortcut `Y` |
| `sync.now.cancel` | Cancel | stops the running exchange (`SyncNowCancelled`) |

While Sync Now is unavailable, the command and the button are disabled, and the reason's text is the Mac help tag and, on iPhone and iPad, a line under the disabled button in the popover (R89). The keys are P12.1's; this table is their English.

| P12.1 reason | Key | English |
|---|---|---|
| `collectionUnavailable` | `sync.now.unavailable.collectionUnavailable` | Sync Now is available once the collection is ready. |
| `reviewActive`, automatic sync running (below) | `sync.now.unavailable.reviewActive` | iCloud keeps syncing automatically while you review. Sync Now is available when you finish reviewing. |
| `reviewActive`, otherwise | `sync.now.unavailable.reviewActive.manual` | Sync Now is available when you finish reviewing. |
| `notConnected` | `sync.now.unavailable.notConnected` | This collection doesn’t sync with iCloud. You can turn on syncing in Account & Sync. |
| `paused` | `sync.now.unavailable.paused` | iCloud sync is paused. Resume it in Account & Sync to sync now. |
| `iCloudUnavailable`, account `noAccount` | `sync.now.unavailable.iCloudUnavailable` | Sign in to iCloud in Settings to sync. **Mac:** Sign in to iCloud in System Settings to sync. |
| `iCloudUnavailable`, other account statuses | the T3 detail of the status's `unavailableReason` | |
| `decisionRequired` | `sync.now.unavailable.decisionRequired` | Choose where to keep this collection before you sync. |
| `recoveryRequired` | `sync.now.unavailable.recoveryRequired` | iCloud sync needs your attention. Open iCloud Sync to see what to do. |

- "Automatic sync running" (decided 2026-10-27): auto is on and the T3 state is `upToDate`, `changesPending`, `syncingCollection`, `syncingMedia` or `offline`, or it is `needsAttention` and every item is one that stops no sync: `conflicts`, `conflictsUnreadable`, `refused`, `referenceWaiting`, `oversize`, `unreadable`, `unsyncable`, `claimStuck`, `generationIncomplete`, `assetUnavailable`, `applyFailed`, `rekeyFailed`, `mediaZoneMissing`, `mediaChanged`, `mediaMissing`, `exchangeIncomplete` or `unsupportedSchema`. Row 2 of P12.1 matches before every other reason, so during review a device that is not connected, paused, unavailable, isolated or stopped would otherwise be told that iCloud keeps syncing; it gets the `.manual` text instead. R89's sentence is true whenever it is shown.
- Decided 2026-10-27: `notConnected` names no control, because Account & Sync's enable control is Connect to iCloud… or, for a `stopped` binding that V9.2 offers to resume, Resume Syncing (T14).
- Decided 2026-10-27: with `decisionRequired` Sync Now is disabled like every other reason: the popover button, the menu command and `Y` (P12.1 row 6 makes it `unavailable`). Choosing Sync Now never opens the T10 sheet. The popover shows the decision's action, `sync.decision.action`, beside the disabled Sync Now, and only that action opens the sheet (`sync/cloudkit-recovery#V11`).
- AnkiConnect's `sync` (U146) shows nothing; it returns its typed error.

## T7. Sync Now progress and results (R105, R2049, R2050, R1555; `sync/policy#P12.3`, `#P12.6`)

Progress, in the popover while the exchange runs, one line for the current P12.3 step:

| Key | English | Step |
|---|---|---|
| `sync.now.step.waiting` | Waiting for iCloud… | 1 |
| `sync.now.step.receiving` | Receiving changes… | 2 and 3 |
| `sync.now.step.sending` | Sending changes… | 4 |
| `sync.now.step.checking` | Checking for new changes… | 5 |
| `sync.now.step.finishing` | Finishing… | 6 |

Step 7 (media) shows T5's lines, not a step line: the result does not wait for media (P12.3).

Results, shown in the popover when the exchange ends and kept as `sync.details.lastAttempt`'s `%1$@`. When the user started the exchange, VoiceOver announces the result title.

| Result | Title key, English | Lines (each when its count is above 0) |
|---|---|---|
| `succeeded` | `sync.result.succeeded`: Finished Syncing | `sync.result.newerPending`: `%lld newer change is waiting to sync.` / `%lld newer changes are waiting to sync.`; `sync.details.heldBack` (T4); `sync.result.incomingWaiting`: `%lld change is waiting for another device to finish syncing.` / `%lld changes are waiting for another device to finish syncing.`; then one media line below |
| `incomplete` | `sync.result.incomplete`: Some Changes Did Not Sync | `sync.result.outstanding`: `%lld change is still waiting to sync.` / `%lld changes are still waiting to sync.`; `sync.result.blocked`: `%lld change can’t sync. See the details below.` / `%lld changes can’t sync. See the details below.` |
| `waiting{class}` | `sync.result.waiting`: Sync Couldn’t Finish Yet | the T8 line of `class`; for class none, `sync.result.notReady`: Meguri is still getting ready to sync. Try again in a moment. |
| `failed{class}` | `sync.result.failed`: Sync Failed | the T8 line of `class` |
| `cancelled` | `sync.result.cancelled`: Sync Stopped | `sync.status.keptLocally` (T3), unless an attention item `collectionCorrupt`, `localStorageFull` or `trackingError` is present, whatever the state |

Media line of `succeeded` (R2050: a collection result never says media is done when it is not):

| `media` | Key | English |
|---|---|---|
| `done` | `sync.result.media.done` | Media files finished syncing. |
| `pending{files, bytes}` | `sync.result.media.pending` | `%1$lld media file is still syncing (%2$@).` / `%1$lld media files are still syncing (%2$@).` |
| `paused` | `sync.result.media.paused` | Media files don’t sync while audio and image sync is off. |

- `blocked` counts the selected entities that are blocked and the fetched entries that wait for `format`, `asset` or `apply_failed` (P12.4); the details are T9's rows.
- Decided 2026-10-27: the success title is "Finished Syncing", never "Up to Date" or "Synced": it says that this device finished its exchange, and nothing about other devices (R2050).

## T8. Error classes (R1561, R1555, R2059; `sync/policy#P15`)

One short line per class, used by `sync.details.error`, the `waiting` and `failed` results and Account & Sync's status. The attention rows (T9) give the longer text and the actions.

| Class | Key | English |
|---|---|---|
| `noAccount` | `sync.error.noAccount` | iCloud isn’t available. |
| `accountChanged` | `sync.error.accountChanged` | The iCloud account on this device changed. |
| `offline` | `sync.error.offline` | You’re offline. |
| `throttled{retryAfter}`, while the retry instant is in the future | `sync.error.throttled.retry` | `iCloud is busy. Try again in about %@.` (T1 duration of the time left until the retry instant, below) |
| `throttled`, no retry-after, or its retry instant has passed | `sync.error.throttled` | iCloud is busy. Try again later. |
| `quotaFull` | `sync.error.quotaFull` | Your iCloud storage is full. |
| `localStorageFull`, no space | `sync.error.localStorageFull` | This device is out of storage space. |
| `localStorageFull`, other failure (`trackingError`) | `sync.error.trackingError` | Meguri couldn’t save its sync information on this device. |
| `conflictRecovery` | `sync.error.conflictRecovery` | iCloud sync needs your attention. |
| `unsupportedSchema` | `sync.error.unsupportedSchema` | Some sync information needs a newer version of Meguri. |
| none, `cloudError` (P15 last two rows) | `sync.error.cloudError` | iCloud sync isn’t working right now. |

- In the `iCloudUnavailable` state, `sync.error.noAccount` is followed by the T3 detail of `unavailableReason`.
- Decided 2026-10-27: the retry instant is the instant the throttled error was reported (the end of the exchange or engine operation that got it) plus `retryAfter`; `sync/policy#P14.4`'s `error` must carry it as that instant, not as the interval. The argument is the time left until it, rounded up as T1 says and refreshed at least once a minute while the line is shown. Once the time left is 0 or less, the line is `sync.error.throttled`. The same rule applies to a kept `waiting{throttled}` result (T7).
- Throttling is quiet (R1555): it shows only in the popover, never in the button's state.

## T9. Attention items (R1561, R2054, R2059; `sync/policy#P14.4`, `sync/cloudkit-recovery#V11`)

Each item is one popover row: its text, then its actions as buttons (T9.2). Items with entities have a details list (T11). The rows also appear in Account & Sync.

### T9.1 Rows

| Item | Key | English |
|---|---|---|
| `isolated`, `signed_out` with an account available again | `sync.attention.signedOut` | This collection stopped syncing when you signed out of iCloud. |
| `isolated`, `account_changed`, while the account's identity is not the binding's | `sync.attention.accountChanged` | You’re signed in to a different iCloud account. This collection was not sent to it. |
| `isolated`, `account_changed`, while the account's identity, read in this process, is the binding's again (V9.2) | `sync.attention.accountChangedBack` | This collection stopped syncing when the iCloud account on this device changed. |
| `isolated`, `epoch_switched` | `sync.attention.replacedElsewhere` | The iCloud collection was replaced from another device. |
| `isolated`, `zone_missing`, `active_missing` | `sync.attention.missing` | This collection is missing from iCloud. Your collection and its unsent changes are still on this device. |
| `isolated`, `zone_deleted_by_user` | `sync.attention.deleted` | Meguri’s iCloud data was deleted. Your collection and its unsent changes are still on this device. |
| `isolated`, `encrypted_data_reset` | `sync.attention.encryptionReset` | The iCloud copy can no longer be read because your iCloud encrypted data was reset. Your collection is still on this device. |
| `isolated`, `file_mismatch` | `sync.attention.fileMismatch` | The collection file on this device isn’t the one that was syncing. |
| `isolated`, any other reason | `sync.attention.isolated` | This collection stopped syncing with iCloud. |
| `rootUnreadable` | `sync.attention.rootUnreadable` | Meguri can’t read which collection is current in iCloud. Your collection and its unsent changes are still on this device. |
| `chooseCollection` | `sync.attention.chooseCollection` | Choose how this collection syncs with iCloud. |
| `conflicts` | `sync.attention.conflicts` | `%lld edit was kept as a copy because it conflicted with a change from another device.` / `%lld edits were kept as copies because they conflicted with changes from other devices.` |
| `conflictsUnreadable` | `sync.attention.conflictsUnreadable` | Some saved conflict copies couldn’t be read. |
| `refused` | `sync.attention.refused` | `%lld item can’t sync because its copy in iCloud is not valid.` / `%lld items can’t sync because their copies in iCloud are not valid.` |
| `referenceWaiting` | `sync.attention.referenceWaiting` | `%lld change from iCloud is waiting for a related change that has not arrived.` / `%lld changes from iCloud are waiting for related changes that have not arrived.` |
| `oversize` | `sync.attention.oversize` | `%lld item is too large to sync.` / `%lld items are too large to sync.` |
| `unreadable` | `sync.attention.unreadable` | `%lld item can’t be read, so it doesn’t sync.` / `%lld items can’t be read, so they don’t sync.` |
| `unsyncable` | `sync.attention.unsyncable` | `%lld media file is empty or too large to sync.` / `%lld media files are empty or too large to sync.` |
| `claimStuck` | `sync.attention.claimStuck` | `%lld note type change can’t sync.` / `%lld note type changes can’t sync.` |
| `generationIncomplete`, one row per generation | `sync.attention.generationIncomplete` | `Changes to the note type “%@” from another device are incomplete.` (the note type's name) |
| `assetUnavailable` | `sync.attention.assetUnavailable` | `%lld change from iCloud couldn’t be downloaded. Meguri tries again the next time the collection opens.` / `%lld changes from iCloud couldn’t be downloaded. Meguri tries again the next time the collection opens.` |
| `applyFailed` | `sync.attention.applyFailed` | `%lld change from iCloud couldn’t be applied. Meguri tries again later.` / `%lld changes from iCloud couldn’t be applied. Meguri tries again later.` |
| `rekeyFailed` | `sync.attention.rekeyFailed` | `%lld item couldn’t be updated to match iCloud. Meguri tries again later.` / `%lld items couldn’t be updated to match iCloud. Meguri tries again later.` |
| `crtMismatch` | `sync.attention.crtMismatch` | This collection and the iCloud collection have different creation dates, so nothing syncs until you choose one. |
| `collectionCorrupt` | `sync.attention.collectionCorrupt` | This collection is damaged, so syncing stopped. |
| `mediaZoneMissing` | `sync.attention.mediaMissingInICloud` | Media files can’t sync because Meguri’s media data is missing from iCloud. Cards and notes still sync. |
| `mediaChanged` | `sync.attention.mediaChanged` | `%lld media file changed while it was syncing. It syncs after Meguri checks it again.` / `%lld media files changed while they were syncing. They sync after Meguri checks them again.` |
| `mediaMissing` | `sync.attention.mediaMissing` | `%lld media file is missing on this device and in iCloud.` / `%lld media files are missing on this device and in iCloud.` |
| `recoveryOpWaiting`, `adopt` waiting for a review session (V14.2 step 7) | `sync.attention.adoptAfterReview` | Replacing this collection with the iCloud collection continues when you finish reviewing. |
| `recoveryOpWaiting`, `adopt` whose files R6.3 cannot prove (V14.3 last row) | `sync.attention.adoptUnproven` | Meguri couldn’t confirm which collection file is current, so syncing stopped. |
| `recoveryOpWaiting`, `publish` | `sync.attention.publishWaiting` | Uploading this collection to iCloud did not finish. |
| `recoveryOpWaiting`, `publish` whose new binding an account event isolated (V15.3) | `sync.attention.publishAccount` | Uploading this collection to iCloud stopped because the iCloud account changed. |
| `recoveryOpWaiting`, `publish` with its switch conflict set (V15.2 step 5) | `sync.attention.publishConflict` | The iCloud collection was replaced from another device while this device was uploading. |
| `quotaFull` while a `publish` op is at step `bound`, `zones` or `sent`, that is during V15.2 step 3 (V15.3); it replaces the whole `quotaFull` row: its text, and its actions, which are V15.3's Cancel Replacement… and storage guidance, with no Try Again | `sync.attention.publishQuota` | iCloud needs room for both collections while the previous one is kept for 30 days. |
| `recoveryOpWaiting`, `abandon_generation` (V18) | `sync.attention.abandonWaiting` | Meguri is finishing abandoning a note type change. |
| `exchangeIncomplete` | `sync.attention.exchangeIncomplete` | Some changes from your last Sync Now did not sync. |
| `quotaFull` | `sync.attention.quotaFull` | Your iCloud storage is full. Changes stay on this device until there’s room. |
| `localStorageFull` | `sync.attention.localStorageFull` | This device is out of storage space, so iCloud sync stopped. Free up space, then try again. |
| `trackingError` | `sync.attention.trackingError` | Meguri couldn’t save its sync information on this device, so iCloud sync stopped. |
| `unsupportedSchema` | `sync.attention.unsupportedSchema` | Some sync information needs a newer version of Meguri. Update Meguri on this device to sync all your changes. |
| `cloudError` | `sync.attention.cloudError` | iCloud sync isn’t working right now. If this continues, copy the debug info and report it. |

- Decided 2026-10-27: `zone_missing` and `active_missing` share one text: both mean the device's iCloud collection is gone, and the choices are the same (V11). `republished` and any reason added later use `sync.attention.isolated`.
- Decided 2026-10-27: `crtMismatch` says "creation dates": `col.crt` is the collection's creation day, and the user knows no other name for it.
- A `signed_out` binding while the account is `noAccount` is not an attention row: the state is `iCloudUnavailable{noAccount}` (T3).
- Decided 2026-10-27: an `account_changed` binding keeps its reason when the user signs back in to its account (V2.2, V9.1), so its row has two texts: `accountChanged` while another identity is signed in, and `accountChangedBack` once the identity read in this process is the binding's, which is when V9.2 can offer Resume.
- Decided 2026-10-27: `rootUnreadable` (`sync/policy#P14.4`, `sync/cloudkit-recovery#V10.2` row 3, V11) has its own row; it is not an `isolated` reason. Its actions are V11's: Replace iCloud Collection… (V15 after confirmation; the core makes its switch conditional on the change tag of the version that could not be read) and Keep on This Device Only… (V16). This closes the `sync/policy#P24` open item for it.
- Decided 2026-10-27: `unreadable` promises no repair. Check Database repairs some unreadable rows (notes covered by `maintenance/check-database-notes` N6.1) and ends `CheckFailed` with nothing changed on others, such as an undecodable deck or note type (`maintenance/check-database` C13). It stays offered, since it is the first remedy of `maintenance/optimize-integrity#M7.1` and changes nothing when it fails; its own result text (U163) then points to the other remedies.
- Decided 2026-10-27: an `adopt` op never waits for Resume (V14.3: a restart deletes a staging op, rolls a `ready` op back or finishes it). Its only waiting rows are `adoptAfterReview` (V14.2 step 7) and `adoptUnproven` (V14.3 last row); both offer Cancel only.

### T9.2 Actions

| Key | English | Runs | Offered on |
|---|---|---|---|
| `sync.action.resume` | Resume Syncing | `resume_binding` (V9.2) after T12's Continue confirmation | `signedOut`, `accountChangedBack`, `isolated` and `localOnly` (a `stopped` binding), only while V9.2 offers it |
| `sync.action.connect` | Connect to iCloud… | the V13 flow (T12) | `localOnly` when Resume is not offered; `signedOut` and `accountChangedBack` when Resume is not offered; `accountChanged`, `fileMismatch`, `isolated`, `chooseCollection` |
| `sync.action.useICloud` | Use iCloud Collection… | the V14 preview (T13.1) | `replacedElsewhere`, `crtMismatch`; `missing`, `deleted`, `encryptionReset` when V10.1 offers V14 |
| `sync.action.replaceICloud` | Replace iCloud Collection… | the V15 preview (T13.2) | `replacedElsewhere`, `missing`, `deleted`, `encryptionReset`, `rootUnreadable` (its switch conditional, V10.2 row 3), `crtMismatch`, `mediaMissingInICloud` |
| `sync.action.keepLocal` | Keep on This Device Only… | T14's `sync.keepLocal` alert, then `keep_local_only` (V16) | `replacedElsewhere`, `missing`, `deleted`, `encryptionReset`, `rootUnreadable`, `publishConflict` |
| `sync.action.reviewConflicts` | Review Conflicts… | V17's list (T13.4) | `conflicts` |
| `sync.action.showDetails` | Show Details… | the item's T11 list | `refused`, `referenceWaiting`, `oversize`, `unreadable`, `unsyncable`, `claimStuck`, `generationIncomplete` when Abandon is not offered, `applyFailed`, `rekeyFailed`, `mediaMissing` |
| `sync.action.copyDebugInfo` | Copy Debug Info | U153's diagnostics copy | the rows V11 gives it: `conflictsUnreadable`, `refused`, `referenceWaiting`, `oversize`, `unreadable`, `unsyncable`, `claimStuck`, `generationIncomplete`, `applyFailed`, `rekeyFailed`, `mediaMissing`, `adoptUnproven` (its R6 journal), `cloudError` |
| `sync.action.checkDatabase` | Check Database… | Check Database (U163, once U128 lands) | `unreadable` |
| `sync.action.abandon` | Abandon Change… | T13.3 | `generationIncomplete` while V18 offers it |
| `sync.action.tryAgain` | Try Again | `SyncNowRequested{command}` (`sync/policy#P12.1`: hands parked names back, or probes tracking first) | `quotaFull` (not while `publishQuota` replaces it), `localStorageFull`, `trackingError`, `cloudError` |
| `sync.action.syncNow` | Sync Now | `SyncNowRequested{command}` | `exchangeIncomplete` |
| `sync.action.resumeOp` | Resume | `resume_recovery_op` (V26.2) | `publishWaiting`, `publishAccount`, as V15.3 offers it |
| `sync.action.cancelOp` | Cancel Replacement… | `cancel_recovery_op` after T13.5's confirmation | `adoptAfterReview`, `adoptUnproven`, `publishWaiting`, `publishAccount`, `publishConflict`, `publishQuota`, as V14.3 and V15.3 allow it |
| `sync.action.useThat` | Use That Collection… | the V14 preview, then `adopt` (V15.2 step 5) | `publishConflict` |
| `sync.action.replaceAgain` | Replace It Again… | a new V15 preview, then `republish` | `publishConflict` |

- Storage guidance (`quotaFull`, `publishQuota`): the row adds `sync.attention.storageGuidance`: "Manage your iCloud storage in Settings." **Mac:** "Manage your iCloud storage in System Settings." Text only, as T3 says for account guidance.
- Actions that V20 refuses during a review session (Connect, Use iCloud Collection, Abandon) stay enabled; the session error is T13.6's `SessionActive` text. Sync Now follows T6.
- Decided 2026-10-27: `sync.action.tryAgain` and `sync.action.syncNow` send the Sync Now request, so they are enabled and disabled exactly as Sync Now is (T6, `sync/policy#P12.1`), with T6's reason as the Mac help tag and, on iPhone and iPad, as the line under the disabled button. A disabled Try Again sends nothing, so it probes no tracking; a tracking stop still clears at P15.2's other triggers, and Try Again works once Sync Now is available again.
- `collectionCorrupt` has no action: the damaged-collection screen (`shell.damagedCollection`, `docs/design/screens.md`) holds the remedies (V19).
- Decided 2026-10-27: `localOnly` is a state, not a T9 row. Its action is shown under the T3 detail in the popover and is Account & Sync's enable control (T14). It is `sync.action.resume` while the newest binding is `stopped` and V9.2 offers Resume, and `sync.action.connect` otherwise; never both. For such a binding Connect would also end at T12's Continue sheet (V13.3 last row); Resume Syncing opens it directly.

## T10. The publication decision (R1548, R2056; `sync/cloudkit-recovery#V12.2`)

The sheet opened by `paused{decisionRequired}`'s action, `sync.decision.action`. Sync Now never opens it (T6).

| Key | English |
|---|---|
| `sync.decision.action` | Choose… |
| `sync.decision.action.label` | Choose where to keep this collection (the accessibility label of `sync.decision.action`) |
| `sync.decision.sheetTitle` | Choose Where to Keep This Collection |
| `sync.decision.title.restore` | Choose where to keep the restored collection. |
| `sync.decision.title.import` | Choose where to keep the imported collection. |
| `sync.decision.title.none` | Choose where to keep this collection. |
| `sync.decision.message` | Until you choose, this collection stays on this device and nothing is sent to iCloud. |
| `sync.decision.replace` | Replace iCloud Collection… |
| `sync.decision.replace.caption` | The iCloud collection is replaced with this one on all your devices. |
| `sync.decision.keepLocal` | Keep on This Device Only |
| `sync.decision.keepLocal.caption` | This collection stays on this device and doesn’t sync. |
| `sync.decision.notNow` | Not Now |
| `sync.decision.unsentInBackup` | `%lld change that was not sent before the replacement is only in the backup made before it.` / `%lld changes that were not sent before the replacement are only in the backup made before it.` |

- The sheet's title is `sync.decision.sheetTitle` (title case, T1). Its first line, the prompt, is a `sync.decision.title.*` sentence chosen by `pendingDecisionReason` (`engine/collection-replace#R6.7`): `restore` gives `.restore`; `colpkgImport` and `manualImport` give `.import`; no reason gives `.none`; then `sync.decision.message`. The same prompt is T3's `paused{decisionRequired}` detail. Decided 2026-10-27: the `sync.decision.title.*` keys keep their names, though they are prompts and not the sheet's title.
- The sheet shows T11's counts for this collection and, with an active iCloud collection, for iCloud; then `sync.decision.unsentInBackup` for the old binding's unsent changes and `sync.preview.conflictsKept`.
- Without an available account, Replace is not shown (V12.2). Replace opens T13.2's preview with this collection as the source; Keep on This Device Only needs no further confirmation, since it changes nothing in iCloud.

## T11. Counts and previews (R2056; `sync/cloudkit-recovery#V13.4`)

### T11.1 Sides

| Key | English |
|---|---|
| `sync.counts.thisDevice` | This Device |
| `sync.counts.iCloud` | iCloud |
| `sync.counts.notes`, `.cards`, `.reviews`, `.decks`, `.noteTypes`, `.mediaFiles` | Notes, Cards, Reviews, Decks, Note Types, Media Files |
| `sync.counts.lastReview` | Last Review |
| `sync.counts.lastChange` | Last Change |
| `sync.counts.unknown` | Unknown |
| `sync.counts.unreadable` | Can’t be read |
| `sync.counts.asOf` | `As of %@` (`MGEpoch.published_at_ms`, T1 instant), under the iCloud side when its counts are from the published summary |

- Values are integers and T1 instants. The iCloud side's Last Review and Last Change are `sync.counts.unknown` when its counts come from the published summary; a damaged local side shows `sync.counts.unreadable` for every value (V14.2 step 1).

### T11.2 Work at risk

| Key | English |
|---|---|
| `sync.preview.unsent` | `%lld unsent change on this device` / `%lld unsent changes on this device`, followed by its kinds (T11.3) |
| `sync.preview.heldBack` | `%lld review answer not yet sent` / `%lld review answers not yet sent` |
| `sync.preview.isolatedUnsent` | `%lld unsent change from an earlier iCloud connection` / `%lld unsent changes from an earlier iCloud connection` |
| `sync.preview.otherDevicesUnknown` | Changes that other devices have not sent yet are not counted. |
| `sync.preview.sentAfterReplacement` | `%lld change sent after the iCloud collection was replaced may only be in the replaced collection.` / `%lld changes sent after the iCloud collection was replaced may only be in the replaced collection.`, followed by its kinds |
| `sync.preview.sentAfterReplacement.adopt` | `After this replacement, it’s only in this device’s backup, and possibly in the replaced iCloud collection for a limited time.` / `After this replacement, they’re only in this device’s backup, and possibly in the replaced iCloud collection for a limited time.` |
| `sync.preview.sentAfterReplacement.publish` | `It’s included when this collection replaces the iCloud collection.` / `They’re included when this collection replaces the iCloud collection.` |
| `sync.preview.otherDevicesSentUnknown` | What other devices sent to the replaced collection isn’t known. |
| `sync.preview.notApplied` | `%lld change received from other devices has not been applied, and the new iCloud collection won’t include it.` / `%lld changes received from other devices have not been applied, and the new iCloud collection won’t include them.`, followed by its reasons (T11.3) |
| `sync.preview.notApplied.noteType` | `Includes an unfinished change to the note type “%@”.` (a remote generation `waiting` with a `claimed` manifest) |
| `sync.preview.conflictsKept` | `%lld saved conflict copy is kept.` / `%lld saved conflict copies are kept.` |

- `sync.preview.sentAfterReplacement.adopt` and `.publish` are plural entries on the same count as `sync.preview.sentAfterReplacement`, and show no number.
- Decided 2026-10-27: `.adopt` names no duration. Each device deletes the replaced zone 30 days after the later of the switch and its own first-seen instant (`sync/cloudkit-records#K18.5`), so from this device the zone may already be gone or near its end, and after a second switch it is not the previous collection V14.1 offers.

### T11.3 Kinds and reasons

Kinds, for the breakdowns above (joined with the T1 list format) and the T9 details lists:

| Kind | Count key, English (one / other) | Item key, English |
|---|---|---|
| note | `sync.kind.note`: `%lld note` / `%lld notes` | `sync.item.note`: `Note %@` |
| card | `sync.kind.card`: `%lld card` / `%lld cards` | `sync.item.card`: `Card %@` |
| revlog | `sync.kind.review`: `%lld review` / `%lld reviews` | `sync.item.review`: `Review %@` |
| deck | `sync.kind.deck`: `%lld deck` / `%lld decks` | `sync.item.deck`: `Deck %@` |
| preset | `sync.kind.preset`: `%lld preset` / `%lld presets` | `sync.item.preset`: `Preset %@` |
| notetype | `sync.kind.noteType`: `%lld note type` / `%lld note types` | `sync.item.noteType`: `Note Type %@` |
| tag | `sync.kind.tag`: `%lld tag` / `%lld tags` | `sync.item.tag`: `Tag %@` |
| config | `sync.kind.setting`: `%lld collection setting` / `%lld collection settings` | `sync.item.setting`: `Setting %@` |
| media name | `sync.kind.mediaFile`: `%lld media file` / `%lld media files` | `sync.item.mediaFile`: `%@` (the file name alone) |

- An item's `%@` is its key as text (`sync/cloudkit-records#K4.2`): the id in decimal, the tag, the config key. `applyFailed` and `rekeyFailed` rows add the error after the item, as `sync.item.error`: `%1$@: %2$@`.

Inbox reasons, for `sync.preview.notApplied` (K10.3):

| Reason | Key | English (one / other) |
|---|---|---|
| `ready` | `sync.reason.ready` | `%lld ready to apply` |
| `generation`, `structure` | `sync.reason.noteType` | `%lld waiting for a note type change` |
| `reference` | `sync.reason.reference` | `%lld waiting for a related change` |
| `format` | `sync.reason.format` | `%lld needs a newer version of Meguri` / `%lld need a newer version of Meguri` |
| `asset` | `sync.reason.asset` | `%lld not fully downloaded` |
| `apply_failed` | `sync.reason.applyFailed` | `%lld couldn’t be applied` |

Entries with one plural form use it for both.

## T12. Connecting (R2046, R2055, R158; `sync/cloudkit-recovery#V13`, `#V9.2`)

| Key | English | Shown |
|---|---|---|
| `sync.connect.action` | Connect to iCloud… | T9.2; Account & Sync's enable control (T14). Changed 2026-11-20: no longer Welcome's choice (N18.2), and unused once T19 ships (T19.8) |
| `sync.connect.reading` | Getting details of the iCloud collection… | while V13.2 reads and prefetches; with `sync.now.cancel` |
| `sync.connect.downloading` | Downloading your collection from iCloud… | V13.3 row 2, then V14's progress (T13.1) |
| `sync.connect.downloadingMedia` | Downloading media… | after the collection, with T5's lines (R1592) |
| `sync.connect.upload.title` | Upload This Collection to iCloud? | V13.3 row 3, with this device's T11 counts |
| `sync.connect.upload.message` | Your other devices can then use this collection. It uses your iCloud storage. | |
| `sync.connect.upload.confirm` | Upload | |
| `sync.connect.useICloud.title` | Use the iCloud Collection on This Device? | V13.3 row 4, with both sides' counts |
| `sync.connect.useICloud.message` | This device’s empty collection is backed up, then replaced. | |
| `sync.connect.useICloud.confirm` | Use iCloud Collection | |
| `sync.connect.two.title` | This Device and iCloud Each Have a Collection | V13.5, with both sides' counts and T11.2 |
| `sync.connect.two.message` | Meguri doesn’t merge two collections. Choose which one to keep. | |
| `sync.connect.two.useICloud` | Use the iCloud Collection on This Device… | opens T13.1 |
| `sync.connect.two.useICloud.caption` | This device’s collection is backed up, then replaced. | |
| `sync.connect.two.replace` | Replace the iCloud Collection with This Device’s… | opens T13.2 |
| `sync.connect.two.replace.caption` | The iCloud collection can be restored for 30 days. Your other devices will ask what to do. | |
| `sync.connect.two.keepLocal` | Keep This Device’s Collection Without iCloud | local only; nothing is sent |
| `sync.connect.continue.title` | Continue Syncing This Collection? | V13.3 last row, V9.2 Resume |
| `sync.connect.continue.message` | `%lld unsent change on this device will sync.` / `%lld unsent changes on this device will sync.` | |
| `sync.connect.continue.confirm` | Continue Syncing | |

- Every sheet has the system Cancel; Cancel changes nothing and a pending fence stays (V13.5).
- V13.3 row 1 (no collection, nothing in iCloud) is U166's start-empty or import choice; a collection started empty publishes with no confirmation, so this file adds no text for it.

## T13. Recovery confirmations (R125, R2054, R2056; `sync/cloudkit-recovery#V14`, `#V15`, `#V17`, `#V18`, `#V21`)

### T13.1 Replace local collection from iCloud (V14)

| Key | English | Use |
|---|---|---|
| `sync.adopt.action` | Replace Local Collection from iCloud… | Account & Sync's advanced action (T14) |
| `sync.adopt.previous` | `Restore Previous iCloud Collection from %@…` (T1 date of the previous epoch's `published_at_ms`) | Account & Sync, while V14.1 offers the previous source |
| `sync.adopt.title` | Replace Local Collection from iCloud | the preview sheet |
| `sync.adopt.direction` | This device’s collection will be replaced with the iCloud collection. | |
| `sync.adopt.backup` | This device’s collection is backed up first. Media files are not in the backup. | |
| `sync.adopt.mediaWithheld` | `%lld media file on this device isn’t in the iCloud collection. It stays on this device and isn’t uploaded.` / `%lld media files on this device are not in the iCloud collection. They stay on this device and are not uploaded.` | active source, count above 0 |
| `sync.adopt.mediaSuperseded` | `%lld media file on this device differs from the iCloud version. The iCloud version replaces it, and this device’s file moves to the media trash.` / `%lld media files on this device differ from the iCloud versions. The iCloud versions replace them, and this device’s files move to the media trash.` | active source, count above 0 |
| `sync.adopt.previousLocal` | After this, the collection stays on this device only until you choose to replace the iCloud collection with it. | previous source (V14.1) |
| `sync.adopt.damaged` | The damaged collection can’t be read. Its unsent changes are only in the damaged file and its backup copy. | from the damaged-collection screen (V19) |
| `sync.adopt.confirm` | Replace | destructive button |
| `sync.adopt.staging` | Downloading the iCloud collection… | step 4 |
| `sync.adopt.validating` | Checking the downloaded collection… | step 5 |
| `sync.adopt.backingUp` | Backing up this collection… | step 7 |
| `sync.adopt.replacing` | Replacing… | step 8 |

The sheet also shows both sides' T11 counts, T11.2's lines and `sync.preview.conflictsKept`. Cancel is shown while V14.3 allows it.

| Key | English | Use |
|---|---|---|
| `sync.recovery.tryAgain` | Try Again | under a T13.6 line in the V14 and V15 sheets, the T12 sheets and the T10 sheet: runs the failed call again, or for a failed preview read or a staging that a restart deleted (V14.3), reads the preview again and shows it for a new confirmation |

- Decided 2026-10-27: `sync.recovery.tryAgain` is its own key, not T9.2's `sync.action.tryAgain`, which requests a Sync Now. A Try Again after V14.3's `ready` rollback or a staging failure starts from the preview (V14.2 step 1), so the user confirms again.

### T13.2 Replace iCloud collection (V15)

| Key | English | Use |
|---|---|---|
| `sync.publish.action` | Replace iCloud Collection… | Account & Sync's advanced action (T14) and T9.2 |
| `sync.publish.title` | Replace iCloud Collection | the preview sheet |
| `sync.publish.direction` | The iCloud collection will be replaced with this one on all your devices. | |
| `sync.publish.carriedMedia` | `%lld media file this collection uses isn’t on this device. It downloads if iCloud has it, and otherwise shows as missing until a device that has it uploads it.` / `%lld media files this collection uses are not on this device. They download if iCloud has them, and otherwise show as missing until a device that has them uploads them.` | count above 0 |
| `sync.publish.otherDevices` | Changes other devices have not sent yet are not included. Those devices will ask what to do. | |
| `sync.publish.retention` | The current iCloud collection can be restored for 30 days. | when `MGActive` names an epoch |
| `sync.publish.space` | `Needs %1$@ of iCloud storage. %2$@ available.` | when CloudKit reports the space available |
| `sync.publish.spaceUnknown` | `Needs about %@ of iCloud storage.` | otherwise |
| `sync.publish.confirm` | Replace iCloud Collection | destructive button |
| `sync.publish.preparing` | Preparing… | steps `confirmed`, `bound` |
| `sync.publish.uploading` | Uploading this collection to iCloud… | step `zones`, with T5's lines for media |
| `sync.publish.finishing` | Finishing… | steps `sent`, `complete` |
| `sync.publish.cancelling` | Cancelling… | step `cancelling` |

The sheet also shows both sides' T11 counts, `sync.preview.notApplied`, `sync.preview.sentAfterReplacement` with its `.publish` line, and `sync.preview.conflictsKept`. Uploading runs in the background; the popover shows it as `sync.publish.uploading` until the switch.

### T13.3 Abandon an incomplete change (V18)

| Key | English |
|---|---|
| `sync.abandon.title` | `Abandon the Change to “%@”?` (the note type's name) |
| `sync.abandon.message` | Changes to this note type from another device did not finish arriving. Abandoning them returns all your devices to this device’s version of the note type. Content that only the abandoned version has is kept as a conflict copy. |
| `sync.abandon.confirm` | Abandon Change |

### T13.4 Review Conflicts (V17)

| Key | English |
|---|---|
| `sync.conflicts.title` | Review Conflicts |
| `sync.conflicts.empty` | No Conflicts |
| `sync.conflicts.earlierConnection` | From an earlier iCloud connection |
| `sync.conflicts.savedVersion` | Saved Version |
| `sync.conflicts.currentVersion` | Current Version |
| `sync.conflicts.deleted` | Deleted |
| `sync.conflicts.recover` | Restore… |
| `sync.conflicts.recoverConfirm` | Restore |
| `sync.conflicts.resolve` | Mark as Resolved |
| `sync.conflicts.unresolve` | Mark as Unresolved |
| `sync.conflicts.bytesUnavailable` | This file’s saved copy isn’t available on this device. |
| `sync.conflicts.different` | Different |
| `sync.conflicts.restored` | Saved version restored. |

Group kinds, the secondary text of a group's header (V17.5's object label is its primary text):

| K16 kind | Key | English |
|---|---|---|
| note (a card's copies are in its note's group) | `sync.conflicts.kind.note` | Note |
| deck | `sync.conflicts.kind.deck` | Deck |
| preset | `sync.conflicts.kind.preset` | Preset |
| notetype | `sync.conflicts.kind.noteType` | Note Type |
| media name | `sync.conflicts.kind.mediaFile` | Media File |

Reason captions, one per copy:

| Reason (K16) | Key | English |
|---|---|---|
| `field_conflict` | `sync.conflicts.reason.field` | Edited on two devices. |
| `edit_vs_delete` | `sync.conflicts.reason.deleted` | Edited on one device and deleted on another. |
| `blob_conflict` | `sync.conflicts.reason.options` | Its options were changed on two devices. |
| `notetype_conflict` | `sync.conflicts.reason.noteType` | The note type was changed on two devices. |
| `missing_notetype` | `sync.conflicts.reason.missingNoteType` | Its note type no longer exists. |
| `structure_rollback` | `sync.conflicts.reason.rollback` | Kept when a note type change was undone. |
| `unmapped_structure` | `sync.conflicts.reason.unmapped` | Its note type changed in a way Meguri couldn’t follow. |
| `media_content` | `sync.conflicts.reason.media` | Replaced by a different file with the same name. |

- A copy's object label is V17.5's, shown as is. `sync.conflicts.earlierConnection` labels copies of an isolated or salvage binding (V17.2). Restore… opens the `preview_recovery` result; its validation reason is `RecoveryInvalid`'s text (T13.6). Mark as Resolved needs no confirmation: `unresolve` undoes it while the copy exists. `sync.conflicts.different` is the text marker on each compared part whose two versions differ, so the difference is never shown by colour alone. `sync.conflicts.restored` is the toast after a Restore, with the system Undo.

### T13.5 Cancelling a recovery op (V14.3, V15.3)

| Key | English |
|---|---|
| `sync.cancelOp.adopt.title` | Cancel Replacing This Collection? |
| `sync.cancelOp.adopt.message` | This device keeps its current collection. |
| `sync.cancelOp.adoptUnproven.message` | Meguri then asks how this collection syncs with iCloud. |
| `sync.cancelOp.publish.title` | Cancel Replacing the iCloud Collection? |
| `sync.cancelOp.publish.message` | The iCloud collection stays as it was. |
| `sync.cancelOp.publish.message.switchSent` | The upload may already have replaced the iCloud collection. If it did, Meguri asks what to do next. |
| `sync.cancelOp.confirm` | Cancel Replacement |
| `sync.cancelOp.keep` | Continue |

- `adoptUnproven` (V14.3 last row) uses its own message: V13 follows its Cancel. A `publish` op whose switch sent is true (V2.3, V15.3) uses `.switchSent`, since `MGActive` may already name its collection and the binding before's root fetch then isolates it `epoch_switched` (T9.1 `replacedElsewhere`). Decided 2026-10-27: the confirmation never says the iCloud collection is unchanged unless the op never sent its switch.

### T13.6 Errors of the recovery calls (V21)

Shown in one line in the sheet or row that made the call. The call's state is as V21 says; nothing here claims a change.

| Error | Key | English |
|---|---|---|
| `SessionActive` | `sync.recovery.error.sessionActive` | Finish reviewing, then try again. |
| `Busy{running_op}` | `sync.recovery.error.busy` | `Wait until “%@” finishes.` (the running operation's name, U153) |
| `AccountUnavailable` | `sync.recovery.error.accountUnavailable` | iCloud isn’t available. Check your iCloud account, then try again. |
| `AccountMismatch` | `sync.recovery.error.accountMismatch` | This collection belongs to a different iCloud account. |
| `EpochIncomplete` | `sync.recovery.error.epochIncomplete` | The iCloud collection isn’t complete yet. Try again later. |
| `EpochChanged` | `sync.recovery.error.epochChanged` | The iCloud collection changed while you were deciding. Review the new details and choose again. |
| `InsufficientSpace{needed, available}` | `sync.recovery.error.insufficientSpace` | `Not enough space on this device. %1$@ needed, %2$@ available.` |
| `NotOffered` | `sync.recovery.error.notOffered` | This option is no longer available. |
| `ConflictNotFound` | `sync.recovery.error.conflictNotFound` | This conflict copy no longer exists. |
| `RecoveryInvalid{reason}` | `sync.recovery.error.recoveryInvalid` | `This version can’t be restored: %@` (the reason, as `preview_recovery` gives it) |
| `Cancelled` | `sync.recovery.error.cancelled` | Cancelled. |
| `StagingRefused{gate}`, `StagingCorrupt` (V14.2 step 5, `engine/collection-replace#R9`) | `sync.recovery.error.stagingInvalid` | The downloaded iCloud collection couldn’t be used. This collection is unchanged. |
| `SafetyBackupFailed` (V14.2 step 7, `package/backup-triggers#T9`) | `sync.recovery.error.backupFailed` | Couldn’t back up this collection, so nothing was replaced. It is unchanged. |
| `LiveWalNotEmpty`, `StagingNotFinalised` of an `adopt` (V14.3, `engine/collection-replace#R9`) | `sync.recovery.error.replaceFailed` | Couldn’t replace this collection. It is unchanged. |
| `Io` of an `adopt` (`engine/collection-replace#R9`) | `sync.recovery.error.replaceIncomplete` | Meguri couldn’t finish replacing this collection. |
| any other passed-through error (V21 last line) | `sync.recovery.error.other` | Meguri couldn’t finish this. Try again. |

- `Cancelled` says nothing about what changed: a `publish` op may have sent its switch (T13.5). `EpochChanged` reopens the preview with new preview reads.
- The rows `stagingInvalid`, `backupFailed` and `replaceFailed` say the collection is unchanged because V14.2 step 5 and V14.3 make it so: each of their errors comes only before the R5 rename (`engine/collection-replace` R3, R4 and R5 step 1; V14.2 steps 5 and 7), and V14.3's first `ready` row restores the binding before. Each is shown with `sync.recovery.tryAgain` (V14.3). `InsufficientSpace` of an adoption gets its own row above, also with Try Again.
- Decided 2026-10-27: `Io` claims nothing. R9's `Io` is any file-system failure, also R5 step 5's directory sync after the rename and the steps after it, where the new file already stands (`engine/collection-replace#R6.3`, `#R8`); the error alone does not say which. It is shown with `sync.recovery.tryAgain`, which starts from a new preview (T13.1), so the counts the user confirms are read after the failure. `sync/cloudkit-recovery#V14.3` should state the outcome of an `Io` at or after the rename.
- The calls of V26.2 show every row with `sync.recovery.tryAgain` except `NotOffered`, `ConflictNotFound`, `RecoveryInvalid` and `Cancelled`, where trying again cannot help.

### T13.7 An adoption that did not finish (V14.2, V14.3)

| Key | English | Use |
|---|---|---|
| `sync.attention.adoptWaiting` | Replacing this collection with the iCloud collection was not finished. | followed by `sync.cancelOp.adopt.message` (T13.5), with `sync.recovery.tryAgain`: in the V14 sheets (screen contracts `sync.replaceLocal` and `sync.connect`, cases d and an adoption chosen in e) after `StagingRefused`, `StagingCorrupt`, `SafetyBackupFailed`, `LiveWalNotEmpty` or `StagingNotFinalised`. After an `Io` the sheet shows `sync.recovery.error.replaceIncomplete` (T13.6) instead, since the collection may already be replaced |

- Decided 2026-10-27: the key keeps the name the screen contracts give it, but it is not a T9 row: an `adopt` op never waits after these errors (T9.1, V14.3). Where a screen contract names this line, it stands in place of the error's T13.6 row (`stagingInvalid`, `backupFailed`, `replaceFailed`), never in place of `replaceIncomplete`; both say that this device's collection is unchanged, which V14.2 step 5 and V14.3 make true. With no collection on this device (V13.3 case b) the line is `sync.error.cloudError` instead, since there is no collection to keep.

### T13.8 Where the Review Conflicts keys appear (V17)

Decided 2026-10-27, so that the screen contract `sync.reviewConflicts` and T13.4 give one answer; this section adds no key and changes none (T1):

- `sync.conflicts.restored` is shown, as T13.4 says: the toast after a Restore, with the system Undo for that one undoable step (R173). The contract's "no Restore toast" note gave a missing text as its reason, and T13.4 has the text. The comparison still refreshes and the copy stays unresolved, as the contract says.
- `sync.conflicts.different` appears only where the comparison lists every saved part: for `missing_notetype`, `structure_rollback` and `unmapped_structure`, on each part whose two versions differ. Where the page lists only the parts that differ, every part differs, so no part has the marker. When the object is gone, the current side reads `sync.conflicts.deleted` and no part has the marker.
- The list's sections are headed by their kind's `sync.kind.*` count (T11.3), as the contract says. Inside a section, each group's header has the object label as its primary text and the `sync.conflicts.kind.*` noun as its secondary text, as T13.4 says, so a header read alone (VoiceOver's headings rotor) still names the kind.

## T14. Account & Sync (R117, R122, R123, R2046, R2058, R2059; `sync/cloudkit-recovery#V16`)

| Key | English | Use |
|---|---|---|
| `sync.settings.section` | Account & Sync | section title |
| `sync.settings.iCloud` | iCloud | the status row: T3 title and detail, then T4 and T9 |
| `sync.settings.account.available` | Signed in to iCloud | account row |
| `sync.settings.account.none` | Not signed in to iCloud | |
| `sync.settings.account.restricted` | iCloud is restricted on this device | |
| `sync.settings.account.temporary` | iCloud is temporarily unavailable | |
| `sync.settings.account.unknown` | iCloud account status unknown | account status is unknown; device setup does not imply sign-out |
| `sync.settings.account.setupNeeded` | Meguri can’t use iCloud on this device | account status `configuration` (`unavailableReason` `setupNeeded`, P14.4) |
| `sync.settings.account.other` | Signed in to a different iCloud account than this collection uses | the account does not match the binding |
| `sync.settings.automatic` | Sync Automatically | toggle (R123) |
| `sync.settings.automatic.footer` | When this is off, changes sync only when you choose Sync Now. | |
| `sync.settings.media` | Sync Audio and Images | toggle (R122) |
| `sync.settings.media.footer` | When this is off, cards and notes still sync. | |
| `sync.settings.pause` | Pause iCloud Sync | button (V16) |
| `sync.settings.resumePause` | Resume iCloud Sync | the same button while paused |
| `sync.settings.stop` | Stop Using iCloud on This Device… | button (V16) |
| `sync.settings.stop.title` | Stop Using iCloud on This Device? | confirmation |
| `sync.settings.stop.message` | This collection stays on this device and stops syncing. Nothing in iCloud is deleted. | |
| `sync.settings.stop.confirm` | Stop Using iCloud | |
| `sync.keepLocal.title` | Keep This Collection on This Device Only? | alert of T9.2's `sync.action.keepLocal` |
| `sync.keepLocal.message` | This collection stops syncing with iCloud. Nothing in iCloud is deleted. | |
| `sync.keepLocal.confirm` | Keep on This Device Only | |
| `sync.settings.conflicts` | Review Conflicts… | with the unresolved count as its value (V17.2) |
| `sync.settings.advanced` | Advanced | group of `sync.adopt.action`, `sync.adopt.previous`, `sync.publish.action` |
| `sync.settings.privacy` | Your collection is stored in your private iCloud account and uses your iCloud storage. With Advanced Data Protection on, your collection’s content and media are end-to-end encrypted; some sync information, such as sizes and dates, is not. | section footer (R2058, R2059) |

- The enable control is `sync.connect.action` while no binding is usable, except `sync.action.resume` for a `stopped` binding that V9.2 offers to resume (T9.2), and Pause or Resume otherwise (R117). Pause and Resume need no confirmation (V16). T10's Keep on This Device Only needs no alert: the sheet is the confirmation, and its caption says the effect.
- Account & Sync never shows an Anki endpoint, account or transfer result (R1560).

## T15. Mac quit (R127; `sync/policy#P11.4`)

| Key | English |
|---|---|
| `sync.quit.title` | Sending Changes to iCloud… |
| `sync.quit.message` | Meguri quits when this finishes, or in 10 seconds. Changes that are not sent stay on this Mac and sync next time. |
| `sync.quit.skip` | Quit Without Syncing |

- The sheet shows only for `QuitFlush`; `QuitNow` shows nothing. VoiceOver announces the title when it appears. It never says the changes were synced: at `until` or on Quit Without Syncing the app quits with the pending work durable (P11.4).

## T16. Tests

The oracle has no CloudKit, so the text is checked by Meguri's Swift tests: `SyncStatusModelTests` (U151, in `app/Packages/MeguriKit/Tests/AppModelTests/`: each P14.2 state and reason gives the T3 keys; T4's and T5's lines appear exactly under their conditions; the T6 reason and its `.manual` variant for each combination of P12.1 row 2 with auto, state and attention items; `decisionRequired` leaves Sync Now disabled; the media free-space pause shows `sync.media.noSpace` and keeps the state and `sync.status.keptLocally`; `iCloudUnavailable` and `paused{decisionRequired}` with `collectionCorrupt`, `localStorageFull` or `trackingError` show no `sync.status.keptLocally`; a throttled error 70 s before its retry instant shows "about 2 minutes", and after the instant `sync.error.throttled`), a `SyncButtonStateTests` suite in U161's `app/Packages/MeguriKit/Tests/SyncUITests/` (U161: the button's label and value, the help tag, T7's result lines for each P12.6 result and media value, `cancelled` with and without the three items, AE91's incomplete result showing no completion text, every T9 item with its actions and their conditions, `rootUnreadable` and both `account_changed` texts included; Try Again disabled with Sync Now) and U161's snapshots of T10 to T15. U161's Files list in its plan unit section, not its table row, gives these paths.

Decided 2026-10-27: `SyncStringCatalogTests` (in `app/Packages/MeguriKit/Tests/AppModelTests/`; U151 adds it with its keys, and U161 adds the rows of the keys it adds) loads `Sync.xcstrings` (T1) and checks every key of this file against an expected table taken from this file: the key exists, its English value is exactly this one, and its plural and Mac variations are those given. The same test fails when any entry of the catalog contains "AnkiWeb", "record", "zone" or "CloudKit". U167's `lint-strings` checks this catalog as every other: developer comments, plural variants and no concatenation.

T19's tests are T19.9.

T18's keys are rows of `SyncStringCatalogTests` like every other key here. `MeguriSyncPresentationTests` (in `app/Packages/MeguriKit/Tests/AppModelTests/`) checks that each T18.4 row, each T18.5 stage and failure and each row of T18.7's Sync Now table gives exactly its keys and action; that an exactly accepted upload with unfinished local activation, and `activationInterrupted`, offer only `sync.meguri.setup.finish` and no new choice; that the T18.2 lines come from the old service's local status, an unknown value giving the `.unknown` line and never `.none`, and held answers never giving `.none`; and that no T18 value contains a T18.1 word.

## T17. Changes from the draft wording

- `sync/policy#P12.1`'s explanation column was draft text (P22, P24). Its keys stay; T6 replaces the English: "Sync is unavailable until the collection is open." becomes "Sync Now is available once the collection is ready."; the review text is truthful only while automatic sync runs, so a `.manual` key is added; "in Settings" becomes "in Account & Sync" for Meguri's own section; "Sign in to iCloud to sync." gets the system place and per-reason variants; "Choose whether this collection replaces the one in iCloud before syncing." becomes "Choose where to keep this collection before you sync." to match the decision sheet (T10); "Open iCloud status to continue." becomes "Open iCloud Sync to see what to do.", naming the button.
- `sync/cloudkit-recovery#V11` quotes meaning, not copy; T9, T10, T12 and T13 give the copy for its quoted texts, in title case where they are labels ("Use the iCloud Collection on This Device…", "Restore Previous iCloud Collection from %@…"). V15.3's quota reason and V15.2 step 5's conflict text are kept word for word, with "The" added to the latter.

## T18. Meguri Sync (`sync/meguri-provider` MP2–MP5; `docs/design/native-interface-contract.md` N12.4)

The text of Meguri Sync, which syncs with the user's own Meguri Sync server, and of the Sync Service choice (screen contracts `settings.accountSync`, `sync.serviceSwitch`, `sync.meguriSetup`, `sync.status`). The states are the provider phase, the Meguri connection phase and the setup and round stages that the core reports (MP1–MP5); this section adds no state and starts no request.

### T18.1 Rules

- T1 applies. These words are also never used in T18 text: Anki, AnkiWeb, hostkey, bootstrap, baseline, generation, provider, handoff.
- From `sync/meguri-provider`, with their words unchanged: Sync Service, iCloud Sync, Meguri Sync, Server Address, Username, Password, Sign In, Last exchange completed, Use This Device’s Collection…, Use Server Collection…, Cancel, Collection transferred; media incomplete, Show Affected Files, Retry. MP4's “Server acceptance unknown” is a status title here, so T1 puts it in title case.
- No T18 text says that devices match or that another device has the changes (MP2). Collection and media results are separate lines; a finished collection never reads as a finished setup (MP5).
- A count or fact the old service does not report locally is shown as unknown, never as zero (MP3.1). Opening a sheet or Account & Sync reads only local state.
- Decided 2026-11-17 (root, delegated): the Sync Service picker has only iCloud Sync and Meguri Sync; local-only use is each service's separate Stop Using action (MP2). Every change of service opens `sync.serviceSwitch` and needs its explicit confirmation, except a choice made while no service is selected, which has no old service to stop. The handoff recovery offers only Keep on This Device Only…. The collection choice confirms in a preview sheet (N11.8), never a dialog. Files that can’t transfer get no other remedy text: Show Affected Files lists each actual name and its reason, Retry recomputes the list, and local study stays available; Meguri never deletes or converts a file automatically and never points to a media folder the platform does not expose.

### T18.2 Sync Service and changing it (MP2, MP3)

| Key | English | Use |
|---|---|---|
| `sync.service.label` | Sync Service | the Picker, first row of Account & Sync |
| `sync.service.iCloud` | iCloud Sync | picker option |
| `sync.service.meguri` | Meguri Sync | picker option; label of the Meguri status row |
| `sync.service.none.footer` | Choose a service to sync this collection. | section footer while no service is selected |
| `sync.service.changing` | Changing Sync Service… | under the picker while a change runs |
| `sync.service.changeFailed` | Changing the sync service didn’t finish. | under the picker after a change reports failure; the picker and the section then show the selection and phases the core reports, which the failure may have changed |
| `sync.service.cancel` | Cancel | the cancel action of the `sync.serviceSwitch` sheet, the `sync.service.recovery.title` alert and the `sync.meguri.signOut.title` alert; it changes nothing |
| `sync.service.recovery` | Changing the sync service didn’t finish. No service syncs until you choose what to do. | under the picker while the change waits for recovery (MP3.5) |
| `sync.service.recovery.action` | Keep on This Device Only… | button under that line |
| `sync.service.recovery.title` | Keep This Collection on This Device Only? | alert |
| `sync.service.recovery.message` | Neither service syncs this collection. The collection, its media and its backups stay on this device. Nothing in iCloud or on your Meguri Sync server is deleted, and changes that are only there are not copied. To sync again, choose a service and set it up again. | |
| `sync.service.recovery.confirm` | Keep on This Device Only | destructive |
| `sync.service.blocked` | Meguri can’t read this device’s sync settings, so no service syncs. | under the picker while the provider phase is blocked (MP1); no action |

The `sync.serviceSwitch` sheet. `%@` is the old service's name, `sync.service.iCloud` or `sync.service.meguri`. The lines show in this order, each under its condition.

| Key | English | Shown |
|---|---|---|
| `sync.switch.title.iCloud` | Switch to iCloud Sync | sheet title, destination iCloud Sync |
| `sync.switch.title.meguri` | Switch to Meguri Sync | destination Meguri Sync |
| `sync.switch.title.localOnly` | Stop Using Meguri Sync on This Device | destination local-only, from `sync.meguri.stop` |
| `sync.switch.pending` | `%1$lld change on this device hasn’t been sent to %2$@.` / `%1$lld changes on this device haven’t been sent to %2$@.` | the old service reports a count above 0 |
| `sync.switch.pending.some` | Some changes on this device haven’t been sent to %@. | it reports pending changes without a count |
| `sync.switch.heldBack` | `%1$lld review answer on this device hasn’t been sent to %2$@ yet.` / `%1$lld review answers on this device haven’t been sent to %2$@ yet.` | it reports held answers above 0 (iCloud: P14.4 `heldBack`, which `pending` does not count) |
| `sync.switch.pending.none` | No changes on this device are waiting to be sent to %@. | it reports both pending changes and held answers, and both are 0 |
| `sync.switch.pending.unknown` | Meguri can’t tell whether changes or review answers on this device are waiting to be sent to %@. | otherwise, including any count it does not report |
| `sync.switch.media` | `%1$lld media file hasn’t synced with %2$@.` / `%1$lld media files haven’t synced with %2$@.` | it reports media waiting, or files not transferred (T18.6), above 0 |
| `sync.switch.media.unknown` | Meguri can’t tell whether media files are waiting to sync with %@. | its media count is not known |
| `sync.switch.acceptanceUnknown` | Meguri doesn’t know whether your Meguri Sync server received this device’s last collection upload. No service can send changes until that’s settled. | a Meguri upload's acceptance is unknown (MP3), whatever the old service |
| `sync.switch.kept` | Changes that haven’t reached this device stay with %@. Nothing in iCloud or on your Meguri Sync server is deleted. | always |
| `sync.switch.otherDevices` | Your other devices keep their current service until you change it on each one. | destination iCloud Sync or Meguri Sync |
| `sync.switch.signOut` | Your Meguri Sync sign-in is removed from this device. | old service Meguri Sync |
| `sync.switch.next.iCloud` | Next, connect to iCloud and choose which collection to keep. | destination iCloud Sync |
| `sync.switch.next.meguri` | Next, sign in to your Meguri Sync server and choose which collection to keep. | destination Meguri Sync |
| `sync.switch.next.localOnly` | This collection stays on this device and stops syncing. | destination local-only |
| `sync.switch.busy` | You can change the sync service after the current review or collection change finishes. | MP3.1 refuses now; the confirm button is disabled |
| `sync.switch.confirm.iCloud` | Switch to iCloud Sync | destructive button |
| `sync.switch.confirm.meguri` | Switch to Meguri Sync | destructive button |
| `sync.switch.confirm.localOnly` | Stop Using Meguri Sync | destructive button |

- The old service's local status gives the lines: for iCloud Sync, P14.4's `pending`, `heldBack` and media waiting; for Meguri Sync, the round's pending work and the T18.6 counts. Pending and held work are both disclosed: `sync.switch.pending.none` never shows while held answers exist or are not reported. Meguri Sync's round reports no count of either, so it shows `sync.switch.pending.some` while the round has pending work and `sync.switch.pending.unknown` otherwise, also without a round or setup stage; it never shows `.none`. Nothing is read from a server.
- Leaving Meguri Sync, by a switch or by Stop Using, removes this device's Meguri Sync sign-in (MP2's Remove Connection), so no hidden sign-in stays behind.

### T18.3 Server and sign-in (MP2)

| Key | English | Use |
|---|---|---|
| `sync.meguri.address` | Server Address | text field; while signed in, a read-only row with the address |
| `sync.meguri.username` | Username | text field |
| `sync.meguri.password` | Password | secure field |
| `sync.meguri.signIn.title` | Sign In to Meguri Sync | the Account & Sync sign-in section's single heading |
| `sync.meguri.signIn` | Sign In | button |
| `sync.meguri.signIn.preparing` | Preparing Sign-In… | visible progress during explicit local Host preparation and offline connection load |
| `sync.meguri.signIn.footer` | Signing in doesn’t send or change your collection. | footer under the sign-in fields (MP5) |
| `sync.meguri.error.address` | Enter a valid server address. Meguri Sync needs an https address. | endpoint normalization refuses the address; no request is sent |
| `sync.meguri.error.credentials` | The username or password isn’t correct. | sign-in error: credentials refused |
| `sync.meguri.error.redirect` | The server sent Meguri to a different address. Check the server address, then sign in again. | sign-in error: redirect refused (MP2) |
| `sync.meguri.error.network` | Meguri couldn’t reach the server. Check the address and your connection, then try again. | sign-in error: no connection |
| `sync.meguri.error.server` | The server couldn’t complete sign-in. Try again later. | sign-in error: server failure |
| `sync.meguri.error.other` | Sign-in didn’t finish. Try again. | any other sign-in error |
| `sync.meguri.error.busy` | Meguri is finishing another task. Wait, then try signing in again. | typed local busy refusal |
| `sync.meguri.error.localSetup` | Meguri couldn’t prepare sign-in on this device. Reopen Meguri, then try again. | explicit click could not attach local sign-in setup |
| `sync.meguri.error.credentialStore` | Meguri can’t read or save your sign-in on this device. Unlock this device, then try again. | typed local credential-store failure; never a credential or network refusal |
| `sync.meguri.connection.retry` | Try Again | connection blocked: reads the stored sign-in again; no request |
| `sync.meguri.signOut` | Sign Out… | button, while signed in |
| `sync.meguri.signOut.title` | Sign Out of Meguri Sync? | alert |
| `sync.meguri.signOut.message` | Meguri Sync stops, your sign-in is removed from this device, and this collection then stays on this device only. Your collection, media and backups stay. Nothing on the server is deleted. | |
| `sync.meguri.signOut.confirm` | Sign Out | destructive |
| `sync.meguri.signingOut` | Signing Out… | while the sign-out drains work |
| `sync.meguri.signOut.failed` | Sign-out didn’t finish. | removal reported failure; the section then shows the selection and connection phase the core reports, which the failure may have changed |
| `sync.meguri.stop` | Stop Using Meguri Sync on This Device… | button; opens `sync.serviceSwitch` to local-only |

- The sign-in error line shows under the fields after a failed attempt, until the next attempt or an address edit. The password field is empty after every attempt.
- Account & Sync uses one `sync.meguri.signIn.title` heading, native fields in address, username, password order, and one prominent system Sign In button. It omits the repeated service label and the ordinary Not Set Up / Not Signed In title and introductory detail while those fields explain the task. Cancelled, blocked, paused, setup and ongoing sync states remain visible. The shared status popover keeps its service label and T18.4 behavior. Errors are inline native labels with an error SF Symbol and identifier `settings.accountSync.meguri.signInError`; text conveys the error without color.
- Only an explicit Sign In click may prepare the existing local NoCollection Host if needed, then join offline attachment/load before ordinary authentication. Preparing Sign-In… remains visible during that work. Draft fields and service changes are disabled while processing; a captured address edit, changed attachment or lifecycle cancellation fences an obsolete attempt. Two rapid submits issue one attempt. Sign-in neither creates a collection nor starts a transfer; no preparation happens on view rendering.
- Decided 2026-11-17 (root, delegated): Sign Out stops Meguri Sync, drains its work and deletes this device's sign-in; only after both succeed does no service stay selected (local-only). It never selects iCloud Sync (MP2). It keeps the collection, media, backups, server data and every unresolved operation's evidence. Until it succeeds, no text says the sign-in was removed: `sync.meguri.signingOut` shows, then the local-only form, or `sync.meguri.signOut.failed` with whatever selection and phase the core then reports. No failure text says the sign-in or the selection is unchanged.

### T18.4 Status (MP2, MP5)

The row labelled `sync.service.meguri` shows one title and one detail, from the first condition that applies. No detail is shown where the table gives none.

| Condition | Title key, English | Detail key, English |
|---|---|---|
| provider phase handoff recovery or blocked (T18.2 shows the reason) | `sync.meguri.status.stopped.title`: Stopped | none |
| connection blocked (Keychain or storage, MP2) | `sync.meguri.status.connectionBlocked.title`: Can’t Use Sign-In | `sync.meguri.status.connectionBlocked.detail`: Meguri can’t read or save your Meguri Sync sign-in on this device. Unlock this device, then choose Try Again. |
| connection unconfigured | `sync.meguri.status.notSetUp.title`: Not Set Up | `sync.meguri.status.notSetUp.detail`: Enter your server address and sign in to start. |
| connection signed out | `sync.meguri.status.signedOut.title`: Not Signed In | `sync.meguri.status.signedOut.detail`: Sign in to your Meguri Sync server to continue. |
| connection cancelled | `sync.meguri.status.signedOut.title` | `sync.meguri.status.cancelled.detail`: Sign-in was cancelled. |
| connection authenticating | `sync.meguri.status.signingIn.title`: Signing In… | none |
| a setup stage is reported | T18.5 | T18.5 |
| connection setup required, no setup stage and no round | `sync.meguri.status.signedIn.title`: Signed In | `sync.meguri.status.setupUnavailable`: Collection setup isn’t available yet. This collection stays on this device. |
| paused | `sync.meguri.status.paused.title`: Paused | `sync.meguri.status.paused.detail`: Meguri Sync is paused. Changes stay on this device. |
| round idle | `sync.meguri.status.on.title`: On | `sync.meguri.lastExchange`: `Last exchange completed %@.` (T1 instant); none without an instant |
| round with pending work | `sync.meguri.status.pending.title`: Waiting to Sync | `sync.meguri.status.pending.detail`: Some changes on this device haven’t been sent yet. Followed by `sync.meguri.lastExchange` when an instant exists |
| round exchanging the collection | `sync.meguri.status.syncingCollection.title`: Syncing Collection… | none |
| round exchanging media | `sync.meguri.status.syncingMedia.title`: Syncing Media… | none |
| round media blocked | T18.6 | T18.6 |
| round failed, authentication | `sync.meguri.status.signInRequired.title`: Sign In Required | `sync.meguri.status.signInRequired.detail`: The server didn’t accept your sign-in, so Meguri Sync stopped. Changes stay on this device. |
| round failed, acceptance unknown | `sync.meguri.status.acceptanceUnknown.title`: Server Acceptance Unknown | `sync.meguri.round.acceptanceUnknown`: Meguri doesn’t know whether the server received the last changes. They stay on this device. |
| round failed, full sync required | `sync.meguri.status.chooseCollection.title`: Choose a Collection | `sync.meguri.round.fullSyncRequired`: The server needs you to choose which collection to keep before syncing continues. |
| round failed, network | `sync.meguri.status.failed.title`: Last Exchange Didn’t Finish | `sync.meguri.round.network`: Meguri couldn’t reach your Meguri Sync server. Changes stay on this device. |
| round failed, server | `sync.meguri.status.failed.title` | `sync.meguri.round.server`: The server reported a problem. Changes stay on this device. |
| round failed, cancelled | `sync.meguri.status.failed.title` | `sync.meguri.round.cancelled`: The exchange was cancelled. Changes stay on this device. |

- The sign-in fields (T18.3) show for unconfigured, signed out, cancelled and Sign In Required. Choose a Collection shows the T18.5 entries once the setup stage is reported.

### T18.5 Collection setup (MP4, MP5)

| Stage | Title key, English | Detail key, English | Controls |
|---|---|---|---|
| required, a local collection exists (zero cards included) | `sync.meguri.status.setup.title`: Setup Required | `sync.meguri.setup.choose`: Choose which collection to keep. Syncing starts after the collection and its media transfer. | `sync.meguri.setup.useDevice`: Use This Device’s Collection…; `sync.meguri.setup.useServer`: Use Server Collection… |
| required, no local collection | `sync.meguri.status.setup.title` | `sync.meguri.setup.download.detail`: Download your collection from your Meguri Sync server to start. | `sync.meguri.setup.download`: Download from Meguri Sync… |
| preparing | `sync.meguri.status.preparing.title`: Preparing… | `sync.meguri.setup.preparing`: Nothing is replaced yet. | `sync.meguri.cancel`: Cancel |
| preview | `sync.meguri.status.setup.title` | `sync.meguri.setup.review`: Review the replacement to continue. | the `sync.meguriSetup` sheet is open |
| transferring the collection | `sync.meguri.status.transferringCollection.title`: Transferring Collection… | none | |
| acceptance unknown | `sync.meguri.status.acceptanceUnknown.title` | `sync.meguri.setup.acceptanceUnknown`: Meguri doesn’t know whether the server received this device’s collection. Your copies and backups are kept, and no service sends changes until this is settled. You can keep studying on this device. | none |
| transferring media | `sync.meguri.status.transferringMedia.title`: Transferring Media… | `sync.meguri.setup.collectionDone`: The collection transferred. Syncing starts when the media finishes. Followed by `sync.meguri.media.filesLeft`: `%lld media file left` / `%lld media files left`, when the count is known | |
| media blocked | T18.6 | T18.6 | T18.6 |
| the server accepted the upload exactly, and local activation did not finish (MP4) | `sync.meguri.status.acceptedLocalIncomplete.title`: Setup Not Finished on This Device | `sync.meguri.setup.acceptedLocalIncomplete`: The server accepted this device’s collection, but setup didn’t finish on this device. Your copies and backups are kept, and no service sends changes until setup finishes. You can keep studying on this device. | `sync.meguri.setup.finish`: Finish Setup, shown and enabled only while the core exposes a permitted recovery command for the retained operation; no new choice until it is settled |
| failed | `sync.meguri.status.setupFailed.title`: Setup Didn’t Finish | the failure's line, below | `activationInterrupted`: only `sync.meguri.setup.finish`, shown and enabled only while the core exposes a permitted recovery command for the retained operation, and no new choice until it is settled; `authentication` and `identityChanged`: the sign-in fields; every other failure: the entries of the required stage |

| Failure | Key | English |
|---|---|---|
| `remoteUnavailable` | `sync.meguri.setup.error.remoteUnavailable` | Meguri couldn’t get the server’s collection details. Check your connection, then try again. |
| `remoteChanged` | `sync.meguri.setup.error.remoteChanged` | The server’s collection changed after you chose. Choose again. |
| `localChanged` | `sync.meguri.setup.error.localChanged` | This device’s collection changed after you chose. Choose again. |
| `identityChanged` | `sync.meguri.setup.error.identityChanged` | The server address or account changed. Sign in again, then choose again. |
| `backupFailed` | `sync.meguri.setup.error.backupFailed` | Meguri couldn’t back up this device’s collection, so nothing was replaced. |
| `storage` | `sync.meguri.setup.error.storage` | Meguri couldn’t save to this device. Check its free space, then try again. |
| `server` | `sync.meguri.setup.error.server` | The server reported a problem. Try again later. |
| `authentication` | `sync.meguri.setup.error.authentication` | The server didn’t accept your sign-in. Sign in again to continue. |
| `invalidDownload` | `sync.meguri.setup.error.invalidDownload` | The collection from the server couldn’t be used, so nothing was replaced. |
| `activationInterrupted` | `sync.meguri.setup.error.activationInterrupted` | Replacing this device’s collection didn’t finish. Your backup and the downloaded collection are kept until this is settled. |
| `cancelled` | `sync.meguri.setup.error.cancelled` | Setup was cancelled. |

The `sync.meguriSetup` preview sheet, in this order:

| Key | English | Shown |
|---|---|---|
| `sync.meguri.preview.title.useDevice` | Replace Server Collection | sheet title, Use This Device’s Collection… |
| `sync.meguri.preview.title.useServer` | Replace This Device’s Collection | Use Server Collection… |
| `sync.meguri.preview.title.download` | Download from Meguri Sync | Download from Meguri Sync… |
| `sync.meguri.preview.server` | Server | label; the value is the endpoint being set up |
| `sync.counts.thisDevice` with `sync.counts.cards`, `.notes`, `.reviews` (T11.1) | | this device's counts; not for a download |
| `sync.meguri.preview.remoteEmpty` | The server reports no collection, but it may still hold data. | the server reports an empty collection (MP4) |
| `sync.meguri.preview.direction.useDevice` | The server’s collection will be replaced with this device’s collection. Meguri first backs up this device’s collection and keeps a copy of the server’s collection. | |
| `sync.meguri.preview.direction.useServer` | This device’s collection will be replaced with the server’s collection. Meguri backs up this device’s collection first. | |
| `sync.meguri.preview.direction.download` | Your collection is downloaded from the server to this device. | |
| `sync.meguri.preview.unsent` | This device has changes that haven’t been sent. They will be replaced; the backup keeps them. | Use Server Collection…, with pending local work |
| `sync.meguri.preview.otherClients` | Stop syncing on your other devices until this finishes. Changes they send to the server meanwhile can be replaced. | Use This Device’s Collection… (MP4) |
| `sync.meguri.preview.media` | Media then syncs both ways. On either side, files can be added, replaced or deleted. Files that no card uses are kept. | always (MP5) |
| `sync.meguri.preview.automatic` | After setup, Meguri Sync syncs this collection and its media automatically. | always (MP5) |
| `sync.meguri.preview.confirm.useDevice` | Replace Server Collection | destructive button |
| `sync.meguri.preview.confirm.useServer` | Replace This Device’s Collection | destructive button |
| `sync.meguri.preview.confirm.download` | Download | confirm button |

- The sheet has the system Cancel (`sync.meguri.cancel`), which cancels the preparation and changes nothing. The confirm button applies only to the preview shown; a changed preview replaces the sheet's facts and needs a new choice (MP4).

### T18.6 Media (MP5)

| Key | English | Use |
|---|---|---|
| `sync.meguri.status.mediaBlocked.title` | Media Incomplete | status title, setup or round |
| `sync.meguri.media.blocked` | Collection transferred; media incomplete. | its detail |
| `sync.meguri.media.notTransferred` | `%lld media file hasn’t transferred.` / `%lld media files haven’t transferred.` | the files listed as affected, above 0 |
| `sync.meguri.media.remotePending` | `%lld file on the server hasn’t reached this device.` / `%lld files on the server haven’t reached this device.` | files waiting on the server, above 0 |
| `sync.meguri.media.unverified` | Meguri couldn’t confirm which media files transferred. | the accounting is not verified |
| `sync.meguri.media.setupWaits` | Syncing starts after every media file transfers. | setup stage only |
| `sync.meguri.media.study` | You can keep studying on this device. | always in this row |
| `sync.meguri.media.showAffected` | Show Affected Files | button, while files are listed; pushes the list |
| `sync.meguri.media.retry` | Retry | button: checks the media folder again and recomputes the list |
| `sync.meguri.media.list.title` | Affected Files | page title |
| `sync.meguri.media.state.pending` | Waiting to transfer | row value |
| `sync.meguri.media.state.empty` | Empty file | |
| `sync.meguri.media.state.overLimit` | Too large to transfer | over the 100 MiB limit (MP5) |
| `sync.meguri.media.state.invalidName` | Name can’t be used | |
| `sync.meguri.media.state.notRegularFile` | Not a regular file | |
| `sync.meguri.media.lossyName` | Some characters in this name can’t be shown. | under a name that is not valid text |
| `sync.meguri.media.list.footer` | Media can’t finish syncing while a file is listed here. | page footer |
| `sync.meguri.media.list.excluded` | Meguri keeps files it can’t transfer and doesn’t change or delete them. | page footer, under the line above, while an empty, too-large, unusable-name or non-regular file is listed; files waiting to transfer can still change when media syncs (MP5) |

### T18.7 Sync Now, Pause and the sync button (MP2, MP5)

| Key | English | Use |
|---|---|---|
| `sync.now.command` (T6) | Sync Now | the command, by the table below |
| `sync.meguri.now.unavailable.paused` | Meguri Sync is paused. Resume it in Account & Sync to sync now. | reason: paused |
| `sync.meguri.now.unavailable.acceptanceUnknown` | Meguri Sync can’t send changes until Meguri knows whether the server received the last ones. | reason: acceptance unknown |
| `sync.meguri.now.unavailable.signIn` | Sign in to Meguri Sync in Account & Sync to sync. | reason: Sign In Required |
| `sync.meguri.now.unavailable.stopped` | Meguri Sync is stopped. Open Account & Sync to see what to do. | reason: handoff recovery, or provider or connection blocked |
| `sync.meguri.pause` | Pause Meguri Sync | button while a round is reported and not paused |
| `sync.meguri.resume` | Resume Meguri Sync | the same button while paused |
| `sync.meguri.button.label` | Meguri Sync | the sync button's accessibility label while Meguri Sync is selected |
| `sync.meguri.setup.continue` | Continue Setup… | popover button in place of Sync Now while no round is reported; opens Account & Sync |
| `sync.meguri.now.chooseCollection` | Choose Collection… | popover button in place of Sync Now while the round needs a full sync; opens Account & Sync |

Sync Now (the popover button, File › Sync Now and `Y`) follows the first row that applies. It never starts a normal exchange outside the last row (MP5). "Opens Account & Sync" means Settings › Account & Sync at Meguri Sync, presented as the popover's Account & Sync link is (N11.8); it starts no request.

| Condition | Sync Now |
|---|---|
| provider phase handoff recovery or blocked, or connection blocked, with or without a round | disabled; reason `sync.meguri.now.unavailable.stopped` |
| paused | disabled; reason `sync.meguri.now.unavailable.paused`; no network work (MP5) |
| no round: any connection or setup state, including setup's acceptance unknown, media blocked and failures | the popover shows `sync.meguri.setup.continue` instead; File › Sync Now and `Y` open Account & Sync, where the explicit setup or recovery continues |
| round failed, acceptance unknown | disabled; reason `sync.meguri.now.unavailable.acceptanceUnknown`; no upload or new writer starts (MP3) |
| round failed, full sync required | the popover shows `sync.meguri.now.chooseCollection` instead; File › Sync Now and `Y` open Account & Sync, whose explicit collection choice is the only way on (MP4) |
| round media blocked | the popover shows `sync.meguri.media.showAffected` instead, which opens Account & Sync at the affected list; File › Sync Now and `Y` open Account & Sync, where Retry is the explicit recovery |
| round failed, authentication | disabled; reason `sync.meguri.now.unavailable.signIn` |
| a round exchanging the collection or media | disabled, with no reason line; the status row shows the progress |
| otherwise (idle, pending work, failed for network, server or cancellation) | enabled; runs the normal round |

- In Account & Sync, Sync Now shows only for the disabled and enabled rows. For the rows that open Account & Sync, the status row's own controls are that flow.
- While Meguri Sync is selected, the sync button's value is the T18.4 title without its "…", and on the Mac its help tag is that title and detail (as T2).

## T19. Service, setup and the three kinds (2026-11-19)

Decided by Antoine on 2026-11-19 (`docs/design/decisions.md`: `settings.root`, `settings.accountSync`, `sync.connect`, `sync.status`, `sync.serviceSwitch`; `docs/design/native-interface-contract.md` N15). The text of the Sync section of Settings, the Service control, setup without a Connect step, the sheet that asks which collection to keep, the three kinds of problem and Troubleshooting, for both services. The behaviour is `sync/policy#P25`, `sync/cloudkit-recovery#V27` and `sync/meguri-provider#MP6`; T19 is shown when they are implemented, and T2 to T18 stay the shipped text until then. Where T2 to T18 differ, T19 wins. T19 adds no state and changes no condition.

**Unused keys, stated once** (decided 2026-11-19 (review F12)). A key that a decision withdraws is removed from the String Catalog when the change ships, and its name is never reused; this holds for T19.8's keys and for the keys other specs mark withdrawn on 2026-11-19 (`review/reviewer-strings` S17, `decks/deck-options-strings`, the FSRS sheet's and the Add toast's). T1's "keys never change once shipped" means that a key's name and meaning are never changed or reused, not that a dead key stays. The exceptions, which stay in the catalog though no view shows them, are listed in T19.8 with their reason.

### T19.1 Rules

- T1 and T18.1 apply, with the changes below.
- Meguri's settings section for sync is "Sync". A sentence says "Sync settings". This replaces T1's "Account & Sync" for every key of T19; T1's rule that "Settings" alone is the system's stands.
- The services are named "Meguri Sync" and "iCloud". Where iCloud is a choice, a heading or a row label, `sync.service.beta` follows it as a separate secondary label, never joined into one string; VoiceOver reads `sync.service.iCloud.accessibility`. Inside a sentence iCloud has no label.
- A **needs you** text is exactly one sentence, with at most one action. A **waiting** text has no action, except Try Again on the two acceptance-unknown rows of T19.5 (decided 2026-11-19 (review F4)) and on its paused setup media row (changed 2026-11-21 (resume fix)). An **issues** entry is a row of a list that opens.
- Both services use the same status words (T3's titles and "Last synced").
- No text offers or names the user's Pause, Resume, Connect, Stop Using, a media setting, Review Conflicts or Show Details. Sign Out… is offered only in the signed-in Meguri Sync Account destination (user decision, 2026-11-26; T19.6). No text says that cards and media sync together.

### T19.2 The Service control, Off and switching (N15; V27.5; MP6.1, MP6.4)

| Key | English | Use |
|---|---|---|
| `sync.settings.sync.section` | Sync | the section's title |
| `sync.service.control` | Service | the control's label |
| `sync.service.off` | Off | first value |
| `sync.service.meguri` (T18.2) | Meguri Sync | second value |
| `sync.service.iCloud.name` | iCloud | third value; the iCloud side's heading in `sync.keepCollection` |
| `sync.service.beta` | Beta | the label after iCloud (T19.1) |
| `sync.service.iCloud.accessibility` | iCloud, Beta | accessibility label of the value and of the heading |
| `sync.service.none.footer` (T18.2) | Choose a service to sync this collection. | footer while the value is Off |
| `sync.service.settingUp` | Setting Up… | under the control from the choice until the first durable step of setup (V27.2) or the end of preparation (MP6.3); with `sync.service.cancel` |
| `sync.service.opFirst` | Finish or cancel the collection replacement before you change the service. | under the control when V27.5 refuses the change |
| `sync.switch.busy` (T18.2) | | under the control when MP3 step 1 refuses the change |
| `sync.switch.busy.mediaDownload` | You can change the sync service after the media download finishes. | in place of `sync.switch.busy` while setup's media step runs and files are left to download (changed 2026-11-21 (resume fix)) |
| `sync.switch.busy.media` | You can change the sync service after the media finishes. | the same, with no files left to download recorded |
| `sync.recovery.error.sessionActive` (T13.6) | | under the control when iCloud is chosen during a review session (V27.2) |

The values are in the order Off, Meguri Sync, iCloud. A choice made while the value is Off opens no confirmation (T18.1). Every other change opens the `sync.serviceSwitch` sheet once; its lines are T18.2's, in T18.2's order, with these keys added:

| Key | English | Shown |
|---|---|---|
| `sync.switch.title.off` | Turn Off Sync | sheet title, new value Off |
| `sync.switch.next.iCloud.setup` | Next, Meguri sets up iCloud. It asks before it replaces a collection with your cards. | new value iCloud, in place of `sync.switch.next.iCloud` |
| `sync.switch.next.meguri.setup` | Next, sign in to your Meguri Sync server. Meguri asks before it replaces a collection with your cards. | new value Meguri Sync, in place of `sync.switch.next.meguri` |
| `sync.switch.confirm.off` | Turn Off Sync | destructive button, new value Off |

- New value Off shows `sync.switch.next.localOnly` and, when the old service is Meguri Sync, `sync.switch.signOut`. The old service's pending, held and media lines show for Off as for a switch (MP3 step 1), with `sync.service.iCloud` or `sync.service.meguri` as the old service's name.
- While the change runs, `sync.service.changing` shows; after a failure, `sync.service.changeFailed`. No text says the sign-in was removed or the selection is unchanged until the core reports it (T18.3).
- Added 2026-11-22 (real-copy service fix). An installation whose build cannot use iCloud (no verified container and schema, R2065) still changes between Off and Meguri Sync. A change to iCloud there is refused before anything is written, and the line under the picker is `sync.status.unavailable.setupNeeded` in place of `sync.service.changeFailed`. The control stays enabled after any failure: choosing again is the retry. Copy Debug Info names the destination, step (`preparation`, `updateHold`, `handoff`) and reason of the last change that did not finish, as names only.

### T19.3 Setup and the collection to keep (V27.2, V27.4; MP6.3)

"With your cards" (decided 2026-11-19 (review F12)): a collection that holds no user content (V27.1) can be replaced with no question, so no text promises a question before every replacement.

Under the sign-in fields, in place of `sync.meguri.signIn.footer`:

| Key | English | Shown |
|---|---|---|
| `sync.meguri.signIn.footer.setup` | After you sign in, Meguri sets up syncing with this server, then syncs automatically. It asks before it replaces a collection with your cards. | Sync Automatically reads on (P25.3) |
| `sync.meguri.signIn.footer.setup.manual` | After you sign in, Meguri sets up syncing with this server. It asks before it replaces a collection with your cards. | Sync Automatically reads off |

Progress uses the existing keys: `sync.connect.reading`, `sync.connect.downloading`, `sync.connect.downloadingMedia`, T13.1's and T13.2's step lines, and T18.5's stages.

What setup did, when it asked nothing. One line under the status title in the Sync section and the popover, from the end of the collection's transfer until Meguri is next launched, and an entry of the sync log:

| Key | English | After |
|---|---|---|
| `sync.setup.done.uploaded.iCloud` | This device’s collection was uploaded to iCloud. | V27.2 rows 4 and 6 |
| `sync.setup.done.downloaded.iCloud` | The iCloud collection was downloaded to this device. | V27.2 rows 2 and 5 |
| `sync.setup.done.resumed.iCloud` | This collection syncs with iCloud again. | V27.2 row 3 |
| `sync.setup.done.uploaded.meguri` | This device’s collection was uploaded to your Meguri Sync server. | MP6.3 row 3 |
| `sync.setup.done.downloaded.meguri` | The collection on your Meguri Sync server was downloaded to this device. | MP6.3 rows 1 and 2 |
| `sync.setup.done.created.iCloud` | A new, empty collection was created on this device and syncs with iCloud. | V27.2 row 1 (decided 2026-11-19 (review F6)) |
| `sync.setup.done.backup` | The collection that was on this device is in Backups. | V27.2 row 5 and MP6.3 row 2: it follows the downloaded line (T1 "followed by") |

- While the setup's media still transfers, T5's or T18.6's lines show with it; no line says the setup finished before its media did (MP5, R2050).

The `sync.keepCollection` sheet, in this order. "Service" is iCloud or Meguri Sync.

| Key | English | Shown |
|---|---|---|
| `sync.keep.title` | Which Collection Do You Want to Keep? | sheet title |
| the T19.4 or T19.5 sentence of the row that opened the sheet | | first line, when a status action opened it |
| `sync.keep.message` | Meguri doesn’t merge two collections. The one you don’t keep is replaced. | always |
| `sync.counts.thisDevice` with `sync.counts.notes`, `.cards`, `.reviews`, `.mediaFiles`, `.lastChange` (T11.1) | | this device's side |
| `sync.service.iCloud.name` with `sync.service.beta`, or `sync.service.meguri`, with the same rows; `sync.counts.asOf` when the counts are from the published summary | | the service's side |
| `sync.keep.device` | Keep This Device’s Collection | destructive button |
| `sync.keep.device.effect.iCloud` | The iCloud collection is replaced on all your devices. It stays in iCloud for 30 days and can be restored from there. | under it, iCloud |
| `sync.keep.device.effect.meguri` | The collection on your Meguri Sync server is replaced. Meguri first saves a copy of it on this device. | under it, Meguri Sync |
| `sync.keep.service.iCloud` | Keep the iCloud Collection | destructive button |
| `sync.keep.service.meguri` | Keep the Meguri Sync Collection | destructive button |
| `sync.keep.service.effect` | This device’s collection is replaced. Meguri backs it up on this device first; media files are not in the backup. | under either |
| `sync.service.cancel` (T18.2) | Cancel | the cancel action |

Under each button, after its effect line, the work that the choice loses or that the user must know, each line only when its count is above 0 or its condition holds:

| Under | iCloud | Meguri Sync |
|---|---|---|
| Keep the service's collection | `sync.preview.unsent`, `sync.preview.heldBack`, `sync.preview.isolatedUnsent`, `sync.preview.sentAfterReplacement` with its `.adopt` line, `sync.adopt.mediaWithheld`, `sync.adopt.mediaSuperseded` (T11.2, T13.1) | `sync.meguri.preview.unsent` (T18.5) |
| Keep this device's collection | `sync.preview.notApplied` with `sync.preview.notApplied.noteType`, `sync.preview.sentAfterReplacement` with its `.publish` line, `sync.publish.carriedMedia`, `sync.publish.otherDevices`, `sync.publish.space` or `sync.publish.spaceUnknown`, `sync.decision.unsentInBackup` for a fence (T11.2, T13.2, T10) | `sync.meguri.preview.otherClients`, always (MP4) |
| After both | `sync.preview.otherDevicesUnknown`; `sync.preview.conflictsKept` | `sync.meguri.preview.remoteEmpty` when it applies; `sync.meguri.preview.media`, always; `sync.meguri.preview.automatic` while Sync Automatically reads on |

- Neither destructive button is the default (N11.8). A button is the confirmation: no second sheet or alert follows. After it, the sheet shows the chosen direction's progress (T13.1, T13.2, T18.5) and its errors (T13.6, T18.5) with `sync.recovery.tryAgain`.
- A direction that is not offered (V27.4) shows its button disabled with the reason under it: `sync.recovery.error.sessionActive`, `sync.recovery.error.accountUnavailable`, or one of:

| Key | English |
|---|---|
| `sync.keep.unavailable.none` | There is no collection in iCloud to keep. |
| `sync.keep.unavailable.unreadable` | The iCloud collection can’t be read, so it can’t be kept. |

- **One direction** (V27.4, V27.6: lost iCloud data, another account with one side empty). The sheet is T12's: `sync.connect.upload.title`, the row's sentence, `sync.connect.upload.message`, this device's counts and `sync.connect.upload.confirm`; or `sync.connect.useICloud.title`, the row's sentence, `sync.connect.useICloud.message`, both sides' counts and `sync.connect.useICloud.confirm`. These keys are shown nowhere else.
- **One direction, Meguri Sync** (added 2026-11-20; `sync/meguri-provider#MP6.3`'s lines "Changed 2026-11-20", under review there: these forms are shown as that approval allows). The sheet keeps `sync.keepCollection`'s order with one destructive button and Cancel. The keys below take the place of `sync.keep.title`, `sync.keep.message` and the button's key:

| Key | English | Shown |
|---|---|---|
| `sync.keep.uploadOnly.title` | Upload This Device’s Collection? | sheet title, upload only: a changed endpoint or account, the server proved to hold no user content |
| `sync.keep.uploadOnly.message.meguri` | This device synced with another server or account before. The account you signed in to has no collection yet, so Meguri can only upload this device’s collection to it. | in place of `sync.keep.message` |
| `sync.keep.uploadOnly.confirm` | Upload This Device’s Collection | the destructive button, in place of `sync.keep.device` |
| `sync.keep.downloadOnly.title.meguri` | Download the Meguri Sync Collection? | sheet title, download only: no collection on this device, and media files or a media folder that cannot be read |
| `sync.keep.downloadOnly.message.meguri` | This device has no collection, but it may have media files. Meguri can only download the collection from your Meguri Sync server. | in place of `sync.keep.message` (changed 2026-11-20 from the built "This device has media files but no collection. …", which is not true when the folder cannot be read) |
| `sync.keep.downloadOnly.confirm` | Download the Collection | the destructive button, in place of `sync.keep.service.meguri` |

- Upload only: this device's counts; the service's side has `sync.meguri.preview.server` with the endpoint, `sync.settings.account` with the account signed in to, and the counts of the retained server copy. Under the button only `sync.meguri.preview.otherClients`. `sync.keep.device.effect.meguri` is not shown: the server holds no collection with user content, so nothing there "is replaced", and no text says it is. Changed 2026-11-20 from the built sheet, which shows it under this button too (not built).
- Download only: this device's side has only `sync.counts.mediaFiles`, and only when the folder can be read; the service's side has `sync.meguri.preview.server` and `sync.settings.account`, with no counts. Under the button no effect line: nothing on this device is replaced or backed up, so `sync.keep.service.effect` is not shown.
- After the button, in both forms: `sync.meguri.preview.media`, always, since the media reconciliation can change this device's files; `sync.meguri.preview.automatic` while Sync Automatically reads on.
- When the facts change while the sheet is open, `sync.recovery.error.epochChanged` (iCloud) or `sync.meguri.setup.error.remoteChanged` or `.localChanged` (Meguri Sync) shows, the counts reload, and a new choice is needed.

### T19.4 The three kinds: iCloud (`sync/policy#P14`, `#P25.4`; V11, V27)

How the main status (the sync button's value, the popover and the Sync section's status row) is made:

1. When a **needs you** row below applies: the title `sync.status.needsAttention.title`, the row's sentence as the detail, and the row's action as one button. With several, the first in the table's order shows; the next shows once it is settled.
2. Otherwise: the T3 title and detail of the P14.2 state, or, in the state `needsAttention`, of `stateWithoutAttention` (P25.4). When a **waiting** row applies, its text is the detail, the first in the table's order.
3. When one or more **issues** rows apply: one row under the status, `sync.issues.row`, which opens the list. It shows with either of the above.
4. T3's `sync.status.keptLocally` rule, T4's lines and T5's media lines are unchanged.

| Key | English |
|---|---|
| `sync.status.off.title` | Sync Off (the title of `localOnly`, with `sync.status.localOnly.detail`) |
| `sync.issues.row` | `%lld sync issue` / `%lld sync issues` (the number of issue rows, not of their items) |
| `sync.issues.title` | Sync Issues (the list's title) |
| `sync.issues.footer` | Your other changes keep syncing. |

States (`sync/policy#P14.2`):

| State | Kind | Text | Action |
|---|---|---|---|
| `localOnly` | none; the Service control reads Off | `sync.status.off.title`, `sync.status.localOnly.detail` | |
| `iCloudUnavailable{noAccount}` | needs you | `sync.status.unavailable.noAccount` | none: Apple gives no way to open the account pane (T3) |
| `iCloudUnavailable{restricted}` | needs you | `sync.status.unavailable.restricted` | none |
| `iCloudUnavailable{temporary}` | waiting | `sync.status.unavailable.temporary` | |
| `iCloudUnavailable{setupNeeded}` | needs you | `sync.status.unavailable.setupNeeded` | `sync.action.copyDebugInfo` |
| `paused{decisionRequired}` | needs you | by `pendingDecisionReason`: `sync.needsYou.fence.restore`: Choose which collection to keep after restoring a backup. `sync.needsYou.fence.import`: Choose which collection to keep after importing a collection. `sync.needsYou.fence.none`: Choose which collection to keep. | `sync.decision.action`: opens `sync.keepCollection` |
| `paused{userPaused}` | never reported (P25.2) | | |
| `paused{automaticOff}`, the update hold included | none | T3 | |
| `offline` | waiting | T3 | |
| `syncingCollection`, `syncingMedia`, `changesPending`, `upToDate` | none | T3 | |

Attention items (`sync/policy#P14.4`; the T9.1 rows), needs you first, in the order that decides which shows:

| # | Item (T9.1 row) | Kind | Sentence | Action |
|---|---|---|---|---|
| 1 | `collectionCorrupt` | needs you | `sync.attention.collectionCorrupt` | `sync.needsYou.action.repair`: Show Repair Options. Shows the damaged-collection screen (V19) |
| 2 | `localStorageFull` | needs you | `sync.needsYou.localStorageFull`: This device is out of storage space, so syncing stopped. | `sync.action.tryAgain` |
| 3 | `trackingError` | needs you | `sync.attention.trackingError` | `sync.action.tryAgain` |
| 4 | `unsupportedSchema` | needs you | `sync.needsYou.update`: Update Meguri on this device to sync all your changes. | none |
| 5 | `isolated`, `signed_out` (`signedOut`) | needs you | `sync.attention.signedOut` | `sync.action.resume` while V9.2 offers it (V27.6: no sheet follows); otherwise `sync.needsYou.action.setUp` |
| 6 | `isolated`, `account_changed`, another identity (`accountChanged`) | needs you | `sync.needsYou.accountChanged`: You’re signed in to a different iCloud account, and this collection was not sent to it. | `sync.decision.action`: `sync.keepCollection`, always (V27.6) |
| 7 | `isolated`, `account_changed`, the binding's identity again (`accountChangedBack`) | needs you | `sync.attention.accountChangedBack` | as row 5 |
| 8 | `isolated`, `epoch_switched` (`replacedElsewhere`) | needs you | `sync.attention.replacedElsewhere` | `sync.decision.action` |
| 9 | `isolated`, `zone_missing`, `active_missing` (`missing`) | needs you | `sync.needsYou.missing`: This collection is missing from iCloud, and it is still on this device. | `sync.decision.action` (V27.6: one direction unless V10.1 offers V14) |
| 10 | `isolated`, `zone_deleted_by_user` (`deleted`) | needs you | `sync.needsYou.deleted`: Meguri’s iCloud data was deleted, and your collection is still on this device. | as row 9 |
| 11 | `isolated`, `encrypted_data_reset` (`encryptionReset`) | needs you | `sync.needsYou.encryptionReset`: The iCloud copy can no longer be read because your iCloud encrypted data was reset. | as row 9 |
| 12 | `isolated`, `file_mismatch` (`fileMismatch`) | needs you | `sync.attention.fileMismatch` | `sync.needsYou.action.setUp` |
| 13 | `isolated`, any other reason | needs you | `sync.attention.isolated` | `sync.needsYou.action.setUp` |
| 14 | `rootUnreadable` | needs you | `sync.needsYou.rootUnreadable`: Meguri can’t read which collection is current in iCloud. | `sync.decision.action` (keeping iCloud's is not offered) |
| 15 | `chooseCollection`, V13.6's decision | needs you | `sync.attention.chooseCollection` | `sync.decision.action` |
| 16 | `chooseCollection`, otherwise (no binding with iCloud selected, a setup that failed before its first durable step) | needs you | `sync.needsYou.setUp`: iCloud isn’t set up for this collection yet. | `sync.needsYou.action.setUp` |
| 17 | `crtMismatch` | needs you | `sync.attention.crtMismatch` | `sync.decision.action` |
| 18 | `mediaZoneMissing` | needs you | `sync.needsYou.mediaMissingInICloud`: Media files can’t sync because Meguri’s media data is missing from iCloud. | `sync.decision.action` (one direction: V15) |
| 19 | `recoveryOpWaiting`, `adoptUnproven` | needs you | `sync.attention.adoptUnproven` | `sync.action.cancelOp` |
| 20 | `recoveryOpWaiting`, `publishConflict` | needs you | `sync.attention.publishConflict` | `sync.decision.action`: keeping iCloud's is V15.2 step 5's "use that collection", keeping this device's is `republish` |
| 21 | `publishQuota` (in place of `quotaFull`, T9.1) | needs you | `sync.attention.publishQuota` | `sync.action.cancelOp` |
| 22 | `recoveryOpWaiting`, `publishAccount` | needs you | `sync.attention.publishAccount` | `sync.action.resumeOp` while V15.3 offers it; otherwise `sync.action.cancelOp` |
| 23 | `recoveryOpWaiting`, `publishWaiting`, while V15.3 offers Resume | needs you | `sync.attention.publishWaiting` | `sync.action.resumeOp` |
| 23a | `recoveryOpWaiting`, an `adopt` op in V14.3's staging transport wait, while V14.3 offers Resume (added 2026-11-20) | needs you | `sync.attention.adoptWaiting` (T13.7) | `sync.action.resumeOp`: `resume_recovery_op` (V26.2), the only call that repeats the pending staging work |
| 23b | `recoveryOpWaiting`, the same op, while V14.3 does not offer Resume (added 2026-11-20; V14.3 offers Resume throughout the transport wait, so this row is the fallback for an op whose Resume is not offered) | waiting | `sync.attention.adoptWaiting` | |
| 24 | `quotaFull` | needs you | `sync.needsYou.quotaFull`: Your iCloud storage is full, so changes stay on this device. | `sync.action.tryAgain` |
| 25 | `generationIncomplete`, while V18 offers Abandon | needs you | `sync.attention.generationIncomplete` | `sync.action.abandon` |
| 26 | `recoveryOpWaiting`, `publishWaiting`, otherwise | waiting | `sync.attention.publishWaiting` | |
| 27 | `recoveryOpWaiting`, `adoptAfterReview` | waiting | `sync.attention.adoptAfterReview` | |
| 28 | `recoveryOpWaiting`, `abandonWaiting` | waiting | `sync.attention.abandonWaiting` | |
| 29 | `cloudError` | waiting | `sync.waiting.cloudError`: iCloud sync isn’t working right now. Meguri keeps trying. | |
| 30 | `exchangeIncomplete` | waiting | `sync.attention.exchangeIncomplete` | |
| 31 | `referenceWaiting` | waiting | `sync.attention.referenceWaiting` | |
| 32 | `assetUnavailable` | waiting | `sync.attention.assetUnavailable` | |
| 33 | `mediaChanged` | waiting | `sync.attention.mediaChanged` | |
| 34 | `conflictsUnreadable` | issues | `sync.attention.conflictsUnreadable` | |
| 35 | `refused` | issues | `sync.attention.refused` | |
| 36 | `oversize` | issues | `sync.attention.oversize` | |
| 37 | `unreadable` | issues | `sync.attention.unreadable` | `sync.action.checkDatabase`, in its row of the list |
| 38 | `unsyncable` | issues | `sync.attention.unsyncable` | |
| 39 | `claimStuck` | issues | `sync.attention.claimStuck` | |
| 40 | `generationIncomplete`, otherwise | issues | `sync.attention.generationIncomplete` | |
| 41 | `applyFailed` | issues | `sync.attention.applyFailed` | |
| 42 | `rekeyFailed` | issues | `sync.attention.rekeyFailed` | |
| 43 | `mediaMissing` | issues | `sync.attention.mediaMissing` | |
| 44 | `conflicts` | none: no entry in the main status (V27.7) | `sync.trouble.conflictCopies` in Troubleshooting (T19.6) | none |

Actions added:

| Key | English | Runs |
|---|---|---|
| `sync.needsYou.action.setUp` | Set Up Again… | V27.2's table for iCloud; it may end with no question |
| `sync.needsYou.action.repair` | Show Repair Options | the damaged-collection screen |

- The error classes of T8 are unchanged. `offline`, `throttled` and `cloudError` as `sync.details.error` are waiting texts: the engine retries them. Media downloads paused for free space keep `sync.media.noSpace` (T5) as a media line, as before.
- `sync.action.resume`, `sync.action.resumeOp` and `sync.action.tryAgain` run at once. `sync.action.cancelOp` and `sync.action.abandon` keep their confirmations (T13.5, T13.3). `sync.action.tryAgain` is enabled as Sync Now is (T9.2).
- A Cancel Replacement that is not a row's action (rows 22, 23, 23a, 23b, 26, 27, while V14.3 or V15.3 allows it) is in Troubleshooting (T19.6).
- An issues row of the list shows its T9.1 text and, under it, its T11.3 items, which Show Details used to open. `applyFailed` and `rekeyFailed` rows add `sync.item.error`. The list's only button is row 37's. Copy Debug Info is in Troubleshooting.
- Rows 23a and 23b (2026-11-20) number in between so that no row is renumbered; they are in the order that decides which shows.
- Rows 4 and the two `iCloudUnavailable` rows have no action, because none exists on the platform; they stay needs you, since only the user can end them.

### T19.5 The three kinds: Meguri Sync (T18.4 to T18.6; MP6)

The same four rules make the main status. The first condition that applies decides, in T18.4's order.

| Condition (T18.4, T18.5, T18.6) | Kind | Text | Action |
|---|---|---|---|
| provider phase handoff recovery | needs you | `sync.needsYou.switchUnfinished`: Changing the sync service didn’t finish, so nothing syncs. | `sync.service.recovery.action`, with the `sync.service.recovery.title` alert (T18.2) |
| provider phase blocked | needs you | `sync.needsYou.blocked`: Meguri can’t read this device’s sync settings, so nothing syncs, and Export Collection still saves everything. (decided 2026-11-19 (review F5); the Service control stays enabled, MP6.5) | `sync.action.copyDebugInfo` (MP6.5) |
| connection blocked | needs you | `sync.needsYou.meguri.signInStore`: Meguri can’t read or save your Meguri Sync sign-in on this device. | `sync.meguri.connection.retry` |
| connection unconfigured, signed out or cancelled | needs you | `sync.meguri.status.signedOut.detail` | `sync.meguri.signIn`: the fields of T18.3 |
| connection authenticating | none | `sync.meguri.status.signingIn.title` | |
| structure upload pending (`sync/meguri-provider#MP7` step 5), while no MP7 job runs and no setup stage other than "setup required" is recorded; in place of MP6.3's collection choice for this binding (while a job runs it shows the job's own title, kind none); while the server refuses the sign-in (round failed, authentication), this row shows the text and action of the row "setup failed, `authentication`; round failed, authentication" instead (`sync.needsYou.meguri.signIn`, `sync.meguri.signIn`), so the sign-in comes before the upload; and while acceptance is unknown, the acceptance-unknown row shows first ("setup acceptance unknown", which this row's stage condition already leaves out, or "round failed, acceptance unknown", whose text and Try Again show instead), as MP7 step 5 says a lost acknowledgement does (added 2026-11-22 (structure upload)) | needs you | `sync.needsYou.meguri.structureUpload`: Upload this collection to finish your note type changes. | `sync.needsYou.meguri.action.upload`: Upload…, runs MP7 step 5's upload |
| setup required, before preparation starts | none | `sync.service.settingUp` | |
| setup preparing | none | `sync.meguri.status.preparing.title`, `sync.meguri.setup.preparing` | `sync.meguri.cancel` |
| setup preview | none; `sync.keepCollection` is open | | |
| setup transferring the collection (changed 2026-11-21 (resume fix): transferring media has the row below) | none | T18.5 | |
| setup transferring media (changed 2026-11-21 (resume fix): in place of T18.5's row) | none | title `sync.meguri.status.downloadingMedia.title`: Downloading Media…, while files are left to download, otherwise `sync.meguri.status.transferringMedia.title`; detail `sync.meguri.media.left` with the files left, or `sync.meguri.media.left.unknown`, as for the paused row below. T18.5's `sync.meguri.setup.collectionDone` and its following `sync.meguri.media.filesLeft` line are not shown | |
| setup acceptance unknown | waiting | `sync.meguri.setup.acceptanceUnknown` | `sync.recovery.tryAgain`: runs MP6.9's settling step (decided 2026-11-19 (review F4)) |
| setup or round media blocked | issues | `sync.meguri.media.notTransferred` as the row that opens Affected Files (T18.6), in place of `sync.issues.row`; `sync.meguri.media.blocked` as the status detail | `sync.meguri.media.retry`, in the list. Changed 2026-11-21 (resume fix): while no affected files are known, the row is waiting, with no issues row, and its action `sync.recovery.tryAgain` is on the status row |
| setup media paused: the media step stopped before it finished and continues by itself (`sync/meguri-provider#MP6.5`; changed 2026-11-21 (resume fix)) | waiting | title `sync.meguri.status.downloadPaused.title`: Media Download Paused, while files are left to download as last recorded, otherwise `sync.meguri.status.transferPaused.title`: Media Transfer Paused; detail `sync.meguri.media.left`: `%lld media file is left. Syncing starts when it finishes.` / `%lld media files are left. Syncing starts when they finish.`, with the files left the core recorded, or `sync.meguri.media.left.unknown`: Syncing starts when the media finishes., when none is recorded | `sync.recovery.tryAgain`: runs the media step now |
| setup media stopped: its automatic attempts are used up, the device is full, or a refusal a later attempt is not known to clear (MP6.5; changed 2026-11-21 (resume fix)) | needs you | `sync.needsYou.meguri.mediaStopped.download`: Downloading the media didn’t finish, so syncing hasn’t started., while files are left to download as last recorded, otherwise `sync.needsYou.meguri.mediaStopped`: Transferring the media didn’t finish, so syncing hasn’t started. | `sync.recovery.tryAgain`: runs the media step now, and its automatic attempts start again |
| the server accepted the upload, local activation unfinished | needs you while the core permits Finish Setup, otherwise waiting | `sync.needsYou.meguri.finishSetup`: The server accepted this device’s collection, but setup didn’t finish; finishing replaces any changes made here since the upload, and Meguri backs them up first. (Changed 2026-11-20, with `sync/meguri-provider#MP6.9`'s line under review; it said "…but setup didn’t finish on this device.", which left out that finishing replaces this device's later changes. Not built.) | `sync.meguri.setup.finish` |
| setup failed, `activationInterrupted` | needs you while the core permits Finish Setup, otherwise waiting | `sync.needsYou.meguri.activation`: Replacing this device’s collection didn’t finish, and your backup is kept. | `sync.meguri.setup.finish` |
| setup failed, `authentication`; round failed, authentication | needs you | `sync.needsYou.meguri.signIn`: The server didn’t accept your sign-in, so Meguri Sync stopped. | `sync.meguri.signIn` |
| setup failed, `identityChanged` | needs you | `sync.needsYou.meguri.identityChanged`: The server address or account changed, so you need to sign in again. | `sync.meguri.signIn` |
| setup failed, `storage` | needs you | `sync.needsYou.meguri.storage`: Meguri couldn’t save to this device, so setup stopped. | `sync.recovery.tryAgain` |
| setup failed, `localUnusable`: a collection file is on this device but is not open, damaged or closed (`sync/meguri-provider#MP6.3`, added 2026-11-20) | needs you | `sync.needsYou.meguri.localUnusable`: Meguri can’t open the collection on this device, so setup stopped. (New key, 2026-11-20, with MP6.3's line under review; the built app shows `sync.attention.collectionCorrupt` here, which says the collection is damaged although it may only be closed. Not built.) | `sync.action.copyDebugInfo` |
| setup failed, `remoteUnavailable`, `remoteChanged`, `localChanged`, `backupFailed`, `server`, `invalidDownload` | needs you | `sync.needsYou.meguri.setupFailed`: Setting up Meguri Sync didn’t finish, and nothing was replaced. | `sync.recovery.tryAgain`: runs MP6.3 again from preparation |
| setup failed, `cancelled` | none: the service is Off (MP6.3) | | |
| connection setup required, no setup stage and no round | waiting | `sync.meguri.status.setupUnavailable` | |
| paused | never reported (MP6.2) | | |
| round idle | none | `sync.status.upToDate.title`, `sync.status.upToDate.detail` (MP6.6) | |
| round with pending work | none | `sync.status.pending.title`, `sync.status.pending.detail` | |
| round exchanging the collection | none | `sync.status.syncing.title` | |
| round exchanging media | none | `sync.status.syncingMedia.title` | |
| round failed, acceptance unknown | waiting | `sync.meguri.round.acceptanceUnknown` | `sync.recovery.tryAgain`: runs MP6.9's settling step (decided 2026-11-19 (review F4)) |
| round failed, full sync required | needs you | `sync.needsYou.meguri.chooseCollection`: Choose which collection to keep before syncing continues. | `sync.decision.action`: opens `sync.keepCollection` (MP6.3 row 4) |
| round failed, network | waiting | `sync.meguri.round.network` | |
| round failed, server | waiting | `sync.meguri.round.server` | |
| round failed, cancelled | none | as round with pending work | |

- The failure's own T18.5 line is the first line of that attempt's entry in the sync log, so the reason is not lost when the status shows one sentence.
- MP6.9's outcomes show in these rows as MP6.9 says: "neither" (the upload was overtaken) is "round failed, full sync required"'s sentence and action during setup too, and Cancel in the sheet it opens leaves the service Off, because that setup never finished; "indistinct" stays the acceptance-unknown row (added 2026-11-20).
- `sync.needsYou.meguri.localUnusable` makes no claim about the cause: the file may be damaged or only closed. The local-only screens of V19 still apply to a damaged collection (added 2026-11-20).
- `sync.needsYou.meguri.setupFailed` says nothing was replaced, which MP4 makes true for each failure it covers: every one comes before activation or dispatch. `activationInterrupted` has its own row and makes no such claim.
- With Sync Automatically off, the status of an idle round is T3's `sync.status.automaticOff.title` and `.detail`, as for iCloud.

### T19.6 The Sync section and Troubleshooting (N15)

| Key | English | Use |
|---|---|---|
| `sync.settings.account` | Account | row, Meguri Sync only: its value is the username; it opens the page with Server Address, Username and Sign Out… while signed in, and the sign-in fields while not signed in (T18.3; user decision, 2026-11-26) |
| `sync.settings.account.signOut` | Sign Out… | native destructive button in the signed-in Account destination only (added 2026-11-26); opens T21.1's Service → Off alert and reuses its progress, refusal and error text; the withdrawn `sync.meguri.signOut` keys stay withdrawn |
| `sync.settings.automatic` (T14) | Sync Automatically | switch, shown while a service is selected; disabled, showing its stored value, until the service is connected (`sync/policy#P25.3`; decided 2026-11-19 (review F8)) |
| `sync.settings.automatic.footer` (T14) | | its footer |
| `sync.now.command` (T6) | Sync Now | button |
| `sync.settings.troubleshooting` | Troubleshooting | row; opens the page; also its title |
| `sync.trouble.replaceDevice` | Replace This Device’s Collection… | iCloud: T13.1's preview with the active source; Meguri Sync: T18.5's `useServer` preview (MP6.7) |
| `sync.trouble.replaceService.iCloud` | Replace the iCloud Collection… | T13.2's preview |
| `sync.trouble.replaceService.meguri` | Replace the Meguri Sync Collection… | T18.5's `useDevice` preview (MP6.7) |
| `sync.adopt.previous` (T13.1) | | iCloud, while V14.1 offers the previous source |
| `sync.action.cancelOp` (T9.2) | | while an op can be cancelled and Cancel is not its status action (T19.4) |
| `sync.trouble.log` | Sync Log | opens the log of real recorded attempts (N7's rules) |
| `sync.action.copyDebugInfo` (T9.2) | Copy Debug Info | always |
| `sync.status.openSettings` | Sync Settings | the status popover's button that opens the Sync section (added 2026-11-20; T19.7: rows that opened Account & Sync open the Sync section) |
| `sync.trouble.conflictCopies` | `%lld conflict copy is saved on this device.` / `%lld conflict copies are saved on this device.` | iCloud, one plain line with no action, while P14.4's `conflicts` is above 0 (V27.7) |

- `sync.settings.privacy` (T14) stays the Sync section's footer while iCloud is the service.
- The two replace entries show for the selected service once it is set up. Each opens its preview and changes nothing; the preview's destructive button is the confirmation (N11.8). While the entry cannot run, it is disabled with its T13.6 or T18.5 reason under it.
- The parent Sync section shows no media toggle, no Pause, no Sign Out, no Stop Using row and no Advanced group. Account's Sign Out… uses MP6.4's guarded Off handoff. Cancel keeps Account open; success returns to Sync settings at Off, where choosing Meguri Sync shows the editable sign-in fields.
- Replaced media (decided 2026-11-19 (review F7); V27.7), in Troubleshooting, only while replaced-media copies exist:

| Key | English | Use |
|---|---|---|
| `sync.trouble.replacedMedia` | Show Replaced Media | row; opens the copies' folder in the Files app, or in Finder on Mac |
| `sync.trouble.replacedMedia.footer` | Media files that sync replaced are kept here until you delete them. | the row's footer |

### T19.7 Sync Now (T6, T18.7)

| Reason | Key | English |
|---|---|---|
| `notConnected` | `sync.now.unavailable.off` | Sync is off. Choose a service in Sync settings to sync this collection. |
| Meguri Sync: Sign In Required | `sync.meguri.now.unavailable.signIn.sync` | Sign in to Meguri Sync in Sync settings to sync. |
| Meguri Sync: handoff recovery, or provider or connection blocked | `sync.meguri.now.unavailable.stopped.sync` | Meguri Sync is stopped. Open Sync settings to see what to do. |

- Changed 2026-11-21 (resume fix). While Meguri Sync's setup has not finished, Sync Now is disabled with no reason line; on the Mac its help tag is `sync.meguri.now.unavailable.setup`: Available when setup finishes., in the Sync section and the panel.
- The reason `paused` no longer exists for either service (P25.2, MP6.2). During the update hold Sync Now is available, and choosing it ends the hold.
- T18.7's rows that open Account & Sync open the Sync section. For "round failed, full sync required" the popover shows the status action `sync.decision.action` in place of `sync.meguri.now.chooseCollection`.
- Every other row of T6 and T18.7 is unchanged.

### T19.8 Keys that become unused

| Section | Keys |
|---|---|
| T3 | `sync.status.localOnly.title`, `sync.status.paused.user`, `sync.status.needsAttention.count` |
| T5 | `sync.media.off`; in `sync.media.waiting`'s condition, "media is on" always holds |
| T6 | `sync.now.unavailable.notConnected`, `sync.now.unavailable.paused` |
| T7 | `sync.result.media.paused` |
| T9.1 | `sync.attention.accountChanged`, `.missing`, `.deleted`, `.encryptionReset`, `.rootUnreadable`, `.conflicts`, `.mediaMissingInICloud`, `.quotaFull`, `.localStorageFull`, `.unsupportedSchema`, `.cloudError`, `sync.attention.storageGuidance` |
| T9.2 | `sync.action.connect`, `.useICloud`, `.replaceICloud`, `.keepLocal`, `.reviewConflicts`, `.showDetails`, `.syncNow`, `.useThat`, `.replaceAgain` |
| T10 | `sync.decision.sheetTitle`, `sync.decision.title.restore`, `.title.import`, `.title.none`, `sync.decision.message`, `.replace`, `.replace.caption`, `.keepLocal`, `.keepLocal.caption`, `.notNow` |
| T12 | `sync.connect.action`, every `sync.connect.two.*`, every `sync.connect.continue.*` |
| T13.1, T13.2 | `sync.adopt.action`, `sync.publish.action` |
| T13.4, T13.6 | every `sync.conflicts.*`, `sync.recovery.error.conflictNotFound`, `sync.recovery.error.recoveryInvalid`: not shown (V27.7). Exception to the removal rule: they stay in the catalog, because V17.3's and V17.4's operations stay in the core and a later review screen uses these texts |
| T14 | `sync.settings.section`, `sync.settings.media`, `.media.footer`, `sync.settings.pause`, `.resumePause`, `sync.settings.stop`, `.stop.title`, `.stop.message`, `.stop.confirm`, every `sync.keepLocal.*`, `sync.settings.conflicts`, `sync.settings.advanced` |
| T18.2 | `sync.service.label`, `sync.service.recovery`, `sync.switch.title.localOnly`, `sync.switch.confirm.localOnly`, `sync.switch.next.iCloud`, `sync.switch.next.meguri` |
| T18.3 | `sync.meguri.signIn.footer`, `sync.meguri.signOut`, `.signOut.title`, `.signOut.message`, `.signOut.confirm`, `sync.meguri.signingOut`, `sync.meguri.signOut.failed`, `sync.meguri.stop` |
| T18.4 | `sync.meguri.status.connectionBlocked.detail`, `sync.meguri.status.paused.title`, `.paused.detail`, `sync.meguri.status.on.title`, `sync.meguri.lastExchange`, `sync.meguri.status.pending.title`, `.pending.detail`, `sync.meguri.status.syncingCollection.title`, `sync.meguri.status.syncingMedia.title`, `sync.meguri.status.signInRequired.detail`, `sync.meguri.round.fullSyncRequired`, `sync.meguri.round.cancelled` |
| T18.5 | `sync.meguri.setup.choose`, `.useDevice`, `.useServer`, `.download.detail`, `.download`, `.review`, `sync.meguri.setup.error.cancelled`, `sync.meguri.preview.title.download`, `.direction.download`, `.confirm.download` |
| T18.7 | `sync.meguri.now.unavailable.paused`, `.signIn`, `.stopped`, `sync.meguri.pause`, `sync.meguri.resume`, `sync.meguri.now.chooseCollection` |
| T18.5, T18.6 (changed 2026-11-21 (resume fix): one explanation in the status row) | `sync.meguri.setup.collectionDone`, `sync.meguri.media.unverified`, `sync.meguri.media.setupWaits`, `sync.meguri.media.study` |

- Every key of the table above is removed from the catalog when T19 ships, except the T13.4 and T13.6 row (decided 2026-11-19 (review F12)). `sync.service.blocked` (T18.2) is not on the list: it stays in use as the line under the picker, and only the needs-you sentence is `sync.needsYou.blocked` (decided 2026-11-19 (re-review R3)).
- Still used, though their flow changed: `sync.connect.upload.*` and `sync.connect.useICloud.*` (T19.3, one direction only); `sync.service.iCloud` (the old service's name in the `sync.serviceSwitch` lines); `sync.decision.action`, `.action.label` and `.unsentInBackup`; the `useDevice` and `useServer` preview keys of T18.5 (Troubleshooting); `sync.status.needsAttention.title`; the T9.1 texts that T19.4 names.

### T19.9 Tests

- `SyncStringCatalogTests` (T16) gets a row for each key of T19, and checks that the English value of every needs you text of T19.4 and T19.5 is one sentence: one sentence-ending mark, at its end.
- `SyncKindsPresentationTests` (in `app/Packages/MeguriKit/Tests/AppModelTests/`): each row of T19.4's two tables and of T19.5 gives its kind, its text key and its action; with two needs you rows the first in order shows; with only waiting or issues rows the title is `stateWithoutAttention`'s; `conflicts` alone changes nothing in the main status and shows `sync.trouble.conflictCopies` in Troubleshooting; a failed Meguri setup's own line is in the log entry.
- A source check that no view of the app names a key of T19.8.
- U161's snapshots cover the Sync section for Off, each service set up, a needs you row, an issues row, `sync.keepCollection` for both services and in its one-direction form, and Troubleshooting for both services.

## T20. The status panel (2026-11-21, round 10)

Changed 2026-11-21 (round 10). Decided by Antoine on 2026-11-20 (`docs/design/decisions.md`, row `sync.statusPopover`, answer 6A; `docs/design/native-interface-contract.md` N23). In force: approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/round10-sync-panel-20261121.md`). The text of the panel that the sync status button opens, for both services. It replaces, for the popover only, the parts of T2, T3, T6, T7, T8, T9.2, T18.3, T18.7, T19.3, T19.4 and T19.7 that N23.5 lists; T19.4 and T19.5 still make the status, and no state, condition or existing value changes.

### T20.1 What the panel shows

| Part | Text | Shown |
|---|---|---|
| status line | the main status's title (T19.4 rule 1 or 2; T19.5) | always |
| sentence | the Text column of T19.4 or T19.5 for the row that applies, or, where T19.4 rule 2 applies, the T3 detail of the state; for an up-to-date status `sync.status.upToDate.detail` with its instant (MP6.6) | when the status has a detail that differs from the title |
| files left | `sync.meguri.media.filesLeft`: `%lld media file left` / `%lld media files left` | under the bar, while media transfers and a count is known: iCloud's `syncingMedia` (its upload and download files). Changed 2026-11-21 (resume fix): not for Meguri Sync, whose status sentence carries its files left (T19.5) |
| issues | `sync.issues.row`, or `sync.meguri.media.notTransferred` for Meguri Sync's media-blocked row (T19.5) when the affected count is known, else `sync.issues.row` | while the status has issues; plain text, no link. Changed 2026-11-21 (resume fix): Meguri Sync's media-blocked row has issues only while its affected count is known and above 0 (T19.5) |
| button | `sync.status.openSettings`: Sync Settings | always |
| button | `sync.now.command`: Sync Now | in the action's place, as N23.3 says |
| button | the status action's key (T19.4, T19.5) | in the action's place, as N23.3 says |
| button | `sync.status.signIn`: Sign In… | in place of `sync.meguri.signIn` for Meguri Sync's needs you rows whose action it is; opens the Sync section at the sign-in fields |
| button | `sync.status.cancelSetup`: Cancel Setup | in place of `sync.meguri.cancel` while setup prepares; never the default |

- `sync.meguri.media.filesLeft` names no service, so iCloud's panel uses it too; T5's `sync.media.filesLeft` stays the media indicator's text beside the button.
- The panel shows none of T4's lines, T5's other lines, T7's step and result lines or T19.3's setup-done lines; the Sync section's status row keeps them. VoiceOver still announces the end of a Sync Now the person started, by its T7 title (T1).
- No reason line under a disabled Sync Now or Try Again. The reason stays the Mac help tag of the panel's button and the line under the Sync section's Sync Now and File › Sync Now (T19.7).
- Copy Debug Info (`sync.action.copyDebugInfo`) is a button of the panel only where it is the status action as built: T19.4's `setupNeeded` state and row 1 (until the damaged-collection screen exists, row 1's built action is Copy Debug Info), and T19.5's blocked provider and `localUnusable` row.
- The button's accessibility label (T2): `sync.button.label` with iCloud selected, `sync.meguri.button.label` with Meguri Sync, `sync.button.label.off` with no service.

### T20.2 Keys added

| Key | English | Use |
|---|---|---|
| `sync.status.signIn` | Sign In… | T20.1; the label ends with "…" because it opens the fields and changes nothing (T1) |
| `sync.status.cancelSetup` | Cancel Setup | T20.1; names what it cancels, since a plain Cancel in a panel reads as closing the panel |
| `sync.button.label.off` | Sync | the sync button's accessibility label while no service is selected |

### T20.3 Tests

- `SyncStringCatalogTests`: a row for each key of T20.2.
- `SyncPanelPresentationTests` (`app/Packages/MeguriKit/Tests/AppModelTests/`), over `SyncPanel` (AppModel): each row of N23.3's table gives its buttons and which is the default, or none; Sync Now stays, disabled, while the status moves; a needs you row with a built action shows that action and no Sync Now; with no service the only button is Sync Settings; an issues row's action never reaches the panel; Finish Setup, Resume and Cancel Setup are never the default; the bar's value for Meguri Sync's setup equals the Account row's value; iCloud's media transfer under an up-to-date state shows no bar; a detail equal to the title is not shown; both services' states map to N23.2's five shapes, iCloud's to the `icloud` symbols and Meguri Sync's and Off's to the drawn shapes, with no `icloud` symbol outside iCloud.
- The panel's states rendered in the test VM through the routes `settings/sync/panel/<state>` and `settings/sync/status/<state>` (Mac, iPad, iPhone; light and dark).
- `SyncStatusPanelKeyTests` (Mac UI test, in the VM): Return runs the default button (Sync Off: Sync Settings opens the Settings window); Esc closes the panel and runs nothing. It is recorded as queued while macOS drops XCUITest input (`mac-input`) and runs again from `tools/mg catch-up`.

## T21. The service change confirmation and setup not finished (2026-11-22)

Changed 2026-11-22. Antoine, on the Mac's Turn Off Sync sheet: "This screen doesn't look good at all maybe we can redesign it?"; on the Sync page before the first sign-in: "does it make sense to show the needs attention, sync automatically, sync now if you still haven't signed in at all… Think of what would be like the ideal page here." Decided by Claude (coordinator) under Antoine's delegation, with his standing permission of 2026-11-20 to update specs (`docs/design/decisions.md`, rows `sync.serviceSwitch` and `settings.accountSync` of 2026-11-22; `docs/design/native-interface-contract.md` N15.7). Where T18.2, T19.2 or T19.5 differ, T21 wins. No state, condition or behaviour of the change itself changes (MP3 step 1 still refuses in the core); the user is still told, before confirming, everything the old service's local status says has not been sent, and that anything not known is not known (MP3.1).

### T21.1 The confirmation

The `sync.serviceSwitch` confirmation is the system alert on every platform (on the Mac attached to the Settings window), as Delete Deck asks on the Mac (N20.7); one pattern on the Mac, iPad and iPhone. Changed 2026-11-22 (coordinator review): the touch confirmation dialog it first used showed as a narrow popover with no visible Cancel. It has a question as its title, one message, and two buttons: Cancel (the cancel role: Esc, and the tap outside on iPad) and the destructive button (the destructive role, never the default: Return confirms nothing). It opens only while the change can run; while it cannot, the reason stays under the Service control (T18.2's `sync.switch.busy`, T19.2). When the destructive button is chosen the confirmation closes; `sync.service.changing` and `sync.service.changeFailed` show under the Service control (T19.2).

`%@` is the old service's name: `sync.service.meguri` or `sync.service.iCloud.name` (T19.1: no Beta label inside a sentence).

| Key | English | Shown |
|---|---|---|
| `sync.switch.title.off` | Turn Off Sync? | title, new value Off (value changed: now a question) |
| `sync.switch.title.meguri` | Switch to Meguri Sync? | title, new value Meguri Sync (value changed) |
| `sync.switch.title.iCloud` | Switch to iCloud? | title, new value iCloud (value changed: T19.1's name) |
| `sync.switch.message.off.meguri` | This collection stays on this device and stops syncing. You’ll be signed out of Meguri Sync. Nothing on your server is deleted. | first paragraph, Meguri Sync to Off |
| `sync.switch.message.off.iCloud` | This collection stays on this device and stops syncing. Nothing in iCloud is deleted. | first paragraph, iCloud to Off |
| `sync.switch.message.meguri` | Next, sign in to your Meguri Sync server. Meguri asks before it replaces a collection with your cards. Nothing in iCloud is deleted, and your other devices keep using iCloud until you change them. | first paragraph, iCloud to Meguri Sync |
| `sync.switch.message.iCloud` | Next, Meguri sets up iCloud and asks before it replaces a collection with your cards. You’ll be signed out of Meguri Sync. Nothing on your server is deleted, and your other devices keep using Meguri Sync until you change them. | first paragraph, Meguri Sync to iCloud |
| `sync.switch.unsent.heading` | Not yet sent to %@: | first line of the list, while the list has a line |
| `sync.switch.unsent.item` | • %@ | each line of the list, around its text |
| `sync.switch.unsent.changes` | `%lld change` / `%lld changes` | the old service reports pending changes above 0 |
| `sync.switch.unsent.some` | Some changes | it reports pending changes without a count: Meguri Sync's round reports that it left work for a later round, and no count of this device's unsent rows (its review answers are among those changes) |
| `sync.switch.unsent.heldBack` | `%lld review answer` / `%lld review answers` | it reports held answers above 0 (iCloud: P14.4 `heldBack`, which pending changes do not count) |
| `sync.switch.unsent.media` | `%lld media file` / `%lld media files` | it reports media files on this device not yet sent above 0: iCloud's upload files; Meguri Sync's files waiting to upload or excluded from the transfer (T18.6). Files still to download from the service are not counted: they are not this device's to send |
| `sync.switch.unsent.kept` | These stay only on this device. | after the list, when every count is known |
| `sync.switch.unsent.none` | Everything on this device has been sent to %@. | in place of the list: every count is known and 0 |
| `sync.switch.unknown` | Meguri couldn’t check whether everything on this device has been sent to %@. Anything not sent stays only on this device. | in place of the list: no count is known above 0 and one or more is not known |
| `sync.switch.unknown.more` | Meguri couldn’t check whether anything else is waiting. Anything not sent stays only on this device. | after the list, when one or more count is not known |
| `sync.switch.acceptanceUnknown` (T18.2) | | its own paragraph last, under its T18.2 condition |
| `sync.switch.confirm.off` (T19.2) | Turn Off Sync | destructive button, new value Off |
| `sync.switch.confirm.meguri` (T18.2) | Switch to Meguri Sync | destructive button, new value Meguri Sync |
| `sync.switch.confirm.iCloud` | Switch to iCloud | destructive button, new value iCloud (value changed: T19.1's name) |
| `sync.service.cancel` (T18.2) | Cancel | cancel button |

- The counts are the old service's local status, as T18.2 reads them: for iCloud P14.4's `pending`, `heldBack` and the media files waiting to upload; for Meguri Sync the round's pending work (no count: "Some changes") and the T18.6 counts of this device's files not transferred. A count it does not report is not known, never 0. Meguri Sync sends review answers as changes, so what is known about its changes covers them; it reports no count of its unsent changes, so with Meguri Sync as the old service `sync.switch.unsent.none` never shows.
- The message is the first paragraph; then the heading and the list's lines, each on a line of its own, as one paragraph; then `sync.switch.unsent.kept` or `sync.switch.unknown.more` as the next paragraph; or, with no list, `sync.switch.unsent.none` or `sync.switch.unknown`. Then `sync.switch.acceptanceUnknown` when it applies. Each text stays its own catalog entry (T1's "followed by").
- Nothing in the confirmation names iCloud when the old and the new service are Meguri Sync and Off, and nothing names Meguri Sync when they are iCloud and Off, except `sync.switch.acceptanceUnknown`, which T18.2 shows whatever the old service because an upload of unknown acceptance holds every service.
- The confirmation closes, changing nothing, when the change stops being possible while it is open (a review or another change starts).
- Keys withdrawn (removed from the catalog when T21 ships, their names never reused): `sync.switch.pending`, `sync.switch.pending.some`, `sync.switch.heldBack`, `sync.switch.pending.none`, `sync.switch.pending.unknown`, `sync.switch.media`, `sync.switch.media.unknown`, `sync.switch.kept`, `sync.switch.otherDevices`, `sync.switch.signOut`, `sync.switch.next.localOnly`, `sync.switch.next.iCloud.setup`, `sync.switch.next.meguri.setup`.

### T21.2 Meguri Sync before the first sign-in

"Setup not finished": Meguri Sync is the selected service, the provider is neither blocked nor in handoff recovery, and its connection is unconfigured; or it is signed out, cancelled or signing in while this device has not been signed in to it; or it is blocked after a first Sign In from the fields failed (its error shows under the fields, T18.3), until a sign-in succeeds or the connection is removed; editing the fields does not end it. "Signed in to it" is kept in this device's settings: it becomes true when a sign-in succeeds or a round reports a credential failure, and false when the connection is removed or a service change deletes the sign-in, so a relaunch gives the same answer. It is not a problem: none of the three kinds (T19.1); it does not make sync wait on the person (the review's status button, N16.2, and study preparation treat it as no service).

| Key | English | Use |
|---|---|---|
| `sync.meguri.status.notSetUp.title` (T18.4) | Not Set Up | the main status's title (the status panel, the sync button's help tag) |
| `sync.meguri.status.notSetUp.detail` (T18.4) | Sign in to your Meguri Sync server to finish setting up. | its sentence (value changed: it said "Enter your server address and sign in to start.") |
| `sync.meguri.address.prompt` | https://sync.example.com | the Server Address field's placeholder |
| `sync.meguri.required.prompt` | Required | the Username and Password fields' placeholder |

- In T19.5's table, the row "connection unconfigured, signed out or cancelled" becomes two rows, in its place: "setup not finished" (above): kind none, the text `sync.meguri.status.notSetUp.detail`, the action `sync.meguri.signIn` (the fields); then "connection signed out or cancelled, otherwise": needs you, `sync.meguri.status.signedOut.detail`, `sync.meguri.signIn`, as before. "The server didn't accept your sign-in" (`sync.needsYou.meguri.signIn`) and "identity changed" keep their needs you rows.
- Its symbol is Sync Off's (the secondary `cloud`, `docs/design/decisions.md` 2026-11-22), never the needs-you symbol. The sync button's help tag and accessibility value are the title and the sentence (T2, T18.7). The status panel shows them with Sign In… (T20.1).
- Leaving Meguri Sync in this state asks nothing, as a choice made from Off does (T18.1): nothing has been sent or signed in to lose.
- The Sync section in this state (N15.7) has the Service control; the sign-in fields and Sign In with `sync.meguri.signIn.footer.setup` or `.setup.manual` (T19.3) as their footer, and any sign-in error under them; and Troubleshooting. It shows no status row, no Sync Automatically and no Sync Now (and so neither `sync.settings.automatic.footer` nor a Sync Now reason); they show once a sign-in succeeds. While the first sign-in runs, `sync.meguri.status.signingIn.title` shows under the fields as progress.

### T21.3 Tests

- `ServiceSwitchConfirmationTests` (`app/Packages/MeguriKit/Tests/AppModelTests/`): for each old service and new value, the title, message and button keys; with counts known above 0, 0, and not known, for each service, the list's lines, the closing sentence and the absence of the list; no text names the service that is neither the old nor the new one.
- `SyncKindsPresentationTests`: the two rows that replace "connection unconfigured, signed out or cancelled", their kinds, symbols and actions.
- `SyncStringCatalogTests`: a row for each key of T21 and a check that no source names a key T21.1 withdraws.
- The confirmation's seven examples (`settings/sync/window/switch-*`) and the page before the first sign-in (`settings/sync/window/meguri-never-signed-in`, `settings/sync/panel/meguri-never-signed-in`) rendered in the test VM, Mac, iPad and iPhone, light and dark.

## T22. The sync button syncs (2026-11-22)

Changed 2026-11-22. Antoine, on his folded iPhone Duo: a press of the button should start the sync at once, "This way nothing opens". Decided by Claude (coordinator) under Antoine's delegation, with his standing permission of 2026-11-20 to update specs (`docs/design/decisions.md`, row `sync.status` of 2026-11-22, "The sync button syncs"; `docs/design/native-interface-contract.md` N23.6). It changes what a tap on the toolbar's sync status button does and the button's accessibility text. No state, condition, availability or existing value changes: the status is still T19.4 and T19.5, Sync Now still follows T6, T18.7 and T19.7, and the panel is still T20. Where T2, T18.7 or T20.1 say the button opens the popover or panel, T22 wins.

### T22.1 The tap, the menu and the text

A tap follows the first row that applies.

| Status | A tap |
|---|---|
| needs you (T19.4, T19.5), or no service selected | opens the panel (T20), as before |
| a status with its own action that is not needs you (Cancel Setup while setup prepares, Try Again for an acceptance that is not known or a paused setup media step; N23.3 row 4) | opens the panel |
| the status moves (the panel's bar shows, N23.1) | nothing: no second sync starts and nothing opens |
| Sync Now can run (the panel's Sync Now would be enabled: T6's available for iCloud, T18.7's last row for Meguri Sync) | runs Sync Now, as File › Sync Now does (`sync/policy#P12.1`; MP5's normal round); nothing opens |
| any other status (Meguri Sync before its setup finishes or with round media blocked, an unavailable Sync Now) | opens the panel, which says why |

- While the sync it started runs, the button shows the moving symbol (N23.2). When it ends up to date, the check shows in the accent colour for about a second, then in the default colour. When it ends in any other state, the button shows that state's symbol (needs you in orange).
- VoiceOver announces the end of a sync started from the button (T1): `sync.result.succeeded` ("Finished Syncing") when it ends up to date, otherwise the new status title without its "…".
- A long press on touch, and a click and hold on the Mac, show a menu (a secondary click on a Mac toolbar is the system's own toolbar menu): its section header is the status title, then on a second line the panel's sentence (T20.1) when there is one; then `sync.now.command` (Sync Now) where the panel shows Sync Now (N23.3's Sync Now row), enabled exactly as the panel's (disabled while the status moves); then `sync.status.openSettings` (Sync Settings), which opens the Sync section as the panel's button does. The menu shows no reason line and no action of the status.
- Accessibility: the label is `sync.button.accessibilityLabel` for every service (in place of T20.1's `sync.button.label`, `sync.meguri.button.label` and `sync.button.label.off`, which the button no longer uses; the Sync section and the panel still name the service where they did); the value is the status title without its "…"; the hint is `sync.button.hint.syncNow` when a tap syncs, `sync.button.hint.status` when a tap opens the panel, and none while the status moves. On the Mac the help tag is still the title and the sentence (T2).
- T2's media indicator beside the button (`sync.media.filesLeft`) is withdrawn: the toolbar item is the symbol alone, so the system draws it in the group of its neighbours. iCloud's files left stay in the panel under its bar (T20.1); the key stays in the catalog.

### T22.2 Keys added

| Key | English | Use |
|---|---|---|
| `sync.button.accessibilityLabel` | Sync | the sync button's accessibility label, whatever the service |
| `sync.button.hint.syncNow` | Syncs now. | its accessibility hint while a tap syncs |
| `sync.button.hint.status` | Shows the sync status. | its accessibility hint while a tap opens the panel |

### T22.3 Tests

- `SyncStringCatalogTests`: a row for each key of T22.2.
- `SyncButtonTests` (`app/Packages/MeguriKit/Tests/AppModelTests/`), over `SyncButton` (AppModel): each row of T22.1's table gives its tap; the feedback gives `succeeded` only for a sync that was requested, moved and ended up to date, `ended` for one that ended in another state or was refused into needs you, and nothing for a sync it did not start or a request that never moved.
- `SyncButtonPlacementTests` (UI test, iPhone and the folded Duo): the button is in the same trailing group as search and Add, at the top and after scrolling to the bottom, on the deck list and a deck page, and no toolbar item is drawn empty.
- The button in each state captured in the test VM over a generated collection (`-MeguriSyncRender`), on the folded Duo, iPhone, iPad and Mac, light and dark.

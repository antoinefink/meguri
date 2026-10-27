# iCloud sync strings

Spec: sync/status-strings
Requirements: R89, R102, R103, R104, R105, R106, R107, R117, R122, R123, R124, R125, R127, R158, R166, R1548, R1551, R1555, R1558, R1559, R1560, R1561, R1562, R1583, R1592, R2046, R2048, R2049, R2050, R2054, R2055, R2056, R2058, R2059
Anki: none (Meguri design)
Scenarios: none (Meguri's Swift tests: T16)

The user-visible text of routine iCloud sync: the sync button, the status popover, the Sync Now command and its results, the attention items, the error classes, the recovery choices and their confirmations, Account & Sync's iCloud rows and the Mac quit sheet. U151's `SyncStatusModel` and U161 take every such string only from this file (KTD1); U166's connect entry and U153's damaged-collection remedy reuse the T12 and T13 keys for the same flows. It fixes the draft wording that `sync/policy#P24` left open: P12.1's explanation texts are replaced by T6 under the same keys.

The states, items, results and choices are those of `sync/policy` (P12, P14, P15) and `sync/cloudkit-recovery` (V9 to V21); this file adds no state and changes no condition. Manual Anki transfers (Settings Anki Transfers, `sync/anki-transfers`) are frozen and have their own text; none of it is here (R1560).

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
| `sync.connect.action` | Connect to iCloud… | T9.2; Account & Sync's enable control (T14); U166's Welcome choice |
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

## T17. Changes from the draft wording

- `sync/policy#P12.1`'s explanation column was draft text (P22, P24). Its keys stay; T6 replaces the English: "Sync is unavailable until the collection is open." becomes "Sync Now is available once the collection is ready."; the review text is truthful only while automatic sync runs, so a `.manual` key is added; "in Settings" becomes "in Account & Sync" for Meguri's own section; "Sign in to iCloud to sync." gets the system place and per-reason variants; "Choose whether this collection replaces the one in iCloud before syncing." becomes "Choose where to keep this collection before you sync." to match the decision sheet (T10); "Open iCloud status to continue." becomes "Open iCloud Sync to see what to do.", naming the button.
- `sync/cloudkit-recovery#V11` quotes meaning, not copy; T9, T10, T12 and T13 give the copy for its quoted texts, in title case where they are labels ("Use the iCloud Collection on This Device…", "Restore Previous iCloud Collection from %@…"). V15.3's quota reason and V15.2 step 5's conflict text are kept word for word, with "The" added to the latter.

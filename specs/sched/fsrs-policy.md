# FSRS-only policy and collection compatibility

Spec: sched/fsrs-policy
Requirements: R702, R703, R704, R705, R763, R764; AE29
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/compat/

Meguri schedules only with FSRS, on Anki's current scheduler, through the `fsrs` crate at 6.6.2 (`sched/fsrs-transitions#T1`). This file says what Meguri does with a collection that is not in that state: the global FSRS switch is off (R702, R703), a preset holds parameters Meguri cannot use (R704), or the collection is still on the previous scheduler (R705). It also lists the scheduling data that Meguri keeps but never uses or runs (R763, R764).

Changed 2026-11-19 (A1): FSRS is always on. The app turns it on by itself when a collection opens without it, with no sheet, no prompt and no option; a collection whose enable fails still opens, with study refused (A1.4). The lines marked "Changed 2026-11-19" point to A1; the core's status, mark and op (S1 to S3, E1 to E5) are otherwise as written.

Terms and readers come from other specs:

- home deck, home preset, built-in defaults, parameter list, decay and desired retention in force: `sched/presets` V1, V6 and V7;
- the bool reader of collection config keys: `sched/config-keys` G1 (only the JSON value `true`, stored as a BLOB, reads as on);
- a config write: `storage/config-table` C4 (compact JSON as a BLOB, `usn` -1, `mtime_secs` now, `col.mod` now in milliseconds; no write when the stored value is the same);
- card data: `model/card-data-json`;
- memory states from the review log: `sched/memory-state` (U46);
- scenario ops `enable_fsrs` and `enable_v3`: `conformance/fsrs-policy-ops`.

## S1. The policy status

`fsrsPolicyStatus()` reads the collection config key `fsrs` with the bool reader and the seen-on mark (S2), and returns one of three statuses:

| Status | When | What the app shows (U51) |
|---|---|---|
| `ok` | `fsrs` reads on | nothing |
| `needsEnable` | `fsrs` reads off and the mark is not set | the first-time sheet (R702); the plan writes this status `needsEnable{firstTime}` |
| `needsReEnable` | `fsrs` reads off and the mark is set | the sheet that says FSRS was turned on again (R703) |

- `fsrs` reads off when it is missing, `false`, or any other value: `1`, `"true"`, `null`, text that is not JSON, a TEXT storage class. Anki reads it the same way (oracle: a stored `1` or `"true"` leaves FSRS off in Anki).
- Reading the status writes nothing, at open or at any other time.
- The status does not look at presets. A collection with FSRS on and an unsupported preset (U1, U2) is `ok`.
- A new collection holds no `fsrs` key, as in Anki (U28), so its status is `needsEnable`.
- `needsEnable`'s `firstTime` is true: this device has no seen-on mark. A repeat enable is `needsReEnable`; card history and a different device's state do not decide the wording.
- Changed 2026-11-19 (A1.2): the app shows nothing for any status. The third column of the table is withdrawn: `needsEnable` and `needsReEnable` both lead to the same silent `enableFsrs`, and the two statuses differ only in the diagnostic record (A1.5).

## S2. The seen-on mark (R703)

The mark records that this device has seen FSRS on in this collection, so that Meguri can tell "turned off again by another client" from "never on".

- The engine sets it whenever it finds `fsrs` on: at every open, after every sync's commit, and when `enableFsrs` commits (E1). A sync's commit is a CloudKit apply (a `CloudIncoming` op, `sync/cloudkit-records#K13.2`), a collection replacement (Replace local collection from iCloud, `sync/cloudkit-recovery#V14`; a manual Anki one-way import), or a manual Anki normal sync's merge (U107, frozen). The engine reads `fsrs` in the transaction of every committed op whose source is `SyncIncoming` or `CloudIncoming`, and after every replacement through `engine/collection-replace` (`CloudAdopt` included), and sets the mark when it reads on. Setting it is not a collection write.
- It is device-only state (KTD12, `storage/config-table` C8): never in the collection file, never synced, exported or backed up with it, and never in the CloudKit tracking store. It belongs to the collection's place on the device, so it survives a replacement of the collection file (Replace local collection from iCloud, one-way download, restore, `.colpkg` import). It is removed only with the collection.
- In Meguri's own collection, `fsrs` changes only through `enableFsrs`, syncs and replacements. So `needsReEnable` means that another client turned FSRS off after this device saw it on.
- A lost mark only changes the wording: the next status is `needsEnable` instead of `needsReEnable`.
- Changed 2026-11-19 (A1.5): no wording depends on the mark any more. It stays, with every rule of this section, so that the diagnostic record can tell "turned off by another client" from "never on".
- The mark is durable device state, loaded on open and retained across process restarts. A read failure is treated as a lost mark and reported diagnostically; it does not refuse the collection. A write failure keeps the in-memory seen-on state for this host, reports a diagnostic and can be retried at the next S2 lifecycle check. It must not fail or roll back an already committed collection operation, or report that `enableFsrs` left FSRS off. Loss of the mark after restart still has only the wording consequence above.

## S3. Study while the status is not `ok`

- `startSession` refuses with `FsrsPolicyPending` while the status is not `ok`, and writes nothing (U43's C5 result). This check comes before the preset check of U4.
- A review session that is already open goes on when a sync (a `CloudIncoming` op included) turns `fsrs` off: answering, undo and the session's queue never look at the status, and the newest answer stays undoable.
- The app runs `enableFsrs` after `endSession`. When the app ends first, the next open finds `needsReEnable` (S2) and the app runs it then. Changed 2026-11-19 (A1.2, A1.3): it runs with no sheet, in the background.
- `enableFsrs` refuses with `SessionActive` while a review session is open (E4), so nothing clears the session's undo history.
- The engine has no timer for the policy and publishes no event for it. The app reads the status after every open and after every sync, including a `CloudIncoming` change set that changes `fsrs` (U51, `sched/app-coordination#F1`).
- Review-session admission and `enableFsrs` use the same collection-owned admission state, serialized on the writer. `enableFsrs` checks it when its write begins, not just before submission. U43 must atomically check the current policy and reserve that state before exposing a session, then release it only after `endSession` drains the session's writes. An earlier admission makes enable refuse; an earlier enable completes before admission checks the resulting policy. A caller-supplied boolean is not evidence that no session exists. Until U43 lands, the guard admits no review session; U43 owns the real acquire/release path and its race tests.

## E1. `enableFsrs(cancel, progress)` (R702)

One op that turns FSRS on:

1. When `fsrs` already reads on, it computes nothing and writes nothing (`col.mod` stays). Anki's switch writes nothing in this case too (scenario `sched/compat/re-enable`, last steps).
2. It writes `fsrs` = `true` (config write). A stored value that is not a boolean is replaced.
3. It computes FSRS values for the cards of E2 and writes them as E3 says.
4. After the commit it sets the seen-on mark (S2).

It never runs by itself: the app runs it after its sheet (U51). Opening a collection never runs it.

Changed 2026-11-19 (A1.1): "after its sheet" is withdrawn; there is no sheet. The engine still never runs the op by itself, and an engine open still never runs it: the app runs it, without asking, for an open collection whose status is not `ok` (A1.2).

## A1. FSRS always on, with no prompt (2026-11-19)

Decided by Antoine on 2026-11-19 (`docs/design/decisions.md`, `home.fsrsSheet`; `docs/design/native-interface-contract.md` N18.1): FSRS is always enabled. Meguri turns it on by itself when a collection opens without it, with no prompt and no option anywhere. Where S1, S2, S3, E1 and `sched/app-coordination` F1 to F4 differ, this section wins. It replaces the sheet of R702 and R703 and AE29's "after the one-time sheet"; what `enableFsrs` writes (E1 to E4, AE29's result) is unchanged. The app side is `sched/app-coordination#F5`.

### A1.1 What stays in the core

- `fsrsPolicyStatus()` (S1), the seen-on mark (S2), the admission rules (S3), `enableFsrs` (E1 to E4) and the scheduler switch at open (V2) are unchanged. No new op, config key, event or open step is added.
- The engine's open still writes nothing for the policy (S1, E1). The conformance sessions of V3 and `conformance/fsrs-policy-ops` are therefore unchanged: a scenario's collection gets FSRS on only through its `enable_fsrs` op, and `sched/compat/re-enable`'s "the open with FSRS off writes nothing" still holds for the engine.
- `startSession` still refuses with `FsrsPolicyPending` while the status is not `ok` (S3). It is what keeps Meguri from scheduling a card in a collection whose switch is off: while the enable waits, runs or has failed, study is refused and everything else works.

### A1.2 At open

- A collection whose status is `needsEnable` or `needsReEnable` opens as any other, and the app then runs `enableFsrs` by itself, with no question (`sched/app-coordination#F5.1`). Opening never waits for the op, and the op never keeps the collection from opening.
- **Sync first when it can** (orchestrator decision, 2026-11-19). When a sync service is connected and would sync by itself at this open, the op starts only after that open-time sync's collection exchange has ended, whatever its result, or after the service's own rules have given it up: Sync Automatically off or the update hold, offline, an account or sign-in that needs the user, a setup or decision that is pending, a failure that waits for a retry. With no service, or one that does not sync at open, the op starts at once. The app adds no network wait, timer or prompt of its own: it acts on what the service already reports. A study entry point used while the op still waits for that sync starts it at once, so study never waits on the network.
- Residual risk: a review made on another device and synced only after the op can have its card state overwritten for that card; the review-log row survives, and the memory state is computed again from the log.
- This holds for every way a collection comes to be open on a device: a launch, a new empty collection (which holds no `fsrs` key, S1), an imported or restored collection, and a collection that a sync service's setup or replacement put on the device (`CollectionReplaced` is treated as an open).
- While the op waits or runs, the collection is open and usable: browsing, adding and editing notes, import, export, sync and settings work. Only study waits: an entry point shows the normal loading state of a starting session and goes on when the op commits (`sched/app-coordination#F4`). Nothing names FSRS, memory states or a count of cards, and nothing is asked.
- No cancel is offered. When the app quits or is killed while the op runs, the transaction rolls back (E4): `fsrs` is still off, no card changed, and the next open starts again.
- With the status `ok` the open reads the status and writes nothing.

### A1.3 When another client turns FSRS off

- Another client can turn `fsrs` off, and a sync brings that to this device (S2). Meguri turns it on again, each time, with no message: in the background as soon as no review session is open, or at the next open by A1.2 (`sched/app-coordination#F5.2`). The decision stands however often it happens; A1.5's record is the only trace.
- An open review session goes on as S3 says; the op runs after `endSession`.
- Between the sync's commit and the op's commit, `startSession` refuses with `FsrsPolicyPending` (S3); study entry points wait for the op and then go on.

### A1.4 When the op fails

- One rule for a failure at open and a failure in the background (orchestrator decision, 2026-11-19): **the collection stays open.** Study is refused (S3): each study entry point shows one plain sentence with Try Again, which runs the op again (`sched/app-coordination#F5.3`). Browsing, adding notes, import, export, sync and settings stay available, so the user can repair the cause in Meguri or export the collection.
- Nothing else is shown: no alert at open, no banner. The next open tries again by A1.2.
- A failed op wrote nothing (E4). Meguri never schedules a card while `fsrs` reads off, and never offers a way to study with it off.
- Known repeating cause: a preset whose stored ignore-date text `sched/fsrs-items#I1.3` refuses fails the whole op (`sched/memory-state#M4.3`, unchanged). Unsupported presets do not fail it: their cards are skipped (E2, U4).

### A1.5 Sync, and what is recorded

- The op's writes are ordinary local changes: the `fsrs` config row (`usn` -1) and each changed card (`mod` now, `usn` -1; E3). They sync like any other change, by whichever service is connected, at its next sync; they force no full sync and start no sync by themselves. Other clients then have FSRS on (E5's last item).
- The app writes one entry to its diagnostic log for each automatic enable: the status that caused it (`needsEnable`, or `needsReEnable` when another client had turned FSRS off after this device saw it on), the number of cards computed and the result. A repeated re-enable of the same collection is recorded each time; it is never limited, delayed or shown. Copy Debug Info includes the entries.
- The op clears the undo history (E4), also the steps the user made between the open and the op.

### A1.6 For the owner

- **No consent before the first write.** Opening a collection that has FSRS off changes it soon after: the switch, and the memory state of every reviewed card. Due dates and intervals do not change (E4). After the next sync every other client of that collection has FSRS on too.
- **Unsynced work elsewhere**: A1.2's residual risk. Sync-first removes it for what the service holds at open; it stays for a device that syncs later, for an open while offline, and when study is started before the open-time sync ends.
- **A client that keeps turning FSRS off** makes Meguri rewrite the reviewed cards and sync them again each time (A1.3). Only the log shows it.
- **A collection whose op always fails** opens, but cannot be studied in Meguri until its cause is repaired (A1.4).
- **Undo history is cleared without notice** when the op runs after the user has already made changes (A1.5).
- **Spec-writer choices**: the app, not the engine's open, runs the op (A1.1), so that conformance sessions and raw opens stay as the oracle's; the open-time sync is waited for only through the service's own reports, and a study entry point ends the wait (A1.2); no FSRS wording in the loading state or the error.

## E2. The cards it computes

The computation is U46's bulk recompute (`sched/memory-state#M4`, caller `#M7.4`); this unit chooses the card set and the source (E4). The set is the cards whose home deck (`sched/presets#V1`: `odid` when it is not 0, else `did`) exists and is a normal deck. Of the set, M4.1 processes ("computes") a card when all of these hold:

- it is not new (`type` is not 0); suspended and buried cards are computed;
- it has at least one review-log row, of any type;
- its home preset is not unsupported (U1, U2; M4.3).

Every other card keeps its bytes: new cards, cards with no review-log row, cards of unsupported presets, and cards whose home deck is missing or is itself a filtered deck (they are not in the set).

The card's home preset gives the inputs:

| Home preset | Inputs |
|---|---|
| exists and is supported | its parameter list and decay (`sched/presets#V6`), the desired retention in force (`sched/presets#V7`: the home deck's own value, else the preset's), its historical retention and its ignore date |
| missing (the deck's preset id names no preset) | the built-in defaults (`sched/presets#V1`): empty parameter list (decay 0.154), preset desired retention 0.9, historical retention 0.9, no ignore date. The desired retention in force is still `sched/presets#V7`'s: the home deck's own value when it has one, else 0.9 (2026-11-17: an earlier text could be read as always 0.9; implemented code must be checked: U50's `enableFsrs` inputs for a deck with a missing preset) |
| unsupported (U1, U2) | none: the card is skipped and keeps its bytes |

## E3. What it writes on a computed card

`sched/memory-state#M4.1` and `#M4.2` are the authority; in short:

- U46 computes the card's memory state from its review log with these inputs (its memory item, `sched/fsrs-items`). It gives a memory state (stability, difficulty) or none (for example when the only rows are Set Due Date rows). The card's own `factor` and `ivl` are not used when there is no item.
- The plan's SM-2 fallback here is the incomplete-history start of `sched/fsrs-items` I5, including historical retention. It is not the single-card M2 fallback from the card's own interval and ease; a no-history card remains untouched and a processed card without an item has no `s` or `d`.
- The card data then holds: `s` and `d` from the memory state, or neither when there is none; `dr` = the desired retention in force; `decay` = the decay of the parameter list. `pos`, `lrt`, `cd` and unknown keys stay. The text is written per `model/card-data-json` C4.
- A card whose `s`, `d`, `dr` and `decay` compare equal to its stored ones, as M4.2 compares them, keeps its row: `data`, `mod` and `usn` stay (R687). A changed card gets its data rebuilt, `mod` = now (seconds) and the local `usn` (-1).
- Nothing else on the card changes: `type`, `queue`, `due`, `ivl`, `factor`, `reps`, `lapses`, `left`, `odue`, `odid`, `did`, `flags`, `ord`, `nid`.

Examples from `sched/compat/enable-fsrs` (FSRS turned on at 2030-03-01 10:01 UTC, day 59; the golden is the authority):

| Card | Home preset | History | Data after |
|---|---|---|---|
| alpha | Default (empty list) | learning Again, Good, Good; reviews Good, Good, Hard, Good | `{"s":29.659,"d":7.562,"dr":0.9,"decay":0.154}` |
| bravo | Default | two reviews, no learning start | `{"s":37.5,"d":6.617,"dr":0.9,"decay":0.154}` |
| delta | Default | one learning answer (Good), ten minutes ago | `{"s":2.3065,"d":2.118,"dr":0.9,"decay":0.154}` |
| echo | Default | new card | `{}`, row unchanged |
| golf | Default | review card, no review-log row | `{}`, row unchanged |
| hotel | Default | one Set Due Date row only | `{"dr":0.9,"decay":0.154}` |
| india | Default | as bravo, with custom data | `{"s":37.5,"d":6.617,"dr":0.9,"decay":0.154,"cd":"{\"v\":1}"}` |
| juliet | 21 values ending in 0.3, retention 0.85 | as alpha | `{"s":29.5456,"d":7.562,"dr":0.85,"decay":0.3}` |
| lima | 19 values | learning, three reviews | `{"s":38.213,"d":5.245,"dr":0.9,"decay":0.5}` |
| mike | Default, deck's own retention 80 % | learning, two reviews | `{"s":78.8749,"d":2.097,"dr":0.8,"decay":0.154}` |

## E4. Limits of `enableFsrs`

- It never changes a due date, an interval, a queue or a card type (R702), and never adds, changes or removes a review-log row: there is no rescheduling and no Rescheduled entry.
- It never writes a deck, a preset, a note, a note type, a tag, a grave or any config key other than `fsrs`.
- It is one op and one transaction, with source `UserNonUndoable`: it commits whole or not at all, and it clears the undo history (U25).
- Its distinct `enable_fsrs` catalog entry (`engine/op-model` O9) permits a long, cancellable write. The flag write and U46 card writes share that transaction; it does not invoke a separate committed recompute or U49's deck-options save.
- Cancel: the token is checked between batches. A cancelled op rolls back: `fsrs` is still off, no card changed, and the status is what it was.
- Progress: selected cards visited out of the number E2 selects; visits include cards skipped without a write, so a completed traversal reaches its total.
- While a review session is open it refuses with `SessionActive` and writes nothing (S3).

## E5. Differences from Anki's switch

In Anki the user turns FSRS on in the deck options screen and saves. The oracle's `enable_fsrs` is that save, made so that it does nothing else (`conformance/fsrs-policy-ops` F2). That save does more than `enableFsrs`, and Meguri leaves these out:

- Anki writes the screen's four global options every time (the custom scheduling script, "New cards ignore review limit", "Limits start from top" and the health check), adding the keys that are missing. Meguri writes only `fsrs` (R763).
- Anki saves the presets the screen sends: the screen fills an empty FSRS-6 list from the older lists when it loads, and the save clears the older lists of a preset sent with an empty FSRS-6 list. The save also replaces a sent preset's ignore date text that sorts after today's date as a string with today's date. For a text that `sched/fsrs-items#I1.3` refuses (such as `garbage`), Anki then computes with today's date where Meguri, which keeps the stored text, fails the whole switch (`sched/memory-state#M4.3`). For a valid future date (such as `2099-01-01`), Anki computes with today's date as the cutoff and Meguri with the stored date (added 2026-11-17). Meguri writes no preset.
- Anki gives a deck whose preset is missing the preset of the deck the screen was opened for, and computes its cards with that preset. Meguri leaves the deck as it is and computes with the built-in defaults (E2).
- Anki fails the whole save when the parameter list of a preset it sends or computes with cannot be built. Meguri skips those presets (U4).
- Anki writes every computed card, changed or not. Meguri writes only changed cards (R687, `sched/memory-state#M4.2`). No golden of this unit has a computed card whose values stay the same, so it needs no deviation entry; a scenario that has one joins the deviations `memory-recompute-unchanged-mod` and `memory-recompute-unchanged-usn`.
- Anki lets the user keep FSRS off. Meguri turns it on, and that change syncs to the other clients (R702). Changed 2026-11-19 (A1): with no sheet and no question.

## U1. Presets Meguri cannot schedule (R704)

A preset is unsupported when U45's check refuses it: U45 returns `UnsupportedParams` for its cards (`sched/fsrs-transitions#T13`). That is the one check U45, U46, U47 and this unit share; this unit never decides with a rule of its own, and T13 is the authority where this section and T13 differ. The check refuses a preset for either of two reasons:

- its parameter list (`sched/presets#V6`: the first non-empty one of `fsrs_params_6`, `fsrs_params_5`, `fsrs_params_4`) is one that the crate's model constructor at 6.6.2 refuses (`sched/fsrs-transitions#T1`): a length other than 0, 17, 19 or 21, or a value that is not finite once the crate has filled a short list to 21 values;
- it holds an FSRS list later than `fsrs_params_6` (U2).

The list is used as stored: Meguri never fills, clips or converts it to decide. Out-of-range but finite values (a negative value, 100.0) are accepted by the constructor.

| List used | Supported |
|---|---|
| empty | yes |
| 21 finite values; 19 finite values | yes |
| 34 values (a later FSRS version's count) | no |
| 20 values | no |
| 21 values, one of them NaN | no |
| 19 values, one of them +∞ | no |
| 17 values with the 6th value -0.5 (filling takes the logarithm of 3 × -0.5 + 1) | no |
| `fsrs_params_6` with 34 values and a valid 19-value `fsrs_params_5` | no: the first non-empty list counts, Meguri never falls back to an older one |

## U2. Presets with a later FSRS list (R704)

A later Anki release may store a list after `fsrs_params_6` in either of two places, and both make the preset unsupported (`sched/fsrs-transitions#T13` item 3):

- **The deck config blob.** An unknown field (`model/protobuf-records#P4`) with number 7 or 8, the numbers Anki 26.09.3 reserves for later parameter lists (`model/protobuf-records#P6.6`), unless it is a length-delimited field of length 0. Meguri keeps such a field as read (`sched/presets#V8`), so it survives in the blob.
- **`other`.** The preset's `other` bytes (`model/protobuf-records#P7`: the schema-11 members beyond the known keys, `model/schema11-json#S3`) hold a JSON object with a top-level member whose name is `fsrsParams` followed by a decimal number of at least 7 (`fsrsParams7`, `fsrsParams8`, ...) and whose value is a JSON array with at least one element. A later Anki that sends its list through sync's schema-11 JSON lands there: Anki 26.09.3 and Meguri keep unknown members in `other` (`model/schema11-json#S10`).

T13 defines the exact detection boundaries, including repeated unknown fields, ASCII key matching, leading zeros, unbounded decimal suffixes and malformed `other`. U50 consumes that shared result; it never reparses opaque preset bytes in the scheduler or defines a more permissive fallback.

Scheduling that preset with its FSRS-6 list would use parameters the user's newer client no longer uses.

| Preset holds (next to a valid `fsrs_params_6`) | Unsupported |
|---|---|
| unknown field 7: a packed float list `[4.0]` | yes |
| unknown field 8 as a varint | yes |
| unknown field 7 as a length-delimited field of length 0 | no |
| unknown field 39 (reserved by Anki, not for parameter lists) or any other unknown number | no |
| `other` = `{"fsrsParams7":[0.2,1.3]}` | yes |
| `other` = `{"x":1,"fsrsParams12":[1]}` | yes |
| `other` = `{"fsrsParams7":[]}` | no |
| `other` = `{"fsrsParams7":"0.2,1.3"}` | no |
| `other` = `{"fsrsparams7":[1]}` or `{"fsrsParams6":[1]}` | no |
| `other` empty, not valid JSON, or JSON that is not an object | no |

Anki 26.09.3 reads neither place: it ignores such a field or member and schedules with `fsrs_params_6`. The oracle at the pin cannot make such a preset schedule differently, so these rows are Meguri-only tests (T2), and the scenario `sched/compat/unsupported-preset-kept` shows only that Anki keeps an `other` list and its preset's bytes.

## U3. What is not detected

- An unknown field with a number other than 7 or 8, and an `other` member with any other name, may be any later option: it does not make a preset unsupported.
- Out-of-range bounded options (`sched/presets#V3`) do not make a preset unsupported; reads replace them.
- A missing preset is not unsupported: its cards use the built-in defaults (`sched/presets#V1`).

## U4. What an unsupported preset does (R704)

- Its decks still count (U41) and browse (U84). Searches, statistics and Card Info read its cards' stored values.
- `startSession` on a deck whose tree holds a card with an unsupported home preset refuses with `PresetUnsupported{presetIds}`: the ids of the unsupported home presets it found, ascending. The `FsrsPolicyPending` check (S3) comes first. Nothing is written (U43's C5 result).
- Grade Now on a selection that holds such a card refuses with `PresetUnsupported` and writes nothing (U48). Answering such a card on any path fails with U45's `UnsupportedParams` and writes nothing.
- Nothing automatic writes the preset or its cards: `enableFsrs` (E2), U46's bulk recomputes and U47's optimizer (`sched/optimizer#O3`) skip it. The preset row keeps its bytes, `mtime_secs` and `usn` through open, saves of other presets and sync (`sched/presets#V8`); its cards keep their rows. Only an explicit edit of that preset writes it; U49 decides whether its editor offers one.
- The study message ("This preset needs a newer Meguri") is U51's.
- A preset can become unsupported while a review session is open, when a sync brings a 34-value list or a later list. U1 and U2 are the test behind U43's `PresetUnsupported` session event; when the session emits it and what it does next are U43's (`sched/review-session`).

Anki 26.09.3 with such presets: it counts their decks, and it fails with an error when it computes answer states for a card of a preset whose list the crate refuses (its study screen cannot show the card). It ignores a later list (U2) and schedules with `fsrs_params_6`; Meguri refuses instead (R704).

## V1. The scheduler version (R705)

- The storage open gate refuses the v1 scheduler (`storage/open-and-connections#C2.2`, `V1Scheduler`, deviation `storage-v1-scheduler-refused`), so every collection the engine opens has `schedVer` 2.
- With `schedVer` 2, the config key `sched2021` read with the bool reader says which scheduler the collection is on: on means the current scheduler; off (missing, `false`, `1`, `"true"`, `null`, unreadable) means the previous one, v2. Anki 26.09.3 reads it the same way, and its desktop client does not study a v2 collection until the user accepts its scheduler update.

## V2. The switch at open

- An open step of every engine open (`engine/lifecycle` L2 step 3), after the gate and its upgrade, before the collection is `Ready`: when `sched2021` reads off, write `sched2021` = `true` (config write: BLOB `true`, `usn` -1, `mtime_secs` now, `col.mod` now in milliseconds), as one op without an undo entry. When it reads on, nothing.
- Nothing else: no other config key, no card, no deck. In particular no `creationOffset`: Anki's desktop adds that key when the current scheduler loads without it, Meguri never does (`sched/day-clock#D12.3`).
- Silent: no event, no message (R705).
- Anki writes exactly this when the user accepts its scheduler update: `sched2021` = `true`, a local change, `col.mod` moved, nothing else when `creationOffset` is present (scenarios `sched/compat/v3-switch/*`). Meguri writes it at engine open instead of waiting for the user. This R705 timing difference is proved by the direct `v3_switch_at_open` engine test; it requires no transform of a raw-storage observation (V3).
- Only at the engine's open. A sync that brings `sched2021` off, or a replaced collection that has it off, keeps it so until the next open. Meguri's scheduler never reads the key, so nothing depends on it in between.

## V3. Where the open steps run in scenarios

The switch (V2) and the seen-on mark (S2) run at every real engine open through `CoreHost`. Scenario sessions follow `engine/host` H4: storage `open`, raw setup and protocol `reopen` run the storage gate alone and run neither policy step. Reads do not promote a Raw session. The first engine operation promotes it and runs the complete hosted open; an engine-owned setup operation can do so earlier. A handoff closes the backend and the next segment follows the same boundary.

In `sched/compat/v3-switch/*`, the observation immediately after raw `reopen` therefore retains the old `sched2021` row on both sides. `enable_v3` then promotes Meguri through its real engine open, and the oracle accepts the scheduler update. The subsequent rows agree without a deviation transform. Raw storage-upgrade and preservation goldens retain their existing contract; separate engine tests prove V2 at production open.

## K1. Data Meguri keeps but never uses (R763)

These keep their bytes through every Meguri operation (answers, day rollovers, `enableFsrs`, recomputes, the optimizer, saves of presets and decks, sync) and keep syncing as they are:

| Where | What | Notes |
|---|---|---|
| config | `cardStateCustomizer` | K2 |
| config | `fsrsLegacyEvaluate` | another client's evaluation option |
| config | `fsrsHealthCheck` | read by U47 (`sched/optimizer#O6`), never written |
| config | `fsrsShortTermWithStepsEnabled` | read by U45, never written |
| config | `newCardsIgnoreReviewLimit`, `applyAllParentLimits` | written only by U49's save, and only when the user changes them (`sched/config-keys` G4) |
| config | every key that `sched/config-keys` G2 does not list as written by Meguri, unknown keys included | `storage/config-table#C7` |
| preset | `easy_days_percentages` (Easy Days), the SM-2 options, `fsrs_params_4`, `fsrs_params_5`, `other`, unknown fields | `sched/presets#V9`, `model/protobuf-records#P8` |
| deck | `extend_new`, `extend_review`, `learning_studied` | `model/protobuf-records#P8` |
| filtered deck | its definition (searches, limits, orders, reschedule and preview settings) | the decks area (U28) |

`learning_studied` is the stored scalar. In legacy `lrnToday`, the first element comes from the shared `last_day_studied` (`model/schema11-json#S7.1`); that day may advance while the preserved count stays. The answer rule is `sched/answering#A6`; R322/P8 preservation takes precedence over the reference counter reset.

## K2. The custom scheduling script (R764)

- `cardStateCustomizer` holds a JSON string of JavaScript that Anki's reviewers run on the four answer states before an answer is saved. Meguri never runs it, never reads it for scheduling and never writes it. A collection with a script schedules in Meguri exactly as without one (deviation from Anki's reviewers: custom scheduling is out of scope).
- Anki's scheduler core does not run the script either; only its reviewer screens do. So the oracle's answers, and Meguri's, are the scheduler's own states (scenario `sched/compat/custom-scheduling-ignored`: a script that would set every Good review to 99 days leaves the intervals at the scheduler's 8 and 17 days).
- A script may store custom data on a card (`cd` in the card data). Meguri never sets `cd` and keeps an existing one (`model/card-data-json` C2, C4).

## K3. The preservation test

`core/crates/meguri-engine/tests/fsrs_policy.rs` holds the cross-cutting preservation test (R763, T0). It builds a collection in which every K1 value is set to a value other than its default, plus an unknown config key, an unknown field on a normal preset (a number other than 7 and 8), a 34-value preset (U1), a preset with a later list in `other` and one with an unknown field 7 (U2). After each of these steps it compares a digest with the digest taken before the step:

1. a day of answers on a normal preset, across a day rollover;
2. `enableFsrs` from FSRS off;
3. a bulk recompute over all cards (U46);
4. an optimizer commit for a normal preset (U47, `sched/optimizer#O9`);
5. a save of another preset;
6. a `SyncIncoming` op that changes other keys and cards (`engine/op-model`; it needs no real sync);
7. close and open.

The digest covers: the K1 config rows (`KEY`, `usn`, `mtime_secs`, `val` bytes), the unknown config key's row, the whole rows of the three unsupported presets and of their cards, the Easy Days field bytes and unknown fields of the normal presets, and the `extend_new`, `extend_review` and `learning_studied` values of the decks. Equal digests mean the test passes.

A step whose op has not landed when this unit does is added by the unit that lands it (T3): U47 (the optimizer) is in milestone M1, after this unit (M1R in the reviews-first plan of 2026-10-17), and U44 (answers) is not among this unit's dependencies.

## T1. Scenarios

| Scenario | Covers |
|---|---|
| `sched/compat/enable-fsrs` | AE29, E1 to E4: crafted SM-2 history; memory states, `dr` and `decay` per card; new cards and cards without history unchanged; due, interval, queue and type unchanged; only `fsrs` among other rows changes |
| `sched/compat/enable-fsrs-live` | AE29 (its sheet withdrawn, A1) as a live chain: Anki studies a week with SM-2, Meguri turns FSRS on, the golden is Anki turning it on |
| `sched/compat/re-enable` | R703, E1, E3: FSRS turned off by another client and on again; the open with FSRS off writes nothing; enabling while on writes nothing |
| `sched/compat/v3-switch/missing`, `false`, `number`, `string` | V1, V2 with deviation `sched-v3-switch-at-open` |
| `sched/compat/v3-switch/already-on` | V2: nothing written |
| `sched/compat/custom-scheduling-ignored` | K1, K2 through two days of answers |
| `sched/compat/keys-survive-sync` | K1 through Meguri's study and normal sync (actor scenario) |
| `sched/compat/unsupported-preset-kept` | U1, U2, U4: both presets and their cards keep their bytes while another preset is answered and saved |

Goldens are recorded by the oracle maintainer with the driver ops of `conformance/fsrs-policy-ops`.

## T2. Meguri-only tests

`core/crates/meguri-engine/tests/fsrs_policy.rs` asserts what no oracle can show:

- S1, S2: the status for each `fsrs` value of S1; `needsReEnable` after a `SyncIncoming` op (`engine/op-model`) changes `fsrs` from on to off, also after close and open; `needsEnable` with no mark; the mark kept across a collection replacement.
- E1 to E4: a collection with a 34-value preset and a preset of 17 finite values whose 6th value is -0.5 (U1): `enableFsrs` skips the cards of both (bytes unchanged) and computes the others, and `startSession` on the second preset's deck refuses with `PresetUnsupported` (U4); a deck whose preset is missing is computed with the built-in defaults and keeps its preset id; a deck with `config_id` 999 and its own desired retention 80 holding a review card with history gets `dr` 0.8 and `decay` 0.154, with `s` and `d` from the built-in defaults' parameters and historical retention (E2; the desired retention plays no part in the memory state); a cancelled `enableFsrs` writes nothing and leaves `fsrs` off; undo history cleared after it.
- U1 to U4: a preset of each row of the U1 and U2 tables is reported unsupported or not, through U45's check; the deck-tree query that U43's `startSession` uses returns the unsupported home presets' ids, ascending, and none for a tree without such a card; the preset row and its cards keep their bytes through open, a save of another preset and a `SyncIncoming` op.
- V2: a `schedVer` 2 collection without `sched2021` opens with it on, `creationOffset` untouched (also when missing); direct test `v3_switch_at_open`. Raw reopen remains unchanged until an engine operation promotes it (V3).
- K3: the preservation test.
- A1 adds no core test: the op and the status are unchanged. Its tests are the app's (`sched/app-coordination#T1`).

## T3. Tests that land with later units

U43 and U48 depend on this unit, and U41, U44, U47 and U107 are not among its dependencies, so some of the plan's U50 scenarios can only run once those units land. This unit provides what they call: `fsrsPolicyStatus()`, the deck-tree query of unsupported home presets, and `enableFsrs`'s `SessionActive` refusal, which reads U43's open-session state (before U43 no session can be open, so the refusal is never taken). The later unit's implementer adds the test to this file when it lands:

| Unit | Test |
|---|---|
| U43 | `startSession` refuses with `FsrsPolicyPending` before `PresetUnsupported{presetIds}`, writing nothing (S3, U4). A `SyncIncoming` op during an open session turns `fsrs` off: answers go on, `enableFsrs` refuses with `SessionActive`, the newest answer stays undoable; after `endSession`, `enableFsrs` succeeds and due dates are unchanged (S3). Session events: the admission that `enableFsrs` reads and the `SessionStarted` and `SessionEnded` events that the CloudKit policy consumes (`sched/review-session#C2`, C16, C17; `sync/policy#P3`, P9) come from the same session registry, so `SessionStarted` is emitted only after admission succeeds, and `SessionEnded` only after admission is released; between the two, `enableFsrs` refuses with `SessionActive`, and once `SessionEnded` is emitted it succeeds. A `CloudIncoming` op that turns `fsrs` off during the session gives the same results, except that the newest answer stays undoable only when the op touches no row of it (`engine/undo#M9.8`). |
| U48 | Grade Now on a selection holding a card of a 34-value preset refuses with `PresetUnsupported` and writes nothing (U4). |
| U41 | the deck of a 34-value preset reports its counts (U4), when U41 lands after this unit; otherwise this unit's implementer writes it. |
| U44 | K3 step 1, when U44 lands after this unit; otherwise this unit's implementer writes it. |
| U47 | K3 step 4: an optimizer run that adopts parameters for a normal preset leaves the unsupported presets and their cards' `data` byte-identical (U4). |
| U107 | the 34-value preset's row and its cards unchanged through a real normal sync (U4). Frozen (KTD26). |
| U114 | a `CloudIncoming` op that changes `fsrs` from on to off gives `needsReEnable`, also after close and open (S1, S2); one that turns it on sets the mark; during an open session the S3 rules of the U43 row hold; the 34-value preset's row and its cards keep their bytes through a CloudKit round trip between two devices (U4, K1). U50 needs a small code change when the `CloudIncoming` source exists: the writer's post-commit `fsrs` observation (`executor/writer.rs`, today only for `SyncIncoming`) extends to `CloudIncoming` (S2), with the tests of this row. |

Scenarios: this unit adds the typed model entries and adapters of `enable_fsrs` and `enable_v3` (`conformance/fsrs-policy-ops#F5`) and marks a `sched/compat/` scenario active in `conformance/scenarios/sched/area.yaml` when every op it uses has a Meguri adapter; the unit that lands the last missing adapter (answers U44, actor sync U107) activates the rest. Until then this unit's tests may play a scenario's steps on the engine against its golden, as `day_conformance.rs` does for the day scenarios.

# FSRS-only policy and collection compatibility

Spec: sched/fsrs-policy
Requirements: R702, R703, R704, R705, R763, R764; AE29
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/compat/

Meguri schedules only with FSRS, on Anki's current scheduler, through the `fsrs` crate at 6.6.2 (`sched/fsrs-transitions#T1`). This file says what Meguri does with a collection that is not in that state: the global FSRS switch is off (R702, R703), a preset holds parameters Meguri cannot use (R704), or the collection is still on the previous scheduler (R705). It also lists the scheduling data that Meguri keeps but never uses or runs (R763, R764).

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

## S2. The seen-on mark (R703)

The mark records that this device has seen FSRS on in this collection, so that Meguri can tell "turned off again by another client" from "never on".

- The engine sets it whenever it finds `fsrs` on: at every open, after every sync's commit (a normal sync's merge and a collection replacement by a one-way sync), and when `enableFsrs` commits (E1). Setting it is not a collection write.
- It is device-only state (KTD12, `storage/config-table` C8): never in the collection file, never synced, exported or backed up with it. It belongs to the collection's place on the device, so it survives a replacement of the collection file (one-way download, restore, `.colpkg` import). It is removed only with the collection.
- In Meguri's own collection, `fsrs` changes only through `enableFsrs`, syncs and replacements. So `needsReEnable` means that another client turned FSRS off after this device saw it on.
- A lost mark only changes the wording: the next status is `needsEnable` instead of `needsReEnable`.

## S3. Study while the status is not `ok`

- `startSession` refuses with `FsrsPolicyPending` while the status is not `ok`, and writes nothing (U43's C5 result). This check comes before the preset check of U4.
- A review session that is already open goes on when a sync turns `fsrs` off: answering, undo and the session's queue never look at the status, and the newest answer stays undoable.
- The app runs `enableFsrs` after `endSession`. When the app ends first, the next open finds `needsReEnable` (S2) and the app runs it then.
- `enableFsrs` refuses with `SessionActive` while a review session is open (E4), so nothing clears the session's undo history.
- The engine has no timer for the policy and publishes no event for it. The app reads the status after every open and after every sync (U51).

## E1. `enableFsrs(cancel, progress)` (R702)

One op that turns FSRS on:

1. When `fsrs` already reads on, it computes nothing and writes nothing (`col.mod` stays). Anki's switch writes nothing in this case too (scenario `sched/compat/re-enable`, last steps).
2. It writes `fsrs` = `true` (config write). A stored value that is not a boolean is replaced.
3. It computes FSRS values for the cards of E2 and writes them as E3 says.
4. After the commit it sets the seen-on mark (S2).

It never runs by itself: the app runs it after its sheet (U51). Opening a collection never runs it.

## E2. The cards it computes

A card is computed when all of these hold:

- it is not new (`type` is not 0); suspended and buried cards are computed;
- it has at least one review-log row, of any type;
- its home deck exists and is a normal deck. A card in a filtered deck is computed through its home deck (`odid`).

Every other card keeps its bytes: new cards, cards with no review-log row, cards whose home deck is missing or is itself a filtered deck.

The card's home preset gives the inputs:

| Home preset | Inputs |
|---|---|
| exists and is supported | its parameter list and decay (`sched/presets#V6`), the desired retention in force (`sched/presets#V7`: the home deck's own value, else the preset's), its historical retention and its ignore date |
| missing (the deck's preset id names no preset) | the built-in defaults (`sched/presets#V1`): empty parameter list, desired retention 0.9, historical retention 0.9, no ignore date |
| unsupported (U1, U2) | none: the card is skipped and keeps its bytes |

## E3. What it writes on a computed card

- U46 computes the card's memory state from its review log with these inputs (`sched/memory-state`). It gives a memory state (stability, difficulty) or none (for example when the only rows are Set Due Date rows).
- The card data then holds: `s` and `d` from the memory state, or neither when there is none; `dr` = the desired retention in force; `decay` = the decay of the parameter list. `pos`, `lrt`, `cd` and unknown keys stay. The text is written per `model/card-data-json` C4.
- A card whose data text does not change keeps its row: `mod` and `usn` stay (R687). A card whose data changes gets `mod` = now (seconds) and `usn` -1.
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
- Cancel: the token is checked between batches. A cancelled op rolls back: `fsrs` is still off, no card changed, and the status is what it was.
- Progress: cards computed out of the number of cards E2 selects.
- While a review session is open it refuses with `SessionActive` and writes nothing (S3).

## E5. Differences from Anki's switch

In Anki the user turns FSRS on in the deck options screen and saves. The oracle's `enable_fsrs` is that save, made so that it does nothing else (`conformance/fsrs-policy-ops` F2). That save does more than `enableFsrs`, and Meguri leaves these out:

- Anki writes the screen's four global options every time (the custom scheduling script, "New cards ignore review limit", "Limits start from top" and the health check), adding the keys that are missing. Meguri writes only `fsrs` (R763).
- Anki saves the presets the screen sends: the screen fills an empty FSRS-6 list from the older lists when it loads, and the save clears the older lists of a preset sent with an empty FSRS-6 list. Meguri writes no preset.
- Anki gives a deck whose preset is missing the preset of the deck the screen was opened for, and computes its cards with that preset. Meguri leaves the deck as it is and computes with the built-in defaults (E2).
- Anki fails the whole save when the parameter list of a preset it sends or computes with cannot be built. Meguri skips those presets (U4).
- Anki writes every computed card, changed or not. Meguri writes only changed cards (R687).
- Anki lets the user keep FSRS off. Meguri turns it on, and that change syncs to the other clients (R702).

## U1. Presets Meguri cannot schedule: lists the crate refuses (R704)

A preset is unsupported when its parameter list (`sched/presets#V6`: the first non-empty one of `fsrs_params_6`, `fsrs_params_5`, `fsrs_params_4`) is refused by the crate's model constructor at 6.6.2 (U45 turns the refusal into `UnsupportedParams`). The crate refuses a list when:

- its length is not 0, 17, 19 or 21; or
- after the crate fills a short list to 21 values, any value is not finite.

The list is used as stored: Meguri never fills, clips or converts it to decide, it asks the crate. Out-of-range but finite values (a negative value, 100.0) are accepted by the constructor.

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

A preset is also unsupported when its `other` bytes (`model/protobuf-records#P7`: the schema-11 members beyond the known keys, `model/schema11-json#S3`) hold a JSON object with a member that:

- is named `fsrsParams` followed by a decimal integer of at least 7 with no leading zero; and
- has a JSON array with at least one element as its value.

A later Anki that adds an FSRS list sends it through sync's schema-11 JSON, and Anki 26.09.3 and Meguri keep it in `other` (`model/schema11-json#S10`). Scheduling that preset with its FSRS-6 list would use parameters the user's newer client no longer uses.

| `other` | Unsupported |
|---|---|
| `{"fsrsParams7":[0.2,1.3]}` | yes |
| `{"x":1,"fsrsParams12":[1]}` | yes |
| `{"fsrsParams7":[]}` | no |
| `{"fsrsParams7":"0.2,1.3"}` | no |
| `{"fsrsParams07":[1]}`, `{"fsrsparams7":[1]}`, `{"fsrsParams6":[1]}` | no |
| empty, not valid JSON, or JSON that is not an object | no |

When an object repeats a member name, the last occurrence counts (`storage/config-table#C3`'s rule for JSON objects).

## U3. What is not detected

- An unknown field of the deck config protobuf message (`model/protobuf-records#P4`) is never read as a later list. No Anki release up to 26.09.3 defines one, so nothing tells such a field from any other later option; refusing study for every preset that carries a later option would be wrong more often than right. Anki 26.09.3 ignores such a field in the same way. When an oracle bump brings a release that defines a later list field, its field number joins U2 (R2022).
- Out-of-range bounded options (`sched/presets#V3`) do not make a preset unsupported; reads replace them.
- A missing preset is not unsupported: its cards use the built-in defaults (`sched/presets#V1`).

## U4. What an unsupported preset does (R704)

- Its decks still count (U41) and browse (U84). Searches, statistics and Card Info read its cards' stored values.
- `startSession` on a deck whose tree holds a card with an unsupported home preset refuses with `PresetUnsupported{presetIds}`: the ids of the unsupported home presets it found, ascending. The `FsrsPolicyPending` check (S3) comes first. Nothing is written (U43's C5 result).
- Grade Now on a selection that holds such a card refuses with `PresetUnsupported` and writes nothing (U48). Answering such a card on any path fails with U45's `UnsupportedParams` and writes nothing.
- Nothing automatic writes the preset or its cards: `enableFsrs` (E2), U46's bulk recomputes and U47's optimizer skip it. The preset row keeps its bytes, `mtime_secs` and `usn` through open, saves of other presets and sync (`sched/presets#V8`); its cards keep their rows. Only an explicit edit of that preset writes it; U49 decides whether its editor offers one.
- The study message ("This preset needs a newer Meguri") is U51's.

Anki 26.09.3 with such presets: it counts their decks, and it fails with an error when it computes answer states for a card of a U1 preset (its study screen cannot show the card). It ignores a U2 list and schedules with `fsrs_params_6`; Meguri refuses instead (R704).

## V1. The scheduler version (R705)

- The storage open gate refuses the v1 scheduler (`storage/open-and-connections#C2.2`, `V1Scheduler`, deviation `storage-v1-scheduler-refused`), so every collection the engine opens has `schedVer` 2.
- With `schedVer` 2, the config key `sched2021` read with the bool reader says which scheduler the collection is on: on means the current scheduler; off (missing, `false`, `1`, `"true"`, `null`, unreadable) means the previous one, v2. Anki 26.09.3 reads it the same way, and its desktop client does not study a v2 collection until the user accepts its scheduler update.

## V2. The switch at open

- An open step of every engine open (`engine/lifecycle` L2 step 3), after the gate and its upgrade, before the collection is `Ready`: when `sched2021` reads off, write `sched2021` = `true` (config write: BLOB `true`, `usn` -1, `mtime_secs` now, `col.mod` now in milliseconds), as one op without an undo entry. When it reads on, nothing.
- Nothing else: no other config key, no card, no deck. In particular no `creationOffset`: Anki's desktop adds that key when the current scheduler loads without it, Meguri never does (`sched/day-clock#D12.3`).
- Silent: no event, no message (R705).
- Anki writes exactly this when the user accepts its scheduler update: `sched2021` = `true`, a local change, `col.mod` moved, nothing else when `creationOffset` is present (scenarios `sched/compat/v3-switch/*`). Meguri writes it at open instead of waiting for the user: deviation `sched-v3-switch-at-open`.
- Only at the engine's open. A sync that brings `sched2021` off, or a replaced collection that has it off, keeps it so until the next open. Meguri's scheduler never reads the key, so nothing depends on it in between.

## V3. Where the open steps run in scenarios

The switch (V2) and the seen-on mark (S2) run whenever Meguri's engine opens a collection: the new collection of a scenario's setup, `reopen`, and handing a collection in at a handoff. The storage area's `open` step uses the storage open gate alone and runs neither, so the upgrade goldens keep their schema-14 to 17 files without `sched2021` (`conformance/storage-ops` P1, as for the day clock in `sched/day-clock#D14.2`).

## K1. Data Meguri keeps but never uses (R763)

These keep their bytes through every Meguri operation (answers, day rollovers, `enableFsrs`, recomputes, the optimizer, saves of presets and decks, sync) and keep syncing as they are:

| Where | What | Notes |
|---|---|---|
| config | `cardStateCustomizer` | K2 |
| config | `fsrsLegacyEvaluate` | another client's evaluation option |
| config | `fsrsHealthCheck` | read by U47, never written |
| config | `fsrsShortTermWithStepsEnabled` | read by U45, never written |
| config | `newCardsIgnoreReviewLimit`, `applyAllParentLimits` | written only by U49's save, and only when the user changes them (`sched/config-keys` G4) |
| config | every key that `sched/config-keys` G2 does not list as written by Meguri, unknown keys included | `storage/config-table#C7` |
| preset | `easy_days_percentages` (Easy Days), the SM-2 options, `fsrs_params_4`, `fsrs_params_5`, `other`, unknown fields | `sched/presets#V9`, `model/protobuf-records#P8` |
| deck | `extend_new`, `extend_review`, `learning_studied` | `model/protobuf-records#P8` |
| filtered deck | its definition (searches, limits, orders, reschedule and preview settings) | the decks area (U28) |

## K2. The custom scheduling script (R764)

- `cardStateCustomizer` holds a JSON string of JavaScript that Anki's reviewers run on the four answer states before an answer is saved. Meguri never runs it, never reads it for scheduling and never writes it. A collection with a script schedules in Meguri exactly as without one (deviation from Anki's reviewers: custom scheduling is out of scope).
- Anki's scheduler core does not run the script either; only its reviewer screens do. So the oracle's answers, and Meguri's, are the scheduler's own states (scenario `sched/compat/custom-scheduling-ignored`: a script that would set every Good review to 99 days leaves the intervals at the scheduler's 8 and 17 days).
- A script may store custom data on a card (`cd` in the card data). Meguri never sets `cd` and keeps an existing one (`model/card-data-json` C2, C4).

## K3. The preservation test

`core/crates/meguri-engine/tests/fsrs_policy.rs` holds the cross-cutting preservation test (R763, T0). It builds a collection in which every K1 value is set to a value other than its default, plus an unknown config key, an unknown deck-config field, a 34-value preset (U1) and a preset with a later list (U2). After each of these steps it compares a digest with the digest taken before the step:

1. a day of answers on a normal preset, across a day rollover;
2. `enableFsrs` from FSRS off;
3. a bulk recompute over all cards (U46);
4. an optimizer commit for a normal preset (U47);
5. a save of another preset;
6. a `SyncIncoming` merge that changes other keys and cards;
7. close and open.

The digest covers: the K1 config rows (`KEY`, `usn`, `mtime_secs`, `val` bytes), the unknown config key's row, the whole rows of the two unsupported presets and of their cards, the Easy Days field bytes and unknown fields of the normal presets, and the `extend_new` and `extend_review` values of the decks. Equal digests mean the test passes.

## T1. Scenarios

| Scenario | Covers |
|---|---|
| `sched/compat/enable-fsrs` | AE29, E1 to E4: crafted SM-2 history; memory states, `dr` and `decay` per card; new cards and cards without history unchanged; due, interval, queue and type unchanged; only `fsrs` among other rows changes |
| `sched/compat/enable-fsrs-live` | AE29 as a live chain: Anki studies a week with SM-2, Meguri turns FSRS on, the golden is Anki turning it on |
| `sched/compat/re-enable` | R703, E1, E3: FSRS turned off by another client and on again; the open with FSRS off writes nothing; enabling while on writes nothing |
| `sched/compat/v3-switch/missing`, `false`, `number`, `string` | V1, V2 with deviation `sched-v3-switch-at-open` |
| `sched/compat/v3-switch/already-on` | V2: nothing written |
| `sched/compat/custom-scheduling-ignored` | K1, K2 through two days of answers |
| `sched/compat/keys-survive-sync` | K1 through Meguri's study and normal sync (actor scenario) |
| `sched/compat/unsupported-preset-kept` | U1, U2, U4: both presets and their cards keep their bytes while another preset is answered and saved |

Goldens are recorded by the oracle maintainer with the driver ops of `conformance/fsrs-policy-ops`.

## T2. Meguri-only tests

`core/crates/meguri-engine/tests/fsrs_policy.rs` asserts what no oracle can show:

- S1, S2: the status for each `fsrs` value of S1; `needsReEnable` after a `SyncIncoming` change of `fsrs` from on to off, also after close and open; `needsEnable` with no mark; the mark kept across a collection replacement.
- S3: a sync during an open session turns `fsrs` off; answers go on; `enableFsrs` refuses with `SessionActive`; the newest answer stays undoable; after `endSession`, `enableFsrs` succeeds and due dates are unchanged. `startSession` refuses with `FsrsPolicyPending` before `PresetUnsupported`.
- E1 to E4: a collection with a 34-value preset: `enableFsrs` skips its cards (bytes unchanged) and computes the others; a deck whose preset is missing is computed with the built-in defaults and keeps its preset id; a cancelled `enableFsrs` writes nothing and leaves `fsrs` off; undo history cleared after it.
- U1 to U4: the tables of U1 and U2; `startSession` returns `PresetUnsupported` with the preset ids; Grade Now refuses; counts are reported for the deck.
- V2: a `schedVer` 2 collection without `sched2021` opens with it on, `creationOffset` untouched (also when missing); the test named by the deviation, `v3_switch_at_open`.
- K3: the preservation test.

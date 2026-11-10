# Review settings keys

Spec: review/review-preferences
Requirements: R128, R129, R130, R131, R132, R133; supports R114, R115, R116, R323, R582, R759, R760, R761; R131 and R354 disagree (P4.1, not settled)
Anki: 26.09.3 (key names, readers, defaults, unit conversions, Anki's Preferences screen and save and its timebox start, as P3 and P7 describe them); none for P3.1, P4.1's accessibility rule, P6.1 and which rows Meguri shows
Scenarios: conformance/scenarios/storage/settings-keys/

The collection config keys behind Settings › Review (U160): for each row, the key, how it reads, how the row shows it, what a change writes and who uses the value. The table and byte rules are `storage/config-table`; the rules every Settings row shares (write only on a change, one key, no undo entry, unknown keys kept) are `storage/settings-keys` S3. The device-local rows of the same page (progress bar, Space and Return, answer keys, Silent Mode, ducking, link opening, Double Tap Prevention) never touch the collection (`storage/settings-keys` S2) and are not in this file.

## P1. The rows

| Row (requirement) | Key | Reader | Default | The row shows | A change stores | Write rules | Uses the value |
|---|---|---|---|---|---|---|---|
| Next day starts at (R128, R760) | `rollover` | `sched/day-clock#D2` | 4 | the effective hour, 0 to 23 | the hour, an integer | P3.1, P6.1 | `sched/day-clock` |
| Learn ahead limit (R129, R760) | `collapseTime` | u32, seconds | 1200 | the seconds / 60 rounded down, in minutes | minutes × 60, an integer | P3.1, P6.1 | the queue (`sched/queue-building`), counts |
| Timebox time limit (R130, R759) | `timeLim` | u32, seconds | 0 | the seconds / 60 rounded down, in minutes; 0 means off | minutes × 60, an integer | P3, P3.1 | the timebox prompt (`review/reviewer-strings` S7) |
| Show next review time above answer buttons (R131; not settled, P4.1) | `estTimes` | bool | `true` | on when the value reads `true` | `true` or `false` | P4, only if P4.1's R131 branch holds | the answer buttons (P4.1) |
| Show play buttons on cards with audio (R133) | `hideAudioPlayButtons` | bool | `false` | on when the value reads `false` (inverted) | the opposite of the toggle: on stores `false` | P4 | `render/card-render-pipeline#K9`, `render/display-transform` D6 |
| Interrupt current audio when answering (R133) | `interruptAudioWhenAnswering` | bool | `true` | on when the value reads `true` | `true` or `false` | P4 | `render/audio-playback#A1` |

- The labels are R128 to R133's, taken from U160's String Catalog. Where the page places the rows among its device-local ones, and which control each row uses, belong to the page's screen contract. `docs/design/screen-inventory.yaml` has only `settings.root` so far, with no contract for Settings › Review; `docs/design/workflow.md` asks for one before U160 builds the page.
- Every row of this table is a synced setting: it carries the "Synced" marker (R113, `docs/design/native-interface-contract.md` N11.4).
- `dueCounts` and `loadBalancerEnabled` have no row on this page (P5).

## P2. Reading

The readers are `sched/config-keys#G1`'s. A missing key, a value not stored as a BLOB, bytes that are not JSON and JSON of another shape read as the key's default; a read never writes (`storage/config-table#C3`).

| Stored `val` | `estTimes` | `hideAudioPlayButtons` | `interruptAudioWhenAnswering` | `timeLim` |
|---|---|---|---|---|
| missing | `true` | `false` | `true` | 0 |
| `true` | `true` | `true` | `true` | 0 |
| `false` | `false` | `false` | `false` | 0 |
| `1` | `true` | `false` | `true` | 1 |
| `0` | `true` | `false` | `true` | 0 |
| `"true"`, `null` | `true` | `false` | `true` | 0 |
| `600` | `true` | `false` | `true` | 600 |
| `600.0`, `6e2`, `"600"`, `-60` | `true` | `false` | `true` | 0 |
| `true` stored as TEXT | `true` | `false` | `true` | 0 |

- `collapseTime` reads as `sched/config-keys#G2` says (u32, 1200), `rollover` as `sched/day-clock#D2` (an integer from 0 to 255, above 23 effective 23, otherwise 4).
- Each reader reads the key again after a change is reported for it, by this device or by sync (`engine/change-stream`), so an open Settings page and an open review screen show the new value.

## P3. Timebox time limit (`timeLim`, R130)

- The row accepts whole minutes from 0 to 9999 (P3.1).
- It shows the stored seconds divided by 60, rounded down: `600` shows 10, `90` shows 1, `59` shows 0 (off). A stored value above the range (`700000` shows 11666) is shown as it is; the user can only commit values within the range.
- A change is written only when the minutes committed differ from the minutes shown (`storage/settings-keys` S3). The value written is the minutes × 60, so the seconds a row shows are what the next timebox uses: with `90` stored, the row shows 1, committing 1 again writes nothing and the key keeps `90`; committing 2 writes `120`.
- The write is one whole-value compact write of the key (`storage/config-table#C4`, `#C5`), as S3's Settings write (P6). A new collection stores `0` (`storage/new-collection` B4); 0 means no timebox prompt (`review/reviewer-strings` S7).
- The timebox clock starts when the session starts and restarts as `review/reviewer-strings` S7 says (Continue, a crossing answer with nothing left to present), whatever `timeLim` holds. With `timeLim` 0 the clock still runs; only the check never triggers. Anki also starts its timebox when study starts, whatever the limit.
- A session starts with the value read at its start (`sched/review-session`, `Started{timeLim}`). A change reported while a session is open (P2) replaces that value for the following checks: the next accepted answer compares the timebox time (S7, R431) with the new limit, and the prompt's minutes come from it. The change resets neither the timebox time nor its card count, and starts no new session. So turning the limit on mid-session can show the prompt at the next accepted answer, counting from the session start or the last restart. Anki reads the key at every check, so its next answer also uses the new limit, with the same result.

### P3.1 Entering hours and minutes (`rollover`, `collapseTime`, `timeLim`)

- A row's value is committed when its control settles: Return, Tab or focus leaving a text field, or one step of a stepper or picker. A value is never written per keystroke: typing 25 in Learn ahead limit and pressing Return writes `1500` once.
- A committed value is accepted when it is a whole number in the row's range: the hour 0 to 23, learn ahead 0 to 999 minutes, timebox 0 to 9999 minutes. Any other committed value (out of range, negative, with a fraction, empty, not a number) is refused. Nothing is written, no alert appears, and the row shows its current read again. A stepper or picker cannot leave the range.
- A committed value equal to the one shown writes nothing (S3 item 2), also when the stored value is another shape or not a whole number of minutes.

## P4. The boolean rows (`estTimes`, `hideAudioPlayButtons`, `interruptAudioWhenAnswering`)

- Each row is a toggle that shows P1's reading. Flipping it writes the new boolean (for the play buttons, the inverted one) through S3's Settings write (P6): one whole-value compact write of `true` or `false`.
- A missing key stays missing until the user flips its row; a stored value of another type (`1`, `"true"`) keeps its bytes until then. Going back to the default writes the default value; Settings never removes a key.
- Example: `hideAudioPlayButtons` stored as `1`. The row reads `false` and shows on. Opening and closing Settings writes nothing. Turning the row off writes `true`; turning it on again writes `false` (the `1` is gone).
- The `estTimes` row exists, and is written, only if P4.1's R131 branch holds.

### P4.1 What `estTimes` changes (not settled)

The plan disagrees with itself. R131 says the row "shows or hides the interval label". R354 says interval labels are always shown and the key is preserved unchanged, as a deviation from Anki, and U178's test list follows R354 ("`estTimes` = false → labels still shown"); `review/answer-flow` F4 records the same open point. Until the plan settles it, the interim behavior is the R354 branch below, which writes nothing: U178 keeps its current test list, U160 builds no `estTimes` row, and nothing in Meguri reads or writes the key. The R131 branch is built only once the plan chooses it.

- **If R131 holds.**
  - When `estTimes` reads `true`, each answer button shows its interval label (`sched/answer-labels`), placed as the design contract says (N11.2, N12.3 on the Mac, N13.3 on touch), and its accessibility label is `review.answer.accessibilityLabel` with the spoken interval (`review/reviewer-strings` S13).
  - When it reads `false`, no answer button shows its interval, and each button's accessibility label is its grade name alone (Meguri's rule: the spoken interval follows the visible one). The four buttons keep their order and equal widths, and each takes the standard height of a button showing only its name. The intervals are still computed by the session; only their display changes, never what answering applies.
  - The review screen reads the key each time an answer side is presented and again when a change of the key is reported (P2). An answer side on screen then shows or hides its intervals at once, without re-rendering the card.
- **If R354 holds.** Interval labels are always shown, whatever `estTimes` holds. Settings shows no `estTimes` row and never writes the key, which keeps its bytes, `usn` and `mtime_secs` (R354, R582). P1's `estTimes` row and P4's `estTimes` write are dropped; the key is then like `dueCounts` (P5). `conformance/review-config-ops` R2 still compares its read.

## P5. Review keys Settings never writes

- `dueCounts` (Anki's "show remaining card count") has no row: Meguri never shows remaining counts in review, and its progress bar is a device-local setting (R132). Settings and the review screen never read or write `dueCounts`; it keeps its bytes, `usn` and `mtime_secs` (`storage/config-table#C7`, R582).
- Anki's preferences save also writes `dayLearnFirst` and `newSpread` (the previous scheduler's options), `creationOffset`, `renderLatex` and `fsrsShortTermWithStepsEnabled` (`sched/config-keys#G2`), each from the value it read (P7). Of these, Anki's Preferences screen has a control only for `renderLatex`. Meguri shows none of them on any Settings page; Settings never writes them, and they survive unchanged (`storage/config-table#C7`). `sched/day-clock#D2` reads `creationOffset`.
- `loadBalancerEnabled` is in Anki's review preferences group. Meguri's "Load balancer" toggle is in deck options' Advanced section (R665, R751, U232); Settings has no row for it (`storage/settings-keys` S2).

## P6. Changes and undo

- A write that changes a key reports a config change naming it (`engine/change-stream`), so open screens follow:
  - the answer buttons (P4.1, if R131 holds);
  - the card page's play buttons (`render/card-render-pipeline#K9`): the card on screen keeps its page, on both sides; the next card presented is rendered with the new value. Its `renderKey` includes the effective `hideAudioPlayButtons` (`sched/review-session` C6), so a page prepared before the change is not used;
  - the audio controller (`render/audio-playback#A1`), for transitions after the change;
  - the timebox (P3).
- The `timeLim` and boolean writes are S3's Settings write: an op of source `UserNonUndoable` marked skip-undo (`engine/undo` M2). It records no undo entry (R115); earlier undo steps stay undoable, and the redo history is emptied.

### P6.1 The `rollover` and `collapseTime` rows

- Each row submits the Settings form of `sched/deck-options#O10`'s scheduling preferences save (edit request for O10) with the one field the user changed: `rollover` as the hour, or the learn-ahead minutes.
- It is written only when the committed value differs from the value shown: for `rollover` the effective hour, for `collapseTime` the shown minutes. It writes only that key: `rollover` as the integer hour, `collapseTime` as minutes × 60. It never writes the other field or `loadBalancerEnabled`, and adds no missing key.
- It is a skip-undo write of source `UserNonUndoable`, like P6's: no "Preferences" undo step.
- O10's other rules apply unchanged: validation (`out_of_range`, nothing written), the day clock computing afresh when `rollover` changes (`sched/day-clock#D6`, `#D8`, `#D13`), the rollover op when the day number changes (`sched/day-clock#D9`, which clears both histories), and the queue rebuild and count refresh (R761).
- Example: a new collection has `collapseTime` `1200` and no `rollover` or `loadBalancerEnabled` row (`storage/new-collection` B4). The row shows 4. Committing 4 writes nothing. Committing 5 writes `rollover` `5` and nothing else, with no undo step. O10's undoable op (`conformance/deck-options-ops` E4) would also insert `loadBalancerEnabled` `true` and record "Preferences".

## P7. Differences from Anki

- **One save of everything.** Anki's Preferences screen rebuilds its preferences from its controls when it closes and compares them with those it read on opening. When they differ, it saves the scheduling, review, editing and backup groups together as one undoable step named "Preferences" (`conformance/undo-ops` V4 `preferences`). That save writes `rollover` only when the hour differs from the effective hour. It sets each other key of the groups from the control or from the value it read: `collapseTime`, `dayLearnFirst`, `newSpread`, `timeLim`, `estTimes`, `dueCounts`, `hideAudioPlayButtons`, `interruptAudioWhenAnswering`, `loadBalancerEnabled`, `fsrsShortTermWithStepsEnabled` and the editing keys (`editor/config-keys#K6`), each write skipped when the compact bytes are equal. So the save:
  - adds each missing key with its read default;
  - replaces a value of another shape with its typed read: `estTimes` `0` becomes `true`, which turns Anki's own interval labels back on (they read the raw value, below); `timeLim` `600.0` becomes `0`, which turns Anki's timebox off; a `newSpread` other than 0, 1 or 2 becomes `0`;
  - removes a `creationOffset` row that does not read as a 32-bit integer (for example `null`), and adds none.

  Meguri writes only the key of the row the user changed (P3, P4, P6.1, `storage/settings-keys` S3).
- **Saving with no change.** Anki's screen shows the learn-ahead and timebox limits as the seconds / 60 rounded down, rebuilds them as the shown minutes × 60, and its spin boxes clamp: learn ahead 0 to 999 minutes, timebox 0 to 9999, hour 0 to 23, minutes between backups 5 to 9999, each backup count 0 to 9999. A stored value that does not survive that round trip makes the rebuilt preferences differ, so closing the screen without touching anything performs the whole save above as one undoable "Preferences" step. Examples: `timeLim` or `collapseTime` not a whole number of minutes (`90` becomes `60`), a learn ahead above 999 minutes, a timebox above 9999 minutes, minutes between backups below 5. Meguri writes nothing on opening and closing (`storage/settings-keys` S4) and keeps the stored seconds until that row itself changes (P3, P6.1).
- **Ranges.** Meguri accepts the same ranges as Anki's spin boxes and refuses committed values outside them instead of clamping (P3.1).
- **The reviewer's own reads.** Anki's review screen reads `estTimes` and `dueCounts` as raw JSON values by truthiness, and its timebox check reads `timeLim` raw after every answer; a missing key makes any of the three reads fail. Its Preferences screen reads them with P2's typed reads. So a stored `0` hides Anki's interval labels while its Preferences shows the box checked, and a stored `600.0` gives Anki a 10-minute timebox. Meguri uses P2's reads everywhere. With the values Anki writes (`true`, `false`, integers) both read the same.
- **Undo.** Anki records the Preferences save as an undoable step; Meguri records none for P3, P4 and P6.1 (R115).

## P8. Tests and scenarios

The oracle runs Anki's collection library, not its Preferences or review screens (`conformance/AGENTS.md`), so the screens' rules (P1's display, P3's minutes rule and clock, P3.1, P4.1, P6.1's single-key form) are Meguri's tests: `SettingsStoreTests` (U160) for P1 to P4, P3.1, P6 and P6.1, the session's timebox tests (U185) for P3's mid-session change, and `AnswerButtonsTests` (U178) for P4.1 once it is settled.

| Scenario | Covers | Status |
|---|---|---|
| `storage/settings-keys/review-key-writes` | P3, P4: the stored side of each write over a missing key, the same value and a value of another type; bytes, `usn`, `mtime_secs`, `col.mod`; no undo entry; `dueCounts` untouched | draft, inactive, golden not recorded |
| `storage/settings-keys/prefs-keep-seconds` | P3, P7 with `sched/deck-options#O10`'s undoable op: a scheduling save of another field keeps a stored `collapseTime` and `timeLim` of 90 seconds; a save of the effective hour over a stored `30` writes nothing | draft, inactive, golden not recorded |
| `storage/settings-keys/review-reads` (planned) | P2's table through Anki's review preferences read | needs the draft kind `review_config` (`conformance/review-config-ops` R2); not written |

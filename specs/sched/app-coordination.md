# App-side scheduling coordination

Spec: sched/app-coordination
Requirements: R459, R464, R597, R702, R703, R704, R1854
Anki: none (Meguri-defined; no oracle parity)
Scenarios: none (U51's app tests against a fake core, from the tables below)

What the app does so that the core's day clock (`sched/day-clock`) and FSRS policy (`sched/fsrs-policy`) take effect while Meguri runs: when it calls `checkRollover()`, when its one day-clock timer fires, and when it shows the FSRS sheet and runs `enableFsrs`. Changed 2026-11-19 (F5): there is no FSRS sheet; the app runs `enableFsrs` by itself. The core decides the day, the next boundary and the policy status; the app only chooses when to ask and shows the answer (KTD5). Nothing here polls (R1854): the app holds at most one day-clock timer, armed for one instant, and the policy coordinator has no timer.

Anki's desktop client also checks the day from a one-shot timer armed at its cached boundary and re-arms it after each check. In addition it refreshes its deck screens on a fixed ten-minute interval; Meguri has no such interval.

## A1. When the app checks the day

The day-clock monitor (one per open collection, owned by the collection store, U151) calls `checkRollover()` on the interactive lane (KTD9) when:

1. its timer fires (A2);
2. the system reports a time zone change (R464);
3. the system reports that its clock was set, or a significant time change (for example a daylight saving change, or midnight on iOS);
4. on the Mac, the system wakes from sleep;
5. the lifecycle is `Ready` again after a check failed (A2).

On return to the foreground the monitor makes no call of its own: U151's foreground sequence calls the core's `resume()` (U32), which runs the check, and the monitor takes `nextDayAt` from its `ResumeReport`.

Before a call caused by a zone or clock change, the clock the app supplies to the core (the current offset and zone id of `sched/day-clock#D1`, through the host configuration, U37/U38) must already report the new zone and offset. A zone value cached before the change makes the check find nothing (Foundation can keep the system zone it read earlier until its cache is reset).

The other checks run in the core and need nothing from the monitor (`sched/day-clock#D8`): the open-time check; the check before every CloudKit incoming apply (the policy's `ApplyReady`, `sync/policy#P4`, `sync/cloudkit-records#K13.2`) and once after every Sync Now exchange, whatever its result (`sync/policy#P12.7`), both U113 and U114; the check before and after a manual Anki transfer (U107, frozen, KTD26); and the check before every answer (U43).

## A2. The timer

- The monitor holds at most one timer. Arming replaces the armed timer; closing the collection cancels it.
- It arms from every value it learns:
  - each `checkRollover()` result (`next_day_at`), changed or not;
  - the `nextDayAt` of a `ResumeReport`;
  - every `NextDayAtChanged{nextDayAt}` event, whatever caused it (a synced or edited `rollover`, a `creationOffset` or `crt` change, an offset change seen by any check, a day change);
  - after the collection opens and after `CollectionReplaced`, one `checkRollover()` call. The core's open-time check has already run, so this call writes and emits nothing unless the clock moved in between (`sched/day-clock#D8`).
- `RolledOver` and `TimeZoneChanged` need no timer action of their own: a moved boundary always comes with `NextDayAtChanged` or in a check result.
- The deadline is the earlier of:
  - `nextDayAt`;
  - the next instant after now at which the device's current time zone changes its UTC offset (daylight saving), from the platform's time zone data, computed again at every arming;

  but never earlier than the start of the next whole second after now (now in whole seconds as in `sched/day-clock#D1`, plus one). A `nextDayAt` at or before now (the first second of a day in legacy timing, `sched/day-clock#D4`, or a value that arrived late) so gives one check at the next whole second, never a loop.
- Why the offset change: after an offset change the day clock computes the day afresh (`sched/day-clock#D6`, `#D12.1`), and the boundary under the new offset can come before the `nextDayAt` computed under the old one. On the night Paris moves its clocks forward, a timer armed only at `nextDayAt` fires at 05:00 local, an hour after Meguri's new day started (A3 rows 6 to 8).
- Use an absolute wall-clock deadline.
  Run A1 checks 3 and 4 after a clock adjustment or wake.
  An elapsed-duration timer alone cannot provide this behavior.
- When the timer fires, the monitor calls `checkRollover()` once and arms from its result.
- A call that fails (`Unavailable`: no collection, or an exclusive phase, `sched/day-clock#D9`) arms nothing. The monitor calls again when the lifecycle is `Ready` again or on `CollectionReplaced`, whichever comes first. Meanwhile the core runs its own check once the collection is ready (D9), and the `NextDayAtChanged` that check emits also arms the timer.
- Order: the result of a call (the monitor's `checkRollover()`, or U151's `resume()`) does not replace a deadline set by a `NextDayAtChanged` event that the monitor received after that call began. Events arrive in the order the core emitted them, and a check that finds a boundary other than the last one reported emits its own event, so this keeps the newest value. A stale later deadline would miss the boundary; a stale earlier one costs one check.

## A3. Examples

Europe/Paris, `crt` 1894676400 (2030-01-15 04:00 +01:00), `creationOffset` -60, `rollover` 4 unless the row says otherwise (`sched/day-clock#D3.1`). Instants are epoch seconds; local times are Paris times unless the row names another zone. "Learns" lists the check result and the events the monitor receives.

| # | Situation | The monitor learns | Armed for |
|---|---|---|---|
| 1 | The collection opens on 2030-01-20 10:00 (1895130000) | check: today 5, `next_day_at` 1895194800 (01-21 04:00) | 1895194800 (Paris's next offset change, 2030-03-31, is later) |
| 2 | Row 1's timer fires | check: today 6; `RolledOver{5, 6}`, `NextDayAtChanged{1895281200}` (01-22 04:00) | 1895281200 |
| 3 | `rollover` 6, today 4, armed for 1895115600 (01-20 06:00). At 03:00 (1895104800) a sync brings `rollover` 4 | `NextDayAtChanged{1895108400}` (01-20 04:00); the day does not change | 1895108400 |
| 4 | As row 3, but the sync lands at 05:00 (1895112000) | `RolledOver{4, 5}`, `NextDayAtChanged{1895194800}` (01-21 04:00) | 1895194800 |
| 5 | `rollover` 6, today 5, armed for 1895202000 (01-21 06:00). At 10:00 a sync brings `rollover` 4 | `NextDayAtChanged{1895194800}` | 1895194800 |
| 6 | Checked on 2030-03-30 12:00 +01:00: today 74, `next_day_at` 1901156400 (03-31 04:00 +01:00, which is 05:00 local after the change). Paris moves to +02:00 at 1901149200 | (nothing more) | 1901149200, the offset change |
| 7 | Row 6's timer fires (03:00 +02:00) | check: today 74, `next_day_at` 1901152800 (04:00 +02:00); `NextDayAtChanged{1901152800}`; the core writes `localOffset` -120 | 1901152800 |
| 8 | Row 7's timer fires | check: today 75; `RolledOver{74, 75}`, `NextDayAtChanged{1901239200}` (04-01 04:00 +02:00) | 1901239200 |
| 9 | Checked on 2030-10-26 12:00 +02:00: today 284, `next_day_at` 1919296800 (10-27 04:00 +02:00, which is 03:00 local after the change). Paris moves to +01:00 at 1919293200 | (nothing more) | 1919293200, the offset change |
| 10 | Row 9's timer fires (02:00 +01:00) | check: today 284, `next_day_at` 1919300400 (04:00 +01:00); `NextDayAtChanged{1919300400}` | 1919300400 |
| 11 | On 2030-01-20 10:00 the zone becomes Africa/Ceuta (same offset) | check: today 5, `next_day_at` 1895194800; `TimeZoneChanged{Africa/Ceuta}` | 1895194800 |
| 12 | On 2030-01-20 10:00 Paris time (18:00 +09:00) the zone becomes Asia/Tokyo | check: today 5, `next_day_at` 1895166000 (01-21 04:00 +09:00); `TimeZoneChanged{Asia/Tokyo}`, `NextDayAtChanged{1895166000}` | 1895166000 (Tokyo has no offset change) |
| 13 | On 2030-01-20 23:00 Paris time (01-21 07:00 +09:00) the zone becomes Asia/Tokyo | check: today 6; `TimeZoneChanged{Asia/Tokyo}`, `RolledOver{5, 6}`, `NextDayAtChanged{1895252400}` (01-22 04:00 +09:00) | 1895252400 |
| 14 | Armed as in row 1, the clock is set from 2030-01-20 10:00 to 2030-01-22 10:00 | check: today 7; `RolledOver{5, 7}`, `NextDayAtChanged{1895367600}` (01-23 04:00) | 1895367600 |
| 15 | Armed as in row 1, the clock is set back from 2030-01-20 10:00 to 2030-01-18 10:00 | check: today 5, `next_day_at` 1895194800 unchanged (`sched/day-clock#D6`) | 1895194800, about 66 hours later on the wall clock |
| 16 | Armed as in row 1, the Mac sleeps from 2030-01-20 23:00 to 2030-01-21 08:00 | at the wake: check: today 6; `RolledOver{5, 6}`, `NextDayAtChanged{1895281200}` | 1895281200 |
| 17 | Legacy timing (`crt` 1894755600, no `creationOffset`, `sched/day-clock#D4`), today 3, armed for 1895108400 (01-20 04:00); the timer fires | check: today 4, `next_day_at` 1895108400, equal to now; `RolledOver{3, 4}`, no `NextDayAtChanged` | 1895108401, the next whole second |
| 18 | Row 17's timer fires | check: today 4, `next_day_at` 1895194800; `NextDayAtChanged{1895194800}` | 1895194800 |

## F1. When the app reads the FSRS policy status

The FSRS policy coordinator reads `fsrsPolicyStatus()` (`sched/fsrs-policy#S1`) on the query lane:

1. after the collection opens;
2. after `CollectionReplaced` (Replace local collection from iCloud, `sync/cloudkit-recovery#V14`; a manual Anki import; restore; `.colpkg` import): the new collection can have `fsrs` off even when the old one had it on;
3. after every sync: every Sync Now exchange, whatever its result (its `ExchangeEnded` result, `sync/policy#P12.7` and P12.6, early stops included; an exchange ended by a close or a replacement records nothing, and the new generation's open read follows item 2; a failed exchange may still have applied entries, and the read is idempotent), each committed change set with source `CloudIncoming` (`sync/cloudkit-records#K13.2`) whose `config_keys` include `fsrs` (`engine/change-stream#S1`), and a completed manual Anki transfer (KTD26);
4. when a review session ends, if a read during the session found a status other than `ok` (F3);
5. when a study entry point is refused with `FsrsPolicyPending` (F4).

It reads at no other time and has no timer. The core publishes no event for the policy (`sched/fsrs-policy#S3`).

Changed 2026-11-19 (F5): the items are unchanged; what a read that is not `ok` starts is F5, not a sheet. After items 1 and 2 the job may first wait for the open-time sync (F5.1).

## F2. The sheet and `enableFsrs` (Removed 2026-11-19)

Removed by Antoine's decision of 2026-11-19 (`sched/fsrs-policy#A1`): Meguri shows no FSRS sheet, in the first-time or the re-enabled form, and has no action for it. `FsrsEnabledSheet`, its texts in the String Catalog, its snapshots and the screen `home.fsrsSheet` are removed. F5 says what a status other than `ok` starts.

## F3. During a review session

- Changed 2026-11-19 (F5.2): "the coordinator acts on that read" below means that it runs `enableFsrs` in the background.
- A status other than `ok` found while a review session is open does not interrupt it: no sheet, no `enableFsrs` (it would refuse with `SessionActive`, `sched/fsrs-policy#S3`), and the newest answer stays undoable.
- When the session ends, the coordinator reads the status again (it can have changed since) and acts on that read.
- When the app quits before the session ends, the next open reads the status (F1 item 1).

## F4. Study while the status is not `ok`

- While a flow runs, study entry points wait for it to end, then go on.
- `startSession` refuses with `FsrsPolicyPending` while the status is not `ok` (U43's session start result; `sched/fsrs-policy#S3`). An entry point refused this way starts the flow (F1 item 5) instead of showing an error, so study is never blocked without the sheet.
- Changed 2026-11-19 (F5.2, F5.3): "a flow" is F5's job, and "the sheet" is gone: an entry point refused with `FsrsPolicyPending` starts or joins the job, shows the normal loading state of a starting session while it runs, and goes on when it commits. When it fails, the entry point shows F5.3's sentence with Try Again.

## F5. FSRS turned on by itself (2026-11-19)

The app side of `sched/fsrs-policy#A1`. No view asks, explains or offers a choice; no setting, menu command or debug option turns FSRS off or skips the step.

### F5.1 After an open

- The collection is presented as soon as it is open, whatever the status. The read of F1 item 1 or 2 that finds `needsEnable` or `needsReEnable` schedules one `enableFsrs(cancel, progress)` job:
  - at once, when no exchange is to be expected by itself (decided 2026-11-19 (review F2)): the service is Off, Sync Automatically is off (the update hold included, `sync/policy#P25.2`), or the service's status needs the user (an account or sign-in, a pending setup or decision);
  - otherwise, with a connected service in any other state, offline and waiting for a retry included: not at once. F1 item 3's read after the next collection exchange, whatever its result, starts the job, or finds `ok` when the sync brought FSRS on. Media transfer is not waited for. A change of the service to one of the states of the first item also starts it.
- The coordinator has no timer and makes no network call for this: it reads the status the service already publishes.
- A study entry point used while the job is still waiting for the sync starts it at once (F4).
- The job runs in the background and shows nothing. No Cancel is shown, and the app requests the job's cancel only to close the collection (app quit, a replacement).

### F5.2 While the collection is open

- A read of F1 items 3 to 5 that finds a status other than `ok` starts the job in the background at once, unless one runs already or a review session is open (F3); it shows nothing. It is one job per status found, not one per read.
- Study entry points wait for it (F4). Everything else stays usable: browsing, the editor, import, export, sync, settings.
- `enableFsrs` writes nothing when `fsrs` already reads on, so a job that starts after a sync turned FSRS on again is harmless.
- `CollectionReplaced` during the job ends it; F5.1 applies to the new collection.

### F5.3 Failure

- A job that ends without a commit (an error; a cancel caused by anything but a close) leaves the collection open and shows nothing by itself, whether it was scheduled by F5.1 or F5.2.
- The next study entry point is refused (F4) and shows this text with one Try Again button, which starts the job again:

  > Meguri couldn’t prepare this collection for study.

  String Catalog keys (decided 2026-11-19 (review F12)): `study.prepare.failed` for the sentence and `study.prepare.tryAgain` for the button.

- Nothing else is disabled or marked. There is no "study anyway". The next open applies F5.1.
- The underlying error goes to the diagnostic log and Copy Debug Info, not into the sentence.
- The text is Meguri's own, from the String Catalog; no Anki text applies.

### F5.4 Record

- Each job writes one entry to the app's diagnostic log: the status that caused it, whether F5.1 or F5.2 scheduled it, the cards computed and the result. A re-enable after another client turned FSRS off (`needsReEnable`) is recorded every time it happens. Nothing is shown for a job that commits.

## M1. Presets Meguri cannot schedule (R704)

When `startSession` refuses with `PresetUnsupported{presetIds}` (`sched/fsrs-policy#U4`, U43), or an open session reports `PresetUnsupported` (U43's session event), study shows this message, word for word from R704:

> This preset needs a newer Meguri

This unit adds the text to the String Catalog; Home (U152) and the review screen (U175) show it. The presets' decks stay browsable and keep their counts.

## T1. Tests

U51's app tests drive the monitor with an injected clock and time zone and a fake core that reports the rows of A3, and check A1 and A2: the trigger list, one call per trigger, the deadline rule, no re-arm after a failed call, and the order rule. The coordinator's tests script the status reads of F1 to F4. Changed 2026-11-19 (F5): no test expects a sheet. Added: with `needsEnable` and with `needsReEnable` at open the collection is presented at once and nothing is asked; with the service Off, with Sync Automatically off and with each needs-the-user state, the job starts at once; with a connected service that is syncing, offline or waiting for a retry it does not start, and starts only after a collection exchange ends, for a successful and a failed exchange, and not at all when the exchange brought `fsrs` on; a change to a needs-the-user state during the wait starts it, and a change to offline does not; a study entry point during the wait starts it and goes on at its commit; no timer exists during the wait (R1854); a failed job, after an open and in the background alike, leaves the collection open, the editor, import, export and sync usable, and study refused with the F5.3 text and Try Again, which runs the job again; `CollectionReplaced` applies F5.1 to the new collection; a not-`ok` read after a later sync starts one background job, waits for an open review session, and a second read during the job starts none; each job writes its log entry, a second `needsReEnable` included; with `ok`, no job runs. A source check that no view or String Catalog key of the removed sheet remains.

# App-side scheduling coordination

Spec: sched/app-coordination
Requirements: R459, R464, R597, R702, R703, R704, R1854
Anki: none (Meguri-defined; no oracle parity)
Scenarios: none (U51's app tests against a fake core, from the tables below)

What the app does so that the core's day clock (`sched/day-clock`) and FSRS policy (`sched/fsrs-policy`) take effect while Meguri runs: when it calls `checkRollover()`, when its one day-clock timer fires, and when it shows the FSRS sheet and runs `enableFsrs`. The core decides the day, the next boundary and the policy status; the app only chooses when to ask and shows the answer (KTD5). Nothing here polls (R1854): the app holds at most one day-clock timer, armed for one instant, and the policy coordinator has no timer.

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

## F2. The sheet and `enableFsrs`

- A read that returns `needsEnable` or `needsReEnable` starts the flow below, unless a flow is already running (its sheet is shown or its `enableFsrs` job runs): a read during a flow does nothing. This is R702's "one-time": one sheet per status found, not one per read.
- While a review session is open the flow waits (F3).
- The sheet's text follows the status. `needsEnable`: the first-time text (R702: Meguri schedules with FSRS; it turns FSRS on for this collection, computes memory states from the review history, and changes no due date or interval; the change reaches other devices by sync). `needsReEnable`: the re-enabled text (R703: another device turned FSRS off, and Meguri turned it on again). The wording is Meguri's own, from the String Catalog; no Anki text applies.
- The sheet has one action and no way to keep FSRS off (R702's deviation from Anki).
- The action runs `enableFsrs(cancel, progress)` (`sched/fsrs-policy#E1`) as a job, showing its progress (cards computed out of the cards selected, `#E4`).
- When the op commits the status is `ok`, the flow ends, and the sheet does not show again. A later read finds `ok`, or `needsReEnable` once another device turns FSRS off again.
- Anything that ends the flow without a commit (the sheet dismissed, if it can be; cancel; an error; `SessionActive`; the app quitting) leaves the status as it was: a cancelled or failed op writes nothing (`#E4`). The next F1 read starts the flow again, with the text of the status it finds.
- `enableFsrs` writes nothing when `fsrs` already reads on (`#E1` step 1), so the action stays safe when a sync turned FSRS on while the sheet was shown.
- `CollectionReplaced` during a flow closes the sheet (U151); F1 item 2 reads the new collection's status.

## F3. During a review session

- A status other than `ok` found while a review session is open does not interrupt it: no sheet, no `enableFsrs` (it would refuse with `SessionActive`, `sched/fsrs-policy#S3`), and the newest answer stays undoable.
- When the session ends, the coordinator reads the status again (it can have changed since) and acts on that read.
- When the app quits before the session ends, the next open reads the status (F1 item 1).

## F4. Study while the status is not `ok`

- While a flow runs, study entry points wait for it to end, then go on.
- `startSession` refuses with `FsrsPolicyPending` while the status is not `ok` (U43's session start result; `sched/fsrs-policy#S3`). An entry point refused this way starts the flow (F1 item 5) instead of showing an error, so study is never blocked without the sheet.

## M1. Presets Meguri cannot schedule (R704)

When `startSession` refuses with `PresetUnsupported{presetIds}` (`sched/fsrs-policy#U4`, U43), or an open session reports `PresetUnsupported` (U43's session event), study shows this message, word for word from R704:

> This preset needs a newer Meguri

This unit adds the text to the String Catalog; Home (U152) and the review screen (U175) show it. The presets' decks stay browsable and keep their counts.

## T1. Tests

U51's app tests drive the monitor with an injected clock and time zone and a fake core that reports the rows of A3, and check A1 and A2: the trigger list, one call per trigger, the deadline rule, no re-arm after a failed call, and the order rule. The coordinator's tests script the status reads of F1 to F4. No oracle scenario applies: everything here is Meguri's. U51's verification also looks for repeating timers in a 60-second idle trace (R1854).

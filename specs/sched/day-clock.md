# Day clock and rollover

Spec: sched/day-clock
Requirements: R592, R593, R594, R595, R596, R597, R598, R599, R760, R464, R1897, R1898, R1899, R1900
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/day/, conformance/scenarios/sched/unbury-on-rollover/

How the scheduler's day number ("today") and the instant the next day starts (`next_day_at`) are computed from the collection and the device clock, when they are computed again, what the computation writes, and what happens when the day changes. Everything that is due "today" depends on these two values (R593), so they must equal Anki's at every instant that the scenarios check.

The collection's `crt` column and the config keys of D2 are interface names (`specs/storage/config-table.md`, `specs/storage/schema-18.md`). Reads of config values follow `storage/config-table#C3`, writes `storage/config-table#C4`, except where D5 and D7 say otherwise about `col.mod`.

## D1. Terms

- **Instants** are whole seconds since the Unix epoch. The day clock takes `now` as the clock's milliseconds divided by 1000, rounded down. It never uses fractions of a second.
- **Offsets.** An offset is the difference between local time and UTC, in seconds, positive east of UTC (+01:00 is 3600). Config keys store offsets as **minutes west**: the offset in seconds, negated, divided by 60 (+01:00 is `-60`, -08:00 is `480`, +05:45 is `-345`, -03:30 is `210`).
- **Current offset**: the device's offset in effect at `now`, which the core gets from the clock seam (`meguri-model` `Clock::utc_offset_at`). The core never looks up a time zone itself.
- **Local date of an instant under an offset**: the calendar date (proleptic Gregorian) of the instant plus the offset. Days between two dates are counted in calendar days.
- **Boundary of a date at hour h under an offset `o`**: the instant whose local time under `o` is that date at h:00:00. One fixed offset is used for the whole computation. It is never corrected for a time zone rule, so on a day when the zone changes its offset the boundary can differ from the zone's real h:00 (D3.3).
- **Effective rollover hour `h`**, **creation offset**, **legacy timing**: D2.
- **Day query**: one computation of today and `next_day_at`, with its writes (D5).

## D2. The timing keys

| Key | Read as (C3 shape) | Effective value |
|---|---|---|
| `rollover` | an integer from 0 to 255 | the value, or 23 when it is above 23. When the key is missing or unreadable (a negative number, a number above 255, a number with a fraction or exponent such as `4.0`, a string, `true`, `null`), the hour is 4 and the day query writes `4` (D5). |
| `creationOffset` | a 32-bit signed integer, minutes west | the creation offset, when the value is an integer from -1439 to 1439. Any other state (missing, `null`, `-60.0`, `"-60"`, `1440`) means the collection has no creation offset and uses legacy timing (D4). Nothing in the day clock writes this key (D11, D12.3). |
| `localOffset` | a 32-bit signed integer, minutes west | the last offset the device saw, when the value is an integer from -1439 to 1439; otherwise 0 (UTC). Written by the day query (D5). |
| `lastUnburied` | an integer from 0 to 4294967295 | the last day on which buried cards were restored; 0 when missing or unreadable. Written by the unbury (D7). |
| `collapseTime` | an integer from 0 to 4294967295, seconds | the learn-ahead limit (R602, R760; used by the queue area); 1200 when missing or unreadable. Never written by the day clock. |

- These keys are synced like any other config key. Another device's writes reach this collection through sync.
- `rollover` is the setting "Next day starts at" (R760). A settings screen shows the effective hour; when it saves, it writes an integer from 0 to 23.
- `crt` is the `col` row's `crt` column, in epoch seconds.
- `schedVer` 1 collections use another day count. Meguri refuses to open them (`storage/open-and-connections#C2`), so this spec does not cover it.

## D3. The day count with a creation offset

When the collection has a creation offset (D2):

1. `C` = the local date of `crt` under the creation offset.
2. `N` = the local date of `now` under the current offset.
3. `R` = the boundary of `N` at hour `h` under the current offset.
4. The day has rolled over today when `R` is at or before `now`.
5. Today = (days from `C` to `N`), minus 1 when the day has not rolled over today. A negative result is 0.
6. `next_day_at` = `R` + 86,400 when the day has rolled over today, else `R`.

### D3.1 Examples

Europe/Paris in January (+01:00), `crt` 2030-01-15 04:00 +01:00 (1894676400), `creationOffset` -60, rollover 4 unless given:

| now (local) | h | today | next_day_at (local) |
|---|---|---|---|
| 2030-01-15 10:00:00 | 4 | 0 | 2030-01-16 04:00:00 |
| 2030-01-16 03:59:59 | 4 | 0 | 2030-01-16 04:00:00 |
| 2030-01-16 04:00:00 | 4 | 1 | 2030-01-17 04:00:00 |
| 2030-01-20 10:00:00 | 12 | 4 | 2030-01-20 12:00:00 |
| 2030-01-20 12:00:00 | 12 | 5 | 2030-01-21 12:00:00 |
| 2030-01-12 10:00:00 (before `crt`) | 4 | 0 | 2030-01-13 04:00:00 |

- Asia/Kathmandu (+05:45), created 2030-01-15 10:00 local: at 2030-01-20 03:59:59 today is 4; at 04:00:00 and 04:00:01 it is 5, with `next_day_at` 2030-01-21 04:00 local (AE84).
- Created in Asia/Tokyo (`crt` 2030-01-15 04:00 +09:00 = 1894647600, `creationOffset` -540), studied in America/Los_Angeles (-08:00): today is 0 at 2030-01-15 10:00 -08:00, 0 at 2030-01-16 03:59:59, 1 at 04:00:00 (R1900).
- Created in America/Los_Angeles (`crt` 2030-01-14 04:00 -08:00 = 1894622400, `creationOffset` 480), studied in Asia/Tokyo: today is already 1 at 2030-01-15 10:00 +09:00, because `N` is a calendar day after `C`; it becomes 2 at 2030-01-16 04:00 +09:00.
- Only the calendar date of `crt` counts, not its time of day: a `crt` of 2030-01-14 23:30 +01:00 with `creationOffset` 1439 has `C` = 2030-01-13.

### D3.2 Hours 0 to 23

With hour 0 the day always has rolled over (`R` is the start of the date), so today is the plain date difference and `next_day_at` is the next local midnight. With hour 23 a new day starts at 23:00. The matrix scenarios (D14.1) check every hour in eleven zones.

### D3.3 Days when the zone changes its offset

The computation applies the current offset to the whole date, so:

- A 23-hour or 25-hour day counts as one day (R592). Europe/Paris, rollover 4: day 10 starts 2030-03-30 04:00 +01:00, day 11 at 2030-03-31 04:00 +02:00 (23 hours later), day 220 at 2030-10-26 04:00 +02:00, day 221 at 2030-10-27 04:00 +01:00 (25 hours later). Australia/Lord_Howe moves by 30 minutes, giving 24.5-hour and 23.5-hour days.
- A computation made before the change gives a boundary under the old offset. Paris, rollover 4, computed on 2030-03-30 at noon: `next_day_at` is 2030-03-31 04:00 +01:00, which is 05:00 local after the change. A computation made after the change gives 04:00 +02:00. Which of the two applies at a given instant depends on the cache (D6).
- Hour inside a skipped hour: Paris, rollover 2, on 2030-03-31 local 02:00 does not exist. At 01:59:59 +01:00 `next_day_at` is 02:00 +01:00, the instant the clocks jump to 03:00 +02:00. At that instant the offset is +02:00, `R` (02:00 +02:00) is already past, so day 6 starts and `next_day_at` is 2030-04-01 02:00 +02:00.
- Hour inside a repeated hour: America/New_York, rollover 1, on 2030-11-03 local 01:00 happens twice. The day starts at the first 01:00 (-04:00); the next day starts at 2030-11-04 01:00 -05:00, 25 hours later.

## D4. Legacy timing (no creation offset)

When the collection has no creation offset (D2):

1. `A` = the boundary, at hour `h` under the **current** offset, of the local date of `crt` under the **current** offset.
2. Today = (`now` - `A`) / 86,400, rounded toward zero, as a signed number. A negative result is stored as an unsigned 32-bit number: -1 becomes 4294967295, -2 becomes 4294967294. This happens only when `now` is a full day or more before `A`; between one day before `A` and `A` today is 0.
3. `next_day_at` = the boundary of the local date of `now` at hour `h` under the current offset; when that instant is before `now`, add 86,400. So during the second that starts a day, `next_day_at` equals `now`; one second later it is the next day's boundary.

Examples, Europe/Paris in January, `crt` 2030-01-16 02:00 +01:00 (1894755600), so `A` is 2030-01-16 at hour `h` (+01:00):

| now (local) | h | today | next_day_at (local) |
|---|---|---|---|
| 2030-01-14 03:00:00 | 4 | 4294967294 | 2030-01-14 04:00:00 |
| 2030-01-15 05:00:00 | 4 | 0 | 2030-01-16 04:00:00 |
| 2030-01-16 04:00:00 | 4 | 0 | 2030-01-16 04:00:00 (= now) |
| 2030-01-16 04:00:01 | 4 | 0 | 2030-01-17 04:00:00 |
| 2030-01-20 04:00:00 | 4 | 4 | 2030-01-20 04:00:00 (= now) |
| 2030-01-22 12:00:00 | 12 | 6 | 2030-01-22 12:00:00 (= now) |
| 2030-01-25 00:00:00 | 0 | 9 | 2030-01-25 00:00:00 (= now) |

- The two timings differ when the offset at `crt` and the current offset put `crt` on different dates. `crt` 2030-01-14 23:30 +01:00 seen on 2030-06-01 10:00 +02:00: legacy timing gives 137 (`crt` is 2030-01-15 00:30 under +02:00), timing with `creationOffset` -60 gives 138.
- Meguri keeps a collection's timing mode as it finds it (R595): it never adds, removes or changes `creationOffset` (D12.3).

## D5. The day query

D5 and D7 describe Anki's observed day query and Meguri's explicit compatibility query, including the conformance `timing` observation. Meguri's automatic engine checks use D8's no-churn policy instead; the day math is identical.

A compatibility day query returns today and `next_day_at` for `now`:

1. When the cached result is still valid (D6), it returns that result and skips step 2.
2. Otherwise it computes afresh:
   - When `rollover` is missing or unreadable (D2), it writes `rollover` = `4`.
   - When the current offset differs from `localOffset`'s effective value (D2), it writes `localOffset` = the current offset in minutes west (offsets are whole minutes). A missing or unreadable `localOffset` reads as 0, so on a device at UTC a missing key stays missing.
   - It applies D3 or D4 and caches the result.
3. In both cases it then runs the unbury check (D7).

Writes made by a day query:

- Each write follows `storage/config-table#C4` (nothing is written when the key already holds the value) and stamps the row: `usn` -1, `mtime_secs` = `now`.
- They do not move `col.mod`, are not recorded for undo, and do not clear the undo history. Anki makes them outside any operation. A pending row is still sent by the next sync, but on its own it does not make a sync necessary.
- A new collection holds neither `rollover` nor `localOffset` (D11). Its first day query writes `rollover` 4, and `localOffset` unless the device is at UTC (R596).

## D6. Reusing a computed day

A computed result is valid while `now` is before its `next_day_at`. In that time a query returns the cached day and boundary unchanged. This also holds when the clock moves back: Paris, rollover 4, computed on 2030-01-20 at 10:00 (day 5): at 2030-01-18 10:00 the query still answers day 5, until 2030-01-21 04:00. After a reopen it answers day 3.

Meguri also drops the cached result, and computes afresh at the next query, when any of these differs from the values it was computed with:

- `crt`;
- the creation offset, or whether there is one;
- the effective rollover hour;
- the current offset.

It also drops it when the collection is opened and after a raw write (`db_exec` in scenarios, `specs/conformance/record-ops.md` W1).

Anki drops its cached result when the collection is opened, when its own settings code writes `rollover` or `creationOffset`, when a sync changes `crt`, and when the scheduler version changes. It also drops it whenever a path other than the cached query writes `localOffset`, or repairs a missing or unreadable `rollover` (D5 step 2's writes). The fresh computations of D6.1 make both writes; the statistics graphs make only the `localOffset` write, after reading the cached day. Each of them clears the cached day exactly when it makes one of these writes: when `rollover` is missing or unreadable, or when `localOffset`'s effective value differs from the current offset (after the device's offset changed, or after `localOffset` was written by another path). After an offset change, the first of them to run writes `localOffset` and so clears the cached day; the next query then computes afresh under the new offset, before the old boundary, and caches that day. Otherwise these paths write nothing and leave the cached day alone. So a write of `rollover` or `creationOffset` by another path (sync, add-on, the generic config API) leaves the cached day in place until the boundary passes, unless a fresh path repairs `rollover` or writes `localOffset` first; a change of offset leaves it in place until the first such path runs or the boundary passes. D12.1 and D12.2 list what this changes. (2026-11-17: the drop on a `localOffset` write was added. Meguri's day clock is unchanged.)

### D6.1 Where Anki does not use the cached day

Not every Anki path reads the cached result. These compute the day afresh for the instant of the call (with D5's writes, under the current offset and the stored `rollover`), and so ignore D6:

- the deck list's due counts (its unbury check still uses the cached day);
- the day a study queue is built for;
- the interval labels on the answer buttons (seconds left until `next_day_at`).

Answers, searches that compare due days, statistics, the browser's due column and the `timing` observation use the cached day. The statistics graphs read the cached day for their own day and then the current offset, which writes `localOffset` when it changed. Each fresh path above writes `localOffset` the same way, and that write drops the cached day (D6). The deck list runs its unbury check on the cached day before its fresh computation, so its check still sees the old day; later queries see the new one. The two agree except, before the cached boundary, after an offset change when none of these paths has run since, after a `rollover` or `creationOffset` write outside Anki's settings code, or after a clock that moved back. Meguri has one day everywhere, the day clock's (D8), which in the first two cases equals Anki's fresh computation (D12.1, D12.2) and after a clock moved back equals Anki's cached day. Units that compare deck counts or queues at such instants (U41, U43) decide their scenarios with this in mind.

## D7. Unburying at the start of a day

The unbury check runs at the end of every day query (D5 step 3).

- It runs the unbury when `lastUnburied` (D2) is below today, or above today + 7. Otherwise it does nothing. With today 10: `lastUnburied` 9 or 18 unburies; 10 and 17 do not; a string reads as 0 and unburies.
- The unbury gives every card whose `queue` is -2 (buried by the scheduler) or -3 (buried by the user) the queue its `type` and `due` call for, in every deck, filtered decks included:

  | `type` | `due` | new `queue` |
  |---|---|---|
  | 0 (new) | any | 0 |
  | 2 (review) | any | 2 |
  | 1 (learning) or 3 (relearning) | above 1,000,000,000 (a timestamp) | 1 |
  | 1 or 3 | 1,000,000,000 or less (a day number) | 3 |

- Only `queue` changes. The card keeps its `mod` and `usn` (R599), so each device unburies its own copy and no card change syncs. No other card changes: suspended cards (queue -1) stay suspended.
- Then it writes `lastUnburied` = today, also when no card was buried. The write follows D5's rules: row stamped, `col.mod` unchanged.
- The unbury is not recorded for undo and does not clear the undo history by itself (D9 clears it when the day changes).
- Because the check runs at every query, the first day query on a new day unburies, whether or not this session saw the change: a collection opened on a later day is unburied at its first query (D8), before anything reads counts.
- `lastUnburied` is synced with the other config keys. A device that receives another device's `lastUnburied` equal to today before its own first day query of the day finds nothing to do and keeps its own cards buried until the next day. Both sync paths can do this. Anki's incremental sync replaces the receiver's whole config (`specs/conformance/sync-ops.md` S7.3). CloudKit sync replaces a clean local `lastUnburied` with the incoming value, and when both sides changed it keeps the larger value (`sync/cloudkit-records#K15.8`). So the unbury runs before incoming data is applied. Anki's client runs a day query with the unbury check when its status check calls for an incremental sync, before the exchange (not when nothing changed and not before a one-way sync), and Anki's sync server does the same on its own collection when a sync starts. Meguri runs `checkRollover()` and lets its op commit before every CloudKit apply (a `CloudIncoming` op that applies inbox entries, `sync/cloudkit-records#K13.2`; the barriers of `engine/op-model#O1.1` write nothing and need no check) and before a manual Anki transfer applies incoming data (D8). (2026-10-27: the CloudKit case was added; the day clock's own behavior is unchanged.)

Example (AE24): Paris, rollover 4, cards buried on day 5 (`lastUnburied` 5). At 2030-01-21 03:59:59 today is 5 and nothing changes. At 04:00:00 today is 6: the cards return to their queues as in the table, `lastUnburied` becomes 6, and `col.mod` keeps its value.

## D8. Meguri's day clock

The engine keeps one day clock per open collection: the cached result (D6), the last day, `next_day_at` and IANA zone id it reported, and an effective unbury-check day with the timing inputs and stored `lastUnburied` value on which it depends. The effective check day is device-local memory, never a new synced key or sidecar. It exposes:

- `checkRollover()`: runs the automatic day check and reports what changed (below). Idempotent and cheap: a second call in the same state writes nothing and emits nothing.
- `today` and `nextDayAt`: the values of a day query run at the time of the read. Every core path that needs the day gets it through these or `checkRollover()`. A read that finds no valid cached result (D6) runs `checkRollover()` first, so its writes, unbury and events follow the steps below.
- Code running inside an op on the writer thread never runs the day query itself: it would have to submit the `Rollover` op and wait for the writer it occupies. Its caller runs `checkRollover()` before submitting the op and passes the day in (U43 does so before every answer).

Each `checkRollover()`:

1. Computes the day (D5, D6) and checks unburying (D7), with the automatic policy below. When today differs from the last day seen (either way), its writes commit in the rollover op (D9); otherwise, when it wrote anything, in a plain op with source `Rollover` (D10).
2. When the clock's zone id differs from the last one seen, emits `TimeZoneChanged{zoneId}` on the change stream, even when the day and the offset did not change (Europe/Paris to Africa/Ceuta).
3. When today differs from the last day seen, emits `RolledOver{oldDay, newDay}` once the rollover op has committed. A day change caused by an offset change (travel, DST) takes the same path (R464).
4. When `next_day_at` differs from the last one seen, emits `NextDayAtChanged{nextDayAt}`.
5. Records the reported values.

Automatic checks preserve data when no semantic maintenance is needed (R1590, AE66):

- A missing or unreadable `rollover` supplies the effective default 4 without writing or repairing the config row. Only an explicit compatibility query performs D5's repair.
- A real change of UTC offset still writes `localOffset` exactly as D5 says (R594). Automatic checks must not suppress a required offset change to pass a preservation test.
- Start with the persisted `lastUnburied` day, interpreted by D2. While that value and the relevant timing inputs remain unchanged, use the effective checked day to avoid repeating an already completed unbury check. Apply D7's condition and restore eligible buried cards with only their queue changed. When at least one card is restored, record today in synced `lastUnburied` as R598 requires, preserving card mtime/usn and `col.mod`. When none is restored, write no `lastUnburied` row; remember the completed check day only in memory. A changed persisted `lastUnburied`, generation replacement or external raw mutation invalidates the corresponding remembered basis, so it cannot hide incoming state.
- Real bury operations, whether a user action, sibling burying or a generic queue-write API, establish today's `lastUnburied` marker in the same user/scheduler transaction before storing buried queues. This prevents both another check and a same-day reopen from restoring newly buried cards when an earlier empty automatic check left no synced marker. U43/U44 and every later bury entry point must preserve this rule; an untracked raw fixture write does not stand in for a valid bury operation.
- No other config write is permitted by this policy. Required offset changes and actual unburying have their specific R594/R598 assertions; empty launch or idle bookkeeping is not an exception to N12's unchanged raw dump and clean pending-change query.

The first `checkRollover()` after the collection opens records the values and emits nothing; it still restores eligible buried cards before anything reads counts, using this automatic policy. It is an open-time step of the lifecycle (U24): it runs before the collection is `Ready`.

A generation change (`engine/lifecycle` L8: Replace local collection from iCloud, `sync/cloudkit-recovery#V14`; a manual Anki one-way import; restore; `.colpkg` import) counts as an open: the day clock drops its cached result, effective unbury-check state and the values it reported, and the first `checkRollover()` on the new generation records, emits nothing and applies the automatic policy. It runs before anything reads counts on the new generation, as at open, so a downloaded collection's cards buried on an earlier day are restored first. The app reads `today` and `nextDayAt` again on `CollectionReplaced`, as after an open. `engine/lifecycle` L8 runs no open steps today; U40 adds this step there or registers the day clock as a `GenerationListener` whose check runs once the state is `Ready`.

Callers: the open-time step; the app on foreground, on its timer at `next_day_at` and on a system time, zone or clock change (U51, `sched/app-coordination` A1 and A2); the CloudKit incoming apply, whose `Rollover` op commits before each `CloudIncoming` apply is submitted (`sync/policy#P4` `ApplyReady`, `sync/cloudkit-records#K13.2`, D7; U113, U114); U113 once after every Sync Now exchange, whatever its result (`succeeded`, `incomplete`, `waiting`, `failed` or `cancelled`, `sync/policy#P12.6`, an exchange that stops early included; R597, `sync/policy#P12.7`, decided 2026-10-27; the check is idempotent); the study session before every answer (U43). A change set that touches `rollover`, `creationOffset` or `crt`, from any source (`CloudIncoming` included), makes the day clock compute afresh and report at once; this covers R597's "after a sync" for automatic CloudKit applies. Manual Anki transfers (U107, frozen, KTD26) call it before they apply incoming data and after they end.

The core runs no timer (R1854). In legacy timing `next_day_at` equals `now` for the whole first second of a day (D4), so a host timer armed at `next_day_at` must wait for an instant after `now`, at least the next whole second, or it fires in a loop during that second. The app's timer also fires at the zone's next UTC offset change when that comes first, because the boundary under the new offset can come earlier (D12.1, `sched/app-coordination#A2`).

### D8.1 Searches read the day without a day query

Changed 2026-11-21 (implementation review): new section, an exception to D8's second bullet ("Every core path that needs the day gets it through these or `checkRollover()`"). A search, sorted or not (`search/query-model` Q8, `browser/result-sets` H9), a resort, and the Browse rows read for a result (`browser/columns` C1) take today and `next_day_at` from the day calculation alone: D3, or D4 for a legacy collection, for the request's now, from the stored `crt` and creation offset, `rollover` read as D8's automatic checks read it (the effective default 4 when missing or unreadable) and the device's current offset. The calculation writes nothing: no D5 step 2 write, no unbury check (D7), no cached result, no `Rollover` op and no event. It does not read the day clock's cached result, so after the clock moves back before a cached boundary (D6) a search uses the earlier day. The conformance executor does the same for `search` and `browser_rows` (`conformance/browser-ops` B1, B3).

The day query's writes and its unbury belong to `checkRollover()`'s callers above and to the reads that start study (a queue build, an answer) or read deck counts. A search is none of these, so a search that runs after a day boundary before any of them still finds the cards buried on the earlier day buried. How Anki's search differs is `search/query-model` Q8.

## D9. The rollover op

When `checkRollover()` sees the day change, its writes under D8's automatic policy (D5, D7: the unbury of the new day among them) and these effects form one op with source `Rollover`:

1. Clear undo and redo (U25) with reason `ClearedByRollover` (R598, R462).
2. Mark the study queues stale; the session rebuilds them as R461 says (U43).
3. Leave the per-deck studied counters alone; they reset lazily when read for the new day (U42).

- The op's writes are those of D5 and D7: they do not move `col.mod` and are not recorded for undo. The undo clear of step 1 is the only effect on the undo history.
- The op appears on the change stream with the cards and config keys it changed, so counts refresh.
- During an exclusive phase (one-way sync) the op is refused or parked like any other op (U24). The day clock runs it again once the lifecycle is `Ready`.

## D10. Writes of the day clock

Writes of the explicit compatibility query (D5, D7) and the narrower automatic policy (D8) commit through ops with source `Rollover`, also when no day change is seen (a first query after an open, a new offset, a `lastUnburied` more than 7 days ahead). That source neither records undo entries nor clears the undo history; only D9 step 1 clears it. So a new `localOffset` (offset change without a day change) or a `rollover` restored by an explicit query never empties the back button (coverage note sched #7).

## D11. New collections (R596)

The bootstrap of a new collection (U28) takes `crt` and the creation offset from a pure function of the creation instant `t` and the clock:

1. `o` = the offset in effect at `t`.
2. `F` = the boundary of the local date of `t` (under `o`) at hour 4, under `o`. The hour is always 4; the function does not read `rollover`.
3. `crt` = `F` when `F` is at or before `t`, else `F` - 86,400.
4. Creation offset = the offset in effect **at the instant `crt`**, in minutes west. It differs from `o` when the zone changed its offset between `crt` and `t`.

The collection stores `crt` in the `col` row and `creationOffset` among the config keys a new collection starts with (`usn` 0, `mtime_secs` 0, like those keys). It has no `rollover`, `localOffset` or `lastUnburied` key. An explicit compatibility query writes them as D5/D7 say; an automatic engine check follows D8 and can use rollover 4 without storing the key.

| Zone | Created at (local) | `crt` | `creationOffset` |
|---|---|---|---|
| Europe/Paris | 2030-01-15 03:30 +01:00 | 2030-01-14 04:00 +01:00 (1894590000) | -60 |
| UTC | 2030-01-15 04:00:00 | the same instant (1894680000) | 0 |
| Asia/Tokyo | 2030-01-15 10:00 +09:00 | 2030-01-15 04:00 +09:00 (1894647600) | -540 |
| Europe/Paris | 2030-03-31 03:30 +02:00 (after the clocks moved forward) | 2030-03-30 04:00 +02:00 (1901066400), an instant at which Paris was at +01:00 (03:00 local) | -60 |
| Europe/Paris | 2030-10-27 03:30 +01:00 (after the clocks moved back) | 2030-10-26 04:00 +01:00 (1919214000), an instant at which Paris was at +02:00 (05:00 local) | -120 |

## D12. Differences from Anki

### D12.1 The offset changes (R464, R597)

Meguri computes the day afresh at the first query after the current offset changes (D6). Anki keeps a day computed under the old offset until its boundary passes or until a path of D6.1 or a statistics graphs request runs, whichever comes first. The D6.1 paths' own day equals Meguri's (a graphs request still uses the old day itself), and once one of them or a graphs request has run, Anki's cached day equals Meguri's too (D6). The difference therefore holds only while no such path has run since the offset change. Paris, rollover 4, day computed on 2030-03-30 at noon, only `timing` observations afterwards: on 2030-03-31 at 03:59:59 +02:00 Meguri's `next_day_at` is 04:00 +02:00 and Anki's 04:00 +01:00; at 04:00:00 +02:00 Meguri starts day 2 and Anki stays on day 1 until 05:00 +02:00. Deviations `day-clock-offset-change-*`, scenario `sched/day/offset-change-recompute`. After travel east past the rollover hour, Meguri starts the new day at the first query; Anki waits until the old boundary passes, or until the deck list, a queue build, the answer-button labels or the statistics graphs run.

### D12.2 Timing keys change by sync or config write (R597, R760)

Meguri computes the day afresh as soon as `rollover`, `creationOffset` or `crt` change, from any source. Anki does this only when its settings code makes the change. Paris, `rollover` 4 changed to 12 at 2030-01-20 10:00 by a config write: Meguri goes back to day 4 until 12:00; Anki keeps day 5 and its 04:00 boundary until the next morning. Deviations `day-clock-config-change-*`, scenario `sched/day/rollover-change-recompute`. A change made in Anki's own settings screen behaves as in Meguri.

### D12.3 Legacy collections stay legacy (R595)

Anki's desktop layer adds `creationOffset` (the offset in effect at `crt`) whenever it opens a collection that lacks it, which changes the day count when the two timings differ (D4). Meguri never does. A `creationOffset` that arrives by sync (another device added it) is collection data: the day clock computes afresh with it at once (D6, D8), and the collection uses timing with a creation offset from then on. A sync that leaves the key absent leaves the timing legacy. Deviations `legacy-timing-kept-on-reopen-*`, scenario `sched/day/legacy-timing-kept-on-reopen`.

### D12.4 Not reproduced, not observable through the oracle

- Anki also restores buried cards, without the `lastUnburied` condition, when a study queue built on an earlier day is used again. Meguri applies only D7's condition (R598). The two differ only when the day changes while `lastUnburied` lies between today and today + 7, for example after the clock moved back across a boundary.
- Anki clears the undo history at a day change only when a study queue built on the previous day is used again; Meguri clears it at every day change (D9, R598, AE24).
- Anki computes `creationOffset` and `localOffset` times 60 in 32-bit arithmetic; values above 35,791,394 in magnitude overflow there. Meguri treats every value outside -1439 to 1439 as unusable (D2).
- Anki's sync server reads `localOffset` instead of a device clock. That is not part of the client day clock.

## D13. Reading `rollover` and `collapseTime` elsewhere

Other areas read the effective rollover hour and the learn-ahead limit through D2's rules and never write them on read. Saving the settings screen (U49, U160) writes `rollover` as an integer from 0 to 23 and `collapseTime` in seconds; the day clock then computes afresh (D6, D8).

## D14. Scenarios

### D14.1 Coverage

| Scenarios | Covers |
|---|---|
| `sched/day/matrix/<zone>` (generated by `conformance/corpora/sched/generate.py`) | every hour 0 to 23 in each R1899 zone, one second before, at and after the boundary (D3, R1898, R1899) |
| `sched/day/ae84-kathmandu` | AE84's day numbers (its deck counts wait, D14.3) |
| `sched/day/dst-europe-paris`, `dst-america-new-york`, `dst-america-los-angeles`, `dst-australia-lord-howe` | both offset changes of 2030, 23-, 25-, 23.5- and 24.5-hour days, `localOffset` written once per change (D3.3, D5) |
| `sched/day/dst-gap-rollover-2-europe-paris`, `dst-overlap-rollover-1-america-new-york` | a rollover hour inside a skipped and a repeated hour (D3.3) |
| `sched/day/creation-*` | D11, the day-0 clamp before `crt`, no `localOffset` at UTC (D5) |
| `sched/day/created-tokyo-studied-los-angeles`, `created-los-angeles-studied-tokyo` | R1900 |
| `sched/day/legacy-timing`, `legacy-timing-after-answer` | D4 |
| `sched/day/config-values` | D2's readings and repairs |
| `sched/day/clock-back` | D6 when the clock moves back |
| `sched/day/offset-change-recompute`, `rollover-change-recompute`, `legacy-timing-kept-on-reopen` | D12.1 to D12.3, with deviations |
| `sched/unbury-on-rollover/both-kinds` | AE24, D7's table, `mod`, `usn` and `col.mod` unchanged |
| `sched/unbury-on-rollover/open-next-day` | D7 at the first query after an open |
| `sched/unbury-on-rollover/last-unburied-window` | D7's condition |

All have goldens. The unbury scenarios write their notes and cards with `db_exec`, so they need neither note setup nor the `card` observation.

### D14.2 Where the day query runs in scenarios

- The `timing` observation (`specs/conformance/observations.md` O4) is one explicit compatibility day query (D5, D7) on each side. It can write the timing keys and restore cards, including the observed empty-unbury marker write. It is distinct from the automatic engine policy; its existing goldens stay unchanged.
- The real engine lifecycle runs D8's automatic check at open (scenario setup and handoffs may use that lifecycle). Meguri's scenario adapters also run explicit compatibility queries before an answer and right after a `set_config` of `rollover` or `creationOffset`; these queries preserve the timing scenarios' observation contract. The protocol's storage-level `open` step (`specs/conformance/storage-ops.md` P1) and `reopen: {}` run none: `reopen` closes and opens the file, discards cached timing, and leaves the first day query to a later `timing` observation or scheduler operation. The oracle runs a day query without the unbury check inside `answer` and `next_states`, and none when it opens a collection or writes config. `card`, `db_exec`, `set_clock` and `advance` run none on either side; a `db_exec` that writes drops Meguri's cached day (D6).
- In actor scenarios (`specs/conformance/sync-ops.md`), an oracle actor's `sync: {kind: normal}` runs a day query with the unbury check only when the status check calls for an incremental sync; Meguri actors run `checkRollover()` before every sync (D7, D8). The scenario's sync server, Anki's own (`sync-ops.md` S3.1), runs one on its collection when a sync starts. A one-way sync reopens the actor's collection, which adds `creationOffset` to a legacy collection on the oracle (D12.3).
- A timing scenario that expects day-query writes or unburying aligns both sides as follows (raw storage preservation scenarios intentionally request no timing):
  - starts with `observe: [timing]`;
  - observes `timing` right after every `reopen` and handoff, before the clock moves;
  - after moving the clock across a boundary, observes `timing` before any `answer`, `sync` or read.
- The oracle reuses a computed day (D6). To compare a day after an offset change or a `set_config` of `rollover` or `creationOffset`, a scenario either reopens the collection at that instant and observes `timing` (not for legacy collections: the oracle's open adds `creationOffset`, D12.3), or observes only after the clock has passed the oracle's cached `next_day_at`. In the second case the config change must not start a new day at the instant it is written, or Meguri unburies and writes `lastUnburied` at that earlier instant. After an offset change only, a third way also aligns the two sides: a step that reaches one of the oracle's fresh paths (D6.1) after the change, which writes `localOffset` and drops the oracle's cached day (D6). These steps are `deck_counts`, `answer_labels`, a `queue` observation that builds the queue (`specs/conformance/queue-ops.md` Q2.3), and `stats`, `today_stats` and `stats_presentation`, which request the statistics graphs. Later `timing` observations, answers and searches then use the new offset on both sides. The `deck_tree` observation computes no day and does not do this. A scenario that runs one of these steps after an offset change drops the oracle's cached day whether it means to or not, and needs no `day-clock-offset-change-*` deviation for the observations that follow (a `deck_counts` step's own unbury check still runs on the old day, D6.1, so the step itself can still differ when cards are buried). Otherwise the scenario needs a deviation.

### D14.3 Waiting for other areas

- Deck counts at the rollover instants (AE84's counts, R1897) need the queue area's `deck_tree` observation (U41).
- `sched/day/legacy-timing-after-answer` needs note setup (U27) and answers (U44).
- Meguri's executor needs the `timing` observation and the `reopen` op (U40 implements both adapters).
- The sync half of U40's legacy-timing test ("keeps its timing mode after a sync") has no scenario: actor scenarios are live, Meguri actors need the sync units (U106, U107), and an actor's first sync of a collection that holds cards is a one-way sync, which reopens the collection, after which the oracle's collection is no longer legacy (D12.3). U40 checks it with engine tests: a `SyncIncoming` op whose change set leaves `creationOffset` absent keeps legacy timing (today unchanged, no `creationOffset` row), and one that adds it switches the count at once (D12.3).
- Meguri-only behavior has no scenario and is checked by U40's engine tests: D8's events (`TimeZoneChanged` for Europe/Paris to Africa/Ceuta without `RolledOver`; one event for two calls on the same day; one recompute and at most one event for a `SyncIncoming` change of `rollover` from 4 to 6), D9 (undo cleared with `ClearedByRollover`, a refused or parked `Rollover` op run once the state is `Ready`), D8's generation change, and the absence of core timers (R1854).
- CloudKit callers (U113, U114) add these tests when they land: `ApplyReady` runs `checkRollover()` and its op commits before the `CloudIncoming` apply; a `CloudIncoming` op that brings another device's `lastUnburied` equal to today, on a device whose day has changed and which has buried cards, finds them already restored; a `CloudIncoming` change of `rollover` from 4 to 6 gives one recompute and at most one event; every Sync Now exchange runs `checkRollover()` once, whatever its result: one that succeeds, one that fails after its fetch applied entries, and one cancelled before its capture. U40's day clock needs no change for them.
- Engine day-conformance tests replay explicit compatibility queries through the real writer and assert that the automatic day clock reports the same day and next boundary. They must not make a compatibility replay stand in for actual automatic lifecycle verification. Preservation and rollover tests exercise that automatic policy directly: unchanged raw dumps and clean pending queries through 100 launch/idle cycles across days, correct day events without empty marker writes, actual next-day unbury of both kinds with stamps preserved, same-day bury/reopen staying buried, and an offset change writing only the required `localOffset` when nothing needs unburying. No new dump masks or golden deviations are introduced for this distinction.

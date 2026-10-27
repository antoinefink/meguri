# Widget snapshot

Spec: stats/widget-snapshot
Requirements: R149, R312, R315, R316, R317, R318, R1274, R1309, R1431, R1433, R1434, R1435, R1436, R1438, R1453, R1840
Anki: none (Meguri format, no Anki behaviour; its values follow `stats/heatmap-addon` and `stats/studied-today`, which carry the pins)
Scenarios: conformance/scenarios/stats/widget/

The file the heatmap and streak widget reads (KTD16): what it holds, when the core rebuilds and rewrites it, and what it must never hold. It is Meguri's own format, with no Anki or add-on behaviour: the core writes it (U100) and the widget extension decodes it (U165). Every value in it comes from the Home heatmap rules of `stats/heatmap-addon` and the studied-today values of `stats/studied-today`, so the widget runs no date, streak or level logic of its own (R1435).

## W1. File

- One file, `widget-snapshot.json`, in the App Group container (KTD12). Swift passes its path when the collection opens (U37). The core never looks up App Group containers and writes no other file there, apart from W6's temporary file.
- UTF-8 JSON in the compact form of `specs/storage/config-table.md` C5: no whitespace, object members sorted by name. So the same content always gives the same bytes.
- Integers only, no floats. Instants are epoch milliseconds. Dates are `YYYY-MM-DD` heatmap days (`stats/day-buckets` B4.5).
- With no collection open, the core writes nothing. Without the file the widget shows its empty state, "Open Meguri to get started" (R1438).

## W2. Schema version

- `schema` is 1.
- Readers ignore members they do not know. Adding a member keeps the version. Removing a member, or changing a member's type or meaning, raises it.
- A reader that meets a file it cannot decode, or a `schema` other than the ones it knows, treats the file as missing (U165).

## W3. Members

The snapshot is for the Home scope: the whole collection under the `heatmap` settings, `limdecks`, `limcdel`, `limresched`, `limhist` and `limdate` included (`stats/heatmap-addon` A2, A3; R1431).

| Member | Type | Value |
|---|---|---|
| `schema` | integer | 1 (W2) |
| `state` | string | `"ready"`, or `"no-history"` when the Home scope has no history (`stats/heatmap-addon` A2.2) |
| `refreshedAt` | integer | the instant the snapshot was built |
| `zone` | string | the IANA zone id the values were computed in (`stats/day-buckets` B1) |
| `rollover` | integer | the rollover hour `R`, 0 to 23 |
| `weekStart` | integer | the week start, the value of `firstDayOfWeek` after `specs/editor/config-keys.md` K5: 0, 1, 5 or 6 (R149) |
| `today` | string | T, the heatmap's today at `refreshedAt` |
| `nextRolloverAt` | integer | at(T + 1) by W4's rule, which is also `upcoming[0].at` |
| `history` | object | `start` (a date), `counts` and `levels` (arrays of integers of equal length): the Home history count and level (`stats/heatmap-addon` A2.1, A7) of each date from `start` to `today` in order, `today` included; 0 for a date outside the history window |
| `forecast` | object | `levels`: an array of integers, the forecast level (A7) of each date from `today` on, index 0 being `today`; trailing zeros left out, and a date past the array's end has level 0 |
| `todayCell` | object | `kind` (`"today"` or `"future"`), `count` and `level`: today's cell (A4). `count` is the review count, so 0 when `kind` is `"future"` |
| `streaks` | object | `dailyAverage`, `daysLearned`, `longestStreak`, `currentStreak`, each `{"level": …, "value": …}` (A8) |
| `studiedToday` | object | `cards`, `timeMs` and `text`: `stats/counting-rules` N6's two numbers and the Home line of `stats/studied-today` T2, as the Home screen shows it |
| `upcoming` | array | 7 objects (W4) |

- **Ranges.** `history.start` is the earlier of January 1 of T's year and the first day of the week (per `weekStart`) 19 weeks before the week that contains T, so the year grids and the 20-week grid of U165's widget families can be drawn. The forecast covers dates up to December 31 of the year of `upcoming[6].today + 6`, so a horizon that crosses the year end has the next year's forecast too.
- **`no-history`.** The snapshot holds `schema`, `state`, `refreshedAt`, `zone`, `rollover`, `weekStart`, `today`, `nextRolloverAt`, `studiedToday` and `upcoming` only, each `upcoming` entry with `at`, `today` and `studiedToday` only. The widget shows R1309's "No reviews yet". `studiedToday` can be non-zero here, since N6 ignores the `heatmap` settings, and the entries reset it at each rollover. Decided 2026-10-27: the upcoming entries stay so that the studied-today values never outlive their day.
- **Forecast as levels only.** The forecast appears only as cell levels: no due count, no count of cards due today (R312, R318).

## W4. Upcoming states

`upcoming` holds the states at the next 7 heatmap rollovers, so the widget's timeline can move to each of them without the app (R316, R1435).

- **Rollover instant.** The heatmap's today becomes date D at `at(D)` = the first instant of local date D in the zone, plus `R` × 3,600 seconds of elapsed time. The first instant is local midnight, or the first instant after a gap when midnight falls in one. This is where `now − R` hours reaches D's start (`stats/day-buckets` B4.2), with the offset at the moved instant.
- `upcoming[i]` (i = 0 to 6) is for D = T + 1 + i: `at` = `at(D)`, `today` = D.
- Each entry holds `at`, `today`, `todayCell`, `streaks` and `studiedToday`, computed with the collection as it is now. Decided 2026-10-27: every entry keeps the build's history window, filtered history and level scale `a` (`stats/heatmap-addon` A2.2, A7). A `limhist` window does not slide with the entries, and the entries share the snapshot's `state`, so no entry is `no-history` in a `ready` snapshot. The next build, at the latest at `nextRolloverAt` while the app is open (W5), moves the window. An entry's values:
  - `todayCell`: D has history count 0 (rows after the build's T count on T, `stats/day-buckets` B4.3). With `forecast.levels[i + 1]` at least 1, `kind` is `"future"`, `count` 0 and `level` that value; otherwise `kind` `"today"`, `count` 0, `level` 0. So the cell keeps the level the widget drew for D the day before.
  - `streaks`: A8 over the build's filtered history with T = D. Daily average and Longest streak stay as built, Days learned spans the longer range, and the current streak follows R1330 (AE51: 364 now, 0 at the entry of 2026-10-13 04:00, longest still 364). The levels follow the values (A8.2).
  - `studiedToday`: N6's two numbers and T2's line at the instant e(D) below, usually 0 with the zero line.
- **Studied-today instant.** e(D) is the later of at(D) and the boundary of D at hour `R` under the UTC offset in effect at at(D) (`sched/day-clock` D1). Anki's day D starts there, so an entry never holds the previous Anki day's values. The window uses Anki's today and `next_day_at` at e(D): `sched/day-clock` D3, or D4 in legacy timing, evaluated with `now` = e(D), the zone's UTC offset at e(D) as the current offset, the current `crt`, the creation offset and the effective rollover hour (D2). Decided 2026-10-27: U100 evaluates D3 or D4 itself, as a pure computation in `meguri-stats`, or calls U40's day math where U40 exposes it as one. It never reads the day clock's `today` or `nextDayAt` (D8), uses no cached result (D6), reads no `localOffset`, and writes and emits nothing. Europe/Paris, `R` 4, T 2026-10-24: at(2026-10-25) is 02:00Z, but under +01:00 Anki's day starts at 04:00 local (03:00Z), so e is 03:00Z. Between at(D) and e(D) the widget shows D's studied-today values while Home still shows the day before's, and when Anki's day starts before at(D) (a spring-forward night) the widget keeps the old values until at(D). Each gap is at most the offset change. The timeline moves only at at(D).
- History before an entry's `today` is the snapshot's `history` (a date after the snapshot's `today` with no history has count 0). Dates after it use `forecast`. Its `today` cell is its own `todayCell`. The far end of a `limfcst` window does not move with the entries.

| Zone, `R` | T | `upcoming[0].at` |
|---|---|---|
| Europe/Paris, 4 | 2026-10-12 | 2026-10-13 04:00 +02:00 (2026-10-13T02:00:00Z) |
| Europe/Paris, 4 | 2026-03-28 | 2026-03-29 05:00 +02:00 (03:00Z): midnight is +01:00, four hours later the clock reads 05:00 |
| Europe/Paris, 2 | 2026-03-28 | 2026-03-29 03:00 +02:00 (01:00Z): 02:00 local does not exist that night |
| Europe/Paris, 4 | 2026-10-24 | 2026-10-25 03:00 +01:00 (02:00Z) |

In a zone whose local date can step back (a fall-back across midnight, which some zones have), the same rule holds: at(D) is still the first instant of local date D plus `R` hours of elapsed time, and the entries stay in date order. After at(D), T can step back to D − 1 for up to one hour (B4.1 at the repeated instants); the timeline does not show that step and keeps D's entry. No zone in the test matrix has such a change.

## W5. When the snapshot is rebuilt

- **Changes.** The core rebuilds after any committed change that can alter a member: review-log rows added or removed, a card's existence, `did`, `odid`, `type`, `queue`, `due` or `odue` (`stats/heatmap-addon` A3), the config keys `heatmap`, `rollover` and `firstDayOfWeek`, deck renames, moves and deletions (they change `limdecks`' descendants), and the events `CollectionReplaced`, `RolledOver`, `NextDayAtChanged` and `TimeZoneChanged` (`sched/day-clock` D8; R1434). A changed collection file from another process counts as a change (R1453).
- **Debounce.** A rebuild starts after 2 seconds without such a change. Fifty answers within 10 seconds give one rebuild, about 2 seconds after the last.
- **Rollover.** While the collection is open, the core also rebuilds at `nextRolloverAt` (R1274), which can differ from Anki's `next_day_at` by up to an offset change (`stats/day-buckets` B4.4, B6). Decided 2026-10-27: the trigger is a one-shot deadline in U100's scheduler, the mechanism of the debounce: each build re-arms it at the new `nextRolloverAt`, and it is a `SuspendParticipant` that suspending stops, as it stops the debounce; the foreground `flush` rebuilds and re-arms it. When it fires and the injected clock is still before `nextRolloverAt` (the clock moved), it re-arms without a rebuild. It is a single deadline per day, not polling, so R1854 holds. Like the debounce, it is an exception to `sched/day-clock` D8's "the core runs no timer", which is about the day clock: the day clock still runs no timer, and its `RolledOver` and `NextDayAtChanged` events trigger a rebuild as above.
- **Flush.** `flush(reason, deadline)` rebuilds at once, skipping the debounce, and cancels a pending debounced rebuild. The app calls it at session end, on return to the foreground and before suspending (U151, U165). Suspending runs a pending rebuild within the deadline, then stops the timer.
- After `CollectionReplaced`, the next snapshot holds only the new collection's values.
- Building reads the core's resident values only (`stats/counting-rules` N7), never the review log, and takes at most 10 ms (U104).

## W6. When the file is written

1. Build the content. Compare it, without `refreshedAt`, to the last content written. On the first build after open, the last content is read back from the file; a missing or undecodable file counts as different.
2. Write when the content differs, or when the last written `refreshedAt` is more than 12 hours old (so the widget's one-day staleness line, R317 and R1436, stays truthful without reloads). Otherwise leave the file untouched, mtime included.
3. A write goes to `widget-snapshot.json.tmp` in the same directory, is synced to disk, then renamed over `widget-snapshot.json`. A crash before the rename leaves the previous file valid. A leftover temporary file is overwritten by the next write; readers never read it.
4. Report `{changed, refreshedOnly, writeSeq, failed}`: `changed` when the file was written with new content, `refreshedOnly` when it was written for step 2's age rule alone, `writeSeq` the number of the last successful write since the collection opened (1 for the first, 0 before any), `failed` when step 5 applies. `flush` returns the record after every build. The change stream gets it as `WidgetSnapshotWritten` only when `changed` or `refreshedOnly` is true; a build that wrote nothing, or failed, sends no event. The app asks WidgetKit to reload only when `changed` is true (KTD16).
5. Decided 2026-10-27: a write fails when creating or writing the temporary file, the sync or the rename fails (the directory is missing or not writable, the disk is full). The core then keeps the last written content, its `refreshedAt` and `writeSeq` as they were, since they change only after a successful rename. It removes the temporary file when it can, reports `failed` true with `changed` and `refreshedOnly` false, and logs the error. It adds no timer: the next build (a change, the rollover deadline, `flush`) compares with the unchanged last content and tries again. The core never creates the directory. With no path passed at open (U37), the core builds and writes nothing, and `flush` returns `changed`, `refreshedOnly` and `failed` false with `writeSeq` 0.

Two builds from the same state give the same bytes, `refreshedAt` aside.

## W7. Size

The file is at most 32,768 bytes (R1840). By W3's ranges it holds at most about 370 history dates and 740 forecast levels, about 10,000 bytes for a collection with 1.2 million rows. A test asserts the limit for that collection and for a snapshot built on 2026-12-28 (forecast into 2027).

## W8. Privacy

The snapshot holds counts, levels, dates, instants, the zone id, the rollover hour, the week start and the studied-today line, and nothing else (R312, R318). It never holds:

- note content, field text, tags, card templates or media names;
- deck names or deck ids, card ids, note ids or review-log ids;
- due counts, search texts, file paths, account or device names.

A test scans every golden for these (W9). The widget never opens the collection (R317, R1840).

## W9. Scenarios

Meguri goldens, no oracle: `conformance/scenarios/stats/widget/` with goldens in `conformance/goldens/stats/widget/`, which U165 also uses as decoding fixtures. They need an observation of the snapshot's content; its shape goes into `specs/conformance/stats-ops.md` with U100's executor support.

| Scenario | Pins |
|---|---|
| `ae51-widget` (AE51) | daily rows 2025-10-13 to 2026-10-11, built 2026-10-12 21:00: current streak 364; `upcoming[0]` at 2026-10-13 04:00 with current 0, longest 364 |
| `no-history` | `state` `"no-history"` and W3's short member set |
| `year-end` | built 2026-12-28: `upcoming` crosses into 2027, forecast levels through 2027-12-31 |
| `gap-rollover` | Europe/Paris, `R` 2, built 2026-03-28: `nextRolloverAt` 2026-03-29T01:00:00Z |
| `fall-back` | Europe/Paris, `R` 4, 50 answers on 2026-10-24 and cards due on 10-24 and 10-25, built 2026-10-24 21:00: `upcoming[0]` at 2026-10-25T02:00:00Z with `studiedToday` 0 (e(D) 03:00Z) and its `todayCell` level the forecast level of 10-25 |
| `limhist-slide` | `limhist` 30, only rows on T − 29: every entry keeps that history, `state` `"ready"` and streaks (Days learned over 31 to 37 days) |
| `limdecks` | a deck in `limdecks`: its rows leave every count |
| `week-start` | `firstDayOfWeek` 1: `weekStart` 1 and `history.start` on a Monday |
| `no-content` | notes with field text, tags and deck names: none of them in the file (W8) |

Trigger, debounce, write-rule, atomicity and determinism tests are core tests (U100).

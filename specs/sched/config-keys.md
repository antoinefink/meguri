# Scheduling config keys

Spec: sched/config-keys
Requirements: R590, R582, R607, R608, R658, R665, R692, R700, R702, R705, R709, R715, R741, R751, R759, R760, R763, R764, R1265, R1266
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/data/

The collection config keys the scheduler reads or writes: each key's JSON shape, the value it reads as when missing or unreadable, its meaning, and who writes it. The table, its byte-level rules (typed reads that never fail, the compact form, no write of an unchanged value) and the keys Meguri never touches are `storage/config-table`. The day-clock keys are defined in `sched/day-clock#D2`; this file lists them so the scheduler's set is complete.

## G1. Readers

Each key is read with one of these readers (`storage/config-table#C3`). A missing key, a `val` that is not a BLOB, bytes that are not JSON, and JSON of another shape all read as the key's default; a read never writes.

| Reader | Accepts | Does not accept (reads as the default) |
|---|---|---|
| bool | `true`, `false` | `1`, `0`, `"true"`, `null` |
| u32 | a JSON integer from 0 to 4294967295, written without fraction or exponent | `-1`, `4294967296`, `5.0`, `5e0`, `"5"` |
| i32 | a JSON integer from -2147483648 to 2147483647 | `2147483648`, `1.0`, `"1"` |
| string | a JSON string | `1`, `null`, `["a"]` |

Writing a key stores its value in the compact JSON form, as a local change, and writes nothing when the stored value is the same (`storage/config-table#C4`, `#C5`). Booleans are written as `true` or `false`, integers in decimal, strings as JSON strings.

## G2. The keys

| Key | Reader | Default | Meaning | Written by (Meguri) |
|---|---|---|---|---|
| `collapseTime` | u32 | 1200 | learn-ahead limit in seconds ("Learn ahead limit", minutes in the UI, R760) | Settings (U49): minutes × 60 |
| `rollover` | `sched/day-clock#D2` | 4 | next day starts at this hour (R760) | explicit compatibility day query; Settings; automatic checks use the effective default without repairing it (D8) |
| `creationOffset`, `localOffset` | `sched/day-clock#D2` | none, 0 | day-count offsets | the day clock (`localOffset` only) |
| `lastUnburied` | u32 | 0 | last day buried cards were restored | explicit compatibility query (D7); automatic check only after actual restoration, and real bury actions atomically before burying (D8) |
| `nextPos` | u32 | 0 (a new collection holds 1) | next new-card position (R590, `sched/card-data#K7`) | card generation: the read value + 1, wrapping from 4294967295 to 0 |
| `timeLim` | u32 | 0 | timebox limit in seconds, 0 = off (R759; minutes in the UI) | Settings: minutes × 60 |
| `newCardsIgnoreReviewLimit` | bool | false | "New cards ignore review limit" (R608, R741) | deck options save (U49) |
| `applyAllParentLimits` | bool | false | "Limits start from top" (R607, R741) | deck options save (U49) |
| `loadBalancerEnabled` | bool | true | load balancer (R665, R751) | its toggle (U49) |
| `fsrs` | bool | false | FSRS on | the FSRS-only policy turns it on (R702, U50) |
| `fsrsShortTermWithStepsEnabled` | bool | false | short-term scheduling also for presets with steps (R658) | never (no UI); read by U45 |
| `fsrsHealthCheck` | bool | true | health check after optimizing (R700) | never; read by U47 |
| `fsrsLegacyEvaluate` | bool | false | another client's evaluation option | never (R763) |
| `lastFsrsOptimize` | i32 | 0 | day number of the last automatic optimization (R692) | the optimizer (U47): today's day number |
| `setDueReviewer` | string | `"1"` | last text entered in Set Due Date from review (R709) | Set Due Date in review (U48) |
| `setDueBrowser` | string | `"0"` | last text entered in Set Due Date from the browser (R709, R1265) | Set Due Date in the browser (U48) |
| `restorePositionReviewer` | bool | true | Forget's "Restore original position" from review (R715) | Forget in review (U48) |
| `restorePositionBrowser` | bool | true | the same from the browser (R715) | Forget in the browser (U48) |
| `resetCountsReviewer` | bool | false | Forget's "Reset repetition and lapse counts" from review (R715) | Forget in review (U48) |
| `resetCountsBrowser` | bool | false | the same from the browser (R715) | Forget in the browser (U48) |
| `randomOrderReposition` | bool | false | Reposition dialog's "Randomize order" default | never (R1266) |
| `shiftPositionOfExistingCards` | bool | false | Reposition dialog's "Shift position of existing cards" default | never (R1266) |
| `schedVer` | `storage/open-and-connections#C2.2` | v1 | scheduler version; only the integer 2 opens | never (v1 collections are refused) |
| `sched2021` | bool | false | with `schedVer` 2: the current (v3) scheduler | the open path turns it on when false (R705, U50) |
| `cardStateCustomizer` | string | `""` | another client's custom scheduling JavaScript | never; Meguri ignores it (R763, R764) |

- `lastFsrsOptimize` is read as an i32 and compared as a day number: a value that does not fit reads as 0, so the optimizer treats the collection as never optimized.
- Day numbers and `nextPos` are unsigned 32-bit values; `nextPos` 4294967295 makes the next note's cards share position 4294967295 and stores 0 (golden `sched/data/positions`).
- Keys written by "never" are kept as stored, with their bytes, `usn` and `mtime_secs`, through every operation (`storage/config-table#C7`, R763). Meguri still reads them where the table says.

## G3. Keys the scheduler does not use

`dayLearnFirst` and `newSpread` belong to Anki's previous scheduler; the current one does not read them. `estTimes` and `dueCounts` are reviewer display options (R582), `activeDecks` a legacy list, `curDeck` the current deck (`decks/decks#D11`). Meguri's scheduler reads none of them for scheduling and never writes them (`curDeck` is the decks area's). Like every key Meguri does not use, they survive unchanged (`storage/config-table#C7`).

## G4. Writes of Anki's deck options save (for U49)

Anki's deck options save writes `newCardsIgnoreReviewLimit`, `applyAllParentLimits`, `fsrsHealthCheck` and `cardStateCustomizer` every time, adding them when they were missing, and `fsrs` when it changed (`decks/deck-presets#P12`). Writing the same value is no write (G1), so Meguri differs from Anki only in whether a missing key is added. Meguri never writes `cardStateCustomizer` or `fsrsHealthCheck` (R763); whether its save adds the two limit keys when they are missing is U49's decision.

## G5. Scenarios and tests

Typed reads are not observable through the oracle by themselves; the goldens show them through their effects:

| Scenario | Covers |
|---|---|
| `sched/data/positions` | `nextPos`: 41, 4294967295 and a string |
| `sched/unbury-on-rollover/*`, `sched/day/*` (U40) | the day-clock keys |

Meguri's own tests read every key of G2 from the values of G1's table (missing, each accepted shape, each refused shape) and check the default; they write each key Meguri writes and check the compact bytes.

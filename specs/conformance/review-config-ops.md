# Review settings observation

Spec: conformance/review-config-ops
Requirements: R128, R129, R130, R131, R133 (the typed reads of `review/review-preferences` P2)
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/settings-keys/

A draft observation kind for `review/review-preferences` P2: the values Anki's own reads give for the review settings keys, so that a scenario can compare Meguri's typed reads with Anki's for stored values of every shape. Writes need no new op: `set_config` (`record-ops.md` W4) and `db_exec` store the values, and `set_scheduling_prefs` (`deck-options-ops.md` E4) is the scheduling save.

## R1. Why a kind

The keys' reads have no effect the existing kinds show: `answer_labels` are computed whatever `estTimes` holds, the card page's play buttons are outside `render` (`render/card-render-pipeline#K9`), and the timebox and audio rules are screen behavior. Only the read itself can be compared.

## R2. Observation kind `review_config`

- **Request** `review_config`, no arguments.
- **Value** an object with exactly these six members, one per key the Settings › Review rows read (`review/review-preferences#P1`). Every member reads as `sched/config-keys#G1` and `storage/config-table#C3` say: a missing key, a `val` whose storage class is not BLOB (a TEXT value included), bytes that are not JSON and JSON of another shape all give the member's default.

  | Member | Reader | Accepts | Default |
  |---|---|---|---|
  | `estTimes`, `interruptAudioWhenAnswering` | bool | the JSON literals `true` and `false` | `true` |
  | `hideAudioPlayButtons` | bool | the same | `false` |
  | `timeLim` | u32 | a JSON integer from 0 to 4294967295 written without fraction or exponent (`600`; not `600.0`, `6e2`, `"600"` or `-60`) | 0 |
  | `collapseTime` | u32 | the same | 1200 |
  | `rollover` | `sched/day-clock#D2` | the effective hour, 0 to 23: a stored integer from 0 to 23 as itself, from 24 to 255 as 23 | 4 |

- `dueCounts` is not a member: nothing in Meguri reads it (`review/review-preferences#P5`), and its bytes are compared by `db_exec` reads instead.
- **Oracle.** Anki's preferences read, its scheduling and review groups: the hour, the learn-ahead seconds, the three review booleans above and the timebox seconds, reported with the member names above. That read uses typed reads, not the review screen's raw reads (`review/review-preferences#P7`).
- **Meguri.** The adapter reports the typed reads the app uses (`review/review-preferences#P2`, `sched/day-clock#D2`).
- Nothing in the collection changes: no row is written, `col.mod` does not move, and neither undo history changes. In particular the `rollover` member never writes `4` for a missing key: that is the day query's write (`sched/day-clock#D5`), and this read is not a day query.

## R3. Status

Draft (`dsl.md` D7.2, `observations.md` O4): no driver op, schema shape or typed model exists yet. The oracle maintainer adds them together, as `conformance/AGENTS.md` asks. Once they exist, `storage/settings-keys/review-reads` is written:

| Scenario | Steps |
|---|---|
| `storage/settings-keys/review-reads` | for each of R2's six keys: the key deleted, then stored as each value of `review/review-preferences#P2`'s table (with `db_exec`, one `review_config` after each), including a value stored as TEXT; `collapseTime` also as `90` and `4294967296`; `rollover` also as 23, 24, 255, 256, `4.0` and `-1` |

It stays inactive until Meguri's adapter implements the kind. The two scenarios already in `conformance/scenarios/storage/settings-keys/` use only stable ops and need nothing from this file.

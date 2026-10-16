# FSRS policy ops for scenarios

Spec: conformance/fsrs-policy-ops
Requirements: R702, R703, R705, R1883
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/compat/

Two scenario ops (`dsl.md` D7.2) for the FSRS-only policy (`sched/fsrs-policy`): `enable_fsrs` turns the global FSRS switch on, and `enable_v3` is Anki's scheduler update, which Meguri does at open instead. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` holds their shapes, and `protocol.json` lists them for oracle workers.

## F1. Shapes

| Op | Argument | Output | Rejections |
|---|---|---|---|
| `enable_fsrs` | `{}` | none (`{}`) | none |
| `enable_v3` | `{}` | none (`{}`) | none |

Any other argument is `invalid_scenario`. Neither op takes logical ids.

## F2. `enable_fsrs`

Turns FSRS on for the collection, without rescheduling.

- **Meguri:** runs `enableFsrs` (`sched/fsrs-policy` E1 to E4) with no cancel request; progress is ignored. Its `SessionActive` refusal cannot happen in a scenario, which has no review session; if it does, it is a run error.
- **Oracle:** Anki's deck options save that turns FSRS on, as a user makes it who ticks FSRS in the deck options screen and saves with "reschedule cards on change" off:
  - the deck the screen is opened for: the Default deck (id 1);
  - the presets sent: only that deck's preset, exactly as stored (read with Anki's preset read, not with the screen's loading, which fills an empty FSRS-6 list from the older ones). No other preset is sent, so no other preset is saved;
  - mode: normal (neither "apply to child decks" nor "optimize all presets");
  - FSRS on; reschedule cards on change off (protobuf `fsrs_reschedule` false);
  - the custom scheduling script, "New cards ignore review limit", "Limits start from top", the health check and the Default deck's limits: the values the screen reads now, sent back unchanged.
- **Check (oracle):** after the save, the driver compares every row of every table except `cards`, the `config` row `fsrs` and `col.mod` with the state before the step. Any difference fails the step with `invalid_scenario`: the scenario does not meet F2.1, so Anki's save did more than `enableFsrs` does, and the golden would hold Anki's extra writes. The `cards` table and the `fsrs` row are compared by the golden, not by this check.

### F2.1 Preconditions

Anki's save writes more than `enableFsrs` unless (`sched/fsrs-policy` E5):

- the config keys `cardStateCustomizer` (a string), `newCardsIgnoreReviewLimit`, `applyAllParentLimits` and `fsrsHealthCheck` (booleans) are present, each stored as the compact JSON Anki writes for it (setup's `config` section writes them so). Anki writes all four on every save and adds missing ones;
- every normal deck's preset exists. Anki moves a deck whose preset is missing to the Default deck's preset;
- the Default deck has no This-deck or Today-only limits;
- the Default deck's preset has an ignore date that is empty or not after today, and a non-empty FSRS-6 list or no list at all. Anki's save moves a later date back to today and clears the older lists of a preset sent with an empty FSRS-6 list;
- every preset that a normal deck uses has a parameter list the crate can build (`sched/fsrs-policy` U1). Otherwise Anki's save fails, a run error (`anki_error`).

Anki's save also runs a day query. Scenarios follow `sched/day-clock#D14.2`: after a clock move across a boundary they observe `timing` before `enable_fsrs`. Then nothing in the day clock writes at the step, and the check above passes.

## F3. `enable_v3`

Anki's scheduler update for a collection on the previous scheduler (R705).

- **Oracle:** the update as Anki's desktop client makes it when the user accepts it: it turns the current scheduler on (`sched2021` = `true`, a config write without an undo entry that keeps the undo history), then the desktop layer loads the current scheduler, which writes `creationOffset` (through the scheduling preferences) when that key is missing. On a collection already on the current scheduler it writes nothing.
- **Meguri:** does nothing. Meguri switches when its engine opens a collection (`sched/fsrs-policy` V2, V3), so after `reopen` or a handoff its collection already holds what Anki's update writes. A Meguri that did not switch fails the scenario's next observation.
- **Precondition:** `creationOffset` is present. Without it Anki's update also adds it, and Meguri never does (`sched/day-clock#D12.3`); such a scenario needs a deviation.

## F4. Where the policy's open steps run

Meguri runs the open steps of `sched/fsrs-policy` (the switch of V2 and the seen-on mark of S2) whenever its engine opens a collection: the new collection of a scenario's setup, `reopen`, and handing a collection in at a handoff. The storage `open` step (`storage-ops.md` P1) uses the storage open gate alone and runs neither, as it runs no day query (`sched/day-clock#D14.2`).

## F5. Status

- `dsl.md` D7.2 lists both ops as stable with these shapes; `scenario.schema.json` declares them with no properties. Neither has an entry in `observation.schema.json`: their output is `{}`.
- Driver: the oracle maintainer adds both ops (an area module under `oracle/driver/ops/`) with the F2 check, and records the goldens of `conformance/scenarios/sched/compat/`.
- Meguri's executor: U50's implementer adds the typed model entries (`core/crates/meguri-testkit/src/scenario/`) and the adapters (`exec/sched.rs`). Until the typed model reads the two ops, T0 reports the scenarios that use them as `invalid`, as `dsl.md` D10 says; until the adapters exist they are `pending`.

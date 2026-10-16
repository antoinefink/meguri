# Deck options ops: saving deck options, the ignore-date count

Spec: conformance/deck-options-ops
Requirements: R686, R687, R690, R1883
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/memory/

The scenario op and the observation kind that the memory-state scenarios need (U46): `save_deck_options`, the deck options screen's Save, and `ignored_before_count`, the count behind "N of M cards included". Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. What a save does to memory states is `sched/memory-state` M4 and M7.1.

## E1. `save_deck_options` (op)

- **Argument** `{deck, preset?, options?, deck_desired_retention?}`:
  - `deck`: a deck from `setup.decks` (`Default` for the default deck): the deck whose options screen is open;
  - `preset`: `Default` or a preset from `setup.presets`: the preset the deck is switched to; missing means the deck's current preset;
  - `options`: a mapping of preset options to new values, applied to that preset. Keys: the options of `setup.presets` (`dsl.md` D6.2, with the same value types; `fsrs_params` sets the FSRS-6 list, `fsrs_params_6`), plus `historical_retention` (a number, `sched/fsrs-items#I1.4`) and `ignore_reviews_before` (a string, the date text as stored, `sched/fsrs-items#I1.3`);
  - `deck_desired_retention`: a number sets the deck's own desired retention ("This deck", `sched/presets#V7`); `null` removes it; missing leaves it as it is.
- **Effect.** What the deck options screen of `deck` does when the user changes these values and saves, as one operation:
  1. The deck uses the chosen preset. That preset's options are its stored options, read with `sched/presets#V3`, with `options` applied; every other option keeps its value.
  2. The deck's own desired retention is set, removed or kept as the argument says. The deck's other limits keep their values.
  3. Everything else the screen holds keeps its stored value: the global `fsrs` key and the other collection settings the screen shows, the other presets. Changes apply to `deck` only, not to its subdecks; no preset is removed; "reschedule cards on change" is off.
  4. The save writes the preset and the deck, and then does what Anki's save does after them: it reorders a deck's new cards when its insertion order changed, remaps the remaining steps of learning cards when steps changed (R644), and recomputes memory states (`sched/memory-state` M7.1).
  5. It is one undoable step named "Deck Options" (`undo-ops.md` V4 `deck_options`).
- **Output**: none (`{}`).
- **Rejections**: none. A save Anki refuses (a parameter list its crate cannot build) is a run error; scenarios avoid it.
- **Scenarios.** Today-only deck limits are outside this op: scenarios that use it give their decks none. To show which cards a save stamps, set every card's `mod` and `usn` with a `db_exec` before the save (`sched/memory/recompute`).
- **Meguri.** The adapter is U49's deck options save. Until it exists, scenarios using the op are `pending` (`dsl.md` D10).

## E2. `ignored_before_count` (observation kind)

- **Request** `{ignored_before_count: {preset, dates}}`: `preset` is `Default` or a preset from `setup.presets`; `dates` is a non-empty list of strings, each a date text as the screen would hold it. At most one request per observe step.
- **Value** a list with one entry per date, in order: `{included, total}` (non-negative integers), or `{error: "invalid_date"}` when the text is not a date Anki reads (`sched/fsrs-items#I1.3`).
- **Meaning.** `sched/memory-state#M5`: `total` is the number of cards that the preset's search finds and that are not new; `included` the number of those with a review-log row of stored `type` 0 after the date's cutoff. Reading it changes nothing.
- **Meguri.** The adapter calls U46's count query with the preset and each date.

## E3. Driver and executor status

- The oracle driver implements `save_deck_options` and `ignored_before_count` with U46's oracle-maintainer work, together with `protocol.json` and the testkit's scenario model; until then `sched/memory/recompute` and `sched/memory/ignore-before-count` have no goldens.
- Meguri's executor implements `ignored_before_count` with U46's adapter, and `save_deck_options` with U49's.

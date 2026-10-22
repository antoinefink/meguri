# Deck options ops: saving deck options, the scheduling settings, the ignore-date count

Spec: conformance/deck-options-ops
Requirements: R644, R686, R687, R690, R732, R733, R737, R740, R741, R744, R760, R761, R1883
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/memory/, conformance/scenarios/sched/options/

The scenario ops and the observation kind that the memory-state (U46) and deck options (U49) scenarios need: `save_deck_options`, the deck options screen's Save; `set_scheduling_prefs`, the scheduling settings' save; and `ignored_before_count`, the count behind "N of M cards included". Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. What a save does is `sched/deck-options` O7 to O9 (and `sched/memory-state` M4, M7.1 for memory states); the settings are `sched/deck-options` O10.

## E1. `save_deck_options` (op)

- **Argument** `{deck, preset?, options?, deck_desired_retention?, limits?, globals?, remove?, subtree?}`:
  - `deck`: a deck the collection holds, by its stored name in human form (`Default` for the default deck): the deck whose options screen is open. A filtered deck is a run error.
  - `preset`: `Default` or a preset the collection holds, by its stored name: the preset the deck is switched to; missing means the deck's current preset.
  - `options`: a mapping of preset options to new values, applied to that preset. Keys: the options of `setup.presets` (`dsl.md` D6.2, with the same value types; `fsrs_params` sets the FSRS-6 list, `fsrs_params_6`), plus `historical_retention` (a number, `sched/fsrs-items#I1.4`), `ignore_reviews_before` (a string, the date text as stored, `sched/fsrs-items#I1.3`) and `insertion_order` (`sequential` or `random`: `new_card_insert_order` 0 or 1).
  - `deck_desired_retention`: a number sets the deck's own desired retention ("This deck", `sched/presets#V7`); `null` removes it; missing leaves it as it is.
  - `limits`: `{new?, review?, new_today?, review_today?}`, each an integer from 0 to 9999 or `null`: the deck's This-deck limits (`new`, `review`) and Today-only limits (`new_today`, `review_today`). A number sets it, `null` clears it (`sched/deck-options#O9`). A missing This-deck limit keeps its stored value. A missing Today-only limit is passed on with its stored value when its stored day is today, and as cleared otherwise: what the screen sends when that tab is left alone (`sched/deck-options#O6.2`).
  - `globals`: `{new_cards_ignore_review_limit?, apply_all_parent_limits?}`, booleans: the global options `newCardsIgnoreReviewLimit` and `applyAllParentLimits`; a missing one keeps the value read.
  - `remove`: a non-empty list of distinct preset names to remove in this save (R733). Removing the preset the deck is switched to is a run error (`invalid_scenario`).
  - `subtree`: `true` saves for the deck and all its subdecks ("Save to all subdecks", R732); `false` when missing.
- **Effect.** What the deck options screen of `deck` does when the user makes these changes and saves, as one operation (`sched/deck-options#O7`):
  1. The presets of `remove` are removed, which forces a one-way sync (`scm` set). The removal is confirmed: Meguri's executor passes the confirmation, as `notetype-ops.md` T9 does.
  2. The chosen preset's options are its stored options, read with `sched/presets#V3`, with `options` applied; every other option keeps its value. No other preset is edited.
  3. The deck, every normal subdeck with `subtree`, and every normal deck whose preset was removed or names no preset get the chosen preset and the deck's values (its limits and desired retention after `limits` and `deck_desired_retention`).
  4. The global options are set as `globals` says. Every other collection-wide setting the screen shows keeps its stored value (the `fsrs` key included); "reschedule cards on change" is off.
  5. Then what Anki's save does after them: new cards of decks whose insertion order changed are re-sorted, remaining steps of learning cards remapped (R644), memory states recomputed (`sched/memory-state` M7.1).
  6. It is one undoable step named "Deck Options" (`undo-ops.md` V4 `deck_options`).
- **Output**: none (`{}`).
- **Rejection** `default_preset` when `remove` names the default preset: nothing changes. Any other save Anki refuses (a parameter list its crate cannot build) is a run error; scenarios avoid it.
- **Meguri.** The adapter builds a draft from U49's snapshot of `deck`, applies the argument and runs U49's save (`sched/deck-options#O7`) with the confirmation. The options the screen shows go through the draft. The hidden ones (`fsrs_params`, `starting_ease`, `easy_bonus`, `hard_interval`, `interval_modifier`, `lapse_new_interval`, `graduating_interval_days`, `easy_interval_days`), which the screen never writes (R752), are applied by the adapter inside the same undo step, with the preset update of `decks/deck-presets#P10` (for `fsrs_params`, the optimizer's setter followed by the recompute of `sched/memory-state#M7.3`), so that the rows equal a save that changed them; only U46's scenarios use them.

## E2. `ignored_before_count` (observation kind)

- **Request** `{ignored_before_count: {preset, dates}}`: `preset` is `Default` or a preset from `setup.presets`; `dates` is a non-empty list of strings, each a date text as the screen would hold it. At most one request per observe step.
- **Value** a list with one entry per date, in order: `{included, total}` (non-negative integers), or `{error: "invalid_date"}` when the text is not a date Anki reads (`sched/fsrs-items#I1.3`).
- **Meaning.** `sched/memory-state#M5`: `total` is the number of cards that the preset's search finds and that are not new; `included` the number of those with a review-log row of stored `type` 0 after the date's cutoff. Reading it changes nothing. The oracle builds the default search as the screen does (name escaped), so a preset whose name holds `\` or `"` counts 0 of 0 there; scenarios do not use such names (M5, M10).
- **Meguri.** The adapter calls U46's count query with the preset and each date.

## E3. Driver and executor status

- The oracle driver implements `save_deck_options` with all E1 arguments and the rejection `default_preset`, `set_scheduling_prefs` (E4), and `ignored_before_count` (E2). A missing Today-only limit follows E1's rule.
- The goldens of `sched/memory/recompute` and `sched/memory/ignore-before-count` are recorded on the Linux and Apple oracles. All eleven `sched/options/` scenarios are recorded and pass comparison on both oracles, including `insertion-order-option`, `deck-limits`, `subtree`, `remove-preset`, `globals` and `prefs-rollover`.
- Meguri's executor implements `ignored_before_count` with U46's adapter, and `save_deck_options` and `set_scheduling_prefs` with U49's. Until then scenarios using them are `pending` (`dsl.md` D10).

- U46's shared recompute engine operation does not by itself implement E1. Keep `save_deck_options` unavailable until the real save also performs its preset, deck, step-remapping and insertion-order effects. U46 tests its real recompute, undo and cancellation directly against the golden's input/output pairs; it must not advertise a fixture-only or partial Save adapter. The count observation is independent and can become active with its real U46 query.

## E4. `set_scheduling_prefs` (op)

- **Argument** `{rollover?, learn_ahead_minutes?, load_balancer?}`, at least one member: `rollover` an integer from 0 to 23 ("Next day starts at"), `learn_ahead_minutes` an integer from 0 to 999 ("Learn ahead limit"), `load_balancer` a boolean.
- **Effect.** The scheduling settings' save with these values, everything else the settings hold kept as read, as one undoable step named "Preferences" (`undo-ops.md` V4 `preferences`): `rollover` is stored as given, `collapseTime` as the minutes × 60, `loadBalancerEnabled` as the boolean (`sched/deck-options#O10`). A changed `rollover` takes effect at once: the next day query computes the day with the new hour (`sched/day-clock#D6`, `#D13`).
- **Output**: none (`{}`). No rejections.
- **Oracle.** Anki's preferences are read, the three values replaced (the learn-ahead limit in seconds) and saved back with Anki's preferences save, which drops the cached day when the hour changes. That save also writes the other keys of the scheduling and reviewing settings with the values read, adding the missing ones.
- **Meguri.** The adapter runs U49's scheduling preferences op (`sched/deck-options#O10`), which writes only the three keys.

## E5. Writing deck options scenarios

- **Keys Anki adds.** Anki's deck options save writes `newCardsIgnoreReviewLimit`, `applyAllParentLimits`, `cardStateCustomizer` and `fsrsHealthCheck`, adding them when missing, and Anki's preferences save writes several reviewing and scheduling keys; Meguri writes only what changes (`sched/deck-options#O12`). Scenarios read config rows only for the keys they set to a value that differs from the one read, and never `col.mod`, `db_dump`'s `config` table or `converge` after such a save.
- **Re-sorted cards.** Anki stamps every new card of a re-sorted deck, Meguri only those whose position changes: read positions (`due`, `odue`) after a re-sort, never `mod` or `usn` of new cards.
- **Random order.** After a Random re-sort, read only what holds for every draw: per deck, the number of new cards, the smallest and largest position, the number of distinct positions and of notes, and whether siblings share a position (`sched/options/insertion-order-switch`).
- **Undo.** `legacy_json` and `blob_decode` reopen the collection on the oracle, which empties both histories (`undo-ops.md` V5): observe them after the undo checks. A settings change that changes the day clears Meguri's history at the rollover and not Anki's (`sched/day-clock#D9`, `#D12.4`): run a `db_exec` that changes nothing before a later undo check.
- **Day queries.** Observe `timing` right after `set_scheduling_prefs` changes `rollover`, before the clock moves, so both executors run the day query and its unbury at the same instant (`sched/day-clock#D14.2`).
- **Stamps.** To show which cards a save writes, set every card's `mod` and `usn` with a `db_exec` before it and `advance` the clock (`sched/memory/recompute`).

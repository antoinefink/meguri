# Deck ops: decks, presets, the current deck and the deck tree

Spec: conformance/deck-ops
Requirements: R1883, R55, R57, R60, R61, R62, R63, R65, R66, R68, R69, R322, R1465, R1474, R732, R733
Anki: 26.09.3
Scenarios: conformance/scenarios/decks/

The scenario ops (`dsl.md` D7.2) and observation kinds (`observations.md` O4) that the decks area needs. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The behavior they run is `specs/decks/decks.md` and `specs/decks/deck-presets.md`.

## K1. Naming decks and presets in steps

- A deck is named by its stored name in human form (`::`), compared exactly, byte for byte, with no case folding and no conversion: the name the collection holds at that moment, whether `setup.decks`, an earlier op or a `db_exec` gave it. A name that matches no deck is a run error (`unknown_ref`).
- A preset is named by its stored name, compared exactly. A name that matches no preset, or matches more than one, is a run error (`unknown_ref`, `invalid_scenario`); scenarios give their presets distinct names.
- The typed name an op takes (`add_deck`, `rename_deck`) is passed to the operation as given, so it may hold anything the operation converts (`decks/decks#D1`).
- Ops that create or rename a deck report its stored name, which can differ from the typed one; later steps use the reported name.
- Every op reads the decks and presets it uses from the collection when the step runs, so rows that a `db_exec` wrote are seen (`record-ops.md` W1).
- `add_note`'s `deck` (`dsl.md` D7.2) may name any deck the collection holds at that moment by its stored human name, not only those of `setup.decks`.

## K2. `add_deck`

- **Argument** `{name}`: the typed name, a string.
- **Effect** `decks/decks#D5`, as one operation.
- **Output** `{add_deck: {name}}`: the stored human name of the deck the operation returns (the existing deck when the name was already there).
- **Rejections** `filtered_parent`, `too_deep` (`decks/decks#D3`); nothing changes.

## K3. `rename_deck`

- **Argument** `{deck, name}`: the deck (K1) and the typed new full name.
- **Effect** `decks/decks#D6`.
- **Output** `{rename_deck: {name}}`: the deck's stored human name after the op.
- **Rejections** `filtered_parent`, `too_deep`, and `under_descendant`, which only Meguri records (`decks/decks#D15.1`, with a deviation for each scenario that shows it).

## K4. `reparent_decks`

- **Argument** `{decks: [<name>, ...], parent}`: at least one deck, in order; `parent` is the target deck, or the empty string for the top level.
- **Effect** `decks/decks#D7`.
- **Output** `{reparent_decks: {count}}`: the number of listed decks that moved.
- **Rejection** `filtered_parent` when the target is a filtered deck; nothing changes.

## K5. `delete_decks`

- **Argument** `{decks: [<name>, ...]}`: at least one deck, in order.
- **Effect** `decks/decks#D8` (the backup of R2008 is not part of the op on either side).
- **Output** `{delete_decks: {count}}`: the number of cards removed.
- Logical ids of removed cards and notes stay valid: `card` reports `null` for a removed card (`record-ops.md` W3).

## K6. `set_deck_collapsed`

- **Argument** `{decks: [<name>, ...], scope, collapsed}`: at least one deck; `scope` is `study` or `browser`; `collapsed` a boolean.
- **Effect** `decks/decks#D10` with the decks in order. The oracle runs Anki's collapse once per deck; Meguri runs one operation for the list. The rows they leave are the same.
- **Output** none (`{}`).
- Scenarios never observe the undo status after this op: Anki records no step, Meguri records "Expand/Collapse" (`decks/decks#D15.3`).

## K7. `set_current_deck`

- **Argument** `{deck}`.
- **Effect** `decks/decks#D11`: the `curDeck` write.
- **Output** none (`{}`).
- Scenarios never observe the undo status after this op (`decks/decks#D15.4`).

## K8. `add_preset`, `clone_preset`, `rename_preset`

- `add_preset {name}`: `decks/deck-presets#P4`, with the name as given.
- `clone_preset {preset, name}`: `decks/deck-presets#P5`.
- `rename_preset {preset, name}`: `decks/deck-presets#P6`.
- **Output** none (`{}`). The ops do not apply the menu's naming rule (P7): scenarios give names that differ from every other preset's.
- They assign no preset to any deck and write nothing else: no config key, no deck. On the oracle that means Anki's deck-config storage calls with the values of P4 (the backend defaults plus the FSRS-6 default parameters), not the deck options save, which also writes collection-wide keys (`decks/deck-presets#P12`).

## K9. `remove_preset`

- **Argument** `{preset, fallback}`: the preset to remove and the fallback preset (`decks/deck-presets#P8`); they differ (`invalid_scenario` otherwise).
- **Effect** `decks/deck-presets#P8`, confirmed: the oracle never asks, and Meguri's executor passes `schema_confirmed` (as `notetype-ops.md` T9 does for note types). It sets `scm`; scenarios read it with `db_exec`.
- **Output** none (`{}`).
- **Rejection** `default_preset` when `preset` is the default preset (id 1); nothing changes.
- The op writes nothing else: no config key, no deck limit.

## K10. `set_deck_preset`

- **Argument** `{deck, preset, subtree?}`: `subtree` is a boolean, `false` when missing.
- **Effect** `decks/deck-presets#P9`.
- **Output** none (`{}`).
- **Rejection** `filtered_deck` when `deck` is a filtered deck; nothing changes.
- The op writes nothing but `config_id` and the stamps of the decks that change: no limit, no config key.

## K11. `decks` (observation kind)

- **Request** `decks`, no arguments.
- **Value** an object with one member per deck the collection holds, keyed by its stored name in human form: `{id, filtered, config_id, study_collapsed, browser_collapsed, mtime_secs, usn}`. `id` is the deck's id (deck ids are real ids by definition here, as in `db_dump`: they come from the frozen clock); `filtered` a boolean; `config_id` the normal deck's `config_id`, `null` for a filtered deck; the two flags from the deck common; `mtime_secs` and `usn` from the row.
- It reads the collection when the step runs and changes nothing.

## K12. `deck_tree` (observation kind)

- **Request** `deck_tree`, no arguments.
- **Value** the tree of `decks/decks#D12` as a list of the top-level nodes in order, each `{name, children}`: `name` the last component of the deck's name, `children` a list of nodes. The default deck is left out when D12 hides it.
- It changes nothing in the collection. It has no counts; the queue area (U41) adds them to this kind.

## K13. Writing deck scenarios

- **Filtered decks.** No op creates one. A scenario inserts the `decks` row with `db_exec` (`model-ops.md` L3) and moves cards into it with `db_exec` (`did`, `odid`, `odue`, `due`, `queue`), as Anki's build leaves them (`decks/decks#D9`). It gives the row a small fixed id (below 1,000,000), so that ids the clock allocates later do not depend on it (`storage/ids-usn-mtime` I2), `mtime_secs` and `usn` 0, the deck common `X'08011001'` and the kind `X'12130801120a0a066465636b3a78106428d804383c'`: a filtered deck that reschedules, with one search `deck:x`, limit 100, order 0, and preview delays of 600 s (Hard) and 60 s (Again), which Anki reads (oracle). The ops of this file see such rows (K1).
- **Schema stamp.** `col.ls` set with `db_exec` stands for a sync (`UPDATE col SET ls = scm`); the next schema change then sets `scm` again (`notetypes/schema-changes` G5).
- **Stamps.** Scenarios `advance` the clock before an op whose stamps they read, so `now_s` differs from the creation time.
- **Undo.** Scenarios observe no undo status after `set_deck_collapsed` or `set_current_deck` (K6, K7).

## K14. Status

- The oracle driver implements none of these ops and kinds yet: U28's oracle-maintainer work adds them, records the goldens of `conformance/scenarios/decks/`, and extends `add_note`'s `deck` as K1 says.
- Meguri's executor implements them with U28's deck and preset operations; until then the scenarios are `pending` (`dsl.md` D10). Until the testkit model (`core/crates/meguri-testkit/src/scenario/`) reads these ops and kinds, T0 reports the scenarios that use them as `invalid`; U28's implementer adds the typed arguments with the adapters.

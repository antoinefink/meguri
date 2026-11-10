# Undo ops: undo, redo, config writes with an undo step, the undo status

Spec: conformance/undo-ops
Requirements: R1883, R370, R719, R720, R1887; R1727 (the Meguri-only move step)
Anki: 26.09.3
Scenarios: conformance/scenarios/undo/

The scenario ops and the observation kind that the undo scenarios need (U25): `undo` and `redo`, two forms of config write that record an undo step, and `undo_status`, the step an undo or a redo would apply. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. What an undo or a redo leaves in the collection is `specs/engine/undo.md` A1–A9.

## V1. `undo` and `redo`

- **Argument** `{}`.
- **Output**: none (`{}`).
- **Effect.** `undo` applies the step at the top of the collection's undo history, as the user's Undo does; `redo` applies the step at the top of the redo history. On Meguri these are the engine's undo and redo with scope `Global` (`engine/undo` M4, M5), through the long-write variant when peek reports a declared write. What they write is `engine/undo` A4–A8; what they do to the two histories is A2.
- **Rejections** (`dsl.md` D7.3). A refusal changes nothing in the collection.

  | Op | Kind | When |
  |---|---|---|
  | `undo` | `nothing_to_undo` | the undo history holds no step: nothing was done since the history was last emptied, every step was undone, or the 30-step limit dropped the rest |
  | `undo` | `changed_externally` | Meguri only: the step's own check (`engine/undo` M4.2) found one of its rows changed outside the history, and dropped the step and every older one |
  | `redo` | `nothing_to_redo` | the redo history holds no step |
  | `redo` | `changed_externally` | Meguri only: as for `undo`, for the redo step (M5) |

  Anki answers both empty cases with the same "undo empty" error; the kind names the op. Anki has no check of the step's rows, so the oracle never records `changed_externally`: where Meguri records it, a deviation in `conformance/deviations.yaml` says so (`engine/undo` A8).
- **Meguri's adapter.** Every other refusal of Meguri's undo maps to `nothing_to_undo` (for `redo`, `nothing_to_redo`), whatever reason M3 gives: `None`, a `Cleared…` reason, `AlreadyUploaded`, `UploadPending`, or `ChangedExternally` left by a truncation that emptied the history. In each of these cases Anki's history would be empty too.

## V2. `undo_status` (observation kind)

- **Request** `undo_status`, with no arguments.
- **Value** `{undo: <step> | null, redo: <step> | null}`: the step an `undo` would apply now and the step a `redo` would apply now, each as its id from V4, or `null` when there is none. Reading it changes neither history.
- **Oracle.** The collection's undo status gives each step's name in the collection's language. The driver maps a name back to its V4 id through the backend's own translations, never by comparing with English text written in the driver. A name that is not in V4 (an add-on's custom step) is a run error until V4 lists it.
- **Meguri.** `peek(Global)` and `peek_redo(Global)` (`engine/undo` M3): an available step gives its name, mapped to its V4 id; a reason gives `null`. Meguri's step names are V4's English names exactly (R370).

## V3. Config writes with an undo step

Two forms that record an "Update Config" step, for the config undo scenarios (Anki's config API, as the web views' settings use it).

### V3.1 `set_config` with `undoable`

- **Argument** `{key, value, undoable?}`: `key` and `value` as `record-ops.md` W4; `undoable` a boolean, `false` when missing.
- With `undoable: false` the op is W4 exactly.
- With `undoable: true` the write is one undoable operation named "Update Config" (V4 `update_config`): the key is written as W4 says (the same comparison, stamps and `col.mod`), and when it wrote, it records a step. A write that writes nothing records nothing and leaves `col.mod` alone.
- **Output**: none (`{}`).

### V3.2 `remove_config`

- **Argument** `{key}`: a non-empty string.
- **Effect.** Removes the key through the config API (`storage/config-table` C10) as one undoable operation named "Update Config": the row is deleted, no grave is added, `col.mod` moves to now, and a step is recorded. A missing key changes nothing and records nothing.
- **Output**: none (`{}`).

Both forms, like every operation, empty the redo history even when they write nothing (`engine/undo` A2).

## V4. Step ids and names

The closed list of step ids that `undo_status` reports, with Anki's English name of each step (R370, R719). Meguri names its steps with these names exactly; the ids are the conformance language's own. A name that several Anki operations share has one id. A row marked "Meguri only" is a step that no Anki operation records: its name is Meguri's, the oracle never reports it, and only Meguri's executor and tests see it.

| Id | Name | Made by (scenario op today) |
|---|---|---|
| `add_deck` | Add Deck | |
| `add_note` | Add Note | `add_note`, `setup.notes` |
| `add_note_type` | Add Note Type | |
| `answer_card` | Answer Card | `answer` (`sched/answering` A7) |
| `build_deck` | Build Deck | (building or rebuilding a filtered deck) |
| `bury` | Bury | `bury` (`sched-ops.md` J2) |
| `change_deck` | Change Deck | |
| `change_note_type` | Change Note Type | |
| `clear_unused_tags` | Clear Unused Tags | |
| `custom_study` | Custom Study | |
| `deck_options` | Deck Options | `save_deck_options` (`deck-options-ops.md` E1) |
| `delete_deck` | Delete Deck | |
| `delete_note` | Delete Note | `delete_notes` |
| `empty` | Empty | `empty_filtered` (`sched-ops.md` J9) |
| `empty_cards` | Empty Cards | |
| `find_and_replace` | Find and Replace | |
| `grade_now` | Grade Now | `grade_now` (`sched-ops.md` J8) |
| `image_occlusion` | Image Occlusion | |
| `import` | Import | |
| `move_received_card` | Move Received Card | (Meguri only: a move from Migaku's Recently received, `servers/migaku-received-log` L13, whatever the number of rows) |
| `preferences` | Preferences | `set_scheduling_prefs` (`deck-options-ops.md` E4) |
| `remove_note_type` | Remove Note Type | |
| `remove_tag` | Remove Tag | |
| `rename_deck` | Rename Deck | (renaming or moving a deck) |
| `rename_tag` | Rename Tag | (renaming or moving a tag) |
| `reposition` | Reposition | |
| `reset_card` | Reset Card | `forget` (`sched-ops.md` J7) |
| `select_deck` | Select Deck | (Anki only: Meguri records no step for selecting a deck, R42) |
| `set_due_date` | Set Due Date | `set_due` (`sched-ops.md` J6) |
| `set_flag` | Set Flag | |
| `suspend` | Suspend | `suspend` (`sched-ops.md` J3) |
| `toggle_load_balancer` | Toggle Load Balancer | |
| `unbury_unsuspend` | Unbury/Unsuspend | `unsuspend`, `unbury_deck` (`sched-ops.md` J4, J5) |
| `update_card` | Update Card | |
| `update_config` | Update Config | `set_config` with `undoable: true`, `remove_config` |
| `update_deck` | Update Deck | |
| `update_note` | Update Note | `edit_note` |
| `update_note_type` | Update Note Type | |
| `update_tag` | Update Tag | |

`set_config` without `undoable` records no step (W4), and `db_exec`, `check_db`, `reopen`, `open`, `legacy_update` and `handoff` empty both histories on the oracle (V5).

## V5. Writing undo scenarios

- **Start from an empty history.** What the undo history holds after `setup` is not part of the language: Anki records the setup's note additions as "Add Note" steps, and Meguri's executor need not. An undo scenario runs a `db_exec` whose statement does not begin with `select` before its first `undo`, `redo` or `undo_status`; `record-ops.md` W1 empties both histories after such a statement on both executors. A statement that changes nothing does (`UPDATE notes SET usn = usn WHERE 0`), and so does one the scenario needs anyway (a server `usn`).
- **Steps that empty Anki's history.** Besides such a `db_exec`, the oracle closes and opens the collection for `reopen`, `handoff`, `legacy_update` and the observations `legacy_json` and `blob_decode`, and runs `check_db` without an undo record: each empties both histories in Anki. Keep them out of the stretch between a step and its undo unless that is what the scenario shows.
- **The clock matters.** Two note edits less than 60 seconds apart can merge into one step (`engine/undo` A3): `advance` at least `1m` between edits that must stay separate steps.
- **One scheduler day.** Keep a step and its undo in the same scheduler day: Meguri empties the history at rollover, Anki only when it next builds its study queues.
- **Logical ids.** Undoing a deletion brings back rows whose `#nN` and `#cN` stayed valid (`record-ops.md` W3). A card that an `edit_note` generates gets no logical id on the oracle: observe it through `db_dump` or `db_exec`.

## V6. Driver and executor status

- The oracle driver implements `undo`, `redo`, `remove_config`, `set_config`'s `undoable` and `undo_status` with U25's oracle-maintainer work, together with `protocol.json` and the testkit's scenario model; until then the undo scenarios have no goldens.
- Meguri's executor implements `undo`, `redo` and `undo_status` in its undo adapter (`core/crates/meguri-testkit/src/exec/undo.rs`), and `undoable` and `remove_config` beside `set_config`, once the conformance session runs ops through the engine (U38's `CoreHost::open`). Until then the undo scenarios are `pending` (`dsl.md` D10), and so is any that needs another area's adapter (`edit_note` and `delete_notes`: U27; `answer`: U44) or the new collection that every undo scenario starts from (`dsl.md` D8, U28).

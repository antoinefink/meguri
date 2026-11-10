# Change Note Type ops

Spec: conformance/change-notetype-ops
Requirements: R1883, R944, R945, R946, R947, R948
Anki: 26.09.3
Scenarios: conformance/scenarios/notetypes/change/

The two scenario ops of Change Note Type: the info query and the change, the way the change screen uses them (open it on a selection, then save). Both executors implement them with the meanings below. The behavior they exercise is `specs/notetypes/change-notetype.md`. Names follow `notetype-ops.md` T1; the change runs as accepted, with no confirmation, and Meguri's safety backup goes to the scenario's backups folder (`notetype-ops.md` T9).

## H1. `change_notetype_info` (op)

The change screen opening on a selection with a target note type.

- **Argument** `{notes: [#nN, ...], notetype: <name>}`: at least one note reference; `notetype` the new note type by its stored name (`notetype-ops.md` T1; `unknown_ref` otherwise). A reference to a note deleted since is allowed: the refusals below cover it.
- **Effect.** None on the collection. The executor remembers the step's selection, old and new note type, default maps and schema stamp for the next `change_notetype` steps (H2), until the next `change_notetype_info` step replaces them. A refused step replaces them with nothing, so a `change_notetype` step right after it is `invalid_scenario`.
- **Output** `{change_notetype_info: <info>}`:

  ```
  {
    "old_notetype": <stored name>, "new_notetype": <stored name>,
    "old_fields": [<name>, ...], "new_fields": [<name>, ...],
    "old_templates": [<name>, ...], "new_templates": [<name>, ...],
    "fields": [<old ordinal> | null, ...],
    "templates": [<old ordinal> | null, ...] | null,
    "discarded_fields": [<old ordinal>, ...],
    "dropped_templates": [<old ordinal>, ...] | null,
    "removed_cards": <count>,
    "field_content": [<count>, ...],
    "card_counts": [[<ordinal>, <count>], ...],
    "schema": <col.scm>
  }
  ```

  - `fields` and `templates` are the default maps (`notetypes/change-notetype` M2, M3), `null` for none; `templates` is `null` when either note type is cloze (M1).
  - `discarded_fields`, `dropped_templates` and `removed_cards` are M6 for the default maps (`dropped_templates` is `null` exactly when `templates` is).
  - `field_content` has one count per old field, in ordinal order; `card_counts` lists the ordinals the selected notes' cards have, ascending, with their counts (M5). Anki has no such output: the oracle derives them from the stored rows by M5's definitions (a note's `flds` split at U+001F; a field counts when its text is not the empty string).
  - `schema` is the stamp `col.scm` as read (M5), an integer.
- **Rejections** (`dsl.md` D7.3), in M4's order: `not_found` (the first listed note does not exist), `multiple_notetypes` (another listed note does not exist or uses another note type).

## H2. `change_notetype` (op)

Saving the change screen: the change of `notetypes/change-notetype` M8 and M9 with the selection, the old and new note type and the schema stamp of the latest `change_notetype_info` step.

- **Argument** `{fields?, templates?}`:
  - `fields`: the field map, one entry per field of the new note type, each an old field ordinal or `null`. Missing: the info step's default field map.
  - `templates`: the template map, one entry per template of the new note type, each an old template ordinal or `null`, no ordinal twice; allowed only when the info step's `templates` is not `null`. Missing: the info step's default template map (or none).
  - A map of the wrong length, an ordinal that the old note type does not have, an ordinal twice in `templates`, `templates` given when the info step's is `null`, and a `change_notetype` step with no earlier successful `change_notetype_info` step are `invalid_scenario`. Both executors check before acting.
- **Effect.** The change, run as accepted (`notetype-ops.md` T9): the oracle never asks, and Meguri's executor submits the change with no confirmation input, which it does not have (`notetypes/schema-changes#G4`). On Meguri its safety backup (M8 check 5) goes to the scenario's backups folder (`backup-ops.md` F1); no change scenario observes it. The executor passes the remembered schema stamp; it does not read `col.scm` again. Changed 2026-10-27: the executor no longer passes a schema confirmation.
- **Output** `{change_notetype: {notes: <count>}}`: the number of notes changed (M9's result).
- **Rejections**, in M8's order: `no_changes` (M7), `schema_changed` (M8 check 3), `not_found` (M8 check 4: a listed note no longer exists), `multiple_notetypes` (M8 check 4; Meguri's only: no scenario reaches it, since only a schema change can move a note to another note type). A refused change writes nothing. Meguri's executor reports `SafetyBackupFailed` as a run error.
- **Logical ids.** Cards the change creates get no logical id, as the cards an edit adds (`note-ops.md` N3); scenarios read them with `db_exec`, naming notes by their sort field. Cards it deletes keep their logical ids: `card` reports `null` for them.

## H3. Oracle

The oracle runs the screen's calls through Anki's own operations:

- `change_notetype_info`: the selection check of Anki's Browse action (the note type shared by the notes, with M4's refusals), then Anki's change info for the old and new note type. The names, default maps and stamp come from it; the preview is computed from the default maps as Anki's change screen lists unused fields and templates (M6), and `removed_cards`, `field_content` and `card_counts` from the stored rows.
- `change_notetype`: `no_changes` as Anki's screen decides it (same note type, identity maps); otherwise Anki's change of note type with the remembered note ids, note types, stamp and maps. `schema_changed` is Anki's schema check (the oracle may compare the stamp with `col.scm` itself before the call: Anki refuses with a generic invalid-input error); `not_found` is Anki's missing-note error. Anki rolls the whole change back on either. The oracle never shows or answers a schema-change question (`notetype-ops.md` T9).

## H4. Status

As of 2026-10-27, the language, schemas and Meguri parser accept both ops. The `notetypes/change/` scenarios and their recorded goldens are on master. `notetypes/change/undo` (added 2026-11-10, `notetypes/change-notetype` M12) also needs `undo`, `redo` and `undo_status` and waits for `tools/mg oracle record`. They remain inactive until U35 supplies and verifies the missing adapters. Parser support does not accept U35.

Readiness of the three parts:

1. **Scenario language.** `specs/conformance/dsl.md` D7.2's op table defines `change_notetype_info` (`{notes, notetype}`, output H1, rejections `not_found`, `multiple_notetypes`) and `change_notetype` (`{fields?, templates?}`, output H2, rejections `no_changes`, `schema_changed`, `not_found`, `multiple_notetypes`, the last Meguri's only), each pointing here. `conformance/schema/scenario.schema.json` defines the two argument shapes (`notes` a non-empty list of note references, `notetype` a non-empty string; `fields` and `templates` lists of non-negative integers or `null`); `observation.schema.json` defines H1's info object (exactly its 14 members) and H2's `{notes}`, with the rejection kinds above. `protocol.json`'s step description lists both. `conformance/AGENTS.md` notes that `change_notetype` acts on what the latest `change_notetype_info` step kept, not on the collection at its own step, so edits meant to race the save go between the two.
2. **Oracle** (an oracle-maintainer session): the driver implements both ops as H3 says, with tests. The M12 scenarios and their goldens recorded at the current pin are now present.
3. **Meguri** (U35's implementer): the testkit's scenario model already reads both ops' typed arguments (`core/crates/meguri-testkit/src/scenario/`). U35's notetypes adapter must map them to U35's info query and change, which needs `setup.notes` and `add_note` (U27), `edit_note`, `delete_notes` and `answer`. Keep the scenarios inactive until the adapters pass their required checks.

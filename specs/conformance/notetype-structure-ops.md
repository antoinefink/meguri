# Note type structure ops

Spec: conformance/notetype-structure-ops
Requirements: R1883, R914, R918, R923, R924, R933
Anki: 26.09.3
Scenarios: conformance/scenarios/notetypes/structure/

The scenario ops and the observation kind that U34's scenarios add to `notetype-ops.md`: removing a note type, Restore to Default, and reading the template texts of a note type. Structure edits themselves go through `update_notetype` (`notetype-ops.md` T5 with U34's capabilities, T10.1). Both executors implement these with the meanings below. The behavior they exercise is `specs/notetypes/structure-changes.md` and `specs/notetypes/restore-to-default.md`. Names follow `notetype-ops.md` T1; schema changes run as accepted, with no confirmation, and Meguri's safety backups go to the scenario's backups folder (T9).

## M1. `remove_notetype` (op)

- **Argument** `{notetype}`: the note type (T1).
- **Effect.** Removes the note type as `notetypes/structure-changes` S12 says, as one undoable local change ("Remove Note Type"). Every executor reads the note type from the collection when the step runs, so rows a `db_exec` wrote are seen. Changed 2026-11-20 (round 9 review): Meguri's executor passes as confirmed counts the note type's notes and cards it reads at the step itself (`notetype-ops.md` T9).
- **Output** none (`{}`).
- **Rejections** none. Every scenario collection holds the stock kinds, so the note type removed is never the last one: Meguri's `LastNotetype` and Anki's re-adding of a Basic note type cannot occur. Meguri's executor reports `LastNotetype` or `SafetyBackupFailed` as a run error.
- **Errors.** An unknown name is `unknown_ref`.

## M2. `restore_notetype` (op)

- **Argument** `{notetype, kind?}`: the note type (T1); `kind` one of `basic`, `basic_and_reversed`, `basic_optional_reversed`, `basic_typing`, `cloze`, `image_occlusion` (the names of `notetype-ops.md` T3), absent when the scenario lets the stored kind decide.
- **Effect.** Restore to Default as `notetypes/restore-to-default` D1 to D6 say, with the executor's own stock kind (Anki's on the oracle, Meguri's on Meguri: their template text and CSS differ by design, R908), as one undoable local change ("Update Note Type"). The note type is read from the collection when the step runs. Changed 2026-11-20 (round 9 review): Meguri's executor passes as confirmed losses a `restore_summary` (`notetypes/restore-to-default` D7.1) it reads at the step itself, with its digest (`notetype-ops.md` T9).
- **Output** none (`{}`).
- **Rejection** `needs_stock_kind`: no `kind` given and the stored `original_stock_kind` is not 1 to 6 (D2). Meguri also refuses with `needs_stock_kind` when the stored stock kind's K1 `kind` differs from the note type's stored `kind` (`notetypes/restore-to-default` D2, deviation `restore-kind-mismatch`); the oracle checks only the range 1 to 6, so no scenario uses such a mismatch. Nothing changes (`dsl.md` D7.3). The oracle checks the stored value before calling Anki, whose own refusal there is a generic invalid-input error. Meguri's executor reports `SafetyBackupFailed` as a run error.
- **Observing.** Scenarios observe a restored note type with `notetype_structure`, never `notetype_templates` (M3) or a dump of `notetypes`, `fields` or `templates` (R1959).

## M3. `notetype_templates` (observation kind)

The template texts of note types the scenario defined, as stored.

- **Request** `{notetype_templates: {notetypes: [<name>, ...]}}`: a non-empty list of stored names (T1).
- **Value** `{<name>: [<template>, ...]}`, one member per name, the templates in ordinal order, each `{name, front, back, browser_front, browser_back}`: the template's name, `q_format`, `a_format`, `q_format_browser` and `a_format_browser` (`model/protobuf-records` P6.3) as stored, empty strings included.
- **What is read.** The stored rows when the step runs; nothing changes.
- **Only scenario note types.** A scenario requests only note types whose templates it wrote itself, and note types derived from them by its own edits. It never requests a stock kind, a clone of one or a restored note type: Meguri's stock text differs from Anki's by design, and Anki's would land in a golden (R908, R1959).
- **Errors.** An unknown name is `unknown_ref`; a malformed request is `invalid_scenario`.

## M4. Capabilities

`remove_notetype`, `restore_notetype` and `notetype_templates` are ops and a kind of their own (`dsl.md` D8). `update_notetype` steps need the capabilities of `notetype-ops.md` T10.1; U34 declares those listed for it there.

## M5. Status

As of 2026-10-27, the scenario language, schemas and Meguri parser accept these three interfaces. The S16 and D8 scenarios and their recorded goldens are on master. The scenarios added on 2026-11-10 (the undo legs, `remove-last`, `first-field-duplicates`) use only interfaces the oracle has and are recorded too. They remain inactive until U34 supplies and verifies the missing adapters. Parser support does not accept U34.

Readiness of the three parts:

1. **Scenario language.** `specs/conformance/dsl.md`'s op table defines `remove_notetype` (`{notetype}`, no output) and `restore_notetype` (`{notetype, kind?}`, no output, rejection `needs_stock_kind`), and `specs/conformance/observations.md` defines `notetype_templates`, each pointing here. `conformance/schema/scenario.schema.json` defines the two op shapes and the request of M3 (a `notetypes` list of names, nothing else); `observation.schema.json` defines M3's value (per name an array of objects with exactly the five string members) and `restore_notetype`'s rejection, limited to `needs_stock_kind`.
2. **Oracle** (an oracle-maintainer session): the driver implements `remove_notetype` with the collection's note type removal, `restore_notetype` with the collection's restore-to-stock call (the given kind as its forced kind; without one, the M2 check first), and `notetype_templates` from the stored templates, together with `protocol.json`'s step description. So that each op reads the stored rows (`notetype-ops.md` T1), it reopens the collection before `remove_notetype`, `restore_notetype` (after the M2 check, so a `needs_stock_kind` refusal leaves the history alone) and `update_notetype`, which empties Anki's undo and redo histories before the step: undoing that step still works, but no earlier step can be undone across it. `notetype_templates` and `notetype_structure` read through the backend getters that bypass the note type cache, with no reopen, so they leave both histories alone and may sit between a step and its undo or redo. The S16 and D8 scenarios and their recorded goldens are now present.
3. **Meguri** (U34's implementer): the testkit's scenario model already reads these shapes (`core/crates/meguri-testkit/src/scenario/`). U34's notetypes adapter must map `remove_notetype` to the removal op (S12), `restore_notetype` to the restore op (`restore-to-default` D1), `notetype_templates` to a read of the stored templates, and `update_notetype` edits beyond U29's plain save to the draft apply op (S1): the executor loads the stored aggregate as a draft whose fields and templates have their stored ordinals as origins, applies the edits in order (an added field or template has no origin; `sort_field` names the draft field it refers to), and submits it with the base token it loaded and `skip_checks` as given. No op takes a confirmation input. Keep the scenarios inactive until the adapters pass their required checks.

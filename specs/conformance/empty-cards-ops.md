# Empty Cards ops

Spec: conformance/empty-cards-ops
Requirements: R1883, R952, R1015
Anki: 26.09.3
Scenarios: conformance/scenarios/cardgen/empty/, conformance/scenarios/maintenance/empty-cards/

The two scenario ops of Empty Cards: the scan and the delete, the way the Empty Cards screen uses them (scan, show, then delete what it listed). Both executors implement them with the meanings below. The behavior they exercise is `specs/notes/empty-cards.md`.

## Y1. `empty_cards` (op)

- **Argument** `{}`.
- **Effect.** The scan of `notes/empty-cards` E2. Nothing in the collection changes. The executor remembers the scan's listed cards (real card ids, per note, in E2's order) and its all-empty flags for the next `delete_empty_cards` steps (Y2), until the next `empty_cards` step replaces them.
- **Output** `{empty_cards: [<note type>, ...]}`, in E2's note type order, `[]` when nothing is empty:

  ```
  {"notetype": <stored name>,
   "notes": [{"note": #nN, "ords": [<ordinal>, ...], "cards": <count>, "all_empty": <bool>}, ...]}
  ```

  - `notes` in ascending note id order; `note` is the note's logical id, or its real id as an integer when it has none.
  - `ords`: the ordinals of the note's empty cards, ascending (an ordinal appears once per empty card that has it). Cards are named by ordinal rather than logical id because the cards an edit or a change adds have none (`note-ops.md` N3).
  - `cards`: the note's number of cards; `all_empty`: E2's flag.
  - The labels of E2 (template names, card and cloze numbers) are not observed: they are display text.
- **Rejections**: none.

## Y2. `delete_empty_cards` (op)

The Empty Cards screen's Delete, for the cards the latest `empty_cards` step listed.

- **Argument** `{keep_notes: <bool>}`: the "Keep notes with no valid cards" option.
- **Effect.** The delete of `notes/empty-cards` E4 with every card the remembered scan listed and the option. A `delete_empty_cards` step with no earlier `empty_cards` step is `invalid_scenario`. A second delete for the same scan is allowed: its cards are gone and are skipped. On Meguri the delete op's descriptor carries the safety class of `notes/empty-cards` E6, so the executor takes U127's backup before the write phase, as for any caller; no observation reads backup files, and U131's tests check the backup and its order. On the oracle there is no backup.
- **Output** `{delete_empty_cards: {deleted: <count>, kept_valid: <count>}}`: the cards deleted and the listed cards kept because they became valid (E4).
- **Rejections**: none.

## Y3. Oracle

- `empty_cards` first reopens the collection, which drops Anki's note type cache and empties its undo history, then calls Anki's empty-cards report and keeps it. The report's notes carry their empty card ids (already sorted by ordinal, then id) and the all-empty flag; the oracle reads each card's ordinal and the note's card count from the stored rows and groups the notes under their note type's name, in the report's order.
- `delete_empty_cards` builds the card list as Anki's Empty Cards screen does from the kept report: for each note, every listed card, except that with `keep_notes` a note flagged all empty leaves out its first listed card. It removes them with Anki's removal of cards and the notes they leave empty, as one undoable operation: the `empty_cards` step of `undo-ops.md` V4. `deleted` is the number of cards that operation removed; `kept_valid` is always 0, since Anki deletes listed cards without looking again (E5).

## Y4. Status

As of 2026-10-27, the language, schemas and Meguri parser accept both ops. The `cardgen/empty/` scenarios and their recorded goldens are on master. They remain inactive until U35 supplies and verifies the missing adapters. Parser support does not accept U35.

U131's scenarios `maintenance/empty-cards/report`, `undo-redo` and `incremental-sync` (`notes/empty-cards` E7) use these ops, the last two with `undo`, `redo`, `undo_status` and, in the third, actors (`sync-ops.md`); they have no goldens yet. On the oracle the scan's reopen empties Anki's undo history (Y3) and the delete then records one `empty_cards` step (Y3, `undo-ops.md` V4), so an undo after a delete reaches that step on both sides when the scenario has emptied Meguri's history first (V5).

Readiness of the three parts:

1. **Scenario language.** `specs/conformance/dsl.md` D7.2's op table defines `empty_cards` (`{}`, output Y1, no rejection) and `delete_empty_cards` (`{keep_notes}`, output Y2, no rejection), each pointing here. `conformance/schema/scenario.schema.json` defines the two argument shapes (`{}` with no member; `keep_notes` a required boolean); `observation.schema.json` defines Y1's list (per note type exactly `notetype` and `notes`, per note exactly `note`, `ords`, `cards` and `all_empty`) and Y2's `{deleted, kept_valid}`. `protocol.json`'s step description lists both. `conformance/AGENTS.md` notes that `delete_empty_cards` acts on the cards the latest `empty_cards` step listed, not on the collection at its own step.
2. **Oracle** (an oracle-maintainer session): the driver implements both ops as Y3 says, with tests. The E7 scenarios and their goldens recorded at the current pin are now present. `conformance/deviations.yaml` contains E5's four `empty-cards-recheck` entries, with pointers and replacement values from `cardgen/empty/recheck`'s recorded steps.
3. **Meguri** (U35's implementer): the testkit's scenario model already reads both ops' typed arguments (`core/crates/meguri-testkit/src/scenario/`). U35's notes adapter must map them to U35's scan and delete. `cardgen/empty/occlusion` also needs `ensure_io_notetype` (`notetype-ops.md` T6), which the model already reads. Keep the scenarios inactive until the adapters pass their required checks.

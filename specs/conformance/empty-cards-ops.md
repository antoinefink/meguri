# Empty Cards ops

Spec: conformance/empty-cards-ops
Requirements: R1883, R952, R1015
Anki: 26.09.3
Scenarios: conformance/scenarios/cardgen/empty/

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
- **Effect.** The delete of `notes/empty-cards` E4 with every card the remembered scan listed and the option. A `delete_empty_cards` step with no earlier `empty_cards` step is `invalid_scenario`. A second delete for the same scan is allowed: its cards are gone and are skipped. The op takes no safety backup on either executor: U131's maintenance flow takes it before calling the delete (`notes/empty-cards` E6), and its scenarios are U131's.
- **Output** `{delete_empty_cards: {deleted: <count>, kept_valid: <count>}}`: the cards deleted and the listed cards kept because they became valid (E4).
- **Rejections**: none.

## Y3. Oracle

- `empty_cards` calls Anki's empty-cards report and keeps it. The report's notes carry their empty card ids (already sorted by ordinal, then id) and the all-empty flag; the oracle reads each card's ordinal and the note's card count from the stored rows and groups the notes under their note type's name, in the report's order.
- `delete_empty_cards` builds the card list as Anki's Empty Cards screen does from the kept report: for each note, every listed card, except that with `keep_notes` a note flagged all empty leaves out its first listed card. It removes them with Anki's removal of cards and the notes they leave empty, as one undoable operation. `deleted` is the number of cards that operation removed; `kept_valid` is always 0, since Anki deletes listed cards without looking again (E5).

## Y4. Status

Decided 2026-10-27: both ops are specified here. As of 2026-10-27 the oracle driver implements them (step 2 without its goldens); the schema shapes of step 1 and Meguri's executor (step 3) are still to come, so the scenarios of `cardgen/empty/` (`notes/empty-cards` E7) are not in `conformance/` and have no goldens. They are added in this order:

1. **Scenario language.** `specs/conformance/dsl.md` D7.2's op table gains `empty_cards` (`{}`, output Y1, no rejection) and `delete_empty_cards` (`{keep_notes}`, output Y2, no rejection), each pointing here. `conformance/schema/scenario.schema.json` gets the two argument shapes (`{}` with no member; `keep_notes` a required boolean); `observation.schema.json` gets Y1's list (per note type exactly `notetype` and `notes`, per note exactly `note`, `ords`, `cards` and `all_empty`) and Y2's `{deleted, kept_valid}`. `protocol.json`'s step description lists both. `conformance/AGENTS.md` notes that `delete_empty_cards` acts on the cards the latest `empty_cards` step listed, not on the collection at its own step.
2. **Oracle** (an oracle-maintainer session): the driver implements both ops as Y3 says, with tests. A spec writer then adds the scenarios E7 lists, and the oracle records their goldens at the current pin. `conformance/deviations.yaml` gets the four `empty-cards-recheck` entries of `notes/empty-cards` E5 with `cardgen/empty/recheck`'s golden, their pointers and replacement values taken from that golden's steps.
3. **Meguri** (U35's implementer): the testkit's scenario model reads both ops' typed arguments (`core/crates/meguri-testkit/src/scenario/`); the notes adapter maps them to U35's scan and delete. `cardgen/empty/occlusion` also needs `ensure_io_notetype` (`notetype-ops.md` T6), which the model already reads. Until the model reads the two ops, T0 reports the scenarios that use them as `invalid`; until the adapter has them, `pending`.

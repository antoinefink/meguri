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
  - The output carries no labels and no first-field text of E2. The stripped note type name and the stripped template-name labels are the `empty_cards_labels` observation (Y5, planned). The first-field snapshot and the card and cloze number wording stay unobserved: they are display text.
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

As of 2026-11-17, the language, schemas and Meguri parser accept both ops. The eight `cardgen/empty/` scenarios and their goldens, recorded at the current pin, are on master. They remain inactive until U35 supplies and verifies the missing adapters (`conformance/scenarios/cardgen/area.yaml` activates only two `position/` cases). Parser support does not accept U35.

U131's scenarios `maintenance/empty-cards/report`, `undo-redo` and `incremental-sync` (`notes/empty-cards` E7) use these ops, `undo-redo` with `undo`, `redo` and `undo_status`, and `incremental-sync` with actors (`sync-ops.md`). Their goldens are recorded at the current pin. They stay inactive (`conformance/scenarios/maintenance/area.yaml` activates only `optimize/*`) until U131's flow passes them. Y5's `empty_cards_labels` is planned and has no scenario yet. On the oracle the scan's reopen empties Anki's undo history (Y3) and the delete then records one `empty_cards` step (Y3, `undo-ops.md` V4), so an undo after a delete reaches that step on both sides when the scenario has emptied Meguri's history first (V5).

Readiness of each part:

1. **Scenario language.** `specs/conformance/dsl.md` D7.2's op table defines `empty_cards` (`{}`, output Y1, no rejection) and `delete_empty_cards` (`{keep_notes}`, output Y2, no rejection), each pointing here. `conformance/schema/scenario.schema.json` defines the two argument shapes (`{}` with no member; `keep_notes` a required boolean); `observation.schema.json` defines Y1's list (per note type exactly `notetype` and `notes`, per note exactly `note`, `ords`, `cards` and `all_empty`) and Y2's `{deleted, kept_valid}`. `protocol.json`'s step description lists both. `conformance/AGENTS.md` notes that `delete_empty_cards` acts on the cards the latest `empty_cards` step listed, not on the collection at its own step.
2. **Oracle** (an oracle-maintainer session): the driver implements both ops as Y3 says, with tests. The E7 scenarios and their goldens recorded at the current pin are now present. `conformance/deviations.yaml` contains E5's four `empty-cards-recheck` entries, with pointers and replacement values from `cardgen/empty/recheck`'s recorded steps.
3. **Meguri** (U35's implementer): the testkit's scenario model already reads both ops' typed arguments (`core/crates/meguri-testkit/src/scenario/`). U35's notes adapter must map them to U35's scan and delete. The `cardgen/empty/` scenarios also use these ops and observations, each of which needs its own adapter before the scenario can pass:
   - `edit_note` (every scenario but `special-front-edit`), `db_exec` (every scenario but `occlusion`) and `advance` (every scenario but `occlusion` and `scan`);
   - `delete_notes` (`scan`, `recheck`) and `add_note` (`occlusion`);
   - `ensure_io_notetype` (`occlusion`; `notetype-ops.md` T6);
   - `update_notetype` (`after-template-change`, `special-front-edit`; U34);
   - `undo`, `redo` and the `undo_status` observation (`undo`; `undo-ops.md`);
   - the `card` and `db_dump` observations.

   The model reads all of them. Keep the scenarios inactive until the adapters pass their required checks.
4. **`empty_cards_labels`** (Y5): not in the language yet. It needs, together: its row in `observations.md` O4, its value in `conformance/schema/observation.schema.json` and in the kind list of `protocol.json`, the testkit model, and the driver (an oracle-maintainer session). Only then can the scenario `maintenance/empty-cards/labels` (Y5.4) be written and recorded. Until then the scenario and its three harvest rows (`harvest-followups.md` H2, owner U131) stay planned.

## Y5. `empty_cards_labels` (observation, planned)

Planned 2026-11-17: no schema, model or driver support yet (Y4 item 4). It observes the two parts of E2's report text that are not display wording: the note type name and the card labels made from template names. Both are user text after `text/strip-html` H4.

### Y5.1 Request and value

- **Request** `empty_cards_labels`, with no argument, at most once per step. It reads the scan that the latest `empty_cards` step kept (Y1), not the collection at its own step: a rename after that scan does not change the value. Without an earlier `empty_cards` step since the start or the latest `handoff`, it is `invalid_scenario`, as for Y2.
- **Value** a list in Y1's note type order, one member per note type that Y1 lists, with its notes in Y1's order:

  ```
  {"notetype": <stored name>,
   "label": <the name stripped by H4>,
   "notes": [{"note": #nN, "labels": <the joined labels stripped by H4> | null}, ...]}
  ```

  - `notetype` and `note` are as in Y1, so the two outputs join.
  - `labels` is E2's label text for a standard note type: the stored names of the templates of the note's empty ordinals, joined with ", " in E2's card order, then stripped as one string.
  - `labels` is `null` when the note's note type is a cloze note type, or when one of its empty ordinals has no template. Those labels carry Anki's wording ("Cloze", "Card <number>"), which is localized display text (R1887), and their numbers are already Y1's `ords`.
- Never changes the collection.

### Y5.2 Meguri

`label` and `labels` come from the data U131's report keeps for that scan: E2's stripped note type name and stripped joined labels. They are the strings U131's report and U163's screen show.

### Y5.3 Oracle

- The `empty_cards` step also keeps the report's text (the `report` field of Anki's empty-cards report, English locale). The observation reads that text as a plain string. It does not parse it as HTML: H4 decodes entities, and the report inserts the decoded text without escaping it again, so a stored `&lt;` comes out as a literal `<`.
- `label`: per note type, in order, the text between the literal `<div><b>Empty cards for ` and the next `:</b></div><ol>`.
- `labels`: per note, the text after that note's `[anki:nid:<note id>] ` marker and the first ` cards empty (` after it, up to the `).</li>` that ends the item. The note id comes from the kept report. The oracle sets `null` itself for the cases of Y5.1, from the note type kind and template count it reads when the `empty_cards` step runs.
- A scenario's note type and template names must not produce, after H4, any of these delimiter strings.

### Y5.4 Scenario (planned)

`maintenance/empty-cards/labels` (U131), with its own note types. After one `empty_cards` step, it observes `empty_cards_labels` with:

- a note type and a template with plain names, which reach the report unchanged;
- a note type name and a template name that each hold a script element;
- names that hold an element with an event-handler attribute and a script element, with text around them: the markup goes and the text stays;
- E2's cross-name case: one note whose two empty cards' templates are named `a<b` and `c>d`, which gives `ad`;
- a stored entity in a name (`&amp;`), which comes out decoded;
- a cloze note, whose `labels` is `null`.

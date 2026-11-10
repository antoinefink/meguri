# Card excerpt ops: the `card_excerpts` observation

Spec: conformance/card-excerpt-ops
Requirements: R1883, R1884, R1887; supports R96, R1119, R1122, R1268
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/columns/question-deck/, conformance/scenarios/browser/modes/rows-excerpts

One observation kind (`observations.md` O4) for the card excerpts call of `specs/browser/columns.md` C16: the Question and Deck cells of listed cards, in request order, with missing and damaged cards in their places. Both executors implement it with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold its shapes. The oracle runs Anki's collection library, not its desktop browser: it reads the cells from Anki's Browse rows, which Anki computes in its backend.

## E1. Request

- `{card_excerpts: [<card reference>, ...]}`: a non-empty list of card references (`#cN`). A reference may appear more than once; each place gets its own entry.
- At most one `card_excerpts` request per observe step. There is no text form (`observations.md` O2): `card_excerpts(#c1)` is invalid.
- A reference to a card that does not exist yet is `unknown_ref`. Any other shape (an empty list, a note reference, a mapping) is `invalid_scenario`.

## E2. Value

`{"card_excerpts": [<entry>, ...]}`, one entry per reference, in request order:

| Entry | When (`browser/columns` C16.1) |
|---|---|
| `{"question": <string>, "deck": <string>}` | found: `question` is the Question cell's text (C10 step 3), `deck` the Deck cell (C7.4) |
| `{"error": "missing"}` | missing: no card has the reference's id (its note was deleted, for example) |
| `{"error": "damaged"}` | damaged: the card exists but its note, note type, deck, home deck or template does not (C3.1) |

Strings compare exactly (`observations.md` O7), the empty string included. No localized text is part of the value (R1887).

## E3. Oracle

- Each reference is resolved to its card's real id. For each, in request order, the driver asks Anki's backend for that id's Browse row with the active columns set to `question` and `deck` for the request (backend state that is never stored), and reports the two cell texts exactly as Anki returns them. Anki adds no isolation marks or other text to these two cells.
- Anki reads a row as a card or as a note by the config key `browserTableShowNotesMode`. The driver reads that key (`browser/config-keys` K2.1) at each `card_excerpts` step, before the first row; when it reads as true, the step is `invalid_scenario`. A scenario therefore never has the key read as true at a `card_excerpts` step: it may write it with `set_config` only after its last `card_excerpts` step, or write `false`. Meguri's call is cards mode always (`browser/columns` C16.1) and never reads the key.
- Errors per row: Anki's error for a card that no longer exists → `missing`. Its not-found error (a missing note, note type, deck, home deck or template) or its error asking for Check Database → `damaged`. Any other failure is a run error (`anki_error`). One row's error never stops the other rows.
- Anki writes the cells of a blank cloze card and of a template error as localized messages, which cannot be told from question text. Scenarios avoid both (E5); the spec writer checks every recorded golden for them.
- Nothing in the collection changes: no row is written, `col.mod` does not move.

## E4. Meguri

- The executor resolves the references to real ids and makes one card excerpts call (`browser/columns` C16.1) with them, in request order, through a handle of the current generation. It maps each record to its entry: found → `{question, deck}` with the question's text; missing → `{error: "missing"}`; damaged → `{error: "damaged"}`.
- A found record whose question is not text (blank or template error, C16.2) has no entry form here: the step fails with a run error that names the outcome. Scenarios avoid them (E5); Meguri's engine tests cover them.
- The call itself is the product's entry point (the one the FFI export wraps), not a Browse row: the observation tests what the review summary receives.

## E5. What scenarios hold

- Note types, decks and notes come from `setup` (`dsl.md` D6.3 to D6.5). Browser question formats are set after the setup with `update_notetype` `template` edits (`notetype-ops.md` T5), a plain save that changes no card.
- A filtered card is a card whose `did` names an ordinary deck and whose `odid` and `odue` hold its home deck and its due, written with `db_exec` (`record-ops.md` W1). The Deck cell reads only the two deck names, so it is the same as for a card of a real filtered deck, which the setup cannot create.
- Damaged cards are made with `db_exec`: a card's `did` or `odid` set to an id no deck has, a card's `ord` set past the templates of its standard note type, a note's `mid` set to an id no note type has, or a note's row removed. Scenarios never rewrite or remove note type or deck rows with `db_exec`: Anki keeps them cached across raw writes, so its rows would not see the change.
- Missing cards are cards of notes removed with `delete_notes` (`record-ops.md` W3).
- Scenarios avoid blank cloze cards, template errors and notes mode at a `card_excerpts` step (E3): a `set_config` of `browserTableShowNotesMode` to true comes after the last such step, if at all.
- A scenario that reads `col.mod` or stamps follows `specs/sched/day-clock.md` D14.2 (it starts with `observe: [timing]`) and moves the clock before the observation, so that a write by the call would carry a later stamp than everything the setup wrote.

## E6. Status

- Oracle: the driver implements E1 to E3. Every scenario under `conformance/scenarios/browser/columns/question-deck/` has its golden in `conformance/goldens/browser/columns/question-deck/`, recorded with `tools/mg oracle record browser/columns/question-deck`; none holds a localized message. `browser/modes/rows-excerpts` (`browser/result-sets` H16) compares the kind with `browser_rows` and has no golden yet.
- Meguri: the testkit model reads the kind (`core/crates/meguri-testkit/src/scenario/`) and the browser adapter (`core/crates/meguri-testkit/src/exec/browser.rs`) implements it with U85a's card excerpts call. Until the model reads it, T0 reports the scenarios that use it as `invalid`; U85a's implementer adds the model support with the adapter and marks the scenarios active in `conformance/scenarios/browser/area.yaml`.
- What else the scenarios need on Meguri (`dsl.md` D8): the new collection and the `decks` setup section (U28), the `notetypes` and `notes` sections (U29, U27), `update_notetype` plain saves (U29), `delete_notes` (U27), `db_exec` and the `timing` observation.

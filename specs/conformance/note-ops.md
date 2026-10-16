# Note ops: adds, raw template and deck edits, and the field check

Spec: conformance/note-ops
Requirements: R1883, R801, R807, R808
Anki: 26.09.3
Scenarios: conformance/scenarios/notes/, conformance/scenarios/cardgen/

What the note and card-generation scenarios rely on from the existing ops, and the `fields_check` observation kind they add. The behavior they exercise is `specs/notes/`. Both executors implement the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold the shapes.

## N1. `add_note` and `setup.notes`

- An `add_note` step, and each `setup.notes` entry, is an Add-screen add: Meguri's executor adds with `update_add_defaults` true (`specs/notes/note-records.md` A4), so the step writes `curModel`, `_nt_<id>_lastDeck` and `_deck_<id>_lastNotetype` as Anki does. Adds without the flag (local servers) have no scenario op.
- `deck` names a deck of `setup.decks`; the executor passes the id that deck got during setup, whatever happened to it since (a `db_exec` may have deleted it or turned it into a filtered deck). So `deck: Gone` after Gone's row was deleted adds with a missing deck id. Without `deck`, the deck id is 1.
- Each new card gets the next `#cN` in ordinal order (`dsl.md` D7.1).

## N2. Raw edits of note types and decks

No op sets a template's deck override, makes a filtered deck or stores a template text that a save would refuse. Scenarios write such states with `db_exec` (`record-ops.md` W1) and then `reopen` (`sync-ops.md` S6), because Anki keeps note types and decks cached across operations and would not see the rows otherwise. Meguri's executor reads them from the file as for any `db_exec`.

- **Deck override**: append protobuf field 5 (varint, wire type 0: tag byte `28`) holding the deck id to the template's `templates.config` blob; a later occurrence replaces an earlier one (`specs/model/protobuf-records.md` P2). Scenarios build the bytes from the deck's id:

  ```sql
  UPDATE templates SET config = CAST(config || unhex('28' || (SELECT printf('%02x%02x%02x%02x%02x%02x',
    (id & 127) | 128, ((id >> 7) & 127) | 128, ((id >> 14) & 127) | 128,
    ((id >> 21) & 127) | 128, ((id >> 28) & 127) | 128, id >> 35)
    FROM decks WHERE name = '<deck>')) AS BLOB)
  WHERE ord = <ordinal> AND ntid = (SELECT id FROM notetypes WHERE name = '<note type>')
  ```

  (a six-byte varint, right for ids from 2^35 to 2^42, which deck ids made from the clock are). `unhex` is SQLite's own function (3.41 and later), which both executors' SQLite has.
- **Template text**: append protobuf field 1 (tag byte `0a`, then the length and the UTF-8 bytes) to replace the front.
- **Filtered deck**: replace a deck's `kind` blob with a filtered kind (the bytes of `model/proto/deck-filtered-bytes`).
- **Deleted deck**: `DELETE FROM decks WHERE name = '<deck>'` after any override that should point to it is written.

## N3. Cards that edits add

`edit_note` (`record-ops.md` W2) assigns no logical id to the cards a save adds, on both executors. Scenarios read cards with `db_exec`, one row per card, naming the note by its sort field: `SELECT n.sfld, c.ord, … FROM cards c JOIN notes n ON n.id = c.nid ORDER BY n.id, c.ord, c.id`, and deck names with `replace(d.name, char(31), '::')`. Real ids of the cards of one cloze note are never observed, since Anki creates them in an unspecified order (`specs/notes/card-generation.md` G8).

## N4. `fields_check` (observation kind)

The field check of `specs/notes/field-check.md` on drafts. It reads the collection and changes nothing.

- **Request** `{fields_check: {cases: [<case>, ...]}}`, at least one case, at most one request per observe step. A case is one of:
  - `{notetype: <name>, fields: <fields>}`: a new note's draft. `notetype` is a note type the collection holds, by its stored name; `fields` as in `add_note` (`dsl.md` D7.2: a list from the first field, missing trailing fields empty, or a mapping of field names), values as given, unsaved.
  - `{note: #nN, fields?: <fields>}`: the draft of an existing note: its stored fields, with `fields` replacing as in `edit_note` (`record-ops.md` W2). The note must exist (`unknown_ref` otherwise).
- **Value** `[<result>, ...]`, one per case, in case order. A result is `{state: <state>}` with `state` one of `normal`, `empty`, `duplicate`, `missing_cloze`, `notetype_not_cloze`, `field_not_cloze` (`field-check.md` K3), and, only when `state` is `duplicate`, `duplicates`: the logical ids (`#nN`) of the notes of K4, in ascending real note id order.
- **Oracle.** It builds the draft as a note object (a new note of the note type, or the stored note with the replaced fields), takes the state from Anki's note fields check, and for `duplicate` runs Anki's duplicate search for the note type and the draft's first field (`dupe:`), removing the draft's own note id and mapping the rest to logical ids.
- **Meguri.** Its executor calls the field check (U27) with the same draft.
- **Errors.** A malformed request, an unknown field name or a list longer than the note type's fields is `invalid_scenario`; an unknown note type name or note reference is `unknown_ref`. No rejection kinds.

## N5. Status

- `add_note`, `edit_note`, `delete_notes`, `db_exec`, `reopen`, `set_config`, `legacy_update`, `answer` and `check_db` are stable; every `cardgen/` and `notes/` scenario that uses only them has a golden.
- `fields_check` is new. `observations.md` O4 lists the kind and the schemas hold its shapes. U27's oracle-maintainer work adds it to the oracle driver, to `protocol.json` and to the testkit's scenario model (the typed request, which both executors parse), then records the goldens of `notes/fields-check/states` and `notes/fields-check/duplicates` (`tools/mg oracle record notes/fields-check`). Until the testkit model reads the kind, `tools/mg goldens-check` and T0 report the two scenarios `invalid` ("unknown observation kind"); until their goldens exist, `missing`.
- Meguri's executor: the notes adapter (U27's implementer) implements `edit_note` and `delete_notes` (`record-ops.md` W5), the Add-screen flag of N1, and `fields_check` through U27's field check. Until then the scenarios are `pending`.

# Note type ops

Spec: conformance/notetype-ops
Requirements: R1883, R903, R905, R906, R907, R909, R913, R916, R917, R942, R943, R949, R950, R1475
Anki: 26.09.3
Scenarios: conformance/scenarios/notetypes/

The scenario ops and the observation kind the note type scenarios need: adding note types (from scratch, stock, clone), saving edits to one, ensuring the Image Occlusion note type, and reading a note type's structure without its template text or CSS. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The behavior they exercise is `specs/notetypes/`.

## T1. Naming note types in steps

- Every op and observation here names a note type by its name as stored at that moment, compared exactly (no case folding). A name that matches no note type is a run error (`unknown_ref`); a name that two note types share cannot happen (stored names are unique under unicase).
- `add_note` (`dsl.md` D7.2) may name any note type the collection holds at that moment by its stored name, the stock kinds and those added by these ops included, not only those of `setup.notetypes`. The executor reads the note type when the step runs.
- Ops that create or rename a note type report its stored name, which can differ from the requested one (`+` appended, `"` removed, NFC): later steps use the reported name.
- Every op reads the note types it changes from the collection when the step runs, so rows a `db_exec` wrote before are seen (`record-ops.md` W1).

## T2. `add_notetype` (op)

Adds a note type from scratch (`notetypes/records` N5.1) as one undoable local change, with the template checks (a checked save).

- **Argument** `{name, kind?, fields, templates, css?}`, as a `setup.notetypes` entry (`dsl.md` D6.4): `kind` `standard` (default) or `cloze`; `fields` a non-empty list of names; `templates` a non-empty list of `{name, front?, back?}` (texts empty when missing); `css` a string, `""` when missing.
- **Effect.** The note type of N5.1 with these values: new fields as N5.2 (fresh random ids), new templates as N5.3 (fresh random ids), `original_stock_kind` 1, the default LaTeX header and footer, `curModel` set to the new id. N4 normalizes the names.
- **Output** `{add_notetype: {name: <stored name>}}`.
- **Rejections** T7.

## T3. `add_stock_notetype` (op)

"Add: <stock kind>" (R916; `notetypes/stock-kinds` K8). For scenarios about the stock kinds (R1959).

- **Argument** `{kind, name?}`: `kind` one of `basic`, `basic_and_reversed`, `basic_optional_reversed`, `basic_typing`, `cloze`, `image_occlusion`; `name` defaults to the kind's name (`notetypes/stock-kinds` K1).
- **Effect.** Adds the executor's own version of that stock kind (Anki's on the oracle, Meguri's on Meguri; their template text and CSS differ by design, R908) under that name, as one undoable local change: local stamps, `curModel` set to the new id.
- **Output** `{add_stock_notetype: {name: <stored name>}}`.
- **Rejections** T7 (`invalid_notetype` for a name made only of `"`).

## T4. `clone_notetype` (op)

"Clone: <name>" (R916; `notetypes/records` N7).

- **Argument** `{notetype, name?}`: the source note type (T1); `name` defaults to `<source name> copy`.
- **Effect.** Adds a copy of the source as N7 says (same field and template ids, `original_id` cleared, new id, local stamps, template rows' `mtime_secs` and `usn` 0, `curModel` set to the clone), as one undoable local change, with the template checks.
- **Output** `{clone_notetype: {name: <stored name>}}`.
- **Rejections** T7.

## T5. `update_notetype` (op)

Applies a list of edits to a note type and saves the result once, as one undoable local change: the way an editor's draft is saved.

- **Argument** `{notetype, edits, skip_checks?}`: the note type (T1); `edits` a list, possibly empty, of the edits below, applied in order to a draft of the stored note type; `skip_checks` (default `false`) saves without the template checks, as the field editor does (`notetypes/validation` V1).
- **Edits.** Each is a mapping with exactly one key. Field and template names refer to the draft as the earlier edits left it, compared exactly; an unknown name is a run error (`unknown_ref`).

  | Edit | Meaning |
  |---|---|
  | `{rename: <name>}` | renames the note type |
  | `{css: <text>}` | sets the styling |
  | `{sort_field: <field>}` | makes that field the sort field; it must be a field of the stored note type, not one an earlier edit of this update added (`invalid_scenario` otherwise) |
  | `{add_field: <name>}` | appends a new field with the defaults of `notetypes/records` N5.2 (fresh random id) |
  | `{remove_field: <field>}` | removes the field |
  | `{rename_field: {from, to}}` | renames the field |
  | `{move_field: {field, to}}` | moves the field to position `to` (0-based) of the draft's field list; `to` must be a position of that list |
  | `{field_options: {field, sticky?, rtl?, font_name?, font_size?, description?, plain_text?, collapsed?, exclude_from_search?}}` | sets the given options (`model/protobuf-records` P6.2 names) |
  | `{add_template: {name, front?, back?}}` | appends a new template (N5.3, fresh random id; texts empty when missing) |
  | `{remove_template: <template>}` | removes the template |
  | `{rename_template: {from, to}}` | renames the template |
  | `{move_template: {template, to}}` | moves the template to position `to` (0-based) |
  | `{template: {name, front?, back?, browser_front?, browser_back?, browser_font_name?, browser_font_size?, deck?}}` | sets the given values of the named template; `deck` is a deck name (the default deck, `Default`, or one of `setup.decks`) for the deck override, or `null` to clear it |

- **Effect.** The draft is saved through the note type save that Anki's note type editors use: `notetypes/records` N3 and N4, the checks unless `skip_checks`, and every effect on notes, cards and `col.scm` that the save has (`notetypes/schema-changes` G2; U34 for notes and cards). A field or template kept from the stored note type keeps its identity (its id, options and, for a template, its row's `mtime_secs` and `usn`) wherever the edits move it; the save matches them by identity, never by name.
- **Output** `{update_notetype: {name: <stored name after the save>}}`.
- **Rejections** T7.
- **Meguri's executor** maps an update to Meguri's ops: an update whose edits are only `rename`, `css`, `field_options`, `rename_template` and `template` without `front` or `back` is a plain save (U29); any other is U34's draft apply. Schema changes run confirmed (T9). Until U34 lands, scenarios that need its edits are `pending`.

## T6. `ensure_io_notetype` (op)

The on-demand Image Occlusion note type (R913; `notetypes/records` N9).

- **Argument** `{}`.
- **Effect.** When the collection has an Image Occlusion note type (N9: the lowest id among note types with `original_stock_kind` 6 and at least 4 fields), nothing. Otherwise adds the executor's stock Image Occlusion kind named `Image Occlusion` (`+` appended on a collision) with local stamps, keeping `curModel`.
- **Output** `{ensure_io_notetype: {name: <the Image Occlusion note type's stored name>}}`, after the effect.

## T7. Rejections

When the save refuses (`dsl.md` D7.3), the step records `{<op>: {error: <kind>}}` and nothing changes. The kinds, for `add_notetype`, `add_stock_notetype`, `clone_notetype` and `update_notetype`:

| Kind | When (`notetypes/validation`) |
|---|---|
| `invalid_notetype` | V2: no field, no template, or a name the normalization refuses |
| `template_parse_error` | V3 step 2.1 |
| `duplicate_front` | V3 step 1 |
| `no_front_field` | V3 step 2.2 |
| `unknown_field` | V3 step 2.3 |
| `missing_cloze` | V3 step 3 |

When several apply, the first in V3's order is the kind, after V2. Rejections compare by kind only.

## T8. `notetype_structure` (observation kind)

The structure of note types, without template text, CSS, LaTeX values or ids, so that stock kinds can be compared between executors (R909, R1959).

- **Request** `notetype_structure` (every note type of the collection) or `{notetype_structure: {notetypes: [<name>, ...]}}` (T1).
- **Value** `{<stored name>: <structure>, ...}`, one member per note type, where a structure is:

  ```
  {
    "kind": "standard" | "cloze",
    "original_stock_kind": <integer>,
    "original_id": <integer> | null,
    "sort_field": <integer>,
    "fields": [{"name", "sticky", "rtl", "font_name", "font_size", "description",
                "plain_text", "collapsed", "exclude_from_search",
                "tag": <integer> | null, "prevent_deletion"}, ...],
    "templates": [{"name", "deck": <deck name> | <integer> | null,
                   "requirement": {"kind": "none" | "any" | "all", "fields": [<integer>, ...]} | null}, ...]
  }
  ```

  - `fields` and `templates` are in ordinal order. Field option names are those of `model/protobuf-records` P6.2; `tag` is `null` when the field has none; `original_id` is `null` when absent.
  - `deck` is the deck override: `null` when `target_deck_id` is 0, the deck's human name (`::`) when that deck exists, the id as an integer otherwise.
  - `requirement` is the stored `reqs` entry whose `card_ord` is the template's position (`notetypes/records` N6), `fields` ascending as stored; `null` when there is none.
- **What is read.** The stored records when the step runs. Nothing changes.
- **Errors.** An unknown name is a run error (`unknown_ref`).

## T9. Schema changes in scenarios

- An op that is a schema change runs as if the user accepted the warning: the oracle never asks, and Meguri's executor submits it with `schema_confirmed` true (`notetypes/schema-changes` G4). The gate's refusal is tested outside scenarios.
- Scenarios observe `col.scm`, `col.ls` and `col.mod` with `db_exec` (`record-ops.md` W1). They put the collection in the state right after a sync with `db_exec: {sql: "UPDATE col SET ls = scm"}` and then advance the clock before the first op, so that a change sets `scm` above `ls` (under a frozen clock a change in the same millisecond leaves `scm` equal to `ls`).

## T10. Status

- These ops and the kind are stable (`dsl.md` D7.2, `observations.md` O4): the oracle driver implements T2 to T8 through Anki's own note type operations, with the meanings above, and the scenarios that use them have goldens. The protocol's step description lists the ops and the kind.
- **Meguri** (U29's implementer, with the adapters): the testkit model reads these shapes (`core/crates/meguri-testkit/src/scenario/`), and the notetypes adapter implements `add_notetype`, `add_stock_notetype`, `clone_notetype`, `ensure_io_notetype`, the plain-save part of `update_notetype` and `notetype_structure`; U34 adds the rest of `update_notetype`. Until the testkit model reads them, T0 reports the scenarios that use them as `invalid`.

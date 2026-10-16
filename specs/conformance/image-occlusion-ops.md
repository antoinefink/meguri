# Image-occlusion observation

Spec: conformance/image-occlusion-ops
Requirements: R1883, R1907, R990, R1003, R1004, R1005, R1006, R1007, R1008
Anki: 26.09.3
Scenarios: conformance/scenarios/render/image-occlusion/format/

The `image_occlusion` observation kind evaluates two image-occlusion functions of `specs/render/image-occlusion-format.md` on a list of inputs: Anki's raw reading of an Occlusion field and its field-role lookup. Both executors implement it with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold its shapes. Meguri's strict reading, writer and number rules have no oracle counterpart and are not part of it.

## J1. `image_occlusion` (observation kind)

- **Pure.** It never reads or changes the scenario's collection, so it may appear in any observe step, with or without setup (like `text`, `observations.md` O9).
- **Request** `{image_occlusion: {fns: [<function>, ...], inputs: [<input>, ...]}}`: `fns` is a non-empty list of distinct function names from J2; `inputs` is a list, possibly empty. Every function runs on every input.
  - `parse` takes strings: Occlusion field texts. A string must be in Unicode NFC and must not contain a character below U+0020 other than TAB (U+0009) and LF (U+000A): the oracle stores each input in a note, and a note save changes such text.
  - `fields` takes lists, one per note type: the `tag` of each of its fields in field order, each an integer from 0 to 4294967295 or `null` for no tag. A list has at least one item. `fields` shares a request with no other function.
  - A step may hold several `image_occlusion` requests when their functions differ; their values merge (`observations.md` O2).
- **Value** `{<function>: [<output>, ...], ...}`: one member per requested function, an array with one output per input, in input order.
- **Errors.** A malformed request (an unknown or repeated function, `fields` with another function, an input of the wrong shape, a string outside the rule above, a tag out of range) is `invalid_scenario`. No function refuses a valid input.

## J2. Functions

| Function | Input | Output | Defined in |
|---|---|---|---|
| `parse` | string | `{"hide_all": <bool>, "numbers": [{"number": <int>, "shapes": [{"shape": <string>, "props": [[<key>, <value>], ...]}, ...]}, ...]}` | `render/image-occlusion-format#F4`, `#F5` |
| `fields` | list of tags | `{"occlusions": <int>, "image": <int>, "header": <int>, "back_extra": <int>}`, or `null` when the note type has no field roles | `render/image-occlusion-format#F2` |

- `parse`: `numbers` lists, in ascending order, each number that has a shape; `shapes` lists that number's shapes in field order; `shape` is the shape name and `props` its properties as `[key, value]` pairs in the order read, values unescaped, repeated keys kept. `hide_all` is the mode of F5. A field with no shape gives `{"hide_all": false, "numbers": []}`.
- `fields`: the four field indexes (from 0) of the roles Occlusion, Image, Header and Back Extra. The member names are those of Anki's protobuf message for these indexes.

## J3. Executors

- **Oracle.**
  - `parse`: on the worker's scratch collection, a cloze note type the driver defines (no stock note type, R1959) with four untagged fields, so that the field roles are positions 0 to 3, and a template using `{{cloze:}}` on the first field. For each input, it adds a note whose first field holds the input and whose other fields are empty, checks that the stored field equals the input (a difference is an internal error), reads the note with Anki's image-occlusion note query (the call Anki's mask editor uses to load a note), and removes the note. Anki returns the numbers in no fixed order: the driver sorts them ascending and keeps each number's shapes and properties in the order Anki gives them.
  - `fields`: for each input, the driver adds to the scratch collection a cloze note type with that many fields, field i carrying tag i of the input (none for `null`), asks Anki's image-occlusion field query for its indexes, and removes the note type. When Anki answers with its error for a note type without field roles (it asks for a database check), the output is `null`; any other error is an internal error.
- **Meguri.** The render adapter of `meguri-testkit` calls the raw reading of `meguri-template` (F4, F5) for `parse`, and the field-role lookup of `meguri-engine` (F2) on note type fields carrying the given tags for `fields`.

## J4. Scenarios

`conformance/scenarios/render/image-occlusion/format/*.yaml` (`render/image-occlusion-format#F16`). Except `cards`, they define no note type and need no setup.

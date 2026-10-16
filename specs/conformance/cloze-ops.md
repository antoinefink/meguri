# Cloze observation

Spec: conformance/cloze-ops
Requirements: R1883, R1907, R1909, R963, R969, R970, R971, R972, R973, R974, R975, R977
Anki: 26.09.3
Scenarios: conformance/scenarios/render/cloze/

The `cloze` observation kind evaluates the cloze functions of `specs/render/cloze.md` on a list of inputs. Both executors implement it with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold its shapes.

## Z1. `cloze` (observation kind)

- **Pure.** It never reads or changes the scenario's collection, so it may appear in any observe step, with or without setup (like `text`, `observations.md` O9).
- **Request** `{cloze: {fns: [<function>, ...], inputs: [<input>, ...]}}`: `fns` is a non-empty list of distinct function names from Z2; `inputs` is a list, possibly empty. Every function runs on every input.
  - `numbers` takes strings and shares a request with no other function.
  - Every other function takes pairs `[<text>, <card>]`: a string and an integer card number from 1 to 65535 (`render/cloze#C1`).
  - A step may hold several `cloze` requests when their functions differ; their values merge (`observations.md` O2).
- **Value** `{<function>: [<output>, ...], ...}`: one member per requested function, an array with one output per input, in input order.
- **Errors.** A malformed request (an unknown function, `numbers` with another function, an input of the wrong shape, a card number out of range) is `invalid_scenario`. No function refuses a valid input: every string is a valid text.

## Z2. Functions

| Function | Input | Output | Defined in |
|---|---|---|---|
| `numbers` | string | list of integers: the text's cloze numbers, ascending, each once | `render/cloze#C5` |
| `front` | `[text, card]` | string: what `{{cloze:F}}` renders to on the front of card `card` when field F holds `text` | `render/cloze#C6`, `#C8` |
| `back` | `[text, card]` | string: the same on the back | `render/cloze#C6`, `#C8` |
| `only_front` | `[text, card]` | string: what `{{cloze-only:F}}` renders to on the front | `render/cloze#C9` |
| `only_back` | `[text, card]` | string: the same on the back | `render/cloze#C9` |
| `typing` | `[text, card]` | string: what a `{{type:cloze:F}}` field expects on card `card` | `render/cloze#C10` |

`front`, `back`, `only_front` and `only_back` are exactly the replacement's own output: nothing else the renderer adds to a side is part of the value (the blank-front and missing-cloze messages of R976, styles, AV processing).

## Z3. Executors

- **Oracle.** `numbers` and `typing` use Anki's cloze-number query (on a note whose only field holds the text) and its cloze type-in extraction. The four rendering functions run Anki's template renderer on a card with template ordinal `card` − 1 of a standard (non-cloze) note type whose front and back templates contain the filter on a field holding the text, and keep only that filter's output. A template that renders only an empty field triggers the renderer's blank-front message, so the driver gives the template other content that it then removes, so that an empty text still yields its output (the empty string).
- **Meguri.** The render adapter of `meguri-testkit` calls `meguri-template`'s cloze functions directly, with the card number and the side.

## Z4. Scenarios

`conformance/scenarios/render/cloze/*.yaml` (`render/cloze#C14`). Those that use this kind define no note type and need no setup; the card numbers in their inputs are the cards' template ordinals plus one. `render/cloze/blank-check` and `render/cloze/templates` use the `template` kind instead (`render-ops.md` Q1).

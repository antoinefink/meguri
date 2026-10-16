# Type-in and review HTML observations

Spec: conformance/type-answer-ops
Requirements: R1883, R1907, R530, R532, R537, R964
Anki: 26.09.3
Scenarios: conformance/scenarios/render/type-answer/, conformance/scenarios/render/media-escape/, conformance/scenarios/render/image-occlusion/render/

Two observation kinds for U64: `type_answer` evaluates the type-in comparison on given texts, and `review_html` gives the question and answer HTML that review shows for a stored card, with the media name encoding and the type-in box and comparison applied. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The behavior they run is `specs/render/type-answer.md` and `specs/render/media-display.md`.

## K1. `type_answer` (observation kind)

- **Pure.** It never reads or changes the scenario's collection, so it may appear in any observe step, with or without setup (like `text`, `observations.md` O9).
- **Request** `{type_answer: {fns: [<function>, ...], inputs: [[<expected>, <typed>], ...]}}`: `fns` is a non-empty list of distinct names from the table; `inputs` is a list, possibly empty, of pairs of strings. Every function runs on every pair.
- **Value** `{<function>: [<output>, ...], ...}`: one member per requested function, an array with one string per input, in input order.

| Function | Output | Defined in |
|---|---|---|
| `compare` | the comparison HTML in mode plain (combining characters count): what the plain and cloze markers show | `render/type-answer#A6` |
| `compare_nc` | the same in mode nc (combining characters ignored): what the nc marker shows | `render/type-answer#A6` |

- **Errors.** A malformed request (an unknown or repeated function, an input that is not a pair of strings) is `invalid_scenario`. No function refuses a valid input: every pair of strings has an output. Meguri's own `ci` (`render/type-answer#A9`) is not a function here: the oracle has nothing to compare it with.

## K2. `review_html` (observation kind)

- **Request** `{review_html: [<case>, ...]}`: a non-empty list of cases `{card: "#cN", typed?: <string>}`. `typed` is the text the user typed into the box (default: the empty string). A card may appear in several cases. One `review_html` request per observe step: its value is a list, and two lists do not merge (`observations.md` O2).
- **Value** `[{"question": <string>, "answer": <string>}, ...]`, one result per case, in case order.
- **Meaning.** For the card as stored when the step runs, in review mode:
  - `question`: the card's question side text (`render/type-answer#A1`), media names encoded (`render/media-display#D1`), then the question filter (`render/type-answer#A4`);
  - `answer`: the card's answer side text, media names encoded, then the answer filter (`render/type-answer#A5`) with the card's type-in as that question decided it and the case's `typed`.

  This is `render/type-answer#A3` without its steps 3 and 5: play markers stay as markers (no play buttons) and the box has none of R533's attributes. Each warning of `render/type-answer#A8` is its token, never its text (R1887). Meguri's modifiers (`render/type-answer#A9`) apply on Meguri's side; `conformance/deviations.yaml` says where its answers differ.
- **Cases are independent.** Each case renders its card again; nothing carries over from an earlier case. Nothing in the collection changes.
- **Cards to avoid.** A card whose question or answer is a template error, the blank-front notice or the missing-cloze notice (`render/card-render-pipeline`, U62) has no defined value here, since those notices are localized text. Scenarios avoid such cards; `render` (U62) observes them.
- **Errors.** A malformed request is `invalid_scenario`: not a list, an empty list, a case that is not a mapping, a key other than `card` and `typed`, a `typed` that is not a string, a second `review_html` request in the step. A card that does not exist yet is `unknown_ref`.

## K3. Executors

- **Oracle.**
  - `type_answer`: Anki's answer comparison, the collection call its reviewer makes, told to count combining characters for `compare` and to ignore them for `compare_nc`, on the worker's scratch collection; the call reads and writes nothing.
  - `review_html`: for each case, the card is read from the scenario's collection, and its question and answer are taken from Anki's card question and answer calls (the render its reviewer uses). Each side goes through Anki's media name escaping (the collection media call in its encoding direction). Then come the reviewer's two type-in filters, question side then answer side, as the desktop app of the pinned release has them. The oracle image holds Anki's library, not its desktop app, so the driver carries those two functions from the tag, unchanged except for three things: the card, note and note type are the case's; the two warning texts are the tokens of `render/type-answer#A8`; the reviewer's comparison hooks pass their values through. The typed text is the case's `typed`.
- **Meguri.** The render adapter of `meguri-testkit` renders the card in review mode with U62's pipeline (`render/card-render-pipeline`), then applies U64's media display encoding and type-in functions in observation form (no play buttons, no R533 attributes, warning tokens). `type_answer` calls meguri-template's comparison directly.

## K4. Status

- Both kinds are in the oracle driver and listed in `conformance/schema/protocol.json`. The goldens of `render/type-answer/*` (the corpus included), `render/media-escape/cards`, `render/image-occlusion/render/cards` and `render/image-occlusion/render/audio` are recorded. `render/image-occlusion/render/audio-tags` uses U62's `render` kind and is recorded once the driver has it.
- `render/image-occlusion/render/divs` (the `cloze` kind) and `render/media-escape/strings` (the `text` kind) have goldens now.
- U64 teaches meguri-testkit both kinds and marks the scenarios active in `conformance/scenarios/render/area.yaml` in the change that lands them; until then T0 reports them invalid (unknown kind) or pending.

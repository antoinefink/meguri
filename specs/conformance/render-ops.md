# Render ops: the `template` and `render` observations

Spec: conformance/render-ops
Requirements: R935, R976, R1883, R1887, R1907, R1908, R1909
Anki: 26.09.3
Scenarios: conformance/scenarios/render/

The observation kind (`observations.md` O4) that evaluates the template engine on given template texts, so the render area's syntax, filter and special-field scenarios run without card generation or the render pipeline. Both executors implement it with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold its shapes. The behavior it runs is `specs/render/template-syntax.md`, `filters.md` and `special-fields.md`. The per-card `render` kind (Q4) renders stored cards through the card render pipeline (`specs/render/card-render-pipeline.md`), audio tags and play markers included.

## Q1. `template`

### Q1.1 Request

```yaml
- observe:
    - template:
        notetype: {name: Probe, kind: standard, fields: [Front, Back]}
        cases:
          - {front: "{{Front}}", back: "{{FrontSide}}<hr>{{Back}}", fields: [ein, zwei]}
          - {front: "{{Back}}", ord: 1, name: Reverse, fields: {Back: zwei}, tags: [de]}
          - {card: "#c1", front: "{{Deck}}"}
```

- `cases`: a non-empty list; each case is a **new-note case** or a **card case**.
- `notetype` (required when any case is a new-note case, otherwise not allowed): the note type of every new-note case of the request.
  - `name`: the note type name, non-empty, NFC, without `"`;
  - `kind`: `standard` (default) or `cloze`;
  - `fields`: a non-empty list of field names that a note type save keeps unchanged: each non-empty, NFC, without `:`, `{`, `}` or `"`, not starting with `#`, `/`, `^` or Unicode White_Space, not ending with Unicode White_Space, and no two equal under the `unicase` comparison (`text/unicode-primitives#U2`), so `Front` and `front` cannot both be fields.
- One `template` request per observe step: its value is a list, and two lists do not merge (O2).
- Names, values and template texts are strings; quote anything YAML would read as another type.

### Q1.2 New-note case

`{front, back?, ord?, name?, fields?, tags?}`: a note of `notetype` that is not added to any collection, and its card at ordinal `ord` that does not exist either (template preview of a new note, U71).

- `front`, `back`: the question and answer template texts (`back` defaults to empty). They are evaluated as given: no save, no validation, so templates that a note type save would refuse (unknown fields, structural errors) are fine.
- `ord`: the card ordinal, an integer from 0 to 499 (default 0). For a cloze note type it is the cloze number minus one.
- `name`: the template name, the `Card` special's value; a non-empty string, default `Card ` followed by `ord` + 1 (`Card 1`).
- `fields`: the note's field values as in `add_note` (`dsl.md` D7.2): a list in field order (missing trailing values are empty) or a mapping of field name to value (missing fields are empty). Values are used exactly as given: no save normalization, so emptiness is `text/field-emptiness#E1` on the given text.
- `tags`: the note's tags, a list of strings (default none), used as given and in that order.
- The special values are a new card's (`special-fields.md` P3): `Type` is `notetype.name`, `Card` is `name`, `Tags` the tags joined by one space, `Deck` and `Subdeck` `(Deck)`, `CardFlag` `flag0`, `CardID` `0`, and `c<ord+1>` is `1`, each unless a field of the same name exists.

### Q1.3 Card case

`{card: #cN, front, back?}`: card #cN as stored in the scenario's collection, rendered with the given template texts instead of its stored ones.

- The note, its note type (name, fields, kind), the card's ordinal and the card itself (`did`, `odid`, `flags`, `id`) come from the collection at this step, so every special has its real value (P3).
- `Card` is the stored template's name: for a standard note type the template at the card's ordinal, for a cloze note type the first template.
- `ord`, `name`, `fields` and `tags` are not allowed in a card case.

### Q1.4 Evaluation

For each case, independently:

1. Build the field map (`special-fields.md` P2).
2. Render `front` on the question side (`template-syntax.md` T1–T8). On an error, the case's value is that error with side `question`.
3. Apply the blank check (T10) to the note type's kind and the field map. When blank, the value is the question output with `answer` null and `empty` true. The answer template is not evaluated.
4. Otherwise render `back` on the answer side with the FrontSide text equal to the question output of step 2. On an error, the value is that error with side `answer`; otherwise the two outputs with `empty` false.

The outputs are the template engine's text only: no audio or TTS tag is extracted, nothing is turned into play markers, LaTeX is not rendered and no media name is changed (those are U62's, observed by `render`). FrontSide therefore carries the question output as it is, where review would pass the play-marker form (P4).

### Q1.5 Value

`{"template": [<result>, ...]}`, one result per case in case order:

| Result | When |
|---|---|
| `{"question": <string>, "answer": <string>, "empty": false}` | both sides rendered |
| `{"question": <string>, "answer": null, "empty": true}` | the question is blank (T10) |
| `{"error": {"kind": <kind>, "side": "question" \| "answer"}}` | T9; `kind` is one of Q2 |

No message text appears (R1887). Results are compared exactly (O7).

### Q1.6 Invalid requests

A request that breaks Q1.1–Q1.3 is `invalid_scenario`: a wrong type, a missing `notetype`, a name or field name outside its rules, a field mapping naming no field, more values than fields, `ord` out of range, a card case with new-note keys. A card case naming a card that does not exist yet is `unknown_ref`. A card whose ordinal has no template in its standard note type is a run error; scenarios avoid it.

Nothing in the scenario's collection changes. New-note cases never read it.

## Q2. Error kinds

`unclosed_conditional`, `unopened_conditional`, `mismatched_conditional`, `unknown_field`, `unknown_conditional` (`template-syntax.md` T9). The list is closed: an oracle message that matches none of them is a run error (`anki_error`) until this list grows.

## Q3. Status

- The oracle driver implements the kind: every scenario under `conformance/scenarios/render/` that uses only `template` and the other stable kinds has its golden, recorded with `tools/mg oracle record render`.
- Meguri's executor (the testkit's `template` adapter, U60) runs new-note cases with meguri-template and a field map built as Q1.2 says. New-note cases of a cloze note type (the blank check of T10, `render/cloze#C15`) and templates that use the `cloze` or `cloze-only` filter run with U61's cloze module (`render/cloze/blank-check`, `render/cloze/templates`).
- Card cases also need notes, cards, decks and note types in the collection (the `notes`, `decks` and `notetypes` setup sections and the `ids` observation: U27, U28, U29) and, for a cloze card, the cloze blank check (U61). U62 adds card cases to the adapter and activates `special-fields/cards`. `template/legacy-req` needs the `legacy_update` and `legacy_json` adapters and a note type save that recomputes each template's requirement (`template-syntax.md` T11.3); U36 activates it. Until then T0 reports both `pending`, and meguri-template's golden-pair tests check the engine's part of both against their goldens (`conformance/scenarios/render/area.yaml`).

## Q4. `render`

The outputs of the card render pipeline for stored cards, in review mode and in browser mode, before any display step (`specs/render/card-render-pipeline.md` K4, K5). Replay buttons, notices, the style block, media-name encoding and the type-in box are not part of it (`display-transform.md`, U64).

### Q4.1 Request

`render` (every card, in logical-id order), `render(#c1, #c3)` or `{render: [#c1, #c3]}` (`observations.md` O2): a per-card kind with no other argument. A card reference that does not exist yet is `unknown_ref`.

### Q4.2 Value

`{"#cN": <card result> | null, ...}`: `null` for a card that no longer exists (its note was deleted), otherwise `{"review": <review result>, "browser": <browser result>}`.

Review result (K4):

| Result | When |
|---|---|
| `{"question": <string>, "answer": <string>, "question_av": [<entry>, ...], "answer_av": [<entry>, ...], "blank": null}` | rendered |
| `{"question": <string>, "answer": null, "question_av": [<entry>, ...], "answer_av": [], "blank": "empty_front" \| "missing_cloze"}` | blank (K4 step 2) |
| `{"error": {"kind": <kind>, "side": "question" \| "answer"}}` | template error (K6); `kind` from Q2 |

- `question` and `answer` are the K4 question and answer texts: after audio extraction (play markers in place of sound tags and TTS directives), without any style block, notice, replay button or other display step. A blank card's question is its rendered question without Anki's blank notice.
- An `<entry>` is `{"sound": <name>}` (the decoded name, `av-tags.md` A6) or `{"tts": <language>}` (A6), in index order.

Browser result (K5):

| Result | When |
|---|---|
| `{"question": <string>, "answer": <string>, "blank": null}` | rendered |
| `{"question": <string>, "answer": null, "blank": "missing_cloze"}` | a cloze card with no cloze for its number |
| `{"error": {"kind": <kind>, "side": "question" \| "answer"}}` | template error |

- `question` and `answer` are the K5 texts: the browser formats when not empty, FrontSide empty, no audio extraction.

No message text appears (R1887). Values are compared exactly (`observations.md` O7).

### Q4.3 Evaluation

- Each card is rendered from the collection as stored at this step (note, note type, decks, the card's `did`, `odid`, `flags`), never from a cache that a `db_exec` could have left stale. Scenarios do not write note type rows with `db_exec` before a `render`.
- Nothing in the collection changes: no row, no media file, no config key. LaTeX tags stay as written (K8).

### Q4.4 Normalization

Anki writes a localized error message where a TTS directive has no language (`av-tags.md` A6). In every string of the value, each occurrence of exactly that text, `[` + message + `]`, is reported as `[tts: lang not set]`, the text Meguri writes. Nothing else is normalized: every other byte is Anki's.

### Q4.5 Invalid requests and run errors

- A request that breaks Q4.1 is `invalid_scenario`.
- A card that cannot be rendered at all (K2: its note type has no template at its ordinal) is a run error; scenarios avoid it.
- A template error whose message matches no kind of Q2 is a run error (`anki_error`), as for `template`.

## Q5. Status of `render`

- The oracle driver implements Q4 and the browser formats of `dsl.md` D6.4: the render scenarios of U62 (`render/pipeline`, `render/errors`, `render/av-tags`) have their goldens, recorded with `tools/mg oracle record render/pipeline render/av-tags render/errors`.
- Meguri's executor (the testkit's `render` adapter, U62) reports the engine's comparable outputs (`card-render-pipeline.md` K11). It needs the `notes`, `decks` and `notetypes` setup sections (U27, U28, U29) and the `edit_note` and `db_exec` ops for the scenarios that use them; until then those scenarios are `pending`. The scenario model must also read the browser formats of `dsl.md` D6.4: until it does, `render/pipeline/browser` is `invalid` in T0 and in `tools/mg goldens-check`.

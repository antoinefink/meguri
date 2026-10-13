# Field emptiness

Spec: text/field-emptiness
Requirements: R779
Anki: 26.09.3
Scenarios: conformance/scenarios/text/fields/empty.yaml, conformance/scenarios/text/corpus/

Whether a field counts as empty for card generation and template conditionals.

## E1. Predicate

A field is empty when its whole text is a sequence, possibly empty, of these items and nothing else:

- an ASCII whitespace character: U+0009, U+000A, U+000B, U+000C, U+000D or U+0020;
- a spacer tag: `<`, an optional `/`, then `br` or `div` (ASCII case-insensitive), an optional single U+0020, an optional `/`, then `>`.

So `<br>`, `<BR/>`, `<br />`, `</br />`, `<Br >`, `<div/>`, `<div />`, `</div>` and `<DIV>` are spacer tags, while `<br  >` (two spaces), `<br` + TAB + `>`, `<br` + LF + `>`, `< br>`, `<br/ >`, `<br //>` and `<div class="x">` are not.

Everything else makes the field non-empty, including `&nbsp;` and other entities (no decoding happens here), U+00A0, U+3000, other tags (`<span></span>`, `<p></p>`) and comments (`<!-- -->`).

## E2. Emptiness of a saved field

Card generation, conditionals and Empty Cards look at the stored field, which went through the save normalization of `checksum-and-sort-field.md` C1 (control characters other than TAB and LF removed, then NFC). So the emptiness of a value given to a save is E1 applied to that normalized value:

| Value given to the save | Empty? |
|---|---|
| `<br/> <div></div>` + LF | yes |
| `<DIV>`, `<Br >` | yes |
| U+000B U+000C U+000D | yes (removed on save) |
| `<b` U+000D `r>` | yes (saves as `<br>`) |
| `<br>` U+0001 | yes |
| `&nbsp;`, `<span></span>` | no |
| U+00A0, U+3000 | no |
| `&#32;` | no |

## E3. Where it is used

Card generation decides which templates produce cards (U27), the `{{#Field}}` and `{{^Field}}` sections test it (U60), and Empty Cards uses both (U131). The "first field is empty" warning when adding a note is a different rule, on the stripped first field (`checksum-and-sort-field.md` C5).

## E4. Scenarios

- `fields/empty`: the rows above and their neighbours.
- `corpus/fields-*`: 48,000 distinct generated values, about 5% of them built only from whitespace and spacer tags.
- Observation function (`specs/conformance/observations.md` O8): `field_empty`, which applies E2.

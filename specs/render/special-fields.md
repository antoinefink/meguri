# Special fields

Spec: render/special-fields
Requirements: R955, R960, R1909
Anki: 26.09.3
Scenarios: conformance/scenarios/render/special-fields/

The names a template can use besides the note's own fields, the values they take, and how they interact with note fields of the same name. The template engine (U60) reads them from the field map its caller builds; the render pipeline (U62) and template preview (U71) build that map as described here.

## P1. Names

`FrontSide`, `Tags`, `Type`, `Deck`, `Subdeck`, `Card`, `CardFlag`, `CardID`, and the card-number key `c<N>` (P2). Names are case-sensitive: `{{tags}}` is an ordinary (usually unknown) field.

## P2. The field map

For one card of one note, the map holds, in this order of precedence:

1. every field of the note type, by its name, with the note's text for it;
2. each of `Tags`, `Type`, `Deck`, `Subdeck`, `Card`, `CardFlag`, `CardID` with its P3 value, unless a note field already has exactly that name;
3. `c` followed by the card's ordinal + 1 in decimal (`c1` for ordinal 0), with the value `1`, unless a note field already has exactly that name.

`FrontSide` is never in the map (P4). A note field named like a special wins for every use, replacement and section alike: a note field `Tags` with an empty value makes `{{Tags}}` empty and `{{#Tags}}` hidden, even when the note has tags. `Subdeck` is always derived from the card's deck, even when a note field named `Deck` replaces the `Deck` value.

## P3. Values

| Name | Value |
|---|---|
| `Tags` | the note's tags as stored, in stored order, joined by one U+0020; empty when the note has none |
| `Type` | the note type's name |
| `Deck` | the full name of a deck, components joined by `::`: the deck `odid` names when `odid` is not 0 (a card in a filtered deck shows its original deck), otherwise the deck `did` names; `(Deck)` when that deck does not exist, which includes a card not yet added (deck id 0) |
| `Subdeck` | the text after the last `::` of the `Deck` value; the whole value when it has no `::` (so `(Deck)` gives `(Deck)`) |
| `Card` | the name of the template being rendered: for a standard note type, the template at the card's ordinal; for a cloze note type, its first template |
| `CardFlag` | `flag` followed by the card's whole `flags` column in decimal: `flag0` for an unflagged card, `flag1` to `flag7` for the user flags (`tags/flags-and-marked`); bits above the low three are not masked, so a stored 9 gives `flag9` |
| `CardID` | the card's id in decimal; `0` for a card not yet added |
| `c<ordinal+1>` | `1` |

So a card not yet added (template preview of a new note, U71) has `Deck` and `Subdeck` `(Deck)`, `CardFlag` `flag0` and `CardID` `0`.

## P4. FrontSide

- `{{FrontSide}}` is resolved by the engine itself (`template-syntax.md` T7 rule 1): on the question side it outputs nothing; on the answer side it outputs the FrontSide text its caller passes. Any filters on it are ignored. A note field named `FrontSide` does not change this.
- What the caller passes: in review and preview, the card's question text after the render pipeline has turned its audio and TTS tags into play markers (U62, R961), so sounds of the front appear as replay buttons, not as sounds to play again. The `template` observation passes the question output of the same case instead (`specs/conformance/render-ops.md` Q1.4).
- Sections: `FrontSide` is not in the map, so `{{#FrontSide}}` and `{{^FrontSide}}` are `unknown_conditional` errors unless the note has a field of that name, in which case they test that field.
- Analysis: the blank check and card generation treat `FrontSide` like any other name, by the field map, so it counts as non-empty only when the note has a non-empty field of that name (`template-syntax.md` T10, T11.2). Without such a field, a question template made only of `{{FrontSide}}` is blank; with one, it is not blank although it outputs nothing.

## P5. Emptiness of special values

Sections and the blank check judge specials by their values (`text/field-emptiness#E1`), like note fields:

- `{{#Tags}}` shows only when the note has tags;
- `Type`, `Deck`, `Subdeck`, `Card`, `CardFlag`, `CardID` and `c<ordinal+1>` are never empty, so their sections always show (`{{#CardFlag}}` shows on an unflagged card: its value is `flag0`).

## P6. Card generation (note for U27)

Deciding which cards a standard note generates uses the strict analysis (`template-syntax.md` T11.2) with a different set of non-empty names than rendering: the note's non-empty fields, plus `Type`, `Deck`, `Subdeck`, `Card`, `CardFlag` and `CardID` regardless of value (unless a note field has the name), plus `Tags` when the note has at least one tag (unless a note field has the name). The `FrontSide` special and `c<N>` are never added; a note field named `FrontSide` counts like any other field. U27's spec owns the rest of card generation.

## P7. Scenarios

- `special-fields/new-note`: every value for a note and card not yet added, emptiness of each special in sections, note fields named like specials (`Tags`, `FrontSide`, `c1`, `Type`), FrontSide with filters and on the question side.
- `special-fields/cards`: real cards: nested deck names and `Subdeck`, a card moved to another deck with `odid` set, a card whose deck or original deck is missing, flags 0, 3, 7 and 9, `CardID`, stored tags, the template name of a standard and of a cloze note type.
- Observation kind: `template` (`specs/conformance/render-ops.md` Q1).

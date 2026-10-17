# Card generation

Spec: notes/card-generation
Requirements: R782, R783, R784, R785, R786, R787, R788, R789, R790; supports R1008
Anki: 26.09.3
Scenarios: conformance/scenarios/cardgen/

Which cards a note has: which templates (standard note types) or cloze ordinals (cloze note types) require a card, when cards are created, and the row a new card gets. Template parsing and analysis are `render/template-syntax` (structural errors T6, referenced names T11.1, the strict analysis T11.2); special names are `render/special-fields`; field emptiness is `text/field-emptiness` E2; cloze numbers are `render/cloze` C5. The deck and the position of each new card are `notes/deck-placement-and-position`.

## G1. When cards are generated

| Event | Generation | Placement |
|---|---|---|
| adding a note, also in a bulk add (`notes/note-records` A2, A5) | G6, G7 for a note with no card | D2 |
| an update that changes the note (`notes/note-records` U3) | G7 | D3 |
| Check Database, for each note (U128) | G7 | D3 |
| an import that adds or updates notes (U120, U121) | as the importer's spec says | its spec |
| a note type change that adds a template or changes a front (U34), Change Note Type (U35) | G9 | D4 |

Nothing else creates cards: incoming sync, undo and redo, scheduling operations and an update that changes nothing (`notes/note-records` U2) never do.

## G2. Non-empty names (standard note types)

For one note, the names that count as non-empty are:

- each field of the note type whose stored value is not empty (`text/field-emptiness` E1 on the saved value), by its name;
- `Type`, `Deck`, `Subdeck`, `Card`, `CardFlag` and `CardID`, each unless the note type has a field of exactly that name;
- `Tags` when the note has at least one tag, unless the note type has a field named `Tags`.

`FrontSide` and the `c<number>` names are never added. A note field named like a special name counts by its own value only: a field `Tags` that is empty makes `Tags` empty even when the note has tags, and a field `FrontSide` counts like any other field (`render/special-fields` P2, P6). Names are compared exactly, case included.

## G3. Required templates (standard note types)

The template at ordinal t requires a card when its front:

1. parses without a structural error (`render/template-syntax` T6); a front that does not parse never requires one (R790); and
2. renders something by the strict analysis (T11.2) with G2's names.

The back is never read (R783). Filters do not matter (`{{text:A}}` counts like `{{A}}`). Text, comments, sections whose contents hold no counting replacement, and `{{FrontSide}}` without a field of that name never require a card.

Examples (scenarios `cardgen/normal/specials-and-emptiness`, `cardgen/normal/conditionals`, `cardgen/normal/unusable-front`), with fields A and B:

| Front | A `&nbsp;`, B empty | A `<br>`, B `x` | A `<div> </div>` LF, B `<span></span>` | both empty | A `a`, B `b` |
|---|---|---|---|---|---|
| `{{A}}` | yes | no | no | no | yes |
| `{{Deck}}` | yes | yes | yes | yes | yes |
| `{{FrontSide}}` | no | no | no | no | no |
| `{{#A}}static{{/A}}` | no | no | no | no | no |
| `{{^A}}{{B}}{{/A}}` | no | yes | yes | no | no |
| `{{text:A}}` | yes | no | no | no | yes |
| `{{#B}}{{Card}}{{/B}}` | no | yes | yes | no | yes |
| `{{#CardID}}{{B}}{{/CardID}}` | no | yes | yes | no | yes |

| Front | No tags | Tags `t` |
|---|---|---|
| `{{#Tags}}{{Front}}{{/Tags}}`, Front not empty | no | yes |
| `{{^Tags}}{{Front}}{{/Tags}}`, Front not empty | yes | no |
| `{{#Tags}}{{Front}}{{/Tags}}` with a note field `Tags` that is empty | no | no |
| `{{#Tags}}{{Front}}{{/Tags}}` with a note field `Tags` holding `v` | yes | yes |
| `{{FrontSide}}` with a note field `FrontSide` holding `f` | yes | yes |
| `{{#B}}{{B}}` (does not parse), `static text` (no name) | no | no |

## G4. Required ordinals (cloze note types)

- The note's cloze numbers are the union of the cloze numbers (`render/cloze` C5) of all its fields, every field, whether or not a `{{cloze:}}` replacement uses it (R785).
- Each number n requires the ordinal min(n - 1, 499). Number 0 is never a cloze number, so `{{c0::…}}` alone requires nothing (R786).
- The templates are not read: a cloze note type's front need not even parse.

| Fields | Required ordinals |
|---|---|
| `{{c1::a}} {{c3::b}} {{c0::label}} {{c700::z}}` (AE33) | 0, 2, 499 |
| Text `only extra`, Extra `{{c2::in Extra}}` | 1 |
| `{{c2::a {{c4::nested}}}} {{c1,3::list}}` | 0, 1, 2, 3 |
| `{{c0::only zero}}`, `plain`, `{{c1::open` | none (so G6 applies) |

An image-occlusion note follows the same rule: its masks hold clozes with numbers, its text labels are c0 (`render/cloze` C11, U63), so two masks and one label give ordinals 0 and 1 (AE33).

## G5. Ordinal 499 once

Meguri creates at most one card per ordinal in one generation, so a note whose new numbers include several of 500 and above gets one card of ordinal 499 (R786: they share the card of c500). Anki 26.09.3 creates one card per new number instead: `{{c1::a}} {{c500::p}} {{c501::q}} {{c999::r}}` gets three cards of ordinal 499 there, and Check Database later removes the extra ones (U128). Deviation `cloze-ordinal-499-once`, scenario `cardgen/cloze/above-500`. Once a note has a card of ordinal 499, neither adds another.

## G6. Every note gets a card

When a generation finds that the note has no card at all and requires none, the card of ordinal 0 is created (R787), for standard and cloze note types alike. This covers every added note and, on its next generation, an existing note left with no card by another program or a raw write. Note type regeneration (G9) never applies it, since it visits only notes that have cards. The card created this way does not use template 0's deck override (`notes/deck-placement-and-position` D2).

## G7. Existing notes: only additions

A generation for a note creates the required ordinals (G3, G4) that none of the note's existing cards has. It never removes, moves or changes an existing card, even when its template no longer renders anything or its cloze number has gone (R788); Empty Cards (U35, U131) and note deletion remove such cards. An existing card whose ordinal has no template counts as existing.

Example (AE31, scenario `cardgen/normal/edits-only-add`): a note with templates `{{Front}}` and `{{#Add Reverse}}{{Back}}{{/Add Reverse}}` and an empty Add Reverse has the card of ordinal 0; typing `x` into Add Reverse adds ordinal 1; clearing Add Reverse later keeps it, and no grave is written.

## G8. Creation order

Within one note the new cards are created in ascending ordinal order; across notes, in the order the operation handles them (a bulk add's list order, G9's note order). Card ids are allocated in creation order (`storage/ids-usn-mtime` I2). Anki creates the cards of one cloze note in an unspecified order, so scenarios compare the ordinals, decks and positions of a multi-card cloze note, never which real id each card got.

## G9. Note type regeneration (R789)

After a note type's templates change (U34 decides when: a template added, a front changed), the cards the new templates require are created for its notes. Given the note type as it will be saved:

1. Take every note of the note type that has at least one card, in ascending note id order.
2. For a standard note type, skip a note whose number of cards equals the number of templates; such a note is taken as complete without reading its fields.
3. For every other note, create the missing cards as G7 says, placed by D4, positioned by P2 (one counter value per note that needs one).

Meguri runs it as a job in two halves (U24):

- **Plan**: computes, for each note, the cards to create (note, ordinal, deck, and whether the position is a sibling's or the counter's) without writing anything. It reports progress as notes done out of notes of the type, at most about ten times a second, and can be cancelled; a cancelled plan leaves the collection untouched.
- **Commit**: allocates ids, draws counter values in note order and writes the cards, inside the caller's write transaction, so a note type save (U34) and the cards it requires are one journal and one undo step. A note whose cards changed between plan and commit is planned again at commit (U24's preconditions).

U34 calls the plan with its in-memory draft of the note type, before saving it.

## G10. The row of a new card

| Column | Value |
|---|---|
| `id` | allocated in G8's order (`storage/ids-usn-mtime` I2, I3) |
| `nid` | the note |
| `did` | `notes/deck-placement-and-position` D2, D3, D4 |
| `ord` | the ordinal |
| `mod`, `usn` | `now_s`, -1 |
| `type`, `queue` | 0, 0 |
| `due` | the position (`notes/deck-placement-and-position` P2, P3) |
| `ivl`, `factor`, `reps`, `lapses`, `left`, `odue`, `odid`, `flags` | 0 |
| `data` | `{}` (`model/card-data-json` C1) |

## G11. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `cardgen/normal/conditionals` | G2, G3 (Tags, a field named Tags), G6, G7 | recorded |
| `cardgen/normal/specials-and-emptiness` | G2, G3 (specials, FrontSide field, emptiness) | recorded |
| `cardgen/normal/unusable-front` | G3 (fronts that do not parse or name nothing), G4 (cloze ignores its front), G6, G7 | recorded |
| `cardgen/normal/edits-only-add` | G7, AE31 | recorded |
| `cardgen/cloze/numbers` | G4, G6, G7, AE33 | recorded |
| `cardgen/cloze/above-500` | G5 | recorded; deviation `cloze-ordinal-499-once` |

G9's results are compared with the oracle through U34's template scenarios (a template added to a note type with notes, a front edited); U27's Rust tests check the plan and commit halves, progress and cancellation on a 20,000-note type against expectations built from G2 to G7.

## G12. Differences from the Product Contract's wording

- R786 says numbers above 500 "share the card with ordinal 499": Meguri makes one card (G5, a deviation from Anki 26.09.3, which makes several).
- R787's card of ordinal 0 does not use template 0's deck override, although R791 speaks of "each new card": Anki places it like a card without override, and so does Meguri.

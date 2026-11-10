# Search syntax reference

Spec: search/syntax-reference
Requirements: R1093; supports R1086, R1090, R1106, R1109
Anki: 26.09.3 for what each example finds (the cited sections, all checked against the oracle); none for the choice of examples and for inserting one (Meguri design)
Scenarios: conformance/scenarios/search/reference/ (draft, inactive: Y11)

The examples of the syntax reference that U214 opens from the search field (R1093; the control that opens it and how it is presented on each platform are the screen contract's, `browse.browser`): one or more per key family, every family of `search/editing-services` D3.1 included. Each row gives the example's exact text and what it finds; the cited section is the rule. The reference's headings and the sentence it shows beside each example are Meguri's wording (`browser/browse-strings` W6), written from the "Finds" column; the example texts are fixed here and are taken only from this file.

## Y1. Rules for every example

- **Exact text.** The reference shows and inserts the example text exactly as listed.
- **Inserting.** Inserting an example replaces the field's selected text, or inserts at the cursor when nothing is selected. One space (U+0020) is added before the example when the character before it is neither white space nor `(`, and one after it when the character after it is neither white space nor `)`; nothing is added at the start or the end of the field. The cursor then follows the example and its added space. In an empty field the field text becomes exactly the example. Inserting is an edit, not a commit: the live run follows as for typing (R1106), and Return commits (R1109). The example's own text is kept, so an example holding `OR` inserted next to other terms joins them by the usual precedence (`search/grammar` G5), which the tokens then show (R1086).
- **Canonical.** Every example is its own canonical text (`search/canonical-form`): committing an example inserted in an empty field (R1109) leaves the field text unchanged, and its tokens (D3) are those the reference shows.
- **No error.** Every example parses with no error (`search/errors` E1). It runs with no run error (E3) on a collection that has a cloze note type and no note type whose every field is excluded from unqualified searches. On other collections some examples fail, and the run error shows as for any search; the reference does not hide them: without a cloze note type `sc:` fails (`search/text-matching` T9.1); when some note type has every field excluded, `re:` and `w:` fail (T7.3); when no note type has an included field, the plain-text examples of Y2 and Y3 (`dog`, `d*g`, `"a dog"`, `3\:30` and the rest), `nc:` and `sc:` fail as well (T7.2).
- **Placeholders.** Field, deck, tag, preset, note type, template, custom-data names and ids in the examples are sample values; nothing has to exist for an example to run. The reference shows them as they stand: it does not substitute names or ids from the collection.
- **Order.** The reference shows the sections Y2 to Y9 in this order and the rows of each in table order.

## Y2. Combining terms

| Example | Finds | Rule |
|---|---|---|
| `dog cat` | notes holding both `dog` and `cat`; `and` may be written between them, and committing drops it | `search/grammar` G2, `search/canonical-form` F3 |
| `dog OR cat` | notes holding either | G2 |
| `(dog OR cat) fish` | notes holding `fish` and either of the others: brackets group, and AND binds tighter than OR without them | G2, G5 |
| `-cat` | notes not holding `cat` | G4 |
| `dog -(cat OR fish)` | notes holding `dog` and neither of the others | G4, G5 |
| `"a dog"` | the text `a dog`, space included: quotes keep a term together | G3, `search/text-matching` T5 |
| `"or"` | the word `or` itself: `and` and `or` alone are operators unless quoted | G2, `search/canonical-form` F4 |
| `"-dog"` | text holding `-dog`: a term that starts with `-` negates unless quoted | G4, F4 |

## Y3. Text in any field

| Example | Finds | Rule |
|---|---|---|
| `dog` | notes with `dog` anywhere in a field or in the sort field, letter case ignored (`hotdog` too) | `search/text-matching` T5, `search/case-folding` C2 |
| `d*g` | `*` stands for any run of characters, none included (`dg`, `dog`, `drag`) | T3 |
| `d_g` | `_` stands for exactly one character (`dog`, `dig`) | T3 |
| `a\_b` | a literal `_`: `\` before `*`, `_`, `"`, `:`, `(`, `)`, `-` or `\` makes it plain text | `search/grammar` G7, T3 |
| `3\:30` | the text `3:30`: a colon that is not a key needs `\` | G6 |
| `w:dog` | `dog` as a whole word (`dog.`, not `dogs` or `hotdog`) | T10 |
| `nc:uber` | text ignoring accents and other combining marks (`über`, `Uber`) | T8 |
| `re:\d{3}` | a regular expression over the fields (three digits in a row) | T11 |
| `sc:capital` | notes of cloze note types, with their cloze markup removed before matching | T9 |

## Y4. One field

| Example | Finds | Rule |
|---|---|---|
| `front:dog` | notes whose field `Front` is exactly `dog`, letter case ignored; the field name may be any field of any note type | `search/fields` F1, F4 |
| `front:*dog*` | notes whose `Front` holds `dog` | F4 |
| `front:` | notes whose `Front` is empty | F4.1 |
| `front:_*` | notes whose `Front` is not empty | F4.1 |
| `front:re:^d` | a regular expression on `Front` | F5 |
| `front:nc:uber` | `Front`, ignoring accents and combining marks | F6 |
| `"my field:dog"` | a field whose name holds a space: the whole term goes in quotes | F1 |

## Y5. Tags, decks, presets, note types and templates

| Example | Finds | Rule |
|---|---|---|
| `tag:animal` | notes tagged `animal` or any tag under it (`animal::mammal`), letter case ignored; not `animals` | `search/names-and-ids` N2.2 |
| `tag:animal::*` | notes with a tag under `animal`, not `animal` itself | N2.2 |
| `tag:none` | notes without tags | N2.2 |
| `tag:re:^ani` | a regular expression on the tags | N2.3 |
| `deck:Japanese` | cards in the deck `Japanese` and its subdecks, also from a filtered deck whose home deck it is | N3.3, N3.7 |
| `"deck:Japanese Core"` | a deck whose name holds a space | N3.3, `search/canonical-form` F4 |
| `deck:current` | cards in the current deck and its subdecks | N3.4 |
| `deck:filtered` | cards in filtered decks | N3.2 |
| `deck:*` | every card | N3.2 |
| `did:1` | cards in the deck with that id, without its subdecks | N3.5 |
| `preset:Daily` | cards in decks that use the preset `Daily` | N3.6 |
| `note:Vocabulary` | notes of the note type `Vocabulary` | N4.1 |
| `mid:1700000000003` | notes of the note type with that id | N5 |
| `card:1` | cards of a note type's first template; on a cloze note type, cloze 1 | N4.2 |
| `card:Forward` | cards of the template named `Forward` | N4.2 |

## Y6. Card state and flags

| Example | Finds | Rule |
|---|---|---|
| `is:due` | cards waiting to be studied: review and interday-learning cards due today or earlier, and learning and preview cards due no later than now plus the learn-ahead limit | `search/state-and-flags` S1 |
| `is:new` | new cards | S5 |
| `is:learn` | cards in learning or relearning | S5 |
| `is:review` | review cards, relearning cards included | S5 |
| `is:suspended` | suspended cards | S5 |
| `is:buried` | buried cards, by the user or with a sibling | S5 |
| `is:buried-manually` | cards the user buried | S5 |
| `is:buried-sibling` | cards buried with a sibling | S5 |
| `flag:1` | cards with flag 1 (red); 2 orange, 3 green, 4 blue, 5 pink, 6 turquoise, 7 purple | S6 |
| `flag:0` | cards with no flag | S6 |

## Y7. Card properties

| Example | Finds | Rule |
|---|---|---|
| `prop:ivl>=10` | cards with an interval of 10 days or more; the comparisons are `<`, `<=`, `=`, `!=`, `>=`, `>` | `search/properties` P1, P2 |
| `prop:due=1` | review and interday-learning cards due tomorrow; learning and preview cards count whole days from the start of tomorrow, so they match when due the day after tomorrow | P4 |
| `prop:due<0` | review and interday-learning cards due before today (overdue), and learning and preview cards due no later than the start of today | P4 |
| `prop:reps>10` | cards answered more than 10 times | P2 |
| `prop:lapses>3` | cards forgotten more than 3 times | P2 |
| `prop:ease<2.5` | cards with an ease below 250%, new and learning cards included (their stored ease is 0) | P3 |
| `prop:pos<=100` | new cards at position 100 or before | P5 |
| `prop:s>30` | cards whose FSRS stability is above 30 days | P7 |
| `prop:d>0.8` | cards whose FSRS difficulty is above 0.8 on a 0 to 1 scale | P7 |
| `prop:r<0.9` | cards whose FSRS retrievability is now below 90% | P8 |
| `prop:cdn:streak>3` | cards whose custom data `streak` is a number above 3 | P9 |
| `prop:cds:source=web` | cards whose custom data `source` is the text `web` | P9 |
| `has-cd:streak` | cards that have the custom data `streak` | P9 |

## Y8. Dates and history

| Example | Finds | Rule |
|---|---|---|
| `added:7` | cards added in the last 7 days, today being the first | `search/history` H2, H3 |
| `edited:3` | notes edited in the last 3 days | H4 |
| `rated:1` | cards answered today | H5 |
| `rated:7:1` | cards answered Again in the last 7 days (2 Hard, 3 Good, 4 Easy) | H5 |
| `introduced:30` | cards first answered in the last 30 days | H7 |
| `resched:7` | cards with a review-log entry written without an answer in the last 7 days: Set Due Date, Reset (`sched/manual-ops`) or an FSRS reschedule | H6 |
| `prop:rated=-1` | cards answered yesterday: 0 is today, -1 yesterday | H8 |
| `prop:resched=0` | cards with such an entry today | H8 |

## Y9. Ids and duplicates

| Example | Finds | Rule |
|---|---|---|
| `nid:1700000000000` | the note with that id; several ids are separated by commas | `search/names-and-ids` N5 |
| `cid:1700000000001,1700000000002` | the cards with those ids | N5 |
| `dupe:1700000000003,dog` | notes of the note type with that id whose first field is `dog` once formatting is removed | N6 |

## Y10. Every key family

Every span kind of `search/editing-services` D3.1 has an example, as R1093 asks:

| Kind | Examples in |
|---|---|
| `plain` | Y2, Y3 |
| `field` | Y4 |
| `text` (`re:`, `nc:`, `sc:`, `w:`) | Y3 |
| `tag` | Y5 |
| `deck` (`deck:`, `did:`) | Y5 |
| `preset` | Y5 |
| `notetype` (`note:`, `mid:`) | Y5 |
| `template` | Y5 |
| `flag`, `state` | Y6 |
| `property` | Y7, Y8 (`prop:rated`, `prop:resched`) |
| `date_range` | Y8 |
| `ids` | Y9 |
| `duplicates` | Y9 |
| `custom_data` | Y7 |
| `operator`, `group_open`, `group_close` | Y2 |

A new span kind needs a row here in the same change.

## Y11. Tests and scenarios

- `conformance/scenarios/search/reference/examples` (draft, inactive, no golden yet) observes `canonical_search` of every example (`specs/conformance/search-ops.md` S2), each expected to be its own text (Y1), and `search` of every example in both modes (S1) on a collection with a standard note type `Memo` (fields `Front`, `Back`, `my field`), a cloze note type and a few notes, each expected to give ids and no error. Its tags are every `needs-*` of search-ops S4, so it becomes active once U81 and U82 have all landed.
- U214's `SyntaxReferenceView` tests check that the view lists exactly the examples of Y2 to Y9 in order, that every D3.1 kind has an example (Y10), that inserting an example in an empty field and committing leaves the field text unchanged, and Y1's insertion rule with `is:new`: the cursor at the end of `dog` gives `dog is:new`; the cursor between `dog` and `cat` with no space between them gives `dog is:new cat`; the cursor inside `()` gives `(is:new)`; `cat` selected in `dog cat` gives `dog is:new`; each leaves the cursor after the example and its added space, and none commits.

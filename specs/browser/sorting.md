# Browser sorting

Spec: browser/sorting
Requirements: R1139, R1140, R1143, R1144, R1145; supports R1083, R1141, R1142, R1146, R1920, R2016
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/sort/
Status: Approved 2026-11-20 by Antoine in chat, after an independent review and five reviewer rounds (`docs/process/review-log/browse-note-types-specs-20261120.md`) (R1232; round 8, decided 2026-11-20) for the lines of S1 that begin "Changed 2026-11-20 (round 8)". Every other section is in force and unchanged.

How Browse orders the rows of a search for each sortable column, in cards mode and notes mode, ascending and descending. Anki's order is given as a list of keys per column; Meguri adds one final key so that the order is total (R1145). Terms (cards of a note, first card, home due, card data, memory state, now, today) are `browser/columns` C1. Which rows a search returns is `search/query-model`; the sort never changes that set, except as S9 says.

## S1. Sortable columns

Changed 2026-11-20 (round 8): field columns `fld:<name>` sort in both modes by `browser/workspace` V4.2, first direction ascending.

| Key | Cards mode | Notes mode | First direction |
|---|---|---|---|
| `noteFld` | yes | yes | ascending |
| `question`, `answer` | no | no | none |
| `template` | yes | yes | descending |
| `deck` | yes | yes | ascending |
| `cardDue`, `cardEase`, `cardIvl`, `cardReps`, `cardLapses` | yes | yes | descending |
| `noteCrt`, `cardMod`, `noteMod` | yes | yes | descending |
| `note`, `noteTags` | yes | yes | ascending |
| `originalPosition` | yes | yes | descending |
| `stability`, `difficulty`, `retrievability` | yes | no | descending |

- This is `browser/config-keys` K3. The first direction is the one the first click on a header uses; the second click reverses (R1139). The iPhone sort menu offers the same columns (R1146).
- A sort request names a mode, a column key and a direction; the core never chooses a direction. The first direction is catalog metadata (`browser/columns` C2), which only the sort-by-column write of `browser/config-keys` K5.2 uses. A key outside the 19, or a column not sortable in the mode, is refused with kind `unsortable_column`, the kind of `browser/config-keys` K5.2, before anything runs (U85); the conformance observation records it as `unsortable` (`specs/conformance/browser-ops.md` B1). Decided 2026-10-27: sort requests and sort writes share that one refusal kind; the observation keeps `unsortable` so that the goldens recorded with it stay valid. Meguri shows no sort affordance on such headers instead of Anki's message (R1140).
- A stored sort that is unsortable falls back to `noteCrt` (`browser/config-keys` K5.1, R1141). This is Meguri's rule: Anki falls back only for a stored key that names no column, and runs a known column that is unsortable in the mode (`question`, `answer`, notes-mode `stability`, `difficulty`, `retrievability`) as an unsorted search (`browser/config-keys` K5.3).

## S2. Comparing values

- Keys compare the way SQLite 3 orders values: NULL before everything; then numbers, INTEGER and REAL together by numeric value; then text by the key's collation; then blobs.
- Every key reads the value as stored, with SQLite's arithmetic and comparison, not the decoded value the cells use (`browser/columns` C1, `model/row-records` R4). `sfld` mixes storage classes by design (below); the card columns do so only in legacy or damaged rows, and then sort by what they hold: a REAL `ivl` 3.5 sorts after an INTEGER 3 under Interval although both cells show 3; a REAL `mod` or `due` sorts by its value; a `due` outside the `i32` range sorts by its stored value although its cells read 0; a TEXT value sorts after every number.
- **NOCASE** (SQLite's built-in collation): text compared byte by byte in UTF-8 after mapping the 26 ASCII capital letters to small ones; when one text is a prefix of the other, the shorter comes first. Nothing else is folded: `É` and `é` differ, and `é` sorts after `z` (its first byte is above every ASCII byte).
- **unicase**: `text/unicode-primitives` U2, the comparison of the `unicase` collation.
- **binary**: UTF-8 bytes, which is code point order.
- Real numbers (averages, card data values) compare by value.

`sfld` holds a number when the sort-field text reads as one (`text/checksum-and-sort-field` C3). With NOCASE, these sort fields sort in this order: `-4`, `1.5`, `9`, `10`, 12 with a space on each side (stored as the number 12), the empty field, `apple` and `Apple` (equal), `Banana` and `banana` (equal), `zebra`, `éclair`. Numbers come first by value, then texts.

## S3. Ranks

- **Deck rank**: a deck's position when all decks of the collection are ordered by their stored name under unicase. The stored name joins components with U+001F, which sorts before every printable character, so a deck's subdecks follow it directly: `_x`, `A`, `A::B`, `A B`, `a2`, `AB`, `B`, `B::c`, `Z`, `Ä`. Deck names are unique under unicase, so no two decks share a rank.
- **Note type rank**: a note type's position when all note types are ordered by name under unicase (unique).
- **Template rank**: a template's position when every template of every note type is ordered by name under unicase, then by note type id, then by ordinal. Two note types' templates both named `Card 1` (or `Card 1` and `card 1`) rank by note type id.
- A record that does not exist has no rank: the key is NULL.

## S4. Keys in cards mode

For a card c of note n (the note's row exists unless S9 says otherwise):

| Column | Keys, in order |
|---|---|
| `noteFld` | n's `sfld` (NOCASE); c's `ord` |
| `template` | the template rank of (n's note type, c's `ord`), or when that note type has no template at that ordinal the rank of (n's note type, 0), NULL when neither exists; c's `ord` |
| `deck` | the deck rank of c's `did` (the filtered deck for a filtered card) |
| `cardDue` | E(c) (S6) |
| `cardEase` | 1 when c's `type` is 0, else 0; c's `factor` |
| `cardIvl` | c's `ivl` |
| `cardReps` | c's `reps` |
| `cardLapses` | c's `lapses` |
| `noteCrt` | n's `id`; c's `ord` |
| `cardMod` | c's `mod` |
| `noteMod` | n's `mod`; c's `ord` |
| `note` | the note type rank of n's note type |
| `noteTags` | n's stored `tags` text, binary (the whole stored text, separators included) |
| `originalPosition` | P of the first card of c's note (S5.1); c's note id |
| `stability` | the card data `s` as `search/card-extractors` X4 reads it (on its own, without `d`), NULL when absent |
| `difficulty` | the card data `d` as `search/card-extractors` X4 reads it, NULL when absent |
| `retrievability` | R(c) (S7), NULL without a memory state |

So Cloze 2 sorts before Cloze 10 (ordinal 1 before 9) under the same template rank (R1143, R1121), new cards come last in ascending Ease, and all cards of one note share their Position keys (the note's first card decides).

## S5. Keys in notes mode

For a note n. Where the table says "or NULL", a note for which the aggregate has no input has the single key NULL instead of the pair; all such notes are equal, and they come first ascending and last descending (R1144, AE50).

| Column | Keys, in order |
|---|---|
| `noteFld` | n's `sfld` (NOCASE) |
| `template` (Cards) | the number of n's cards; n's id. Or NULL: no card |
| `deck` | the number of distinct `did` values among n's cards whose deck exists; the deck rank of the first of those cards (lowest card id); n's id. Or NULL: no card in an existing deck |
| `cardDue` | the smallest E (S6) among n's cards that have `odid` 0, `type` not 0 and `queue` above 0; n's id. Or NULL: no such card (only new, suspended, buried or filtered cards) |
| `cardEase` | the average `factor` of n's cards whose `type` is not 0, as a real number, not wrapped; n's id. Or NULL: no such card |
| `cardIvl` | the average `ivl` of n's cards whose `type` is 2 or 3, as a real number, not wrapped; n's id. Or NULL: no such card |
| `cardReps` | the sum of `reps` over n's cards; n's id. Or NULL: no card |
| `cardLapses` | the sum of `lapses`; n's id. Or NULL: no card |
| `cardMod` | the largest `mod` of n's cards; n's id. Or NULL: no card |
| `noteCrt` | n's `id` |
| `noteMod` | n's `mod` |
| `note` | the note type rank of n's note type |
| `noteTags` | n's stored `tags` text, binary |
| `originalPosition` | P of n's first card (S5.1); n's id. Or NULL: no card |

- Aggregates range over all of n's cards, not only those the search matched (S9).
- The aggregates are exact SQL averages and sums: the 16-bit ease sum, the 32-bit interval product and the 32-bit Reviews and Lapses sums of the cells (`browser/columns` C13) apply to the cells only. A note with 27 review cards at `factor` 2,500 shows Avg. Ease 7 but sorts at 2,500 (250%).
- The note id key after an aggregate is Anki's: notes with an equal aggregate keep note id order, reversed when the sort is descending.

### S5.1 The position value P

P of a card: the card data `pos` when present (`search/card-extractors` X7); otherwise `due` as stored when `type` is 0; otherwise 0. A review card without `pos` has P 0, not NULL, so it sorts before new cards and after negative positions such as a filtered new card's −100000. New positions are normally at least 1, but 0 is possible (Reposition accepts a start of 0, and a collection without `nextPos` gives its first new card 0); a new card at position 0 then has an equal P to a review card without `pos`, so the two form one equal-key run in cards mode (S8) and tie on the aggregate in notes mode.

## S6. The Due key

E of a card, from its stored `due` and `type` (not its home due), in SQLite arithmetic (S2): 64-bit integers for every `due` in the `i32` range, a REAL result when `due` is REAL or when the product overflows 64 bits:

- `due` when `due` is above 1,000,000,000 or `type` is 0;
- otherwise (`due` − today) × 86,400 + now.

New positions, learning timestamps and review days so fall on one scale (R1143): positions are small numbers, the others are instants near now. Ascending, new cards come before the rest, in position order; then, by instant, overdue reviews, today's reviews and learning cards due later today, then future days (an interday learning card due tomorrow and a review due tomorrow have the same E). A card in a filtered deck sorts by its filtered-deck `due` (−100000 and up): a filtered review card comes before every new card, a filtered new card before every other new card.

| Card (today 0, now 1,894,701,600 = 2030-01-15 10:00 UTC) | E |
|---|---|
| new, `due` 5 | 5 |
| learning, `queue` 1, `due` 1,894,702,200 | 1,894,702,200 |
| review, `due` −3 | 1,894,442,400 |
| review, `due` 0 | 1,894,701,600 |
| review or interday learning, `due` 1 | 1,894,788,000 |
| review in a filtered deck, `due` −100000 | −6,745,298,400 |
| new in a filtered deck, `due` −99999 | −99,999 |

Suspended and buried cards sort by the same E as any other card.

E is the sort's own value: the Due cell (`browser/columns` C7.5, from the home due) and the `prop:due` search (`search/properties` P4) compute theirs differently, by design. A suspended learning card has an E, an empty Due cell, and never matches `prop:due`.

## S7. The Retrievability key

R(c) is the retrievability of `search/card-extractors` X5 (the function `meguri_retrievability` of X2), called with the arguments of X1: the card's `data`; D, its `odue` when that is not 0 and its `due` otherwise; its `ivl`; and today and now of the search (S10). It is NULL without a memory state (X5 step 1; card data that is empty or unreadable has none) and when the result is NaN (X2.2). R is compared as a number.

This is the quantity `prop:r` compares (U82): both call the same function. It equals the Retrievability cell (`browser/columns` C11) when `lrt` is present and lies in 0..now; otherwise the two can differ. Note that D tests `odue`, not `odid`, for the home due.

## S8. Direction, ties and reverse order

- **Ascending** compares the keys of S4 or S5 in order; **descending** compares the same keys, each reversed, secondary keys included (NULL then comes last). So descending Sort Field lists a note's cards in decreasing ordinal, and descending Card Type lists Cloze 10 before Cloze 2.
- **Final key** (Meguri, R1145): the card id in cards mode, the note id in notes mode, ascending in an ascending sort and descending in a descending one. The order is then total, and reversing an ascending result gives exactly the descending result, so reversing in memory equals a descending query (R1142, U85).
- **Equal-key run**: rows equal on every S4 or S5 key, the final key aside. Anki returns such rows in an order its database chooses, the same in both directions: table order for some columns, an index's order for others. Meguri orders them by the final key. The conformance observation compares each run as a set (`specs/conformance/browser-ops.md` B2).
- Where Anki already ends with an id key (notes-mode aggregates, cards-mode Position's note id, Created), its order is fully fixed and Meguri reproduces it exactly: the equal-key runs there are the NULL notes and, for Position, the cards of one note.

Example: four cards with equal `mod` sorted by Card Modified are one equal-key run: Meguri lists them by ascending card id, or descending for a descending sort; Anki lists them in table order both ways.

## S9. Rows of a sorted search

- In cards mode, a sort on Sort Field, Card Type, Created, Note Modified, Note Type or Tags tests only cards whose note row exists, as a query that reads note columns does (`search/query-model` Q5): a card without a note, which only a damaged collection holds, is left out of such a sorted result, while an unsorted search or any other sort returns it. A Browse handle keeps such cards aside rather than forgetting them, so a resort or toggle lists them again under a sort that returns them (`browser/result-sets` H5, H8, H11). In notes mode the result set never changes; a note without cards gets NULL keys where S5 says so.
- Keys over a note's cards (S5, S5.1, the first card) use every card of the note, also cards the search did not match: notes mode sorted by Due with the search `is:new` still orders a note with one new and one review card by its review card.

## S10. Meguri constraints

- No TEMP table or other object is created for a sort: reader connections are `query_only` (`storage/open-and-connections`) and nothing may be added to the collection (R2016). Ranks and per-note aggregates are subqueries, joins or window functions inside the statement, or keys computed in memory over the result. They are computed afresh for each sort, so a deck rename or a new note type is seen at once.
- Due and Retrievability read now and today from the search context (`search/query-model` Q8) once, when the search or resort starts (`browser/result-sets` H8), for every row.
- A sort change shows its first page within R1083's budget on the 150k-card fixture (U90).

## S11. Scenarios

Observation: `search` with `order` (`specs/conformance/browser-ops.md` B1, B2). Every scenario has its golden (B6). Each scenario sorts its whole collection in both modes and both directions, unless noted.

| Scenario | Covers |
|---|---|
| `browser/sort/sort-field` | S2: numbers, empty text, NOCASE pairs, non-ASCII after `z`, HTML stripped into `sfld`; the ordinal key reversed |
| `browser/sort/card-type` | S3 template rank: equal names across note types, unicase-equal names, cloze numbers 1, 2 and 10; notes-mode card counts and their note id order |
| `browser/sort/deck` | S3 deck rank with `::`, spaces, digits, `_`, non-ASCII; a filtered card by its filtered deck; notes-mode distinct-deck counts and the first card's deck |
| `browser/sort/due` | S6 on every card state; notes mode over eligible cards only, a note with only new cards first ascending and last descending (AE50) |
| `browser/sort/ease` | new cards last ascending, factor ties, learning cards' 0; notes-mode averages and NULL notes |
| `browser/sort/interval` | `ivl`; notes-mode average over review and relearning cards only |
| `browser/sort/counts` | Reviews and Lapses; notes-mode sums and their note id order |
| `browser/sort/created` | note id then ordinal; notes mode note id |
| `browser/sort/modified` | Card Modified and Note Modified with equal and distinct stamps; notes-mode largest `mod` |
| `browser/sort/note-type` | S3 note type rank under unicase |
| `browser/sort/tags` | binary order of stored tags: capitals before small letters, non-ASCII last, untagged first |
| `browser/sort/position` | S5.1: `pos`, new positions, review cards without `pos`, a filtered new card, a note whose first card is ordinal 1 |
| `browser/sort/fsrs` | Stability, Difficulty, Retrievability in cards mode: NULLs, `s` without `d`, cards with and without `lrt`; notes mode refuses all three |
| `browser/sort/unsortable` | S1: Question and Answer in both modes, unknown and empty keys; parse errors win over `unsortable`, and `unsortable` wins over run errors such as an invalid regex (browser-ops B1). Its regex row needs the regex term (U81, tag `needs-text`) |
| `browser/sort/damaged` | S9: a card without a note dropped by note-column sorts only; a note without cards first ascending |
| `browser/sort/subset` | S9: aggregates over all of a note's cards under a restricting search. Needs the state and deck terms (U82, tags `needs-state`, `needs-deck`) |
| `browser/sort/catalog-agreement` | S1 and `browser/config-keys` K3: every one of the 19 keys in both modes and both directions; unsortable exactly where S1 says (Question and Answer in both modes, Stability, Difficulty and Retrievability in notes mode), and every sortable key orders rows that differ in it where the setup lets them differ. No card has a memory state, so the cards-mode Stability, Difficulty and Retrievability keys are NULL on every card and show only that the keys are accepted (their order is `browser/sort/fsrs`) |

The oracle driver implements `order`. The testkit's scenario model reads it. Meguri's executor registers the `search` observation kind but does not implement `order` yet, so T0 runs a sorted `search` and gives the scenario `error` (`unsupported_observation`, `dsl.md` D10), not `pending`, until U84's adapter implements it (browser-ops B6). While the scenarios are inactive this does not fail T0 (D10). A scenario that also observes `browser_rows` is `pending` instead, because the executor lacks that kind (`browser/columns` C15). `browser/sort/position` and `subset` name the card that `edit_note` adds with `adopt` (browser-ops B5). `browser/sort/damaged` breaks rows with `db_exec` as the active `card_excerpts` scenarios `browser/columns/question-deck/damaged` and `damaged-notetype` do (a missing deck, an ordinal with no template, a deleted note row: the row errors of `browser/columns` C3.1), and also deletes one note's cards; it shows only where such rows sort.

Meguri-only tests (U84, `tests/sort_props.rs`): for every sortable column and mode, the reversed ascending result equals the descending result (S8); the final key makes every order total; no sort creates a schema object.

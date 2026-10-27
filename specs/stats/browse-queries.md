# Stats browse queries

Spec: stats/browse-queries
Requirements: R1351, R1383, R1393, R1417, R1419, R1420, AE60
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/browse-queries/

The search text a click on a Stats graph opens in the Browser: the scope's part, the item's part, and how the two are joined. Scopes are `stats/graphs-common` G2. Histogram bar queries are `stats/graph-presentation` (U226); this file states the scope text, the join and the queries of fixed items. Search syntax is `search/grammar`, canonical text `search/canonical-form`. The general join and replace services of the search field are `search/join-and-replace` (U88); the Stats join below is its own rule, because R1417 and AE60 fix its exact text.

Decided 2026-10-27: U88's `join` writes canonical text (`search/join-and-replace` J6), which cannot give AE60's bytes (`deck:"Japanese"` quoted, and an explicit `AND`, which canonical text writes as one space, `search/canonical-form`), so the Stats link does not use U88's builder or `join`. This file governs Stats "Show in Browser". `search/join-and-replace`'s list of callers ("Stats \"Show in Browser\"") and the plan's U98 approach 5 and U225 ("through U88's builder and `join`") are superseded for Stats; their owners drop Stats from those lines. U88 may still test that AE60's text parses and that its canonical form equals a golden: that test does not write the Stats text.

## Q1. Who builds what

| Part | Built by | Rule |
|---|---|---|
| scope text | U225's `StatsBrowseLink` | Q2 |
| item text of a fixed item (Card Counts label, Calendar cell) | the core, in the graph's record (`stats/graphs-common` G6) | Q4 |
| item text of a histogram bar | U226 | `stats/graph-presentation` |
| joined text | U225's `StatsBrowseLink`, the only place that joins | Q3 |

Hand-written concatenation elsewhere is a review finding. Every text here is Anki search syntax only, so it also works when the user saves it and opens it in Anki (R1420).

## Q2. Scope text

### Q2.1 Collection

No text: the item is used alone.

### Q2.2 Deck

For Deck `D`, `deck:"` + E + `"`, where E is `D`'s full name with `::` between levels and each of these characters escaped (R1351):

| Character | Written |
|---|---|
| `\` | `\\` |
| `"` | `\"` |
| `*` | `\*` |
| `_` | `\_` |

Every other character is written as it is. Read back, the text matches exactly the decks of `stats/graphs-common` G2.1, wildcards and quotes included (`search/names-and-ids` N3.3).

Some names cannot be matched that way:

- The full name is exactly `current` or `filtered`, which `deck:` reads as special values (`search/names-and-ids` N3.2). The scope text is `deck:"Current"` or `deck:"Filtered"`: N3.2 reads those as ordinary names, and letter case folds, so they match that deck and its subdecks only. This is `search/join-and-replace` J3.1's form for the same names.

For the other names the scope text is `did:` + the ids of the scope's decks (G2.1, read when the link is built), in ascending order, joined by `,`, because no `deck:` text gives back the stored name:

- a level holds a White_Space character other than U+0020, which the search turns into a space (`search/grammar` G1);
- a level is not in NFC, holds an ASCII control character, or starts or ends with White_Space or `:` (the value's levels are normalized before matching, N3.3 step 2, and the stored name's are not);
- a level is empty, which the value's normalization turns into `blank` (N3.3 step 2);
- a level holds `::`, which the value splits into two levels (N3.3 step 2).

`did:` matches `did` or a non-zero `odid` in the list, as the Deck scope does (`search/names-and-ids` N3.5). Decided 2026-10-27: the id list keeps the Browser's cards equal to the graph's for these names, at the cost of a text that a later rename does not follow. `current` and `filtered` take the capitalized name instead, because a `deck:` text matches them exactly and stays readable. `search/join-and-replace` J3 writes the `did:` cases' names by its own rules (J3, J10), not as an id list; the Stats link differs because its text must give exactly the graph's cards (R1417), and its Deck scope is an id (G2.1).

### Q2.3 Search

The typed text with leading and trailing White_Space removed; nothing else changes. The text has parsed already: an invalid search shows its error and makes no graph clickable (`stats/graphs-common` G8.2). A text that is empty after the trim is the Collection scope (Q2.1).

## Q3. The join

With scope text S and item text I:

- Collection: I.
- Deck or Search: `(` + S + `) AND ` + I.
- When I has an `OR` at its top level (outside every group), it is written `(` + I + `)` in the join. None of Q4's items has one.

Nothing else is added, removed or rewritten: the joined text is not put into canonical form (`search/canonical-form`), so AE60's text stays exact. Wrapping a text that parses in a group never changes its meaning (`search/grammar` G2, G5): the joined text matches exactly the cards the scope's search matches that also match I (R1417).

| Scope | Item | Joined text |
|---|---|---|
| Collection | `"is:new"` | `"is:new"` |
| Deck `Japanese`, Young, separate on (AE60) | `("is:review" AND -"is:learn") AND "prop:ivl<21" AND -("is:buried" OR "is:suspended")` | `(deck:"Japanese") AND ("is:review" AND -"is:learn") AND "prop:ivl<21" AND -("is:buried" OR "is:suspended")` |
| Deck `a_b*` | `"is:new"` | `(deck:"a\_b\*") AND "is:new"` |
| Deck `x "y"` | `"is:buried"` | `(deck:"x \"y\"") AND "is:buried"` |
| Deck `a\b::c` | `"is:new"` | `(deck:"a\\b::c") AND "is:new"` |
| Deck `Lang::日本語` | Calendar cell −3 | `(deck:"Lang::日本語") AND "prop:rated=-3"` |
| Deck `current` with child `current::a` | `"is:suspended"` | `(deck:"Current") AND "is:suspended"` |
| Deck `a` + U+00A0 + `b` (id 1700000000001) with child (id 1700000000002) | `"is:suspended"` | `(did:1700000000001,1700000000002) AND "is:suspended"` |
| Search `tag:a or tag:b` | `"is:buried"` | `(tag:a or tag:b) AND "is:buried"` |
| Search `  is:due  ` | `"is:new"` | `(is:due) AND "is:new"` |
| Search of spaces only | `"is:new"` | `"is:new"` |

## Q4. Fixed-item queries

### Q4.1 Card Counts labels (R1393)

With X = ` AND -("is:buried" OR "is:suspended")` when `cardCountsSeparateInactive` is true (`stats/graphs-common` G7), and empty otherwise:

| Label | Item text | Shown |
|---|---|---|
| New | `"is:new"` + X | always |
| Learning | `(-"is:review" AND "is:learn")` + X | always |
| Relearning | `("is:review" AND "is:learn")` + X | always |
| Young | `("is:review" AND -"is:learn") AND "prop:ivl<21"` + X | always |
| Mature | `("is:review" AND -"is:learn") AND "prop:ivl>=21"` + X | always |
| Suspended | `"is:suspended"` | only when separating |
| Buried | `"is:buried"` | only when separating |

- X is never appended to Suspended and Buried, as in Anki: with it they would match nothing. R1393's "each with ... appended" covers the five type labels only. Decided 2026-10-27.
- When not separating, Suspended and Buried have no row and no query. The Total row has no query.
- Each label's joined text matches exactly the cards its count counts, in every scope (AE60). That is the test of U101's counting rules against these texts.

### Q4.2 Calendar cells (R1383)

- A cell whose count is above 0 and whose day index d (`stats/day-buckets` B3) is 0 or below has the item text `"prop:rated=` + d + `"`, d in decimal with `-` for negative values: `"prop:rated=0"` for today, `"prop:rated=-3"`.
- A cell with no reviews has no query.
- A cell with d ≥ 1 (rows dated after today, clock skew) has no query: `prop:rated` accepts only days of at most 0 (`search/history` H8). Decided 2026-10-27, for R1420.
- The Browser then lists the cards with an answer (`ease` 1 or above) in search day d (`search/history` H8). That is not the cell's count, which counts rows, of every answer kind, by B3's day. The two differ by design, in Meguri as in Anki.

## Q5. Opening the Browser

- The joined text goes to U150's `openBrowser(search:)` route (R1417). The Browser opens in the same window (sidebar Browse on Mac and iPad, the Browse tab on iPhone), shows exactly that text in its search field, and runs it in its current cards or notes mode, as Anki's Browser does with a search handed to it.
- On touch, the item's tooltip shows "Show in Browser" first; on Mac a click opens at once (R1418, U226).
- Coming back restores the Stats screen as it was (R1419, U225).

## Q6. Validity

- Every item text of Q4, joined with each scope kind and with the deck names `a_b`, `a*b`, `a"b`, `a\b`, `(x)`, `-x`, `x::current`, `Filtered`, `current`, `filtered`, `日本語::子`, `a b`, a name holding U+00A0, a name with an empty level and a level holding `::`, parses with the core's parser (U80) without error (R1420).
- Each of those texts round-trips through the canonical writer (`search/canonical-form`, U80): its canonical text parses again to the same query, and the canonical text of that is the same text. The joined text itself is not replaced by its canonical form (Q3).
- Its card set equals the scope's cards (`stats/graphs-common` G2) that match the item, on the fixture collection (property test, U98).
- U226's bar queries meet the same two checks through this join.

## Q7. Differences from Anki

Anki hands its Browser the text: a space, the scope search, a space, the item text. The scope search is `deck:current` for its Deck scope, empty for Collection, and the typed text for a search. Meguri differs:

- **Brackets and AND.** Without brackets, a scope such as `tag:a or tag:b` joined with an item means `tag:a OR (tag:b AND item)` in Anki. Meguri brackets the scope and writes `AND` (R1417), so the Browser lists the scope's cards that match the item. The text differs from Anki's for every Deck and Search scope; the cards differ only when the scope has a top-level `OR`. Pending `conformance/deviations.yaml` entry `stats-browse-join-grouped`, added with scenario `search-scope-or` (Q8); it covers this item, Deck by name and Card Counts spacing.
- **Deck by name.** The deck's name (Q2.2) instead of `deck:current` (R1353): the same cards while the current deck is the scope's deck.
- **Card Counts spacing.** Anki's item texts have no space before the `AND -(` suffix and write the Mature item's inner AND implicitly. They match the same cards; Meguri's are R1393's texts.
- **Future Calendar cells.** Anki makes them clickable with `"prop:rated=1"` and the like, which its search rejects. Meguri gives them no query (Q4.2). Pending `conformance/deviations.yaml` entry `stats-calendar-future-cell`, added with the Meguri-only test of Q8 that asserts it.

## Q8. Scenarios

`conformance/scenarios/stats/browse-queries/`, for the oracle-maintainer. They observe `search` (`conformance/search-ops` S1) for the joined texts, `canonical_search` for parsing, and `stats` (`stats/graphs-common` G14) for the counts. None exists yet: the `stats` kind has no executor.

| Scenario | Pins | Expected |
|---|---|---|
| `card-counts-labels` | Q4.1 | one card in each state (new, learning, relearning, young, mature, suspended review, buried new), in deck `Lang`: for Collection and Deck `Lang`, separating on and off, each label's joined text finds exactly the cards its `card_counts` entry counts |
| `ae60-young` (AE60) | Q3, Q4.1 | deck `Japanese`, 4 young cards of which 1 suspended, separating on: the AE60 text finds the 3 others, the Young count is 3 |
| `deck-scope-escaping` | Q2.2 | decks `a_b`, `axb`, `a*b`, `ab`, `a"b`, `a\b`, `(x)`, `-x`, `日本語::子`, one card each: `(deck:"<E>") AND "is:new"` for each finds only that deck's card (and `日本語::子`'s under `日本語`) |
| `search-scope-or` | Q3, Q7 | tags `a` and `b`, new and review cards: `(tag:a or tag:b) AND "is:new"` finds the new cards of both tags; Anki's ` tag:a or tag:b "is:new"` also finds `a`'s review cards |
| `calendar-cells` | Q4.2 | answers at `N` − 86,400 s exactly, at `N` − 86,400 s + 1 s and two days back, plus a Manual row with `ease` 0 today: `"prop:rated=0"`, `"prop:rated=-1"` and `"prop:rated=-2"` find the cards H8 gives; the `reviews` day counts are B3's |
| `parse-all` | Q6 | `canonical_search` of every joined text of Q3's table and of the separated and plain label texts for Deck `Lang`: each parses |

Meguri-only tests: the `did:` form, with a name of each Q2.2 case (an empty level and a level holding `::` included), and `deck:"Current"` for decks `current` and `current::a` (Q2.2), future cells without a query (Q4.2), the top-level-OR wrap of an item (Q3), and the hand-off to the Browser (Q5, U225's UI tests).

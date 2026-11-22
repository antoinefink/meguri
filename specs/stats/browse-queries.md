# Stats browse queries

Spec: stats/browse-queries
Requirements: R1351, R1383, R1393, R1417, R1419, R1420, AE60
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/browse-queries/
Status: Approved 2026-11-22 (R1357) for Q9 and the lines that begin "Changed 2026-11-22 (round 11)". Approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/statistics-round11-20261122.md`); Antoine approved the round 11 design in chat on 2026-11-22 (`docs/design/decisions.md`, rows `stats.overview` of 2026-11-22). Every other section is in force and unchanged.

The search text a click on a Stats graph opens in the Browser: the scope's part, the item's part, and how the two are joined. Scopes are `stats/graphs-common` G2. Histogram bar queries are `stats/graph-presentation` (U226); this file states the scope text, the join and the queries of fixed items. Search syntax is `search/grammar`, canonical text `search/canonical-form`. The general join and replace services of the search field are `search/join-and-replace` (U88); the Stats join below is its own rule, because R1417 and AE60 fix its exact text.

Decided 2026-10-27: U88's `join` writes canonical text (`search/join-and-replace` J6), which cannot give AE60's bytes (`deck:"Japanese"` quoted, and an explicit `AND`, which canonical text writes as one space, `search/canonical-form`), so the Stats link does not use U88's builder or `join`. This file governs Stats "Show in Browser". `search/join-and-replace`'s list of callers ("Stats \"Show in Browser\"") and the plan's U98 approach 5 and U225 ("through U88's builder and `join`") are superseded for Stats; their owners drop Stats from those lines. U88 may still test that AE60's text parses and that its canonical form equals a golden: that test does not write the Stats text.

## Q1. Who builds what

Changed 2026-11-22 (round 11): the builders are now U101 (the Card Counts label texts in the record, C4.3), U98 (`stats_browse_scope`), U294 (the item texts of Q9's bars, from the page's bins) and U303 (`StatsBrowseLink`, the only place that joins); U225 and U226 are superseded (`docs/plans/2026-11-22-1200-feat-statistics-page-implementation-plan.md`). The rules below are unchanged.

| Part | Built by | Rule |
|---|---|---|
| scope text | the core (U98's `stats_browse_scope`), from the request's snapshot, in the Data result (`stats/graphs-common` G8.2) | Q2 |
| item text of a fixed item (Card Counts label, Calendar cell) | the core, in the graph's record (`stats/graphs-common` G6) | Q4 |
| item text of a histogram bar | U226 | `stats/graph-presentation` |
| joined text | U225's `StatsBrowseLink`, the only place that joins | Q3 |

Decided 2026-10-27: the core writes the scope text, because Q2.2 needs what only the core holds: the ids of G2.1's deck set under its simple case folding, and the stored levels (U+001F-separated), which the `::`-joined display name cannot show (a level holding `::`). The scope text is one of: none (Collection, Q2.1), a text S (Q2.2, Q2.3), or `unavailable` (Q2.2: no item of the scope is clickable). `StatsBrowseLink` only wraps and joins (Q3); it never inspects deck names. Rust tests of `stats_browse_scope` (U98, `browse_queries.rs`) cover every case of Q2.

Hand-written concatenation elsewhere is a review finding, with one exception: the test-only join of Q6. Every text here is Anki search syntax only, so it also works when the user saves it and opens it in Anki (R1420).

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

For the other names the scope text is `did:` + the ids of the scope's decks (G2.1, read from the request's snapshot), in ascending order, joined by `,`, because no `deck:` text gives back the stored name:

- a level holds a White_Space character other than U+0020, which the search turns into a space (`search/grammar` G1);
- a level is not in NFC, holds an ASCII control character, or starts or ends with White_Space or `:` (the value's levels are normalized before matching, N3.3 step 2, and the stored name's are not);
- a level is empty, which the value's normalization turns into `blank` (N3.3 step 2);
- a level holds `::`, which the value splits into two levels (N3.3 step 2).

`did:` matches `did` or a non-zero `odid` in the list, as the Deck scope does (`search/names-and-ids` N3.5).

`did:` takes only digit runs (`search/grammar` G8), so a negative id cannot be written. Decided 2026-10-27: when the name needs the `did:` form and any id of the scope's decks is negative (an imported or hand-built collection), the scope text is `unavailable`: no item of that scope is clickable, as for an invalid search (Q2.3). A `deck:` text instead would give other cards than the graph's (R1417), and an id list without those decks would too. A negative id with a name that `deck:` matches exactly changes nothing. Decided 2026-10-27: the id list keeps the Browser's cards equal to the graph's for these names, at the cost of a text that a later rename does not follow. `current` and `filtered` take the capitalized name instead, because a `deck:` text matches them exactly and stays readable. `search/join-and-replace` J3 writes the `did:` cases' names by its own rules (J3, J10), not as an id list; the Stats link differs because its text must give exactly the graph's cards (R1417), and its Deck scope is an id (G2.1).

### Q2.3 Search

The text the core received with leading and trailing White_Space removed; nothing else changes. That text is the field's after `search/join-and-replace` J2 (U+001C to U+001F already U+0020, `stats/graphs-common` G2), so the Browser's own J2 pass on the handed search (Q5) changes nothing and its cards are the graph's. The text has parsed already: an invalid search shows its error and makes no graph clickable (`stats/graphs-common` G8.2). A text that is empty after the trim is the Collection scope (Q2.1).

## Q3. The join

With scope text S and item text I:

- Collection: I, unchanged, even when it has a top-level `OR`.
- Deck or Search: `(` + S + `) AND ` + I, where I is written `(` + I + `)` when it has an `OR` at its top level (outside every group). None of Q4's items has one.
- Scope text `unavailable` (Q2.2): no joined text; the item is not clickable.

Nothing else is added, removed or rewritten: the joined text is not put into canonical form (`search/canonical-form`), so AE60's text stays exact. Wrapping a text that parses in a group never changes its meaning (`search/grammar` G2, G5): the joined text matches exactly the cards the scope's search matches that also match I (R1417).

| Scope | Item | Joined text |
|---|---|---|
| Collection | `"is:new"` | `"is:new"` |
| Collection | `"is:new" OR "is:due"` | `"is:new" OR "is:due"` |
| Deck `Lang` | `"is:new" OR "is:due"` | `(deck:"Lang") AND ("is:new" OR "is:due")` |
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
| Deck `a` + U+00A0 + `b` (id −5) | `"is:new"` | none (`unavailable`) |

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

Changed 2026-11-22 (round 11): the Statistics page opens Browse as Q9 says, not with the joined text: it hands Browse the tokens Q9 makes (the deck's part and the item's part), and the field shows those tokens, not Q3's text; its action is "Show in Browse" (`stats/statistics-page` S8.4). The other bullets apply (same window, cards or notes mode, coming back restores the page, recent searches record the tokens' canonical form).

- The joined text goes to the `open(search:)` route on AppNavigation (R1417, U210; the plan's U225 calls the same route U150's `openBrowser(search:)`, `browser/entry-points` E1). The Browser opens in the same window (sidebar Browse on Mac and iPad, the Browse tab on iPhone), shows exactly that text in its search field, and runs it in its current cards or notes mode, as Anki's Browser does with a search handed to it.
- On touch, the item's tooltip shows "Show in Browser" first; on Mac a click opens at once (R1418, U226).
- Coming back restores the Stats screen as it was (R1419, U225).
- Browse records the joined text's canonical form in recent searches while showing and running the text as written; a joined text that does not parse (unreachable, Q2.3, Q6) is shown as written with its error inline, and nothing runs or is recorded (`browser/entry-points` E3 rules 4 and 5, E6).

## Q6. Validity

- Every item text of Q4, joined with each scope kind and with the deck names `a_b`, `a*b`, `a"b`, `a\b`, `(x)`, `-x`, `x::current`, `Filtered`, `current`, `filtered`, `日本語::子`, `a b`, a name holding U+00A0, a name with an empty level and a level holding `::`, parses with the core's parser (U80) without error (R1420).
- Each of those texts round-trips through the canonical writer (`search/canonical-form`, U80): its canonical text parses again to the same query, and the canonical text of that is the same text. The joined text itself is not replaced by its canonical form (Q3).
- Its card set equals the scope's cards (`stats/graphs-common` G2) that match the item, on the fixture collection (property test, U98).
- Decided 2026-10-27, where the joined texts of these tests come from: U98's Rust tests may hold one test-only join in the test code, never in shipped code, built from the scope text of `stats_browse_scope`. A test asserts that it gives every row of Q3's table byte for byte, and U225's Swift tests assert the same table for `StatsBrowseLink`, so both joins are pinned to one table.
- U226's bar queries meet the same two checks through this join.

## Q7. Differences from Anki

Anki hands its Browser the text: a space, the scope search, a space, the item text. The scope search is `deck:current` for its Deck scope, empty for Collection, and the typed text for a search. Meguri differs:

- **Brackets and AND.** Without brackets, a scope such as `tag:a or tag:b` joined with an item means `tag:a OR (tag:b AND item)` in Anki. Meguri brackets the scope and writes `AND` (R1417), so the Browser lists the scope's cards that match the item. The text differs from Anki's for every Deck and Search scope; the cards differ only when the scope has a top-level `OR`. No `conformance/deviations.yaml` entry: no scenario observes either side's join. `search-scope-or` (Q8) searches literal texts, Meguri's joined text and Anki's, which both sides read alike, so its golden needs no transform; Meguri's own join output is pinned by the Meguri-only test of Q3's table (Q6). This covers this item, Deck by name and Card Counts spacing. An entry `stats-browse-join-grouped` is added only if a later observation compares the joined text itself.
- **Deck by name.** The deck's name (Q2.2) instead of `deck:current` (R1353): the same cards while the current deck is the scope's deck.
- **Card Counts spacing.** Anki's item texts have no space before the `AND -(` suffix and write the Mature item's inner AND implicitly. They match the same cards; Meguri's are R1393's texts.
- **Future Calendar cells.** Anki makes them clickable with `"prop:rated=1"` and the like, which its search rejects. Meguri gives them no query (Q4.2). Pending `conformance/deviations.yaml` entry `stats-calendar-future-cell`, added with the Meguri-only test of Q8 that asserts it.

## Q8. Scenarios

`conformance/scenarios/stats/browse-queries/`, for the oracle-maintainer. They observe `search` (`conformance/search-ops` S1) for the joined texts, `canonical_search` for parsing, and `stats` (`stats/graphs-common` G14) for the counts. As of 2026-11-10 all seven are in the tree, inactive. They need no new kind, so their goldens come from the Anki oracle; Meguri's runner waits for the `stats` adapter (`conformance/stats-ops` H10).

| Scenario | Pins | Expected |
|---|---|---|
| `card-counts-labels` | Q4.1, Q3 | in deck `Lang`: new, learning, relearning, young, mature, suspended review, sibling-buried new, user-buried learning and young, suspended new, learning and relearning cards, a user-buried relearning card, and a young card now in the filtered deck `Cram` (`conformance/deck-ops` K13) with `odid` `Lang`; a new and a mature card in `Other`; tags `p` and `q` on some of them. For Collection, Deck `Lang` and Search `tag:p or tag:q`, separating on and off, each label's joined text finds exactly the cards its `card_counts` entry counts (`Cram`'s card inside Deck `Lang`, `stats/graphs-common` G2.1) |
| `ae60-young` (AE60) | Q3, Q4.1 | deck `Japanese`, 4 young cards of which 1 suspended, separating on: the AE60 text finds the 3 others, the Young count is 3 |
| `deck-scope-escaping` | Q2.2 | decks `a_b`, `axb`, `a*b`, `ab`, `a"b`, `a\b`, `(x)`, `-x`, `日本語::子`, one card each: `(deck:"<E>") AND "is:new"` for each finds only that deck's card (and `日本語::子`'s under `日本語`) |
| `search-scope-or` | Q3, Q7 | tags `a` and `b`, new and review cards: `(tag:a or tag:b) AND "is:new"` finds the new cards of both tags; Anki's ` tag:a or tag:b "is:new"` also finds `a`'s review cards |
| `calendar-cells` | Q4.2 | answers at `N` − 86,400 s exactly, at `N` − 86,400 s + 1 s and two days back, plus a Manual row with `ease` 0 today: `"prop:rated=0"`, `"prop:rated=-1"` and `"prop:rated=-2"` find the cards H8 gives; the `reviews` day counts are B3's |
| `special-deck-names` | Q2.2 | decks `current`, `current::a`, `x::current`, `filtered`, `filtered::b`, `a b` and `Other`, a filtered deck `Cram` holding a card of `Other`, and raw deck rows with a level holding U+00A0 (and a child), a level holding `::` and a level starting with a space, `curDeck` on the U+00A0 deck: `deck:"Current"` and `deck:"Filtered"` find that deck and its subdecks, not the current deck's or every filtered deck's cards; the `did:` lists find their decks' cards, while the `deck:` texts of those names find other decks or nothing. Search side only: the texts `stats_browse_scope` writes and their equality with the Deck scope's cards are Meguri-only (`stats/graphs-common` G13) |
| `parse-all` | Q6 | `canonical_search` of every joined text of Q3's table and of the separated and plain label texts for Deck `Lang`: each parses |

Meguri-only tests: the `did:` form, with a name of each Q2.2 case (an empty level and a level holding `::` included), `deck:"Current"` for decks `current` and `current::a`, and `unavailable` for a `did:` case with a negative deck id (Q2.2), all as `stats_browse_scope` returns them (Q1); `deck:"\_\*"` for a deck named `_*` finding only that deck's cards (`stats/graphs-common` G13); future cells without a query (Q4.2); the top-level-OR wrap of an item in a Deck scope and its absence in Collection (Q3); a Stats search holding U+001F, whose graph scope and Browser cards are equal (Q2.3); Q3's table for both joins (Q6); and the hand-off to the Browser (Q5, U225's UI tests).

## Q9. The Statistics page's items (round 11)

Changed 2026-11-22 (round 11; `stats/statistics-page` S8.4). The Statistics page has no Search scope (`stats/history-reads` H1), so its scope text is Q2.1's or Q2.2's. Show in Browse opens Browse (Q5's route) with a query made of tokens (`search/filter-tokens`): the deck's part and the item's part, each read into tokens by `search/filter-tokens` T7 on its own, not as Q3's joined text (whose bracketed scope T7 would keep as one advanced token). The deck's part is the scope text of Q2.2: a name `deck:` matches becomes the Deck token, so its sidebar row is selected and its token menu works; Q2.2's `did:` text stays an advanced token. The item's part becomes the tokens T7 gives it, or an advanced token when no kind shows it. Tokens narrow each other, so the query finds what Q3's joined text finds. A card-ids item is made as the heatmap's day is (`browser/workspace` V15): the deck's part as above, when a deck is the filter, and a Studied On token holding the ids that `stats/history-reads` H13 returned. Scope text `unavailable` makes no item of the page clickable (Q2.2).

| Box (`stats/statistics-page`) | Item | Item text |
|---|---|---|
| Card Counts, a state row | the row | Q4.1's text with X, always separating (S3.4) |
| Reviews Due, a bar of days `a` to `b` (0 ≤ `a` ≤ `b`) | the cards due then | `"prop:due=<a>"` when `a` = `b`, else `"prop:due>=<a>" AND "prop:due<=<b>"` |
| Reviews Due, the Overdue bar | the overdue cards | `"prop:due<=-1"` |
| Added and Started, an Added bar of days `a` to `b` (`a` ≤ `b` ≤ 0) | the cards added then | `"added:<1 − a>"` when `b` = 0, else `"added:<1 − a>" AND -"added:<−b>"` |
| Stability, a bar of keys [`a`, `b`) | its cards | `"prop:s<0.5"` for the first bar (`a` = 0, negative stabilities included), `"prop:s>=729.5"` for the last ([730, ∞)), else `"prop:s>=<a − 0.5>" AND "prop:s<<b − 0.5>"` |
| Difficulty, a bar [`a`, `a` + 10) of percent keys | its cards | `"prop:d<0.1"` for the first, `"prop:d>=0.9"` for the last (keys of 100 and above included), else `"prop:d>=<a ÷ 100>" AND "prop:d<<(a + 10) ÷ 100>"` |
| Retrievability, a bar [`a`, `a` + 5) | its cards | `"prop:r<0.05"`, `"prop:r>=0.95"`, else `"prop:r>=<a ÷ 100>" AND "prop:r<<(a + 5) ÷ 100>"` |
| Reviews Over Time, a day bar | Cards Reviewed on <date> | card ids, H13 `reviewed`; Studied On token labelled "Studied on <date>" |
| Activity, a day with reviews | Cards Reviewed on <date> | card ids, H13 `answered`; the same token |
| Forgotten, a day bar | Cards Forgotten on <date> | card ids, H13 `forgotten`; Studied On token labelled "Forgotten on <date>" |

- Numbers are written as `stats/graph-presentation` P4.7 writes them (shortest decimal, no grouping).
- As in Anki and P6 and P10 to P11, a `prop:` query uses the search's own rules, which can differ from what the bar counts: `prop:due` never matches buried cards, which Future Due counts for k ≥ 1 (`stats/graphs-cards` C3.6), `prop:r` reads retrievability by X5's elapsed rule, not C9.1's (C9.6), `prop:d` compares the decimal difficulty while the bars use C8.1's f32 keys (d 2.08 and 4.6 fall one key low), and `prop:s` at exactly a half (s = 2.5) can sit on the other side of a bar edge from the key's rounding. The card-ids items find exactly the existing cards the bar's rows belong to; in All Decks a bar also counts deleted cards' rows, which have no card to list.
- A day item's date is the day's date (`stats/history-reads` H1). An empty day is not a link (the card-ids item refuses an empty list).
- Tests (Meguri-only): a card-ids item for a deck named with U+00A0 (a `did:` case with non-negative ids) opens Browse on exactly the graph's cards; each item text parses (Q6) and, on the fixture, finds the cards Q9 says for Deck and All Decks; the two card-ids tokens carry their labels; Back returns to the page's scroll position and selection (`stats/statistics-page` S20).

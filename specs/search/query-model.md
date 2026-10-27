# Search query model and modes

Spec: search/query-model
Requirements: R1021, R1023, R1024, R1025, R1080, R1085, R1916
Anki: 26.09.3
Scenarios: conformance/scenarios/search/structure/

What a parsed search means: its parts, which rows it returns in cards mode and notes mode, how terms combine on damaged collections, which outside values it reads, and what the compiled plan must record. The text side is `grammar.md`; what each term matches is U81 (`text-matching.md`, `fields.md`, `case-folding.md`) and U82 (`names-and-ids.md`, `state-and-flags.md`, `properties.md`, `history.md`).

## Q1. The query

- A parsed search is AND, OR and NOT over terms, plus groups. The shape is Meguri's own. It must keep everything `canonical-form.md` writes: groups as groups (also a group of one item), each negation, negated operator words, and each term's stored data (Q3).
- Within one sequence, AND binds tighter than OR (`grammar.md` G2): a sequence is the OR of its runs of AND-joined items. Implicit and explicit AND are the same. A group is one item of its parent sequence.
- NOT applies to exactly one item.

## Q2. The whole collection

The empty search (`grammar.md` G1) is the whole-collection term. It is true for every row, and it is the only way to write it: `deck:*` is a deck term (card scope, Q5) that is also true for every card, which matters on damaged collections (Q6).

## Q3. Terms and their data

Every term keeps what `grammar.md` G8 reads from it. The matching unit owns its meaning.

| Term | Data | Scope (Q5) | Matching |
|---|---|---|---|
| plain text | text | note | U81 |
| field term | field name, mode (normal, regex, nc), text | note | U81 |
| `re:` | pattern | note | U81 |
| `nc:`, `sc:`, `w:` | text | note | U81 |
| `tag:` | mode (normal, regex, nc), text | note | U82 |
| `note:` | name pattern | note | U82 |
| `mid:` | note type id (64-bit) | note | U82 |
| `edited:` | days (1 or more) | note | U82 |
| `dupe:` | note type id, text | note | U82 |
| `deck:` | name pattern | card | U82 |
| `preset:` | name, escapes kept | card | U82 |
| `did:`, `cid:` | id list text | card | U82 |
| `is:` | one of 8 states | card | U82 |
| `flag:` | 0–7 | card | U82 |
| `added:`, `introduced:` | days (1 or more) | card | U82 |
| `rated:` | days (1 or more), button (none or 1–4) | card | U82 |
| `resched:` | days (0 or more) | card | U82 |
| `prop:` | property (with key for `cdn:`/`cds:`), operator (one of six), operand (typed by property, `grammar.md` G8.1) | card | U82 |
| `has-cd:` | key | card | U82 |
| `card:` | template ordinal (0-based) or name pattern | card and note | U82 |
| `nid:` | id list text | either | U82 |
| whole collection | none | either | Q2 |
| negated operator word | `and` or `or` | none | never runs (`errors.md` E3) |

## Q4. Cards mode and notes mode

A search runs in one of two modes (R1080):

- **Cards mode** returns cards. A card matches when the query is true for that card together with its note.
- **Notes mode** returns notes. A note matches when at least one of its cards makes the whole query true, the query being tested on one card at a time with the note. Card terms therefore match a note if any of its cards matches, but terms are not tested independently against different cards: in notes mode `is:suspended is:review` returns the notes that have one card both suspended and in review, not a note with one suspended card and another review card.
- Negation follows from that: notes mode `-is:suspended` returns the notes that have at least one card that is not suspended, so a note with one suspended and one unsuspended card is returned by both `is:suspended` and `-is:suspended`.
- Each id appears once; the order is not part of this spec (no order: the set only; sorting is U84).

Example (`conformance/scenarios/search/structure/card-note-modes.yaml`): a note with a review card and a suspended new card. `is:suspended` returns the suspended card in cards mode and the note in notes mode; `is:suspended is:review` returns nothing in either mode.

## Q5. Scope

Each term has a scope (the Q3 column):

- **card**: it reads card columns only;
- **note**: it reads note columns only;
- **card and note**: it reads both;
- **either**: it reads only what both rows hold (the note id) or nothing.

The scope of a query combines its terms' scopes, through AND, OR, NOT and groups alike: "either" adds nothing; card with note gives card and note; card and note with anything gives card and note; equal scopes stay. A search made only of "either" terms has scope "either".

The mode then decides what each row is tested on:

| Mode | Query scope | Rows tested | Returned |
|---|---|---|---|
| cards | card or either | every card alone; its note need not exist | the cards for which the query is true |
| cards | note, or card and note | every card whose note exists, with that note | the cards for which the query is true |
| notes | note or either | every note alone; it need not have cards | the notes for which the query is true |
| notes | card, or card and note | every pair of a card and its existing note | the distinct notes of the pairs for which the query is true |

`nid:` reads the note id from the card when cards are tested alone, and from the note otherwise; the result is the same either way.

## Q6. Damaged collections

On a sound collection every card has a note and every note has a card, and Q5 gives the same results as Q4. A collection can still hold cards whose note row is gone and notes with no cards (Check Database repairs both, but searches run before it). Q5 then decides, as in Anki (`conformance/scenarios/search/structure/damaged-rows.yaml` and `whole-collection.yaml`):

- the empty search in cards mode returns every card, those without a note included; in notes mode every note, those without cards included;
- `deck:*` and every other card term return only cards in cards mode (cards without a note included), and in notes mode only notes that have cards;
- a note term (`dog`) in cards mode skips cards without a note; in notes mode it can return a note without cards;
- a query that mixes card and note terms (`is:new or cat`) tests only card-and-note pairs in both modes, so it returns neither cards without a note nor notes without cards, even where one of its terms alone would.

## Q7. Truth values

A term is true, false or unknown for a row; some terms of U82 are unknown where a value is missing (a custom-data key the card lacks, a card with no memory state). AND, OR and NOT follow SQL's three-valued logic: NOT unknown is unknown; false AND unknown is false; true OR unknown is true; otherwise unknown with anything is unknown. A row is returned only when the whole query is true. So a term and its negation can both leave a row out.

## Q8. Search context

Searching never calls scheduling or engine code (KTD2). Every outside value a query depends on comes in a `SearchContext` that the engine fills:

| Value | Source | Read by |
|---|---|---|
| now (epoch seconds) | the clock | `is:due`, `prop:r` |
| today (days elapsed) | U40 day clock | `is:due`, `prop:due`, `prop:r` |
| next-day-at (epoch seconds of the next day boundary) | U40 | `added:`, `edited:`, `introduced:`, `rated:`, `resched:`, `prop:due`, `prop:rated`, `prop:resched` |
| learn-ahead seconds | config `collapseTime` when it reads as an unsigned 32-bit integer; 1200 when it is missing or reads otherwise (`"x"`, −1, 60.0; `state-and-flags.md` S3) | `is:due` |
| current deck id | config `curDeck` | `deck:current` |
| note text normalization | config `normalize_note_text` (true when missing) | plain text, field text, `re:`, `nc:`, `sc:`, `w:`, `dupe:` |
| ignore accents | config `ignoreAccentsInSearch` (false when missing) | plain text, `sc:`, `w:` |
| fold mode | `Unicode` in the product; `AsciiCompat` only with the `test-hooks` Rust feature (U91) | the U81 text terms (`case-folding.md`) |

`search/state-and-flags#S1–S3` defines the due and buried leaves, including their exact context dependencies. The U81 and U82 specs are the authority on what each term reads; this table is the union U80 must carry. The plan records, for each term, which of these values it read, so that caches (U90) key on exactly those. The whole collection, NOT, AND, OR and groups read none.

## Q9. The plan

Compiling a parsed query gives a plan for one mode:

- **Statement.** One SQL statement that selects card ids (cards mode) or distinct note ids (notes mode), with Q5's rows: it reads only the cards table or only the notes table when the query scope allows it (Q5's first and third rows), and joins cards to notes otherwise. All user text goes in bound parameters; numbers and ids may be written into the statement. The statement holds no ordering (U84 adds it).
- **Terms.** AND, OR, NOT and groups compile to their SQL forms, keeping Q7's logic. Each term compiles through its owning unit (U81, U82); until that unit lands, a term may compile to a stub, and scenarios that need it stay inactive.
- **Slow terms.** Terms that call Meguri's SQL functions per row (regex, `nc:`, Unicode folding, `prop:r` and any other U81/U82 marks) are recorded as slow, so that U85 can choose streaming execution.
- **Context reads.** The set of Q8 values the plan read (Q8).
- **Errors.** Building the plan raises every cause of `errors.md` E3 except a database refusal: `search_failed` for a negated operator word, a non-ASCII digit in an id list, a pattern longer than the limit, a regular expression over the compiled size limit, no searchable field, all fields excluded, `sc:` with no cloze note type and a `prop:` operand out of range (`properties.md` P14), and `invalid_regex` (U81). They are checked in the order of `errors.md` E4 step 3, and the first one met is the error. Errors the database raises when the statement runs (the refusals of E3) map to `search_failed`.

## Q10. Streaming execution

A plan with slow terms (Q9) may run in two stages (U85, `browser/result-sets` H3). Its result, order included, must equal the result of the plan's statement run whole.

1. **Relaxed query.** Replace every slow term by TRUE when an even number of negations stand above it (none counts as even), and by FALSE when an odd number do. Groups, AND and OR do not count. In `-(re:x tag:y)` the slow term `re:x` is under one negation and becomes FALSE; in `re:x or tag:y` it becomes TRUE.
2. **Candidates.** The relaxed query runs in SQL, sorted by the request's sort with its final key (`browser/sorting` S8). It tests the rows that Q5 gives for the original query, not for the relaxed one: replacing `re:x` (note scope) does not make a cards-mode search return cards without a note.
3. **Superset.** Every row the original query returns is a candidate. Under Q7's order false < unknown < true, AND and OR never lower their value when an operand goes up, and NOT reverses the order; so each replacement raises the value of the whole query or keeps it, and a row for which the original is true is true for the relaxed one.
4. **Final test.** The candidates are taken in sort order, in chunks, and the original query is tested on each, as Q5 tests rows in its mode and scope (in notes mode with card or card-and-note scope, a candidate note is kept when one of its card-and-note pairs makes the query true; otherwise each candidate is tested as Q5's table says, so in notes mode with note or either scope the note alone, whether it has cards or not). A candidate is kept when the query is true. Because candidates come in final order, the kept rows of the chunks tested so far are the start of the result: the first page is final as soon as it is full or the candidates end.
5. **One snapshot.** Both stages, and the sort keys, read one read transaction.
6. **Errors.** As for the whole statement: plan errors before anything runs; a database error in either stage maps to `search_failed`.

The relaxed query is also the cheap-predicate key of U90's caches.

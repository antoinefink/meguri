# Studied today: the Home line and the Stats Today section

Spec: stats/studied-today
Requirements: R1277, R1278, R1279, R1282, R1288, R1295, R1296, R1297, R1298, R1300, R1369, R1370, R1371, R1372, R1448, R1889, R304, R95
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/studied-today/, conformance/scenarios/stats/today/

Two texts report today's answers:

- **The Home line** under the deck list: one sentence such as `Studied 3 cards in 13 seconds today (4.33s/card)`, for the whole collection (T2).
- **The Today section** at the top of the Stats screen: up to four lines for the screen's scope (T3, T4).

Anki builds the Home line in its core and the Today lines in its web page. The two use the same sentence for the studied count, but they read different rows (T3.1) and write numbers by different rules (T5). Meguri reproduces both exactly, so in rare cases the Home line and the first Today line differ, as they do in Anki (T4.5). Never write the two with one number writer.

Days come from `stats/day-buckets`, the rows and the Home line's two numbers from `stats/counting-rules` (N1, N6). Every string here is English and is shown as the core returns it (T6).

## T1. Terms

- **Row**: a review-log row (`stats/counting-rules` N1). Its columns here: `id` (the review instant, epoch milliseconds), `cid`, `ease`, `lastIvl`, `time` and `type`.
- **N**: `next_day_at`, the start of the next scheduler day in epoch seconds (`stats/day-buckets` B1).
- **S**: the start of Anki's day 0, `(N − 86,400) × 1,000` epoch milliseconds (`stats/day-buckets` B2).
- **Deleted card's row**: a row whose `cid` matches no card (N1).
- Rows hold integers in `type`, `ease` and `time` in every collection Anki or Meguri writes. `lastIvl` can also hold a fractional (REAL) value. Other storage classes are outside this spec.

## T2. The Home line

### T2.1 Numbers

The line is built from two numbers, `cards` and `time_ms`, which are exactly `stats/counting-rules` N6:

- the rows with `id > S` (strictly after the start: a row stamped exactly `S` is not counted), with no upper end, whose `type` is neither 4 nor 5;
- every deck, deleted cards' rows included, any `ease`; the heatmap settings never apply;
- `cards` is the number of those rows (R1278); `time_ms` the sum of their `time` values as stored, negative values and values above any answer-time limit included (R1279).

The core reads both from its resident values (`stats/counting-rules` N7): building the Home line runs no database query (R1288).

### T2.2 Text

```
Studied <count> <span> today (<per-card>s/card)
```

| Part | Text |
|---|---|
| `<count>` | `1 card` when `cards` is 1, otherwise `<cards> cards`. `cards` is written as plain decimal digits, never grouped (`1234 cards`). |
| `<span>` | `in <amount> second` or `in <amount> seconds` when the unit is seconds, `in <amount> minute` or `in <amount> minutes` when it is minutes. The singular form is used exactly when `<amount>` is written `1` or `-1` (T5.4). |
| `<per-card>` | `<per>`: the seconds per card. |

`<amount>` and `<per>` are computed and written by T5.1:

- the unit is seconds while the total is below 60 seconds in absolute value, and minutes from 60 seconds on. It is never hours or a larger unit, however long the total (R1297): five hours is `in 300 minutes`;
- `<amount>` is the total in that unit;
- `<per>` is the total in seconds divided by `cards`, and 0 when `cards` is 0.

Words are separated by single spaces. The line has no line break and no invisible characters (T6.3).

### T2.3 The zero line

With no counted row the line still shows (R1298):

```
Studied 0 cards in 0 seconds today (0s/card)
```

Decided 2026-11-10: this is the line, not an open question. It is the default the plan's deferred decision on the zero line sets (R1298 over R44), and Meguri follows that default. R44's `No cards studied today` is never shown. Only a new product decision changes this section.

### T2.4 Examples

Every row was checked against the oracle at the current pin. Scenario `studied-today/format-table` observes exactly these inputs when it lands (`specs/conformance/stats-ops.md` H8). The last column is the first Today line for the same numbers (T4.2), where it differs or helps.

| `cards` | `time_ms` | Home line | First Today line |
|---|---|---|---|
| 0 | 0 | `Studied 0 cards in 0 seconds today (0s/card)` | (empty state, T4.1) |
| 1 | 0 | `Studied 1 card in 0 seconds today (0s/card)` | same |
| 1 | 1000 | `Studied 1 card in 1 second today (1s/card)` | same |
| 2 | 1000 | `Studied 2 cards in 1 second today (0.5s/card)` | same |
| 3 | 13000 | `Studied 3 cards in 13 seconds today (4.33s/card)` | same |
| 1 | 59000 | `Studied 1 card in 59 seconds today (59s/card)` | same |
| 1 | 59994 | `Studied 1 card in 59.99 seconds today (59.99s/card)` | same |
| 1 | 59995 | `Studied 1 card in 60 seconds today (60s/card)` | same |
| 1 | 59999 | `Studied 1 card in 60 seconds today (60s/card)` | same |
| 1 | 60000 | `Studied 1 card in 1 minute today (60s/card)` | same |
| 1 | 61000 | `Studied 1 card in 1.02 minutes today (61s/card)` | same |
| 2 | 90000 | `Studied 2 cards in 1.5 minutes today (45s/card)` | same |
| 1 | 90500 | `Studied 1 card in 1.51 minutes today (90.5s/card)` | same |
| 1 | 119999 | `Studied 1 card in 2 minutes today (120s/card)` | same |
| 1 | 3540000 | `Studied 1 card in 59 minutes today (3540s/card)` | `Studied 1 card in 59 minutes today (3,540s/card)` |
| 1 | 3600000 | `Studied 1 card in 60 minutes today (3600s/card)` | `Studied 1 card in 60 minutes today (3,600s/card)` |
| 300 | 5400000 | `Studied 300 cards in 90 minutes today (18s/card)` | same |
| 10 | 18000000 | `Studied 10 cards in 300 minutes today (1800s/card)` | `Studied 10 cards in 300 minutes today (1,800s/card)` |
| 1 | 900000 | `Studied 1 card in 15 minutes today (900s/card)` | same |
| 1 | 999 | `Studied 1 card in 1 second today (1s/card)` | same |
| 1 | 4 | `Studied 1 card in 0 seconds today (0s/card)` | same |
| 1 | 5 | `Studied 1 card in 0.01 seconds today (0.01s/card)` | same |
| 1 | 125 | `Studied 1 card in 0.13 seconds today (0.13s/card)` | same |
| 8 | 1000 | `Studied 8 cards in 1 second today (0.13s/card)` | same |
| 1 | 1005 | `Studied 1 card in 1.01 seconds today (1.01s/card)` | same |
| 1 | 8855 | `Studied 1 card in 8.85 seconds today (8.85s/card)` | `Studied 1 card in 8.86 seconds today (8.86s/card)` |
| 7 | 100000 | `Studied 7 cards in 1.67 minutes today (14.29s/card)` | same |
| 1000 | 1000000 | `Studied 1000 cards in 16.67 minutes today (1s/card)` | `Studied 1,000 cards in 16.67 minutes today (1s/card)` |
| 1234 | 1234500 | `Studied 1234 cards in 20.58 minutes today (1s/card)` | `Studied 1,234 cards in 20.58 minutes today (1s/card)` |
| 0 | 5000 | `Studied 0 cards in 5 seconds today (0s/card)` | (cannot occur) |
| 1 | -1 | `Studied 1 card in -0 seconds today (-0s/card)` | (cannot occur, T3.3) |
| 1 | -1000 | `Studied 1 card in -1 second today (-1s/card)` | (cannot occur) |
| 1 | -5000 | `Studied 1 card in -5 seconds today (-5s/card)` | (cannot occur) |
| 1 | -90000 | `Studied 1 card in -1.5 minutes today (-90s/card)` | (cannot occur) |
| 1 | 131072125 | `Studied 1 card in 2184.54 minutes today (131072.12s/card)` | `Studied 1 card in 2,184.54 minutes today (131,072.13s/card)` |
| 50 | 400000 | `Studied 50 cards in 6.67 minutes today (8s/card)` | same (AE53) |

## T3. The Today section: values

### T3.1 Rows

The Today section of a scope (T3.2) reads the rows with `id ≥ S`: unlike the Home line, a row stamped exactly `S` counts (R1369). There is no upper end: rows dated after now, or after `N`, count, as on the Home line.

Each row's kind is read from the low byte of `type`: `k = type mod 256` (the remainder from 0 to 255, so −1 gives 255 and 260 gives 4).

| `k` | Kind | Counted |
|---|---|---|
| 0 | Learn | yes |
| 1 | Review | yes |
| 2 | Relearn | yes |
| 3 | Filtered | yes |
| 4 | Manual | no |
| 5 | Rescheduled | no |
| 6 to 255 | Learn | yes |

For every `type` from 0 to 255 this agrees with the Home line's rule. A `type` outside that range can differ: 260 is left out here but counted on the Home line; −1 and 6 are counted by both, here as Learn (scenario `today/odd-values`).

The history control of the Stats screen (last 12 months, all history) never changes these rows (R1369).

### T3.2 Scope

A scope selects which counted rows belong to the section. A row's card is judged as it is when the section is computed: whether it exists and which deck it is in now, not at review time (R1282).

| Scope | Rows |
|---|---|
| Collection | every counted row, deleted cards' rows included |
| Deck `D` | the counted rows of existing cards whose `did` is `D` or a descendant of `D`, or whose `odid` is not 0 and is `D` or a descendant of `D` |
| Search `q` | the counted rows of the existing cards the search `q` matches (`specs/search/`), when the section is computed |

- The decks of the Deck scope are those of `stats/graphs-common` G2.1: `D` and the decks whose stored name starts with `D`'s stored name followed by the level separator, the names compared under simple case folding as `deck:` compares them (`search/names-and-ids` N3.3). So with decks `Lang` and `lang::Verbs` (a child whose parent part differs in case, written by another client), the scope of `Lang` holds the rows of `lang::Verbs`. The Deck scope covers the cards a `deck:"D"` search matches. A card in a filtered deck therefore counts in its home deck's scope and in the filtered deck's scope. This differs from deck pages' heatmaps, which use only the home deck (`stats/counting-rules` N4). No U97 scenario has a card in a filtered deck.
- A Deck id that names no deck (deleted by a sync while the screen shows it) gives the error `unknown_deck`, never zeros or the Collection values, as `stats/graphs-common` G8.2's scope error, which U225 shows as it shows that one.
- A search text that is empty, or holds only characters with the Unicode White_Space property, is the Collection scope: deleted cards' rows count. Any other text is a search, including `deck:*`, which matches every existing card and so leaves deleted cards' rows out (scenario `today/search-scopes`).
- Moving a card to another deck moves all its rows between Deck scopes at once (scenario `today/deck-subtree`). Deleting a card removes its rows from every Deck and Search scope, and leaves them in the Collection scope (AE53, scenario `today/ae53-deleted-card`).

### T3.3 Values

Over the rows of T3.1 in the scope:

| Value | Meaning |
|---|---|
| `answers` | the number of rows (R1278) |
| `time_ms` | the sum of the rows' time, modulo 2^32 (4,294,967,296): a row's `time` when it is an integer from 0 to 4,294,967,295, otherwise 0 |
| `correct` | rows with `ease` greater than 1 |
| `again` | `answers − correct`: rows rated Again, and rows with `ease` 0 (for example an unrated Filtered row) |
| `learn`, `review`, `relearn`, `filtered` | rows of each kind (T3.1 table); `filtered` counts the Filtered kind (R1371) |
| `mature` | rows whose last interval is 21 days or more: `lastIvl` read as an integer (a fractional value truncated toward zero, so 20.9 is 20 and 21.5 is 21), then compared with 21. Negative values (learning steps in seconds) are never mature. The row's kind does not matter. |
| `mature_correct` | `mature` rows with `ease` greater than 1 |

- The time rule differs from the Home line: there, negative times and times above 4,294,967,295 ms count as stored (scenario `today/odd-values`).
- Times are never recomputed or capped (R1279). The `time_ms` sum is kept in 32 unsigned bits, as Anki keeps it: a total above 4,294,967,295 ms wraps around. Decided 2026-10-27: Meguri reproduces the wrap, so the observed values (H7) and the first Today line equal Anki's; a wider sum would be an undeclared difference. Two rows with `time` 3,000,000,000 give `time_ms` 1,705,032,704 (6,000,000,000 − 4,294,967,296), so the first Today line is `Studied 2 cards in 28,417.21 minutes today (852,516.35s/card)`, while the Home line, which sums as stored (T2.1), is `Studied 2 cards in 100000 minutes today (3000000s/card)` (scenario `today/time-wrap`). The counts cannot wrap in practice (2^32 rows in one day).

### T3.4 Read only

Computing the section changes nothing in the collection (R1295).

## T4. The Today section: lines

### T4.1 No answers

When `answers` is 0 the section has one line:

```
No cards have been studied today.
```

This holds whatever other rows exist: a day with only Manual and Rescheduled rows has no answers (scenario `today/empty`).

### T4.2 Four lines

Otherwise the section has four lines, in this order. Numbers are written by T5.2 unless a line says otherwise.

1. **Studied.** The sentence of T2.2 with `answers` as the count and `time_ms` as the total time: the same unit rule (seconds below 60 s, minutes otherwise, never hours), the same wording and plural rule, but every number written by T5.2 (grouped digits, 64-bit arithmetic).
2. **Again.** `Again count: <A> (<P>%)`. `<A>` is `again` written as plain decimal digits, never grouped (`Again count: 1234 (100%)`). `<P>` is T5.3.
3. **Kinds.** `Learn: <learn>, Review: <review>, Relearn: <relearn>, Filtered: <filtered>`. Meguri omits `, Filtered: <filtered>` when `filtered` is 0 (R1372: Meguri has no filtered decks, so the term is noise for most users): `Learn: 3, Review: 4, Relearn: 2`. With `filtered` above 0 the line is Anki's, all four parts. This is a deliberate deviation from Anki, which always shows the fourth part. The core omits the part when it writes the line; the UI never edits, parses or hides parts of a line (T6.2). Decided 2026-10-27: this overrides the plan's U97 note that the UI hides the part when it is 0. No observation carries the text (T6.4), and a `conformance/deviations.yaml` entry needs a scenario, a pointer and a transform (`specs/conformance/dsl.md` D13), so the difference is registered in T11, as `sched/review-session` P11 registers its differences, and the unit test `core/crates/meguri-stats/tests/studied_today_text.rs` asserts it.
4. **Mature.** When `mature` is above 0: `Correct answers on mature cards: <mature_correct>/<mature> (<p>%)`, where `<p>` is `mature_correct / mature × 100` in 64-bit floats written by T5.2. When `mature` is 0: `No mature cards were studied today.`

### T4.3 Title

The section's title is `Today`. It is a label: the UI takes it from its String Catalog like any other heading. Only the lines are core text.

### T4.4 Examples

Each set is the scenario's values (the observed numbers come from the oracle; the lines follow from T4.2 and T5, with the R1372 form of the third line).

| Values | Lines |
|---|---|
| `today/lines-mix`: answers 11, time 77,000 ms, correct 7, learn 3, review 4, relearn 2, filtered 2, mature 5, mature correct 3 | `Studied 11 cards in 1.28 minutes today (7s/card)` / `Again count: 4 (36.36%)` / `Learn: 3, Review: 4, Relearn: 2, Filtered: 2` / `Correct answers on mature cards: 3/5 (60%)` |
| `today/no-mature`: 4, 26,000 ms, correct 2, learn 1, review 2, relearn 1, filtered 0, mature 0 | `Studied 4 cards in 26 seconds today (6.5s/card)` / `Again count: 2 (50%)` / `Learn: 1, Review: 2, Relearn: 1` / `No mature cards were studied today.` |
| `today/odd-values`: 7, 25,000 ms, correct 6, learn 2, review 4, relearn 1, filtered 0, mature 2, mature correct 1 | `Studied 7 cards in 25 seconds today (3.57s/card)` / `Again count: 1 (14.29%)` / `Learn: 2, Review: 4, Relearn: 1` / `Correct answers on mature cards: 1/2 (50%)` |
| `today/ae53-deleted-card`, Collection: 50, 400,000 ms, correct 50, review 50, others 0 | `Studied 50 cards in 6.67 minutes today (8s/card)` / `Again count: 0 (0%)` / `Learn: 0, Review: 50, Relearn: 0` / `No mature cards were studied today.` |
| `today/ae53-deleted-card`, Deck Lang after the deletion: 40, 320,000 ms, correct 40, review 40 | `Studied 40 cards in 5.33 minutes today (8s/card)` / `Again count: 0 (0%)` / `Learn: 0, Review: 40, Relearn: 0` / `No mature cards were studied today.` |
| every `today/day-start-*`: 4, 60,000 ms, correct 4, review 4 | `Studied 4 cards in 1 minute today (15s/card)` / `Again count: 0 (0%)` / `Learn: 0, Review: 4, Relearn: 0` / `No mature cards were studied today.` |
| `studied-today/refresh` after two answers: 2, 16,000 ms, correct 1, learn 2 | `Studied 2 cards in 16 seconds today (8s/card)` / `Again count: 1 (50%)` / `Learn: 2, Review: 0, Relearn: 0` / `No mature cards were studied today.` |
| answers 1,234, 1,234,500 ms, correct 1,000, learn 1,234 | `Studied 1,234 cards in 20.58 minutes today (1s/card)` / `Again count: 234 (18.96%)` / `Learn: 1,234, Review: 0, Relearn: 0` / `No mature cards were studied today.` |
| answers 1,500, 5,400,000 ms, correct 1,499, learn 200, review 1,000, relearn 100, filtered 200, mature 1,500, mature correct 1,234 | `Studied 1,500 cards in 90 minutes today (3.6s/card)` / `Again count: 1 (0.07%)` / `Learn: 200, Review: 1,000, Relearn: 100, Filtered: 200` / `Correct answers on mature cards: 1,234/1,500 (82.27%)` |

Single lines:

| Input | Line |
|---|---|
| again 1 of 3 | `Again count: 1 (33.33%)` |
| again 1 of 32 | `Again count: 1 (3.13%)` |
| again 1 of 8 | `Again count: 1 (12.5%)` |
| again 1 of 7 | `Again count: 1 (14.29%)` |
| again 29 of 200 | `Again count: 29 (14.5%)` |
| again 3 of 3 | `Again count: 3 (100%)` |
| mature 2 of 3 correct | `Correct answers on mature cards: 2/3 (66.67%)` |
| mature 31 of 32 correct | `Correct answers on mature cards: 31/32 (96.88%)` |
| mature 1 of 8 correct | `Correct answers on mature cards: 1/8 (12.5%)` |
| mature 0 of 4 correct | `Correct answers on mature cards: 0/4 (0%)` |
| learn 1,234, review 0, relearn 5, filtered 6 | `Learn: 1,234, Review: 0, Relearn: 5, Filtered: 6` |
| learn 1, review 2, relearn 3, filtered 0 | `Learn: 1, Review: 2, Relearn: 3` (Anki: `Learn: 1, Review: 2, Relearn: 3, Filtered: 0`) |

### T4.5 Where the Home line and the first Today line differ

For the same day in the Collection scope:

- the row stamped exactly `S` counts only in the Today section (T3.1; every `today/day-start-*` scenario: Home `Studied 3 cards in 56 seconds today (18.67s/card)`, Today `Studied 4 cards in 1 minute today (15s/card)`);
- a `type` outside 0 to 255 can count in one and not the other (T3.1), and a time outside 0 to 4,294,967,295 ms counts as stored on the Home line and as 0 in the Today section (T3.3); a Today total above 4,294,967,295 ms wraps around, the Home total does not (T3.3, `today/time-wrap`);
- numbers of 1,000 and more are grouped only in the Today section (`1234 cards`, `1,234 cards`);
- the two-decimal rounding can differ in the last digit, because the Home line rounds 32-bit values and the Today section rounds the shortest decimal of a 64-bit value: 1 answer of 8,855 ms is `(8.85s/card)` on the Home line and `(8.86s/card)` in the Today section. Such differences come from ordinary totals, not only from extreme ones (16,345 ms and 76,965 ms for 1 answer are two more).

## T5. Writing numbers

### T5.1 Home numbers

The Home line computes in IEEE 754 binary32 (32-bit) floats, as Anki's core does. Given `cards` and `time_ms`:

1. `s` = `time_ms / 1,000` computed in binary64, then rounded to the nearest binary32 value (ties to even).
2. The unit is minutes when `|s| ≥ 60`, otherwise seconds.
3. `amount` = `s / 60` in binary32 for minutes, `s` for seconds.
4. `per` = `s / c` in binary32, where `c` is `cards` rounded to the nearest binary32; `per` = 0 when `cards` is 0.
5. Each of `amount` and `per` is rounded to two decimals in binary32: multiply by 100 (a binary32 product), round that to an integer with halves away from zero, divide by 100 (a binary32 quotient).
6. Each result is written from its exact binary value, extended exactly to binary64: as a decimal with two fraction digits, rounded to nearest with ties to even; then trailing zeros after the point are removed, then a trailing point. A negative value keeps its sign, negative zero included (`-0`). No digit grouping.

`cards` is written as plain decimal digits.

These steps reproduce Anki's output on every input tried, including the ones where 32-bit rounding shows: 0.005 s per card is written `0.01` and 0.004 s is `0`; 1.005 s is `1.01`; 8.855 s is `8.85`; 59.995 s is `60` (with the unit still seconds, T2.4); 131,072.125 s per card is `131072.12` (ties to even in step 6). A check of 29,220 inputs (counts 0 to 1,000,000, totals from −200,000 ms to 10,000,000,000 ms) against the oracle found no difference.

### T5.2 Stats numbers

The Today section computes in IEEE 754 binary64 (64-bit) floats, as Anki's web page does: the total in seconds is `time_ms / 1,000`, the minutes `seconds / 60`, the seconds per card `seconds / answers`, the mature percentage `mature_correct / mature × 100` (divide first, then multiply).

A number is written in English (en-US):

1. Take the shortest decimal that reads back as the same binary64 value: the fewest significant digits, the nearest such decimal when several qualify (the form JavaScript's number-to-string conversion gives, `0.1` for the binary64 value nearest 0.1).
2. Round that decimal to two fraction digits, halves away from zero: `1.005` gives `1.01`, `3.125` gives `3.13`, `8.855` gives `8.86`.
3. Remove trailing zeros after the point, then a trailing point.
4. Group the integer part in threes from the right with `,`: `1,234`, `1,234,567.5`. Numbers below 1,000 have no separator.

Integers (`answers`, the counts of the kinds line, `mature_correct`, `mature`) are written by the same steps, so they are grouped too.

### T5.3 The again percentage

1. `q` = `again / answers × 100` in binary64 (divide first, then multiply).
2. `r` = the integer nearest to `q × 100` (a binary64 product), halves toward positive infinity.
3. `<P>` is `r / 100` (binary64) written by T5.2. It never needs grouping (at most 100) and never has more than two fraction digits.

Examples: 1 of 3 is `33.33`, 1 of 32 is `3.13`, 29 of 200 is `14.5`, 0 is `0`, all is `100`.

### T5.4 Singular and plural

- `card` is singular exactly when the count is 1. `0 cards`, `2 cards`, `1,000 cards`.
- `second` and `minute` are singular exactly when `<amount>` is written `1` (or `-1`, Home line only). The rounded text is what counts: 0.999 s is `in 1 second`, 1.004 s is `in 1 second`, 1.01 s is `in 1.01 seconds`, 0 is `in 0 seconds`, −0.001 s is `in -0 seconds`.

## T6. What the core returns

### T6.1 Home

The Home payload holds `cards`, `time_ms` and the line (T2). The UI shows the line verbatim (R1448, R1296–R1298): it formats no number and adds no punctuation.

While the resident values do not yet hold the state the read sees (a rebuild or catch-up, `stats/counting-rules` N7 "Reads while the values are not ready"), the result is `building` instead, with no numbers and no line. It is never the zero line. U228 hides the line until a later read returns the payload.

### T6.2 Today

For a scope (T3.2), the Today payload holds the values of T3.3 (with `again`) and the lines of T4: one line when `answers` is 0, otherwise four. `filtered` is returned even when the kinds line omits it (R1372). The UI shows the lines verbatim, one per row, under its own `Today` heading. A Deck id that names no deck gives `unknown_deck` instead (T3.2).

### T6.3 Text form

- Lines are always English, whatever the device language (R1448; the plan's Open Questions keep these strings verbatim in every locale).
- No line contains U+2068 or U+2069. Anki wraps each number in that pair of invisible direction marks; Meguri leaves them out.
- No line contains a line break. Anki's Studied sentence holds two line breaks, after the count and after `today`, which its pages show as spaces; Meguri writes single spaces there.

### T6.4 What the oracle checks

- The Home line is compared with Anki's text (R1448, `specs/conformance/stats-ops.md` H5 and H6).
- The Today section is compared through its values (H7). Anki writes its lines in its web page, which the oracle does not run, so the lines are tested against T4.4 and T5 by unit tests (`core/crates/meguri-stats/tests/studied_today_text.rs`). T5.2 and T5.3 were checked against the JavaScript number formatting of the en-US locale on 71,903 inputs with no difference.

### T6.5 The summary's active-day mark (U228)

The review summary's streak line (R95) shows an active-day mark when the session made today an active day. Decided 2026-10-27: this is a heatmap rule, never a studied-today one, since the two use different days and rows (`stats/day-buckets` B4.4; `stats/counting-rules` N3 against N6). The mark shows when, for the same `T`, the Home day → count map (`stats/counting-rules` N7, the counts the Home heatmap draws) had no count on `T` when read before the session's first answer, and has a count of at least 1 on `T` after the session. When `T` changed during the session, or either read returned `building`, there is no mark. Studied-today values are never used for it. This overrides the plan's U228 wording ("studied-today was zero before its first answer"). With `limdecks` [Lang] and a first session on Lang, studied today goes from 0 to 1 but `T` stays empty: no mark. At 04:30 CEST on 2026-03-29 in Europe/Paris, rollover 4, Anki's day 0 has already moved to 2026-03-29 but `T` is still 2026-03-28 (`stats/day-buckets` B6), and the answer is on heatmap day 2026-03-28 (B4.4): the mark follows the count of 2026-03-28, the date the Home heatmap shows as today, whatever studied today said before.

## T7. Freshness

Every read reflects the collection when it runs (R1300, data side; R304):

- answers, their undo and redo, incoming changes (CloudKit sync, a manual Anki transfer, an import that replaces the collection) change both texts on the next read;
- deleting a card leaves the Home line and the Collection scope unchanged and removes its rows from every Deck and Search scope;
- moving a card changes Deck and Search scopes;
- when `N` moves (rollover while the collection is open, a change of the `rollover` key, a zone change, `stats/day-buckets` B6), `S` moves with it and both texts cover exactly the new window (scenario `studied-today/refresh`: at 03:59:59 the two answers still count, at 04:00:00 the line is the zero line).

The Home line meets this through the resident values' contract (`stats/counting-rules` N7). When the UI asks again (return from a review session, undo, sync, rollover) belongs to the UI units (U103, U152, U228). Decided 2026-10-27: U228, which hosts the Home line and the Home heatmap, and U103's refresh events ask again on `RolledOver`, `NextDayAtChanged` and `TimeZoneChanged` (`sched/day-clock` D8), since the last two move `N` and `S` without a day change, on the aggregates' readiness events after a `building` result (T6.1), and at the `today_changes_at` instant the last heatmap query returned (`stats/counting-rules` N7, `stats/day-buckets` B6), since no event marks a change of the heatmap's today that comes after the move of `N`.

## T8. Strings

Meguri takes these English texts only from this table. `<…>` marks where a number goes.

| Where | Text |
|---|---|
| Home line and first Today line | `Studied <count> <span> today (<per-card>s/card)` |
| count | `1 card`, `<n> cards` |
| span, seconds | `in <n> second`, `in <n> seconds` |
| span, minutes | `in <n> minute`, `in <n> minutes` |
| Today, no answers | `No cards have been studied today.` |
| Today, again | `Again count: <A> (<P>%)` |
| Today, kinds | `Learn: <a>, Review: <b>, Relearn: <c>, Filtered: <d>` (Meguri: without `, Filtered: <d>` when `<d>` is 0) |
| Today, mature | `Correct answers on mature cards: <c>/<t> (<p>%)` |
| Today, no mature | `No mature cards were studied today.` |
| Today, heading (UI label, T4.3) | `Today` |

## T9. Scenarios

All rows are inserted with `db_exec` on a card id no card has, unless the scenario has notes. Each scenario gets a golden recorded from the oracle under `conformance/goldens/stats/studied-today/` and `conformance/goldens/stats/today/` when the scenarios land (`specs/conformance/stats-ops.md` H8; kinds H5 to H7). The goldens are then authoritative, and their values equal the tables below, which were checked against oracle recordings at the current pin.

`conformance/scenarios/stats/studied-today/`:

| Scenario | Pins | Values |
|---|---|---|
| `format-table` | T2.2, T5.1, T5.4 | the 36 lines of T2.4 |
| `zero-line` | T2.3, T4.1 | `Studied 0 cards in 0 seconds today (0s/card)`; Collection all 0 |
| `time-units` | T2.2 units, R1279 | `Studied 1 card in 59 seconds today (59s/card)`, `Studied 2 cards in 1 minute today (30s/card)`, `Studied 3 cards in 90 minutes today (1800s/card)`, `Studied 4 cards in 300 minutes today (4500s/card)` |
| `refresh` | T7 through answers and a rollover | Home: zero line, `Studied 1 card in 4 seconds today (4s/card)`, `Studied 2 cards in 16 seconds today (8s/card)` (twice), zero line, `Studied 1 card in 30 seconds today (30s/card)`; Collection answers 0, 1, 2, 2, 0, 1 (correct 0, 1, 1, 1, 0, 1; all Learn) |

`conformance/scenarios/stats/today/` (all Collection values are `answers`, `time_ms`):

| Scenario | Pins | Values |
|---|---|---|
| `ae53-deleted-card` (AE53) | T3.2 deletion | Home `Studied 50 cards in 6.67 minutes today (8s/card)` both times; Collection 50, 400,000 both times; Lang 50, 400,000, then 40, 320,000 |
| `search-scopes` | T3.2 search texts | Collection, `""` and `" \t "`: 4, 11,000; `deck:*`: 3, 7,000; `deck:Lang`: 2, 4,000; `-deck:Lang`: 1, 3,000 |
| `deck-subtree` | T3.2 descendants, card moves | Lang / Lang::Verbs / Other / Collection: 5 / 3 / 2 / 8, then 6 / 3 / 1 / 8 |
| `lines-mix` | T3.3, T4.2 | Home `Studied 11 cards in 1.28 minutes today (7s/card)`; T4.4 row 1 |
| `no-mature` | T4.2 line 4 | Home `Studied 4 cards in 26 seconds today (6.5s/card)`; T4.4 row 2 |
| `empty` | T4.1 with Manual and Rescheduled rows | Home zero line; Collection all 0 |
| `odd-values` | T3.1 low byte, T3.3 time and `lastIvl` | Home `Studied 8 cards in 83333.71 minutes today (625002.75s/card)` (8 rows, 5,000,022,000 ms); T4.4 row 3 |
| `time-wrap` | T3.3 32-bit total | two Review rows with `ease` 3 and `time` 3,000,000,000: Home `Studied 2 cards in 100000 minutes today (3000000s/card)` (2 rows, 6,000,000,000 ms); Collection 2, 1,705,032,704. Its values follow from T3.3 and T5.1; unlike the other rows, they were not yet checked against an oracle recording. |
| `day-start-utc`, `-kathmandu`, `-st-johns`, `-chatham`, `-paris-dst`, `-los-angeles-dst` | T2.1 against T3.1 at `S`, across zones | Home `Studied 3 cards in 56 seconds today (18.67s/card)` (3 rows, 56,000 ms); Collection 4, 60,000 |

`search-scopes` needs the `deck:` search term (U82, tag `needs-deck`). `refresh`, `ae53-deleted-card`, `search-scopes` and `deck-subtree` need the setup sections `config`, `decks`, `notetypes` and `notes`; `refresh` also needs `answer`, `ae53-deleted-card` `delete_notes`.

## T10. Not covered

- Today counts above 4,294,967,295 rows. (A Today total above 4,294,967,295 ms is covered: T3.3 and `today/time-wrap`. Two rows another client wrote are enough to reach it, since one row's `time` may be up to that value.)
- `type`, `ease` or `time` stored as REAL, TEXT or BLOB, and `ease` outside 0 to 255.
- A card in a filtered deck in a Deck scope (T3.2 states the rule; U98's scenarios cover it).
- Search texts that fail to parse: the Stats screen's handling is U98's (R1351).

## T11. Differences from Anki (R1889)

These differences are deliberate. They change no golden: an observation either does not carry the text or normalizes it, so they have no `conformance/deviations.yaml` entry and allow no transform (`specs/conformance/dsl.md` D13). Each is asserted by the unit test named.

| Situation | Anki 26.09.3 | Meguri | Asserted by |
|---|---|---|---|
| Kinds line with `filtered` 0 (R1372, T4.2 item 3) | `Learn: 1, Review: 2, Relearn: 3, Filtered: 0` | `Learn: 1, Review: 2, Relearn: 3` | `core/crates/meguri-stats/tests/studied_today_text.rs` |
| Direction marks around numbers (T6.3) | each number between U+2068 and U+2069 | none (H5 and H6 remove them from the oracle's text) | the same test |
| Line breaks in the Studied sentence (T6.3) | a line break after the count and after `today` | single spaces (H5 and H6 replace them) | the same test |

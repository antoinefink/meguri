# Search: card properties and custom data

Spec: search/properties
Requirements: R1065, R1066, R1067, R1068, R1070, R1071, R1268
Anki: 26.09.3
Scenarios: conformance/scenarios/search/props/

What `prop:` and `has-cd:` match, and the custom-data SQL function. The functions that read the memory state, retrievability and original position from `cards.data` are `search/card-extractors` (X1–X8), their only definition, which U41's queue order and U84's sorting reuse too; this file only says how the terms call them. `prop:rated` and `prop:resched` are `history.md` H8. The parsing of the value (property, operator, operand and their errors) is `grammar.md` G8.1; the scope of every term here is card (`query-model.md` Q3). Every rule here was checked against the oracle; the scenarios pin the examples.

## P1. Operators

`<`, `<=`, `>`, `>=`, `=` and `!=` are the numeric (or, for `cds:`, text) comparisons of the stored value V with the operand: V < operand, and so on. A V that is unknown (NULL) makes the term unknown, and NOT unknown is unknown (`query-model.md` Q7): such a card is found neither by the term nor by its negation.

## P2. `ivl`, `reps`, `lapses` (R1066)

The columns `cards.ivl`, `cards.reps` and `cards.lapses` compared with the operand (unsigned 32-bit, G8.1). No card type is excluded: `prop:ivl=0` finds new and learning cards, whose `ivl` is 0.

## P3. `ease` (R1066)

- The operand x is a 32-bit float (G8.1). E = x × 1000 computed in 32-bit floats, then converted to an unsigned 32-bit integer by truncation toward zero, with NaN and negative values giving 0 and values of 4294967296 or more giving 4294967295.
- The card matches when `cards.factor` compared with E holds. `prop:ease=2.5` is `factor = 2500`, `prop:ease=1.3` is `factor = 1300`, `prop:ease>=1.2999` is `factor >= 1299`.
- No card type is excluded: new and learning cards have factor 0, so `prop:ease<1` finds them and `prop:ease!=2.5` includes them.
- `prop:ease>inf` is `factor > 4294967295` (nothing), `prop:ease>nan` is `factor > 0`, `prop:ease<-1` is `factor < 0` (nothing). These never fail.

## P4. `due` (R1067)

Let D be the card's `odue` when it is not 0, otherwise its `due` (so a card in a filtered deck is compared on its home due). With the operand N (signed 32-bit, G8.1), today and next-day-at from the search context (`query-model.md` Q8):

| Card queue | The card matches when |
|---|---|
| 2 (review) or 3 (interday learning) | D compared with T holds, where T = N + today computed in signed 32-bit arithmetic that wraps on overflow |
| 1 (learning) or 4 (preview) | Q compared with N holds, where Q = (D − next-day-at) ÷ 86400 in 64-bit integers, the quotient truncated toward zero |
| any other | never |

- New, suspended and buried cards never match, whatever their due.
- Truncation toward zero puts every learning due less than one day either side of next-day-at at 0: one second before the boundary of day 10, a learning card due 10 minutes later and one due 18 hours earlier are both `prop:due=0`; a learning card due 86400 or 86401 seconds before next-day-at is −1.
- The wrap is Anki's: with today 11, `prop:due<2147483647` compares review cards with −2147483638 and finds none of them, while learning cards are compared with 2147483647 and all match. Scenarios pin it (`search/props/scheduling`).

| Card (day 10, 1 s before the boundary) | `prop:due` value |
|---|---|
| review or interday learning, due day 9 | −1 |
| review, due 10 / 11 | 0 / 1 |
| learning, due next-day-at − 86401 s or − 86400 s | −1 |
| learning or preview, due now + 600 s | 0 |
| filtered review, `due` −100000, `odue` day 9 | −1 |
| filtered learning, `odue` next-day-at − 86401 s | −1 |

### P4.1 The due-day rule outside search

This rule (the D and Q of P4) is the one `prop:due` uses. Anki's browser sorts the Due column and writes its cell by other rules (the sort ignores `odue`, for example); those belong to `specs/browser/` (U84), which reuses this rule only where its own spec says so.

## P5. `pos` (R1068)

The card matches when its type is 0 (new) and D (as in P4: `odue` when not 0, else `due`) compared with the operand holds. The test is on the type, not the queue, so suspended and buried new cards match; a new card in a filtered deck is compared on its original position. Cards of other types never match, whatever their `data` holds. Scenario: `search/props/scheduling`.

## P6. Operand literals

`s`, `d`, `r` and `cdn:` compare a stored 64-bit value with a 32-bit float operand turned into decimal text, not with the float itself:

1. The operand y (for `d`, the mapped value of P7) is a 32-bit float. If it is infinite or NaN, the search fails with `search_failed` (P14).
2. L(y) is the shortest decimal that reads back as y (the closest one when several have that length), written in positional notation without an exponent: the integer digits (`0` when there are none), then, when y is not an integer, `.` and the fraction digits; a minus sign for negative values, `-0` for negative zero. This is Rust's `Display` of an `f32`: `21`, `2.5`, `0.1`, `0.0000001`, `1000000000000000000000000000000` (1e30), `-3`.
3. L(y) is written into the SQL statement as a numeric literal, and SQLite reads it: an integer when it has no `.` and fits in signed 64 bits, a 64-bit real otherwise. The comparison is SQLite's numeric comparison of the stored value with that number.

So the operand is the 64-bit number nearest the decimal text, not the 32-bit float widened: a stored stability 2.3 (the 32-bit 2.2999999523…) is below `2.3`, so `prop:s=2.3` misses it and `prop:s<2.3` finds it, and a stored 0.1 satisfies `prop:s>0.1`. Meguri writes L(y) into the statement exactly so (`query-model.md` Q9 allows numbers in the statement); binding the widened float or a separately parsed double could differ.

## P7. `s` and `d` (R1070)

- V is the stability (`s`) or difficulty (`d`) read from `cards.data` (`search/card-extractors` X3, X4), widened exactly from 32 to 64 bits; unknown when the key is absent.
- `prop:s` uses the operand as is (stability in days).
- `prop:d` takes the difficulty on a 0–1 scale and maps it to the stored 1–10 scale: y = x × 9 + 1, computed in 32-bit floats (the product rounded to 32 bits, then the sum). `prop:d>0.3` compares with `3.7`, so a stored 3.7 (3.7000000476…) matches it; `prop:d=0.5` compares with `5.5`. A finite operand whose mapping overflows (`prop:d>1e38`) is infinite and fails (P6).
- Each key is read on its own: `{"s":5}` has a stability for `prop:s` although it has no memory state (`search/card-extractors` X3). New cards are not excluded: a new card whose data holds `s` and `d` matches `prop:s` and `prop:d`.

| Stored `data` | `prop:s>1` | `prop:s=2.3` | `prop:s<2.3` | `prop:d>0.3` |
|---|---|---|---|---|
| `{"s":2.3,"d":3.7}` | yes | no | yes | yes |
| `{"s":0.1,"d":2}` | no | no | yes | no |
| `{"s":5}` | yes | no | no | unknown |
| `{"s":1e40,"d":1}` (s reads as +∞) | yes | no | no | no |
| `{}`, `garbage`, `{"s":5,"d":5,"cd":5}` | unknown | unknown | unknown | unknown |

## P8. `r` (R1070)

- A new card (type 0) never matches, and the term is false for it, not unknown: `-prop:r<0.9` finds every new card.
- For any other card, V is the retrievability R of `search/card-extractors` X5 at the search context's now and today, unknown when the card has no memory state (both `s` and `d`, X3). So a review card with `{}`, `{"s":5}` or unreadable data is found neither by `prop:r<0.9` nor by `-prop:r<0.9`.
- V is compared with L(operand) as P6 says.

Examples at day 30 (`search/props/memory`): `s` 2.5 with `lrt` 10 days ago gives R ≈ 0.7183; `s` 21 one day ago ≈ 0.9945; `s` 100, decay 0.1542, 50 days ago ≈ 0.9403; `s` 5 with no `lrt`, `due` 35, `ivl` 20 ≈ 0.7661 (0.7558 a day later); a relearning card due 10 minutes from now, with no `lrt` and `ivl` 0, is exactly 1; a card whose `lrt` lies in the future is exactly 1.

## P9. Custom data: `cdn:`, `cds:`, `has-cd:` (R1071)

The custom data value C(key) of a card is P12's. Keys of `cdn:` and `cds:` are ASCII letters and digits only (G8.1: `prop:cdn:k-x>1` and `prop:cdn:k_1=1` are `invalid_operator` with `cdn:k`); the key of `has-cd:` is any text (kind U). Keys are case-sensitive.

- **`has-cd:key`**: the card matches when C(key) exists (any value, JSON `null` and the empty string included). It is never unknown, so `-has-cd:key` finds every card without the key.
- **`prop:cds:key<op>text`**: C(key) compared with the operand text (G8.1, kept as written, possibly empty) as text: SQLite's binary collation, which orders by the UTF-8 bytes. Unknown when the key is missing. `prop:cds:k=true` finds the JSON value `true`, `prop:cds:k=null` the JSON value `null`, `prop:cds:k=` the empty string, `"prop:cds:k= 7"` the string ` 7`.
- **`prop:cdn:key<op>number`**: C(key) converted to a 64-bit real the way SQLite's `CAST(... AS REAL)` converts text: leading spaces skipped, then the longest prefix that reads as a decimal number, 0.0 when there is none. Then compared with L(operand) (P6). Unknown when the key is missing.

| Custom data (the `cd` member's JSON) | C(`k`) | `prop:cdn:k>=5` | `prop:cdn:k=0` | `has-cd:k` |
|---|---|---|---|---|
| `{"k":"7","v":"abc"}` | `7` | yes | no | yes |
| `{"k":5}` | `5` | yes | no | yes |
| `{"k":1.5}` | `1.5` | no | no | yes |
| `{"k":"5x"}` | `5x` | yes (5) | no | yes |
| `{"k":" 7"}` | ` 7` | yes (7) | no | yes |
| `{"k":"abc"}`, `{"k":true}`, `{"k":null}`, `{"k":""}` | `abc`, `true`, `null`, empty | no | yes (0) | yes |
| `{"k":"1","k":"9"}` | `9` (the last one) | yes | no | yes |
| `{"K":1}`, `{"other":1}`, `[1,2]`, no `cd` | missing | unknown | unknown | no |

Scenario: `search/props/custom-data`.

## P10. Reading `cards.data`

Removed (2026-10-27): moved to `search/card-extractors` X3 (U82a), with its wording. The functions of this file read `cards.data` as X3 says.

## P11. Retrievability

Removed (2026-10-27): moved to `search/card-extractors` X5 (U82a). `prop:r` (P8) uses that R.

### P11.1 Relative retrievability (for U41)

Removed (2026-10-27): moved to `search/card-extractors` X6 (U82a). No search term uses it.

## P12. Custom data value

C(data, key), for the custom data of P9:

1. `cd` as `search/card-extractors` X3 reads it. When it is absent, C is missing.
2. The `cd` text is parsed as JSON (RFC 8259). When it does not parse or is not an object, C is missing.
3. The member named key (the last one when the name repeats). When there is none, C is missing.
4. A string member gives its content; any other value gives its compact JSON text as `serde_json` writes a value: integers in decimal, other numbers as `serde_json` writes an `f64` (`1.5`, `5.0`), `true`, `false`, `null`, arrays and objects without spaces.

## P13. Original position (for U41 and U84)

Removed (2026-10-27): moved to `search/card-extractors` X7 (U82a). No search term uses it: `prop:pos` reads the card's due (P5).

## P14. Errors

- `search_failed` when an operand of `s`, `r` or `cdn:`, or the mapped operand of `d`, is infinite or NaN (P6): `prop:s>inf`, `prop:s>nan`, `prop:r<-inf`, `prop:cdn:k>inf`, `prop:d>1e38`. Anki writes such an operand into its statement and the statement fails; Meguri raises the error while it builds the plan, before running anything. The range is the whole term (`errors.md` E5).
- `prop:ease` never fails (P3).
- Message: `errors.md` E7, `search_failed (number out of range)`.

## P15. SQL functions (Meguri design)

Not Anki behavior. `meguri_card_f32`, `meguri_retrievability`, `meguri_relative_retrievability` and `meguri_original_position` moved to `search/card-extractors` X2 (U82a), which also gives their registration (X2.1) and argument rules (X2.2). This section keeps the custom-data function and how the terms of this file call the functions.

| Function | Result |
|---|---|
| `meguri_custom_data(data, key)` | C(data, key) of P12 as TEXT, or NULL when missing |

- It is registered with the others (`search/card-extractors` X2.1), deterministic and innocuous, with two arguments. Decided 2026-10-27 (data safety first, as X2.2 does for the other four): it never raises an SQL error or panics: a `data` that is not TEXT, or is empty, reads as no keys and gives NULL, and so does a `key` that is not TEXT.
- Terms call the functions so, with D the card's `odue` when not 0, else its `due` (X1): `prop:s` and `prop:d` compare `meguri_card_f32(c.data, 's')` or `meguri_card_f32(c.data, 'd')` with L(y); `prop:r` is false for type 0 and otherwise compares `meguri_retrievability(c.data, D, c.ivl, today, now)` with L(y); `prop:cdn` compares `CAST(meguri_custom_data(c.data, ?) AS REAL)` with L(y); `prop:cds` compares `meguri_custom_data(c.data, ?)` with a bound text; `has-cd` tests `meguri_custom_data(c.data, ?) IS NOT NULL`. Keys and texts are bound parameters; numbers are written into the statement (P6).

## P16. Plan

- Context reads (`query-model.md` Q8): `prop:due` reads today and next-day-at; `prop:r` reads today and now. The other properties read none.
- Slow terms (`query-model.md` Q9): `prop:s`, `prop:d`, `prop:r`, `prop:cdn`, `prop:cds` and `has-cd` call a function of P15 or `search/card-extractors` X2 per row. `ivl`, `reps`, `lapses`, `ease`, `due` and `pos` compare columns and are not slow.
- Errors raised while building the plan: P14.

## P17. Scenarios

| Scenario | Sections |
|---|---|
| `search/props/scheduling` | P2, P3, P4, P5 |
| `search/props/memory` | P6, P7, P8, P14; `search/card-extractors` X3, X5 |
| `search/props/custom-data` | P9, P12, P14 |
| `search/history/dst-paris`, `search/history/dst-new-york` | P4 across a change of UTC offset |

All carry the tag `needs-prop`. They set card columns and `cards.data` with `db_exec`; card ids come from the frozen clock (`storage/ids-usn-mtime#I2`) and are pinned by an `ids` observation.

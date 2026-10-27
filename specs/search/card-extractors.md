# Search: card-data SQL functions

Spec: search/card-extractors
Requirements: R617, R618, R1068, R1070, R1130, R2016
Anki: 26.09.3
Scenarios: conformance/scenarios/search/extractors/

The four SQL scalar functions that read a card's `cards.data` for queries: a memory-state field, retrievability, relative retrievability and original position. This file is their only definition. U41's review orders (difficulty, retrievability, relative overdueness), U82's `prop:s`, `prop:d` and `prop:r` (`search/properties`) and U84's sorts and cells call them and do not restate them. No search term calls the original-position function (X7): `prop:pos` reads the card's due, `odue` when it is not 0 and `due` otherwise (`search/properties` P5, P13). The custom-data function stays with U82 (`search/properties`).

X3, X5, X6, X7 and the four rows of X2 were moved from `search/properties` P10, P11, P11.1, P13 and P15 when U82a was split from U82; their wording is kept, with section references renamed. Everything else here was added from oracle observations (scenarios in X9).

Notation: w(x) is the integer x reduced to unsigned 32 bits (two's complement wrap: x mod 2^32); a ⊖ b is a − b when a ≥ b and 0 otherwise. "f32" is an IEEE-754 binary32 value and its arithmetic, each step rounded to f32.

## X1. Callers and their arguments

- Every caller passes, for a card:
  - `data`: the card's `data` column;
  - `due`: D, the card's `odue` when it is not 0 and its `due` otherwise, so a card in a filtered deck is evaluated on its home due;
  - `ivl`: the card's `ivl` column;
  - `today`: the scheduler's day number (days elapsed, `sched/day-clock`), and `now`: the instant in epoch seconds, both from one reading of the day clock for the whole operation: the search context for searches (`search/query-model` Q8), the queue build's values for U41.
- No function reads a clock, the collection's config or any other row, so frozen clocks and the callers' day values reach them unchanged.
- Anki's functions of this kind also receive the next day boundary and never read it. Meguri's functions do not take it.

## X2. Functions (Meguri design)

Not Anki behavior: the functions through which Meguri's statements read `cards.data`.

| Function | Result |
|---|---|
| `meguri_card_f32(data, key)` | for `key` `s`, `d`, `dr` or `decay`: the value X3 reads, as a 64-bit real (widened from 32 bits), or NULL when absent (X4); any other key is a function error |
| `meguri_retrievability(data, due, ivl, today, now)` | R of X5 as a 64-bit real, or NULL; `due` is the caller's D |
| `meguri_relative_retrievability(data, due, ivl, today, now)` | RR of X6 |
| `meguri_original_position(data)` | P of X7 as an INTEGER, or NULL |

### X2.1 Registration

- `core/crates/meguri-search/src/sqlfns.rs` defines them, and its `register(conn)` adds them to a connection (U81 and U82 add their own functions to the same module). The engine runs `register` through the connection initializers of `storage/open-and-connections` C5 on the writer and on every reader, so every statement on any of them can call the four functions.
- Each is registered deterministic and innocuous, with the argument count of the table. A call with another argument count fails when SQLite prepares the statement.
- No index, view, trigger, generated column or CHECK constraint of the collection file may use them (R2016): Anki and other clients open the file without them. After an open, no `sql` text in `sqlite_master` names any of them.

### X2.2 Arguments and results

- No function raises an SQL error or panics for any argument of any storage class, with one exception: a `key` of `meguri_card_f32` that is not one of the TEXT values `s`, `d`, `dr`, `decay` (NULL included) is a function error. Callers write the key as a literal, so that error means a programming mistake.
- `data`: TEXT is read by X3. INTEGER, REAL, BLOB and NULL values, and TEXT whose bytes are not valid UTF-8, read as no keys.
- `due`, `ivl`, `today`, `now` are used only when they are INTEGER values. A REAL is not one, even `35.0`, and neither is the TEXT `'35'`: nothing is converted. A function that needs an argument that is not an INTEGER gives NULL:

  | Function | Needs as INTEGER |
  |---|---|
  | retrievability | `due` and `now` always; `ivl` when the data has no `lrt`; `today` when the data has no `lrt` and D ≤ 365000 |
  | relative retrievability | `due`, `ivl` and `now` always; `today` when D ≤ 365000 |

  So retrievability with an `lrt` ignores `ivl` and `today` entirely, while relative retrievability needs `today` for a day-number due even when the data holds an `lrt`.
- A result that is NaN is NULL (SQLite stores no NaN). An infinite result is returned as a REAL infinity.
- The values are computed in f32 and widened exactly; callers compare the 64-bit value (`search/properties` P6).

## X3. Reading `cards.data`

The functions of X2 read `cards.data` this way:

- A value that is not TEXT, or the empty text, has no keys.
- Otherwise the text is read by `model/card-data-json` C3, rules 1–3, 5 and 6 (`pos`, `s`, `d`, `dr`, `decay`, `lrt`, `cd`), with two differences:
  - rule 4 is not applied: `s` and `d` are each present or absent on their own. The memory state is present only when both are;
  - a `cd` member whose value is not a JSON string makes the whole text read as no keys: `{"s":5,"d":5,"cd":5}` has no stability (oracle, `search/extractors/card-data`). C3 rule 3 reads only `cd` as absent there. Anki reads a card row's `data` the same way as these functions, so C3 needs this rule too (an open point for `model/card-data-json`, to be confirmed there by a card-data scenario). Until C3 changes, Meguri's card reader follows C3 and these functions follow this section.
- Unknown keys are ignored.

Added from the oracle (`search/extractors/card-data`):

1. TEXT whose bytes are not valid UTF-8 has no keys, and so does a text that starts with a byte order mark. White space (space, tab, line feed, carriage return) is allowed before, between and after the JSON tokens.
2. "Not a JSON string" for `cd` includes `null`, numbers, booleans, objects and arrays, and, in the array form, a seventh element `null`: `[1,2,3,4,5,6,null]` has no keys, while `[null,2,3]` has `s` 2 and `d` 3. A `cd` string with an unpaired surrogate escape (`"\ud800"`) is not a valid string either: no keys.
3. Nesting. Text whose arrays and objects nest more than 127 levels deep, counting the outermost object or array, has no keys, unless the deeper levels lie inside the value of an unknown key. Such a value is skipped at any depth (130 levels were read). Meguri reads any depth without exhausting its stack.
4. The value of an unknown key is skipped but must still be valid JSON: a raw control character in one of its strings makes the whole text invalid (no keys), while an unpaired surrogate escape or a number beyond the 64-bit float range inside it does not.
5. `-0` is not an integer: `{"pos":-0}` has no `pos`, `{"lrt":-0}` no `lrt`. For `s`, `d`, `dr` and `decay` it reads as −0.0.
6. Key names are compared after their JSON escapes are decoded (`{"\u0070os":7}` has `pos` 7), exactly and case-sensitively (`S`, `D` and `POS` are unknown keys).

| `data` | `s` | `d` | `dr` | `pos` |
|---|---|---|---|---|
| `{"s":2.3,"d":3.7,"dr":0.85,"decay":0.3,"lrt":1896429600,"pos":4}` | 2.3 | 3.7 | 0.85 | 4 |
| `{}`, empty text, `garbage`, `1`, `"x"`, `null`, `[]` | NULL | NULL | NULL | NULL |
| `{"s":5}` | 5 | NULL | NULL | NULL |
| `{"s":null,"d":5}`, `{"s":"5","d":5}` | NULL | 5 | NULL | NULL |
| `{"pos":-1,"s":5,"d":5}`, `{"pos":"7","s":5,"d":5}`, `{"pos":null,"s":5,"d":5}` | 5 | 5 | NULL | NULL |
| `{"cd":"x","s":5,"d":5,"pos":3}` | 5 | 5 | NULL | 3 |
| `{"s":5,"d":5,"cd":5}`, `… "cd":null}`, `{"cd":{},…}`, `{"cd":true,…}`, `… "cd":"\ud800"}` | NULL | NULL | NULL | NULL |
| `{"pos":7,"s":1e400}`, `{"s":1,"s":2,"d":3}`, `{"pos":7,"pos":8}`, `{"pos":7,"s":5,"d":5,}` | NULL | NULL | NULL | NULL |
| `{"s":5,"d":5,"lrt":1896429600,"zz":1e400}`, `{"s":5,"d":5,"zz":{"a":[1e400]}}`, `{"zz":1,"zz":2,"s":5,"d":5}` | 5 | 5 | NULL | NULL |
| `{"s":5,"d":5,"zz":"a<U+0001>b"}` (a raw control character) | NULL | NULL | NULL | NULL |
| `[1,2,3]` | 2 | 3 | NULL | 1 |
| `[1,2,3,4,5,1896429600,"{}"]` | 2 | 3 | 4 | 1 |
| `[1,2,3,4,5,6,"x",8]` | NULL | NULL | NULL | NULL |
| `{"s":5,"d":5,"lrt":` 126 nested arrays `}` / 127 nested arrays | 5 / NULL | 5 / NULL | NULL | NULL |

## X4. Memory-state fields

- `meguri_card_f32(data, key)` gives the key's value as X3 reads it: an f32 (`model/json-numbers` N2) widened exactly to 64 bits, or NULL when the key is absent. Each key is read on its own: `{"s":5}` gives a stability of 5 and no difficulty, although the card has no memory state (X5, X6 need both).
- The f32 reading shows in the widened value: `0.1` gives 0.10000000149011612, `2.3` gives 2.299999952316284, `16777217` gives 16777216, `123456789012345678901234567890` gives about 1.2345679e29, `1e40` gives +∞, `-1e40` gives −∞, `1e-50` gives 0, `-0` gives −0.
- `decay` is read like the other three. The oracle has no way to observe it through this function (Anki's own extractor takes only `s`, `d` and `dr`); X5's use of it is observed.

## X5. Retrievability

R(data, D, ivl, today, now), where D is the `due` argument (X1), `ivl` the card's interval and today and now from the caller.

1. No memory state (X3): R is unknown (NULL).
2. Elapsed seconds e, an unsigned 32-bit value:
   - `lrt` present: e = w(now) ⊖ w(lrt);
   - otherwise, D > 365000 (a learning due in epoch seconds): e = w(now) ⊖ (w(D) ⊖ w(ivl));
   - otherwise (a day number): e = (w(today) ⊖ (w(D) ⊖ w(ivl))) × 86400, the product wrapping in 32 bits.
3. t = e converted to a 32-bit float, divided by 86400 in 32-bit floats.
4. decay = the card's `decay`, or 0.5 when it is absent.
5. R = `fsrs::current_retrievability(MemoryState { stability: s, difficulty: d }, t, decay)` of the pinned `fsrs` crate =6.6.2, which computes, in 32-bit floats, factor = 0.9^(1 / −decay) − 1 and R = (t / s × factor + 1)^(−decay). Meguri calls that function; it does not reimplement it.
6. The result is widened exactly to 64 bits.

A stability of +∞ gives R = 1; an `lrt` after now gives e = 0 and R = 1.

Added from the oracle (`search/extractors/retrievability`, `search/extractors/arguments`):

- D > 365000 compares the 64-bit argument as a signed integer: a negative due is a day number.
- In the seconds branch `ivl` is subtracted as a number of seconds, whatever it holds for the card (a relearning card's `ivl` is its review interval in days).
- `lrt`, `now`, D, `ivl` and `today` all wrap to 32 bits before any subtraction: an `lrt` of −5 or of 2^32 − 1 lies after any 2030 instant (R = 1), an `lrt` of 2^32 + x counts as x, an `lrt` of −2^63 as 0, and a `now` of −1 as 2^32 − 1.
- An `lrt` that is not an integer in the 64-bit range (`1.5`, `1896429600.0`, `1.8964296e9`, `"1896429600"`, `null`, `-0`) is absent, so step 2 falls back to D and `ivl`. A `decay` that is `null` or a string is absent (0.5).
- The formula is applied as written for every stability and decay: a stability of 0 gives R = 0 when e > 0 and NaN (NULL) when e = 0; a negative stability gives NaN (NULL) when the base is negative; a decay of 0 gives R = 1, a negative decay gives values below 1 (−0.5: 0.7874007 after 10 days at stability 5), a decay of +∞ gives 1 and a subnormal decay (1e-40) gives 0.

Examples, stability 5 and difficulty 5 unless the data says otherwise, now 1897293600 (2030-02-14 10:00:00 UTC) and today 30 unless the row says otherwise. Values are the f32 results, shortest decimal:

| `data` | D | `ivl` | today / now | e | R |
|---|---|---|---|---|---|
| `{"s":2.5,"d":5.5,"lrt":1896429600}` | 35 | 10 | | 864000 | 0.7182781 |
| the same, now 1897293599 | 35 | 10 | | 863999 | 0.71827835 |
| `{"s":100,"d":10,"decay":0.1542,"lrt":1892973600}` | 40 | 10 | | 50 days | 0.9403443 |
| `{"s":0.01,"d":5,"lrt":1897293599}` | 35 | 10 | | 1 | 0.9998643 |
| `{"s":5,"d":5}` (no `lrt`) | 35 | 20 | | 15 days | 0.7661308 |
| `{"s":5,"d":5,"decay":0.1542}` | 35 | 20 | | 15 days | 0.80938804 |
| `{"s":5,"d":5,"decay":0.1542,"dr":0.9}` | 35 | 10 | | 5 days | 0.9 |
| `{"s":5,"d":5}` | 35 | 40 | | 30 days (35 ⊖ 40 = 0) | 0.6445033 |
| `{"s":5,"d":5}` | 35 | −5 | | 30 days (w(−5) > 35) | 0.6445033 |
| `{"s":5,"d":5}` | 35 | 20 | today −1 | 4293584896 (wrapped product) | 0.02070639 |
| `{"s":100,"d":5}` | 0 | 0 | today 60000 | 889032704 (wrapped product) | 0.19945656 |
| `{"s":5,"d":5}` | 365000 | 0 | | 0 (day number) | 1 |
| `{"s":5,"d":5}` | 365001 | 0 | | 1896928599 (seconds) | 0.031143775 |
| `{"s":4,"d":6}` | now − 3600 | 0 | | 3600 | 0.9987805 |
| `{"s":4,"d":6}` | now − 3600 | 2 | | 3602 | 0.9987799 |
| `{"s":5,"d":5,"dr":0.9}` | now − 3600 | 86400 | | 90000 | 0.9764264 |
| `{"s":5,"d":5,"dr":0.9}` | 6192257296 (2^32 + now − 3600) | 0 | | 3600 | 0.9990241 |
| `{"s":5,"d":5,"lrt":0}` | 35 | 10 | | now | 0.031140784 |
| `{"s":5,"d":5,"lrt":1896429600}` | 35 | 10 | now −1 | 2398537695 | 0.02769922 |
| `{"s":0,"d":5,"lrt":1897293600}` / `…"lrt":1897207200}` | 35 | 10 | | 0 / 86400 | NULL / 0 |
| `{"s":-1,"d":5,"lrt":1896429600}` | 35 | 10 | | 864000 | NULL |
| `{"s":5,"d":5,"decay":2,"lrt":1896429600}` | 35 | 10 | | 864000 | 0.81428313 |
| `{}`, `{"d":5,"dr":0.9,"lrt":1896429600}`, `{"decay":0.3}` | 35 | 20 | | | NULL |

## X6. Relative retrievability

U41's review order by relative overdueness under FSRS uses RR(data, D, ivl, today, now):

1. e0, an unsigned 32-bit value: when D > 365000, w(now) ⊖ w(D) (from the due itself, not due − ivl); otherwise (w(today) ⊖ w(D − ivl)) × 86400, with D − ivl a 64-bit subtraction that saturates at the 64-bit bounds and the product wrapping in 32 bits.
2. When the memory state and `dr` are both present: dr′ = max(`dr`, 0.0001); decay as in X5 step 4; e = w(now) ⊖ w(`lrt`) when `lrt` is present, else e0; r = max(`fsrs::current_retrievability`(state, e as f32 / 86400, decay), 0.0001); RR = −(r^(−1/decay) − 1) / (dr′^(−1/decay) − 1), all in 32-bit floats.
3. Otherwise: RR = −(⌊e0 / 86400⌋ as f32 + 0.001) / max(`ivl` as f32, 1.0).
4. The result is widened to 64 bits. A `due`, `ivl` or now argument that is not an INTEGER gives NULL, and so does a today that is not one when step 1 needs it. A `data` that is not TEXT, or is empty, has no memory state, so step 3 applies.

U41's spec states where it is used; this file only defines it because `sqlfns.rs` holds it.

Added from the oracle (`search/extractors/relative`, `search/extractors/arguments`):

- Step 3 applies whenever step 2's condition fails: no data, unreadable data, no memory state, or a memory state without `dr` (`{"s":5,"d":5,"lrt":…}` takes step 3 although it has a retrievability). It never gives NULL for want of data.
- Step 1 runs before the data is read, so its argument rules hold in both branches: a day-number due with a `today` that is not an INTEGER gives NULL even when the data holds an `lrt`.
- In step 1 the comparison D > 365000 is signed, as in X5. The seconds branch measures from the due itself and the day branch from D − ivl in days; X5's seconds branch subtracts `ivl` instead, so the two functions can see different elapsed times for the same learning card.

For step 2:

- max(NaN, x) = max(x, NaN) = x, for non-NaN x.
- p = f32(−1) / decay: one f32 division.
- x^p uses the system f32 power operation, as in `fsrs` (KTD24; `conformance/extractor-ops#E4`).

- In step 3, ⌊e0 / 86400⌋ is an unsigned integer division, then the nearest f32; 0.001 is the f32 0.0010000000474974513; `ivl` goes from a 64-bit integer to the nearest f32 before the maximum with 1.0.
- Higher values are more retrievable relative to the target (or, in step 3, less overdue). U41 states the sort direction.

| `data` | D | `ivl` | today | RR |
|---|---|---|---|---|
| `{"s":5,"d":5,"dr":0.9,"lrt":1896429600}` | 35 | 10 | 30 | −2 |
| `{"s":5,"d":5,"dr":0.9}` | 35 | 20 | 30 | −3 |
| `{"s":5,"d":5,"dr":0.8,"lrt":1896429600}` | 35 | 10 | 30 | −0.83401954 |
| `{"s":5,"d":5,"dr":0.9,"decay":0.1542,"lrt":1896429600}` | 35 | 10 | 30 | −1.9999998 |
| `[1,2,3,4,5,1896429600,"{}"]` (`dr` 4, decay 5) | 35 | 10 | 30 | 0.4397356 |
| `{"s":0.05,"d":5,"dr":0.9}` | now − 3600 | 7200 | 30 | −0.83333373 (e0 3600; R uses 10800) |
| `{"s":-1,"d":5,"dr":0.9,"lrt":1896429600}` | 35 | 10 | 30 | −4.2631565e8 (r = 0.0001) |
| `{"s":5,"d":5,"dr":0,"lrt":1896429600}`, `dr` −0.5 | 35 | 10 | 30 | −4.6913597e-9 |
| `{"s":5,"d":5,"dr":1,"lrt":1896429600}` / `"lrt":1897293600` | 35 | 10 | 30 | −∞ / NULL |
| `{"s":5,"d":5,"dr":0.9}` | 36 | 1 | 30 | −0 (not yet due) |
| `{}` | 35 | 20 | 30 | −0.75005 |
| `{}` | 5 | 0 | 30 | −25.001 |
| `{}` | 29 / 30 | 0 | 30 | −1.001 / −0.001 |
| `{}` | 35 | −5 | 30 | −0.001 (D − ivl = 40) |
| `{}` | 35 | 40 | 30 | −2.5000001e-5 |
| `{}` | 0 | 0 | 60000 | −10289.001 (wrapped product) |
| `{}` | −2^63 | 1 | 30 | −30.001 (D − ivl saturates at −2^63, which wraps to 0) |
| `{}` | now − 259201 | 5 | 30 | −0.6002 (3 days from the due itself) |
| `{}` | now + 259200 | 5 | 30 | −0.00020000001 |
| `{"s":5,"d":5,"lrt":1896429600}` | −2^31 | 2^31 − 1 | 30 | −1.3504643e-8 |

## X7. Original position

P(data): `pos` as X3 reads it, as an integer, or NULL when absent. U84's sorting spec states how the Original Position column uses it.

`{"pos":7}` and `[7]` give 7, `{"pos":0}` gives 0 and `{"pos":4294967295}` gives 4294967295. `{"pos":4294967296}`, `{"pos":-1}`, `{"pos":-0}`, `{"pos":3.0}`, `{"pos":1e2}`, `{"pos":"7"}` and `{"pos":null}` give NULL, and so does any text that reads as no keys (`{"pos":7,"cd":5}`, `{"pos":7,"pos":8}`). A `cd` string does not matter: `{"pos":7,"cd":"{}"}` gives 7.

## X8. Days, instants and changes of UTC offset

- The functions see only the integers they are given, so their values change exactly when the caller's today or now change.
- With an `lrt`, R changes with every second of now and never jumps at a rollover. Without one, a review card's R, and RR in both branches for a day-number due, change only when today changes.
- Elapsed seconds are real seconds whatever the zone does. In Europe/Paris, 2030-03-31 has 23 hours: from 2030-03-30 12:00 CET (1901098800) to 2030-03-31 12:00 CEST is 82800 s.

Values from `search/extractors/day-edges`, stability 5 and difficulty 5. Today and now are the scheduler's in Europe/Paris for a collection created on 2030-03-01 at 10:00 with the rollover hour 4: day 28 until 2030-03-30 04:00 CET (1901070000), then day 29.

| `data` | D | `ivl` | today / now | R | RR |
|---|---|---|---|---|---|
| `{"s":5,"d":5}` | 30 | 5 | 28 / 1901069999 | 0.9362816 (3 days) | −0.6002 |
| the same | 30 | 5 | 29 / 1901070000 | 0.9176033 (4 days) | −0.8002 |
| `{"s":5,"d":5,"lrt":1900897200}` | 30 | 5 | 28 / 1901069999 | 0.9561494 | −0.6002 |
| the same | 30 | 5 | 29 / 1901070000 | 0.9561491 | −0.8002 |
| `{"s":5,"d":5}` | 1901068200 | 0 | 28 / 1901069999 | 0.99951196 | −0.001 |
| the same | 1901068200 | 0 | 29 / 1901070000 | 0.99951166 | −0.001 |
| `{"s":5,"d":5,"lrt":1901098800}` | 35 | 10 | 29 / 1901181600 (noon CEST, 23 h) | 0.9782512 | −0.4001 |
| the same | 35 | 10 | 29 / 1901185200 (24 h) | 0.97733754 | −0.4001 |

## X9. Scenarios and tests

- Scenarios `conformance/scenarios/search/extractors/`: `card-data` (X3, X4, X7), `retrievability` (X5), `relative` (X6), `arguments` (X2.2) and `day-edges` (X8). They observe the four functions directly through the `card_extractors` observation (`conformance/extractor-ops`), which gives `s`, `d`, `dr`, R, RR and P for each input. Their goldens (`conformance/goldens/search/extractors/`) are the authority. The tables here were read from the oracle with the same inputs and agree with every golden value.
- `conformance/extractor-ops` E4 compares R and RR within a relative 1e-5, because the platforms' math libraries can differ in the last bit of a power (KTD24). The scenarios leave out FSRS-branch RR values whose retrievability or desired retention is close to 1, where one bit becomes a larger relative difference. Meguri's own tests check exact values on one platform:
  - R equals, bit for bit in f32, `fsrs::current_retrievability` called by the test with the stability, decay and t that X5 steps 2–4 give for its inputs;
  - RR's step 2 equals the X6 formula evaluated by the test in f32 with the same crate call;
  - the step 3 values, the NULL rules of X2.2 and X3, and the wraps need no math library and equal the tables here exactly.
- A property test feeds random `data` values (random bytes, any storage class, JSON with random keys, value types and nesting, invalid JSON, the empty text) and random arguments of every storage class to all four functions: each returns NULL or a value, never an error or a panic (except the invalid key of X2.2).
- After an open, the four functions answer on the writer and on a newly opened reader, and `sqlite_master` names none of them (X2.1).

# Stats card graphs

Spec: stats/graphs-cards
Requirements: R1285, R1293, R1294, R1373, R1374, R1375, R1378, R1390, R1391, R1392, R1393, R1394, R1397, R1398, R1399, R1400, R1401, R1402, R1403, R1415, R1416, R1446
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/graphs/future-due/, conformance/scenarios/stats/graphs/card-counts/, conformance/scenarios/stats/graphs/intervals/, conformance/scenarios/stats/graphs/fsrs/, conformance/scenarios/stats/graphs/added/
Status: Approved 2026-11-22 (R1357) for the lines that begin "Changed 2026-11-22 (round 11)". Approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/statistics-round11-20261122.md`); Antoine approved the round 11 design in chat on 2026-11-22 (`docs/design/decisions.md`, rows `stats.overview` of 2026-11-22). Every other section is in force and unchanged.

The data of the seven card graphs (U101): Future Due, Card Counts, Intervals, Stability, Difficulty, Retrievability and Added. They run on the machinery of `stats/graphs-common` (scope G2, history G3, row reading G4, graph order G5, aggregate shape G6, preferences G7, requests G8, caches G9). This file says, per graph, which cards count, the value each card gives, the record's keys and its extras. Ranges, bins, quantiles, medians of Intervals and Stability, cumulative series, tooltip and table text, and every histogram bar's query are `stats/graph-presentation` (U226, `stats/graphs-common` G12, item 1). Card Counts' label queries are `stats/browse-queries` Q4.1.

Every value here equals Anki's graph data for the same collection, scope and clock (R1446). Meguri adds a few extras that Anki's data does not have (C4.2, C6.2); they change none of Anki's values.

## C1. Cards and context

- Each graph reads every card of the scope (`stats/graphs-common` G2, G2.1: a card in a filtered deck is in the scope of its filtered deck and of its home deck), whatever the history value (G3). Only existing cards count (R1285). No graph in this file reads a review-log row.
- Cards are read as `stats/graphs-common` G4 says: `model/row-records` R4 for the columns (so a `due` or `odue` that is not an INTEGER in the `i32` range reads 0) and `model/card-data-json` C3 for `cards.data`. The values used:

  | Value | Source |
  |---|---|
  | `id`, `nid`, `type`, `queue`, `due`, `odue`, `odid`, `ivl` | the card's columns |
  | memory state (stability s, difficulty d) | `s` and `d`, present only when both are (C3 rule 4) |
  | `decay`, `lrt` | the card data's keys, each absent when the data has none |

- The context is G1's: `N` (`next_day_at`, epoch seconds), `today` (`days_elapsed`) and now. Here now is used in whole epoch seconds: now_s = ⌊now / 1,000⌋.
- No graph here reads the UTC offset or the rollover hour itself. Offset changes, DST days and a collection created in another zone reach these graphs only through `N` and `today`, as the day clock gives them (`sched/day-clock` D8). So Added (C10) and Future Due's timestamp days (C3.2) count fixed 86,400 s spans back from `N`, which on and after a DST change need not match the local rollover days. Scenarios `added/dst-edges` and `future-due/tz-matrix/*` pin this (C13).
- f32 means IEEE-754 binary32, each step rounded to f32. Where a value is summed in f32, the cards are taken in ascending `id`.

## C2. Home due

The home due H of a card is its `odue` when its `odid` is above 0, and its `due` otherwise. A negative `odid` (corrupt or imported data; `model/row-records` R4 reads it as i64) counts as not filtered.

- This is not the rule of `search/properties` P4.1 and `search/card-extractors` X1, which take `odue` when `odue` is not 0. The two differ only for a card whose `odid` and `odue` disagree: `odid` > 0 with `odue` 0 gives H = 0 here, and `odid` 0 or negative with a stray `odue` gives H = `due` here. Decided 2026-10-27: Future Due (C3) and Retrievability (C9) take H as Anki's graph data takes it (R1446). Scenario `future-due/filtered-odue` pins these cases.

## C3. Future Due

### C3.1 Cards

The cards whose `type` is not 0 (new) and whose `queue` is not −1 (suspended). The test is on the type, so a buried or suspended new card is never counted. Buried cards are in the set; C3.3 drops some of them.

### C3.2 Due day

For each card of C3.1, the due day k:

| H | k |
|---|---|
| H > 1,000,000,000 (a timestamp) | (H − `N`) ÷ 86,400, in 64-bit integers, the quotient truncated toward zero |
| otherwise (a day number) | H − `today`, in signed 32-bit arithmetic that wraps on overflow |

The kind is chosen by the value of H, not by the queue (R1373). For ordinary cards both give what `prop:due` gives (`search/properties` P4), which chooses the kind by the queue. They differ when the value and the queue disagree: a learning or preview `due` of 1,000,000,000 or less counts as a day number here, a review or interday learning `due` above 1,000,000,000 as a timestamp, and a non-new card in `queue` 0 is counted here and never matched by `prop:due`.

| Card (`today` = 30) | k |
|---|---|
| learning, H = `N` + 1,800 | 0 |
| learning, H = `N` + 90,000 (25 h) | 1 |
| learning, H = `N` − 1 | 0 |
| learning, H = `N` − 86,400 | −1 |
| review or interday learning, H = 33 | 3 |
| review, H = 25 | −5 |
| review in a filtered deck, `due` −100,000, `odid` > 0, `odue` 35 | 5 |
| learning, H = 1,000,000,000 | 999,999,970 |
| review, H = `N` + 259,200 (above 1,000,000,000) | 3 |
| review, H = −2,147,483,648 | 2,147,483,618 (the wrap) |

### C3.3 Buried cards

A card whose `queue` is −2 or −3 (buried) and whose k is 0 or less is not counted. A buried card with k ≥ 1 is counted on k (R1374).

### C3.4 Daily load

daily load = Σ 1 / max(`ivl`, 1) over the cards of C3.1, buried cards included whatever their k (before C3.3, R1378). Each term is an f32 division of 1 by `ivl` converted to f32; the sum is f32 (C1 order); the result is that sum truncated toward zero to an unsigned 32-bit integer. Example: `ivl` 0, 1, 2, 3, 4 give 3.0833333, so daily load 3.

### C3.5 Backlog

- The map holds every counted card's k, negative values included. One map serves both values of `futureDueShowBacklog` (`stats/graphs-common` G7): with the backlog shown U226 uses it whole; with it hidden U226 leaves out the keys below 0. Overdue cards are left out then, never moved to day 0 (AE57).
- have backlog is true when the map has a key below 0. The Backlog control shows only then (R1375; the control is U230's).
- The core never reads `futureDueShowBacklog` for this data; it only carries it (G7).

### C3.6 Record

Changed 2026-11-22 (round 11): the record also carries the Meguri extra `future_due_young` of `stats/history-reads` H8 (the same map for cards with `ivl` < 21), not observed (C12).

| Member | Value |
|---|---|
| `future_due` | k → number of counted cards (C3.2, C3.3), sparse, keys signed 32-bit |
| `have_backlog` | C3.5 |
| `daily_load` | C3.4 |
| due tomorrow (Meguri) | the count at key 1, or 0; the same with the backlog shown or hidden (`stats/graphs-common` G6 lists it as an extra) |

- The range caps (31, 90, 365 days, the last due day), the total and average of the shown range and the bar queries are U226's.
- A bar's `prop:due` query (U226) can find other cards than the bar counts, in Anki as in Meguri: `prop:due` never matches buried cards (P4), which C3.3 counts for k ≥ 1, and it takes H and the due kind by its own rules (C2, C3.2).

AE57: 30 review cards due 1 to 3 days ago (10 on each day), 5 buried review cards due today and 3 due tomorrow, all with `ivl` 2: `future_due` {−3: 10, −2: 10, −1: 10, 1: 3}, have backlog true, daily load 19 (all 38 cards). With the backlog hidden only key 1 remains; due tomorrow is 3.

## C4. Card Counts

### C4.1 Counts

Two count sets, each with the members `newCards`, `learn`, `relearn`, `young`, `mature`, `suspended`, `buried` (R1390). A card's type bucket is: `type` 0 → `newCards`, 1 → `learn`, 3 → `relearn`, 2 → `young` when `ivl` < 21 and `mature` otherwise.

| `queue` | `excluding_inactive` | `including_inactive` |
|---|---|---|
| −1 | `suspended` | the type bucket |
| −2, −3 | `buried` | the type bucket |
| any other | the type bucket | the type bucket |

- `including_inactive` never counts `suspended` or `buried`: they stay 0.
- `cardCountsSeparateInactive` true (the default) shows `excluding_inactive`, false shows `including_inactive` (R1391). Both sets are in the record (G7); the core does not read the key for this data.
- The Total row and the percentages are U226's: both totals equal the number of cards in the scope.

### C4.2 Notes (Meguri)

The number of distinct `nid` among the scope's cards, the same for both settings (R1392). Anki's data has no such value; it is a Meguri-only test.

### C4.3 Label queries

The record holds the item text of each shown label for both settings, as `stats/browse-queries` Q4.1 writes them (Q1: the core puts fixed-item texts in the record). Each label's joined text finds exactly the cards its count counts (Q4.1, AE60).

### C4.4 Record

Changed 2026-11-22 (round 11): the record also carries the Meguri extra `suspended_by_state` of `stats/history-reads` H8 (the suspended cards by type bucket), not observed (C12).

`including_inactive` and `excluding_inactive` (C4.1), the notes count (C4.2) and the label texts (C4.3).

## C5. Intervals

- Cards whose `type` is 2 or 3 (review, relearning), in every queue, suspended and buried included (R1394).
- Key: the card's `ivl` (unsigned 32-bit) → number of cards. Key 0 is kept in the data; hiding the zero interval is U226's (R1394).
- Record `intervals`. Quantile ranges, the cumulative percentage and the median are U226's (R1293).

## C6. Memory-state graphs

### C6.1 Cards

Stability, Difficulty and Retrievability count every scope card that has a memory state (C1), of every type and queue: new, suspended and buried cards with `s` and `d` count too (R1397).

### C6.2 Cards left out (Meguri)

left out = the number of scope cards whose `type` is 2 or 3 and that have no memory state (R1398). One value, shown with each of the three graphs. Anki's data has no such value; it is a Meguri-only test.

### C6.3 The `fsrs` key

The three graphs are computed whatever the `fsrs` key says, as Anki computes them. The result carries the key (G5, G7); Meguri always shows them (R1397, U231).

## C7. Stability

- Key: s rounded to the nearest integer, halves away from zero, then converted to an unsigned 32-bit integer with saturation (below 0 gives 0, NaN gives 0, +∞ and values above 4,294,967,295 give 4,294,967,295) → number of cards.
- Examples: s 0.4 → 0, 0.5 → 1, 2.5 → 3, 20.49 → 20, −3 → 0, `1e40` (+∞, `model/json-numbers` N2) → 4,294,967,295.
- Record `stability`, with the member `intervals` (Anki uses the Intervals message). Ranges and the median are U226's (R1399).

## C8. Difficulty

### C8.1 Bins

For each card of C6.1: x = ((d − 1) / 9) × 100 in f32. The key is 99 when x is exactly 100, and otherwise ⌊x⌋ converted to an unsigned 32-bit integer with saturation (below 0 and −∞ give 0, values above 4,294,967,295 and +∞ give 4,294,967,295) (R1400). A d of `1e40` or `-1e40` reads as ±∞ (`model/card-data-json` C3 rule 3, `model/json-numbers` N2). x is never NaN: JSON has no NaN and (±∞ − 1) / 9 is ±∞.

| d | x (f32) | key |
|---|---|---|
| 1 | 0 | 0 |
| 1.09 | 1.0000004 | 1 |
| 2.08 | 11.999999 | 11 |
| 4.6 | 39.999996 | 39 |
| 5.5 | 50 | 50 |
| 10 | 100 | 99 |
| 11 | 111.111115 | 111 |
| 0.5 | −5.555556 | 0 |
| `1e40` | +∞ | 4,294,967,295 |
| `-1e40` | −∞ | 0 |

The f32 steps matter: d 2.08 and 4.6 fall one key below the decimal reading.

### C8.2 Median

- The median of the values (d − 1) / 9 (f32, not × 100) over the cards of C6.1: for an odd count the middle value, for an even count the f32 mean (a + b) / 2 of the two middle values; then × 100 in f32. With no card it is 0.
- Anki's data returns it in the Difficulty message's `average` member, and its table shows it as Median difficulty (R1401), so the core returns it. Decided 2026-10-27: a 1% map cannot give it exactly, so it is a scalar beside the map (`stats/graphs-common` G6, what a record's maps cannot give). It is Anki's `average` member, not a Meguri extra, and it is observed (C11, C12). The core computes it from a count per distinct f32 value, cumulated in order, never from one value per card handed to U226 (R1293).
- ±∞ take part like any other value, −∞ first and +∞ last: −∞ and +∞ as the two middle values give NaN. No scenario has that case.
- The first eight cards of C8.1: 26 (the middle values 0.11999999 and 0.39999998). Without d 0.5: 39.999996. The first eight with `1e40` added: 39.999996 (the fifth of nine values). d `-1e40`, 5.5 and 10: 50.0 (the middle of −∞, 0.5 and 1).

### C8.3 Record

`eases` (the map of C8.1; Anki's field name) and `average` (C8.2).

## C9. Retrievability

### C9.1 Elapsed seconds

For each card of C6.1, the elapsed seconds e, an unsigned 32-bit value. Differences and products are taken in 64-bit integers with wrapping (two's-complement) arithmetic, so no stored value overflows or stops the request: an `lrt` of −2^63, which `model/card-data-json` C3 accepts, gives now_s − `lrt` = now_s − 2^63 after the wrap. "mod 2^32" then keeps the low 32 bits (so a negative difference becomes a large e, and that `lrt` gives e = now_s mod 2^32).

A card is **day-due** when its `type` is not 0 and H ≤ 365,000, or its `queue` is 2 or 3, or its `type` is 2 and its `queue` is below 0.

| Card | e |
|---|---|
| `lrt` present | (now_s − `lrt`) mod 2^32 |
| no `lrt`, day-due, `queue` 1 | max(now_s − (H − 86,400 × `ivl`), 0) mod 2^32 |
| no `lrt`, day-due, any other queue | max((`today` − H + `ivl`) × 86,400, 0) mod 2^32 |
| no `lrt`, not day-due | (now_s − H) mod 2^32 |

H is C2's. The review log is never read for a missing `lrt` (`stats/graphs-common` G9.1).

### C9.2 Value

R is `search/card-extractors` X5 steps 3 to 5 with e from C9.1: t = e as f32 / 86,400 in f32, decay = the card's `decay` or 0.5 (a `null` or string `decay` is absent, `model/card-data-json` C3 rule 3), R = `fsrs::current_retrievability` of the pinned `fsrs` crate, in f32 (R1402). The preset's decay is never read. At C9.6's clock, s 5 and d 5: 10 days with decay 0.1542 give R 0.8458846 (key 84), −0.5 give 0.7874007 (78), 0 give 1 (99), and no `decay`, `null` or `"0.2"` give 0.8250286 (82); 15 days with 0.1542 give 0.80938804 (80) and with −0.5 give 0.6557437 (65) (scenario `fsrs/retrievability-decay`). Meguri calls that function; it does not reimplement it. X5's step 6 does not apply: R stays f32 for the bins (C9.3) and the sums (C9.4).

### C9.3 Bins

x = R × 100 in f32. The key is 99 when x is exactly 100, and otherwise ⌊x⌋ converted to an unsigned 32-bit integer with saturation (NaN and values below 0 give 0) → number of cards.

### C9.4 Sums

- `sum_by_card`: the f32 sum of R over the cards of C6.1 (C1 order).
- `average`: `sum_by_card` × 100 / (number of cards of C6.1), in f32; 0 when there is none (R1403).
- `sum_by_note`: for each distinct `nid` among the scope's cards, whether or not a `notes` row has that id (Anki groups by the card's `nid` and never reads the notes table), the mean of R over its scope cards, a card without a memory state adding 0 (f32 sum in C1 order, divided in f32 by the number of its scope cards); then the f32 sum of those means in ascending note id. Anki adds the notes in an unspecified order, so its last bits can vary between runs; scenarios keep the note count small and compare within `stats/graphs-common` G14's tolerance.
- The edge values below are the function's own (X5's list); Meguri gets them by calling it. A NaN R gets key 0 and makes the sums NaN, as in Anki. With a positive decay (0.5 when the data has none):
  - a stability of 0 gives R = 0 (key 0) when e > 0 and NaN when e = 0; a stability of +∞ gives 1 (key 99);
  - a negative stability gives NaN when the base t / s × factor + 1 is below 0, and otherwise R of 1 or more: key 99 at e 0, key 100 or above for a small e (s −3 with e 3,600: key 100), and +∞ (key 4,294,967,295, infinite sums) when the base is exactly 0.
- A stored decay of 0 (either sign) gives R = 1 for every e and stability, NaN base included (key 99). A negative decay makes factor negative, so R is below 1 for a positive stability (−0.5: 0.7874007 after 10 days at stability 5, key 78), and the base falls below 0 for a large enough e. For any decay, a negative base gives NaN only when −decay is not a whole number; otherwise R is a finite value (key 0 when it is negative). Scenario `fsrs/retrievability-decay` has decay 0 and −0.5 with a positive stability; no scenario has the other cases of this item.

Example: note A with a card of R 0.7182781 and a card without a memory state, note B with one card of R 0.7661308: `sum_by_card` 1.4844089, `average` 74.22044, `sum_by_note` 1.1252699 (0.35913906 + 0.7661308).

### C9.5 Now

R is computed for G1's now at every request: never cached, never reused from an earlier refresh (R1294, `stats/graphs-common` G8.3). The column caches hold `lrt`, `due`, `ivl` and the memory state, never R.

### C9.6 Differences from `prop:r`

`prop:r` and the queue order use X5's elapsed rule, which differs from C9.1. Decided 2026-10-27: the graph follows C9.1, which is Anki's graph data (R1446) and the rule `stats/graphs-common` G9.1 refers to. The plan's "through U82a's retrievability function" holds for the `fsrs` call (C9.2) only. The two agree for a card whose `lrt` is from 0 to now_s, and for a review card without an `lrt` whose H is the same by C2 and by X1, is at most 365,000 and is at least its `ivl`. Elsewhere they can differ: an `lrt` of −5 gives the graph e = now_s + 5 and X5 e = 0 (X5 wraps `lrt` to 32 bits first), and a review card with H above 365,000 takes X5's seconds branch but C9.1's day branch.

| Card (s 5, d 5 unless named; `today` 30, now_s 1897293600) | e graph / X5 | R graph / `prop:r` | key |
|---|---|---|---|
| `lrt` = now_s − 864,000, s 2.5 | 864,000 / 864,000 | 0.7182781 / same | 71 |
| review, no `lrt`, H 35, `ivl` 20 | 15 days / 15 days | 0.7661308 / same | 76 |
| `lrt` = now_s + 10 | 4,294,967,286 / 0 | 0.020703057 / 1 | 2 |
| relearning, `queue` 1, no `lrt`, H = now_s + 600, `ivl` 0 | 4,294,966,696 / 0 | 0.020703059 / 1 | 2 |
| relearning, `queue` 1, no `lrt`, H = now_s − 3,600, `ivl` 2, s 4, d 6 | 3,600 / 3,602 | 0.9987805 / 0.9987799 | 99 |
| review, no `lrt`, H 35, `ivl` 40 | 35 days / 30 days | 0.6152273 / 0.6445033 | 61 |
| new (`type` 0, `queue` 0) with a memory state, `due` 5 | now_s − 5 / (not matched, P8) | 0.031140784 / none | 3 |
| review in a filtered deck, `odid` > 0, `odue` 0, no `lrt`, `due` 35, `ivl` 20 | 50 days (H = 0) / 15 days (D = `due`) | 0.54671097 / 0.7661308 | 54 |
| review, `odid` −1, no `lrt`, `due` 35, `odue` 40, `ivl` 20 | 15 days (H = `due`) / 10 days (D = `odue`) | 0.7661308 / 0.8250287 | 76 |
| relearning, `queue` 1, no `lrt`, H 25 (a day number), `ivl` 2 | now_s + 172,775 / 7 days | 0.031139368 / 0.8676336 | 3 |
| suspended review (`queue` −1), no `lrt`, H 1,896,429,600, `ivl` 0 | 0 (day-due by type and queue) / 864,000 | 1 / 0.8250287 | 99 |

A card last reviewed 400 days ago with no `lrt` (H = `today` − 400 + `ivl`) meets the agreement condition only when `today` ≥ 400. At `today` 30 with `ivl` 420 and H 50 the graph gives 400 days and X5 30 days (D ⊖ `ivl` stops at 0).

### C9.7 Record

`retrievability` (the map of C9.3), `average`, `sum_by_card`, `sum_by_note` (C9.4).

## C10. Added

- Every scope card (R1415). Its creation second c = `id` ÷ 1,000 in 64-bit integers, truncated toward zero. Its day a = ⌈(c − `N`) / 86,400⌉, computed in 64-bit floats and converted to a signed 32-bit integer with saturation.
- So day 0 holds c in (`N` − 86,400, `N`], day −k holds (`N` − (k + 1) × 86,400, `N` − k × 86,400], and a card created after `N` (clock skew, an imported id) gets a day of 1 or more. These edges differ from `stats/day-buckets` B3, which truncates toward zero.

  | `id` (`N` = 1791856800) | a |
  |---|---|
  | 1791856800000 (`N`) | 0 |
  | 1791770401000 | 0 |
  | 1791770400999 | −1 |
  | 1791770400000 (`N` − 86,400 s) | −1 |
  | 1791856801000 | 1 |
  | 1757296800000 (`N` − 400 days) | −400 |
  | 1791856799999 (1 ms before `N`) | 0 (the ceiling is −0) |
  | −1 (c = 0: the division truncates toward zero) | −20,739 |
  | −1500 (c = −1) | −20,739 |

- The spans are fixed 86,400 s back from `N`, whatever the zone's DST changes (C1). Europe/Paris, rollover 4: at 2026-10-26 12:00 local (after the October change), `N` = 1793070000 and `id` 1792809000000 (2026-10-24 04:30 CEST, two local days back) gets −3; at 2027-03-29 12:00 local (after the March change), `N` = 1806372000 and `id` 1806114600000 (2027-03-27 03:30 CET, before that day's rollover, three local days back) gets −2.

- Record `added`: a → number of cards. Ranges, the cumulative series, the table and the `added:` bar queries are U226's (R1416).

## C11. Records

Changed 2026-11-22 (round 11): Future Due's Meguri extras add `future_due_young` and Card Counts' add `suspended_by_state` (`stats/history-reads` H8). The Statistics page reads `intervals` only for its median (`stats/statistics-page` S7.13).

| Graph (G5) | Part (C12) | Anki's members | Meguri extras |
|---|---|---|---|
| Future Due | `future_due` | `future_due`, `have_backlog`, `daily_load` | due tomorrow, `future_due_young` (round 11) |
| Card Counts | `card_counts` | `including_inactive`, `excluding_inactive` | notes, label texts, `suspended_by_state` (round 11) |
| Intervals | `intervals` | `intervals` | none |
| Stability | `stability` | `intervals` | left out |
| Difficulty | `difficulty` | `eases`, `average` | left out |
| Retrievability | `retrievability` | `retrievability`, `average`, `sum_by_card`, `sum_by_note` | left out |
| Added | `added` | `added` | none |

Nothing else is in these records (G6).

## C12. Observation

U101's parts of the `stats` kind (`stats/graphs-common` G14), in G14's form (field names as Anki's protobuf file declares them, map keys as decimal strings, default members omitted, the message whole):

- `future_due`: `{future_due: {<k>: <cards>}, have_backlog, daily_load}`;
- `intervals`: `{intervals: {<ivl>: <cards>}}`;
- `stability`: `{intervals: {<rounded s>: <cards>}}`;
- `difficulty`: `{eases: {<key>: <cards>}, average}`;
- `retrievability`: `{retrievability: {<key>: <cards>}, average, sum_by_card, sum_by_note}`;
- `card_counts` and `added` as G14 defines them.
- A map member with no key is omitted, by G14's default rule, as in `stats/graphs-revlog` V8: `stability` with no card is `{}`, and `future_due` with no counted card holds at most `daily_load`.
- A message member is written whenever Anki's data sets it, empty or not; only empty maps and scalar members at their default are omitted. Anki always sets both count sets of `card_counts`, so a scope with no card gives `{excluding_inactive: {}, including_inactive: {}}`, never `{}`, and each of the other six parts gives `{}` (scenario `card-counts/empty-scope`).

Meguri maps the record's Anki members into these shapes; the Meguri extras are not observed. The oracle's graph data reads Anki's clock, which the scenario's `set_clock` and `advance` fix (time stands still between ops), so R is reproducible. Numbers compare with G14's tolerance (1e-6 relative, `conformance/observations` O7), which applies only when both sides write the number with a fraction or exponent: f32 members (`average`, `sum_by_card`, `sum_by_note`) are written as the shortest f32 decimal with a fraction (26.0, never 26), as `conformance/observations` says for 32-bit floats. Keys and counts compare exactly, so retrievability scenarios keep every R at least 1e-4 from a whole percent, where a last-bit difference of the platform's power function (KTD24) could move a key.

## C13. Scenarios

For the oracle-maintainer, under `conformance/scenarios/stats/graphs/`, with the `stats` kind (C12) of `oracle/driver/ops/stats.py`; every scenario below is in the tree with its golden (the first table as of 2026-10-27, the second as of 2026-11-17). The filtered deck `F` is a `decks` row written as `conformance/deck-ops` K13 says (`build_filtered` has no driver op), moved into with `db_exec`. Cards come from setup notes, then `db_exec` sets `type`, `queue`, `due`, `ivl`, `odue`, `odid`, `data`, `did` and, for Added, `id` (`conformance/stats-ops` H3). `did` and `odid` take deck ids from the `decks` table by name (H3, "Card moves"). A card moved into `F` gets `did` 900 (the row's fixed id) and `odid` the home deck's id by name, then the scenario `reopen`s before its observation. All in Europe/Paris, rollover 4, Collection scope and `year` unless named.

The retrievability clock, for the scenarios that name it: `clock.start` 2030-01-15T11:00:00 (Paris wall time, 10:00 UTC), so the new collection's `crt` is 2030-01-15 04:00 +01:00 (`sched/day-clock` D11), then `advance` 30d before `db_exec` and the observation. Now is then 2030-02-14 10:00:00 UTC (now_s 1897293600), `today` 30 and `N` 1897354800, the values of C9.6's table. "Day t + k" is the day number `today` + k. Values are this file's; the goldens are the authority once recorded.

| Scenario | Pins | Expected |
|---|---|---|
| `future-due/ae57-buried-backlog` (AE57) | C3.3 to C3.5 | the AE57 cards of C3.6: `future_due` {−3: 10, −2: 10, −1: 10, 1: 3}, `have_backlog` true, `daily_load` 19 |
| `future-due/due-kinds` | C3.2 | at the retrievability clock: learning cards due `N` + 1,800 s, `N` + 90,000 s, `N` − 1 s, `N` − 86,400 s; an interday learning card due day t; a review card due day t + 3; a learning card with `due` 1,000,000,000: keys 0: 3, 1: 1, −1: 1, 3: 1 and 1,000,000,000 − `today` (999,999,970) |
| `future-due/filtered-odue` | C2 | at the retrievability clock, so −`today` is not key 0: in filtered deck `F` with home `H`: a review card (`due` −100,000, `odue` t + 5) and a learning card with `odue` 0; in `H`: a review card with `odid` 0, `due` t + 2 and `odue` t + 7, and a review card with `odid` −1, `due` t + 3 and `odue` t + 9: keys 5, −`today` (−30), 2 and 3 in Collection and Deck `H`, keys 5 and −30 in Deck `F` |
| `future-due/types-and-queues` | C3.1, C3.3 | a suspended review card, a suspended and a buried new card (none counted), a buried review card due t + 4 (key 4), a buried review card due t (not counted, in `daily_load`) |
| `future-due/daily-load` | C3.4 | five review cards due t + 1 with `ivl` 0 to 4: `daily_load` 3; no `have_backlog` member |
| `card-counts/states` | C4.1 | one card each: new, learning, relearning, young (`ivl` 20), mature (`ivl` 21), suspended mature, buried-by-scheduler new, buried-by-user learning: `excluding_inactive` {newCards 1, learn 1, relearn 1, young 1, mature 1, suspended 1, buried 2}, `including_inactive` {newCards 2, learn 2, relearn 1, young 1, mature 2} |
| `card-counts/filtered-deck` | C4.1, G2.1 | a review card in filtered deck `F` from home `H`: counted by its type in Deck `F`, Deck `H` and Collection |
| `intervals/types-and-queues` | C5 | review cards with `ivl` 0, 1, 21 and 36,500, a suspended review card with `ivl` 21, a relearning card with `ivl` 1, a new card and a learning card with `ivl` 5: {0: 1, 1: 2, 21: 2, 36500: 1} |
| `fsrs/stability-rounding` | C6.1, C7 | observing `stability`: review cards with d 5 and s 0.4, 0.5, 2.5, 20.49, −3 and `1e40`, a new card with s 2.5 and d 5, a review card with s 7 and no `d`: {0: 2, 1: 1, 3: 2, 20: 1, 4294967295: 1} |
| `fsrs/difficulty-bins` | C8 | observing `difficulty`: review cards with s 5 and the eight d values of C8.1: {0: 2, 1: 1, 11: 1, 39: 1, 50: 1, 99: 1, 111: 1}, `average` 26.0 |
| `fsrs/retrievability-elapsed` | C9.1 to C9.3, C9.6 | the first seven rows of C9.6, one card each, at the retrievability clock: `retrievability` {2: 2, 3: 1, 61: 1, 71: 1, 76: 1, 99: 1} |
| `fsrs/retrievability-sums` | C9.4 | the example of C9.4, at the retrievability clock: note A with the first card of C9.6 (`lrt` now_s − 864,000, s 2.5, d 5) and a second card with no `s` and `d`; note B with the second card of C9.6 (review, no `lrt`, H 35, `ivl` 20, s 5, d 5): `sum_by_card` 1.4844089, `average` 74.22044, `sum_by_note` 1.1252699 |
| `fsrs/retrievability-now` | C9.5, R1294 | review cards with `lrt`: `stats` with `year`, `advance` 10 days, `stats` with `year` and `all`: the keys move between the first two outputs and the last two are equal |
| `fsrs/fsrs-off` | C6.3 | two review cards with s 5, d 5 and an `lrt` 10 days back, observed with `fsrs` true and then false: the `stability`, `difficulty` and `retrievability` parts are equal; `context` has `fsrs` only the first time |
| `added/day-edges` | C10 | `clock.start` 2026-10-12T12:00:00 (Paris wall time, 10:00 UTC), so `N` = 1791856800 as in C10's table; cards with the six ids of that table: {0: 2, −1: 2, 1: 1, −400: 1}, with `year` and `all` |

Meguri-only tests (no oracle field): due tomorrow (C3.6), the notes count (C4.2), the label texts in the record for both settings (C4.3), the left-out count (C6.2), f32 summation order (C1, C9.4), the difficulty median from distinct-value counts against a sort of all values (C8.2), and that R is recomputed on every request after a clock change with the caches warm (C9.5). Also:

- AE60, core part (`stats/browse-queries` Q4.1, Q6): the `card-counts/states` cards in a deck `Japanese`, with `cardCountsSeparateInactive` true and false. Each shown label's text, joined with the Deck scope by Q6's test-only join and run through U80, finds exactly the cards of that label's count.
- Graph against `prop:r` (C9.6): a review card with no `lrt`, last reviewed 400 days ago, at `today` ≥ 400 (`advance` 430d from the retrievability clock's start, H = `today` − 400 + `ivl`). Its key is the same with `year` and `all`, and its R equals `prop:r`'s (`search/card-extractors` X5).

Recorded 2026-11-17 (proposed 2026-11-10). The rows that add cards to a scenario of the first table are scenarios of their own, so the first table's goldens stay as they are. Values are the goldens'.

| Scenario | Pins | Expected |
|---|---|---|
| `fsrs/retrievability-decay` | C9.2 | at the retrievability clock, review cards with s 5 and d 5, one per deck: `lrt` now_s − 864,000 with `decay` 0.1542, −0.5, none, `null`, `"0.2"` and 0 give keys 84, 78, 82, 82, 82 and 99; no `lrt`, H 35, `ivl` 20 with the same six decays give 80, 65, 76, 76, 76 and 99. Each deck's `average` is its card's R × 100 (C9.2's values) |
| `fsrs/retrievability-elapsed-odd` | C2, C9.1, C9.6 | the last four rows of C9.6 (keys 54, 76, 3, 99; the filtered card in Deck `F` and in its home deck), and a review card with `lrt` −9223372036854775808 (e 1897293600, R 0.031140784, key 3), one card per deck |
| `fsrs/difficulty-infinite` | C8 | review cards with s 5. Deck `A`: the eight d values of C8.1 and `1e40` (key 4294967295), `average` 39.999996; Deck `B`: `-1e40` (key 0), 5.5 and 10, `average` 50.0; Collection: `average` 45.0 |
| `future-due/due-extremes` | C3.2 | at the retrievability clock, review cards with H `N` + 259,200 (key 3) and −2,147,483,648 (key 2147483618): `daily_load` 1, no `have_backlog` |
| `fsrs/retrievability-orphan` | C9.4 | at the retrievability clock, the two cards of C9.4's example that have a memory state, on notes n1 and n2; n1's `notes` row deleted with `db_exec`: the orphan card stays in the scope (Collection and Deck), `sum_by_card` and `sum_by_note` 1.4844089 (two groups of one card), `average` 74.22044 |
| `added/day-edges-odd` | C10 | at `added/day-edges`' clock, ids 1791856799999 (0), −1 and −1500 (both −20739) |
| `added/dst-edges` | C1, C10 | Europe/Paris. At 2026-10-26 12:00 (`N` 1793070000): ids 1792809000000 (−3, two local days back) and 1792810801000 (−2). `set_clock` 2027-03-29 12:00 (`N` 1806372000): ids 1806114600000 (−2, three local days back) and 1806112800000 (−3); the first two are then −156 |
| `card-counts/empty-scope` | C12 | Search `tag:absent` (no card), every card part: `card_counts` `{excluding_inactive: {}, including_inactive: {}}`, the six others `{}` |
| `future-due/tz-matrix/*` | C1, C3.2, C10 | five zones at 12:00 local, each observing `timing` and every card part in Collection and Deck scope (equal): `paris-fall-back` (Europe/Paris, 2026-10-25; the scheduler day before it lasts 25 h), `lord-howe-dst` (Australia/Lord_Howe, 2026-10-04, its DST start; the day before lasts 23.5 h), `kathmandu` (+05:45), `st-johns` (−02:30) and `other-creation-zone` (Paris, `crt` and `creationOffset` of America/Los_Angeles, `today` 2), as the `buttons/tz-matrix` scenarios set them up. Six learning cards (`ivl` 0) and two review cards (`ivl` 3 and 21) with set ids; values in each scenario's comment. In all five, the parts that the zone cannot move are equal: `intervals` {3: 1, 21: 1}, `stability` {5: 1, 10: 1}, `difficulty` {22: 1, 44: 1} with `average` 33.333336, `retrievability` {81: 1, 97: 1}, `daily_load` 6 |

In `future-due/tz-matrix/paris-fall-back` (`N` 1792983600) a learning card due 1792807200 with that second as its id (2026-10-24 04:00 CEST, the start of the local day before) gets Future Due key −2 and Added day −2, where local days would give −1: `future_due` {−2: 1, −1: 2, 0: 3, 1: 2}, `added` {−2: 2, −1: 2, 0: 4}. In `lord-howe-dst` (`N` 1791133200) a card due and created at 1790961000 (2026-10-03 03:40 +10:30, two local days back) gets −1 in both, where local days would give −2; the parts are those of `paris-fall-back`. In the three zones without a DST change on the day, `future_due` is {−2: 1, −1: 1, 0: 4, 1: 2} and `added` {−2: 1, −1: 2, 0: 5}. In `kathmandu` that includes the ids `N` × 1000 (day 0) and (`N` − 86,400) × 1000 (day −1).

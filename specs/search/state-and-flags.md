# Search: card state and flags

Spec: search/state-and-flags
Requirements: R1062, R1063, R1064, R1268, R1916
Anki: 26.09.3
Scenarios: conformance/scenarios/search/state/, conformance/scenarios/sched/options/prefs-rollover.yaml

What `is:` and `flag:` match. S1–S4 were written first, for `is:due` and `is:buried` alone; S5–S7 add the other state words, flags and the day-change rule of the scenarios (U82). The values are `grammar.md` G8 (`invalid_state`, `invalid_flag`); every term here has card scope (`query-model.md` Q3). Parsing, boolean operations and result modes follow `search/grammar` and `search/query-model`. The scenarios under `conformance/scenarios/search/state/` were checked against the oracle and pin the examples.

## S1. Due predicate

`is:due` is a card-scoped predicate. Its result depends on the stored `queue` and `due`, and the context values in S3:

| Stored queue | Meaning | Matches when |
|---|---|---|
| 2 | review | `due` is at most today |
| 3 | interday learning | `due` is at most today |
| 1 | intraday learning | `due` is at most now plus learn-ahead seconds |
| 4 | preview repeat | `due` is at most now plus learn-ahead seconds |
| any other value | new, suspended, buried, or another queue | never |

Both cutoffs are inclusive. The card's `type` adds no condition: the same table applies to unusual type/queue combinations. The predicate does not require a valid deck or inspect its preset. It reads `due` directly, including for a card in a filtered deck; it never substitutes `odue`, and `odid` does not affect the result.

The intraday cutoff may pass the next day boundary. Do not cap it at that boundary or recalculate today from now. This search is a stored-card predicate, not a request to gather a study queue: there are no daily-limit, sibling-burying, study-eligibility or queue-admission checks.

Oracle examples (`search/state/due`), one second before a day boundary, with the default learn-ahead of 1200 s:

| Card | `is:due`? |
|---|---|
| review, due today / tomorrow / 5 days ago | yes / no / yes |
| learning, due now + 600 / + 1200 / + 1201 / − 100 | yes / yes / no / yes |
| interday learning, due today / tomorrow | yes / no |
| preview, due now + 600 | yes |
| relearning in queue 1, due now + 600 | yes |
| suspended or buried review, due 5 days ago | no |
| filtered review, `due` −100000, home due (`odue`) in 50 days | yes |
| new | no |

At the boundary, today grows by one and now by one second, so the review card due tomorrow, the learning card due at now + 1201 and the interday learning card due tomorrow become due (and the buried review card, unburied by the day change, S7). The scenario then sets `collapseTime` to 0, 1199, `"x"`, −1 and 60.0 and removes it (S3).

## S2. Buried predicate

`is:buried` is true exactly when stored `queue` is -2 (sibling burial) or -3 (manual burial). It is false for every other queue, including -1 (suspended). The result does not depend on `type`, `due`, `odue`, `did` or `odid`. Reading it never unburies cards or performs day maintenance.

## S3. Context and arithmetic

The engine supplies the query context (`search/query-model#Q8`). Search reads no clock, config, scheduler, collection timing service or engine callback.

| Leaf | Declared context dependencies |
|---|---|
| `is:due` | now, today, learn-ahead seconds |
| `is:buried` | none |
| the other `is:` words (S5), `flag:` (S6) | none |

Declare all three due dependencies even when no currently stored card uses one of the queue groups. Neither predicate reads next-day-at. Groups and boolean operators add no dependencies; the plan records the union of its leaves' dependencies (Q9).

The engine fills learn-ahead seconds from the config key `collapseTime` (`query-model.md` Q8): its value when it reads as an unsigned 32-bit integer, otherwise 1200. A missing key, a string (`"x"`), a negative number (−1) and a real (60.0) all give 1200 (`search/state/due`).

No term of this file is slow (`query-model.md` Q9): each compares stored columns.

Context bounds: now uses signed 64-bit seconds; today and learn-ahead seconds each use unsigned 32-bit integers. Keep stored `due` signed. Negative due values may match. Do not cast today to a signed 32-bit value.

Meguri's arithmetic rule for the full representable context domain is mathematical integer addition for now plus learn-ahead seconds: use sufficient width, or cap an overflowing upper cutoff at the largest signed 64-bit value before the comparison. Never wrap, panic, convert through floating point or allow SQLite numeric overflow to alter the comparison. Zero learn-ahead makes the cutoff exactly now. This boundary rule defines Meguri's safe handling beyond ordinary clock values; the queue and comparison semantics of S1 follow the pinned behavior.

## S4. Verification vectors and composition

Use fresh synthetic cards. With today 12, now 2000000000 and learn-ahead 1200:

| Queue | Due | `is:due` | `is:buried` |
|---|---|---|---|
| 2 or 3 | 11 or 12 | true | false |
| 2 or 3 | 13 | false | false |
| 1 or 4 | 2000001199 or 2000001200 | true | false |
| 1 or 4 | 2000001201 | false | false |
| 0 or -1 | 0 | false | false |
| -2 or -3 | 0 | false | true |
| another queue | 0 | false | false |

Repeat applicable rows with different `type` values, with `odid` zero and nonzero, and with an `odue` on the opposite side of the cutoff. Results stay the same. Also cover zero learn-ahead, negative `due`, today 4294967295, and now at the signed 64-bit maximum with positive learn-ahead.

Apply ordinary AND, OR and NOT from Q7; neither leaf introduces an unknown value on valid stored integer columns. A due card cannot be buried under these definitions. Thus `is:due is:buried` matches no card, and `is:due OR is:buried` matches their union. Negation is the boolean complement within the query's row scope, not an alternate scheduler state.

Both result modes use the same per-card truth value (Q4–Q6). Cards mode returns matching card ids. Notes mode returns each note once when one of its cards satisfies the entire expression. A note with a due card and a separate buried card matches the OR expression but does not match the AND expression. A note with a buried and an unburied card can match both `is:buried` and `-is:buried` in separate searches. Preserve Q5–Q6 scope behavior for cards without notes and notes without cards; do not add joins merely to implement these card-only leaves.

## S5. Other state words (R1062)

A card has a type (`cards.type`: 0 new, 1 learning, 2 review, 3 relearning) and a queue (`cards.queue`: 0 new, 1 learning within the day, `due` in epoch seconds; 2 review; 3 learning across days, `due` a day number; 4 preview in a filtered deck, `due` in epoch seconds; −1 suspended; −2 buried by the scheduler, with a sibling; −3 buried by the user). The state words test them as stored:

| Value | A card matches when |
|---|---|
| `new` | type is 0 |
| `learn` | type is 1 or 3 |
| `review` | type is 2 or 3 |
| `suspended` | queue is −1 |
| `buried` | queue is −2 or −3 (S2) |
| `buried-sibling` | queue is −2 |
| `buried-manually` | queue is −3 |
| `due` | S1 |

- `new`, `learn` and `review` look at the type only, so a suspended or buried card keeps its state: a suspended new card is `is:new`, a user-buried relearning card is `is:learn`, `is:review` and `is:buried-manually`.
- A relearning card is both `is:learn` and `is:review`.
- Type and queue values outside these lists (another client's data) match none of the words that test that column.
- The word is compared exactly (G8, kind R): `is:NEW`, `is:Due`, `is:learning` and `is:buried_manually` are `invalid_state`. The key is not case-sensitive (`IS:new`).
- In notes mode a note matches when one of its cards matches the whole query (`query-model.md` Q4).

Scenario: `search/state/is`.

## S6. `flag:` (R1064)

- A card matches `flag:N` when its `flags` column, bitwise AND 7, equals N: the lowest three bits hold the flag, and higher bits (set by other clients) are ignored, so a stored 11 is flag 3.
- N is 0 (no flag), 1 red, 2 orange, 3 green, 4 blue, 5 pink, 6 turquoise, 7 purple. G8 reads it (`flag:+3` and `flag:03` are flag 3); `flag:8`, `flag:-1`, `flag:` and `flag:red` are `invalid_flag`.

Scenario: `search/state/flags`.

## S7. Cards buried before a day change

The first day query of a new day unburies (`sched/day-clock` D5, D7). Changed 2026-11-21 (implementation review): Meguri's search requests (`query-model.md` Q8) compute today with the day calculation alone and run no day query (`sched/day-clock` D8.1, `query-model.md` Q8), so a search never unburies, whether through S2 or through the day it reads; the unbury belongs to the day clock's other callers (D8). Anki's search reads the day without that step, and only its other paths (the `timing` observation among them) unbury. So a scenario that crosses a day boundary with buried cards lists `timing` before `search` in its first observe step after the boundary, as `search/state/due` does; the unburied review card then counts as due. The history scenarios follow the same order (`history.md` H11).

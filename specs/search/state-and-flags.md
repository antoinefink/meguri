# Due and buried search states

Spec: search/state-and-flags
Requirements: R1062, R1063, R1916
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/options/prefs-rollover.yaml

This prerequisite specifies only `is:due` and `is:buried`. Other states and flags remain U82 work. Parsing, boolean operations and result modes follow `search/grammar` and `search/query-model`.

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

## S2. Buried predicate

`is:buried` is true exactly when stored `queue` is -2 (sibling burial) or -3 (manual burial). It is false for every other queue, including -1 (suspended). The result does not depend on `type`, `due`, `odue`, `did` or `odid`. Reading it never unburies cards or performs day maintenance.

## S3. Context and arithmetic

The engine supplies the query context (`search/query-model#Q8`). Search reads no clock, config, scheduler, collection timing service or engine callback.

| Leaf | Declared context dependencies |
|---|---|
| `is:due` | now, today, learn-ahead seconds |
| `is:buried` | none |

Declare all three due dependencies even when no currently stored card uses one of the queue groups. Neither predicate reads next-day-at. Groups and boolean operators add no dependencies; the plan records the union of its leaves' dependencies (Q9).

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

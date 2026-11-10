# Search: history terms

Spec: search/history
Requirements: R1069, R1072, R1073
Anki: 26.09.3
Scenarios: conformance/scenarios/search/history/

What `added:`, `edited:`, `introduced:`, `rated:`, `resched:`, `prop:rated` and `prop:resched` match. Their values are `grammar.md` G8 and G8.1 (days, answer buttons and their errors). Every rule here was checked against the oracle; the scenarios pin the examples, including the days on which the UTC offset changes.

## H1. Terms

| Term | Scope (`query-model.md` Q3) | Reads |
|---|---|---|
| `added:N` | card | the card id |
| `edited:N` | note | `notes.mod` |
| `introduced:N` | card | the card's review log |
| `rated:N`, `rated:N:B` | card | the card's review log |
| `resched:N` | card | the card's review log |
| `prop:rated<op>D`, `prop:rated<op>D:B` | card | the card's review log |
| `prop:resched<op>D` | card | the card's review log |

The review log is the `revlog` table: `id` (the instant of the entry in epoch milliseconds), `cid` (the card), `ease` (the button: 1–4 for an answer, 0 for an entry the user or the scheduler wrote without an answer: Set Due Date, Forget, rescheduling, `specs/sched/` U44, U48). An entry with `ease` 0 is a **manual entry**. Another client can write any integer in `ease`, so the two other kinds are defined on the whole range: for `rated:N` and `prop:rated` without `:B`, an **answer** is an entry with `ease` > 0 (5 and above count, negative values do not); for `introduced:N`, a **first-answer candidate** is an entry with `ease` ≠ 0 (negative values count). Entries are matched by `cid` only, whatever their `type` (`search/history/odd-ease`).

## H2. Days

Every term counts days back from the search context's next-day-at (`query-model.md` Q8; `sched/day-clock` D3–D6, U40), never from local midnight or from the local start of the day:

- cutoff(N) = next-day-at − 86400 × N, in epoch seconds (64-bit), and cutoff_ms(N) = cutoff(N) × 1000.
- N comes from G8: `added:`, `edited:`, `introduced:` and `rated:` raise 0 to 1 (`added:0` is `added:1`); `resched:` keeps 0. There is no upper limit (`rated:4294967295` reaches back to before any review).

A day is therefore always 86400 s long here, even when the local day is not. On a day when the zone changes its offset, the window of `N` days starts at a different local time than the local rollover:

- Europe/Paris, rollover 4, the 23-hour day of 2030-03-30/31 (clocks jump at 02:00 +01:00): one second before the boundary 2030-03-31 04:00 +02:00 (02:00 UTC), cutoff(1) is 2030-03-30 02:00 UTC, which is 03:00 +01:00, an hour before that day's local rollover. An answer at 03:30 +01:00 on 03-30, a card added then and an edit at 03:00:01 +01:00 count for `rated:1`, `added:1` and `edited:1`; an answer at 02:59:59 +01:00 does not, and `prop:rated=-1` finds it.
- America/New_York, rollover 4, the 25-hour day of 2030-11-02/03 (clocks fall back at 02:00 −04:00): one second before the boundary 2030-11-03 04:00 −05:00 (09:00 UTC), cutoff(1) is 2030-11-02 09:00 UTC, which is 05:00 −04:00, an hour after that day's local rollover. An answer at 04:30 −04:00 on 11-02 does not count for `rated:1` (`prop:rated=-1` finds it); one at 05:00:00 −04:00 does.

Scenarios: `search/history/dst-paris`, `search/history/dst-new-york`. Both start the collection after the offset change and only observe after it; Meguri's day clock and Anki's cached day agree there (`sched/day-clock` D6).

## H3. `added:N` (R1072)

The card matches when its id is greater than cutoff_ms(N). A card's id is the instant it was created in milliseconds (`storage/ids-usn-mtime#I2`), so a card created exactly at the boundary instant, with id cutoff_ms(N), is not "added" in those N days, while one created a second later is. In notes mode a note matches when one of its cards does.

## H4. `edited:N` (R1072)

The note matches when `notes.mod` (epoch seconds) is greater than cutoff(N). A note saved exactly at the boundary instant does not match; one saved a second later does.

## H5. `rated:N` and `rated:N:B` (R1072, R1073)

The card matches when it has at least one review-log entry whose `id` is at least cutoff_ms(N) (an entry exactly at the boundary counts) and which is an answer (H1: `ease` > 0) without B, or has `ease` = B with B. Manual entries never count. Entries after now count too (a log written by another device with a later clock).

## H6. `resched:N` (R1072)

The card matches when it has at least one manual entry (`ease` 0) whose `id` is at least cutoff_ms(N). Answers never count. `resched:0` therefore looks only at entries from next-day-at on, and normally matches nothing.

## H7. `introduced:N` (R1072, R1073)

The card matches when it has at least one first-answer candidate (H1: `ease` ≠ 0), and the smallest `id` among those entries is greater than cutoff_ms(N) (an entry exactly at the boundary is not after it). Manual entries are ignored entirely: a card whose only entries are manual is not introduced, and a manual entry before the first answer does not hold the card back. An entry with a negative `ease` counts here, though it is not an answer for H5. The term is never unknown, so `-introduced:N` finds every card it does not find: new cards, cards first answered earlier, cards with only manual entries.

## H8. `prop:rated` and `prop:resched` (R1069)

D (G8.1: at most 0) names a day relative to today: 0 is today, −1 yesterday. With start(D) = (next-day-at + 86400 × (D − 1)) × 1000 and end(D) = (next-day-at + 86400 × D) × 1000, both in milliseconds, the card matches when it has at least one entry e of the right kind (`prop:rated` without a button: an answer, H1 `ease` > 0; with `:B`: `ease` = B; `prop:resched`: a manual entry) such that:

| Operator | e.id |
|---|---|
| `=` | start(D) ≤ e.id < end(D) |
| `!=` | e.id < start(D) or e.id ≥ end(D) |
| `<` | e.id < start(D) |
| `<=` | e.id < end(D) |
| `>` | e.id ≥ end(D) |
| `>=` | e.id ≥ start(D) |

- `!=` asks for an entry outside day D, not for the absence of one: a card answered today and two days ago matches both `prop:rated=0` and `prop:rated!=0`.
- `prop:rated=0` and `prop:rated>-1` count today's answers; `prop:rated>-1` also counts answers after today.
- `prop:rated=1` and `prop:resched=1` are `invalid_negative_whole_number`; `prop:rated=-7:5` is `invalid_answer_button` (G8.1).

## H9. Example

UTC, rollover 4: day 0 starts 2030-01-15 04:00 (B0), day 1 at B1 = 2030-01-16 04:00, day 2 at B2, day 3 at B3. Searches made on day 2 (`search/history/answers`):

| Card | Review log | `rated:1` | `introduced:1` | `resched:1` | `prop:rated=0` | `prop:rated!=0` |
|---|---|---|---|---|---|---|
| c1 | answer at B2 | yes | no (not after B2) | no | yes | no |
| c2 | answer 1 ms before B2 | no | no | no | no | yes |
| c3 | manual at B2 + 5 s | no | no | yes | no | no |
| c4 | Hard on day 1 | no | no | no | no | yes |
| c5 | answers on day 0 and day 2 | yes | no | no | yes | yes |
| c6 | first answer (Again) on day 2 | yes | yes | no | yes | no |
| c7 | manual on day 0, first answer (Easy) on day 2 | yes | yes | no | yes | no |
| c10 | answer and manual entry on day 3 (after now) | yes | yes | yes | no | yes |

At B3 − 1 s nothing changes; at B3 the window moves a day: `rated:1` keeps only c10, `prop:rated=-1` finds c1, c5, c6 and c7.

`search/history/added-edited` pins H3 and H4 on the boundary instants (cards added one second before, at and one second after B1; notes edited at and one second after B2).

## H10. Plan

- Context read: next-day-at (`query-model.md` Q8). These terms do not read now or today.
- Not slow: they compare columns or test the existence of review-log rows. Meguri may use subqueries on `revlog` (it has an index on `cid`) or resolve card id sets when it builds the plan, with the same result.
- No errors beyond G8 and G8.1.

## H11. Scenarios

| Scenario | Sections |
|---|---|
| `search/history/answers` | H5, H6, H7, H8, H9 |
| `search/history/added-edited` | H3, H4 |
| `search/history/odd-ease` | H1, H5, H6, H7, H8 (`ease` outside 0–4, any `type`) |
| `search/history/dst-paris` | H2, H3, H4, H5, H8; `properties.md` P4; `state-and-flags.md` S1, S7 |
| `search/history/dst-new-york` | H2, H3, H4, H5, H8; `properties.md` P4 |

All carry the tag `needs-history`. They write review-log rows and `notes.mod` with `db_exec`, add notes at set clocks, and observe one second before and at a day boundary. Each observe step after a clock change lists `timing` first (`state-and-flags.md` S7 explains why that order matters around a day change).

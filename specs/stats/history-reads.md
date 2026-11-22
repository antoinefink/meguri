# Statistics page: history reads

Spec: stats/history-reads
Requirements: R1289, R1291, R1292, R1294, R1295, R1446 (for the Anki parts it extends), R1837, R1838, R2067 to R2075; Meguri reads for round 11 (`docs/design/decisions.md`, 2026-11-22, `stats.overview`)
Anki: none for H2 to H5, H9 to H12 (Meguri statistics with no Anki counterpart; tests by construction, H20). The extras of H6 to H8 sit beside Anki's records and change none of their values (`stats/graphs-revlog`, `stats/graphs-cards`, R1446).
Scenarios: none; tests named in H20
Status: Approved 2026-11-22 (R1357). Approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/statistics-round11-20261122.md`); Antoine approved the round 11 design in chat on 2026-11-22 (`docs/design/decisions.md`, rows `stats.overview` of 2026-11-22).

This file specifies what the core computes for the round 11 Statistics page (`stats/statistics-page`, `docs/design/native-interface-contract.md` N25) beyond Anki's graph data: card states per day, each card's first review, an FSRS estimate of knowledge per day, retention counts per day, answer counts per day, extra ranges for two existing records, due counts split by interval, a table per deck, the strip's numbers, the activity of every year and the card lists behind one day; and how these values are kept, how progress is reported, and how fast they should be. Anki's graphs keep their own specs (`stats/graphs-common`, `stats/graphs-revlog`, `stats/graphs-cards`); every read here runs on that machinery: the request and refresh of G8, the context of G1, the row reading of G4, the column caches of G9, staleness G10 and read only G11.

Meguri's own statistics have no oracle. Where a value rests on an Anki rule, the rule is cited and not restated: young and mature at 21 days (`stats/graphs-cards` C4.1 for cards, `stats/graphs-revlog` V1 for rows), the retention row rule (`stats/graphs-revlog` V4.1), Anki's day (`stats/day-buckets` B3), daily load (`stats/graphs-cards` C3.4), memory items (`sched/fsrs-items` I3 to I5). Everything else is defined here and tested by construction (H20).

## H1. Terms

- **Page scope.** The deck filter of the Statistics page: All Decks, which is `stats/graphs-common` G2's Collection scope, or a deck `D`, which is G2.1's Deck scope (the cards whose `did` or non-zero `odid` is `D` or a descendant). A Search scope is not offered in this version (round 11, 9A); a request that names one for a part of this file gets the result `unsupported_scope` and no data for that part.
- **Existing cards.** Cards of the `cards` table. A deleted card's review-log rows (a row whose `cid` names no card, `stats/counting-rules` N1) count only where a section says so, and only in All Decks.
- **Day.** A row's day is its day index `d` of `stats/day-buckets` B3 relative to G1's `N`, and every row with `d` ≥ 1 counts on day 0, as `stats/activity-heatmap` AH3 does (R1280). Day 0 is today; day `−k` is `k` days ago. Card days (creation) are `stats/graphs-cards` C10's day `a`, also folded: `a` ≥ 1 counts on day 0.
- **Date of a day.** Day `d` falls on the calendar date `T0 + d`, with `T0` of `stats/activity-heatmap` AH3. Weeks and months group days by these dates (`stats/statistics-page` S4). The core uses this only for H3's month samples and H11's years.
- **Day end.** Under B3 the whole second `e(d) = N + d × 86,400` is the last second of day `d` ≤ −1. A value "at the end of day `d`" counts the rows with `floor(id / 1,000) ≤ e(d)`, exactly the rows B3 puts on day `d` or earlier, and is evaluated at the instant `e(d)`. Day 0 has no end yet: its value is taken at now, over every row (rows after `N` included, folded as above).
- **Window.** The history value of G3: `last12Months` reads days −366 to 0, `allHistory` every day. Each part returns its values for the window's days, and what a series needs from before the window (H2.4).
- **Period windows** (`stats/statistics-page` S3) of the parts that have no time axis: `one_month`, `three_months`, `six_months`, `one_year` and `all_time`, the rows with `t ≥ N − n × 86,400` for n = 30, 90, 182 and 365, and every row the request reads. The first, second, fourth and fifth are `stats/graphs-revlog` V5.2's ranges; `six_months` is Meguri's (H7).
- **Deck key** of an existing card: the pair (`did`, `odid`) as stored now. A page scope selects a key when `did` is in the scope's decks or `odid` is non-zero and in them, which is exactly G2.1. The **home deck** of a key is `odid` when it is not 0, else `did` (as `stats/activity-heatmap` AH4).

## H2. Cards Over Time and Started

### H2.1 Which cards

Every existing card of the page scope, except the cards whose `queue` is −1 now (suspended). Suspending writes no review-log row (`sched/manual-ops`), so the log cannot say since when a card is suspended; those cards are left out of every day, and the part returns their number (`left_out`), which the chart states. Buried cards (`queue` −2 or −3) stay in, counted by their state: burying happens in every session and the rollover undoes it, so leaving them out would rewrite the whole past twice a day. Deleted cards are not in it.

### H2.2 A card's state over time

A card enters the chart on the earlier of its creation day (H1) and the day of its first review-log row. From then on its state on a day is the state after the last of its rows dated on or before that day, by this table, applied to its rows in ascending `id` order. Before its first row it is New.

| Row (`model/row-records` R5, read as `stats/graphs-common` G4 reads it) | State after the row |
|---|---|
| a Reset marker (`type` 4, `factor` 0; `sched/fsrs-items` I2) | New |
| a cram row (`type` 3, `factor` 0) | unchanged |
| any other row with `ivl` ≥ 1 (a day or more) | Young when `ivl` < 21, Mature otherwise |
| any other row with `ease` ≥ 1 and `ivl` ≤ 0 (a step in seconds) | Learning when the state before was New or Learning, Relearning otherwise |
| any other row (`ease` 0 and `ivl` ≤ 0) | unchanged |

- From the day of its newest row on, a card is in its **current state**, `stats/graphs-cards` C4.1's type bucket (`type` 0 New, 1 Learning, 3 Relearning, 2 Young when `ivl` < 21 and Mature otherwise), whatever the table gives for that row. A card with no row is in its current state from its creation day (New for an ordinary new card). So the last day of the series equals, for the same scope, Card Counts' `including_inactive` set minus H8's `suspended_by_state` (the suspended cards by type bucket), state by state, by construction (H20).
- A card imported with its schedule and without its review log has no row: it is in its current state from its creation day (its `id`, kept by the import), not from the day it was imported, which the collection does not record.
- A row dated before the card's creation counts on its own day: the card enters then.
- This table is Meguri's. It reads the interval the row gave and nothing about the card's queue at the time. It can differ from what Anki's Card Counts would have shown that day for rows other clients write in unusual ways (for example a review row with `ivl` 0); the current state of the last day cannot.

### H2.3 Started

A card's **first review** is its oldest graded row (`sched/fsrs-items` I2: `ease` ≥ 1 and not a cram row), whatever came after (a Reset does not start it again). Started counts, per day, the cards of the page scope whose first review is on that day. Unlike H2.1 it keeps suspended and buried cards, so that Started can be read beside Added (`stats/graphs-cards` C10), which counts every existing card. Deleted cards are not counted.

### H2.4 Result

Part `card_states_over_time`:

| Member | Value |
|---|---|
| `start` | the five counts (New, Learning, Relearning, Young, Mature) at the end of the day before the window's first day; zeros with `allHistory` |
| `deltas` | sparse map day → the change of each of the five counts on that day (end of day minus end of the day before), only days with a change |
| `left_out` | H2.1's number of suspended cards |
| `first_day` | the oldest day any card of the series enters, or none |

Part `started`: sparse map day → cards first reviewed that day, the window's days only, and `first_day`, the oldest such day in all history (or none).

The value of a state on day `d` is `start` plus the deltas of the days up to `d`. The core returns changes, not one total per day, so a part is at most one entry per day that changed and every deck key's series adds up to the scope's (H14).

## H3. Knowledge Over Time

### H3.1 Value

For an instant τ, the estimated knowledge of the page scope is the sum, over its existing cards (H2.1's cards: suspended ones left out, as in H2), of the card's chance of recall at τ, R(τ), with each R(τ) kept in [0, 1] (a value below 0 counts 0, above 1 counts 1, and a NaN counts 0). The count of **studied** cards at τ is the number of those cards that give an item at τ (step 1), so every studied card has an estimate and the gap between the two lines is what the estimate says was forgotten; it can be lower than Started's running total (H2.3), for example for cards whose history start moved after a Reset or a cutoff. The sample instants are H1's day ends (H3.2).

R(τ) of a card:

1. Its rows with `floor(id / 1,000)` ≤ τ (H1's day-end cut; for now, every row) give a memory item by `sched/fsrs-items` I3 to I5 in the memory form, as if τ were now: the "card's very last row" of I3 rule 1 is its last row at or before τ, and I4's days are counted back from G1's `N` (only their differences matter). The item is built with the card's home preset as it is today: its parameters (`sched/presets` V6) and its historical retention (I1.4); its cutoff (`sched/fsrs-items` I1.3) applies only to samples at or after the cutoff instant, and an earlier sample is built with no cutoff, so the history before a cutoff date does not drop to nothing (the line can step at that date). A card that gives no item at τ has no state there, adds 0 and is not studied at τ.
2. Its memory state at τ is the state of that item (I5), and its last review instant L is the instant of the item's newest kept row.
3. R(τ) = (x ÷ S × c + 1)^(−decay), with x = (τ − L) ÷ 86,400, c = 0.9^(1 ÷ −decay) − 1, S the state's stability and decay the home preset's as `sched/presets` V6 gives it, in binary64: `stats/card-info` C7.4's curve.

- So a sample uses only what had happened by τ. A review later than τ never changes the past samples; only today's parameters, cutoff and decay, which apply to every sample, and a row inserted before τ (from another device) do. Where a card's history start does not move between τ and now (no Reset marker and no new learning run after τ, and not I3 rule 1's case of a learning start that was the card's last row before the cutoff), its state at τ, when its newest row at or before τ is a kept row (I3), is the state `stats/card-info` C6.3 gives that kept row: what Card Info shows for it. C6.3 gives a row that is not kept the next kept row's state, so the claim and its test cover kept rows only (H20).
- A crate error for a card's item at a sample leaves that card out of that sample and every later one (it adds 0 and is not studied there) and adds it to `failed`, never failing the read.
- **Today's point** is the same replay at now, over the same cards, so the line has one rule from end to end. It is computed for every request at that request's now, from each card's record (its last memory state and instant, H14.1), never kept: only the past day-end and month-end samples are kept sums. It differs from the Retrievability chart's Estimated total knowledge (`stats/graphs-cards` C9.4's `sum_by_card`), which sums the stored memory states of every card with one (suspended, buried and new cards included), with each card's own `decay`, in f32 and with C9.1's elapsed time. The two figures are labelled differently (`stats/statistics-page` S7.1, S7.12) and the chart's page says why they can differ.
- **No estimate.** The replay uses each home preset's parameters whatever the collection's `fsrs` key says (Meguri keeps FSRS on, N18.1). When no card of the scope gives an item at any sample, the part returns no samples and the chart shows `stats/statistics-page` S8.5's line "No cards with a memory estimate".

### H3.2 Samples

| History | Samples |
|---|---|
| `last12Months` | the end of each day from −365 to −1, then now |
| `allHistory` | the same, and before day −365 the end of the last day of each calendar month (by H1's dates), back to the month of H9's `oldest_day` |

The page picks the samples its bars end on (`stats/statistics-page` S4) in Swift, with no core call within the year: one a day for 1M, the week's last day for 3M, 6M and 1Y, the month's last day for All, and now for the current bar.

### H3.3 Result

Part `knowledge_over_time`: for each sample, its instant, the knowledge (binary64) and the studied count; `failed`, the number of cards H3.1 left out for a crate error; `left_out` as H2.4.

### H3.4 Desired retention

The dashed line of the Retention chart is today's desired retention, not a history (none is stored). For a span of days it is the review-weighted average of the retention in force (`sched/presets` V7) for the home decks of the counted rows' cards: the sum of H4.2's `desired_sum` over those days divided by the sum of their `desired_rows`. With no counted row in the span, it is the mean of the distinct retentions in force among the home decks of the scope's existing cards. The `retention_by_day` part (H4.2) carries those distinct values and `several`, true when there is more than one, so the callout can say the line is an average.

## H4. Retention and Forgotten per day

### H4.1 Rows

The rows of `stats/graphs-revlog` V4.1, the retention table's rule, with its young and mature split (`lastIvl` < 21) and its pass and fail (`ease` 1 is a fail, any other rating a pass). This is the page's one meaning of "remembered" (`stats/statistics-page` S2): a review of a card whose last interval was a day or more, or any review row, answered with anything but Again, not cramming. A card with more than one counted row on a day counts each (R1408).

- Page scope as H1: in All Decks deleted cards' rows count, as in the Retention table.
- Days as H1, so a row dated after `N` counts on day 0. The By Period table keeps V4.2's periods, which leave such a row out, and V4.2's edges differ from day bounds by one second (`stats/graphs-revlog` V7): the table and the sum of the chart's last days can differ at those edges. Accepted; both are stated.

### H4.2 Result

Part `retention_by_day`: sparse map day → `{young_passed, young_failed, mature_passed, mature_failed, desired_sum, desired_rows}` for the window's days, where `desired_sum` is the retention in force (H3.4) of each counted row's card, summed, and `desired_rows` the number of those rows; a deleted card's row has no home deck and is in neither. Forgotten on a day is `young_failed + mature_failed`. The part also carries H3.4's distinct retentions in force and `several`.

## H5. The parts a page refresh asks for, by tier

| Tier (`stats/statistics-page` S10) | Parts |
|---|---|
| 1, present state | `card_counts` (C4), `future_due` with H8 (C3), `intervals` (C5, for the median interval only), `stability` (C7), `difficulty` (C8), `retrievability` (C9), `studied_today` (H12) |
| 2, review log | `reviews` with H6 (V2), `retention` (V4) and `retention_by_day` (H4), `hours` and `buttons` with H7 (V5, V6), `added` (C10), `card_states_over_time` and `started` (H2), `by_deck` (H10), `activity_days` (H11) |
| 3, knowledge | `knowledge_over_time` (H3) |

- `calendar` (V3) and `today` (`stats/studied-today` T6.2) are not asked for: the Activity grid (H11) and the strip (H12) replace them on the page.
- Every Data result carries H9's `oldest_day`.

## H6. Answers per day (Reviews Over Time › By Answer)

A Meguri extra in the `reviews` record (`stats/graphs-revlog` V2.2), computed in the same pass, not observed (V8):

- `count_by_answer` and `time_by_answer`: sparse maps day index → four values, for the rows V2 counts that have `ease` 1, 2, 3 or 4 (Again, Hard, Good, Easy): their number and their summed `time` (64 bits, as V2's time). The day index is V2's (B3, unfolded), so a bar's By Answer and By Type are on the same days.
- Rows V2 counts with another `ease` (0, or 5 to 255, written only by other clients) are in By Type and not in By Answer; the callout's total is By Type's, and the chart's page says so when such rows exist (`unrated`, the number of them in the window, is in the extra).

## H7. Answers and Time of Day ranges

Meguri extras in the `hours` and `buttons` records (`stats/graphs-revlog` V5.3, V6.2), not observed:

- A `six_months` range, with the rows `t ≥ N − 182 × 86,400`, beside V5.2's four, in both records; in the record whatever the history (it lies inside the window).
- `retention_by_hour`: for each of the five ranges, 24 entries hour 0 to 23 (V5.1's hour) of `{passed, failed}`, counting the rows of H4.1 (the retention rule, which is not V5's `correct`: V5 counts Learn, Review and Relearn rows and calls `ease` above 1 correct). `all_time` only with `allHistory`, as V5.2.

## H8. Reviews due, young and mature; suspended cards by state

- A Meguri extra in the `card_counts` record (`stats/graphs-cards` C4.4), not observed: `suspended_by_state`, the scope's cards with `queue` −1 counted by C4.1's type bucket (New, Learning, Relearning, Young, Mature). H2's last day equals `including_inactive` minus it (H2.2).
- A Meguri extra in the `future_due` record (`stats/graphs-cards` C3.6), not observed: `future_due_young`, the same map as `future_due` (k → cards, after C3.3) counting only the cards with `ivl` < 21 (learning and relearning cards included, whose `ivl` is that of their last review interval, often 0). Mature on k is `future_due` minus `future_due_young`. The daily load, due tomorrow and the backlog stay C3's.

## H9. Oldest day

Every Data result of a page request carries `oldest_day`: the smallest day (H1, folded) of a review-log row of the page scope with `id` ≥ 0, whatever the window, or none. With `last12Months` it is still the scope's oldest, so the All axis is known from the first result (`stats/statistics-page` S10.3). Read from G9's columns (rows sorted by `id`: the first row in scope).

## H10. By Deck

### H10.1 Rows

- All Decks: one row per top-level normal deck (filtered decks are not rows). A deck `D`: one row per direct normal subdeck of `D`, and first a row for `D` itself when `D` has cards whose home deck is `D` (its own cards, outside its subdecks).
- A row's cards are the existing cards whose home deck (H1) is the row's deck or, except for the own-cards row, a descendant of it. So every existing card of the scope's normal decks is in exactly one row, a card in a filtered deck in its home deck's row, and the rows' values add up. This is the table's own rule: the page's other parts follow G2.1, under which a card in a filtered deck also counts in the filtered deck's subtree. They differ only for cards in filtered decks.
- Deleted cards' rows are in no row, nor are cards whose home deck names no deck (damaged data; Check Database repairs it): "exactly one row" holds for cards whose home deck exists.
- A top-level or child deck with no cards and no reviews in any window is not a row.

### H10.2 Values

For each row and each period window (H1):

| Member | Value |
|---|---|
| `cards` | its existing cards, suspended and buried included (the window plays no part) |
| `reviews` | the number of its cards' rows that `stats/graphs-revlog` V2 counts, in the window |
| `time_ms` | their summed `time` (64 bits) |
| `passed`, `failed` | its cards' rows of H4.1 in the window |

Part `by_deck`: the rows in the deck tree's order (`sched/counts` C1), each with the deck id, and per window the values above; `all_time` only with `allHistory`. The share of time and the retention are the page's (`stats/statistics-page` S7.15).

## H11. Activity, every year

Part `activity_days`: the counts of `stats/activity-heatmap` AH2's rows per day (H1 days, folded at 0 as AH3 does), for every day from the scope's oldest counted row with `id` ≥ 0 (G3) to today, sparse (days with a count only), for the page scope (H1: G2.1 and, in All Decks, deleted cards' rows, as AH4's Collection scope). It reads all history whatever the window, because the grid shows every year (`stats/statistics-page` S7.5). Streaks, days studied and year totals are derived in Swift from it (`stats/statistics-page` S7.5).

- The deck page's heatmap keeps AH4's home-deck rule. For a deck with cards in filtered decks, its deck page grid and its Statistics grid can differ; accepted.

## H12. Strip numbers

Part `studied_today`: `stats/studied-today` T12.1's `answers`, `cards` and `time_ms` for the page scope instead of the whole collection (T12's rows: in the studied-today window, `type` neither 4 nor 5; deleted cards' rows in All Decks only). It reads the stored `type` and `time` as T12.1 and `stats/counting-rules` N6 do, from the snapshot by the review log's `id` range, not G9's columns (whose G4 reading of odd values differs), so for All Decks it equals T12.1 on every row. The other strip figures come from parts already listed: Retention Today from V4's `today` period (`retention`), Remaining Today from the deck list counts the app holds (`sched/counts` C5), its "tomorrow" from C3.6's due tomorrow, Mature from Card Counts, the streak from H11 (`stats/statistics-page` S6).

## H13. Cards behind a day

A read for Show in Browse, run on a click, not with the page: `stats_day_cards(scope, day, kind)` returns the ids of the existing cards of the page scope that have a row of that kind on that day (H1 days), in ascending id order, or `none` when there are none.

| `kind` | Rows |
|---|---|
| `reviewed` | the rows `stats/graphs-revlog` V2 counts (Reviews Over Time) |
| `answered` | AH2's rows (Activity) |
| `forgotten` | H4.1's rows with `ease` 1 (Forgotten) |

It runs on the query lane with a new snapshot and the G1 context of the result on screen (`N`, now and the zone from its context token, G8.3), so a commit since that result (a background sync) never refuses it: it lists the cards as they are now on the day the user selected. Cards deleted since are not listed. A deleted card's rows, which a bar of All Decks counts, have no card to list. Browse gets the ids as `search/join-and-replace` J3's card-ids item (`stats/browse-queries` Q9).

## H14. Keeping the history

### H14.1 What is kept

The replay behind H2 and H3 is kept in a derived-store component, `stats_history` (`storage/derived-store` D4), beside `stats_columns` (G9), under the same D1 rules: never synced, backed up or exported; deleting it loses nothing. It holds:

- one small record per existing card: its deck key, its current inactive flag, the state the H2.2 table left it in and the id of its newest row, its first review day (H2.3), and what H3.1 needs to continue its replay (its item's start and the last memory state with its instant). It keeps no per-sample value: a change that takes a card's contribution out replays the card from its rows first;
- per deck key, per day: H2's deltas and H2.3's started counts, as day-keyed changes;
- per deck key, per H3.2 sample day: the knowledge sum and the studied count.

A page scope's series is the sum of the series of the deck keys it selects (H1), never a new replay. Rows are kept on their own absolute day, never folded: the fold of H1 (a row after `N` on day 0) is applied when a request reads, against that request's `N`, so a rollover moves nothing kept. The days are kept on an absolute grid fixed by `N` modulo 86,400 (the phase of the day boundary), which the component keeps in a record row of its own with the samples' latest day; the store's stamp (`storage/derived-store` D3) gives the generation and seq. A request whose `N` has another phase finds the component not current (H14.3).

H4, H6 to H13 are computed per request from G9's columns and the card columns, like Anki's graphs: one pass over the window's rows, with no kept aggregate of their own.

### H14.2 Built when first needed

The component registers with `deferred until` set to a host signal no host sends (`statsHistoryWanted`), so a rebuild verdict at bind (D5.2) or an invalidation is held until a request calls `request_refresh("stats_history")` (D4); it has no catch-up (its content depends on `cards.queue`, which the rollover's `Raw` unbury changes, D4's last bullet), so a refresh is a rebuild. It is built when a request first needs it and it is not current (`storage/derived-store` D8), off the main thread on the maintenance reader at utility QoS: one pass over the review log in `cid`, `id` order (`sched/fsrs-items` I1.1's bulk read), then the per-key sums. A build at another time, such as after Home has been idle (G9.2's `homeIdle`), is an allowed optimisation, not a requirement (decided 2026-11-22). Until it is current, a request that needs it reports progress (H15) and its other parts return as soon as they are ready.

### H14.3 Updates

Each committed change set (`engine/change-stream` S1) updates the component, with work proportional to the cards it touches. The component's capture, on the collection writer right after the commit, reads by `cid` (an indexed read, as `stats/graphs-common` G9.4 attaches rows) the rows that still exist of every card a replay below needs, so the replay sees exactly that commit's rows; the rows the commit removed, and a removed card's row before the commit, come from the observer payload's before-images (`engine/change-stream` S2: the review-log rows removed and each changed card's row before and after); the apply writes the result on the derived writer (D6). A "replay" re-derives the card's whole contribution from its rows: the H2 deltas and Started on the rows' own days and the H3 samples at and after its earliest changed row.

| Change | Effect |
|---|---|
| `revlog_added` rows all newer than their card's newest cached row | the card's replay continues from its record: the segment of its former newest row, which H2.2 gave the current state, is recomputed with that row's table state (as a cold replay gives it, up to the day before the next row), then the deltas, and Started if one is a first review, on the new rows' own days (a sync after a rollover can bring yesterday's reviews), and every kept sample at or after the earliest new row's instant recomputed for that card; a new Reset marker or a new learning run replays the card (its item's start moves) |
| any other `revlog_added` (a row older than the card's newest, as a sync can bring), `revlog_removed` (an undo, a sync) | the card is replayed again from its rows (an indexed read by `cid`): its old contribution is taken out of every day and sample after the earliest changed row, the new one added |
| `cards` `Added`, `Removed` | `Added`: the card is replayed from its rows, read by `cid` (this covers the undo of a delete and a `CloudRekey`, which shows as `Removed` then `Added`); `Removed`: its contribution, replayed from its rows (read by `cid` in the capture, as they were before the commit), is taken out |
| `cards` `Updated` that changes `did`, `odid`, `queue` to or from −1, or `type` or `ivl` (its current state) | the card is replayed: its contribution moves to its new key, leaves or enters the series, or its last segment changes |
| a preset change of parameters, decay, cutoff or historical retention; a deck's preset change | the affected cards replayed in the background with the new values: their records (the last memory state and instant that today's point continues from) and their H3 samples; H2 unchanged (a desired-retention change touches neither) |
| a different phase of `N` (a rollover hour change, a time-zone or daylight-saving offset change) | the component is not current; rebuilt (H14.2) on the next request: a cold pass, with progress, twice a year in a zone with daylight saving |
| `CollectionReplaced`, Check Database, an external change | invalidated (`storage/derived-store` D10) and rebuilt on the next request |
| one or more new days (`RolledOver`, or an open after days away) | the next request adds every missing day-end (and month-end) sample since the latest kept one: for a card with no row after a sample's instant from its record, for a card with rows after it by replaying the card |

- After a sync only the new rows are read: the cards they touch are continued or replayed, never the whole log.
- A change set that touches more than a tenth of the cards may be applied as a rebuild instead.
- At suspend and close the component persists as D11 says; a fresh component at the next open is read without a replay.

### H14.4 Contract

- **Equivalence.** Every value equals a cold computation of H2 and H3 for the request's snapshot and context, after any sequence of the changes above, including changes landing during a build and a close and reopen. Knowledge sums compare within 1e-9 relative and 1e-6 cards absolute (incremental sums of binary64 terms); counts exactly. Property test (H20).
- **Never stale.** Kept values of another stamp, phase or generation are never returned.
- **Memory.** On disk and resident, the component stays under 32 MB at 150,000 cards and 1,200,000 rows (H19; to be measured). On a memory warning it is released as G9.5 releases `stats_columns` (persist, then unmap); the next request reloads it.

## H15. Progress and partial readiness

- A page request lists the parts it wants; the core returns each part as soon as it is ready, in the order of its tier (`stats/statistics-page` S10: the present state first, then the review log, then knowledge), each as its own Data message for the request, with the context token of the first. The request ends with a final message once every part has returned or failed.
- While a part waits on work longer than a moment (the component's build, H14.2; a knowledge recompute; a review-log read of all history), the core sends progress for that part at most every 100 ms: the fraction done, as rows read over rows to read for the log pass and cards replayed over cards for knowledge, or indeterminate when it cannot be known. The page shows it in that part's boxes (`stats/statistics-page` S10.3).
- Before the first part returns, the request checks the scope and the window for G4's unreadable rows (G9.1's lists make it cheap) and for a scope error; either fails every part alike, as G8.3 says, so the page never shows boxes and then an error for the window.
- H2, H3, H9 and H11 read the scope's rows outside the window too. An unreadable row there (G4's rules) fails only those parts, with `unreadable_row`; the windowed parts still return (`stats/statistics-page` S12.8: those boxes show their line).
- Any other part's failure (`unsupported_scope`, a crate error that stops the whole part) is reported for that part; the others still return.

## H16. Cancellation

Every request carries a cancel token (KTD9). A newer refresh, a deck change, a period change to or from All, or leaving the page cancels the superseded request (a period change among 1M to 1Y keeps it, `stats/statistics-page` S10.5): its parts not yet returned are dropped and its reader is interrupted within 50 ms. A build of `stats_history` started by a cancelled request is not cancelled with it: it continues at utility QoS, so the next request finds it further on, and stops only where `storage/derived-store` D7 stops a refresh (a close, a suspend, a replacement, an invalidation, an exclusive phase). A memory warning releases the component (H14.4) once that build has committed. Nothing a cancelled request computed is kept, except the component's own build.

## H17. Read only

Every read here, the component's builds and updates included, writes nothing to `collection.anki2` (`stats/graphs-common` G11, R1295): no row, `usn`, `mod`, `col.mod` or config key. None of them reads the day clock other than through G1, and no part writes a preference: the page writes none of G7's keys (`stats/statistics-page` S3.4). Scenario `stats/graphs/scope/read-only`'s rule applies; H20 adds a dump comparison around every part.

## H18. Errors

| Case | Result |
|---|---|
| a deck id that names no deck | G8.2's `unknown_deck` for the request |
| a Search scope for a part of this file | `unsupported_scope` for that part |
| an unreadable row of the scope in the window (G4) | `unreadable_row`, every part |
| an unreadable row of the scope outside the window | `unreadable_row` for H2, H3, H9 and H11 only; the Data results then carry no `oldest_day`, and the page's All axis starts at the window's first month |
| a crate error replaying one card (H3.1) | that card in `failed`; the part returns |
| the component cannot be built (disk full, a failed write) | the parts compute from the snapshot directly, without keeping (as G9.2 allows), and the failure is logged |

## H19. Speed (the 150k fixture, `tools/mg fixtures 150k`: 150,000 cards, 1,200,000 review-log rows)

Targets to measure, on the reference devices of R1802 in a Release build, not gates that shape the design (decided 2026-11-22): the page tiers its reads, keeps its results and shows honest progress (`stats/statistics-page` S10), so a slow read is never a blank or frozen page. Each row names its benchmark and its Product Contract requirement; the numbers are estimates until the unit that builds the read measures them and records the result. Changed 2026-11-22: the plan owner assigned R2067 to R2075 to the new rows (the parent plan's Performance requirements, "the Statistics page"); before, this sentence left them to assign.

| Read | Target | Where measured | Requirement |
|---|---|---|---|
| Present-state parts (Card Counts, Reviews Due with H8, Retrievability, Stability, Difficulty, Intervals' median, the strip's H12) for any page scope | well under 100 ms together (R1290's 50 ms for graphs is the aim) | `bench stats_present` | R2067 |
| Review-log parts for `last12Months` with G9's columns current (Reviews with H6, Retention with H4, Answers and Time of Day with H7, Added, H9, H10, H11) | under 300 ms together (R1838) | `bench stats_log_year` | R1838 |
| The same for `allHistory` | under 1 s | `bench stats_log_all` | R2068 |
| H2 and H3 parts with `stats_history` current, any page scope and history | under 50 ms (sums over deck keys) | `bench stats_history_warm` | R2069 |
| Cold build of `stats_history`'s H2 replay (one pass over 1.2 million rows) | a few seconds on an iPhone; target 2 s, under 1 s on the Mac | `bench stats_history_build`, device run | R2070 |
| Cold H3 knowledge (the FSRS replay of every card and about 400 samples) | a few seconds on an iPhone after the H2 pass; target 4 s; parallel across cores | `bench stats_knowledge_build`, device run | R2071 |
| Incremental update for a change set of up to 10 cards (an answer, an undo) | under 5 ms on the derived writer | `bench stats_history_apply` | R2072 |
| Incremental update after a sync of 1,000 rows | under 100 ms | `bench stats_history_apply` | R2072 |
| H13 card ids for a day | under 50 ms | `bench stats_day_cards` | R2073 |
| Cancellation | the reader interrupted within 50 ms (H16) | core test | R2074 |
| Memory | `stats_history` under 32 MB on disk and resident (H14.4) | bench peak memory | R2075 |

The main thread never waits on any of these (R1291): the page stays scrollable and its controls respond throughout (`stats/statistics-page` S19).

## H20. Tests

There is no oracle for H2 to H4 and H9 to H13: these tests check them by construction, in `core/crates/meguri-stats/tests/` (one file per owning unit, each listed in `tests/main.rs`) unless named.

- **H2 against Card Counts.** For random collections (the fixture generator's profiles at 10k, with random suspends, buries, resets, Set Due Date rows, filtered decks and imported cards without logs): the last day of `card_states_over_time` equals Card Counts' `including_inactive` minus `suspended_by_state` (H8), state by state, for every page scope, and `left_out` equals `excluding_inactive`'s `suspended`; burying and the rollover's unbury change no past day.
- **H2 table.** One card per row of H2.2's table and its sequences: New → Learning → Young → Mature → Relearning → Mature; a Reset back to New; a cram row changing nothing; a review row with `ivl` 0 after Mature giving Relearning; a learning row with `ivl` −600 after New giving Learning; a card with no row; a row before the card's creation; two rows on one day (the later decides).
- **H2 deltas.** Summing `deltas` over a window from `start` gives the per-day totals of a direct count for each day; `start` with `last12Months` equals the `allHistory` totals at day −367.
- **Started.** Equals, per day, a plain first graded row per card; a Reset never starts a card again; suspended cards count; cram rows never start a card.
- **H3 against Card Info.** For cards whose history start never moves (H3.1's conditions), the replayed state just after each kept row equals `stats/card-info` C6.3's state for that kept row, and R at a sample equals C7.4's curve at that instant (1e-9 relative). For a card with a Reset after τ, the sample at τ is unchanged by the Reset (causal). Samples on each side of a preset's cutoff date follow H3.1 step 1.
- **H3 bounds.** Knowledge ≤ studied at every sample (each card adds at most 1 and counts as studied exactly when it gives an item); today's point equals the replay at now; a crate error card goes to `failed` and out of that sample and every later one; a preset's parameter change recomputes only that preset's cards; a scope with no item gives no samples.
- **H4 against V4.** For every V4.2 period that the window covers, the sum of `retention_by_day` over the days whose rows fall in the period equals V4.3's counts, on a fixture with no row within one second of a day bound and no row after `N`; with such rows the difference is exactly those rows (H4.1).
- **H6.** `count_by_answer` sums to V2's count minus `unrated` per day; `time_by_answer` likewise.
- **H7.** `six_months` holds the rows at and after its bound and not the one before; `retention_by_hour` over a range sums to H4's counts over the same rows.
- **H8.** `future_due_young` + mature = `future_due` for every key; a card with `ivl` 20 is young, 21 mature.
- **H10.** The rows' `cards` add up to the scope's existing cards in normal decks; a card in a filtered deck counts once, in its home deck's row; the own-cards row shows only when the deck has own cards; `reviews`, `time_ms`, `passed`, `failed` add up to the scope's counts minus deleted cards' rows and the scope cards whose home deck is outside the tree.
- **H11.** Equals AH5's 371 counts on their days for the Collection scope and a deck without filtered cards; extends to all years.
- **H12.** Equals T12.1 for All Decks, odd stored `type` and `time` values included; for a deck, T12's rules over that deck's rows only.
- **H13.** Each kind's ids are exactly the cards a plain scan finds; an empty day gives `none`; a commit after the shown result does not refuse it.
- **H14 equivalence.** Property test over random op sequences (answers and their undo, Set Due Date, Reset, suspend and unsuspend, bury, deck moves, filtered build and empty, card deletion and its undo, imports, a sync that brings older rows, a preset parameter change, a rollover, several rollovers while closed, a sync after a rollover bringing the previous day's rows, a card re-key, a rollover-hour change, a time-zone change, a memory warning, a close and reopen): every warm result equals a cold computation, as H14.4 says.
- **H15, H16.** The first tier's parts come before the second's, the second's before the third; each reported fraction is between 0 and 1 and never decreases; after a cancel during the build no further part is sent, the build itself continues, and the reader is interrupted within 50 ms.
- **H17.** A database dump before and after every part and a build is equal.
- **H9.** `oldest_day` is the same with `last12Months` and `allHistory`, leaves out rows with a negative `id`, and follows the deck filter.
- **Benchmarks.** The rows of H19 in `core/crates/meguri-testkit/benches/` (where the collection benches live), at 150k and 500k (R1803: 2× at 500k), each adding its row to the perf results.

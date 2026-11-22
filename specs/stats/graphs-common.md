# Stats graphs: scopes, history, data and caches

Spec: stats/graphs-common
Requirements: R1281, R1282, R1283, R1285, R1289, R1290, R1291, R1292, R1293, R1294, R1295, R1349, R1350, R1351, R1352, R1353, R1355, R1356, R1357, R1360, R1367, R1446, R1838
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/graphs/scope/
Status: Under review (R1357) for the lines that begin "Changed 2026-11-22 (round 11)". Do not implement them until approved. Every other section is in force and unchanged.

The machinery every Stats-screen graph shares (U98): which cards and review-log rows a request reads (scope and history), how it reads them, what it returns, the column caches behind it, and what the Stats shell (U225) can rely on. Individual graphs are `stats/graphs-revlog` (U99) and `stats/graphs-cards` (U101); binning, ranges and text are `stats/graph-presentation` (U226); browse queries are `stats/browse-queries`. Days are `stats/day-buckets`, the Today section `stats/studied-today`.

## G1. Request context

Each request is computed for one context, taken once by the first request of its refresh and carried to the second by the context token (G8.3):

| Value | Meaning | Source |
|---|---|---|
| `N` | `next_day_at`, epoch seconds | the day clock (`sched/day-clock` D8 `nextDayAt`) |
| `today` | the scheduler's day number (`days_elapsed`) | the day clock |
| `R` | the rollover hour, 0 to 23 | the day clock's effective `rollover` (`sched/day-clock` D13) |
| now | the current instant, epoch milliseconds | the clock (U21) |
| offset | the zone's UTC offset at now, seconds east | the clock's zone and the bundled zone data (`stats/day-buckets` B5) |
| zone | the device zone's IANA id | the clock, read with now (`stats/day-buckets` B1, B5) |
| state | one collection snapshot (`engine/op-model` O6): its generation and seq | the reader the request runs on; the second request's snapshot must have the same generation and seq (G8.3) |

- Every value a request returns is computed from this one context. Rows, cards, the scope's search and the config reads all come from the same snapshot.
- Reading `nextDayAt` may run the day clock's own check (`sched/day-clock` D8), with the writes that check makes, such as `localOffset` or a rollover's unbury. Those writes are the day clock's. The graph request itself writes nothing (G11).
- Order, decided 2026-10-27: the first request reads `nextDayAt` (running that check and waiting for its commit, if any) and the clock first, and only then begins its snapshot. So the snapshot holds what the check wrote, and Card Counts never shows cards that the new day's unbury has freed as buried under the new `N`.
- The result carries `N`, `today`, `R`, the offset and the zone, so the presentation layer (U226) needs no clock of its own for day labels and calendar dates. The Calendar's dates need the zone's rules at instants other than now (`stats/graphs-revlog` V3.2).

## G2. Scopes

The scope is one of three, passed typed to the core (R1350, R1351):

| Scope | Cards | Review-log rows |
|---|---|---|
| Collection | every card of the `cards` table | every row, including deleted cards' rows (R1281) |
| Deck `D` (by id) | G2.1 | the rows of those cards (R1282) |
| Search `q` (text) | the cards the search `q` matches in cards mode (`search/query-model`) | the rows of those cards (R1282) |

- A deleted card's row is a row whose `cid` matches no card (`stats/counting-rules` N1). Only the Collection scope reads them.
- A row belongs to its card as the card is now: a card moved from deck A to deck B after its reviews brings all its rows to B's scope and takes them out of A's (R1282). Nothing about the deck at review time is kept or read.
- The Stats search field is a search field under `search/join-and-replace` J2: U225 replaces every U+001C to U+001F with U+0020 before the text reaches the core, and the browse link uses that same text (`stats/browse-queries` Q2.3). Decided 2026-10-27, so the graph's cards and the Browser's, which applies J2 to a search it is handed, stay equal (R1417). The core takes `Search(text)` as given; the replacement is the field's.
- A Search text that is empty, or that holds only characters with the Unicode White_Space property, is the Collection scope, deleted cards' rows included (`stats/studied-today` T3.2). Any other text is a search, `deck:*` included: it matches every existing card and leaves deleted cards' rows out.
- A search reads the collection's config as any search does (`search/query-model` Q8). `deck:current` typed in a Search scope reads `curDeck`. No request ever writes `curDeck`, and choosing a Deck scope never sets it (R1353).
- A search that fails to parse, or fails when it runs (`search/errors` E1, E3), gives the scope error result of G8.2. No graph data comes with it.

### G2.1 Deck scope

The cards of Deck `D` are those whose `did` is one of the scope's decks, and those whose `odid` is not 0 and is one of them (R1283). The scope's decks, read when the request runs, are `D` and every deck whose stored name starts with `D`'s stored name followed by U+001F, the names compared under simple case folding as `deck:` compares them (`search/names-and-ids` N3.3). For every ordinary name these are exactly the decks the term `deck:"<D's full name, escaped>"` matches (`stats/browse-queries` Q2.2), so a Deck scope gives the cards of Anki's `deck:` search.

- So a card in a filtered deck `F` whose home deck is `H` is in the scope of `F`, of `H`, and of every ancestor of either (scenario `filtered-deck-card`).
- `deck:` wildcards never apply: the scope of `a_b` holds `a_b` and `a_b::c`, never `axb` (scenario `subdeck-wildcards`).
- The names that `deck:"<full name, escaped>"` does not match exactly (`current` and `filtered`, which `deck:` reads as special values, and the others `stats/browse-queries` Q2.2 lists) follow the same rule: the deck and its descendants by name.
- The core takes the deck by id and finds the cards from its own card columns (G9), without running a search (R1283, R1290). The result must equal the rule above. A deck id that names no deck is the scope error `unknown_deck` (G8.2).
- Renaming, moving or deleting decks changes the scope at the next request. Deck names are never cached in the columns.

## G3. History

Changed 2026-11-22 (round 11): the Statistics page has no history control of its own: its period (`stats/statistics-page` S3.2) asks for `last12Months` for 1M to 1Y and `allHistory` for All, and its first period is All, so the default of a first open is `allHistory` (R1352's default is superseded in part). The values, windows and rules below are unchanged.

The history control has two values (R1352):

| Value | Label (U225) | Rows read |
|---|---|---|
| `last12Months` (the default) | last 12 months | rows with `id ≥ (N − 366 × 86,400) × 1,000` |
| `allHistory` | all history | rows with `id ≥ 0` |

- These are Anki's two history choices, one year (365 days of history plus today) and all.
- The window has no upper end: rows dated after now, or after `N`, are read (clock skew on another device). Each graph spec says where such a row goes.
- A row with a negative `id` (an imported row) is read by neither value. Scenario `history-window` pins the start: a row 1 ms before it is left out, a row at it is read.
- Card graphs read every card of the scope, whatever the history (R1352). Their values do not change with it (scenario `card-graphs-ignore-history`).
- The Today section reads its own rows (`stats/studied-today` T3.1), which both values contain.
- A part that covers all history is returned only with `allHistory`: the all-history buckets of Hourly Breakdown and Answer Buttons and the All time row of Retention (R1367, AE59). Reviews returns its day maps for the window; offering its "all" range only with `allHistory` is U226's.

## G4. Reading rows

Rows are read by their typed values (`model/row-records` R1), with the tolerances listed there, as Anki reads them for its graph data:

- Review log (`model/row-records` R5): `ivl` and `lastIvl` after conversion to an integer (a REAL value truncated); `time` as 0 when it is not an INTEGER from 0 to 4,294,967,295 (so a negative or huge `time` counts 0 ms in the graphs, unlike the Home line, `stats/studied-today` T2.1); `type`: a stored INTEGER whose low 8 bits are 0 to 5 reads as that kind; any other stored value (another integer, a REAL, a TEXT that INTEGER affinity left as text, NULL, a BLOB) reads as 0 (Learn). A REAL is never truncated here, unlike `ivl`. So `type` 260 reads as 4 (Manual), `type` −1, 6 and 1.5 read as Learn (scenario `odd-row-values`).
- Cards (`model/row-records` R4), with the card data of `model/card-data-json`: the memory state (`s`, `d`), `decay` and `lrt`, each absent when the data does not hold it.
- A row the request reads that `model/row-records` R10 calls unreadable gives the result `unreadable_row` (G8.2) and no graph data, as Anki's graph request fails. Every R10 case of a column read here counts; for example a review-log `ease` outside 0 to 255 or not an INTEGER, a `factor` that is not an INTEGER or lies outside 0 to 4,294,967,295, a `usn` that is not an INTEGER or lies outside the 32-bit signed range, an `ivl` or `lastIvl` that is NULL or beyond the 32-bit range after conversion, and an unreadable card row (`model/row-records` R4: a `type` of 7 or a `queue` of 9 after the low 8 bits, an `ivl` of −1; a `due` or `odue` out of range reads 0 and is readable). A `usn` of −1, the mark of a change not yet synced, is an ordinary readable value: most rows a collection holds between syncs have it.
- "The request reads" means the rows G2 and G3 select: the scope's cards and its rows in the window, whatever graphs the request names (G8.3). So a bad card row fails a request that asks only for review-log graphs, and the reverse. A bad row outside the history window, or of a card outside the scope, changes nothing (scenarios `unreadable-row` and `unreadable-card`).
- Values are never repaired or written back. Check Database is the repair path (U128).

## G5. Graphs

Changed 2026-11-22 (round 11): the Statistics page also asks for the Meguri parts of `stats/history-reads` H5 (`card_states_over_time`, `started`, `knowledge_over_time`, `retention_by_day`, `by_deck`, `activity_days`, `studied_today`), which run on this machinery with the scopes Collection and Deck only, and for the extras H6 to H9 inside Anki's records. It does not ask for `calendar` or `today`.

The graph set, in Anki's order (R1357): Today, Future Due, Calendar, Reviews, Card Counts, Intervals, Stability, Difficulty, Retrievability, Retention, Hourly Breakdown, Answer Buttons, Added.

- Card Ease is not computed or returned (R1360).
- Anki shows Stability, Difficulty and Retrievability only when the collection's `fsrs` key is true. The result carries that value (G7); what the screen shows for it is U231's.
- A request names the graphs it wants (G8.1). A graph left out costs nothing.

## G6. Aggregates

Each graph's record holds aggregates at the finest resolution, never one value per card or per row (R1293). The presentation layer bins them (U226), so a per-graph control never calls the core (R1290, R1355).

- Value maps are sparse: integer key → count, with only the keys that have data, no padding and no widening. Keys are at unit resolution: 1 day for day offsets, intervals and stability, 1 percent for difficulty and retrievability. Each graph spec gives its keys.
- A review-log row's day offset is its day index of `stats/day-buckets` B3. A card's day offsets (Added, Future Due) are the graph spec's.
- A record may also hold what its maps cannot give: scalar table inputs (average retrievability, the knowledge sums of R1403, daily load, due tomorrow, distinct notes), the retention rows, the per-range Hourly and Answer Buttons buckets, and the item queries of fixed clickable items (`stats/browse-queries` Q4). Each graph spec lists its extras; nothing else is in a record.
- A record stays within U103's limit of 256 KB at 150,000 cards and 1,200,000 rows (one card with an interval of 36,500 days included), under the conditions below.
  - Decided 2026-10-27: the limit counts the bytes of U103's encoding of the record, 262,144 at most. U103 writes each integer array as zigzag LEB128 varints (7 bits per byte: a value from −64 to 63 takes 1 byte, one of magnitude below 8,192 at most 2, below 2²⁷ at most 4) and each run of consecutive day keys as its start key followed by one value array per member, with no key per day. The maps stay sparse (a day with no data starts a new run). So the all-history Reviews record of about 74,000 numbers (`stats/graphs-revlog` V2.2) fits: about 37,000 counts of at most 2 bytes and 37,000 times of at most 4 bytes (a kind's time on one day below 2²⁷ ms, about 37 hours) make about 222,000 bytes.
  - Decided 2026-10-27, the encoding's remaining parts: every sparse map, whatever its key (day index, interval, rounded stability, difficulty or retrievability percent), is written in ascending key order as runs of consecutive keys. A map is its number of runs, then each run: its start key (the first run's as is, each later one as its distance from the previous run's last key), its length, then one value array per member with exactly that many items. Every other list (the retention rows, the Hourly and Answer Buttons buckets) is its length followed by its items. A text (the item queries of `stats/browse-queries` Q4: Card Counts labels, Calendar cells) is its UTF-8 byte length as an unsigned LEB128 varint followed by those bytes; a value array of texts is its items written so, and a member with no text (a Calendar cell with d ≥ 1) is the length 0.
  - Decided 2026-10-27, the conditions: the limit holds, and is tested, when the counted review history starts on 2006-01-01 or later, each row kind has fewer than 8,192 rows and a summed time below 2²⁷ ms on any one day, and every card's due day and interval lie within 36,500 days of today. Outside them (an imported or hand-built history back to 1970, stored times near the u32 maximum that `stats/graphs-revlog` V9 admits, corrupt due values) a record may be larger. Such a record is still returned whole: the request never fails for size, and nothing is dropped, capped or merged to fit, so the Stats screen keeps working on odd imported data.
  - Test (U99 with U103): 20 years of daily study, 7,305 consecutive days with rows of every kind on each day, 150 counted rows a day and stored times of 5,000 to 60,000 ms: the encoded `reviews` record with `allHistory` is at most 262,144 bytes and decodes to the same maps, and so is the encoded `calendar` record with `allHistory`, whose 7,305 cells each carry a count and an item text of up to 18 bytes (`"prop:rated=-7304"`), at most about 155,000 bytes. Then one row of each kind on every day from 1970-01-01, each with a stored time of 4,000,000,000 ms: the request succeeds and its record, larger than 262,144 bytes, decodes to the same maps.
- The day keys a review graph returns lie in the history window; a card graph's lie wherever its cards put them. How many days a chart shows is U226's.

## G7. Graph preferences

Changed 2026-11-22 (round 11): the Statistics page writes none of the three keys: their effects are fixed (`stats/statistics-page` S3.4). The core still reads and returns them; this section's write rule applies to no control.

Three synced config keys change what graphs show. The result carries their values as read in G1's snapshot:

| Key | Type, default | Read |
|---|---|---|
| `firstDayOfWeek` | integer, Sunday | `editor/config-keys` K5 |
| `cardCountsSeparateInactive` | boolean, `true` | a JSON boolean; anything else, or no key, reads `true` |
| `futureDueShowBacklog` | boolean, `true` | as above |

The result also carries `fsrs` (a JSON boolean; anything else, or no key, reads `false`) and `R`.

- The aggregates for both settings of each boolean are in the record (Card Counts both ways, Future Due with and without backlog), so flipping one re-bins without a core call.
- A change from the screen is written through U103's config op, one key at a time, by the rules of `editor/config-keys` K1 (K12 for the two booleans): a compact JSON `true` or `false` (the K5 integer for `firstDayOfWeek`), written at once only when the user flips the toggle or picks a weekday giving a value different from the one read, as one skip-undo write of source `UserNonUndoable`; a missing key or a value of another type is kept until then. Nothing else in this spec writes config. Anki writes all three keys on every change (G13, `editor/config-keys` K6).
- Scenarios observe `fsrs` through the `context` part of the `stats` kind (G14; scenario `fsrs-read`, G15) and the three keys with the draft `graph_preferences` kind (`conformance/stats-ops` H13; scenario `preferences-read`, planned, G15).

## G8. Requests and results

### G8.1 Request

| Member | Meaning |
|---|---|
| scope | `Collection`, `Deck(id)` or `Search(text)` (G2) |
| history | `last12Months` or `allHistory` (G3) |
| graphs | the graphs wanted (G5) |
| refresh | the id of the refresh the request belongs to (G8.3) |
| context | absent in a refresh's first request; in its second, the context token of the first's Data result (G8.3) |
| cancel | a cancel token (KTD9) |

Requests run on the query lane, never on the main thread (R1291).

### G8.2 Result

One of:

- **Data**: the context of G1, the context token (G8.3), the preferences of G7, the scope's browse text (`stats/browse-queries` Q2: none for Collection, the scope text S, or `unavailable`), and one record per requested graph (G6).
- **Context changed**: a second request whose new snapshot does not have its token's generation and seq (G8.3). No data comes with it.
- **Scope error**: the search error, as `search/errors` E1 to E5 give it (kind, `provided`, `context`, range). The screen shows it under the search field and keeps the graphs it shows (R1351). Also `unknown_deck` for a Deck id that names no deck.
- **Unreadable row**: G4, with the table, column and row key of `model/row-records` R10.
- **Cancelled**: the token fired. Nothing is kept from the request.

An error or Context changed result never adds to the memo (G8.3) and never changes the caches.

### G8.3 Refreshes and the memo

Changed 2026-11-22 (round 11): the Statistics page makes one request per refresh, naming every part, and the core returns the parts in tiers as they are ready, with progress for the slow ones (`stats/history-reads` H15). The two-request rule below applies to no screen; the context token, the memo and the rule that every request resolves the whole scope hold for that one request.

A refresh is one computation of the screen for a scope and history: opening Stats, a scope or history change, or a refresh after a change (G10). Each refresh takes a new context (G1).

- U225 makes two requests per refresh: the graphs above the fold, then the rest as soon as the first returns Data, both with the refresh's id (R1355, R1838). After any other result of the first request (a scope error, `unreadable_row`, Cancelled), U225 sends no second request.
- **Context token.** Decided 2026-10-27: no snapshot, transaction or statement is held between the two requests, since nothing stays open across an FFI call. The first request's Data result carries an opaque token holding the refresh id, the scope, the history, `N`, `today`, `R`, now, the offset, the zone, and the snapshot's generation and seq. The second request passes it, takes a new snapshot without reading the day clock again, and:
  - when that snapshot's generation and seq equal the token's (and its refresh, scope and history are the request's), computes with the token's values, so both requests see the same rows, cards, config and clock values;
  - otherwise returns Context changed. U225 then starts a new refresh; the commit in between has made the graphs stale anyway (G10).
- **Memo.** The resolved scope (the scope's cards and its rows in the window) is memoized under (scope, history, generation, seq, `N`, refresh). The core holds at most one memo, the latest refresh's. The second request of that refresh reuses it; no other request does. It is dropped when the second request returns, when a newer refresh's first request starts, on that refresh's cancellation and on a memory warning (G9.5). A newer first request drops it whatever that request then returns: one that ends in a scope error or `unreadable_row` leaves no memo at all, the older refresh's included (it adds none, G8.2). The screen still shows its previous graphs (G8.2, U225); the memo only saves work inside one refresh. A second request that finds no memo resolves the scope again on its snapshot with the token's values, which gives the same result.
- Every request resolves the whole scope, its cards and its window rows, whatever graphs it names. So an unreadable row (G4) or a scope error fails both requests of a refresh alike, and the screen never shows graphs from the first and then an error from the second.
- Every open and every refresh resolves the scope again, because search terms such as `prop:r`, `is:due` and `rated:` depend on the clock, and retrievability is computed for now (R1294).
- A refresh that a newer one supersedes is cancelled through its cancel token (KTD9).

## G9. Column caches

### G9.1 Content

While a collection is open, the core can hold column copies of the review log and of the cards (R1289):

- Review log, sorted by `id`: `id`, the card's slot in the card columns (or none for a deleted card's row), `ease`, `ivl`, `lastIvl`, `factor`, `time`, `type`, each as G4 reads it. Unreadable rows are listed apart with their `id`, `cid` and column, so G4's error is raised exactly when a request selects one.
- Cards: `id`, `nid`, `did`, `odid`, `type`, `queue`, `due`, `odue`, `ivl`, stability, difficulty, `decay` and `lrt` (each of the last four absent when the card data has none), and any other card column a graph spec reads. Unreadable card rows are listed apart in the same way.
- Each column uses the narrowest width that holds every stored value without loss.
- `lrt` is the card data's value only. A card without one gets its last review instant from the retrievability rule of `stats/graphs-cards` (U101), which reads the card's own columns, as Anki's graph data does. The review log is not read for it. Decided 2026-10-27; it replaces the plan's fill from the card's newest review-log row, which Anki's graph data does not do.
- Deck names, notes, tags and note types are not cached. Requests read them from the snapshot.

### G9.2 Build

- The caches are built in the background at utility QoS on the maintenance reader (KTD3), once Home has been idle for 2 s after the collection opened, never on the launch path. The rows of the last-12-months window are built first, then the rest of the review log in the same build (R1838: switching to all history then scans nothing on the request path).
- Decided 2026-10-27: this holds for every refresh of the component (`storage/derived-store` D7), including the rebuild that a bind verdict (D5.2: a new file, `ready` 0, a crash) or an invalidation queues. `stats_columns` registers as a deferred component: its queued refresh does not start before the host's signal `homeIdle` for the open, which U103 sends once Home has been idle for 2 s after the open (a refresh queued after that signal starts at once), or before a graph request chooses to wait for it (below). Until then the component is `Stale` and requests compute from their snapshot. The hold is the `deferred until` item of `storage/derived-store` D4, set to `homeIdle`. The signal enters the core through `stats_home_idle()`, which U98 adds and which only calls the derived host's `signal("homeIdle")` (D4); U103 exports it and calls it once Home has been idle for 2 s after each open, and does nothing else for it. A request that chooses to wait calls `request_refresh("stats_columns")` (D4) and then waits on D8's events.
- A request that finds the caches not current for its snapshot (`storage/derived-store` D8) either waits for the build, which then starts at once, or computes from its snapshot directly. Both give the same values.

### G9.3 Persistence

- The caches are a derived-store component, `stats_columns` (`storage/derived-store` D4), with its stamp, its verdicts at bind (D5.2), its incremental apply (D6) and its rebuild (D7).
- They are kept as column files in a directory next to `meguri-derived.db` that only this component uses, which the core maps read-only, so their pages do not count against the app's memory. The files are part of the derived store under its D1 rules: never synced, backed up or exported, and deleting them, or the whole store, while the collection is closed loses nothing. Names and layout are the implementer's.
- Decided 2026-10-27, because `storage/derived-store` D4 knows only tables and indexes: the files are content of the component through a record table of `stats_columns`, one row per file (name, length, checksum). A file is live only while a row names it, so the files follow every rule the store applies to tables:
  - Writing (a build, or the overlay merge below) writes each file whole under a new name and flushes it to disk; then one derived-writer transaction replaces the record rows (for a rebuild, the swap transaction of D7, with `ready` 1 and the stamp's `seq`). Each record row also holds the base seq: the last commit its file's content holds. A crash before that transaction leaves the old rows and files in force.
  - D5.2's verdicts and D10's emptying act on the record rows like any table. A file is mapped only when the component is fresh at bind and the file's length and checksum match its row; anything else is a rebuild (the component has no catch-up, D4: the rollover's unbury is a `Raw` write).
  - A reader maps only the files named by the rows its snapshot reads, and only while the component is current for that snapshot (D8). Files no row names (left by a crash, a swap, or a deleted or recreated store, D5.1) are deleted at bind, and after a swap once no reader maps them.
- Changes after the mapped base go to an overlay in memory, written by the component's apply on the derived writer. At suspend and close, before the store writes `clean` 1 (D11), the overlay is merged and the files rewritten as above: this is the component's `persist` item (`storage/derived-store` D4), which D11 runs. A failed merge marks the component `ready` 0 with its rebuild queued (D4), so the next bind rebuilds it.
- Decided 2026-10-27, the overlay is versioned by seq, because it is not part of any SQLite snapshot and the derived writer applies a commit after the commit is published (D6):
  - Each overlay entry carries the seq of its commit. The overlay also keeps its applied seq: the stamp's `seq` written by the last derived-writer transaction whose entries it holds, commits with nothing for this component included. It advances only after that transaction has committed, and a rolled-back apply leaves no entry.
  - A request with snapshot seq s, once `is_current` (D8) is true for its snapshot, reads the base its snapshot's record rows name plus exactly the overlay entries with a seq after the base seq and at or before s. Entries after s are ignored.
  - When the overlay's applied seq is before the stamp's `seq` that the snapshot reads, the request waits for the next `DerivedApplied` (D8) or computes from its snapshot directly (G9.2); it never reads an overlay that lacks a commit its snapshot sees.
  - How entries are kept (a delta log, or entries combined once no running request needs the older state) is the implementer's, as long as every request sees exactly its own seq's state.
- A component that is fresh at the next open (D5.2) maps its files without a review-log scan.

### G9.4 Updates

Each committed change set (`engine/change-stream` S1, S2) updates the caches from its payloads:

| Change set member | Effect |
|---|---|
| `revlog_added` | the rows are inserted in `id` order, with their card's slot |
| `revlog_removed` | the rows are removed (undo of an answer, a sync that removes rows) |
| `cards` `Added` | a card slot; its rows attach to it (below) |
| `cards` `Updated`, `memory_state_only` included | its columns change to the after-image |
| `cards` `Removed` | its slot goes; its rows become deleted cards' rows |
| anything else | nothing in the caches (G10 still applies) |

- A change set is applied in this order: `revlog_removed`, `cards` `Removed`, `cards` `Added` and `Updated`, `revlog_added`.
- **Unreadable entries.** The lists of unreadable rows (G9.1) follow the same changes: `revlog_removed` drops a listed review-log row with that `id`; `cards` `Removed` drops a listed card with that id; `cards` `Added` or `Updated` for a listed card id drops it from the list and gives the card its slot from the after-image, which is a readable row. The attach step below also rewrites the `cid` of every listed unreadable review-log row whose `id` is among the ids it read, so a re-keyed card's unreadable row stays in that card's scope. A row that a change set adds or updates and that G4 cannot read is listed apart as G9.1 says, in place of a slot. Decided 2026-10-27.
- **Attaching rows to an added card.** The component's capture (`storage/derived-store` D4), on the collection writer right after the commit, reads the ids of the card's review-log rows by `cid` (an indexed read, as `stats/card-info` reads a card's rows), so it sees the committed rows; it never matches on the cached rows' own card ids. Each cached row with one of those `id`s takes the new card's slot. This covers the undo of a delete and a sync that brings a card back, and also a card re-key (`sync/cloudkit-records#K14.2`, source `CloudRekey`), which rewrites `revlog.cid` in place: no change set lists a review-log row changed in place, so the re-key shows only as the card `Removed` under its old id and `Added` under its new one. Decided 2026-10-27; a rule that attached only cached rows whose card id already equals the new id would leave the re-keyed card's rows detached, out of its Deck and Search scopes, until the next rebuild.
- Ops of every source take this path: answers and their undo and redo, card edits, moves, deletions, Set Due Date and Forget, filtered-deck build and empty, suspend and bury, the rollover's unbury, imports, and the CloudKit sync sources `CloudIncoming` and `CloudRekey` (`engine/change-stream` S1.2). `SyncIncoming`, used only by frozen manual Anki transfers, takes it the same way.
- `CollectionReplaced`, Check Database and an external change invalidate the caches (`storage/derived-store` D10); they are rebuilt, and no request reads the old generation's columns.

### G9.5 Release

- On a memory warning the caches are released: the mapped files unmapped and the overlay dropped (R1292). U103's `releaseStatsCaches()` calls the core's `release_stats_caches()`, which U98 implements: it drops the memo (G8.3) and calls the derived host's `release("stats_columns")` (`storage/derived-store` D4), which runs the component's `persist` item (the merge below) and then its `release` item (unmap and drop). U103 only forwards the call, off the main thread (G9.6). The next request reloads the files, or rebuilds, and returns the same values.
- Decided 2026-10-27: an overlay is never dropped with commits the files lack. A release with a non-empty overlay first merges it and rewrites the files on the derived writer, as at suspend (G9.3), and drops it once the new record rows have committed; when that write fails, the component is marked `ready` 0 and its rebuild queued, as for a failed apply (`storage/derived-store` D6). Only an empty overlay is dropped at once. So reloading the files after a release never loses a commit.
- The resident aggregates of U95 (`stats/counting-rules` N7) are never released this way.
- At 1,200,000 rows the review-log columns stay under about 40 MB (R1292); U104 measures it.

### G9.6 Contract

- **Equivalence.** Every result equals what G2 to G7 give for the snapshot's rows and config and the context, computed afresh. This holds after any sequence of the changes in G9.4, including changes that land during a build, a release, and a close and reopen. Property tests compare with a cold computation over random op sequences (U98, U104); the op mix includes a card re-key (`CloudRekey`) of a card with review-log rows, in a Deck and a Search scope; unreadable card and review-log rows (written raw before the build), their removal and replacement by readable rows, and a re-key of a card that has an unreadable row; a memory warning with a non-empty overlay; and commits that land between the two requests of a refresh (G8.3).
- **Never stale.** Columns written for another stamp or generation are never read.
- **Off the main thread.** Building, loading, updating and releasing never run on the main thread (R1291).

## G10. When graphs are stale

Changed 2026-11-22 (round 11): the Statistics page refreshes half a second after the last of these (`stats/statistics-page` S11), and drops its kept results on `CollectionReplaced` (S10.2).

The screen's graphs are stale, and U225 refreshes them (R1356), after:

- any published change set, whatever it lists: rows and cards change values, and notes, tags, decks, note types, presets and config change Search and Deck scopes and preferences;
- `RolledOver`, `NextDayAtChanged` and `TimeZoneChanged` (`sched/day-clock` D8), which change `N`, `today` and the offset;
- `CollectionReplaced`;
- every open of the screen (R1294).

U103's `StatsChanged{graphsStale}` is the change-set part of this list.

## G11. Read only

Requests, builds, updates and releases never write `collection.anki2` (R1295): no row, `usn`, `mod`, `col.mod` or config key changes. Only the preference writes of G7 and the heatmap settings write config. Scenario `read-only` compares dumps around the observations, after a `timing` observation, as `conformance/stats-ops` H3 says. The PDF export (`stats/pdf-export` X8) reads only the screen's model and writes nothing to the collection either.

## G12. Decisions

Decided 2026-10-27, after comparing the plan's U98 and U226 entries:

1. **Who writes what.** U98's file list gave this file "bin edges, range definitions, quantile method, day-label bounds", while U226 gives the same to `stats/graph-presentation`. The presentation layer owns them, since U226's Swift code is the only one that bins (U98 decision 4). `stats/graph-presentation` owns per-graph ranges and their defaults, bin edges and the 70-bin rule, quantiles from cumulative histograms, day, tooltip and table text, and every histogram bar's query. This file owns scopes, history, row reading, aggregate shape, preferences, the request contract and the caches. `stats/browse-queries` owns the scope text, the join and the fixed-item queries.
2. **Graphs page in the oracle (R1960).** Presentation goldens may be recorded by running Anki's pinned graphs page inside a quarantined oracle image (`oracle/graphs-page/`), as the plan's default says. The page and its assets never leave that image. Goldens hold observed values only, and Meguri's tests and app never load any of it. This keeps R1960's purpose, no Anki web assets in Meguri or its tests, and the editor keeps its observed goldens (U190).
3. **Last review time.** G9.1: from the card data's `lrt` only.

## G13. Differences from Anki

- Anki's Deck scope is the search `deck:current` and sets the current deck when the deck changes. Meguri's is the deck chosen in Stats, by id, and never sets `curDeck` (R1353, R1354).
- For the deck names of G2.1's special list (`stats/browse-queries` Q2.2) the oracle's deck search differs from the Deck scope: Anki's builder writes `deck:current` for a deck named `current`. The same holds for a deck whose full name is exactly `_*`: Anki's search-node builder escapes `\`, `*` and `_` in a deck name except in that one name, so the oracle's `deck:_*` matches every deck with a non-empty name, while the Deck scope (G2.1) and `stats/browse-queries` Q2.2's `deck:"\_\*"` match only that deck and its subdecks. Meguri unit tests pin those names.
- Preference writes: Anki's Stats screen writes all three keys of G7 on every change, so a missing key is created with its current value and a changed one gets a new `usn`. Meguri writes only the changed key (`editor/config-keys` K6). Decided 2026-10-27: kept, because no read value differs (a missing key reads its default); only which keys exist and their `usn` and `mtime_secs` differ.
- The Stats search field replaces U+001C to U+001F with U+0020 (G2, `search/join-and-replace` J2); Anki's Stats page passes the typed text unchanged. Only a text holding those characters gives a different scope.
- Card Ease is not computed (R1360).
- Parts that need all history are left out with `last12Months` (G3), where Anki computes them and hides them. The visible result is the same.

## G14. Observation for scenarios

The scope scenarios observe the draft `stats` kind (`conformance/observations` O4), which this section defines. U99's parts are `stats/graphs-revlog` V8, U101's `stats/graphs-cards` C12; `conformance/stats-ops` H10 gathers the whole kind.

- **Request.** `{stats: [<request>, ...]}`, a non-empty list. A request is `{scope, history, parts}`: `scope` is `collection`, `{deck: <full name with ::>}` or `{search: <text>}` (passed unchanged); `history` is `year` or `all`; `parts` is a non-empty list of part names.
- **Value.** A list with one output per request, in order: an object with one member per requested part, or an error `{error: <kind>}` with the search error members of `conformance/search-ops` S3, or `{error: "unreadable_row"}`.
- **U98's parts.** Each part is one message of Anki's graph data, written as JSON with the field names as the protobuf file declares them (`including_inactive`, `newCards`, `rollover_hour`; not the lowerCamelCase of the canonical protobuf JSON mapping), map keys as decimal strings, and members left at their default omitted. A message member is written whenever Anki's data sets it, empty or not; only empty maps and scalar members at their default are omitted (`stats/graphs-cards` C12). `card_counts` always has both count sets, so a scope with no card gives `{excluding_inactive: {}, including_inactive: {}}` (scenario `stats/graphs/card-counts/empty-scope`). A part holds its message whole, wrapper included:
  - `reviews`: `{count, time}`, each day index → `{learn, relearn, young, mature, filtered}`;
  - `added`: `{added: {<day index>: <number of cards>}}`;
  - `card_counts`: `{including_inactive, excluding_inactive}`, each `{newCards, learn, relearn, young, mature, suspended, buried}`;
  - `context`: `{rollover_hour, fsrs}`.
- **Oracle.** Anki's graph data for search text and days: `collection` → the empty search, `{deck}` → the search Anki's own search builder writes for a deck node with that name (as `conformance/stats-ops` H7), `{search}` → the text as given; `year` → 365 days, `all` → 0. Anki's graph request runs the search and reads the rows in one call, so a database error from either looks the same. The oracle therefore first runs the search text alone as a card search, as `conformance/search-ops` S1 does: an error there is the output, mapped as S1 maps it (a database error there is `search_failed`). Only when that succeeds does it ask for the graph data; a database error raised then is `unreadable_row`. Any other exception is a run error.
- **Meguri.** The core's graph request (G8) with `Collection`, `Deck(<id of the named deck>)` or `Search(<text>)`, mapped into these shapes. A deck name that names no deck is a run error (`unknown_ref`).
- Numbers inside `stats` compare with the 1e-6 tolerance of `conformance/observations` O7.
- Scenarios avoid the deck names `stats/browse-queries` Q2.2 lists (`current`, `filtered` and the `did:` cases) and the full name `_*`, where the oracle's deck search differs (G13).

## G15. Scenarios

`conformance/scenarios/stats/graphs/scope/`, for the oracle-maintainer to add with the `stats` kind (G14) and the stats op of `oracle/driver/ops/stats.py`. As of 2026-11-17 each is in the tree with its golden, except `preferences-read`; the oracle has the kind, the runner not yet. `preferences-read` is planned, not in the tree: it also needs the `graph_preferences` kind (`conformance/stats-ops` H13), which the oracle driver and the schema have but the testkit's scenario model does not parse yet, so goldens-check would refuse it. Its id is `stats/presentation/preferences-read`, as `conformance/stats-ops` H13 names it, although it belongs with these. A copy was prepared and held back with the `presentation/` set; it was never committed and has no golden. It lands, inactive and recorded, in the change that adds the kind to the testkit model; filing it in this directory instead would give it a new id, recorded again. Until then G7's `fsrs` reading is pinned by `fsrs-read`, and the reading of the three keys only by Meguri tests from G7's table. `unreadable-row`'s third case stores a `lastIvl` of 3,000,000,000 instead of NULL, which the `revlog` table refuses (NOT NULL). Review-log rows come from `db_exec` as in `conformance/stats-ops` H3. All in Europe/Paris, rollover 4, unless named.

| Scenario | Pins | Expected |
|---|---|---|
| `deleted-card-rows` (AE53, graph side) | G2 | 5 Review rows today on a card in `Lang`, 2 on a deleted card's id: `reviews` count for day 0 is 7 in Collection and in Search `"  "`, 5 in Deck `Lang` and in Search `deck:*` |
| `moved-card` | G2 | 3 rows while the card is in `A`, then the card moved to `B`: Deck `B` 3, Deck `A` 0, both histories |
| `filtered-deck-card` | G2.1 | a card with `did` = filtered deck `F`, `odid` = `H`: in Deck `F`, Deck `H` and Deck `Top` (parent of `H`); not in sibling `H2`; `card_counts` and `added` likewise |
| `subdeck-wildcards` | G2.1 | decks `a_b`, `a_b::c`, `axb`, one card each: Deck `a_b` has 2 cards, Deck `axb` 1 |
| `history-window` | G3 | rows at start − 1 ms, start, start + 1 ms, now, `N` + 1 day, and id −5: `year` reads 4 of them, `all` 5 (never the negative id) |
| `card-graphs-ignore-history` | G3 | cards created 400 and 10 days ago: `added` and `card_counts` equal for `year` and `all` |
| `invalid-search` | G2, G8.2 | `prop:foo<`, `prop:ivl>`, `"abc`: their search error kinds, no parts |
| `odd-row-values` | G4 | rows with `type` 260, −1, 6 and 1.5, `time` −5, 5,000,000,000 and 1.5, `lastIvl` 20.9 and 21: the 260 row is skipped as Manual, −1, 6 and 1.5 count as Learn, the three times count 0 ms, 20.9 is young, 21 mature |
| `unreadable-row` | G4 | Collection scope, a row with `ease` 300 dated 400 days ago: `year` returns data, `all` returns `unreadable_row`; the same with `factor` −1 and with a `lastIvl` of 3,000,000,000 (beyond 32 bits, G4) |
| `unreadable-card` | G4 | one card in `A`, one in `B`, rows inserted with `usn` −1: the `A` card with `type` 7, then `queue` 9: parts [`reviews`] for Deck `A` (both histories) and Collection return `unreadable_row`, Deck `B` [`reviews`, `added`] returns data; with `due` and `odue` 2^40 instead, Deck `A` and Collection return data (one new card in `A`); then a review-log row today with `ease` 300 on the `B` card: [`card_counts`] alone returns `unreadable_row` for Deck `B` and Collection, data for Deck `A` |
| `fsrs-read` | G7 | no `fsrs` in setup; `context` with `fsrs` missing, then set to `true`, `1`, `"true"`, `null`, then removed: `fsrs` false, true, false, false, false, false |
| `scope-follows-retag` | G2, G10 | Search `tag:verbs`: a note retagged between two observations moves its cards' rows in and out |
| `read-only` | G11 | `timing`, `db_dump`, `stats` for every scope and history, `db_dump`: the dumps are equal |
| `preferences-read` (planned: needs `graph_preferences` in the testkit model) | G7 | no `fsrs` in setup; `graph_preferences` for a new collection, then after writing (`cardCountsSeparateInactive`, `futureDueShowBacklog`, `firstDayOfWeek`) as (`false`, `false`, `1`; `true`, `true`, `5`; `0`, `"false"`, `6`; `null`, `1`, `3`; then `firstDayOfWeek` `"1"`, `1.0` and `true`), then with the three keys removed: `{calendar_first_day_of_week, card_counts_separate_inactive, future_due_show_backlog}` reads `{0, true, true}`, `{1, false, false}`, `{5, true, true}`, `{6, true, true}`, `{0, true, true}` four times, then `{0, true, true}`; then `context` with `fsrs` missing, `true`, `1`, `"true"`, `null` and removed: `fsrs` false, true, false, false, false, false |

Meguri-only tests (no oracle): the deck names `stats/browse-queries` Q2.2 lists and the full name `_*` (G2.1, G13); the memo, context token and refresh rules, including Context changed after a commit between the two requests and no second request after an error (G8.3); the cache contract (G9.6); release and reload, with an empty and a non-empty overlay (G9.5); the background build start, including a rebuild verdict at bind held until `homeIdle` (G9.2).

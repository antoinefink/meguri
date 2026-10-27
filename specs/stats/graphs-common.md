# Stats graphs: scopes, history, data and caches

Spec: stats/graphs-common
Requirements: R1281, R1282, R1283, R1285, R1289, R1290, R1291, R1292, R1293, R1294, R1295, R1349, R1350, R1351, R1352, R1353, R1355, R1356, R1357, R1360, R1367, R1446, R1838
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/graphs/scope/

The machinery every Stats-screen graph shares (U98): which cards and review-log rows a request reads (scope and history), how it reads them, what it returns, the column caches behind it, and what the Stats shell (U225) can rely on. Individual graphs are `stats/graphs-revlog` (U99) and `stats/graphs-cards` (U101); binning, ranges and text are `stats/graph-presentation` (U226); browse queries are `stats/browse-queries`. Days are `stats/day-buckets`, the Today section `stats/studied-today`.

## G1. Request context

Each request is computed for one context, taken once when the request's refresh starts (G8.3):

| Value | Meaning | Source |
|---|---|---|
| `N` | `next_day_at`, epoch seconds | the day clock (`sched/day-clock` D8 `nextDayAt`) |
| `today` | the scheduler's day number (`days_elapsed`) | the day clock |
| `R` | the rollover hour, 0 to 23 | the day clock's effective `rollover` (`sched/day-clock` D13) |
| now | the current instant, epoch milliseconds | the clock (U21) |
| offset | the zone's UTC offset at now, seconds east | the clock's zone and the bundled zone data (`stats/day-buckets` B5) |
| state | one collection snapshot (`engine/op-model` O6): its generation and seq | the reader the request runs on |

- Every value a request returns is computed from this one context. Rows, cards, the scope's search and the config reads all come from the same snapshot.
- Reading `nextDayAt` may run the day clock's own check (`sched/day-clock` D8), with the writes that check makes, such as `localOffset`. Those writes are the day clock's. The graph request itself writes nothing (G11).
- The result carries `N`, `today`, `R` and the offset, so the presentation layer (U226) needs no clock of its own for day labels and calendar dates.

## G2. Scopes

The scope is one of three, passed typed to the core (R1350, R1351):

| Scope | Cards | Review-log rows |
|---|---|---|
| Collection | every card of the `cards` table | every row, including deleted cards' rows (R1281) |
| Deck `D` (by id) | G2.1 | the rows of those cards (R1282) |
| Search `q` (text) | the cards the search `q` matches in cards mode (`search/query-model`) | the rows of those cards (R1282) |

- A deleted card's row is a row whose `cid` matches no card (`stats/counting-rules` N1). Only the Collection scope reads them.
- A row belongs to its card as the card is now: a card moved from deck A to deck B after its reviews brings all its rows to B's scope and takes them out of A's (R1282). Nothing about the deck at review time is kept or read.
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

- Review log (`model/row-records` R5): `ivl` and `lastIvl` after conversion to an integer (a REAL value truncated); `time` as 0 when it is not an INTEGER from 0 to 4,294,967,295 (so a negative or huge `time` counts 0 ms in the graphs, unlike the Home line, `stats/studied-today` T2.1); `type` as its low 8 bits, and 0 (Learn) when those are not 0 to 5. So `type` 260 reads as 4 (Manual), `type` −1 and 6 read as Learn (scenario `odd-row-values`).
- Cards (`model/row-records` R4), with the card data of `model/card-data-json`: the memory state (`s`, `d`), `decay` and `lrt`, each absent when the data does not hold it.
- A row the request reads that `model/row-records` R10 calls unreadable gives the result `unreadable_row` (G8.2) and no graph data, as Anki's graph request fails. That covers a review-log `ease` outside 0 to 255, a negative `factor`, an `ivl` beyond the 32-bit range after conversion, and an unreadable card row.
- "The request reads" means the rows G2 and G3 select. A bad row outside the history window, or of a card outside the scope, changes nothing (scenario `unreadable-row`).
- Values are never repaired or written back. Check Database is the repair path (U128).

## G5. Graphs

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
  - Decided 2026-10-27, the encoding's remaining parts: every sparse map, whatever its key (day index, interval, rounded stability, difficulty or retrievability percent), is written in ascending key order as runs of consecutive keys. A map is its number of runs, then each run: its start key (the first run's as is, each later one as its distance from the previous run's last key), its length, then one value array per member with exactly that many items. Every other list (the retention rows, the Hourly and Answer Buttons buckets) is its length followed by its items.
  - Decided 2026-10-27, the conditions: the limit holds, and is tested, when the counted review history starts on 2006-01-01 or later, each row kind has fewer than 8,192 rows and a summed time below 2²⁷ ms on any one day, and every card's due day and interval lie within 36,500 days of today. Outside them (an imported or hand-built history back to 1970, stored times near the u32 maximum that `stats/graphs-revlog` V9 admits, corrupt due values) a record may be larger. Such a record is still returned whole: the request never fails for size, and nothing is dropped, capped or merged to fit, so the Stats screen keeps working on odd imported data.
  - Test (U99 with U103): 20 years of daily study, 7,305 consecutive days with rows of every kind on each day, 150 counted rows a day and stored times of 5,000 to 60,000 ms: the encoded `reviews` record with `allHistory` is at most 262,144 bytes and decodes to the same maps. Then one row of each kind on every day from 1970-01-01, each with a stored time of 4,000,000,000 ms: the request succeeds and its record, larger than 262,144 bytes, decodes to the same maps.
- The day keys a review graph returns lie in the history window; a card graph's lie wherever its cards put them. How many days a chart shows is U226's.

## G7. Graph preferences

Three synced config keys change what graphs show. The result carries their values as read in G1's snapshot:

| Key | Type, default | Read |
|---|---|---|
| `firstDayOfWeek` | integer, Sunday | `editor/config-keys` K5 |
| `cardCountsSeparateInactive` | boolean, `true` | a JSON boolean; anything else, or no key, reads `true` |
| `futureDueShowBacklog` | boolean, `true` | as above |

The result also carries `fsrs` (a JSON boolean; anything else, or no key, reads `false`) and `R`.

- The aggregates for both settings of each boolean are in the record (Card Counts both ways, Future Due with and without backlog), so flipping one re-bins without a core call.
- A change from the screen is written through U103's config op, one key at a time (`editor/config-keys` K6). Nothing else in this spec writes config.

## G8. Requests and results

### G8.1 Request

| Member | Meaning |
|---|---|
| scope | `Collection`, `Deck(id)` or `Search(text)` (G2) |
| history | `last12Months` or `allHistory` (G3) |
| graphs | the graphs wanted (G5) |
| refresh | the id of the refresh the request belongs to (G8.3) |
| cancel | a cancel token (KTD9) |

Requests run on the query lane, never on the main thread (R1291).

### G8.2 Result

One of:

- **Data**: the context of G1, the preferences of G7, and one record per requested graph (G6).
- **Scope error**: the search error, as `search/errors` E1 to E5 give it (kind, `provided`, `context`, range). The screen shows it under the search field and keeps the graphs it shows (R1351). Also `unknown_deck` for a Deck id that names no deck.
- **Unreadable row**: G4, with the table, column and row key of `model/row-records` R10.
- **Cancelled**: the token fired. Nothing is kept from the request.

An error result never changes the memo (G8.3) or the caches.

### G8.3 Refreshes and the memo

A refresh is one computation of the screen for a scope and history: opening Stats, a scope or history change, or a refresh after a change (G10). Each refresh takes a new context (G1).

- U225 makes two requests per refresh: the graphs above the fold, then the rest as soon as the first returns, both with the refresh's id (R1355, R1838). Both use the refresh's context.
- The resolved scope (the scope's cards and its rows in the window) is memoized under (scope, history, generation, seq, `N`, refresh). The second request of a refresh reuses it; no other request does.
- Every open and every refresh resolves the scope again, because search terms such as `prop:r`, `is:due` and `rated:` depend on the clock, and retrievability is computed for now (R1294).
- A refresh that a newer one supersedes is cancelled through its token (KTD9).

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
- A request that finds the caches not current for its snapshot (`storage/derived-store` D8) either waits for the build, which then starts at once, or computes from its snapshot directly. Both give the same values.

### G9.3 Persistence

- The caches are a derived-store component, `stats_columns` (`storage/derived-store` D4), with its stamp, its verdicts at bind (D5.2), its incremental apply (D6) and its rebuild (D7).
- They are kept as column files in a directory next to `meguri-derived.db` that only this component uses, which the core maps read-only, so their pages do not count against the app's memory. The files are part of the derived store under its D1 rules: never synced, backed up or exported, and deleting them, or the whole store, while the collection is closed loses nothing. Names and layout are the implementer's.
- Decided 2026-10-27, because `storage/derived-store` D4 knows only tables and indexes: the files are content of the component through a record table of `stats_columns`, one row per file (name, length, checksum). A file is live only while a row names it, so the files follow every rule the store applies to tables:
  - Writing (a build, or the overlay merge below) writes each file whole under a new name and flushes it to disk; then one derived-writer transaction replaces the record rows (for a rebuild, the swap transaction of D7, with `ready` 1 and the stamp's `seq`). A crash before that transaction leaves the old rows and files in force.
  - D5.2's verdicts and D10's emptying act on the record rows like any table. A file is mapped only when the component is fresh at bind and the file's length and checksum match its row; anything else is a rebuild (the component has no catch-up, D4: the rollover's unbury is a `Raw` write).
  - A reader maps only the files named by the rows its snapshot reads, and only while the component is current for that snapshot (D8). Files no row names (left by a crash, a swap, or a deleted or recreated store, D5.1) are deleted at bind, and after a swap once no reader maps them.
- Changes after the mapped base go to an overlay in memory. At suspend and close, before the store writes `clean` 1 (D11), the overlay is merged and the files rewritten as above.
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
- **Attaching rows to an added card.** The component's capture (`storage/derived-store` D4), on the collection writer right after the commit, reads the ids of the card's review-log rows by `cid` (an indexed read, as `stats/card-info` reads a card's rows), so it sees the committed rows; it never matches on the cached rows' own card ids. Each cached row with one of those `id`s takes the new card's slot. This covers the undo of a delete and a sync that brings a card back, and also a card re-key (`sync/cloudkit-records#K14.2`, source `CloudRekey`), which rewrites `revlog.cid` in place: no change set lists a review-log row changed in place, so the re-key shows only as the card `Removed` under its old id and `Added` under its new one. Decided 2026-10-27; a rule that attached only cached rows whose card id already equals the new id would leave the re-keyed card's rows detached, out of its Deck and Search scopes, until the next rebuild.
- Ops of every source take this path: answers and their undo and redo, card edits, moves, deletions, Set Due Date and Forget, filtered-deck build and empty, suspend and bury, the rollover's unbury, imports, and the CloudKit sync sources `CloudIncoming` and `CloudRekey` (`engine/change-stream` S1.2). `SyncIncoming`, used only by frozen manual Anki transfers, takes it the same way.
- `CollectionReplaced`, Check Database and an external change invalidate the caches (`storage/derived-store` D10); they are rebuilt, and no request reads the old generation's columns.

### G9.5 Release

- On a memory warning (U103's call) the caches are released: the mapped files unmapped and the overlay dropped (R1292). The next request reloads the files, or rebuilds, and returns the same values.
- The resident aggregates of U95 (`stats/counting-rules` N7) are never released this way.
- At 1,200,000 rows the review-log columns stay under about 40 MB (R1292); U104 measures it.

### G9.6 Contract

- **Equivalence.** Every result equals what G2 to G7 give for the snapshot's rows and config and the context, computed afresh. This holds after any sequence of the changes in G9.4, including changes that land during a build, a release, and a close and reopen. Property tests compare with a cold computation over random op sequences (U98, U104); the op mix includes a card re-key (`CloudRekey`) of a card with review-log rows, in a Deck and a Search scope.
- **Never stale.** Columns written for another stamp or generation are never read.
- **Off the main thread.** Building, loading, updating and releasing never run on the main thread (R1291).

## G10. When graphs are stale

The screen's graphs are stale, and U225 refreshes them (R1356), after:

- any published change set, whatever it lists: rows and cards change values, and notes, tags, decks, note types, presets and config change Search and Deck scopes and preferences;
- `RolledOver`, `NextDayAtChanged` and `TimeZoneChanged` (`sched/day-clock` D8), which change `N`, `today` and the offset;
- `CollectionReplaced`;
- every open of the screen (R1294).

U103's `StatsChanged{graphsStale}` is the change-set part of this list.

## G11. Read only

Requests, builds, updates and releases never write `collection.anki2` (R1295): no row, `usn`, `mod`, `col.mod` or config key changes. Only the preference writes of G7 and the heatmap settings write config. Scenario `read-only` compares dumps around the observations, after a `timing` observation, as `conformance/stats-ops` H3 says.

## G12. Decisions

Decided 2026-10-27, after comparing the plan's U98 and U226 entries:

1. **Who writes what.** U98's file list gave this file "bin edges, range definitions, quantile method, day-label bounds", while U226 gives the same to `stats/graph-presentation`. The presentation layer owns them, since U226's Swift code is the only one that bins (U98 decision 4). `stats/graph-presentation` owns per-graph ranges and their defaults, bin edges and the 70-bin rule, quantiles from cumulative histograms, day, tooltip and table text, and every histogram bar's query. This file owns scopes, history, row reading, aggregate shape, preferences, the request contract and the caches. `stats/browse-queries` owns the scope text, the join and the fixed-item queries.
2. **Graphs page in the oracle (R1960).** Presentation goldens may be recorded by running Anki's pinned graphs page inside a quarantined oracle image (`oracle/graphs-page/`), as the plan's default says. The page and its assets never leave that image. Goldens hold observed values only, and Meguri's tests and app never load any of it. This keeps R1960's purpose, no Anki web assets in Meguri or its tests, and the editor keeps its observed goldens (U190).
3. **Last review time.** G9.1: from the card data's `lrt` only.

## G13. Differences from Anki

- Anki's Deck scope is the search `deck:current` and sets the current deck when the deck changes. Meguri's is the deck chosen in Stats, by id, and never sets `curDeck` (R1353, R1354).
- For the deck names of G2.1's special list (`stats/browse-queries` Q2.2) the oracle's deck search differs from the Deck scope: Anki's builder writes `deck:current` for a deck named `current`. Meguri unit tests pin those names.
- Card Ease is not computed (R1360).
- Parts that need all history are left out with `last12Months` (G3), where Anki computes them and hides them. The visible result is the same.

## G14. Observation for scenarios

The scope scenarios observe the draft `stats` kind (`conformance/observations` O4), which this section defines. U99's parts are `stats/graphs-revlog` V8, U101's `stats/graphs-cards` C12; `conformance/stats-ops` H10 gathers the whole kind.

- **Request.** `{stats: [<request>, ...]}`, a non-empty list. A request is `{scope, history, parts}`: `scope` is `collection`, `{deck: <full name with ::>}` or `{search: <text>}` (passed unchanged); `history` is `year` or `all`; `parts` is a non-empty list of part names.
- **Value.** A list with one output per request, in order: an object with one member per requested part, or an error `{error: <kind>}` with the search error members of `conformance/search-ops` S3, or `{error: "unreadable_row"}`.
- **U98's parts.** Each part is one message of Anki's graph data, written as JSON with the field names as the protobuf file declares them (`including_inactive`, `newCards`, `rollover_hour`; not the lowerCamelCase of the canonical protobuf JSON mapping), map keys as decimal strings, and members left at their default omitted. A part holds its message whole, wrapper included:
  - `reviews`: `{count, time}`, each day index → `{learn, relearn, young, mature, filtered}`;
  - `added`: `{added: {<day index>: <number of cards>}}`;
  - `card_counts`: `{including_inactive, excluding_inactive}`, each `{newCards, learn, relearn, young, mature, suspended, buried}`;
  - `context`: `{rollover_hour, fsrs}`.
- **Oracle.** Anki's graph data for search text and days: `collection` → the empty search, `{deck}` → the search Anki's own search builder writes for a deck node with that name (as `conformance/stats-ops` H7), `{search}` → the text as given; `year` → 365 days, `all` → 0. Anki's graph request runs the search and reads the rows in one call, so a database error from either looks the same. The oracle therefore first runs the search text alone as a card search, as `conformance/search-ops` S1 does: an error there is the output, mapped as S1 maps it (a database error there is `search_failed`). Only when that succeeds does it ask for the graph data; a database error raised then is `unreadable_row`. Any other exception is a run error.
- **Meguri.** The core's graph request (G8) with `Collection`, `Deck(<id of the named deck>)` or `Search(<text>)`, mapped into these shapes. A deck name that names no deck is a run error (`unknown_ref`).
- Numbers inside `stats` compare with the 1e-6 tolerance of `conformance/observations` O7.
- Scenarios avoid the deck names `stats/browse-queries` Q2.2 lists (`current`, `filtered` and the `did:` cases), where the oracle's deck search differs (G13).

## G15. Scenarios

`conformance/scenarios/stats/graphs/scope/`, for the oracle-maintainer to add with the `stats` kind (G14) and the stats op of `oracle/driver/ops/stats.py`. None exists yet: no executor has the kind. Review-log rows come from `db_exec` as in `conformance/stats-ops` H3. All in Europe/Paris, rollover 4, unless named.

| Scenario | Pins | Expected |
|---|---|---|
| `deleted-card-rows` (AE53, graph side) | G2 | 5 Review rows today on a card in `Lang`, 2 on a deleted card's id: `reviews` count for day 0 is 7 in Collection and in Search `"  "`, 5 in Deck `Lang` and in Search `deck:*` |
| `moved-card` | G2 | 3 rows while the card is in `A`, then the card moved to `B`: Deck `B` 3, Deck `A` 0, both histories |
| `filtered-deck-card` | G2.1 | a card with `did` = filtered deck `F`, `odid` = `H`: in Deck `F`, Deck `H` and Deck `Top` (parent of `H`); not in sibling `H2`; `card_counts` and `added` likewise |
| `subdeck-wildcards` | G2.1 | decks `a_b`, `a_b::c`, `axb`, one card each: Deck `a_b` has 2 cards, Deck `axb` 1 |
| `history-window` | G3 | rows at start − 1 ms, start, start + 1 ms, now, `N` + 1 day, and id −5: `year` reads 4 of them, `all` 5 (never the negative id) |
| `card-graphs-ignore-history` | G3 | cards created 400 and 10 days ago: `added` and `card_counts` equal for `year` and `all` |
| `invalid-search` | G2, G8.2 | `prop:foo<`, `prop:ivl>`, `"abc`: their search error kinds, no parts |
| `odd-row-values` | G4 | rows with `type` 260, −1, 6, `time` −5, 5,000,000,000 and 1.5, `lastIvl` 20.9 and 21: the 260 row is skipped as Manual, −1 and 6 count as Learn, the three times count 0 ms, 20.9 is young, 21 mature |
| `unreadable-row` | G4 | Collection scope, a row with `ease` 300 dated 400 days ago: `year` returns data, `all` returns `unreadable_row`; the same with `factor` −1 |
| `scope-follows-retag` | G2, G10 | Search `tag:verbs`: a note retagged between two observations moves its cards' rows in and out |
| `read-only` | G11 | `timing`, `db_dump`, `stats` for every scope and history, `db_dump`: the dumps are equal |

Meguri-only tests (no oracle): the deck names `stats/browse-queries` Q2.2 lists (G2.1); the memo and refresh rules (G8.3); the cache contract (G9.6); release and reload (G9.5); the background build start (G9.2).

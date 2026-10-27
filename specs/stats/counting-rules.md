# Counting rules for review aggregates

Spec: stats/counting-rules
Requirements: R1275, R1276, R1277, R1278, R1279, R1281, R1282, R1283, R1284, R1286, R1287, R1288, R1292, R1295, R1296, R290, R291, R304, R1340, R1341, R1343, R1447, R1453, R1818
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/heatmap-totals/, conformance/scenarios/stats/day-buckets/

Which review-log rows each review aggregate counts, and the contract the core's always-resident aggregates meet. Days come from `stats/day-buckets`. The settings come from `stats/heatmap-config`. The heatmap rules (N2 to N5) follow the Review Heatmap add-on 1.0.1, except where a deviation is named (R1447). The studied-today rule (N6) follows Anki.

## N1. Review-log rows

| Column | Use here |
|---|---|
| `id` | the review instant, epoch milliseconds (`stats/day-buckets` B1) |
| `cid` | the card the row belongs to |
| `ease` | the button pressed: 1 Again, 2 Hard, 3 Good, 4 Easy; 0 for a row without a rating |
| `time` | the time taken, milliseconds, stored when the row was logged |
| `type` | 0 Learn, 1 Review, 2 Relearn, 3 Filtered (cram), 4 Manual, 5 Rescheduled |

- Set Due Date, Forget/Reset and manual reschedules write Manual rows with `ease` 0. Bulk FSRS reschedules write Rescheduled rows with `ease` 0. Answers write rows with types 0 to 3 and `ease` 1 to 4.
- Rows other clients wrote can hold other combinations: a rated Manual row, an unrated Review row, any `time`. N2 and N6 say how each counts. Nothing is clamped or repaired.
- Deleting cards keeps their review-log rows. A row whose `cid` matches no card is **a deleted card's row**.
- A row belongs to its card as the card is now: its existence and deck are read at query time, never at review time (R1282, R1287).
- Every count counts rows, not distinct cards: five answers on one card count 5 (R1278).

## N2. Which rows the heatmap counts

- `limresched` true (the default): rows with `ease` ≥ 1, whatever their type. That includes learning steps, reviews, relearning, filtered and cram rows, and a rated Manual row (R1275, R1276, R290).
- `limresched` false: every row, whatever its `ease` and type.
- No other column plays a part.

From scenario `row-kinds` (one row of each kind on the same day):

| `type` | `ease` | Heatmap, `limresched` true | Heatmap, false | Studied today (N6) |
|---|---|---|---|---|
| 0 | 3 | yes | yes | yes |
| 1 | 1 | yes | yes | yes |
| 2 | 1 | yes | yes | yes |
| 3 | 3 | yes | yes | yes |
| 3 | 0 | no | yes | yes |
| 4 | 0 | no | yes | no |
| 5 | 0 | no | yes | no |
| 4 | 2 | yes | yes | no |
| 1 | 0 | no | yes | yes |
| 1 | 4 (time 900,000) | yes | yes | yes, with 900,000 ms |
| Total | | 6 | 10 | 7 rows, 922,000 ms |

## N3. Home scope

The Home heatmap and its streak stats count the rows N2 keeps, from this set:

- **N3.1** `limcdel` false and `limdecks` empty: every row, deleted cards' rows included (R1281, R291).
- **N3.2** `limcdel` true and `limdecks` empty: only rows whose card exists (R1341).
- **N3.3** `limdecks` a non-empty list: only rows whose card exists and whose card's current deck (`cards.did`) is neither a listed deck nor a descendant of one (R1340). This holds whatever `limcdel` says. A deleted card's row leaves Home as soon as the list is non-empty, also when the listed decks never held that card, and also when every listed id names no deck. An id that names no deck excludes nothing else. Scenario `limdecks-deleted-card` pins this. The add-on oracle's recording of it confirms the rule; its golden lands with the scenario (`specs/conformance/stats-ops.md` H4).
- A descendant of deck `D` is a deck whose full name starts with `D`'s full name followed by `::`. `Excl::Sub` descends from `Excl`; `Exclusive` and `Keep::Excl` do not (scenario `limdecks-subtree`). Names are read at query time, so renaming, moving or deleting a deck changes what is excluded at once (R304).
- N3.3 reads the card's **current** deck: a card sitting in a filtered deck is judged by the filtered deck, as the add-on does. Deck pages use the home deck instead (N4). A product decision to use the home deck on Home too would be a new deviation.
- Each counted row adds 1 to its heatmap day (`stats/day-buckets` B4, with B4.3 at query time). Days with no counted row are absent.

## N4. Deck scope

A deck page's heatmap and streak stats for deck `D` count the rows N2 keeps whose card exists and whose home deck is `D` or a descendant of `D` (R1283, R1284). The home deck is `odid` when it is not 0, else `did`.

- Deleted cards' rows never count here. `limcdel` and `limdecks` do not apply (R1340, R1341). `limresched` does.
- Moving a card to another deck moves all its rows between deck scopes at once. Home does not change unless N3.3 applies (scenarios `deck-scope-move`, `limdecks-subtree`).
- The add-on reads only `did`. Meguri's home-deck rule is the deviation of R1284. It shows only for cards in filtered decks: `conformance/deviations.yaml` entry `heatmap-deck-home-deck`, scenario `filtered-deck-card` (N9; both added with the oracle support, `specs/conformance/stats-ops.md` H4, as `stats/day-buckets` B4.3's entry is).

## N5. History windows

`limhist` and `limdate` limit which days the heatmap shows. `stats/heatmap-addon` (U96) defines them. They apply to Home and deck scopes when a query runs, and never change the counts N7 keeps. U95's scenarios keep both at 0.

## N6. Studied today

The Home line's two numbers (R1296), as Anki computes them:

- rows: every row in the studied-today window (`stats/day-buckets` B2) whose `type` is neither 4 nor 5 (types 0 to 3, and any other value). Any `ease` counts, deleted cards' rows count (R1281), and every deck counts.
- `cards`: the number of those rows (R1278).
- `time_ms`: the sum of their `time` values as stored, with no cap and no recomputation (R1279).
- Heatmap settings never apply (R1343).

The line's text is `stats/studied-today` (U97). Its seconds are `time_ms / 1,000`.

## N7. Resident aggregates (Meguri)

While a collection is open, the core holds these values. They are built once per open and never released, also on a memory warning (R1286, R1288, R1292):

1. the Home count per heatmap day (N3), under the current `heatmap` settings;
2. counts from which any deck scope (N4) can be summed when a query runs, and from which N3.3 can be applied, without reading the review log. They must be addressable by current deck (`did`, for N3.3) as well as by home deck (N4), for example per (day, card, rated) or per (day, home deck, current deck, rated), where rated is `ease` ≥ 1 (N2). Decided 2026-10-27: this overrides the plan's U95 key list (day, home deck, rated, type), which cannot apply N3.3 to a card in a filtered deck. Scenario `filtered-deck-card` and a property-test case with a card in a filtered deck pin it;
3. a card table: for each card id, whether it exists, its home deck, its current deck, its `type`, its queue, its `due` and its `odue` (used by the forecast, U96, which needs `type` and `odue` for cards in filtered decks, `stats/heatmap-addon` A3 items 1 and 3, and by scopes, U98);
4. the studied-today count and time (N6) for the current window.

A query for Home or a deck returns that scope's day → count map (N3 or N4, with `stats/day-buckets` B4.3 applied) and its earliest day, or no day when the map is empty. It also returns `T` (`stats/day-buckets` B4.2) and `today_changes_at`, the instant `T` next changes (`stats/day-buckets` B6, epoch milliseconds), and every query built on it (`stats/heatmap-addon` A11) passes both on. `heatmap_totals` observes the map (`specs/conformance/stats-ops.md` H1). How the heatmap and streaks use the earliest day is U96's. A deck id that names no deck gives the error `unknown_deck`, as `stats/heatmap-addon` A11 does; its host handles it as A11's.

Reads while the values are not ready. Decided 2026-10-27 (no wrong number is ever shown, and R1818 keeps Home interactive during a rebuild): a Home, deck or studied-today query whose resident values do not yet hold the state it reads (a rebuild at open after a key mismatch, after `CollectionReplaced`, an external change, a zone change or a change of `rollover` or `heatmap`, or a catch-up) does not wait and returns no values. Its result is `building`, with the progress of the aggregates' derived-store component (`storage/derived-store` D8) when there is one. It never returns zeros, old values or a partial map. The host asks again at that component's next `Ready` or `DerivedApplied` event (D8). Until then U228 hides the studied-today line and the summary streak line, and the heatmap shows no counts and no streak values; neither shows a placeholder number. The scenario adapters of `heatmap_totals` and `studied_today_raw` (`specs/conformance/stats-ops.md` H1, H2) wait until the values hold the step's state before they read, so `set_config rollover` followed by `observe` never races the rebuild. An adapter that still finds them not ready after 60 seconds fails the step.

Contract:

- **Equivalence.** Wherever a caller can read them, every value equals what N2 to N6 give for the collection's rows and settings at that moment, the current zone, `R`, `N` and now. This holds across answers and their undo and redo (undoing an answer removes its row; redoing restores it), incoming changes (CloudKit sync between Meguri devices; a manual Anki transfer, KTD26, or an import that replaces the whole collection), card deletion and moves, deck renames, reparenting and deletion (local or incoming), changes to the `heatmap` and `rollover` keys, zone changes, rollover, raw writes and changes made by another process (R1453). It also holds when such a change lands while the values are being rebuilt.
- **Fresh after each operation.** A read that follows an operation's result includes that operation (R304).
- **Never stale.** Values kept in Meguri's derived store are used at open only when they were computed from the same collection state, settings, zone and rollover hour, and also:
  - the studied-today values (item 4) only for the same `N` (the same Anki day); with another `N` they are recomputed with a range read of the new window, and the other values stay valid;
  - the heatmap-day values (items 1 and 2) only for the same version of the bundled time zone database (`stats/day-buckets` B5) and, for a zone id that database does not know, the same fallback UTC offset. While the collection is open, a change of that fallback offset is a zone change.

  Decided 2026-10-27: these inputs are part of U95's own validity key, next to the plan's collection mark, rollover, zone id and `heatmap` hash; they are not a component version bump (`storage/derived-store` D5.2), since they change without any change of the stored format. Otherwise the values are rebuilt before anything shows them. A value computed for another state is never shown, not even briefly.
- Anki sees none of this. It is Meguri's design for R1286 to R1288 and R304, tested by property tests against a fresh computation and by the scenarios through `heatmap_totals` and `studied_today_raw`.

## N8. Read only

Building, keeping, storing and querying these values never writes `collection.anki2` (R1295). No row, `usn`, `mod`, `col.mod` or config key changes, and an absent `heatmap` key stays absent (`stats/heatmap-config` K3). Scenario `read-only` pins this with a dump before and after the observations.

## N9. Scenarios

`conformance/scenarios/stats/heatmap-totals/`, with the values N2 to N6 give. Each scenario gets a golden recorded from the oracles when the scenarios land (`specs/conformance/stats-ops.md` H4); the goldens are then authoritative. Each equals this table, with no deviation except in `filtered-deck-card` (N4, `heatmap-deck-home-deck`). All rows are on 2026-10-12 in Europe/Paris, rollover 4.

| Scenario | Pins | Values |
|---|---|---|
| `row-kinds` | N2, N6 table | Home 6, then 10 with `limresched` false; studied 7 rows, 922,000 ms |
| `partial-config` | `stats/heatmap-config` K3 with a deleted card's rated row and Set Due Date row | Home 1; `{limresched: false}` 2; `{limcdel: true}` none; both none; `{}` 1; `{mode, colors}` 1 |
| `read-only` | N8 | the two dumps are equal |
| `ae6-manual-rows` (AE6) | N2 to N4 | Home/Lang 10/10; `limresched` false 55/55; after deleting a reviewed card 55/54; `limresched` true 10/9; studied 10 rows, 30,000 ms throughout |
| `ae53-deleted-card` (AE53) | N3.1, N4, N6 | Home/Lang 50/50, after the deletion 50/40; studied 50 rows, 400,000 ms both times |
| `limcdel` | N3.1, N3.2, N4 | Home/Lang 2/1, then 1/1 |
| `limdecks-subtree` | N3.3, N4 | Home/Excl/Keep 5/2/2; with `limdecks` [Excl] 3/2/2; after moving the Keep card into `Excl::Sub` 2/3/1 |
| `limdecks-deleted-card` | N3.3 | Home 2; with `limdecks` [Other] 1; with [1234] 1; with [] 2 |
| `deck-scope-move` | N4 | Home/A/A::B/C 3/1/1/2, then 3/2/1/1 |
| `filtered-deck-card` | N3.3, N4, N7 item 2, deviation | one rated row on a card whose home deck is `Lang` and which sits in filtered deck `Cram` (a `db_exec` deck row, `specs/conformance/deck-ops.md` K13), one on a card in `Lang`: Home/Lang/Cram 2/2/none (oracle: 2/1/1); with `limdecks` [Lang] Home 1; with [Cram] Home 1 |
| `answers-and-deletes` | N7 through real answers and a deletion | Home/Lang none, 1/1, 2/2, 2/1; studied 0/0, 1/5000, 2/12000, 2/12000 |

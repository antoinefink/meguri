# Oracle driver: Card Info and graph data (U92, U98, U99, U101)

Read at tag 26.09.3 to choose the `anki` Python and backend APIs behind
`card_stats` and `stats` (`driver/ops/stats.py`; meanings in
`specs/conformance/stats-ops.md` H9 and H10, `specs/stats/card-info.md` C3,
C6.1, C13, `specs/stats/graphs-common.md` G14, `graphs-revlog.md` V8 and
`graphs-cards.md` C12). No code was copied. The specs' own sources are in
`stats.md`; the studied-today kinds are `driver-studied-today.md`. This note
was first named `driver-stats.md`; that name is the U95 heatmap report on
`spec/U95` that `stats.md` cites. The heatmap
kinds (H1, H11) and `stats_presentation` (U226, a headless graphs page) are
not in the driver.

| Anki file | Read for |
|---|---|
| `proto/anki/stats.proto` | the members of `CardStatsResponse` and its revlog entry (which are `optional`, the review kind enum, the f32 fields), the graphs request (search text, days) and every message of `GraphsResponse` with its field names (`newCards` in `Counts`), map key types and which field holds which graph (`difficulty` is field 11, an `Eases` message; `eases`, field 5, is left unobserved) |
| `pylib/anki/collection.py` | `card_stats_data` is the backend's card stats call; `find_cards` with `order=False` is the plain card search; `build_search_string` writes a deck node's search |
| `rslib/src/stats/card.rs` | what card stats reads and when it fails (card, note, note type, deck, home deck, home preset, model and cutoff checks), that it writes the card back with an `lrt` when the data has none, that the due estimate adds whole days to the current time, and that a retrievability is given only for a stored state |
| `rslib/src/stats/graphs/mod.rs`, `rslib/src/stats/service.rs` | the graphs request runs the search into a temporary table and then reads rows and cards in one call; every sub-message of the response is set; the history days move only the start of the rows read (0 reads all) |
| `rslib/src/stats/graphs/card_counts.rs`, `retention.rs`, `hours.rs`, `buttons.rs` | the count messages are always present, and the hour and button lists always hold 24 and 4 entries, so writing set sub-messages and every list (G14's default) equals V8's rule |
| `rslib/src/storage/revlog/mod.rs`, `rslib/src/storage/sqlite.rs` | a review-log or card row the typed read refuses raises a database error, which pylib raises as `DBError`; a search error is a `SearchError`, a bad regex an `InvalidInput` |

`stats` runs the scope's search text alone first, mapping its errors through
`observe/search.py`, so that a database error raised by the graphs call
afterwards can only come from reading rows (`unreadable_row`, G14).

Probed in the oracle image (`driver/tests/test_stats.py`, goldens of
`stats/card-info/` and `stats/graphs/`, 2026-10-27, lock `2322165135…`): a
review-log row with `ease` 300, `factor` −1 or `lastIvl` 3,000,000,000 dated
outside the year window fails only the `all` request; the revlog table
refuses a NULL `lastIvl` (NOT NULL), so G15's NULL case cannot be stored; a
REAL or out-of-range `type` reads as Learn and `type` 260 as Manual; a card
whose deck is missing makes card stats fail, and so does a home preset that
is missing (the preset name falls back to Default, but the history replay
reads the preset again without a fallback); card stats on a card whose data
has no `lrt` leaves `col.mod` unchanged and adds only the `lrt`.

Read, not probed (a panic prints Anki's file and line to implementer-readable
logs, `oracle/AGENTS.md`): card stats on a card whose home deck (`odid`, or
`did` when `odid` is 0) is filtered unwraps the missing preset id and panics,
so the harness reports `worker_died`; no scenario observes one.

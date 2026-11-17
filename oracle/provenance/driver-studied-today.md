# Oracle driver: studied-today observations (U95, U97)

Read at tag 26.09.3 to choose the `anki` Python and backend APIs behind
`studied_today_raw`, `studied_today_text`, `studied_today_format` and
`today_stats` (`driver/observe/studied_today.py`; meanings in
`specs/conformance/stats-ops.md` H2 and H5 to H7). No code was copied. The
specs' own sources are in `stats.md`. The heatmap kinds (H1, H11) and the
graph kinds (H9, H10) are not in the driver yet.

| Anki file | Read for |
|---|---|
| `rslib/src/stats/today.rs`, `rslib/src/storage/revlog/studied_today.sql`, `rslib/src/storage/revlog/mod.rs` | the Home line counts rows after `next_day_at - 86400` (in ms), leaves Manual and Rescheduled rows out, and sums `time` divided by 1000.0 |
| `rslib/src/scheduler/service/mod.rs`, `proto/anki/scheduler.proto` | the studied-today message takes a 32-bit card count and seconds as a 64-bit float, which the core narrows to 32 bits; it is the same function the line uses, reads no collection data, but is served only with a collection open |
| `pylib/anki/scheduler/base.py` | `_timing_today` gives `next_day_at` |
| `pylib/anki/collection.py` | `studied_today` is the backend's Home line for the open collection; `build_search_string` turns one `SearchNode` into the search text Anki writes for it |
| `pylib/anki/lang.py`, `pylib/anki/_backend.py` | `without_unicode_isolation` removes U+2068 and U+2069; a backend opened without languages takes `current_lang`, which stays `en` while nothing calls `set_lang` |
| `proto/anki/stats.proto`, `rslib/src/stats/service.rs` | the graphs request (search text, history days) and the nine members of its `Today` message |
| `rslib/src/stats/graphs/mod.rs`, `rslib/src/stats/graphs/today.rs` | a search that is empty after trimming white space reads every review-log row, deleted cards' included; any other search reads the rows of the cards it matches; the history days only move the start of the rows read, always before today |
| `rslib/src/scheduler/mod.rs`, `rslib/src/config/mod.rs` | computing the day (the Home line and the graphs both do) writes the config key `localOffset` when the stored offset differs from the zone's current one |

`studied_today_raw` reads the two numbers with the same row selection and
keeps them only when the backend's message renders Anki's own line from them
and a different line from one card more.

Probed in the oracle image (`driver/tests/test_studied_today.py`): the
generated backend's `graphs(search=, days=)` and
`studied_today_message(cards=, seconds=)` signatures; the search Anki writes
for a deck node escapes `*` and `_` in the name (`deck:x_y` as typed also
matches `xzy`); a fresh collection in a zone off UTC has no `localOffset`
until the first day computation writes it. Goldens of `stats/studied-today/`
and `stats/today/` re-recorded 2026-10-27 at lock `2322165135…` equal the
U97 recordings value for value.

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| covered-existing link of h19b87e6f451b to stats/studied-today/format-table | `rslib/src/scheduler/service/mod.rs` (test module) | Confirming the harvested test exercises the studied-today message for supplied counts, which studied_today_format observes. |
| Harvest row hdb3d8a73ac7b mapped to existing stats/studied-today/zero-line | `rslib/src/scheduler/service/mod.rs` | Which behavior the fresh-collection studied-today test requires (a zero line). The expectation comes from the existing golden. |

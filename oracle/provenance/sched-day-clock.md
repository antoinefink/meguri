# Scheduler day clock and rollover (U40)

Spec-writer notes for `specs/sched/day-clock.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Black-box probes ran through
`tools/mg oracle record` on throwaway scenarios (deleted afterwards); the
committed scenarios under `conformance/scenarios/sched/` repeat every finding
the spec relies on.

| Spec section | Anki files read | Read for |
|---|---|---|
| D2 | `rslib/src/config/mod.rs` | key names, typed reads (rollover as an 8-bit unsigned value capped at 23, offsets as 32-bit signed minutes west, lastUnburied and collapseTime as 32-bit unsigned), defaults |
| D3, D4 | `rslib/src/scheduler/timing.rs`, `rslib/src/timestamp.rs` | day count with and without a creation offset, the fixed-offset boundary, the rounding and the 32-bit wrap of the legacy count, next boundary with `<=` versus `<` |
| D5, D6 | `rslib/src/scheduler/mod.rs`, `rslib/src/config/mod.rs`, `rslib/src/config/undo.rs`, `rslib/src/undo/mod.rs`, `rslib/src/collection/transact.rs` | the cached day and what drops it, the rollover repair write, the localOffset write, undo saves ignored outside an operation, `col.mod` set only by operations |
| D7 | `rslib/src/scheduler/bury_and_suspend.rs`, `rslib/src/scheduler/filtered/card.rs`, `rslib/src/search/sqlwriter.rs`, `rslib/src/scheduler/service/mod.rs`, `rslib/src/decks/tree.rs`, `rslib/src/sync/collection/normal.rs`, `rslib/src/sync/collection/start.rs` | the lastUnburied window, queue from type and due, raw card update, which calls run the check |
| D9, D12.4 | `rslib/src/scheduler/queue/mod.rs`, `qt/aqt/main.py` | the unconditional unbury and undo clear when a study queue from another day is reused; the desktop's timer at the boundary |
| D11 | `rslib/src/storage/sqlite.rs`, `rslib/src/config/schema11.rs`, `rslib/src/scheduler/timing.rs` | crt at creation, creationOffset at the crt instant, keys a new collection holds |
| D12.3 | `pylib/anki/collection.py`, `rslib/src/preferences.rs`, `rslib/src/scheduler/upgrade.rs` | the desktop layer adding creationOffset on open |
| D14.2 | `pylib/anki/scheduler/base.py`, `pylib/anki/scheduler/v3.py` | which Python calls reach the day query with its unbury |

Probe findings (oracle 26.9.3, Europe/Paris unless noted):

- The `timing` observation runs the unbury check; `card`, `answer`, `next_states`, `add_note` and `reopen` do not. `answer` does not rebuild study queues, so the queue-path unbury (D12.4) never shows.
- The day query's writes (`rollover`, `localOffset`, `lastUnburied`) stamp the row and leave `col.mod`; they persist across a reopen. The unbury keeps card `mod` and `usn`.
- `set_config` of `rollover` keeps the cached day until its boundary; `reopen` drops it. A DST change keeps the cached day too (spring forward: day 11 starts at 05:00 CEST instead of 04:00).
- `reopen` of a collection without `creationOffset` adds it (usn -1, the offset at crt).
- rollover 24 and 255 read as 23 and stay stored; 256, -1, 4.0, "6", null and true read as 4 and are rewritten as 4. creationOffset -60.0, "-60" and 1440 give legacy timing; 1439 gives new timing.
- A new collection in UTC gets no localOffset (missing reads as UTC).
- Legacy timing one or more days before crt's boundary gives 4294967294-style values; at the boundary second next_day_at equals now.
- Creation at 03:30 CEST on 2030-03-31 gives crt 1901066400 with creationOffset -60; at 03:30 CET on 2030-10-27, crt 1919214000 with -120.

Harvest rows classified for this spec (`oracle/harvest/links.csv` names the tests): `h419067ffeea3`, `hd79744504c35`, `hdeb239c0cf40`, `hb4123149e0de`, `h4dcd915bda8e`, `ha338afcecf02`, `h0348591aa08e`. The other timing-related rows (`h69f8e4b9fca9` learning order, `h9ee8c470c9b4` learning across days, `h1f57b66e6c57` bury ops) belong to the queue, answering and bury units.

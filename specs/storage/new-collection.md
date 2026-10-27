# A new collection

Spec: storage/new-collection
Requirements: R161, R913, R596, R1449, R2016
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/new-collection/
Status: Approved 2026-10-27 (R2066) for the iCloud bullet of B1, with `sync/cloudkit-recovery`. Every other section is in force and unchanged.

What a collection that Meguri creates holds ("Start Empty", R161, and every other path that needs an empty collection), so that it equals, row for row, the collection Anki creates at the same instant in the same time zone, apart from the stock note types' template text and CSS (R908). The file format is `storage/schema-18`; the day-clock values are `sched/day-clock` D11; the stock note types `notetypes/stock-kinds` K2; the deck and preset records `decks/decks` D4 and `decks/deck-presets` P2. "The creation instant" `t` is the clock's current instant when the collection is created: `now_ms` and `now_s` below are taken at `t`.

## B1. When

- The bootstrap runs only to create a collection where none exists: R161's "Start Empty", and the empty collection that a first sync then fills (R158). It never runs over an existing `collection.anki2`.
- For iCloud (wording of 2026-10-27; the bootstrap itself does not change): "Start Empty" on a device that connects to an account with no iCloud collection makes this collection, which is then published as a new iCloud collection (`sync/cloudkit-recovery#V13.3`, first row). Connecting a device with no collection to an existing iCloud collection (R158, V13.3 second row) does not run the bootstrap and does not fill a bootstrapped collection: the device stages the iCloud collection as a new file (`sync/cloudkit-records#K18.4`) and activates it whole through `engine/collection-replace` (`CloudAdopt`). Its rows come only from the iCloud collection. The words "the empty collection that a first sync then fills" above do not apply to iCloud; they stay as written for manual Anki transfers (frozen; KTD26).
- It creates the collection file only. The media folder and Meguri's own files next to the collection are their owners' (U31, U26).

## B2. The file

- `collection.anki2` with the header facts and the tables and indexes of `storage/schema-18` S1 to S3, exactly: page size 4096, UTF-8, auto-vacuum off, the twelve tables and sixteen indexes with their normalized text. Nothing else in `sqlite_master` except SQLite's statistics tables (S6.2).
- Everything below is written in one transaction. Then `ANALYZE` runs (`storage/schema-18` S5), so the file holds `sqlite_stat1` and `sqlite_stat4`, as Anki's new collections do.
- The file is in WAL mode from then on (S1).
- An interrupted bootstrap never leaves a `collection.anki2` that is not a complete new collection: a later open must either find no file or this whole content. (Building under a temporary name in the collection directory and renaming it once committed and checkpointed is one way.)

## B3. The `col` row

| Column | Value |
|---|---|
| `id` | 1 |
| `crt` | `sched/day-clock` D11 at `t`: 04:00 of the local day of `t`, or of the day before when `t` is before 04:00 |
| `mod` | 0. Nothing the bootstrap writes moves it; the first operation does (`storage/ids-usn-mtime` S5) |
| `scm` | `now_ms` |
| `ver` | 18 |
| `dty`, `usn`, `ls` | 0 |
| `conf`, `models`, `decks`, `dconf`, `tags` | the empty string |

Example (oracle): created 2030-01-15 03:30 in Europe/Paris: `crt` 1894590000 (2030-01-14 04:00 +01:00), `scm` 1894674600000, `mod` 0. Created 2030-01-15 10:00 UTC: `crt` 1894680000, `scm` 1894701600000.

## B4. Config keys

Sixteen keys, each value stored as a BLOB in the compact form of `storage/config-table` C5:

| Key | Value | `usn` | `mtime_secs` |
|---|---|---|---|
| `activeDecks` | `[1]` | 0 | 0 |
| `addToCur` | `true` | 0 | 0 |
| `collapseTime` | `1200` | 0 | 0 |
| `creationOffset` | the offset in effect at `crt`, in minutes west (`sched/day-clock` D11): `-60` in Europe/Paris in winter, `0` at UTC | 0 | 0 |
| `curDeck` | `1` | 0 | 0 |
| `curModel` | the id of the stock Basic note type (`notetypes/stock-kinds` K2), a JSON integer | -1 | `now_s` |
| `dayLearnFirst` | `false` | 0 | 0 |
| `dueCounts` | `true` | 0 | 0 |
| `estTimes` | `true` | 0 | 0 |
| `newSpread` | `0` | 0 | 0 |
| `nextPos` | `1` | 0 | 0 |
| `sched2021` | `true` | 0 | 0 |
| `schedVer` | `2` | 0 | 0 |
| `sortBackwards` | `false` | 0 | 0 |
| `sortType` | `"noteFld"` | 0 | 0 |
| `timeLim` | `0` | 0 | 0 |

- No other key in the factory output. In particular there is no `rollover`, `localOffset` or `lastUnburied`. An explicit compatibility day query (`sched/day-clock` D5, D7), on Meguri or the oracle, writes `rollover` = `4`, and `localOffset` unless the device is at UTC, each with `usn` -1 and `mtime_secs` `now_s`, without moving `col.mod` (oracle: Europe/Paris gives `localOffset` `-60`). On the creation day it writes no `lastUnburied`: today is day 0 and a missing key reads as 0; on a later day that query writes the key even with no buried cards. Meguri's automatic engine open instead follows D8: it uses the effective rollover default 4 without storing it, writes `localOffset` only for a real offset difference (R594), and leaves `lastUnburied` absent when no cards are restored. These post-bootstrap policies do not change the sixteen factory keys or the explicit-query golden comparisons.
- `curModel` is the only key written as a local change; it still leaves `col.mod` at 0.

## B5. The default preset

One `deck_config` row: `id` 1, `name` `Default`, `mtime_secs` 0, `usn` 0, `config` the built-in defaults of `decks/deck-presets` P2, encoded per `model/protobuf-records` P3. Its bytes are those of P6.6's example (oracle, hex):

```
0a080000803f00002041 120400002041 221c0000803f0000803f0000803f0000803f0000803f0000803f0000803f
4814 50c801 5d00002040 656666a63f 6d9a99993f 7d0000803f 8001949d02 880101 900101 980104
a80101 b00108 c0013c ad026666663f c5026666663f e00201
```

## B6. The default deck

One `decks` row: `id` 1, `name` `Default`, `mtime_secs` 0, `usn` 0, `common` `08 01 10 01` and `kind` `0a 02 08 01` (`decks/decks` D4: collapsed in both scopes, normal, preset 1). Meguri always names it `Default`; Anki takes the name of the default deck and preset from the user interface language, `Default` in English.

## B7. Note types

The six stock note types of `notetypes/stock-kinds` K1, in K1's order, as K2 says: ids `now_ms` to `now_ms` + 5, `notetypes` and `templates` rows with `mtime_secs` 0 and `usn` 0, Meguri's own template text and CSS (R908). Anki 26.09.3's new collection has the same six, Image Occlusion included, so a Meguri collection needs no extra Image Occlusion type to match (R913; the on-demand add of `notetypes/records` N9 is for collections that lack it).

## B8. Empty tables

`notes`, `cards`, `revlog`, `graves` and `tags` have no rows. There is no undo history and the change stream reports nothing for the bootstrap; the collection opens afterwards through the normal open (U22, U24).

## B9. Comparison with Anki

- Compared (goldens): the `col` row, every config key with its stamps and value, the default deck and preset with their bytes, the counts of the other tables, `sqlite_master` (`storage/new-collection/rows`, `dump`); the structure of the stock note types (U29's `notetypes/records/stock-new-collection`).
- Not compared: the stock note types' `config` blobs and template rows, which hold each client's own template text and CSS (R908, R1959).
- Live (T1): Anki opens a collection Meguri created, sees the same rows, adds a note to the default deck and finds no problem with Check Database (`storage/new-collection/handoff`).

## B10. Scenarios

| Scenario | Covers |
|---|---|
| `storage/new-collection/rows` | B3, B4, B5, B6, B8 in Europe/Paris before 04:00 (golden recorded) |
| `storage/new-collection/dump` | B2 to B8 at UTC: every row of the tables without stock text, and `sqlite_master` (golden recorded) |
| `storage/new-collection/handoff` | live: Anki works in a Meguri-created collection (golden recorded) |

Meguri's tests (`core/crates/meguri-engine/tests/bootstrap.rs`) also cover B1 (never over an existing file), B2's interrupted bootstrap, and B3's `crt` and `creationOffset` across a DST change (`sched/day-clock` D11's table).

## B11. The scenarios' new collection

- Meguri's conformance executor makes the new collection of `conformance/dsl` D8 with this bootstrap, at `clock.start` in the scenario's `clock.tz`, then opens it through the normal open (B8). It replaces the stand-in of `conformance/notetype-ops` T10.2, which writes a subset of these rows with the same values; the stand-in goes once the bootstrap exists.
- Every scenario that starts from the new collection then starts from the rows Anki's oracle starts from. Besides this area's and the decks area's, these include the undo scenarios, the day and unbury scenarios (`sched/day/`, `sched/unbury-on-rollover/`), which read `crt` and `creationOffset` (B3, B4), and `browser/config/new-collection`, which reads `sortType` and `sortBackwards` (B4).

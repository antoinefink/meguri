# Check Database

Spec: maintenance/check-database
Requirements: R1639, R1640, R1641, R1642 (the card, deck, review-log and position repairs), R1643 (the card, review-log and id repairs), R688 (co-covered, C8.7), R725 (C14), R1489 (C14), R1494, R1847, R1924, R1925; supports R169, R171, R1631, R2008, R2020, R1657
Anki: 26.09.3 for C4's check, C6's statements, C8 to C10 and C11's `scm` rule; C2, C3, C5, C7, C12 to C16 are Meguri design (plan U128)
Scenarios: conformance/scenarios/maintenance/check-db/

Check Database checks the collection file, compacts it, then repairs rows that other programs, old clients or raw writes left inconsistent, and reports what it repaired. The oracle's view of it (the problem kinds, their counts and the `check_db` scenario op) is `conformance/collection-check` K3 step 7, K5 and K10; this spec does not repeat K5's table but gives, for each kind, the rule, the stamps and the order Meguri must reproduce. The notes stage (note types, notes, tags and card generation) is `maintenance/check-database-notes` (U129). The integrity verdict and the optimize statements are `maintenance/optimize-integrity` M6 and M3.

Terms: "now" is the repair instant (C7): `now_ms` in epoch milliseconds, `now_s` = `now_ms` divided by 1,000, rounded down; "today" is the day clock's `days_elapsed` at that instant (`sched/day-clock`). "Local stamps" are `usn` -1 and the modification time `now_s` (`storage/ids-usn-mtime#S2`). "Round" is SQLite's `round()`: to the nearest integer, halves away from zero. "Clamp to [a, b]" replaces a value below a by a and above b by b.

Text and blob values follow SQLite's conversion rules, which the repairs must reproduce (they appear only after raw writes, since an integer column converts a numeric text on store):

- Arithmetic (`+`, modulo), rounding and truncation use the value's longest numeric prefix, 0 when there is none; a blob is read as its bytes' text. So `'12abc'` and the blob `x'3132'` both count as 12, and `'abc'` as 0.
- In a comparison, and in choosing the smaller or larger of two values for a clamp, any text or blob is greater than every number and never equal to one.

Per step: in C8.1 a text or blob `due` of a new card (queue not 4) passes "1,000,000 or more" and becomes 1,000,000 + (prefix modulo 1,000,000), counted in `new_positions_too_high` (`'abc'` gives 1,000,000); in C8.2 a text or blob `due` of a queue-2 card is "above 100,000" and becomes today; in C8.3 a text or blob `odue` is "above 0"; in C8.6 a text or blob `ord` becomes 30,000; in C10.1 a text or blob `time` is not 0 for the `type` test. Every other branch replaces the value by its prefix, rounded (C8.5: truncated) and clamped.

## C1. What Meguri adds to Anki's command

Anki's desktop command makes a forced backup, then runs the integrity check, the optimize, and every repair in one transaction, then shows the list of fixes (or that the database was rebuilt and optimized). Meguri keeps the checks, the repairs, their order, counts and stamps, and adds:

- an engine op with an input, a report and typed errors (C2, C12, C13);
- the integrity check before the safety backup, so a damaged file never enters the backup rotation (C3, C5);
- lifecycle rules: it runs from `Ready` and from `Failed{Corrupt}` (C2, C4);
- stage progress and a cancel point (C15);
- concurrency with review and the local servers (C16);
- the records of the maintenance status (C14).

## C2. The op

- Entry point: `check_database()`, a job (`engine/jobs` J1) with no arguments. It returns `JobHandle{id, cancel, progress}` at once, or a refusal (below) without running. The job ends with one `JobFinished` whose outcome carries the report (C12) or the error (C13). U133 exposes it to Swift; U163 starts it from the Maintenance page and from the damaged-collection screen.
- The repair write is the catalog op `check_database` (`engine/op-model` O9: destructive, not undoable, long write, write not cancellable), source `Maintenance`. The optimize statements are the maintenance tasks `vacuum`, `reindex`, `analyze` and `wal_checkpoint` (O8, O9).
- States. Allowed in `Ready` and in `Failed{Corrupt}` (`engine/lifecycle` L4, L5). In any other state the start is refused with `CollectionUnavailable{reason}`; it is never parked.
- Slot. The job takes the long-operation slot when it starts (`engine/lifecycle` L5, `engine/jobs` J3); when it is held, the start returns `Busy{running_op}`. It holds the slot until the job ends. It never enters `ExclusiveBusy` (L9).
- Confirmation (R2020). The op is destructive and not undoable, so the UI asks before it starts the job (U163). The core takes no confirmation token. Decided 2026-10-27: Anki asks nothing; R2020 requires the question, and its place is the UI that offers the command.
- A review session may be open; it stays usable (R1494, C16).

## C3. Steps, in order

1. **Integrity** (C4), on the maintenance reader.
2. **Safety backup** (C5).
3. **Optimize** (C6), as maintenance tasks on the writer.
4. **Repairs** (C7): one `check_database` op with the stages cards (C8, C9), notes (`maintenance/check-database-notes`) and history (C10), in that order.
5. **After the commit** (C14): undo, change set, derived store, records, slot release.

Anki runs the backup first and the integrity check second. Meguri runs the check first (plan U128): a corrupt verdict then makes no backup, as at Anki's close (`maintenance/optimize-integrity` M7). A sound file is backed up before anything writes it, as in Anki.

## C4. Integrity stage (R1640)

- `PRAGMA main.quick_check` on the maintenance reader, with `maintenance/optimize-integrity` M6's verdict table. Anki uses the same check at this point. `integrity_check` is not used: the optimize's `REINDEX` rebuilds every index, which is what `integrity_check` adds over `quick_check`.
- The same reader also runs `storage/schema-18#S6.1`'s expected-object check; its `missing_indexes` go into the report (C12). Check Database does not create them; Anki's does not either.
- **Sound**: `lastQuickCheck` records `passed: true` (M5.1). In `Failed{Corrupt}` the state returns to `Ready` with the same generation (`maintenance/optimize-integrity` M7.2, `engine/lifecycle` L4). Then C5.
- **Corrupt**: the job ends with `Corrupt` (C13), the rejection `corrupt` of `conformance/collection-check` K10. Nothing is written to the collection, no backup is made and no optimize runs. `lastQuickCheck` records `passed: false`, `lastCheckDatabase` = `{instant, result: {"error": "corrupt"}}` (C14), and a `Ready` collection moves to `Failed{Corrupt}` (L1, as U132's own check does); in `Failed{Corrupt}` the state stays. The remedies are U132's list (M7.1).
- **No verdict** (`SQLITE_INTERRUPT` from a cancel, `SQLITE_IOERR`, `SQLITE_NOMEM`, `SQLITE_BUSY`): the job ends with `Cancelled` (a cancel) or `CheckFailed{stage: integrity, cause}`; nothing is recorded and the state is unchanged.

## C5. Safety backup (R1631, R2008)

- The forced backup of `package/backup-triggers#T6.1` (row Check Database, always), taken after the integrity stage and before the first write to the file, the optimize included. At this point the state is `Ready` (C4), so it is `package/backups#B4`'s backup; a snapshot that fails with a corruption error becomes T7's byte copy (T7.1).
- Who takes it (this overrides T6.2's executor step for this op): the job itself asks the engine's `BackupService` port for the forced backup after C4 and before the first maintenance task (C6), and waits for it. The `check_database` op's descriptor marks its safety backup as already taken, naming this backup, so the executor takes no second one when it queues the repair op's write phase. T6.2's other rules apply (other ops keep committing, failure, cancel, the record).
- Silent (decided 2026-10-27): no message, notification or progress stage of its own. Progress stays at `integrity` until the backup is published. Anki's backup before Check Database is silent too. The report names the file (C12).
- Failure: the job ends with `SafetyBackupFailed{cause}`; nothing was written (T6.2). Cancel: as T6.2.
- The backup is protected from retention from its publication until the job ends (success, failure or cancel), not only until the repair op ends (T6.2's rule, held by the job).

## C6. Optimize stage (R1641)

- `VACUUM`, `REINDEX`, `ANALYZE`, in that order, each a maintenance task with no transaction open (`maintenance/optimize-integrity` M3, row Check Database), then `PRAGMA wal_checkpoint(TRUNCATE)` (M8.3; a busy result is not a failure). `VACUUM` runs with `temp_store=FILE` (M8.1).
- The statements change no row and no stamp (M2).
- Failure: `SQLITE_CORRUPT` (any extended code) or `SQLITE_NOTADB` is a corrupt verdict, handled as C4's corrupt case (`lastQuickCheck` `passed: false`, `Failed{Corrupt}`), except that the backup already exists. Any other failure (`SQLITE_FULL`, `SQLITE_IOERR`, `SQLITE_NOMEM`) ends the job with `CheckFailed{stage: optimize, cause}`: no repair runs and the collection keeps its rows (M5). Anki stops with the error too.
- This optimize does not write `lastOptimize`: that record and the 14-day cadence belong to the automatic optimize (M4). C14 repairs malformed optimize records (M4.1).

## C7. The repair op

- One `check_database` op: one transaction (`engine/op-model` O3) on the writer. Every repair reads the live rows inside that transaction; nothing is computed on a snapshot (O7, re-derivation), so the op never returns `Stale`.
- It declares a long write (O8), so U43 keeps answers in its overlay while it runs (C16).
- The repair instant: the op reads the U21 clock once, when the writer takes it, and uses that instant as "now" in every stage (C8.1, `maintenance/check-database-notes` N4, C10.4). Anki reads the clock several times within its run; under the oracle's frozen clock the two agree.
- Stamping mode `Raw` for rows (`storage/ids-usn-mtime#S3`): each repair writes exactly the stamps C8 to C10 and the notes stage give, and nothing else is stamped. Rows the op creates (decks, note types, cards, config values) get local stamps. `col.mod` does not follow S3's raw-write row: it follows C11 (S5's rule for an operation).
- Writes that change nothing are skipped (`storage/ids-usn-mtime#S4`). The repairs below select only rows whose value differs, so a skip is never visible in a count.
- A repair that fails rolls the whole op back (C13).
- Deleted cards get no grave (C9.1, `maintenance/check-database-notes` N6.2, N6.3), as in Anki: the schema stamp (C11) makes the next manual Anki transfer one-way, which carries the deletions.

## C8. Cards stage: card properties

The fixes C8.1 to C8.6 run in this order, each over every card. A card changed by several fixes is counted once per fix. Each fix that is marked "stamped" writes local stamps on the cards it changes (a later fix stamps a card again with the same values). Summary (the rules are the subsections below):

| Step | Cards changed | New value | Stamped | Counted in |
|---|---|---|---|---|
| C8.1 | `type` 0, `queue` not 4, `due` 1,000,000 or more, and `due` differs from 1,000,000 + (`due` modulo 1,000,000) | `due` = 1,000,000 + (`due` modulo 1,000,000) | yes | `new_positions_too_high` |
| C8.2, first branch | `queue` 2 and `due` above 100,000 | `due` = today | yes | `card_properties` |
| C8.2, second branch | any other card whose `due` differs from `due` rounded and clamped to [-2,147,483,648, 2,147,483,647] | that value | yes | `card_properties` |
| C8.3, first branch | `odue` above 0, (`type` 1 or `queue` 2) and `odid` 0 | `odue` = 0 | yes | `card_properties` |
| C8.3, second branch | any other card whose `odue` differs from `odue` rounded and clamped to [-2,147,483,648, 2,147,483,647] | that value | yes | `card_properties` |
| C8.4 | `ivl` differs from `ivl` rounded and clamped to [0, 2,147,483,647] | that value | yes | `card_properties` |
| C8.5 | `mod` is not an integer | `mod` truncated toward zero | no | `card_properties` |
| C8.6 | `ord` outside [0, 30,000] | `ord` clamped to [0, 30,000] | yes | `card_properties` |

Text and blob values in these columns follow the Terms' conversion rules.

Example (one new card with `ivl` 1.5, `due` 2,000,000, `odue` 1.5): C8.1 folds `due` to 1,000,000 (`new_positions_too_high` 1), C8.3 rounds `odue` to 2 and C8.4 rounds `ivl` to 2 (`card_properties` 2). A second run finds nothing.

### C8.1 New-card positions

The summary's C8.1 row. A `due` from 1,000,000 to 1,999,999 already equals its folded value, so it is neither changed nor counted (K5). 2,500,000 becomes 1,500,000; 2,000,000 becomes 1,000,000. Cards in queue 4 keep their `due`.

### C8.2 Due

The summary's C8.2 rows, the first branch tested first. C8.2 runs after C8.1, on the folded values. A review card due 200,000 becomes due today.

### C8.3 Original due

The summary's C8.3 rows, the first branch tested first. Anki's first branch is skipped for collections on the v1 scheduler; Meguri never opens those (`storage/open-and-connections` C2, `sched/config-keys`).

### C8.4 Interval

The summary's C8.4 row.

### C8.5 Modification time

The summary's C8.5 row: not stamped, since the fix is of `mod` itself; `usn` stays.

### C8.6 Ordinal

The summary's C8.6 row. An `ord` clamped by C8.6 can still have no template: the notes stage then removes the card and counts it again (`maintenance/check-database-notes` N6.3).

### C8.7 Last review time (R688)

After C8.6, every card gets `sched/memory-state#M6`'s rule: cards that are not new and have review-log rows get the expected `lrt` in their card data. Count: `card_last_review_time`. The rows keep `mod` and `usn` (M6). Scenarios `maintenance/check-db/last-review-time`, `sched/memory/lrt-repair`.

The rule reads only `id`, `type` and `data` of the cards and `id`, `cid`, `ease`, `factor` and `type` of the review-log rows. Anki reads every column of both and stops the whole Check Database with an error, changing nothing, when one of them is out of its stored range: a review-log row whose `ivl` or `lastIvl` (truncated) is outside the signed 32-bit range, whose `ease` is not an integer from 0 to 255, whose `factor` is negative, whose `id`, `cid` or `usn` is not an integer, or a card that has review-log rows and such a column (for example a negative `reps`). Meguri goes on: C10.1 later repairs `ivl`, `lastIvl` and `time`, and every other out-of-range column stays as stored. Deviation `check-db-lrt-reads-needed-columns` (C17), which covers C9.3's reads too.

## C9. Cards stage: cards without a note, missing decks, filtered fields

In this order, after C8:

### C9.1 Cards without a note (R1643)

Every card whose `nid` is the id of no note is deleted, with no grave. Its review-log rows stay. Count: `cards_without_note`. Because this runs before C9.2, such a card never makes a deck.

### C9.2 Missing decks (R1642)

For every distinct `did` of a card that is the id of no deck, in ascending order, a normal deck is created with:

- `id` that `did`;
- name `recovered<did>` (the id in decimal, for example `recovered4242`), with `decks/decks#D2`'s unique-name rule (`recovered4242+` when that name is taken);
- the records of `decks/decks#D4` (collapsed in both scopes, preset 1), local stamps.

Count: one per deck created, in `decks_missing`. Scenario `maintenance/check-db/missing-deck`.

### C9.3 Filtered fields outside a filtered deck (R1643)

Every card with `odid` above 0 whose `did` names a normal deck gets `odid` 0 and `odue` 0. Its other columns stay, `mod` and `usn` included. Each such card is counted in `card_properties`. A card whose `did` names a filtered deck is left alone, whatever its `odid` names.

Meguri reads only the cards' `id`, `did`, `odid` and `odue` here. Anki reads every column of each card it changes and stops the run, changing nothing, when one is out of its stored range (for example a negative `reps`); Meguri goes on and leaves those columns. Deviation `check-db-lrt-reads-needed-columns` (C8.7, C17).

## C10. History stage

After the notes stage (`maintenance/check-database-notes`), in this order:

### C10.1 Review-log values (R1643)

Every review-log row where any of these differs is rewritten with all four new values; `usn` and every other column stay:

| Column | New value |
|---|---|
| `ivl` | rounded, clamped to [-2,147,483,648, 2,147,483,647] |
| `lastIvl` | rounded, clamped to [-2,147,483,648, 2,147,483,647] |
| `time` | rounded, clamped to [0, 2,147,483,647] |
| `type` | 5 when the stored `type`, `time` and `ease` are all 0; otherwise unchanged |

The `type` test reads the stored `time`, before its own rounding. Count: rows rewritten, in `revlog_properties`. Scenario `maintenance/check-db/revlog-properties`.

### C10.2 Missing parent decks (R1642)

Going through every deck name in ascending `unicase` order of the stored name (components joined by U+001F, so a parent comes before its subdecks), with a set of names seen so far (compared under `unicase`):

- the name is added to the set;
- when it has a parent name (all but its last component) that is not in the set, that parent is created as `decks/decks#D3` creates missing ancestors (case adoption, every missing ancestor of the parent too, ids by `storage/ids-usn-mtime#I2`, D4's records, local stamps), the parent name is added to the set, and `decks_missing` grows by one.

So the count is one per parent created for a deck, not one per deck created: `A::B::C` alone creates `A::B` and `A` and counts 1; `X::Y` and `X::Z` without `X` create `X` and count 1. Ancestors created for the parent are not added to the set; a later deck whose parent is such an ancestor finds that it exists, creates nothing and still counts 1 (`A::B::C` and `A::D` alone count 2 and create `A::B` and `A`).

### C10.3 Next new-card position (R1642)

The config key `nextPos` becomes the largest `due` of the cards with `type` 0 (any queue) plus 1, or 0 when there is no such card or that value is negative. It is written as a config value (`storage/config-table`, local stamps) only when it differs from the stored one. Not counted.

### C10.4 Ids in the future (R1643)

- The cutoff is `now_ms` + 86,400,000. An id equal to the cutoff is valid.
- Count, `future_ids`: the rows of `notes`, `cards` and `revlog` whose `id` is above the cutoff.
- When the count is above 0, the tables are re-keyed in this order: `notes` (and every card's `nid` that named an old note id), then `cards` (and every review-log row's `cid` that named an old card id), then `revlog`. In each table the rows above the cutoff, in ascending order of their old id, take the new ids `now_ms`, `now_ms` + 1, `now_ms` + 2, ... No stamp changes; graves are not touched.
- Example (scenario `maintenance/check-db/future-ids`, `now_ms` T): a note at T + 2 days and one at T + 1 day; only the first is counted, and it takes T, whatever larger valid ids exist.
- When a new id is already used by a row of that table, Anki stops with a constraint error and changes nothing. Meguri skips every id already used in the table, so the k-th row takes the k-th unused id from `now_ms` up. Deviation `check-db-future-ids-skip-used` (C17).

## C11. The schema stamp and `col.mod`

- `scm` (R1643, `notetypes/schema-changes#G2` reason `check_database_repair`): set to `now_ms` when at least one of these is above 0: C8.7's count, C9.1's count, the number of cards C9.3 changed, C10.1's count, C10.4's count, or one of the notes stage's `duplicate_card_ords`, `cards_without_template`, `field_counts` and `notetypes_recovered` (`maintenance/check-database-notes` N8). Every other repair leaves `scm` as it was (`conformance/collection-check` K5's last paragraph). The op then reports reason 10 in its structure report (`engine/change-stream#S2.5`, under review with R2066).
- `col.mod`: set to `now_ms` when the op changed at least one row (`storage/ids-usn-mtime#S5`). A run that changes nothing leaves it (R1472). Anki sets `col.mod` on every run. Deviation `check-db-no-change-keeps-mod` (C17). Decided 2026-10-27: R1472 is the requirement that decides this; the plan's test "`col.mod` matches the oracle on a healthy collection" is read as "matches up to this deviation".

## C12. The report

The job's result on success:

| Member | Content |
|---|---|
| `problems` | the twelve counts of `conformance/collection-check#K5`, every kind present, 0 for none; `notetypes_recovered` 1 when at least one note type was recovered, else 0, as K5 |
| `recovered_notetypes` | the names of the note types the notes stage created for missing ones (`maintenance/check-database-notes` N4), in creation order |
| `schema_changed` | whether C11 set `scm` |
| `changed` | whether the op changed at least one row |
| `missing_indexes` | C4's list, as names |
| `device_records_repaired` | the `deviceState` keys C14 replaced because they were malformed |
| `safety_backup` | the backup's file name (C5) |
| `instant` | the repair instant, epoch milliseconds |

- The UI's text is Meguri's own, one line per count above 0 and one per other non-empty member, keyed by member (R1887); with all counts 0 and the lists empty it says that no problems were found (R1639). Anki's messages are never shown or stored.
- The `check_db` scenario op returns `problems` only (`conformance/collection-check` K10).

## C13. Errors and a collection too damaged

| Error | When | Collection |
|---|---|---|
| `CollectionUnavailable{reason}` | started in a state other than `Ready` and `Failed{Corrupt}` (C2) | unchanged |
| `Busy{running_op}` | the slot is held (C2) | unchanged |
| `Corrupt` | C4's or C6's corrupt verdict | unchanged; `Failed{Corrupt}` |
| `SafetyBackupFailed{cause}` | C5 | unchanged |
| `CheckFailed{stage, cause}` | C4's check gave no verdict; C6 failed without a corrupt verdict; or a repair failed | unchanged (a C6 statement that completed changed no row) |
| `Cancelled` | C15 | unchanged |

- A repair fails when a row it must read cannot be decoded (`model/row-records`' typed error, naming the table, column and key: an unreadable deck or note type record, a note whose `guid` or other text is not valid UTF-8 beyond `flds`, `tags` and `sfld`, which `maintenance/check-database-notes` N6.1 repairs) or a statement fails (`SQLITE_FULL`, `SQLITE_IOERR`). The `check_database` op rolls back as a whole, so nothing of it is committed, and the job ends with `CheckFailed{stage, cause}`, `stage` being `cards`, `notes` or `history`. Anki also stops at such a row and changes nothing. No later run gets past such a row; the remedies are restore and the iCloud replace (`maintenance/optimize-integrity` M7.1). The safety backup stays.
- A process killed during the repair op leaves the last committed file: the transaction is not in it.
- U133a maps these to its FFI error set.

## C14. After the commit

- **Undo** (R725): both histories are emptied with `ClearedByNonUndoableOp` (`engine/undo`), also when the op changed nothing, as Anki does and as the `check_db` scenario op does (`engine/undo` M6). A run that ends before the repair op leaves them.
  - One exception in Anki: when the notes stage recovers a note type (`maintenance/check-database-notes` N4), Anki adds it through an undoable add, so its undo history holds one "Add Note Type" step after the run. Decided 2026-10-27: Meguri empties both histories in that case too (data safety: undoing that step would remove the note type the moved notes now use). The scenarios that recover a note type do not observe `undo_status`, so no golden shows this; it is listed in C17's last paragraph.
- **Change set**: the op's change set covers every row it changed (`engine/change-stream`), published after the commit.
- **Derived store** (R1489): every component is marked `Stale` and rebuilt (`storage/derived-store`), whether or not a row changed.
- **Records** (`engine/collection-replace#R6.1`, U32's sidecar):
  - `lastCheckDatabase` = `{instant, result}` with `result` the `problems` object, after the commit; `{instant, result: {"error": "corrupt"}}` after a corrupt verdict (C4, C6). Any other error and a cancel record no `lastCheckDatabase`.
  - Check Database is the repair path for malformed maintenance records (decided 2026-10-27, from `maintenance/optimize-integrity` M4.1). A malformed `lastQuickCheck`, `lastCheckDatabase` or `lastBackup` is replaced by the value this run writes to it (C4, this list, the safety backup's record). After the commit, a malformed `optimizeCadenceStart` is replaced by `now_ms`; a malformed `lastOptimize` is removed and `optimizeCadenceStart` set to `now_ms`, so the 14-day cadence starts again from the repair (M4.1). Each replaced or removed key is listed in `device_records_repaired`. The replacement goes through the sidecar's explicit replace of a malformed record, not its ordinary setter, which refuses to replace one (R6.1).
- The status record (`maintenance/optimize-integrity` M10) shows `lastCheckDatabase`.
- **Slot**: released before `JobFinished`; one `Available` event follows (`engine/lifecycle` L6).

## C15. Progress and cancellation

- Progress phases (`engine/jobs` J5), R1639's stages: `integrity` (also during the safety backup), `optimize`, `cards`, `notes` (completed: notes done; total: all notes), `history`. The last three run inside the repair op's write phase: when the writer takes the op, the sink reports `cards` in place of `engine/op-model` O5's `Committing`, then `notes` and `history`, and none of the three offers Cancel (they stand for `Committing` in J5's sense).
- Cancel before the writer takes the repair op: during the check (the reader is interrupted: no verdict, C4), during the backup (T6.2), during a maintenance task (the writer is interrupted while that task runs: `SQLITE_INTERRUPT`, nothing changed, M8.4), or while the repair op waits for the writer (O5). The job ends `Cancelled`; the backup stays; no `lastCheckDatabase` is recorded. A cancel after C4's sound verdict keeps C4's `lastQuickCheck` and its state change (`Failed{Corrupt}` to `Ready`).
- Once the writer takes the repair op, a cancel is ignored and the result reports `past_cancel_point` (O5).
- `suspend` (U32) cancels the job as a cancel does. A repair op already running finishes; if the process is killed meanwhile, its transaction is not committed.

## C16. Concurrency (R1494, R1847)

- The state stays `Ready` while the slot is held: review and edits go on. Ops queued during a maintenance task or the repair op wait for the writer; answers accepted meanwhile stay in U43's overlay and pending-answer journal (KTD6, `engine/op-model` O8). An answer committed before the repair op is repaired like any other row when a repair applies to its new values, and otherwise keeps them.
- The local servers wait for availability (`engine/lifecycle` L6): the held slot makes the collection unavailable to them (R1706, R1797).
- Budget (R1847): on a healthy synthetic collection of 150,000 cards, from the start of the integrity stage to the report, the safety backup excepted (as `conformance/timed-operations` T6.8 times Anki), at most 5 s on the M3 Max. Bench `core/crates/meguri-engine/benches/check_db.rs`.

## C17. Differences from Anki

Each is a deviation entry in `conformance/deviations.yaml`, added with the scenario golden that shows it (R1889):

| Id | Anki 26.09.3 | Meguri |
|---|---|---|
| `check-db-no-change-keeps-mod` | sets `col.mod` on every run | only when a row changed (C11, R1472) |
| `check-db-lrt-reads-needed-columns` | stops with an error, changing nothing, when a card or review-log row has a column outside its range (C8.7, C9.3) | goes on; C10.1 repairs `ivl`, `lastIvl` and `time`; other out-of-range columns stay |
| `check-db-future-ids-skip-used` | stops with a constraint error when a new id is taken (C10.4) | skips used ids |

Not deviations, since no scenario can show them: the order of backup and integrity check (C3); the lifecycle, progress, cancel, records and report (C2, C12 to C15). Not a deviation entry, since no scenario observes it: the undo step Anki keeps after a note type recovery (C14); a scenario that recovers a note type must not observe `undo_status`. The notes stage's differences are `maintenance/check-database-notes` N9.

## C18. Scenarios and tests

### C18.1 Goldens on this branch

| Scenario | Shows |
|---|---|
| `maintenance/check-db/new-positions` | C8.1: 2,000,000 folded and counted; 1,500,000 left |
| `maintenance/check-db/last-review-time` | C8.7, and a second run that finds nothing |
| `maintenance/check-db/missing-deck` | C9.2: `recovered4242` |
| `maintenance/check-db/revlog-properties` | C10.1: `type` 0, `time` 0, `ease` 0 becomes 5 |
| `maintenance/check-db/future-ids` | C10.4: one id two days ahead re-keyed to `now_ms`, one exactly a day ahead kept |
| `sched/memory/lrt-repair` | C8.7 in full (`sched/memory-state#M6`) |

They become active when the runner's `check_db` adapter runs this op: the change that lands it creates `conformance/scenarios/maintenance/area.yaml` listing them (it does not exist yet).

### C18.2 To record (oracle maintainer)

Each uses only existing scenario ops (`db_exec`, `check_db`, `add_note`, `advance`, `answer`, `set_config`, setup) and, as observations, `db_exec` selects and `undo_status`. Unless a row says otherwise, the scenario first sets `usn` 5 on the rows it damages and advances the clock 1 hour, so a stamp shows, and ends with `SELECT mod, scm FROM col` and a second `check_db` that finds nothing.

| Scenario (`maintenance/check-db/`) | Damage and observation |
|---|---|
| `card-due` | a new card `due` 2.5, a review card (queue 2) `due` 200,000, a learning card `due` 3,000,000,000: C8.2's values and stamps, `card_properties` 3, `scm` unchanged |
| `new-position-fold` | new cards `due` 2,500,000 and 1,999,999, a queue-4 new card `due` 2,500,000: C8.1 (1,500,000; unchanged; unchanged), count 1 |
| `card-odue` | a review card with `odue` 10 and `odid` 0, a card with `odue` 1.5: C8.3, count 2 |
| `card-ivl` | `ivl` -5, 1.5 and 3,000,000,000: C8.4, count 3 |
| `card-text-values` | by `db_exec`: a new card `due` `'abc'`, a new card `due` the blob `x'3132'`, a review card (queue 2) `due` `'abc'`, a card `ord` `'abc'`, a card `ivl` `'12abc'`, a card `mod` `'12abc'`: 1,000,000 and 1,000,012 (`new_positions_too_high` 2), today, 30,000, 12 and 12 (`card_properties` 4; the Terms' conversion rules); the `ord` 30,000 card is then removed by the notes stage (`cards_without_template` 1, `scm` set) |
| `card-mod` | `mod` 1894701600.5: C8.5, truncated, no stamp |
| `card-ord` | a standard card `ord` 40,000: C8.6 to 30,000, then removed by the notes stage (`card_properties` 1, `cards_without_template` 1, `scm` set) |
| `orphan-cards` | a note row deleted by `db_exec`: its card deleted, no grave (`SELECT count(*) FROM graves`), review log kept, `scm` set |
| `missing-deck-name-taken` | a deck named `recovered4242` exists, a card in deck 4242: deck `recovered4242+` with id 4242 |
| `missing-parents` | decks renamed by `db_exec` to `A::B::C`, `X::Y`, `X::Z` (U+001F between components): `A::B`, `A` and `X` created, ids and names observed, `decks_missing` 2, `scm` unchanged |
| `filtered-fields` | a card in a normal deck with `odid` 1 and `odue` 7: both 0, no stamp, `card_properties` 1, `scm` set |
| `revlog-values` | review-log rows with `ivl` 1.5, `lastIvl` -2.5, `time` -5 and 3,000,000,000: C10.1's values, `usn` kept, `revlog_properties` count, `scm` set |
| `next-position` | `nextPos` set to 99 by `set_config`, new cards at positions 1 to 3: `nextPos` 4; then every card made a review card by `db_exec` (`type` 2, `queue` 2, `due` 0) and a second `check_db`: `nextPos` 0 |
| `future-ids-cards` | a card and a review-log row with ids two days ahead: re-keyed to `now_ms`, the row's `cid` follows its card, `future_ids` 2 |
| `healthy` | a collection with notes, reviews and decks, no damage: all counts 0, `scm` unchanged, `col.mod` (deviation `check-db-no-change-keeps-mod`), `undo_status` empty after the run |
| `all-records` | every damage above in one collection: one report, counts equal to the sum |

Held for a driver change (the `check_db` op knows only the rejection `corrupt`; these need a rejection for Anki's other failures, for example `{"error": "failed"}` in `conformance/collection-check` K10 and `dsl.md` D7.2): `revlog-ivl-overflow` (a review-log `ivl` of 3,000,000,000: Anki fails, Meguri repairs, deviation `check-db-lrt-reads-needed-columns`) and `future-ids-taken` (a valid note at `now_ms` and a note two days ahead: Anki fails, Meguri gives `now_ms` + 1, deviation `check-db-future-ids-skip-used`).

### C18.3 Meguri tests (`core/crates/meguri-engine/tests/check_db.rs`)

- C3's order: a corrupt file (a table b-tree page overwritten on a copy) ends with `Corrupt`, not `SafetyBackupFailed`; no backup file, no maintenance task and no repair op ran; the file's bytes are unchanged; `Ready` moves to `Failed{Corrupt}`; `lastQuickCheck.passed` false and `lastCheckDatabase` `{instant, result: {"error": "corrupt"}}`.
- From `Failed{Corrupt}`: a sound verdict returns to `Ready` with the same generation and the run completes; a corrupt one leaves the state (`maintenance/optimize-integrity` M9's row).
- The backup is published before the first maintenance task starts (op log order), and is protected until the job ends; exactly one safety backup is made per run (the executor takes none for the repair op); a failing backup gives `SafetyBackupFailed` with the file unchanged.
- A failing `VACUUM` (`SQLITE_FULL` injected) gives `CheckFailed{optimize}` and no repair; a corrupt one gives `Corrupt`.
- A row that cannot be decoded (a deck blob made invalid) gives `CheckFailed{cards}` and no change.
- Cancel at each cancellable point gives `Cancelled` with no record and no change; a cancel during the repair op gives `past_cancel_point` and a complete report.
- `Busy{running_op}` while restore holds the slot; `CollectionUnavailable` in `NoCollection` and `ExclusiveBusy`.
- After a run: both undo histories empty with `ClearedByNonUndoableOp`; every derived component `Stale`; the status shows the run's time and result; malformed `lastOptimize` and `optimizeCadenceStart` repaired and listed.
- An answer submitted during the repair op is committed after it and visible; a card answered after the backup and before the repair op keeps its new scheduling unless a repair applies to its new values.
- T1 (R1925): a collection opened, studied (200 answers) and closed by Meguri gives the oracle's Check Database zero problems (`tools/mg oracle check`).
- Bench (R1847): C16's budget.

# Maintenance specs: automatic optimize and integrity verdict (U132)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write
`specs/maintenance/optimize-integrity.md` and
`specs/conformance/maintenance-ops.md`. No code was copied; the rules were
re-derived in the spec writer's words and checked by black-box probes in the
oracle image (`meguri-oracle:26.9.3-29ec6be928383e7f`, 2026-10-17).

| Spec section | Anki file | Read for |
|---|---|---|
| optimize-integrity M1, maintenance-ops M1.2 | `pylib/anki/collection.py` (`optimize`), `pylib/anki/dbproxy.py`, `rslib/src/backend/dbproxy.rs` | the automatic optimize is two raw statements, `vacuum` then `analyze`, sent one at a time through the query API with no begin or commit request; a statement not starting with `select` marks the proxy modified (stamping `col.mod` only at a later commit request) |
| optimize-integrity M2 (undo, study queues) | `rslib/src/undo/mod.rs` (state after a proxy modification), `rslib/src/backend/dbproxy.rs` | a non-select raw statement discards the undo history and the study queues |
| optimize-integrity M3 | `rslib/src/storage/sqlite.rs` (storage optimize: `vacuum; reindex; analyze`), `rslib/src/dbcheck.rs` (quick check, then optimize, then repairs), `rslib/src/collection/mod.rs` (preparation before upload, then the storage optimize), `rslib/src/sync/collection/upload.rs` | the three-statement variant and its two callers |
| optimize-integrity M4 | `qt/aqt/main.py` (the auto-optimize section, the collection unload), `qt/aqt/profiles.py` (profile defaults, `create`, profile load fallback) | 14 days as `86400 * 14` seconds, `<` comparison against integer Unix seconds, a missing value counts as due, the value is written after the optimize returns; profile defaults fill it with the time the module was loaded, so a new or reset profile waits 14 days |
| optimize-integrity M5 | `qt/aqt/main.py` (collection unload) | close order: optimize, quick check (skipped in developer mode), backup on close unless corrupt, developer mode or restoring; any exception from the optimize or the check marks the close corrupt |
| optimize-integrity M6 | `rslib/src/storage/sqlite.rs` (quick check helper; open with an integrity check flag), `rslib/src/sync/collection/download.rs`, `rslib/src/import_export/package/colpkg/import.rs`, `rslib/src/dbcheck.rs` | verdict rule (first row equal to `ok`, any error corrupt); full check on downloads and `.colpkg` imports; Check Database's first stage |
| optimize-integrity M7 | `qt/aqt/main.py`, `ftl/qt/qt-misc.ftl` (the corrupt-collection warning, the optimizing progress label), `ftl/core/database-check.ftl` | what the user is told; no state kept after a corrupt verdict |
| optimize-integrity M8 | none (SQLite behavior) | probes below |
| optimize-integrity M4.1, M5 (order), M5.1, M7 (Meguri part), M7.1, M7.2, M10 | none (Meguri design: plan U132, R169, R1656, R1657, R2011, R2012, R2056, KTD26) | ported from `spec/U132` on 2026-10-27; M7's remedy list rewritten for the Anki-server freeze (no download remedy, a reserved iCloud replace), M4.1, M5.1, M7.2 and M10 added after review; no Anki source read for the port |

Probes (scripts in the session scratchpad, not kept):

- `anki` wheel (SQLite 3.49.2): a new collection holds `sqlite_stat1` rows only
  for `cards`/`idx_cards_odid` (`0 0`), `col` (`1`) and `config`; after three
  notes of one note type, a 1,000,000-byte blob in a scratch table dropped
  again (245 free pages), and the collection's optimize call: free pages 0,
  `quick_check` and `integrity_check` `ok`, `col` `mod`, `scm`, `usn`, `ls`
  unchanged, `notes` statistics `idx_notes_mid` `3 3`, `ix_notes_csum` `3 1`,
  `ix_notes_usn` `3 3`; `temp_store` 0 and `auto_vacuum` 0 on Anki's connection.
- Python's `sqlite3` (SQLite 3.46.1), WAL files: `VACUUM` with
  `temp_store=MEMORY` fails under a 20 MB `hard_heap_limit` on a 49 MB file and
  succeeds with `FILE`; `VACUUM` succeeds while another connection holds a read
  transaction, which keeps its snapshot; `wal_checkpoint(TRUNCATE)` then
  returns busy (1) and the file keeps 82 MB until the reader ends, after which
  it shrinks to 41 MB; an interrupt during `VACUUM` gives `interrupted`, the
  free page count unchanged, no open transaction, `quick_check` `ok`.

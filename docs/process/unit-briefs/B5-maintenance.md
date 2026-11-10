# Batch B5-maintenance: U128, U129, U130, U131, U132

Implementer view. State read at `cf03979b` (worktree HEAD = `origin/master`). That commit changes only browser, stats and `storage/schema11-downgrade` specs and oracle provenance, so no B5 fact differs from its parent `21fc5c1f`. Sources: the plan `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md` (cited as "plan"), the specs and `conformance/` paths named in each line. "Held" means untracked in the worktree and not on master (`git ls-files` does not list it).

## 1. Batch, grouping, order, priority

- Units:
  - U128 Check Database: integrity, optimize and record repairs (plan §U128).
  - U129 Check Database: note and note type repairs (§U129).
  - U130 Check Media and media trash (§U130).
  - U131 Empty Cards (§U131).
  - U132 Integrity monitoring, corruption handling and optimize (§U132).
- Why grouped: they are all `core/crates/meguri-engine/src/maintenance/` work on shared primitives: the U127 safety backup, the U24 writer maintenance tasks, the U124a/U32 sidecar device records, and the U31 trash. Of these units only U128 takes the U24 long-operation slot (`engine/lifecycle` L9 table: Check Database yes; Check Media and Empty Cards no). U130 takes the media-job lease instead (`check-media#C2`). U128 and U129 share one op, report and `scm` rule (`specs/maintenance/check-database-notes.md` N8).
- Milestones: U132 is in **M1R**. U128 to U131 are in **M1** (plan milestone table, lines ~3862–3863). This batch therefore crosses a milestone boundary, which must be gated (`docs/process/tiers.md`: "at every milestone boundary").
- U132's status: the code is on master (`maintenance/{integrity,optimize,status}.rs`, `tests/integrity.rs`), and focused checks and independent review are done. It awaits the **earlier** maintenance batch gate (U124a, U126, U127, U132, U133a; base `1416a520542a8c5daa38a23bfe398da704c3d6c9`) and has no `Unit:` trailer (`docs/process/review-log/U132.md`, `docs/process/review-log/maintenance-integration-2026-10-27.md`). Do not redo it. Close it through that gate, not as new B5 work.
- Suggested order, with parallel lanes:
  1. U132: close it in the open maintenance batch gate. That needs a full gate rerun from the original base `1416a520`, including all new commits. The original gate `gate/20261027T193251Z-31655` ended `error/cancelled`, not pass, and the later broad gates also ended cancelled/incomplete (`maintenance-integration-2026-10-27.md`). Open items (`review-log/U132.md`, last paragraph): U127 R1848 (escalation `E-20261027T193208Z-235c`), U133a, platform/final evidence and the documented P2 gaps.
  2. U128 and U130 in parallel. U130 does not depend on U128 or U132.
  3. U129, after U128, because it runs inside U128's repair op.
  4. U131, after U35 lands. U35 is not implemented.
  5. Then the U128 "attach to U132" step (§U128 Approach): add Check Database at the head of the remedies and record `lastCheckDatabase` in the status.
- Priority: `docs/process/review-first-delivery.md` says to finish reviews and iCloud sync first (2026-10-29). After the backup, integrity and recovery work is verified, U113/U114 and their dependencies come "before the remaining editing, search and statistics features". U132 is part of that backup, integrity and recovery work. The file does not name U128 to U131. This brief infers that they (M1 maintenance) also come behind U113/U114; confirm with the orchestrator. Anki-server work is frozen (same file, "Anki server freeze — 2026-10-26").

## 2. Dependency status

"Trailer" means that `git log --format=%B | grep "^Unit:"` on master has the id. Trailers for U124a, U126 and U127 are on spec-writer and oracle-maintainer commits only. Their review logs say that the implementation awaits the batch gate (`docs/process/review-log/U126.md`, `U127.md` and `U124a.md`).

| Unit | Depends on | Status |
|---|---|---|
| U128 | U3, U4, U22, U23, U24, U25, U26, U27, U28, U40 | all have trailers |
| | U127 | trailer (spec-writer and oracle commits only). Implementation open: R1848 P1 unresolved (`review-log/U127.md`) |
| | **U132** | **no trailer.** Code is on master and awaits the maintenance batch gate |
| U129 | U27, U29, U30, U61 | trailers |
| | **U128** | **not started** |
| U130 | U20, U24, U27, U31, U60 | trailers |
| | U127 | as above |
| | U111 (plan Unit Index row; §U130 says "coordination") | **frozen** Anki-server unit (plan Frozen Unit Index). Has trailers only on unmerged `spec/U111` |
| | U114 (`specs/maintenance/check-media.md` C11, for `pending_download` only) | **no trailer** |
| U131 | U23, U25, U27, U60, U61 | trailers |
| | U127 | as above |
| | **U35** | **no trailer on master.** Its trailers are only on unmerged `spec/U35`, and no empty-cards scan or delete exists in `meguri-engine/src`. **Blocker**. The shared card removal `cards::remove_in` exists (§4); U35/U131 reuse it |
| U132 | U22, U24, U32 | trailers |
| | U124a, U126, U127 | trailer (spec only). Implementation awaits the same batch gate |

## 3. Per unit

### U128

- Spec ids:
  - `maintenance/check-database#C2` to `#C18`.
  - `conformance/collection-check#K5`, `#K10`.
  - `maintenance/optimize-integrity#M3`, `#M6`, `#M7.1`, `#M7.2`, `#M8.1`, `#M8.3`, `#M8.4`, `#M9`, `#M10`.
  - `conformance/maintenance-ops#M2` (corrupt_page).
  - `package/backup-triggers#T6.1`, `#T6.2`, `#T7`.
  - `sched/memory-state#M6` (lrt).
  - `engine/lifecycle#L4`, `#L5`.
  - `engine/op-model#O9`.
  - `engine/undo#M6`.
  - `engine/collection-replace#R6.1`.
  - `notetypes/schema-changes#G2`.
- Design docs it writes itself: none. Its plan Files have no "(Meguri)" entry, and `specs/maintenance/check-database.md` is already written.
- Committed scenarios (25), all with committed goldens, all **inactive**. `conformance/scenarios/maintenance/area.yaml` lists only `optimize/*`. They are in `conformance/scenarios/maintenance/check-db/`:
  - `new-positions`, `new-position-fold`, `card-due`, `card-odue`, `card-ivl`, `card-text-values`, `card-mod`, `card-ord`;
  - `last-review-time`, `orphan-cards`, `missing-deck`, `missing-deck-name-taken`, `missing-parents`, `filtered-fields`;
  - `revlog-properties`, `revlog-values`, `next-position`, `future-ids`, `future-ids-cards`, `healthy`, `all-records`;
  - Anki-`failed` with a deviation: `revlog-ivl-overflow`, `future-ids-taken`, `parents-under-filtered`, `parents-too-deep`.
- Also inactive, with committed goldens: `sched/memory/lrt-repair` (C18.1; sched area) and `notes/fields/count-repair`. For count-repair, the `conformance/scenarios/notes/area.yaml` comment names U128, but its repair is N6.4, which is U129's.
- Activation: the change that lands the `check_db` adapter adds `check-db/*`, or the names one by one (C18.1, last paragraph). At HEAD the testkit already registers everything the held `meguri-studied-handoff` first segment uses: `answer`, `forget` and `suspend` (`exec/sched.rs` `register`, lines 28–31), `edit_note` and `delete_notes` (`exec/notes.rs`), `advance` (`exec/clock.rs`), the handoff adapter (`exec/storage.rs` line 40), and setup `config`, `notetypes`, `decks` and `notes`. So `check-db/*` is allowed. Once the held scenario lands, the glob makes it active and T1 then requires it, so the adapter change (or the scenario-landing change, whichever comes later) must pass it live. See §5.8.
- Prepared and held (untracked scenario and golden):
  - `check-db/filtered-deck-kept` and `check-db/future-ids-lrt`: held until the oracle-maintainer change that records them is committed. The matching edits to C18.4 are also uncommitted in the worktree.
  - `check-db/meguri-studied-handoff` (live T1, R1925): held for the same reason.
  - `check-db/corrupt-page`: committed M2.4 at HEAD still says "Draft. Nothing implements it yet" and needs, in one change, the `dsl.md` D7.2 row, the `scenario.schema.json` shape, the `protocol.json` step list, the oracle driver op with tests, and the testkit model. All the non-testkit pieces, the oracle driver op and its tests included, are uncommitted worktree edits (the "held until the testkit model reads it" wording is the uncommitted M2.4 text). So it is held first until the oracle-maintainer commit that carries those pieces lands, then until the testkit model reads `corrupt_page`.
- Plan test → evidence:

  | Plan test | Evidence |
  |---|---|
  | new due 2,500,000 folded / 1,500,000 left | `new-position-fold`, `new-positions` |
  | review due 200,000 | `card-due` |
  | negative and overflowing ivl | `card-ivl` |
  | odue on a non-filtered card | `card-odue` |
  | float mod | `card-mod` |
  | ord 40,000 | `card-ord` |
  | missing deck id | `missing-deck`, `missing-deck-name-taken` |
  | `A::B` without `A` | `missing-parents`, `parents-under-filtered`, `parents-too-deep` |
  | wrong lrt | `last-review-time`, `sched/memory/lrt-repair`, held `future-ids-lrt` |
  | orphan card | `orphan-cards` |
  | odid in a normal deck | `filtered-fields`, held `filtered-deck-kept` |
  | revlog ivl overflow / type0 time0 ease0 | `revlog-ivl-overflow` / `revlog-properties`, `revlog-values` |
  | future note id | `future-ids` (2 days, not 3), `future-ids-cards`, `future-ids-taken` |
  | all together | `all-records` |
  | scm only for R1643 categories | goldens of the above |
  | healthy | `healthy` |
  | corrupt page | held `corrupt-page` plus Meguri test (C18.3, first bullet) |
  | backup before repair commit; undo cleared; answer during the run; answer between backup and repair | Meguri `core/crates/meguri-engine/tests/check_db.rs` (C18.3) |
  | T1 R1925 (200 answers) | `tools/mg oracle check` suite (C18.3), plus held `meguri-studied-handoff` |
  | bench 150k cards ≤ 5 s | `benches/check_db.rs` (C16, R1847) |
  | remedies from `Failed(Corrupt)`, status after a run | `tests/integrity.rs` (M9 row "Check Database from `Failed{Corrupt}` (with U128)"), `tests/check_db.rs` |

### U129

- Spec ids:
  - `maintenance/check-database-notes#N1` to `#N10`.
  - `notes/note-records#F1`, `#N2`.
  - `notetypes/stock-kinds#K8`.
  - `notetypes/validation#V1`.
  - `tags/tags#T4`, `#T11`.
  - `notes/card-generation#G6`, `#G7`, `#G10`.
  - `notes/deck-placement-and-position#D3`, `#P2`.
  - `text/checksum-and-sort-field#C2` to `#C4`.
  - `model/protobuf-records#P4`, `#P5`.
  - `conformance/notetype-ops#T5.1`, `#T10.1`.
- Design docs it writes itself: none (`specs/maintenance/check-database-notes.md` is written).
- Committed scenarios (15), all with goldens, all inactive. They are in `check-db-notes/`: `recover-notetype`, `recover-two-same-count`, `recover-one-field`, `duplicate-ords`, `missing-template`, `cloze-kept`, `invalid-utf8`, `future-mtime`, `missing-card`, `no-notetypes`, `io-field-tags`, `tag-rebuild`, `no-card-note`, `all-notes`, `default-deck-missing` (Anki `failed`). Activation adds `check-db-notes/*` (N10.2).
- Prepared and held: `check-db-notes/template-now-nonempty` and `check-db-notes/default-deck-filtered`, until the oracle-maintainer change is committed. The deviation `check-db-default-deck-filtered` is also uncommitted in `conformance/deviations.yaml`.
- Plan test → evidence:

  | Plan test | Evidence |
  |---|---|
  | 3-field note in a 2-field note type | `notes/fields/count-repair`, `recover-one-field` |
  | missing mid | `recover-notetype`, `recover-two-same-count` |
  | duplicate (nid, ord) | `duplicate-ords` |
  | ord 5 on 2 templates | `missing-template` |
  | vanished cloze kept | `cloze-kept` |
  | invalid UTF-8 | `invalid-utf8` |
  | tag parent and collapse | `tag-rebuild` |
  | IO field tags | `io-field-tags` |
  | future mtime | `future-mtime` |
  | newly non-empty template | held `template-now-nonempty`, and `missing-card`, `no-card-note` |
  | all note types deleted | `no-notetypes` |
  | combined | `all-notes` |
  | other Meguri-only cases | `tests/check_db.rs` (N10.3) |

### U130

- Spec ids:
  - `maintenance/check-media#C1` to `#C24`.
  - `conformance/media-ops#F14.1` to `#F14.4`.
  - `media/media-files#M3`, `#M5`, `#M6`, `#M8`, `#M10.1` to `#M10.3`, `#M14`.
  - `media/media-journal#J3`, `#J5`, `#J6`.
  - `sync/media-tracking#T4` to `#T7`, `#T9`.
  - `text/media-references#M2` to `#M5`.
  - `text/media-filenames#F1`.
  - `storage/derived-store#D4`, `#D8`.
  - `engine/op-model#O9` (`check_media_fix`).
  - `package/backup-triggers#T6.1`.
  - `sync/cloudkit-records#K9.3`, `#K9.4`.
- Design docs it writes itself: none (`specs/maintenance/check-media.md` is written).
- Committed scenarios (22), all with goldens (Linux oracle), all inactive (C22). They are in `check-media/`:
  - `report/basic`;
  - `rename/invalid-chars`, `rename/long-name`, `rename/nfd`, `rename/nfd-upper`, `rename/entities`;
  - `refs/transform-repair`, `refs/html-forms`, `refs/case-insensitive`, `refs/empty-oversize`;
  - `inline/extract`, `latex/names`, `templates/field-refs`, `templates/literal-ref`;
  - `actions/tag-missing`, `actions/delete-unused`, `actions/restore-conflict`, `actions/empty-trash`;
  - `rerun/no-change`;
  - `case/same-content`, `case/other-content`, `case/same-content-no-key` (tagged `media-case-variants`, Linux lane only).
- Prepared and held: none.
- Plan test → evidence:

  | Plan test | Evidence |
  |---|---|
  | folder setup (a.jpg, b.jpg, c.png, x/, big file, .DS_Store, empty file) | `report/basic`, `refs/empty-oversize` |
  | NFD café | `rename/nfd`, `rename/nfd-upper`. The trash stays **empty**, which replaces the plan's "old file in the trash" (C22 row) |
  | base64 | `inline/extract` |
  | Cat/cat | `case/*` |
  | Tag Missing and undo | `actions/tag-missing` |
  | Delete Unused | `actions/delete-unused`. Backup and index pending deletion: Meguri test |
  | Restore conflict | `actions/restore-conflict` |
  | Empty Trash | `actions/empty-trash` |
  | media sync during the run (lease); R2018 deletion-path enumeration | Meguri `tests/check_media.rs` (C23) |
  | bench, 30,000 files, 0 rehashes | `benches/check_media.rs` (C19) |

### U131

- Spec ids:
  - `notes/empty-cards#E1`, `#E2`, `#E4`, `#E6`, `#E7`.
  - `conformance/empty-cards-ops#Y1`, `#Y2`.
  - `package/backup-triggers#T6.1`, `#T6.2`.
  - `storage/graves#G3`, `#G6`.
  - `engine/undo`.
  - `engine/change-stream#S1`.
  - R1652, R1653, R1654.
- Design docs it writes itself: none. The plan says the scan, keep and delete semantics are U35's `specs/notes/empty-cards.md`.
- Committed scenarios: none under `maintenance/empty-cards/`. U35's `cardgen/empty/{scan,keep-notes,delete-notes,recheck,occlusion}` have goldens and are inactive. Activating them is U35's job (`empty-cards-ops#Y4`).
- Prepared and held: `maintenance/empty-cards/report`, `undo-redo` and `incremental-sync` (scenario and golden untracked), and U35's `cardgen/empty/undo`. They are held until the oracle-maintainer change is committed. The matching E7 status edits are uncommitted in the worktree.
- Plan test → evidence:

  | Plan test | Evidence |
  |---|---|
  | reversed note with an empty back; cloze c2 | held `report`, `cardgen/empty/scan` |
  | keep on/off | `cardgen/empty/keep-notes`, `delete-notes` |
  | graves, undo, redo | held `undo-redo`, held `cardgen/empty/undo` |
  | U13 two-client incremental sync | held `incremental-sync` (actors, Anki sync server): **frozen harness**, see §5 |
  | IO mask 3 | `cardgen/empty/occlusion` |
  | backup before the delete; report paging | Meguri `tests/empty_cards_maintenance.rs` |

### U132

- Spec ids:
  - `maintenance/optimize-integrity#M1` to `#M10`.
  - `conformance/maintenance-ops#M1`.
  - `engine/lifecycle#L1`, `#L2`, `#L4`.
  - `package/backup-triggers#T3.4`.
- Design docs it writes itself: none (`specs/maintenance/optimize-integrity.md` is written).
- Committed scenarios: `maintenance/optimize/{compact-no-stamps,statistics}` (T0) and `anki-opens-optimized` (live T1). All have goldens and are **active**.
- All other U132 plan tests are Meguri-only in `tests/integrity.rs` (M9 table). The receipts are in `review-log/U132.md`.

## 4. Testkit and adapter work

The model is `core/crates/meguri-testkit/src/scenario/model.rs`, and adapters register in `core/crates/meguri-testkit/src/exec/*.rs`.

| Form | Model | Executor | Defined in | Unit |
|---|---|---|---|---|
| op `corrupt_page {table}` | **missing**. Its schema, protocol and D7.2 row are uncommitted on master | missing | `conformance/maintenance-ops#M2` | U128 |
| op `check_db` | parsed (`Step::CheckDb`) | **no adapter** | `conformance/collection-check#K10` | U128, U129 |
| ops `check_media`, `delete_unused_media`, `tag_missing_media` | parsed. The intake checks the step rule (`intake.rs`) | **no adapter** | `conformance/media-ops#F14.1` to `#F14.3` | U130 |
| setup need `media_files.case_variants` (tag `media-case-variants`) | the need is derived (model.rs ~3606) | not declared. Declare it only on a case-sensitive Linux lane | `conformance/media-ops#F14.4` | U130 |
| runner reports case-variant `pending` apart from others | none | none | `media-ops#F14.4`, last bullet | U130 (with the runner) |
| ops `empty_cards`, `delete_empty_cards` | parsed | **no adapter** (U35 maps them) | `conformance/empty-cards-ops#Y1`, `#Y2`, `#Y4` | U35, then U131 |
| capability `update_notetype.field_options.io` | parsed (model.rs ~3669) | refused as `unsupported_op` (`exec/notetypes.rs` ~219) | `conformance/notetype-ops#T5.1`, `#T10.1` table | U129 |
| ops `optimize`, `db_exec`, `reopen`, `ensure_io_notetype`, `set_tag_collapsed`, `media_*`; observations `db_dump`, `notetype_structure`, `tag_tree`, `media_dir`, `media_trash`, `media_pending`, `undo_status`, `schema_objects`, `card` | present | present | — | — |

Engine pieces already on master (reuse them):
- `JobKind::CheckDatabase` (takes `LongOp::CheckDatabase`), `JobKind::CheckMedia` and `JobKind::EmptyCards` (no slot) in `meguri-engine/src/jobs/mod.rs` (`long_op`).
- The media-job lease: `MediaJobKind::CheckMedia` (`meguri-engine/src/media/lock.rs`) and `MediaService::try_job` (`media/service.rs`). The testkit's `local_check` in `meguri-testkit/src/exec/media.rs` already takes it; `tests/media_ops.rs` tests it.
- The undo name `EMPTY_CARDS` (`meguri-engine/src/undo/names.rs`).
- `cards::remove_in(scope, ids, remove_empty_notes)` (`meguri-engine/src/cards/remove.rs`; module doc "Shared card removal for deck, template and Empty Cards operations"). It removes the cards, writes card graves, and optionally removes and graves notes left without cards. It does not re-evaluate emptiness (E4); the caller does. Not for Check Database, whose deletions get no graves (§6).

Engine pieces that the specs place elsewhere and that do not exist yet:
- U31 store calls `trash_exact`, `summary_with_ids()` and `empty_entries(ids)`, and registration that returns T5 old→new pairs and skipped non-NFC names (`check-media#C2`, `#C10`, `#C17`). They are absent from `meguri-storage/src/media/trash.rs`, which has only `trash`, `restore`, `empty_trash`, `trash_summary` and `trash_entries`.
- The catalog op `check_media_fix`. `executor/catalog.rs` has `CHECK_DATABASE` and `REINDEX`.
- The derived component `media_refs` (`check-media#C15`).
- The U114 read-only query of pending CloudKit download case keys (`check-media#C11`).

## 5. Open questions and decisions

1. **U132 and U127 acceptance (blocking for U128 acceptance).** The maintenance batch gate on base `1416a520` has no pass. The original `gate/20261027T193251Z-31655` ended `error/cancelled`, and the next full gate must use that base again, including all new commits (`review-log/maintenance-integration-2026-10-27.md`, "Original gate" and "Remaining boundaries"). Still open (`review-log/U132.md`, last paragraph): U127 R1848 (over the 2 s limit, escalation `E-20261027T193208Z-235c`), U133a, platform/final evidence and the documented P2 gaps. U128 can build on the master code. It cannot be accepted before U132.
2. **U35 not landed (blocking for U131).** U131 submits U35's delete op and uses U35's scan (`notes/empty-cards#E6`).
3. **U31 store extensions (blocking for U130).** C2, C10 and C17 record "decided 2026-10-27: U31 provides" these calls, but none exist. Decide whether U130 adds them under U31's spec or a U31 follow-up does.
4. **Anki-server freeze (not blocking; record as frozen, never as passing).**
   - U131's U13 two-client test and `maintenance/empty-cards/incremental-sync` (actors with a sync server) are frozen-scope.
   - The U111 media-sync coordination is frozen too. Test the media-job lease directly (`check-media#C23`, first bullet).
   - The CloudKit half of R1654 is ordinary change-set row deletions (`notes/empty-cards#E4`).
   - Source: `docs/process/review-first-delivery.md`.
5. **`corrupt_page` vocabulary (blocking only `check-db/corrupt-page`).** Committed M2.4 says draft, nothing implemented. The oracle-side pieces (D7.2 row, schema, protocol, driver op and tests) are uncommitted; after the oracle-maintainer commit lands them, the testkit model is still needed. The Meguri-only C18.3 test covers the corrupt path meanwhile.
6. **Case-variant scenarios (blocking only their activation).** They stay inactive until the runner reports them apart from other `pending` cases on case-insensitive lanes (`media-ops#F14.4`).
7. **`pending_download` from U114 (not blocking).** The list is empty without iCloud (`check-media#C11`). The `sync/cloudkit-records#K9.4` sentence about case-key tombstones is taken with U114 (`check-media#C18`).
8. **The C18.1 activation pattern (not blocking).** Add `check-db/*` only if the executor declares everything that `meguri-studied-handoff`'s first segment uses (FSRS `answer`, `forget`, `suspend`, `edit_note`, `delete_notes`, `handoff`; D9.2). Otherwise list the scenarios by name. At HEAD every one of these is registered (§3 U128, Activation), so the glob is allowed. Recheck this before the adapter commit. The glob makes the held live scenario T1-required as soon as it lands, so the change must pass it, not only declare it.
9. **The `notes/fields/count-repair` owner (not blocking).** The area comment says U128, but the repair is N6.4 (U129).
10. **Decided, no action:**
    - The core takes no confirmation token for Check Database. The UI asks (U163) (`check-database#C2`).
    - The safety backup is silent (C5).
    - Both undo histories are emptied even on note type recovery or parent creation (C14).
    - Check Media runs only on request (C2).
11. **The spec-ahead file (not blocking; servers scope).** `docs/process/review-log/spec-ahead-servers-open-questions-20261110.md` P17.8 (Migaku `ok: false` during a short Check Database). Interim answer: it follows `engine/lifecycle` L6 availability.

## 6. Traps

Check Database (U128):
- The integrity check comes **before** the safety backup. A corrupt verdict makes no backup, no optimize and no write. The job, not the executor, takes exactly one backup (C3, C5).
- `quick_check` runs on the maintenance reader, never `integrity_check`. Interrupted, I/O, out-of-memory and busy results give "no verdict", which is not corrupt (C4, `optimize-integrity#M6`).
- `scm` is set only for C8.7, C9.1, C9.3 (the card count), C10.1, C10.4 and N8's four kinds. R1642-only repairs leave it alone (C11, `collection-check#K5`, last paragraph).
- `col.mod` changes only if a row changed (deviation `check-db-no-change-keeps-mod`). Undo is cleared even when nothing changed (C14).
- Stamping is `Raw`:
  - C8.5 and C9.3 do not stamp;
  - C9.3 keeps `data` byte for byte;
  - C10.1 keeps `usn`;
  - C10.4 changes no stamps.
  Deleted cards get **no graves** (C7, C9.1, N6.2, N6.3).
- Reproduce SQLite's text and blob conversion rules exactly (the Terms paragraph of `check-database.md`). Use one repair instant, read when the writer takes the op (C7).
- Repairs read live rows inside the transaction and are never `Stale`. `cards`, `notes` and `history` are not cancellable. Expose `cancellable` and not the phase name (C15).
- The Check Database optimize does not write `lastOptimize`. `VACUUM` runs with `temp_store=FILE`, then back to `MEMORY`. A busy `wal_checkpoint` is not a failure (C6, `optimize-integrity#M8`).
- Malformed device records are replaced through R6.1's explicit replace, not its setter, and listed in `device_records_repaired`. `optimizerLog` is removed, not rewritten (C14).

Notes stage (U129):
- Keep the tag rows (deviation `check-db-tags-keep-rows`).
- The IO repair is a direct row write that keeps unknown protobuf fields and names, with no N4 normalization (N5).
- The recovery note type uses an unchecked save. V3 would refuse `duplicate_front` (N4).
- The skip-unchanged comparison of N6.6 uses SQLite affinity.
- The fallback Basic note type gets local stamps (`check-db-basic-stamped`).

Scenario writing:
- `reopen` after any `db_exec` on `notetypes`, `fields`, `templates` or `decks` rows, before `check_db` or `check_media` (N10.2, `check-media#C21`).
- Scenarios that recover a note type or create a parent never observe `undo_status`.
- Keep `check_db` and `optimize` out of undo windows (`conformance/AGENTS.md`).

Check Media (U130):
- It takes **no long-operation slot** but takes the media-job lease (`check-media#C2`; the plan's "C9 media lock" is this lease).
- Old names are trashed by exact name after the fix op. Each name is checked again in the same writer step as its move (C10 step 5, C24). Delete Unused rechecks every name and takes the backup only if a name remains (C14).
- References match case-insensitively on every volume (C11). The fix op strips control characters but never applies NFC (C10 step 2).
- Empty Trash deletes only the confirmed entry ids. Nothing empties the trash automatically (C17).
- The goldens are Linux only. Never record media scenarios with `--platform apple`, and never give them overlays (`media-ops#F13`, `conformance/AGENTS.md`).
- A handoff forgets the latest `check_media` output (F14.1).

Empty Cards (U131):
- The backup comes from U35's op descriptor safety class. U131 must not call the backup itself: one backup per delete (`notes/empty-cards#E6`).
- The delete re-evaluates live rows, writes card and note graves, and is not a schema change (E4, E5).
- `delete_empty_cards` acts on the latest `empty_cards` scan. Keep each pair in one segment (`conformance/AGENTS.md`).

U132:
- A recorded `passed: false` blocks every mutating open step (M5.1).
- The remedy order is Check Database (attached by U128), restore, then iCloud (M7.1). There is no Anki-server remedy.

Process:
- Mark a scenario active only in the change that lands its adapter (`conformance/AGENTS.md`). Never edit goldens.
- The implementer reads no `oracle/`, `refs/` or `local-data/` (`AGENTS.md`).

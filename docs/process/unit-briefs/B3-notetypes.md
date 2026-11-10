# Batch B3-notetypes: U33, U34, U35

Brief written against `cf03979b` (origin/master, "Correct six unimplemented-unit specs after an adversarial audit"; it put the revised `specs/storage/schema11-downgrade.md` on master). Facts only, each with its source. "Held" means a file that is untracked in this worktree, not on master (`git ls-files`).

## 1. Batch

| Unit | Title (plan) | Plan section |
|---|---|---|
| U33 | Schema-11 legacy writer | plan `### U33.` |
| U34 | Note type structure changes: fields, templates, sort field, deletion, Restore to Default | plan `### U34.` |
| U35 | Change Note Type and Empty Cards (generation side) | plan `### U35.` |

- **Why grouped.** All three are M1 units (`docs/process/milestones.yaml` M1 `units`; plan M1 row). Each changes the shape of note types, cards or the `col` JSON columns, and each builds on U21/U22 records, U29's gate and U27's card generation. U34 and U35 attach exports to U37's `meguri-ffi/src/notetypes.rs` and `NotetypeService.swift`. U33 is Rust-only (meguri-storage plus testkit). Batch rules: 3–5 related units, Swift/FFI included, with one full gate at the batch boundary (`docs/process/tiers.md` lines 7–11).
- **Order.** U33 has no dependency on U34 or U35, and its crates are separate (meguri-storage, `exec/storage.rs`). It can run in parallel with U34. U35 depends on U34 (plan U35 Dependencies) and shares `exec/notetypes.rs` and the FFI module with it, so it comes after U34.
- **Priority context.** Delivery priority is reviews and iCloud first: U113, U114 and their dependencies come before the remaining editing, search and statistics work (`docs/process/review-first-delivery.md`, "Next feature priority"). This batch is on neither U113's nor U114's dependency list (plan U113/U114 Dependencies). It is the core of the editing work:
  - U34 feeds U200 and U201.
  - U35 feeds U203, U87, U121 and U131.
  - U33 feeds U123, the legacy `.apkg` export. File import/export is required for the first usable version (`review-first-delivery.md`, "Next delivery").
- **CloudKit link.** U34's and U35's structure reports feed the CloudKit structural generations (`engine/change-stream#S2.5`, owned by U113 and U114).

## 2. Dependency status

"Merged" means the latest commit with a `Unit:` trailer. Trailers also appear on spec-writer and oracle commits, so the role of that commit is given.

| Unit | Depends on | Status |
|---|---|---|
| U33 | U21 | merged (`742fd9fd`) |
| | U22 | merged (`5e24d8a0`) |
| | U32 | merged (`17aed1de`) |
| U34 | U27 | merged (`3d71adf3`) |
| | U29 | merged (`31edf7a7`) |
| | U36 | merged (`9d7ce443`, fixer "Finalize reviewed U36 preservation conformance"); full gate on that tip not recorded (below) |
| | U37 | **not merged**: no `Unit: U37` trailer anywhere in history |
| | U60 | merged (`f8877802`) |
| U35 | U27, U29 | merged (above) |
| | U34 | **not merged** (this batch) |
| | U37 | **not merged** |

- **U37 status.** Its Rust/Swift modules are on master (`core/crates/meguri-ffi/src/notetypes.rs`, `app/Packages/MeguriCore/Sources/CoreBridge/NotetypeService.swift`). `docs/process/review-log/U37.md` says: "Production backup proof now passes through Swift and headless services; the shared integration gate remains pending before acceptance." U34 and U35 attach to those files. They cannot claim U37's acceptance.
- **U36 gate.** `docs/process/review-log/U36.md` ends with review round 3 clean and "The full clean gate is required on this final `Unit: U36` tip." No gate receipt for that tip is recorded in `docs/process/review-log/` (U36.md, the `*gate*` logs). The first full gate failed on a stale CLI assertion and was cancelled; it is kept as failed evidence, "never reused". U34 uses U36's preservation leg (`preservation/notetype/structure-changes`). It cannot claim U36's gate.
- **Related, not plan dependencies:**
  - U127 has trailers only on spec-writer and oracle-maintainer commits. Its log says "No Unit or gate acceptance" with one P1 open, R1848 (`docs/process/review-log/U127.md`). The T6 safety-backup admission is on master (`core/crates/meguri-engine/src/safety.rs`). `U127-local-continuation.md` says "Future real-operation tests for U34/U35/U121 remain explicitly pending."
  - U38 is merged (`e129c7ed`), so the undo adapters exist (`core/crates/meguri-testkit/src/exec/undo.rs`).
  - U113 and U114 are not merged. They own the S2.5 guard (§5).
  - The field rename op landed early, as a U107 prerequisite (`core/crates/meguri-engine/src/notetypes/rename.rs`, `tests/notetype_field_rename.rs`). "Full U34 stays in M1 without a Unit trailer" (`docs/process/review-log/U107-normal-sync.md`). U34 reuses it; it does not re-implement it.
- **Downstream units waiting on this batch** (plan Dependencies): U123 (U33); U200 and U201 (U34); U87, U121, U131 and U203 (U35).

## 3. Per unit

None of the three lists a Meguri design doc in its plan Files, so there is no "(Meguri)" doc to write.

### U33 Schema-11 legacy writer

- **Spec trailers:**
  - `storage/schema11-downgrade#D1` to `#D12`;
  - `conformance/storage-ops#P1`, `#P4` to `#P8`;
  - consumed, not re-specified: `model/schema11-json#S3` to `#S12` (U21), `storage/durability-and-files#F4` and `#F5`;
  - Requirements: R1481, R2013.
- **Files** (plan): `core/crates/meguri-storage/src/schema/downgrade.rs`, `src/schema/ddl11.sql`, `tests/downgrade.rs`. Neither file exists yet; `src/schema/` holds `ddl18.sql`, `expected_objects.rs`, `refuse.rs`, `upgrade/` and `version.rs`.
- **Committed scenarios and goldens.** All four are inactive: `conformance/scenarios/storage/area.yaml` lists only `open/*`, `upgrade/*`, `new-collection/*` and `config/unknown-keys-survive`.
  - `storage/downgrade/contents` + golden (D3 to D5)
  - `storage/downgrade/roundtrip` + golden (D7)
  - `storage/downgrade/config-invalid` + golden (D3.1, D8; rejection `invalid_json`)
  - `storage/downgrade/oracle-opens` + golden. It is live (it has a `handoff`), so T1 runs it, never T0 (`conformance/AGENTS.md`).
  - U33 adds `downgrade/*` to `storage/area.yaml`.
- **Held:** none in `storage/downgrade/`. Related but U123's: `package/legacy-export/unknown-keys-roundtrip` (scenario and golden), prepared, held until the pending prepared-scenario update is committed.
- **Plan test → coverage:**

| Plan test scenario | Covered by |
|---|---|
| Meguri copy → oracle opens and upgrades → dump equals (T1) | `storage/downgrade/oracle-opens` (T1). Meguri's own upgrade of the copy: `roundtrip` (T0) |
| Unknown `other` keys appear in the legacy JSON and survive the round trip | `contents` (`file_col` of `models`, `decks`, `dconf`), `roundtrip`, `oracle-opens` |
| Live collection sha256 unchanged after the copy | Meguri-only `tests/downgrade.rs` (D11, first bullet: SHA-256, `-wal` size, `data_version`). `contents` also shows the collection's dump and objects unchanged |
| (spec) D8 errors, D9 foreign objects, D10 faults and cancel, D12 deviations | Meguri-only `tests/downgrade.rs` (the D11 list). D12 has no golden and no entry in `deviations.yaml` |

### U34 Note type structure changes

- **Spec trailers:**
  - `notetypes/structure-changes#S1` to `#S16`;
  - `notetypes/restore-to-default#D1` to `#D8`;
  - `notetypes/field-rename#F2` to `#F4` (reused);
  - `notetypes/schema-changes#G2`, `#G3`, `#G5`, `#G6`, `#G8`;
  - `conformance/notetype-structure-ops#M1` to `#M5`;
  - `conformance/notetype-ops#T5`, `#T9`, `#T10.1`;
  - `engine/op-model#O9` (`notetype_structure_change`), `engine/change-stream#S2.5`, `package/backup-triggers#T6.1`, `#T6.2`;
  - `conformance/normalized-dump#N11.7` (preservation leg).
- **Committed scenarios and goldens** (all inactive; `conformance/scenarios/notetypes/area.yaml` active lists only `records/*` entries):
  - `notetypes/structure/`: `reorder-fields`, `combined-draft`, `reqs-after-rename`, `add-template`, `edit-front`, `remove-template`, `add-template-placement`, `rename-field`, `remove-field`, `cloze-fields`, `rewrite-format`, `remove-notetype`, `restore-basic`, `restore-forced-kind`, `restore-unknown-kind`. Each has a golden.
  - `notetypes/gate/after-sync` and `notetypes/gate/structure`, with goldens. U34 makes them active (`schema-changes#G8`, `structure-changes#S16`).
  - `preservation/notetype/structure-changes`, with golden. U34 makes it active (`preservation/area.yaml` comment; `normalized-dump` N10 table).
  - Deviation entries are present for `notetype-reqs-after-field-change`, `notetype-reqs-after-field-change-removal`, `deck-placement-regeneration`, `template-rewrite-keeps-text` and `restore-template-add-schema` (`conformance/deviations.yaml`).
- **Held** (prepared, scenario and golden present untracked; master's `structure-changes#S16`, `restore-to-default#D8` and `notetype-structure-ops#M5` still say "not yet recorded"). Held until the 2026-11-10 scenario and spec update is committed to master:
  - `undo-add-template`, `undo-removals`, `undo-restore` (also need `undo`, `redo`, `undo_status`);
  - `remove-last`;
  - `first-field-duplicates` (needs `fields_check`).
- **Plan test → coverage:**

| Plan test scenario | Covered by |
|---|---|
| Rename `Front` → `Question` in four texts; notes untouched; `scm` unchanged | `rename-field`; `gate/structure`, `gate/after-sync` |
| Delete a field used only on the front → first field appended; `flds` rewritten; "requires confirmation after a sync" | `remove-field`; `gate/structure` (its `remove_field` step sets `scm` after a sync). `gate/after-sync` has no field removal. The confirmation is superseded: no op takes one (`schema-changes#G4`, `structure-changes#S15`) |
| Reorder fields → `flds` reordered, `sfld` recomputed | `reorder-fields` |
| Another field to position 0 on 5k notes → `csum` as the oracle's; AE34 duplicate found | `first-field-duplicates` (held); the 5k-note case is Meguri-only `tests/notetype_structure.rs` (S16 list) |
| One draft with a rename, an added field and a reorder → one transaction and undo entry; injected failure | `combined-draft`; injected failure is Meguri-only |
| Add a template on 5k notes → placement and progress; undo leaves no graves | `add-template`, `add-template-placement`, `undo-add-template` (held); 5k, progress and undo are Meguri-only |
| Remove a template → cards removed with graves; count of notes without cards | `remove-template`; the count (S1.3) is Meguri-only |
| Add a conditional to a front → missing cards generated, none deleted | `edit-front` |
| Restore to Default (3 fields, 2 templates → stock); kind 0 → `NeedsStockKind` | `restore-basic`, `restore-unknown-kind`, `restore-forced-kind`, `undo-restore` (held) |
| Every op equals the oracle's normalized dump | all `structure/` goldens |
| LocalApi field edit during a 50k reorder; LocalApi add during reorder, removal or delete; CSS edit in the same second → `Stale` | Meguri-only `tests/notetype_structure.rs` (S16 list) |
| Template add then undo; kill between plan and commit | `undo-add-template` (held); the kill is Meguri-only |
| Removal preview (S11) | Meguri-only (read-only query; no scenario op) |
| U36 leg: field add, rename and template add on the fixture note type | `preservation/notetype/structure-changes` |
| U37 interface check; field add on 5k notes through `NotetypeService` as a job | Meguri-only Swift/FFI tests. The plan names no file |
| (spec, `structure-changes#S12`, `#S13`, `#S16`; not a plan U34 test) Note type removal and its undo | `remove-notetype`, `undo-removals` (held) |

### U35 Change Note Type and Empty Cards

- **Spec trailers:**
  - `notetypes/change-notetype#M1` to `#M12`;
  - `notes/empty-cards#E1` to `#E7`;
  - `conformance/change-notetype-ops#H1` to `#H4`;
  - `conformance/empty-cards-ops#Y1` to `#Y4`;
  - `engine/change-stream#S2.5` (`type_changes`), `package/backup-triggers#T6.1`, `#T6.2`.
- **Committed scenarios and goldens** (all inactive):
  - `notetypes/change/`: `default-maps`, `info-counts`, `to-reversed`, `swap-fields`, `cloze`, `template-map`, `placement`, `refusals`.
  - `cardgen/empty/`: `scan`, `keep-notes`, `delete-notes`, `recheck`, `occlusion`. `conformance/scenarios/cardgen/area.yaml` active lists only `position/*`.
  - Deviation entries are present for `empty-cards-recheck`, `-cards`, `-review-log` and `-graves`. `change-notetype-cloze-unchanged` has no entry; no scenario sends that input (`change-notetype#M7`).
- **Held:**
  - `notetypes/change/undo` and `cardgen/empty/undo` (scenario and golden untracked; need `undo`, `redo`, `undo_status`). Held until the 2026-11-10 update is committed. On master, `change-notetype#M12` says "not yet recorded" and `empty-cards#E7` does not list `cardgen/empty/undo`.
  - `maintenance/empty-cards/{report,undo-redo,incremental-sync}` are U131's, not U35's (`empty-cards#E7`, plan U131 Files). Held.
- **Plan test → coverage:**

| Plan test scenario | Covered by |
|---|---|
| Basic → reversed type, default map → card 2 generated | `to-reversed` |
| Same type, Front and Back swapped | `swap-fields` |
| Cloze c1–c3 → standard type with two templates → ord 2 removed with a grave | `cloze` |
| `scm` changed between info and commit → refused | `refusals` |
| Add Reverse cleared → card 2 empty; keep-notes keeps the oracle's card; undo | `scan`, `keep-notes`, `delete-notes`, `cardgen/empty/undo` (held); undo also Meguri-only `tests/empty_cards.rs` |
| Field filled between scan and delete → card 2 and its revlog survive; reports 1 kept | `recheck` (with its 4 deviations) |
| Info on 10 notes, 7 with Back → per-field and per-ordinal counts | `info-counts` |
| All results equal goldens | all of the above, plus `default-maps`, `template-map`, `placement`, `occlusion` |
| Info and commit through `NotetypeService`; stale stamp decodes as the refusal in Swift | Meguri-only Swift/FFI test |
| (spec, `change-notetype#M12`; not a plan U35 test) Change Note Type undo and redo | `notetypes/change/undo` (held); Meguri-only `tests/change_notetype.rs` (M12 list) |
| 150k-card scan off the writer with progress (Verification) | Meguri-only (`empty-cards#E7` last paragraph) |

## 4. Testkit and adapter work

`core/crates/meguri-testkit/src/scenario/model.rs` already parses every form below. The gap is adapter registration in `src/exec/`: an unregistered form reports `pending` (`meguri-testkit/AGENTS.md`).

| Unit | Form | Status | Defined by |
|---|---|---|---|
| U33 | op `schema11_copy` (rejection `invalid_json`; register with `op_rejecting`) | parsed (`Step::Schema11Copy`), not registered | `conformance/storage-ops#P4` |
| U33 | observations `file_col`, `file_objects`, `file_query` (read a scratch copy as plain SQLite with `unicase`) | parsed, not registered | `#P5`, `#P6`, `#P7`, `#P8.2` |
| U33 | `open: {file}` (session file) | parsed (`OpenSpec::File`); `exec/storage.rs` returns `unsupported_op` "open from a session file is pending" | `#P1.1`, `#P8.1` |
| U33 | `handoff` carrying session files | registered; `copy_handoff` copies the whole session directory except a few named entries. Check that no `.meguri-tmp-export-*` is left behind | `#P1.3`, `#P4.3`, `dsl.md` D9.2 |
| U34 | `update_notetype` parts `sort_field`, `add_field`, `remove_field`, `rename_field`, `move_field`, `add_template`, `remove_template`, `move_template`, `template_text` (each with `op_part`) | edits parsed; `exec/notetypes.rs` registers only `PLAIN_EDITS` and refuses others with "needs U34's draft apply" | `conformance/notetype-ops#T5`, `#T10.1`, `notetype-structure-ops#M5` item 3 |
| U34 | op `remove_notetype` | parsed (`Step::RemoveNotetype`), not registered | `notetype-structure-ops#M1` |
| U34 | op `restore_notetype` (rejection `needs_stock_kind`) | parsed, not registered | `#M2` |
| U34 | observation `notetype_templates` | parsed, not registered | `#M3` |
| U35 | op `change_notetype_info` (rejections `not_found`, `multiple_notetypes`); keeps state for the next step | parsed, not registered | `change-notetype-ops#H1` |
| U35 | op `change_notetype` (rejections `no_changes`, `schema_changed`, `not_found`, `multiple_notetypes`) | parsed, not registered | `#H2` |
| U35 | op `empty_cards`; keeps the scan for the next delete | parsed (`Step::EmptyCards`), not registered | `empty-cards-ops#Y1` |
| U35 | op `delete_empty_cards` | parsed, not registered | `#Y2` |

Already registered and used by these scenarios:
- setup `notetypes`, `notes`, `decks`;
- ops `add_note`, `edit_note`, `delete_notes`, `add_stock_notetype`, `ensure_io_notetype`, `adopt`, `db_exec`, `undo`, `redo`;
- observations `notetype_structure`, `fields_check`, `card`, `db_dump`, `schema_objects`, `undo_status`.

## 5. Open questions and decisions

1. **U33: the 2026-11-10 decisions of `specs/storage/schema11-downgrade.md` are on master.** Settled; not blocking. `cf03979b` (Role: spec-writer, Spec: `storage/schema11-downgrade`) committed them, and the worktree file has no diff against origin/master. Master text:
   - D1 (lines 18–21): `DestinationIsSource` by file identity. With the same resolved parent directory, the name is compared with the four source names after NFC and full Unicode case folding. An existing destination is refused when it is the same file (device and inode) as the source or one of its `-wal`, `-shm`, `-journal`. Both tests run again just before D8's rename.
   - D2 step 4 (line 31): a foreign object that needs an unregistered collation or function fails `VACUUM INTO` with `Io` and SQLite's message; Meguri registers nothing for it (D12 `downgrade-foreign-function`).
   - D3 (line 41): `PRAGMA foreign_keys` 0 before the first transaction, read back as 0, else `Io`. D3.2 (line 79): `PRAGMA secure_delete` on before the first statement; no `VACUUM` after the conversion.
   - D3.1 depth (lines 58–61, test list line 236): a `conf` value nested **127** levels or more, counting its own outermost array or object as level 1, is refused (`UnconvertibleConfig`, too deep); **126** are carried. A record whose object would put an array or object at column level **128** or deeper is refused (`UnreadableRecord`, reason `too deep`). Master no longer says "128 refused, 127 carried".
   - D3.1 (lines 65–66): a `usn` beyond 32 bits signed is refused (`UnreadableRecord`); a deck under id 0 is refused (reason `deck id 0`); a note type under id 0 is carried.
   - D5 (lines 131, 134, 136): −0.0 is not below a lower bound of 0.0, so it is kept and written as `-0.0`. A non-finite unbounded preset `f32`, and a non-finite filtered-deck delay or deck desired retention, refuse the copy (`UnreadableRecord`, `non-finite value` with the field).
   - D10 (line 216): the storage entry point takes a cancellation *predicate*, and "the engine adapts U24's cancellation token to it". This fits the layering: meguri-storage depends only on meguri-model and meguri-text (`meguri-storage/Cargo.toml`; `core/AGENTS.md`). The earlier conflict is gone.
   - D12 (lines 248–267): new ids `downgrade-foreign-keys`, `downgrade-graves-old-name`, `downgrade-foreign-function`, `downgrade-usn-range`, `downgrade-deck-id-0`, `downgrade-deep-json`, `downgrade-non-finite-deck`, and `downgrade-snapshot` split into `.colpkg` and `.apkg` rows. None has a golden or a `deviations.yaml` entry; `tests/downgrade.rs` asserts each (line 250).
2. **U34/U35: the structure report (`engine/change-stream#S2.5`) does not exist in the engine.** No `StructureNotReported`, `layouts` or `type_changes` in `core/crates/meguri-engine/src`. U113 and U114 own S2.5, and neither is merged. `structure-changes#S9` and `change-notetype#M10` require U34 and U35 to fill it, and `tests/op_catalog.rs` to check it. Blocking for those tests and that part only; decide who adds the report type.
3. **U35: op catalog rows.** `change-notetype#M10` and `empty-cards#E6` say the long-write entries for Change Note Type and the Empty Cards delete are "added with the implementation". `engine/op-model#O9` has no such rows and says units add entries "here and in `executor/catalog.rs` together". `catalog.rs` has only `NOTETYPE_STRUCTURE_CHANGE`. Implementers do not edit specs (`roles/implementer.md`), so a spec-writer must add the O9 rows. Blocking only for declaring those long writes.
4. **U37 acceptance pending (§2).** Not blocking to attach exports. U34's template-rename export was listed open in `review-log/U37.md` (round notes).
5. **U127 not accepted.** Its real-operation backup tests for U34 and U35 are pending (`U127-local-continuation.md`). Not blocking: T6 admission is on master. `structure-changes#S16`, `restore-to-default#D8` and `change-notetype#M12` require the backup tests in U34/U35's own suites.
6. **Held scenarios** (§3) wait for the pending prepared-scenario commit. Their activation is not blocking, but they cannot be listed active until they are on master.
7. **Long writes (plan Open Questions).** Non-answer writes during a declared long write, such as `notetype_structure_change`, wait. Default accepted. Not blocking.

## 6. Traps

### U33

- **Never write the source** (R2013). Use a read-only snapshot through the maintenance reader, never the writer or a `query_only` reader. A closed file opens with `mode=ro&immutable=1`. A `-wal` or `-journal` next to a closed source gives `Io` (`#D1`, `#D2`).
- **Destination check by file identity, not path text** (`#D1`). With the same resolved parent directory, compare the name with the source's four names after NFC and full Unicode case folding, also on a case-sensitive volume. An existing destination that is the same file (device and inode) as the source or its `-wal`, `-shm` or `-journal` is refused. Run both tests before writing anything and again just before D8's rename.
- **The `VACUUM INTO` target must not exist.** SQLite 3.54.0 refuses an existing file. Write to `<F4 temp>-snapshot`, then rename it over the F4 temp file (`#D2`). A foreign object that needs an unregistered collation or function fails it with `Io`; register nothing for it (`#D2` step 4).
- **Conversion connection** (`#D3`, `#D3.2`):
  - Turn triggers off (`SQLITE_DBCONFIG_ENABLE_TRIGGER` 0) before the first statement.
  - Set `PRAGMA foreign_keys` to 0 before the first transaction (it has no effect inside one) and read it back as 0, else `Io`.
  - Set `PRAGMA secure_delete` on before the first statement. Run no `VACUUM` after the conversion.
  - Rebuild `graves` through `temp.` without renaming a table (`#D3.3`).
  - Never switch the copy to WAL, and close the conversion connection before the read-only verification (`#D3.5`, `#D8`).
- **Depth limits** (`#D3.1`). A `conf` value of 127 levels or more (its own outermost array or object is level 1) refuses the copy (`UnconvertibleConfig`, too deep); 126 is carried. A record whose object reaches column level 128 refuses it (`UnreadableRecord`, `too deep`). An `other` sits one level deeper in its column for a note type, deck or preset, and three levels deeper for a field or template. The config table's C3 parser alone has no such check.
- **Record reads** (`#D3.1`). A `usn` beyond 32 bits signed and a deck under id 0 are refused, never truncated or carried.
- **Preset and deck values.** Apply V3's 32-bit-float bounds; an out-of-range value is written as its default, and −0.0 is not below 0.0 (IEEE compare, not a total order). A non-finite unbounded preset `f32`, filtered-deck delay or deck retention refuses the copy (`#D5`).
- **Cancellation** is a predicate passed to the storage call; the engine adapts U24's token to it. Never make meguri-storage depend on the engine (`#D10`).
- **`conf` values** are written in C5 compact form (`#D3.1`).
- **`file_*` observations** must never open the copy through the collection open gate, which would upgrade it (`storage-ops#P8.2`).

### U34

- **Matching.** Match fields and templates to the base by origin only, never by name or id. Check the base token twice: on the snapshot, and as the O7 precondition after the backup. Never replan a draft after `Stale` (`#S1.1`, `#S1.2`).
- **Plain save.** `save_in` refuses structure changes; U34 adds its own path, which calls `mark_schema_change` before writing. New ids come from `OpContext::random` (`meguri-engine/AGENTS.md`).
- **Notes and cards.**
  - Every note of the type is stamped on a field or sort change (`#S5`).
  - Generation visits only notes that still have a card. Placement includes the note type's last deck (deviation `deck-placement-regeneration`, `#S7`).
  - `reqs` come from the final fronts (`#S8`).
  - `regenerate::plan` takes U34's in-memory type; `commit_in` re-selects notes from live rows (`cardgen/AGENTS.md`).
- **Removal** (`#S12`):
  - `LastNotetype` is refused.
  - A missing note type succeeds with no backup.
  - There is no backup coalescing for removals.
  - `curModel` goes to the first remaining name under `unicase`.
  - `_nt_<id>_*` keys are removed.
- **Restore** (`restore-to-default#D3`, `#D4`, `#D4.1`, `#D6`):
  - Map by position, with fresh ids and stock row stamps.
  - With a kind change, cards past the stock count are removed with graves.
  - On a mismatch, replan once internally, then return `Stale`.
- **Observing restored types.** Scenarios never observe stock or restored template text: `notetype_templates` is only for the scenario's own note types (R908, R1959; `notetype-structure-ops#M3`).
- **Oracle undo history.** The oracle reopens before `update_notetype`, `remove_notetype` and `restore_notetype`, which empties Anki's undo history (`#M5` item 2). For undo scenarios, follow `conformance/AGENTS.md`: a non-select `db_exec` before the first undo, and `advance 1m`.

### U35

- **State between steps.** `change_notetype` acts on the latest `change_notetype_info` state, and `delete_empty_cards` on the latest scan. Racing edits go between the two steps. Keep each pair inside one segment (`conformance/AGENTS.md`). The executor passes the remembered `scm` stamp and does not re-read it (`#H2`).
- **The change** (`#M8`, `#M9`, `#M7`):
  - Run checks 3 and 4 twice: before the backup, and in the write transaction.
  - Never apply NFC.
  - Stamp every selected note.
  - Delete a template-less card at a renumbering target, with its grave.
  - Place new cards in the **new** note type's last deck.
  - A cloze type changed to itself with identity maps is `no_changes`.
- **Logical ids.** Cards a change creates have no logical id; read them with `db_exec` (`#H2`).
- **The Empty Cards delete** (`#E4`, `#E5`, `#E6`):
  - Re-evaluate each listed card against live rows; this is the `empty-cards-recheck` deviation.
  - Keep-notes keeps the lowest ordinal, then the lowest card id.
  - No deleted-notes log line.
  - Not a schema change.
  - The op descriptor carries the T6.1 safety class, so the executor takes the backup; U131 does not call it.
- **Scan output** (`#E2`). The stripped first field and labels are display text; scenarios never observe them.

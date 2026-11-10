# Batch B4-package: U120, U121, U122, U123, U124

Implementer-view brief. Facts only, each with its source. Base checked: `HEAD` = `origin/master` = `5cae7aef`. Since the first draft at `21fc5c1f`, two commits have landed: `cf03979b` (spec-writer; changes `storage/schema11-downgrade` and browser and stats specs) and `5cae7aef` (oracle-maintainer; AnkiConnect transcripts). Neither changes a package spec, scenario, golden, deviation or crate, and neither carries a `Unit:` trailer. "Committed" means `git ls-files`; "held" means untracked in this checkout (not on master).

## 1. Batch, grouping, order, priority

- Units: U120 `.apkg` reader and import, U121 note type merge on import, U122 latest `.apkg` export, U123 schema-11 writer and legacy export, U124 `.colpkg` import and export (plan `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md` §U120–§U124).
- Why grouped: one crate (`core/crates/meguri-package`) and one staging/container layer that U124a created; one testkit adapter (`core/crates/meguri-testkit/src/exec/package.rs`); one spec family (`specs/package/*`, `specs/conformance/package-ops.md`). The scenarios depend on each other: most `apkg-import/` and `notetype-merge/` scenarios make their package with the E7 export (U122), `apkg-import/legacy-2` needs the L6 legacy export (U123), and the `legacy-export/colpkg-*` and `colpkg/` scenarios need U124's E9 options (section 3).
- Milestone: all five units are in M1 "Daily review driver", together with U33, U34 and U35 (plan line 3863). The batch gate and the M1 gate apply (`docs/process/tiers.md` lines 7–9).
- Priority (`docs/process/review-first-delivery.md`): reviews and iCloud sync (U113/U114) come first, then editing, search and stats. "File import/export remains required for the first usable version." Anki server work is frozen (2026-10-26). Use `Unit:` only when the unit is complete.
- Suggested order:
  1. U120. Fixture-only scenarios first.
  2. U122. Activate the export-dependent scenarios of U120 and U121 with it.
  3. U121. Blocked by U35, and by U34's adapters for most scenarios.
  4. U123. Blocked by U33.
  5. U124. Its latest-format media path (P16, E9 `media`) can go before U123. Its legacy parts (P19, E9 `legacy`) need U120's P17 reader and U123.

## 2. Dependency status

Method: `git log --format=%B | grep '^Unit:'`. ✓ = at least one commit with that trailer. ✗ = none.

| Unit | Depends on (plan) | Status |
|---|---|---|
| U120 | U3 ✓, U4 ✓, U5 ✓, U11 ✓, U12 ✓, U20 ✓, U21 ✓, U22 ✓, U23 ✓, U24 ✓, U25 ✓, U27 ✓, U28 ✓, U29 ✓, U30 ✓, U31 ✓, U32 ✓, U40 ✓, U124a ✓* | not blocked |
| U121 | U120 ✗, U27 ✓, U29 ✓, U35 ✗, U127 ✓* | **blocked: U120, U35** |
| U122 | U120 ✗, U124a ✓*, U20 ✓, U22 ✓, U23 ✓, U28 ✓, U31 ✓, U60 ✓ | blocked only by U120, in plan order |
| U123 | U122 ✗, U21 ✓, U22 ✓, U23 ✓, U33 ✗ | **blocked: U33** (no downgrade code under `core/crates`; `storage/downgrade/*` is not active in `conformance/scenarios/storage/area.yaml`) |
| U124 | U124a ✓*, U120 ✗, U122 ✗, U123 ✗, U22 ✓, U24 ✓, U26 ✓, U31 ✓ | blocked by U120, U122 and U123 |

- \* U124a, U126 and U127 carry `Unit:` trailers only on spec and oracle commits: U124a `004b56c5`, `0533767e`, `d9d08175`, `91cf1f43`; U127 `c15b8e44`, `f74d796c`, `eb170105`. Their code exists without trailers: `meguri-package` `reader.rs`, `writer.rs`, `inspection.rs`, `entry.rs`, `file_export.rs`, `backup.rs`, `restore.rs`. U127's continuation is source only and not accepted (`docs/process/review-log/U127-local-continuation.md`). Escalation `E-20261027T193208Z-235c` is marked blocked-red for U127 (commit `7e1faec5`).
- Needed by the scenarios but not plan dependencies:
  - U34 (no trailer): the structural `update_notetype` parts and `remove_notetype`. The executor registers only the `rename`, `css`, `field_options`, `rename_template` and `template` parts (`exec/notetypes.rs` PLAIN_EDITS). `docs/process/review-log/U107-normal-sync.md:104` says "full U34 stays in M1 without a Unit trailer".
  - U35: the `change_notetype_info` and `change_notetype` adapters. They are not registered in `exec/`.

## 3. Per unit

Columns used in the scenario tables below:
- C+G: scenario and golden are committed.
- Active: no package-area scenario of this batch is active. `conformance/scenarios/package/area.yaml` activates only `collection-package/*`, `backups/*` and `backup-triggers/*`.
- "Extra needs": capabilities the Meguri executor lacks today, beyond `import.apkg`.

No plan Files entry of U120–U124 is marked "(Meguri)", so no unit writes a design doc of its own. The specs named in Files already exist.

### U120

Spec trailers:
- `package/apkg-import#I1`–`#I17`, as touched.
- `package/container#P17` (legacy reader), `package/container#P18` (classification).
- `conformance/package-ops#E6` (testkit adapter).
- Cited rules: `text/media-references#M2`, `#M3`; `media/media-files#M3`, `#M4`, `#M6`, `#M13`; `media/media-journal#J4`; `notes/note-records#F1`; `maintenance/check-database#C10.4`.
- Requirements: R1595–R1616, R2004, R2015, AE67.

Scenarios (`conformance/scenarios/package/apkg-import/`):

| Scenario | State | Extra needs |
|---|---|---|
| deck-tree, ids-cutoff, legacy-1, v1-scheduler, refusals | C+G | none (fixtures only) |
| ae67, cards-existing-pairs, conflicting, decks, ids, options-keys, presets, scheduling-on, scheduling-off, undo, update-matrix | C+G | `export.apkg` (U122) |
| media-rules | C+G | `export.apkg`, `package_media` |
| notetypes | C+G | `export.apkg`; `update_notetype.add_field`, `.remove_field`, `.template_text` (U34) |
| notetype-sort-field | C+G | `export.apkg`; `update_notetype.sort_field` (U34) |
| legacy-2 | C+G | `export.apkg` and `export.apkg_legacy` (L6, U123, which needs U33) |
| media-map-variants | prepared, held until the 2026-11-10 fixture and spec change lands on master | the held fixtures `apkg/media-subdir.apkg`, `legacy-subdir.apkg`, `legacy-no-media-map.apkg`. Committed `package-ops.md` E4.1 still says they are "still to make" |
| overlap | prepared, held (same change) | `export.apkg` |

Also `model/schema11/apkg-notetypes` (C+G). `conformance/scenarios/model/area.yaml` says it "remains pending for U120/U123's package adapters". It needs L6 and `package_col` (L7).

Plan test → scenario or Meguri test:
- AE67 → `ae67`.
- Latest-format overlap → `overlap` (held).
- Legacy `collection.anki21` with a JSON map, and `collection.anki2` only → `legacy-2`, `legacy-1`.
- `meta` 4 → `refusals`.
- No `media` entry → `media-map-variants` (held).
- `../x.jpg` and `a/b.jpg` → `refusals` (legacy traversal) and `media-map-variants` (held).
- Ratio cap → Meguri-only, I18.3. The oracle fixture is still missing (I18.1).
- Classification → Meguri unit test of the P18 table.
- Update matrix → `update-matrix`.
- Same GUID, different note type → `conflicting`.
- Note type cases → `notetypes` and `notetype-sort-field`.
- Decks → `decks` and `deck-tree`.
- Existing pairs → `cards-existing-pairs`.
- With scheduling → `scheduling-on`; without → `scheduling-off`.
- Presets → `presets`.
- Media rules → `media-rules`.
- Id shift → `ids`; card 3 days ahead → `ids-cutoff`.
- Legacy scheduler → `v1-scheduler`.
- Undo and redo → `undo`.
- Option keys (spec I5) → `options-keys`.
- Cancel, InsufficientSpace, failpoint kill and bench → Meguri-only (`tests/apkg_import.rs`, `benches/import.rs`, I18.3).
- Fuzz → `core/fuzz/fuzz_targets/package_reader.rs`, which exists and is extended.

### U121

Spec trailers:
- `package/notetype-merge#J1`–`#J13`.
- `package/apkg-import#I8`.
- `package/backup-triggers#T6.1`, `#T6.2`.
- `conformance/package-ops#E6.2` (re-apply with the token).
- Requirements: R1605, R1606, R1475, R1476, R2008.

Scenarios (`notetype-merge/`, all C+G; deviations `apkg-merge-layout(-notes,-log)`, `apkg-merge-conflict-remap(-cards)` and `apkg-merge-card-ordinal` are in `conformance/deviations.yaml`):

| Scenario | Extra needs |
|---|---|
| kind-conflict, same-schema, update-always-equal, template-renamed | `export.apkg` only |
| by-name | fixture `legacy-1.apkg`; `update_notetype.rename_field` (U34) |
| add-field, copy-cleanup, emptied-field, undo, insert-field, two-new-fields | `export.apkg`; structural field parts (U34) |
| template-moved | `export.apkg`; `add_template`, `move_template`, `remove_template` (U34) |
| guid-conflict | `export.apkg`; `change_notetype_info` and `change_notetype` (U35) |

Plan test → scenario or Meguri test:
- Extra field → `add-field`.
- Template renamed locally → `template-renamed`.
- No ids → `by-name`.
- Cloze vs standard → `kind-conflict`.
- Earlier copy → `copy-cleanup`.
- Preview declined, backup failure, backup before commit → Meguri-only (`tests/apkg_merge.rs`, J15.2; backup ordering from `package/backup-triggers` T10).
- U13 two-client run → frozen server work (section 5).

### U122

Spec trailers:
- `package/apkg-export#A1`–`#A12`, with the importer facts `#A15` and the differences `#A16`.
- `package/container#P15` (writer).
- `package/file-export#E2`–`#E5` (job and artifact).
- `conformance/package-ops#E7`, `#E10`, `#E11`.
- Requirements: R1617–R1624, R2004, R2015.

Scenarios (`apkg-export/`, all C+G):

| Scenario | Extra needs |
|---|---|
| card-selection, deck-limit-reach, default-name-clash, filtered-home-missing, no-scheduling, note-selection, presets, with-scheduling | `package_db_dump` |
| media-selection | `package_media`, with `compare.set` |
| listing, empty-selection | `package_listing` (registered) |
| future-ids | rejection `invalid_ids` |
| media-unnormalized | rejection `media_check_required` |
| deck-with-subdecks | live T1; the oracle imports after the handoff |

`apkg-export/colpkg/export-with-media` is U124's (A17).

Plan test → scenario or Meguri test:
- T1 subdecks → `deck-with-subdecks`.
- No scheduling → `no-scheduling` and `presets`.
- Media map → `media-selection`.
- Three notes → `note-selection`.
- Future id → `future-ids`. "No file created" is Meguri-only (A17).
- Cancel at 50%, disk full, 150k-card collection during answers, R1936 round trip, unchanged live hash → Meguri-only (`tests/apkg_export.rs`, `benches/export.rs`, A17).

### U123

Spec trailers:
- `package/legacy-export#L1`–`#L13`.
- `package/apkg-export#A13`.
- `package/container#P17` (the L10 reread).
- `conformance/package-ops#E8`; `conformance/model-ops#L6`, `#L7`.
- Requirements: R1481, R1619, R1625.
- The downgrade itself is U33's (`storage/schema11-downgrade`).

Scenarios (`legacy-export/`):

| Scenario | State | Needs |
|---|---|---|
| apkg-listing | C+G | L6, `package_listing` |
| apkg-columns | C+G | L6, `package_col` |
| apkg-options | C+G | E8, `package_col`, `package_db_dump` |
| apkg-media | C+G | E8, `package_media`, `package_listing` |
| apkg-oracle-import | C+G | live, E8 |
| colpkg-listing, colpkg-oracle-import (live) | C+G | E9 `legacy` and `media`: both export `{format: colpkg, …, legacy: true, media: true}` (`colpkg-listing.yaml:33`, `colpkg-oracle-import.yaml:22`), so by `package-ops.md` E5 (D8) they need `export.colpkg.legacy` and `export.colpkg.media`, which are both U124's (L8 is "(U124)") |
| unknown-keys-roundtrip | prepared, held (2026-11-10; committed L14 says "to be recorded") | L6, `import.apkg`, `remove_notetype` (U34), `package_col` |

Also `model/schema11/apkg-notetypes`.

Plan test → scenario or Meguri test:
- T1 `.apkg` → `apkg-oracle-import`.
- T1 `.colpkg` → `colpkg-oracle-import`.
- Schema-11 round trip and live hash unchanged → Meguri-only (`tests/legacy_export.rs`, L14).
- Unknown `other` keys → `unknown-keys-roundtrip` (held) and Meguri-only.
- DB equal to the oracle's → `apkg-options`, `apkg-columns`.

### U124

Spec trailers:
- `package/container#P16`, `#P19`.
- `package/apkg-export#A14`.
- `package/legacy-export#L8`.
- `package/file-export#E7`.
- `engine/collection-replace#R7`, `#R11`.
- `conformance/package-ops#E2.2`, `#E9`.
- Requirements: R1596, R1625–R1628, R2004, R1489, R1548.
- The plan says the specs are "read, not written here".

Scenarios:

| Scenario | State | Needs |
|---|---|---|
| `colpkg/media-merge`, `legacy-import`, `oracle-made-import` (live), `refused-media-member`, and their goldens | prepared, held. Committed `package-ops.md` E2.3/E5 still mark `refused-media-member` "draft"; its fixture `variants/media-member-missing.colpkg` is held | E9 `media`/`legacy`; the ColpkgImport path of P16/P19 in the `import` adapter |
| `apkg-export/colpkg/export-with-media` | C+G, live | `export.colpkg.media` |
| `legacy-export/colpkg-*` | C+G | `export.colpkg.legacy` and `export.colpkg.media` (see U123) |

`collection-package/refused-collection` (schema 12, integrity) is active already (U124a).

Plan test → scenario or Meguri test:
- Oracle-made latest `.colpkg` → `oracle-made-import` and `media-merge` (held).
- Legacy schema 11 → `legacy-import` (held).
- Schema 12 and integrity failure → `collection-package/refused-collection`.
- `CollectionReplaced`, sidecar, backup, cancel, kill before or after the rename, kill after a schema-11 rename, export during answers → Meguri-only (`tests/colpkg.rs`; P16.4 and P19 tables).
- T1 export → `export-with-media` and `colpkg-oracle-import`.
- U13 two-client run → frozen.

Existing work: P15 media export and P16 local media import are partly implemented and not accepted (`docs/process/review-log/U124-local-media-import.md`). The Rust media-input refusal is still present at `meguri-package/src/inspection.rs:173,177` (`UnsupportedMedia`).

## 4. Testkit and adapter work

Model parsing: nothing is missing. The model already reads:
- E6 import: `scenario/intake.rs` `apkg`, called from `validate.rs:314`.
- E7, E8, E9 and L6 export: `scenario/package.rs` `PackageExportArgs`, with capabilities `export.apkg`, `export.apkg.legacy`, `export.apkg_legacy`, `export.colpkg.media`, `export.colpkg.legacy`.
- E10 and E11 observations: `intake.rs:362`.
- L7 `package_col`: `validate.rs:734`.
- The `change_notetype*` and `remove_notetype` steps: `model.rs:968–974`.

Executor adapters are missing. Package registrations today (`exec/package.rs:26–30`): `export` with the part `export.colpkg` (through `registry.op`), `import` with the part `import.colpkg` (rejections `too_new` and `corrupt`) and the kind `package_listing`. The same `register` function also registers the backup ops and kinds (lines 31–37: `add_backup_files`, `force_backup`, `backup`, `save_backup_settings`, `backup_files`, `backup_settings`); leave them as they are.

| Missing | Spec | Unit |
|---|---|---|
| `import.apkg` part, with rejection `notetype_kind_conflict` added to `import` | `conformance/package-ops#E6`, `conformance/dsl#D7.2` | U120 (U121 for the token re-apply) |
| `export.apkg`; `export` must become `op_rejecting` with `invalid_ids` and `media_check_required` | `#E7`, D7.2 | U122 |
| `package_media` kind | `#E10` | U122 |
| `package_db_dump` kind | `#E11` | U122 |
| `export.apkg.legacy` (E8), `export.apkg_legacy` (L6) | `#E8`, `conformance/model-ops#L6` | U123 |
| `package_col` kind | `conformance/model-ops#L7` | U123 |
| `export.colpkg.media`, `export.colpkg.legacy` (adapter now returns "collection package options are pending") | `#E9` | U124 |
| `import` of media and legacy `.colpkg` through ColpkgImport (adapter now uses `inspect_collection_package` and `ReplaceReason::Restore` only) | `#E2.2`, `package/container#P16`, `#P19` | U124 |
| `update_notetype` structural parts, `remove_notetype` | `conformance/notetype-ops#T10.1`, `conformance/notetype-structure-ops#M1` | U34 (not in batch) |
| `change_notetype_info`, `change_notetype` | `conformance/change-notetype-ops#H1`, `#H2` | U35 (not in batch) |

Also required when adding them:
- the CLI capability contract test `core/tools/meguri-cli/tests/cli.rs` for new kinds (`core/crates/meguri-testkit/AGENTS.md`);
- the `area.yaml` activation in the same change that lands the adapter (`conformance/AGENTS.md`).

## 5. Open questions and decisions

Blocking:
- U35 is not merged. U121's moves use U35's change-notetype machinery (plan §U121), and `guid-conflict` needs its ops.
- U33 is not merged. U123, `apkg-import/legacy-2`, U124's legacy option and P19 export legs need it. The 2026-11-10 decisions in `specs/storage/schema11-downgrade.md` are committed on master (`cf03979b`); the file has no local change. They are: the `DestinationIsSource` test by file identity with NFC and case folding (D1, lines 18–19), foreign keys off (D3, lines 33 and 41), the JSON depth limit (D3.1, lines 58–59), the refusal of a `usn` beyond 32 bits signed (D3.1, line 65), the refusal of deck id 0 (D3.1, line 66) and `secure_delete` on (D3, line 33; D3.2, line 79). Build U33 against this committed version.
- U34's adapters are missing. Most `notetype-merge/` scenarios and `apkg-import/notetypes` and `notetype-sort-field` stay inactive until U34 adds them.
- Held material is not on master until the spec and oracle change lands: `media-map-variants`, `overlap`, `unknown-keys-roundtrip`, `colpkg/*` and the four E4.1 fixtures. On master, `specs/package/apkg-import.md` I18.1 and `package-ops.md` E4.1, E2.3 and E5 still call them missing or draft.

Not blocking:
- I5 says the option strings are "to be confirmed by `options-keys`". The committed golden `goldens/package/apkg-import/options-keys.json` records `"IfNewer"`, `"Always"`, `"Never"` and JSON booleans.
- Two specs disagree with the current state:
  - `apkg-export.md`'s header says "none recorded yet", but A17 says every scenario is recorded.
  - `legacy-export.md` L14 says E8 and E9 "stay draft until Meguri's typed model reads them", but the model parses them now. `package-ops.md`'s intro calls them "stable typed language".
- Frozen server work, not required now (`review-first-delivery.md`, freeze of 2026-10-26): U121's verification "two-client run in the U13 harness" and U124's "Two-client harness (U13)".
- Keep P16's media-input refusal until the whole P16 path is verified (`package/container#P16.4`).
- Recorded decisions (2026-10-27) that are binding but not open:
  - I3.1: id repair skips used ids.
  - I3.3: field-count repair with F1.
  - I7: a refused record is `Corrupt`.
  - I8: the confirmation token holds the session, the options and a preview digest; the snapshot seq is never checked.
  - I9 step 4: a duplicate GUID is logged `duplicate`.
  - I10.2: ties at one level go by package id.
  - I11.1: an unmapped ordinal is kept or skipped as the spec says.
  - I12.3: checked names go through M3; the M4 trash move happens at publication.
  - I12.4: static entries are written after the commit.
  - J9: a merge carries only `notetypes_merged`; a confirmed preview takes a backup.
  - A2: export choices are remembered on the device, not in the collection.
  - A16: export makes no day query.
  - L1: no extra legacy warning.
  - U124 plan: `file-export` E1/E7 is an earlier partial delivery; this unit adds the two options.
- Recorded decision (2026-11-10), binding but not open:
  - P19: an unnamed numbered member is ignored (`specs/package/container.md` line 434: "Decided 2026-11-10: it is ignored, as Anki ignores it").
- No spec-ahead log covers packages. The only one is `docs/process/review-log/spec-ahead-servers-open-questions-20261110.md`, which has no package entries.

## 6. Known traps

**Scenario dependencies**
- The import scenarios need the export. Only `deck-tree`, `ids-cutoff`, `legacy-1`, `v1-scheduler` and `refusals` (plus the held `media-map-variants`) can be activated with U120 alone (section 3).
- Capability strings are compared exactly: `export.apkg_legacy` (L6, whole collection) is not `export.apkg.legacy` (E8, deck form, which also needs `export.apkg`). Separator folding never applies (`package-ops.md` E5).

**Adapter behavior**
- E6.2: on `ConfirmationRequired` the adapter applies again at once with the token. A safety backup goes to the scenario's backups folder (`backup-ops.md` F1).
- E6.3: `note` is the logical id, or the integer id when there is none. `conflicting` and `missing_notetype` carry the package id. `existing_id` is not observed.
- E6.4: Meguri refuses in inspect and keeps the undo history. Scenarios observe `undo_status` after a refusal only for not-a-zip.
- E7: publication is `ExclusiveCreate`, so the adapter first removes an existing file of that name. A refused export writes no file of its name.
- E9: `media: true` writes every regular media file, referenced or not (P15, file-export E7).
- E10: `package_media` order is per executor; scenarios use `compare.set`.
- E11: read with plain SQLite, no open gate. Dumping `notetypes`, `fields` or `templates` needs A16's deviation.

**Deviations and goldens**
- No deviation entries exist yet for I17's proposed ids: `apkg-import-media-case`, `-field-count-repaired`, `-missing-notetype`, `-missing-deck`, `-duplicate-guid`, `-future-ids-skip-used`, and `apkg-merge-unmapped-ordinal`.
- Existing package entries in the committed `conformance/deviations.yaml` (lines 838–899): `package-latest-missing-media-map` (838), `apkg-merge-layout` (849), `-layout-notes` (856), `-layout-log` (863), `apkg-merge-conflict-remap` (874), `-conflict-remap-cards` (881) and `apkg-merge-card-ordinal` (893). The backup entries that follow are not this batch's. This checkout's working copy of the file has local changes, so its line numbers differ.
- Never hand-edit or bless a golden; only `tools/mg oracle record` writes goldens (`conformance/AGENTS.md`).

**Import rules (`specs/package/apkg-import.md`)**
- I3 step 3: a v1-scheduler collection is accepted only as a package collection, and it is converted (I3.2).
- I5: the five config keys are written at the start of the write phase, only when they differ, and inside the undo step. A failed or cancelled import writes none.
- I6 and I17: imported new cards keep the package's `nextPos` positions. Only G9 regeneration of target notes reads the target's `nextPos`.
- I9: an update keeps the incoming `mod`, not `now_s`. No card is generated from notes; cards come from I11 only.
- I10.2: the suffix `S` is a space plus `now_s`, fixed once per import.
- I11.2: only `queue` 2 or 3, or `type` 2, shifts `due`; `odue` shifts only for `type` 2 with `odue` ≠ 0.
- I12: checked entries go through M3 (lowercased, case variants reused or hash-named). Static entries (`_`, `latex-`) are never renamed and are skipped on any case variant. The latest-format size and SHA-1 are verified. No payload is held whole in memory.
- I14.2: `TableUntouched` on notes, notetypes, fields, templates, deck_config and tags. At most 3 replans, then `Stale`. Answers never cause a replan. A write of more than 5,000 rows is declared long (O8).
- I14.3: media are not part of the undo step.
- I14.4: a backup is taken only for `sort_field_changed`, `notetypes_merged` or a confirmed non-empty merge preview.
- The engine catalog already has `APKG_APPLY` (`meguri-engine/src/executor/catalog.rs:118`), `LongOp::ApkgApply`, the undo name `IMPORT`, and the `NotetypesMerged` and `SortFieldChanged` reasons (`schema_gate.rs`).

**Container and package formats (`specs/package/container.md`)**
- P18: the name rules come first. `collection.apkg` and `backup-*.apkg` are `Colpkg`; `.zip` is `Apkg`. Content decides only for other names.
- P17: legacy map keys are ASCII digits (leading zeros allowed). A name containing `/` is `Corrupt`; other names are kept after the F1 transform. Duplicate kept names are `Corrupt`. Legacy has no `meta`, or version 1 or 2.
- P19: legacy reuse compares size only.
- P16.5: scenarios `advance` before a `media_outside` step that the import must see.
- E2.4: a colpkg-import scenario has no setup, starts with `open`, has no logical ids, and observes `timing` after `open` and after each successful import.

**Export rules**
- `package/legacy-export`:
  - L9: two preflight checks.
  - L10: verification rereads the package against values recorded while writing.
  - L13: a legacy `.colpkg` export never touches the live collection.
- `package/apkg-export`:
  - A8: all checks run before any staging exists.
  - A10/A11: the ratio limit is checked while writing. The package is verified before publication.

**Code layout**
- The plan's file layout (`container/…`, `apkg/import/…`) does not match the crate. Its committed modules (`git ls-files core/crates/meguri-package/src`): `lib.rs`, `reader.rs` with `reader/tests.rs`, `writer.rs`, `media_map.rs`, `inspection.rs`, `inspection_zip.rs`, `entry.rs`, `file_export.rs`, `export_capture.rs`, `server_copy.rs`, `restore.rs`, and `backup.rs` with `backup/listing.rs`, `backup/naming.rs`, `backup/retention.rs`. The plan says "extend" U124a's modules: do not duplicate any of them.

**Testkit**
- Take time only from `Session::clock` and randomness only from `Session::random`.
- Read JSON with floats through `observe::parse_json`.
- New ops or kinds change the model, spec, schema and oracle driver together. Implementers do not touch the oracle; escalate.
- Source: `core/crates/meguri-testkit/AGENTS.md`.

**Process**
- Failpoint kill tests use a child `meguri-cli` built with `test-hooks` (plan §U120; I18.3).

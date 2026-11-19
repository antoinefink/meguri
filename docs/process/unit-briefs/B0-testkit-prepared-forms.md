# B0-testkit-prepared-forms

Brief for one implementer round (`tools/mg cleanroom`). Facts are from the worktree at `5cae7aef` (= origin/master), 2026-11-10 15:45. "Held" means untracked or modified in the worktree, so not on master (`git ls-files`). Other sessions are still adding held files. Count again at the round with `git status --short -uall conformance/scenarios specs` and `tools/mg goldens-check`. The worktree run `20261110T144446Z` gave 1,210 scenarios and 126 problems: 104 `invalid` and 22 `missing`. It also gave 25 schema errors, all `stats_presentation`.

## 1. Batch

- **Units:** none. This is prerequisite infrastructure: scenario-model changes in `core/crates/meguri-testkit/src/scenario/` that only recognize forms. After them every held scenario in B0's scope parses (status `pending`, never `invalid`) once it lands. No adapters, no capability registration, no activation.
- **Why grouped:** one crate, pure Rust testkit internals, same shape as the precedent commits `a4f2f6c5` ("Recognize prepared batch 3 and 4 scenario forms") and `b3f84156` ("Recognize the prepared scenario vocabulary without activating adapters"). Both carried `Role:`, `Spec:` and `Session:` trailers and no `Unit:` trailer.
- **Why it matters:** T0 fails when any scenario is `invalid`. Goldens-check fails when any scenario is `invalid`, `missing` or `stale` (`specs/conformance/dsl.md` D10). Today the testkit rejects the held forms, so the held scenarios cannot land.
- **Suggested order** (follows `docs/process/review-first-delivery.md`):
  1. Forms whose spec and schema are on master: bulk (`find_replace`, `change_deck`, `find_replace_fields`), editor fetch (`http_fixtures`, `url_files`, `add_media_from_url`) and `graph_preferences`. Not blocked.
  2. Forms that exist only in the held spec and schema diff: `corrupt_page`, `compute_fsrs_params`, `optimize_all_presets`, `optimizer_run`, `fsrs_model_check`, and the O7 absolute tolerance. Blocked by Q1.
  3. The package count constant (§4.5). File import/export is required for the first usable version ("File import/export remains required", review-first-delivery.md). Blocked by Q1: the 7 package scenarios that raise the count, their 7 goldens and the 4 fixtures they need are all untracked. The cleanroom runner exports permitted files from the recorded source commit (`tools/cleanroom/README.md`). Change the constant in the commit that lands those files, never before.
  4. Not B0. Specs on master give these forms to their units:
     - The `template` case key `fill_empty` belongs to U71 (`specs/conformance/render-ops.md` Q3: "U71 adds the `fill_empty` case key to the testkit's scenario model and `template` adapter (Q1.4 step 0), declares `template.fill_empty` and activates `preview/*`").
     - `heatmap_totals` belongs to U95 and `heatmap_report` to U96 (`specs/conformance/stats-ops.md` H4, "Meguri": "U95's implementer adds `heatmap_totals` … U96's adds `heatmap_report`"). Their shapes are now in the held schema (Q1). Their 61 scenarios stay held anyway because they have no goldens (Q2), so B0 gains nothing by taking them.
     - `stats_presentation` has no schema shape (H14). Q2.

     B0 takes any of these only if a spec change first moves the work to it.
- **Priority context:** reviews and iCloud come first (U113, U114), then editing, search and stats ("Next feature priority — 2026-10-27", review-first-delivery.md). B0 touches no review or iCloud path. It unblocks the later units for editing (U86, U87, U197), maintenance (U128, U129, U131), note types (U34, U35), packages (U120, U123, U124), the optimizer (U47), search (U81, U82) and stats (U98).
- **Gate:** pure Rust internals. A full gate may cover up to 20 such units (`docs/process/tiers.md` l.7, l.9). Before the round, record as the batch base the master commit the round starts from. At writing, that is `5cae7aef` (origin/master). The main checkout's local `master` is at `a6f79aff`: one unpushed CloudKit commit on `cf03979b` that changes no testkit file and no spec. Neither `cf03979b` nor `5cae7aef` changes a B0 fact.

## 2. Dependency status of the units that later execute these forms

"Merged" means that a `Unit:` trailer is on master (`git log --format=%B | grep '^Unit:'`).

| Unit (executes) | Depends on → merged? | Blocked by |
|---|---|---|
| U86 (`find_replace`, `find_replace_fields`; bulk-ops X5) | U85 ✗ (only U85a ✓), U24 ✓, U25 ✓, U27 ✓, U30 ✓, U20 ✓ | U85 |
| U87 (`change_deck`) | U86 ✗, U83 ✓, U27 ✓, U28 ✓, U29 ✓, U35 ✗, U30 ✓, U44 ✓, U48 ✓, U49 ✓, U122 ✗ | U86, U35, U122 |
| U197 (`http_fixtures`, `url_files`, `add_media_from_url`; editor-ops intro) | U190 ✗, U194 ✗, U2 ✓, U3 ✓, U4 ✓ | U190, U194 |
| U128 (`corrupt_page`, check-db; maintenance-ops M2.4) | U3, U4, U22–U28, U40 ✓, U127 ✓, U132 ✗ | U132 |
| U129 (check-db-notes) | U128 ✗, U27 ✓, U29 ✓, U30 ✓, U61 ✓ | U128 |
| U131 (maintenance/empty-cards) | U35 ✗, U27 ✓, U60 ✓, U61 ✓, U23 ✓, U25 ✓, U127 ✓ | U35 |
| U35 (cardgen/empty/undo, notetypes/change/undo) | U27 ✓, U29 ✓, U34 ✗, U37 ✗ | U34, U37 |
| U34 (notetypes/structure) | U27 ✓, U29 ✓, U36 ✓, U37 ✗, U60 ✓ | U37 |
| U27 (`fields_check` in the notes adapter; note-ops N5) | merged ✓ | none |
| U47 (`compute_fsrs_params`, `optimizer_run`; optimizer-ops P1.5, P3) | core: U46 ✓, U42 ✓, U43 ✓, U49 ✓, U80 ✓, U24 ✓, U26 ✓, U12 ✓; app leg: U51 ✓, U113 ✗, U114 ✗, U151 ✗, U154 ✗, U160 ✗, U163 ✗, U13 ✓, U7 ✓, U9 ✓, U10 ✓ | core leg: none; app leg: U113 and others |
| U45 (`fsrs_model_check` check; P4) | merged ✓ | none (U47 owns the adapter) |
| U95 (adds `heatmap_totals` to the model, with its adapter; `studied_today_raw` adapter; stats-ops H4) | U3, U4, U5, U11, U22–U26, U38, U40, all ✓ | none (the goldens wait for Q2) |
| U96 (adds `heatmap_report` to the model, with its adapter) | U95 ✗, U23 ✓, U24 ✓, U28 ✓, U40 ✓, U88 ✗ | U95, U88 |
| U98 (`graph_preferences`, G8.2 values) | U95 ✗, U80 ✓, U82 ✗, U23, U24, U26, U40, U3, U4 ✓ | U95, U82 |
| U226 (`stats_presentation` Swift tests; graph-presentation P22) | U4 ✓, U9 ✓, U88 ✗, U225 ✗ | U88, U225 |
| U71 (adds `fill_empty` to the model, with the `template` adapter; render-ops Q3) | U62 ✓, U64 ✓, U67 ✓, U29 ✓ | none |
| U120 (package/apkg-import) | all ✓ (U3–U5, U11, U12, U20–U25, U27–U32, U40, U124a) | none |
| U123 (package/legacy-export) | U122 ✗, U21 ✓, U22 ✓, U23 ✓, U33 ✗ | U122, U33 |
| U124 (package/colpkg) | U124a ✓, U120 ✗, U122 ✗, U123 ✗, U22, U24, U26, U31 ✓ | U120, U122, U123 |
| U81 (search text) | U80, U20, U22, U29, U61, U82a, all ✓ | none |
| U82 (search names/history/props) | U81 ✗, the rest ✓ | U81 |

Source: the `**Dependencies:**` lines of `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md` (`### U<n>.`).

## 3. Specs, scenarios, mapping

**Spec trailers** (cite only those whose text is on master when the round runs; see Q1):
- On master: `conformance/dsl#D6`, `conformance/dsl#D6.9`, `conformance/dsl#D6.10`, `conformance/dsl#D7.2`, `conformance/dsl#D8`, `conformance/bulk-ops#X1`, `conformance/bulk-ops#X2`, `conformance/bulk-ops#X3`, `conformance/editor-ops#E2`, `conformance/editor-ops#E3`, `conformance/editor-ops#E4`, `conformance/stats-ops#H13`, `conformance/package-ops#E5`, `conformance/observations#O4`.
- In held text only: `conformance/maintenance-ops#M2` (master still says "Nothing implements it yet"), `conformance/optimizer-ops#P1`–`#P5` (the file is untracked) and `conformance/observations#O7` (the absolute-tolerance row).
- Not B0 (§1 step 4): `conformance/render-ops#Q1`, `#Q3` (U71); `conformance/stats-ops#H1`, `#H4`, `#H11` (U95, U96); `#H14` and `stats/graph-presentation#P22` (U226).

**Meguri design docs:** none. B0 has no plan entry and no "(Meguri)" files.

**Committed scenarios:** none becomes active. Every committed scenario must still load with its recorded hash (`tests/scenario_format.rs` checks every committed golden).

**Held scenarios:** 159 untracked files under `conformance/scenarios/`: 158 scenarios and the new area file `review/area.yaml`. `git ls-files conformance/scenarios/review` is empty. The area file has `active: []`. The forms were taken by parsing every file and checked against the goldens-check run.

| Held group (count) | Golden | New forms used (the testkit lacks them) | Held until | Executing unit |
|---|---|---|---|---|
| browser/find-replace/* (11): ae47, all-fields, all-notes, card-generation, field-names, nfc, no-match, refusals, regex, tags, unchanged-match | yes | op `find_replace` (10 files); field-names uses only the observation `find_replace_fields` | B0 model + landing | U86 |
| browser/bulk/change-deck/{basic,presets} (2) | yes | `change_deck` | B0 model | U87 |
| editor/paste/url/{file,names,status,stored-name,timeout} (5) | yes | setup `http_fixtures` (4 files), setup `url_files` (file), op `add_media_from_url` | B0 model | U197 |
| maintenance/check-db/corrupt-page (1) | yes | `corrupt_page` | Q1 + B0 model (M2.4: "it lands with that model") | U128 |
| maintenance/check-db/{filtered-deck-kept,future-ids-lrt,meguri-studied-handoff} (3) | yes | none | landing with the held specs and deviations | U128 |
| maintenance/check-db-notes/{default-deck-filtered,template-now-nonempty} (2) | yes | none (the deviation `check-db-default-deck-filtered` is held in deviations.yaml) | landing | U129 |
| maintenance/empty-cards/{report,undo-redo,incremental-sync} (3) | yes | none (incremental-sync has `actors`) | landing; see Q7 | U131 |
| cardgen/empty/undo (1) | yes | none | landing | U35 |
| notes/fields-check/{cloze-unicase,entity-markup} (2) | yes | none (`fields_check` is in the model) | landing | U27's notes adapter has `fields_check` (note-ops N5). No source names a unit that activates these two. `notes/area.yaml`'s comment covers only the eight committed notes scenarios. |
| notetypes/change/undo (1) | yes | none | landing | U35 |
| notetypes/structure/{first-field-duplicates,remove-last,undo-add-template,undo-removals,undo-restore} (5) | yes | none | landing | U34 |
| package/apkg-import/{media-map-variants,overlap} (2) | yes | none; they need the held fixtures `package/apkg/{media-subdir,legacy-subdir,legacy-no-media-map}.apkg` | landing + count (§4.5) | U120 |
| package/colpkg/{legacy-import,media-merge,oracle-made-import,refused-media-member} (4, new dir) | yes | none; refused-media-member needs the held fixture `package/variants/media-member-missing.colpkg` | landing + count | U124 |
| package/legacy-export/unknown-keys-roundtrip (1) | yes | none | landing + count | U123 |
| sched/optimizer/* (8): adopt, health-check, items, not-better, relearning-check, weight-search, preset-names, live-adoption | yes | `compute_fsrs_params` (7 files), `optimize_all_presets` (preset-names), `optimizer_run` + `fsrs_model_check` (live-adoption) | Q1 + B0 model | U47; preset-names is reference scope (Q4) |
| search/{decks/preset-missing, dupe/entities, errors/ae42, history/odd-ease, props/due-extremes, regex/glob-size, text/nc-escapes, text/pattern-limit-unreached} (8) | yes | none (the deviations `search-pattern-limit-unreached*` are held) | landing; R1881 check before activation (review-first-delivery.md) | U81, U82 |
| stats/presentation/preferences-read (1) | yes | `graph_preferences` | B0 model | U98 |
| stats/day-buckets/* (17), stats/heatmap-totals/* (15), stats/widget-today-count/limdecks-vs-studied (1) | **no** | `heatmap_totals` (33 files); `studied_today_raw` (already in the model) | Q2 (goldens); model kind U95 | U95 |
| stats/heatmap/* (16), stats/streaks/* (11), stats/heatmap-config/writer-output (1) | **no** | `heatmap_report` (28 files) | Q2 (goldens); model kind U96 | U96 |
| stats/presentation/* other than preferences-read (15) | **no** | `stats_presentation` | Q2 (shape and goldens) | U226 (data: U98, U99) |
| Drafts (22): browser/entry/{default-and-empty,texts}, browser/sidebar/{combine,item-searches}, editor/add-flow/{add-count,sticky-toggle}, editor/image-occlusion/{add-check,audio-fields}, notetypes/manage/list-order, review/actions/{bury-note-head,forget-requeue,set-due-requeue}, review/congrats/learning-boundaries, review/edit/blank-front-stays, review/finished/{learning-window,ready-in}, review/timing/answer-cap-home-preset, review/undo/{bury-position,thirty-answers}, search/reference/examples, storage/settings-keys/{prefs-keep-seconds,review-key-writes} | **no** | none: goldens-check gives each one `missing`, not `invalid` | goldens recorded; most cite held specs (Q1) | the unit of the spec that each file's header cites; no B0 work |

60 held scenarios have goldens, all of them untracked under `conformance/goldens/` at the same ids. 98 have none: 76 under stats/ and 22 drafts (10 of them under review/). Every scenario without a golden stays held, because once it lands goldens-check reports it as `missing`.

**Not in this worktree:** `render/preview/fill-empty` and `fill-empty-cards`. Their goldens are recorded, but the files "stay out of `conformance/scenarios/` until the testkit's scenario model accepts the key" (`specs/render/preview.md` V16; render-ops Q3). They land with U71, not B0.

**Plan test scenarios → ids:** not applicable, because B0 has no plan unit. Add these Meguri-only parser tests and follow the existing patterns:
- `tests/scenario_format.rs` `held_step_vocabulary_is_strict_and_remains_unregistered` (l.2979): one valid example per new op. Reject null, a list and an unknown key. Assert that the op is in `needs().ops` and not in `capabilities().ops`.
- `tests/prepared_batches.rs` `new_observations()` (l.379) and the schema helper `conforms` (l.30): accept or reject each new observation request exactly as the schema does.
- Load every held scenario of B0's groups with its golden hash. This needs the files (Q1).

## 4. Testkit work (each new name was grepped in `core/crates/meguri-testkit/`: 0 hits)

### 4.1 Ops (`Step` enum, `Step::NAMES` and `Step::name`, `src/scenario/model.rs` l.966, l.1160)

| Op | Shape (schema `$defs/step`) | Spec |
|---|---|---|
| `find_replace` | `{notes?, find, replace, regex?, ignore_case?, in?}`. `notes` is a non-empty list of unique note refs. `find` and `replace` are strings and may be empty. `in` is `all`, `tags` or `{field}`. Defaults: `regex` false, `ignore_case` true, `in` all. Rejections: `invalid_regex`, `space_in_replacement`, `field_count`. | bulk-ops X1; dsl D7.2 |
| `change_deck` | `{cards, deck}`: `$defs/distinct_card_refs` and a non-empty name. Rejection: `filtered_deck`. | bulk-ops X2 |
| `add_media_from_url` | `{url}`, which must match the pattern `^\{(fixture\|files)\}/`. A `{fixture}` URL without `setup.http_fixtures` is `invalid_scenario` (E4). Rejection: `fetch_failed`. | editor-ops E4 |
| `corrupt_page` | `{table}`, one of the 12 schema-18 tables (`col, notes, cards, revlog, graves, deck_config, config, tags, fields, templates, notetypes, decks`). No output and no rejection. Not allowed in actor scenarios or before a `handoff` (M2.3; the oracle refuses these before any step runs). | maintenance-ops M2 (held) |
| `compute_fsrs_params` | `{preset, relearning_steps, form?}`: a non-empty string, a u32 (`$defs/u32`, up to 4,294,967,295) and `one_preset` or `all_presets`. Rejections: `invalid_cutoff`, `invalid_search`. | optimizer-ops P1.1 (held) |
| `optimize_all_presets` | `{deck}`, a non-empty name. Oracle only: Meguri never registers it (P2). | optimizer-ops P2 (held) |
| `optimizer_run` | `{}` (`maxProperties: 0`) | optimizer-ops P3 (held) |

### 4.2 Observation kinds (`ObservationKind`, `ALL`, `name`, `shape`, model.rs l.2077, l.2184)

| Kind | Request | Spec |
|---|---|---|
| `find_replace_fields` | `{notes: [#nN…]}`: non-empty and unique, at most one request per observe step | bulk-ops X3 |
| `graph_preferences` | no arguments (schema type `null`) | stats-ops H13 |
| `fsrs_model_check` | `{presets: [name…]}`: non-empty and unique, each name non-empty | optimizer-ops P4 (held); observations O4 (held row) |

Not B0 (§1 step 4), listed so that the model is not shaped against them:
- `heatmap_totals` and `heatmap_report`: bare, or `{decks: [name…]}` with at least one name, each name once. The held `scenario.schema.json` has `$defs/heatmap_decks` (l.354–380, with the bare-name pattern at l.380) and the request keys at l.651–652. The held `observation.schema.json` has the values (l.1072–1089). Master's schemas have neither. U95 and U96 add the kinds to the model (H4).
- `stats_presentation`: `[{scope, history, graph, controls}…]`. No schema shape exists (H14; Q2). U226.
- The `template` case key `fill_empty` (boolean, default false, on the new-note and card cases; `true` needs `template.fill_empty`): render-ops Q1.2–Q1.4 and dsl D8. In the schema it is at l.1239 and l.1251 on master, and at l.1261 and l.1273 in the held schema. U71 (render-ops Q3).

`studied_today_raw` is already in the model (`ObservationKind::StudiedTodayRaw`). Its adapter belongs to U95.

### 4.3 Setup sections (`Setup`, `#[serde(deny_unknown_fields)]`, model.rs l.412; `Setup::ORDER`, l.433)

- `http_fixtures` (`$defs/http_fixture`; editor-ops E2):
  - Each entry is `{path, status?, content_type?, text|fill?, delay_ms?, byte_delay_ms?, redirect_to?}`.
  - Ranges: `status` 200–599; `delay_ms` ≤ 35,000; `byte_delay_ms` ≤ 15,000.
  - `content_type` must match `^[!-~]([ -~]*[!-~])?$`.
  - Never both `text` and `fill`.
  - `redirect_to` only with status 301, 302, 303, 307 or 308. A 204 has no body.
  - Duplicate paths and a dangling `redirect_to` are `invalid_scenario` "when the setup runs" (E2).
- `url_files` (`$defs/media_file`; editor-ops E3): `{name, text}` or `{name, fill}`, each name once.
- Order per dsl D6: `config, presets, decks, notetypes, notes, cards, revlog, media_files, http_fixtures, url_files`. `ORDER` grows from 8 to 10, and `sections()` grows with it.

### 4.4 Comparison

- `observations.md` O7 (held row): `eval.log_loss` of a `compute_fsrs_params` output compares with absolute tolerance 0.002, and only when both values are written with a fraction or an exponent (optimizer-ops P5). It goes in `src/diff/`.
- The `stats_presentation` relative tolerance 1e-6 (O7, already on master) is needed only with U226.
- These rules matter only once a scenario runs. Pending scenarios are not compared.

### 4.5 Constants and tests that change

- `Step::NAMES: [&str; 87]` (model.rs l.1160): 87 → 90 for the three master ops, and 94 with the four held ops. It is asserted at `tests/scenario_format.rs` l.2996–2997, `tests/prepared_batches.rs` l.404 and `core/tools/meguri-cli/tests/cli.rs` l.229.
- `ObservationKind::ALL: [_; 64]` (model.rs l.2184): 64 → 66 (`find_replace_fields`, `graph_preferences`), and 67 with `fsrs_model_check`. It is asserted at `tests/scenario_format.rs` l.3188, `tests/prepared_batches.rs` l.403 and l.405, and `cli.rs` l.186. The three kinds of U95, U96 and U226 bring it to 70 later.
- `prepared_package_contracts_load_with_their_recorded_hashes` (`tests/scenario_format.rs` l.2752–2758) walks `conformance/scenarios/package` on disk and checks each file's golden hash. `assert_eq!(files.len(), 29 + 16 + 14 + 2 + 9 + 14)` = 84. Master has 84 package scenarios (85 tracked yaml files, including `area.yaml`). The count becomes **91** when the 7 held package scenarios land: apkg-import 20→22, colpkg 0→4 (new dir), legacy-export 7→8. Each scenario needs its golden, and they need the 4 held fixtures. Change the constant in the same commit as those files (§1 step 3). In a worktree that holds them, the test already sees 91 and fails against 84.
- `the_text_functions_are_the_schemas` (`scenario_format.rs` l.1376–1399) asserts that every `ObservationKind` is in the schema. A kind therefore cannot enter the model before its shape is in the tree's schema.
- The CLI capability contract `core/tools/meguri-cli/tests/cli.rs` changes with every new kind and op (`core/crates/meguri-testkit/AGENTS.md`). Run it in focused verification.
- `Registry::op_rejecting` (`src/exec/meguri.rs` l.118) asserts that every name is in `Step::NAMES`. Register nothing in B0.

## 5. Open questions and decisions

- **Q1 (blocks §1 steps 2 and 3).** The held spec and schema text is not on master. The cleanroom runner exports permitted files from the recorded source commit (`tools/cleanroom/README.md`).
  - Held files: `conformance/schema/{scenario,observation}.schema.json` and `protocol.json`; `specs/conformance/{dsl,observations,maintenance-ops,stats-ops}.md` (modified); `specs/conformance/optimizer-ops.md` (untracked); and 57 other held specs (25 modified, 32 untracked; at 15:48 `git status --short -uall specs` showed 62 in all, and the count is still growing). The held schemas add `corrupt_page`, `compute_fsrs_params`, `optimize_all_presets`, `optimizer_run` and `fsrs_model_check`, and also the shapes of `heatmap_totals` and `heatmap_report`.
  - The 158 scenarios, `review/area.yaml`, the 60 goldens, the 4 package fixtures and the `deviations.yaml` entries are held too.
  - Decision needed: commit the spec, schema and protocol text first. That is safe, because no test requires a schema op to be in the model (the test only checks that model kinds ⊆ schema, l.1376). Then run B0. Then land each held group that has goldens, with its goldens and fixtures, in the B0 commit or after it. Without that decision, B0 is limited to §1 step 1, and synthetic tests stand in for the held files.
- **Q2 (blocks the 76 stats scenarios without goldens; not B0).**
  - `heatmap_totals` and `heatmap_report` (61 scenarios): the shapes are in the held schema (Q1). The held stats-ops H4 and H11 say that the driver has both kinds on the add-on image (`oracle/heatmap/`). That image installs the add-on's public source, pinned by hash, so it "needs no copy from the user and no user step". This replaces the user step `heatmap-addon` and `MEGURI_HEATMAP_ADDON`. What still blocks them: `tools/mg` does not yet route heatmap selections to the add-on image, so no goldens exist (H4). Their deviation entries stay held until then.
  - `stats_presentation` (15 scenarios): no schema shape and no executor, and the graphs-page image does not exist (H14). The oracle maintainer adds them.
  - H4 says these scenarios "land inactive ahead of the driver". D10 freshness fails on `missing` (and on `invalid` until the model has the kinds), so they stay held.
- **Q3 (non-blocking).** The `render/preview/fill-empty*` files are not in this worktree. They land with U71 and their goldens (render-ops Q3).
- **Q4 (non-blocking).** `sched/optimizer/preset-names` is "to be declared reference scope in `sched/area.yaml`" (`specs/sched/optimizer.md` O17.3; dsl D9.4). That is a change for the spec writer. Until then it is an ordinary pending scenario. Meguri never registers `optimize_all_presets`.
- **Q5 (non-blocking).** Who adds the O7 absolute tolerance, B0 or U47? Nothing uses it until U47 runs the scenarios.
- **Q6 (non-blocking).** editor-ops E4 (decided 2026-11-10): the URL fetch and naming live in the core, so that `meguri-cli` can run the op: "the plan's U197 file list gains that core module (follow-up for the plan)".
- **Q7 (non-blocking).** `maintenance/empty-cards/incremental-sync` is an actor scenario (`sync`), and Anki server work is frozen (review-first-delivery.md, 2026-10-26). The parser already reads actors, so this is an activation question for U131.
- **Q8 (non-blocking).** Stale text on master: `specs/conformance/empty-cards-ops.md` Y4 says that U131's E7 scenarios "have no goldens yet", but held goldens exist. Fix it when they land.
- **Q9 (decision for the implementer).** Whether to enforce at load time the rules that the oracle refuses before running: `corrupt_page` in an actor scenario or before a `handoff` (M2.3), and `{fixture}` without `http_fixtures` (E4). If the testkit mirrors them, such files are `invalid` on both sides.
- **U47 note (not B0).** "Before U47 builds on an O19 item, add its named test and settle the recorded correction" (review-first-delivery.md, 2026-10-27; `specs/sched/optimizer.md` O19).
- `docs/process/review-log/spec-ahead-servers-open-questions-20261110.md` has no item on these forms.

## 6. Traps

- From `core/crates/meguri-testkit/AGENTS.md`:
  - Optional members that the schema types without null use `#[serde(default, deserialize_with = "present")]`. A bare `Option` reads null as missing.
  - Mapping arguments go through `op_mapping` or `mapping`. A derived struct also accepts the list of its values, which the schema forbids.
  - A new op or kind changes `model.rs`, the spec and the schema together. f32 values go through `observe::f32_value`, and floats are read through `observe::parse_json`, never through `serde_json::from_str`.
- Do not touch the parsing in `scenario/yaml.rs`. The scenario hash must equal the oracle's `canon.py` byte for byte (`scenario_format.rs` checks it against every golden).
- Recognition only. A registered adapter would turn held scenarios from `pending` into run attempts (dsl D8). `held_step_vocabulary…` asserts that the new ops are not capabilities. Never register `optimize_all_presets` or a no-op adapter.
- None of the new kinds goes in `ObservationRequest::reads_no_collection` (D8 list, model.rs l.2414). They all read the collection.
- Run-time rules are not load-time rules:
  - `find_replace` keeps refs to deleted notes (X1). `in: {field}` matches under unicase at run time.
  - An unknown `change_deck` deck is `unknown_ref` (X2).
  - A `compute_fsrs_params` preset is resolved by exact name, never under unicase. An unknown preset is `unknown_ref`, and two presets with the same exact name are `invalid_scenario` (P1.1).
  - `http_fixtures` duplicate paths and dangling redirects are checked when the setup runs (E2).
- Colpkg import scenarios must start with `open` and have no setup, no `add_note` and no logical ids (package-ops E2.4). `src/scenario/package.rs` l.213–257 already enforces this, so the held colpkg files must pass it unchanged.
- `conformance/AGENTS.md` says "change spec, schema, the testkit model and, for a new op or kind, the oracle driver together". The driver side exists for every form in B0's scope. Implementers never read or edit `oracle/` (`docs/process/roles/implementer.md`). That includes `oracle/driver/ops/bulk.py`, which `bulk-ops.md` X5 names.
- Apply the `find_replace` refusal undo semantics only in U86's adapter: Anki empties its history on `invalid_regex` and `field_count`, and Meguri keeps it (X1).
- Every held scenario without a golden (76 under stats/, 22 drafts) stays out of master until it has one. Landing such a file turns goldens-check red (`missing`). The stats ones also turn T0 red (`invalid`) until the model has their kinds.

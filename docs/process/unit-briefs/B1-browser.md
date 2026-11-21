# Unit brief: B1-browser (U84, U85, U86, U87, U88)

Facts as of `cf03979b` (= `origin/master`, 2026-11-10, "Correct six unimplemented-unit specs after an adversarial audit", which put the `browser/columns` and `browser/sorting` corrections on master). The local checkout is one unpushed commit ahead (`51c66708`, AnkiConnect transcript recordings and recorder only; no browser or search spec, scenario or code). Implementer view only: no `oracle/`, `refs/`, `local-data/` or Anki source was read. "Master" means committed at that head (`git ls-files`); untracked or modified working-tree files are named as prepared and held. `git diff HEAD -- specs/browser` is clean.

## 1. Batch

- **Units.** U84 rows, columns, sorting; U85 search sessions and paged query API; U86 bulk framework, Find and Replace, Edit Field; U87 card and note bulk actions; U88 search-text services. Plan: `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md` ("### U84." to "### U88.").
- **Why grouped.** One dependency chain over `meguri-search` and the Browse engine: U84 → U85 → U86 → U87. U88 shares the search crate and `browser/config-keys`. Its builder feeds the generated searches of the same Browse screens.
- **Milestones.** U84, U85 and U88 are M1; U86 and U87 are M2 (`docs/process/milestones.yaml`, plan "Delivery Milestones"). The batch crosses a milestone boundary, so a gate is required there (`docs/process/tiers.md`).
- **Suggested order.** U88 (unblocked now), U84 (cells, catalog and sort keys; scenarios that need U81/U82 terms wait), U85 (blocked on U81 and U82), U86, U87 (blocked on U35 and U122). Within the batch, record each unit's starting commit and the batch base. Run one full gate `tools/mg gate --base <batch-start>` at the batch end and at the M1/M2 boundary (`docs/process/tiers.md`).
- **Priority context.** `docs/process/review-first-delivery.md`: working reviews first. Then (2026-10-29) reviews and iCloud sync: U113 and U114 and their dependencies come before the remaining editing, search and statistics features. Neither U113 nor U114 carries a `Unit:` trailer yet. This batch is search/browser work, so it ranks after them. Anki-server work stays frozen.

## 2. Dependency status

"Merged" means a `Unit: <id>` trailer exists at HEAD (`git log --format=%B | grep "^Unit:"`).

| Unit | Depends on | Merged | Not merged (blockers) |
|---|---|---|---|
| U84 | U80, U82, U83, U85a, U23, U28, U29, U60, U20 | U80, U83, U85a, U23, U28, U29, U60, U20; U82a (the FSRS/card-data extractors split from U82) | **U82** (plan: "shared due expression"; see Q1). Scenarios `browser/sort/subset` (needs-state, needs-deck) and `browser/sort/unsortable` (needs-text, U81) need U82/U81 terms |
| U85 | U80, U81, U82, U83, U84, U22, U24, U6 | U80, U83, U22, U24, U6 | **U81, U82, U84** |
| U86 | U85, U24, U25, U27, U30, U20 | U24, U25, U27, U30, U20 | **U85** |
| U87 | U86, U83, U27, U28, U29, U35, U30, U44, U48, U49, U122 | U83, U27, U28, U29, U30, U44, U48, U49 | **U86, U35, U122** |
| U88 | U80, U83, U28, U29, U30 | all | none. `search/builder/names` runs `search` with needs-deck/tag/notetype/text terms (U81, U82) |

Other gaps: U90 (unmerged) owns the U85 budgets and the sorted-list cache hook. U47 (unmerged) is needed for U85's "re: during optimizer training" test. U242 (unmerged) holds the R1831 and R1832 benchmarks. U43's review log assigns U85 a closure obligation: "U85 after U81: production slow streaming regex search concurrent with review and held Sync snapshot; answer p95 <= 5 ms" (`docs/process/review-log/U43.md`).

## 3. Per unit

Spec ids are `<area>/<topic>#<id>` (`specs/README-format.md`). Cite the sections each commit implements.

### U84: rows, columns, sorting

- **Spec ids.** `browser/columns#C1` to `#C15`. `browser/sorting#S1` to `#S11`. `browser/config-keys#K3`, `#K5`. `search/card-extractors#X3`, `#X4`, `#X5`, `#X7` (Retrievability, Position, card data). `conformance/browser-ops#B1` to `#B6` for the adapter.
- **Already implemented.** By `Spec:` trailer, U85a implemented only `browser/columns#C7.4`, `#C10` and `#C16` (`d87bf626`; the fixes `7229f2f3` and `2bbea61e` add only `#C16`). Reuse `qa.rs` and `deck_cell.rs` for those. U85a only *specified* C3.1 and C7.2 (columns.md intro: "U85a specified C3.1, C7.2, …"); no commit implements them.
- **U84 implements C3.1** for `browser_rows` in both modes: the read order, `deleted`, `damaged` and the `unreadable` error (table, column, row id) that `cf03979b` added. No core code has it: there is no `unreadable` in `meguri-search/src/browser` or `meguri-engine/src/browse`, and `CardExcerpt` has only `Missing` and `Damaged` (C16.3 fails the whole excerpt call on an undecodable card instead).
- **Meguri docs to write.** None marked (Meguri) in the plan.
- **Committed scenarios with goldens, inactive.** `browser/sort/*` (16: sort-field, card-type, deck, due, ease, interval, counts, created, modified, note-type, tags, position, fsrs, unsortable, damaged, subset) and `browser/columns/*` (6: cards-mode, notes-mode, question-answer, due-dst, aggregates, row-errors). Already active, owned by U85a: `browser/columns/question-deck/*` (8) (`conformance/scenarios/browser/area.yaml`).
- **On master since `cf03979b` (binding spec).** The `unreadable` row error (C3.1); cells reading card data by X3, so a non-string `cd` gives no keys (C1, "Decided 2026-11-10"); cells reading `due`/`odue`/`mod`/`ivl` with the `model/row-records` R4 tolerances while sort keys read stored values (C1, S2); C12's years and invalid instants; `f32` percents and spans with `-0`, `inf`, `NaN` (C7.12, C11, C12); the notes-mode Reviews/Lapses wrap mod 2^32 (C8, C13); exact SQL averages and sums for the notes-mode keys (S5); S1 "the core never chooses a direction"; the U81/U82 tags on `sort/unsortable` and `sort/subset` (S11).
- **Prepared, held.** None.
- **Plan test → coverage.**

| Plan test | Covered by |
|---|---|
| AE50 notes-mode Due; first Due sort descending | `browser/sort/due`; the first direction is catalog metadata (`browser/columns#C2`, `browser/sorting#S1`), a unit test (Q2) |
| AE44 row state | `browser/columns/cards-mode` (flags 1 and 9, marked), `notes-mode` (marked tint) |
| Sort Field `10, 9, apple, Banana` | `browser/sort/sort-field` |
| Due mixes new, intraday and review cards | `browser/sort/due` |
| Ease new cards last; Created | `browser/sort/ease`, `browser/sort/created` |
| Card Type cloze numeric; notes-mode count | `browser/sort/card-type` |
| Notes-mode Deck, Card Modified, Reviews, Lapses, Position | `browser/sort/deck`, `modified`, `counts`, `position`, `subset` |
| Due wrapped; filtered Deck cell | `browser/columns/cards-mode` |
| Interval markers; notes-mode average | `browser/columns/cards-mode`, `notes-mode`, `aggregates` |
| Retrievability equal to `prop:r` | `browser/columns/cards-mode` (FSRS with and without `lrt`); Meguri test against U82a's function (`#C11`: equal only when `lrt` is in 0..now). `prop:r` itself needs U82 |
| `bqfmt`, answer prefix, `[sound:]` | `browser/columns/question-answer`, `question-deck/browser-format` |
| Unsortable `question`, notes-mode `stability` | `browser/sort/unsortable`, `browser/sort/fsrs` |
| Reversed equals descending | Meguri `core/crates/meguri-search/tests/sort_props.rs` (`#S8`); also no schema object is created (`#S10`) |
| (no plan line) | `browser/sort/damaged`, `note-type`, `tags`, `interval`, `browser/columns/due-dst`, `row-errors` (`deleted`, `damaged`; it has no `unreadable` case) |
| (no scenario; `browser/columns#C15` last paragraph: "U84's unit tests cover them from this text") | Meguri unit tests for: card data with a non-string `cd` in the cells (C1); `unreadable` rows (C3.1); invalid and five-digit-year instants (C12); non-finite and `-0` percents and time spans (C7.12, C11, C12); the notes-mode Reviews/Lapses wrap (C13); a deck count over a missing deck (C8); `bafmt`, `bfont`, `bsize`; RTL sort fields; template errors and blank cloze cards in Question/Answer |

- Activate in `conformance/scenarios/browser/area.yaml` in the change that lands the adapter (`conformance/AGENTS.md`; `conformance/browser-ops#B6`).

### U85: search sessions and paged API

- **Spec ids.** `browser/result-sets#H1` to `#H16`. `search/query-model#Q9`, `#Q10` (streaming). `browser/sorting#S8`, `#S9`. `browser/columns#C16.5` (U85 cross-checks C16). Also `engine/lifecycle` (L3, L11) and `engine/change-stream`.
- **Meguri docs.** `specs/browser/result-sets.md` (marked a Meguri contract in the plan) is already on master. Nothing else is left to write.
- **Committed scenarios with goldens, inactive.** `browser/modes/*` (11: toggle-sets, toggle-damaged, toggle-context, selection-sets, sorted-both-modes, sorted-damaged, streamed-sorted, streamed-damaged, one-shot, rows-current, rows-excerpts), recorded in `25f4fa15`. All except `rows-excerpts` carry needs-* tags (U81/U82 terms).
- **Prepared, held.** None.
- **Adapter contract** (`browser/result-sets#H16`, decided 2026-11-10). The browser adapter answers every `search` of a `browser/` scenario through the core entry points. An unsorted request goes through `search_ids`. A sorted one goes through `start_search`, then `resolve_selection` over 0..count−1. A slow-term plan streams with a test chunk-size setting of 1. `unsortable_column` is recorded as `unsortable`.
- **Plan test → coverage.**

| Plan test | Covered by |
|---|---|
| B supersedes A, seq order | Meguri `core/crates/meguri-engine/tests/browse_handles.rs` (H2–H4) |
| Streamed FirstPage equals the full prefix; count | `browser/modes/streamed-sorted` (membership and order) plus `browse_handles.rs` |
| Streamed `-re:x`, `re:x or tag:y`, `-(re:x tag:y)` | `browser/modes/streamed-sorted`, `streamed-damaged` |
| Toggle with no predicate run (counter); re-run after an edit | `browser/modes/toggle-sets` (sets) plus `browse_handles.rs` (counter, H11) |
| AE45 data: `invalidated_rows` | `browser/modes/rows-current` plus `browse_handles.rs` (H10 index arithmetic) |
| Delete → `removed` = [4] | `browse_handles.rs` (H10); `rows-current` shows `deleted` |
| Mode toggle selection mapping; `resolve_selection` 5 ids | `browser/modes/selection-sets` plus `browse_handles.rs` |
| `search_ids` all notes; error text | `browser/modes/one-shot` (error kind); message text and range are Meguri-only (H16) |
| `StaleGeneration` after `CollectionReplaced` | `browse_handles.rs` |
| CoreBridge cancel; stale-seq filter | `app/Packages/MeguriCore/Tests/CoreBridgeTests/SearchSessionTests.swift` |
| `reverse` then rows equals descending | `browser/modes/sorted-both-modes` plus `browse_handles.rs` |
| `re:` first page during optimizer training | Meguri test; needs U47 and U90 budgets |
| `rows` Question/Deck equal to `card_excerpts` | `browser/modes/rows-excerpts` |
| (no plan line) | `toggle-damaged`, `toggle-context`, `sorted-damaged` |

### U86: bulk framework, Find and Replace, Edit Field

- **Spec ids.** `browser/find-replace#F1` to `#F13`. `conformance/bulk-ops#X1`, `#X3`, `#X4`, `#X5`. `browser/bulk-actions#B16` (counts, steps). `engine/op-model` (O8, O9 `bulk_op`). `engine/undo` (A2, A9, M2). `browser/result-sets#H12` (`resolve_selection`).
- **Meguri docs to write.** `specs/browser/bulk-framework.md` and `specs/browser/batch-edit.md` are absent on master. Other specs already cite them as `browser/bulk-framework` and `browser/batch-edit`: `browser/find-replace` intro, F3 (`selection_summary`), F8, F10 and F11; `browser/bulk-actions` intro, B7.2 (Change Deck runs as a bulk op) and B16 (`{affected, considered}`, Edit Field → `browser/batch-edit`); `conformance/bulk-ops#X3`. The framework doc must record the no-match rule. F9 already decides it: count 0 records no undo step and keeps `col.mod`.
- **Committed scenarios.** None. Batch edit has no oracle; it is Meguri-only, test-first from R1208–R1211.
- **Prepared, held.** These are untracked files with goldens: `browser/find-replace/{ae47, regex, all-fields, nfc, card-generation, tags, all-notes, refusals, no-match, unchanged-match, field-names}`. They are held until the testkit model parses the `find_replace` op and the `find_replace_fields` kind. Until then they would be `invalid` and fail T0 (`conformance/dsl#D10`). `specs/conformance/dsl.md` D7.2 says "Meguri's executor and the typed model pending". Commit `25f4fa15` recorded only scenarios the testkit already reads.
- **Deviation to add.** `find-replace-unchanged-match` in `conformance/deviations.yaml`, with its test (`conformance/bulk-ops#X5`, `browser/find-replace#F13`). It is not on master.
- **Plan test → coverage.**

| Plan test | Covered by |
|---|---|
| AE47 | `browser/find-replace/ae47` (held) |
| Regex `${1}`, ignore case | `browser/find-replace/regex` (held) |
| Tags target, dedupe | `browser/find-replace/tags` (held) |
| NFC | `browser/find-replace/nfc` (held) |
| No match: {0 of N}, no step | `browser/find-replace/no-match` (held) |
| Preview invalid regex with range | Meguri `core/crates/meguri-engine/tests/find_replace.rs` (F10) |
| Card generated, never deleted | `browser/find-replace/card-generation` (held) |
| Cancel on 150k notes leaves the digest equal | `find_replace.rs` |
| `JobHandle` before work; `suspend` cancels | `find_replace.rs` |
| `selection_summary` | field names: `browser/find-replace/field-names` (held, X3); lacking counts and notetype ids: Meguri test |
| AE48 Edit Field Append | Meguri `core/crates/meguri-engine/tests/batch_edit.rs` |
| Clear on empty fields | `batch_edit.rs` |
| 150k notes ≤ 5 s, ~10 Hz progress | U242 benchmark (R1832) |
| (no plan line) | `all-fields`, `all-notes`, `refusals`, `unchanged-match` (deviation) |

### U87: card and note bulk actions

- **Spec ids.** `browser/bulk-actions#B1` to `#B18`. `conformance/bulk-ops#X2`. Also `browser/config-keys#K6.6` (remembered inputs) and `browser/columns#C12`, `#C14` (copy text).
- **Meguri docs to write.** None. `bulk-actions.md` is a spec-writer file and is on master.
- **Committed scenarios with goldens, inactive** (recorded in `25f4fa15`): `browser/bulk/{toggle-suspend, toggle-bury, flag-notes-mode, mark, set-due-notes-mode, set-due-after-op, reset-browser, grade-now, delete-notes, change-notetype-mixed}`.
- **Prepared, held.** `browser/bulk/change-deck/{basic, presets}` are untracked with goldens. They are held until the testkit parses the `change_deck` op (`conformance/bulk-ops#X2`; dsl D7.2 "typed model pending").
- **Plan test → coverage.**

| Plan test | Covered by |
|---|---|
| AE45 Toggle Suspend; current suspended → unsuspend all | `browser/bulk/toggle-suspend` |
| Flag 1 toggle; No Flag | `browser/bulk/flag-notes-mode` |
| Toggle Bury | `browser/bulk/toggle-bury` |
| Toggle Mark | `browser/bulk/mark` |
| Change Deck: filtered home, untouched target, preset → memory state | `browser/bulk/change-deck/basic`, `presets` (held) |
| Target list excludes filtered decks | Meguri `core/crates/meguri-engine/tests/bulk_actions.rs` (B7.1) |
| Set Due Date `3-7!` notes mode | `browser/bulk/set-due-notes-mode` |
| Reset restore on, counts off | `browser/bulk/reset-browser` |
| Set Due Date after an earlier op | `browser/bulk/set-due-after-op` |
| Grade Now Good | `browser/bulk/grade-now` |
| Change Note Type over two notetypes refused | `browser/bulk/change-notetype-mixed` (pending until the U35 executor) |
| Delete Notes, undo byte-identical | `browser/bulk/delete-notes` |
| `copy_text` | `bulk_actions.rs` (B14) |
| Toggle directions, current-item rules, inline new deck, `InvalidArgument`, `affected`/`considered` | `bulk_actions.rs` (B17 last paragraph) |

### U88: search-text services

- **Spec ids.** `search/join-and-replace#J1` to `#J11`. `search/editing-services#D1` to `#D7`. `conformance/search-ops#S5`, `#S6`. Also `search/canonical-form` (F2–F7) and `search/errors#E5`.
- **Meguri docs.** `specs/search/editing-services.md` (marked Meguri in the plan) is already on master. Nothing is left to write.
- **Committed scenarios with goldens, inactive.** `search/join-replace/{ae43, join, replace}` and `search/builder/{items, names}`. Only `builder/names` needs U81/U82 terms (`conformance/search-ops#S4`).
- **Prepared, held.** `conformance/scenarios/search/errors/ae42.yaml` and its golden are untracked. `editing-services#D7` names it as AE42's error-kind pin. Prepared, held until it is committed with its golden: it is held only because it is uncommitted prepared work, not for a technical reason, and could be committed inactive now. It uses only `canonical_search` and `search`, which the testkit model parses, and inactive scenarios with needs-* tags are already committed (`browser/sort/subset`, 10 of the 11 `browser/modes/*`, `search/builder/names`). Activating it waits for U82 (tags needs-deck, needs-tag) and for the R1881 check, which must run before any ported search scenario is activated (`docs/process/review-first-delivery.md`).
- **Plan test → coverage.**

| Plan test | Covered by |
|---|---|
| AE43 replace and join | `search/join-replace/ae43` |
| `a or b` joined with `c` | `search/join-replace/join` |
| Deck `a_b*` → `deck:a\_b\*`, matches only it | `search/builder/names` |
| Tag `x y` matches nothing | `#J9` row; `tests/builder.rs` (J9, N2.2) |
| AE42 Incomplete, then UnclosedQuote range | `core/crates/meguri-search/tests/edit_services.rs` (D5.2, D7); kind pinned by `search/errors/ae42` (held) |
| `spans` with UTF-16 emoji offsets | `edit_services.rs` (D3) |
| `suggest` cases (`de`, `deck:fre`, `flag:imp`, `note:"Basic" card:`) | `edit_services.rs` (D6.3, D6.4) |
| 10k tags, ≤ 50 rows, < 4 ms | micro-benchmark (D6.7) |
| AE60 text parses; canonical equals the golden | `tests/builder.rs` against the `canonical_search` golden of `stats/browse-queries/parse-all` (J11); no new scenario |
| (no plan line) | `search/builder/items`, `search/join-replace/replace`; J10 inputs, `mid:`, id lists and bury states only in `tests/builder.rs` |

## 4. Testkit and adapter work

Model: `core/crates/meguri-testkit/src/scenario/{model,prepared,intake}.rs`. Meguri executors: `src/exec/*.rs` (`register`).

| Form | Parsed by the model | Meguri executor | Needed by | Defined in |
|---|---|---|---|---|
| `search` with `order` | yes (`SearchRequest.order`) | no: `exec/search.rs` returns `unsupported_observation` "sorted search is pending" | U84 (`sort/*`), U85 (`modes/*`) | `conformance/browser-ops#B1`, `#B2` |
| `browser_rows` | yes (intake validation only) | no | U84 (`columns/*`), U85 (`rows-*`) | `conformance/browser-ops#B3`, `#B4` |
| Browser adapter routing `search` through `search_ids` and `start_search` | n/a | no | U85 | `browser/result-sets#H16` |
| `search_build`, `search_join`, `search_replace` | yes (`prepared.rs`) | no | U88 | `conformance/search-ops#S5`, `#S6` |
| `find_replace` op | **no** | no | U86 | `conformance/bulk-ops#X1` |
| `find_replace_fields` kind | **no** | no | U86 | `conformance/bulk-ops#X3` |
| `change_deck` op | **no** | no | U87 | `conformance/bulk-ops#X2` |
| `change_notetype_info` op | yes | no (U35) | U87 (`change-notetype-mixed`) | `conformance/change-notetype-ops` |
| Other ops used (`suspend`, `unsuspend`, `bury`, `forget`, `set_due`, `grade_now`, `set_flag`, `add_note_tags`, `remove_note_tags`, `delete_notes`, `edit_note`, `add_note`, `adopt`, `advance`, `undo`, `db_exec`, `set_config`, `rename_deck`, `set_current_deck`, `update_notetype`) and kinds (`card`, `revlog`, `timing`, `ids`, `undo_status`, `db_dump`, `card_excerpts`) | yes | yes | — | — |
| Setup `config`, `presets`, `decks`, `notetypes`, `notes`; top-level `clock`, `tags` | yes | yes | — | `conformance/dsl` D6 |

## 5. Open questions and decisions

- **Q1 (not blocking; resolve before coding U84 Due).** The plan says U84 reuses U82's "one shared due expression" for the Due sort and cell, "so search, sort and display can never disagree" (plan U82 Approach, U84 Dependencies). The specs give three separate rules: the cell uses the home due (`browser/columns#C7.5`), the sort key uses the stored due (`browser/sorting#S6`), and `prop:due` has its own (`search/properties` P4). Master states they differ "by design" (`browser/sorting#S6`, after the key table; `browser/columns#C7.5`, after the rules). Follow the specs. If the U84 Due work needs nothing from U82, U84's only U82 blockers are the scenario terms.
- **Q2 (not blocking).** The U84 plan test says "the first sort request on Due without an explicit direction uses descending". Master `browser/sorting#S1`: "the core never chooses a direction"; a sort request always names one. The first direction is catalog metadata (`browser/columns#C2`) used only by the K5.2 sort-by-column write. Test it as catalog and K5.2 behavior, not as a core default.
- **Q3 (not blocking).** The plan's U84 Approach keys the render LRU on mtimes. `browser/columns#C10` step 8 (decided 2026-10-27) drops entries by change sets, not by mtime keys. The spec wins.
- **Q4 (not blocking; UI strings).** Title case for the "All fields" In entry and "Restore default name" (`browser/browse-strings#W7`; `docs/process/review-log/spec-ahead-servers-open-questions-20261110.md`, "Title case, R1200 and R899"). This is an open question for Antoine. Interim: R1200's text.
- **Q5 (decided for cells).** `search/card-extractors#X3` notes an open point for `model/card-data-json` C3 (non-string `cd`). Master `browser/columns#C1` ("Card data", "Decided 2026-11-10") decides it for Browse cells: they read card data by X3, so a non-string `cd` makes the whole text read as no keys. `model/card-data-json` C3 itself is unchanged; do not reopen it in U84.
- **Q6 (not blocking).** Spec-ahead owner follow-ups: `find-replace` F7 and F9 should name U220, not U219. `browse-strings` W6 should drop "the Change Note Type mapping" and decide Create Copy's ellipsis with U183 (spec-ahead log, "Owner follow-ups").
- **Q7 (not blocking; stale status text).** `conformance/browser-ops#B6`, the `browser/result-sets` header ("not yet recorded: H16") and H16 ("their goldens are not recorded yet"), and `browser/columns#C15` still say `browser/modes/*` has no goldens. 11 goldens are on master (`25f4fa15`). `conformance/browser-ops#B6` also still says "until the testkit's scenario model reads `order` and `browser_rows`, T0 reports those scenarios as `invalid`". The model already parses both (`SearchRequest.order` and `ObservationKind::BrowserRows` in `meguri-testkit/src/scenario/model.rs`; `BrowserRows` also in `intake.rs` and `validate.rs`); see section 4. Only the "`pending` until U84's adapter lands" part is current.
- **Q8 (not blocking).** The plan's U88 says Stats must use the builder. `search/join-and-replace` intro and J11 say Stats "Show in Browser" is not a caller; U88 only checks that AE60's text parses and canonicalizes. The spec wins.
- **Q9 (open; not blocking).** Where Question/Answer rendering lives. `meguri-search` already depends on `meguri-template` (`core/crates/meguri-search/Cargo.toml` `[dependencies]`), and `meguri-search/src/browser/qa.rs` (U85a) imports `meguri_template`. `docs/plans/research/coverage/search.md` item 3 says search must not depend on it and gives the alternative: move `browser/qa.rs` into `meguri-engine`. Until the orchestrator decides, keep the existing dependency and reuse `qa.rs` where it is; do not add new `meguri-template` uses elsewhere in `meguri-search`.
- **Decided, do not reopen.** `unsortable_column` is the one refusal kind, observed as `unsortable` (S1, 2026-10-27). C13 overflow arithmetic is kept (2026-10-27). Result-sets decisions of 2026-10-27 cover revisions, `NotComplete`, resort clock, undo as a change set, superseded handles and the current-row fallback (H4–H12). Find and Replace with count 0 records no step (F9). Built-name escapes `deck:Current`, `tag:None` and refusing unrepresentable names (J3.1, J3.5). Suggestions are computed in the core (D6, 2026-10-27).

## 6. Traps

- **Goldens.** Only `tools/mg oracle record` writes them; never bless Meguri output. Mark scenarios active in `area.yaml` in the same change that lands their adapter (`conformance/AGENTS.md`).
- **Held scenarios.** Committing the held `find-replace/*` or `change-deck/*` files before the model parses their op or kind makes T0 and goldens-check fail as `invalid` (`conformance/dsl#D10`).
- **Equal-key runs.** Both sides list ties in logical-id order (`conformance/browser-ops#B2`). Meguri finds runs from its own S4/S5 keys; differing keys give differing lists.
- **Final key direction.** The final id key follows the sort direction, so an in-memory reverse equals a descending query (`browser/sorting#S8`).
- **No schema objects.** No TEMP tables or created objects: readers are `query_only` (`browser/sorting#S10`, R2016).
- **Rows sorted by note columns.** In cards mode, note-column sorts drop cards that have no note row. The handle keeps them aside and lists them again under other sorts (`browser/sorting#S9`, `browser/result-sets#H5`).
- **Aggregates.** Notes-mode aggregates range over all of a note's cards, not only the matched ones (S9). Cell arithmetic wraps (C13); sort keys do not: they are exact SQL averages and sums (S5). A note with 27 review cards at `factor` 2,500 shows Avg. Ease 7 but sorts at 2,500.
- **Cells vs keys on odd rows.** Cells read `due`, `odue`, `mod` and `ivl` with the `model/row-records` R4 tolerances (C1); sort keys read the stored values with SQLite comparison (S2). A REAL `ivl` 3.5 sorts after INTEGER 3 although both cells show 3.
- **Due.** The Due cell uses the home due; the sort key E uses the stored `due` (C7.5 vs S6). The first card is the lowest card id (C1). The notes-mode current card is the lowest `ord` (`browser/bulk-actions#B2`).
- **Scenario-writing rules** (`conformance/browser-ops#B5`, `conformance/AGENTS.md`):
  - `db_exec` changes only `cards`, `notes` and `config`.
  - Cards that `edit_note` adds need `adopt`.
  - Never observe real ids of a cloze note's cards (hash order).
  - Filtered decks are `db_exec` rows with ids below 1,000,000.
- **Set Due Date ranges.** A range draws a non-deterministic day. Compare with `db_exec` relations, never the drawn `due`/`ivl` (`conformance/AGENTS.md`, `sched-ops` J10).
- **Undo scenarios** (`undo-ops` V5):
  - Run a non-`select` `db_exec` before the first `undo` or `undo_status`.
  - After `invalid_regex` or `field_count`, Anki empties its histories and Meguri keeps them. Observe undo only when the history was already empty, otherwise add a deviation (`conformance/bulk-ops#X1`, `browser/find-replace#F9`).
- **Stamping.** Find and Replace and Edit Field write only changed notes; Anki stamps unchanged matches (deviation `find-replace-unchanged-match`, only on `unchanged-match`) (`browser/find-replace#F6`, X4).
- **Engine refusals.** `InvalidArgument` for an empty or repeated id list and an empty field name. These are Meguri-only (`browser/find-replace#F11`, `browser/bulk-actions#B7.2`).
- **Preview error range.** Changed 2026-11-21 (implementation review): there is none. The preview's error is `InvalidRegex{message}`, the message only, with no second parse (`browser/find-replace#F10`). The plan's "`re:(`" wording means a regex find text, not search syntax.
- **Change Deck.**
  - An inline new deck is part of the one "Change Deck" step.
  - Memory states are computed inside the op, not through the `memory_recompute` catalog entry.
  - The `bulk_op` write phase is not cancellable (`browser/bulk-actions#B7.2`, `engine/op-model` O9).
- **Remembered inputs.** `setDueBrowser`, `restorePositionBrowser` and `resetCountsBrowser` are written inside their op's undo step, never as a separate UserNonUndoable write (`browser/bulk-actions#B10`, `browser/config-keys#K6.6`).
- **Toggle direction.** Toggles read the current item's stored state inside their own transaction (`browser/bulk-actions#B2`).
- **Target order.** Targets are in table order (`browser/result-sets#H12`); Reset, Grade Now, Change Note Type and Delete Notes depend on it (B1).
- **Join and replace input.** J2 (U+001C–U+001F → space) applies to `join`/`replace` `existing` and field text, not to `canonical_search` (`search/join-and-replace#J2`).
- **Builder deviations.** J10 inputs are deliberate deviations. A scenario that records one needs a deviation entry; they are kept out of the oracle scenarios (J10, J11).
- **Offsets.** Character offsets inside the core, UTF-16 across the FFI; `invalid_offset` for a split surrogate (`search/editing-services#D1`, `browser/result-sets#H15`).
- **Activation gates.** `builder/names` and the needs-* `browser/` scenarios activate only after U81/U82. The R1881 similarity check must run before ported search scenarios become active (`docs/process/review-first-delivery.md`).
- **Plan paths vs the code tree.**
  - The engine has no `src/ops/` directory (U83 put `browser_config.rs` at `core/crates/meguri-engine/src/`).
  - `core/crates/meguri-search/benches/` does not exist.
  - Existing files are `meguri-search/src/browser/{mod,qa,deck_cell}.rs`, `meguri-engine/src/browse/{mod,excerpts}.rs` and `meguri-ffi/src/excerpts.rs`.
  - `meguri-search` already depends on `meguri-template` (Cargo `[dependencies]`; used by `browser/qa.rs`), although `docs/plans/research/coverage/search.md` item 3 says it must not. Whether to move `qa.rs` into `meguri-engine` is open (Q9).

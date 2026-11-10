# B2-search: unit brief (implementer view)

Base read: `cf03979b` (origin/master, = HEAD), 2026-11-10. That commit changes only browser, stats and storage specs and oracle provenance, so the search facts here are unchanged from `21fc5c1f`. Sources: implementer-view only. No `oracle/`, `refs/`, `local-data/`, `build/` similarity report or Anki source was read. "Held" means the file is in the worktree but not in `git ls-files`. "Master" means `git show HEAD:<path>`. The worktree also has uncommitted edits to `specs/search/{text-matching,names-and-ids,properties,history}.md` and `conformance/deviations.yaml`. They are prepared, not on master (§3.5).

## 1. Batch

- Units: U81 (text matching), U82 (names, ids, state, properties, history), U91 (differential corpus and grammar fuzzer), U90 (accelerators and performance budgets).
- Why grouped: all four are search core in `core/crates/meguri-search`. U82 depends on U81. U91 depends on U81 and U82. U90 depends on U91 (plan §U81, §U82, §U90, §U91).
- Order: U81 → U82 → U91 → U90 (plan dependency order). In practice, only U81 and then U82 can start now (§2).
- Milestones: U81 and U82 are M1. U90 and U91 are M2 (plan, Delivery Milestones table, M1 and M2 rows). The batch therefore crosses a milestone boundary. Gate each milestone separately (`docs/process/tiers.md` line 9).
- Cadence: U81 and U82 change only Rust internals (meguri-search, meguri-testkit). Pure Rust may cover at most 20 units per full gate. Record the batch base and each unit's starting commit (`docs/process/tiers.md` lines 7–11).
- Priority: reviews and iCloud sync first, with U113/U114 and their dependencies. Then editing, search and statistics (`docs/process/review-first-delivery.md`, "Next feature priority", 2026-10-27/29). This batch is in the "search" tier.
- Standing instruction: "Keep the 38 ported search scenarios inactive until U81/U82 and complete the required R1881 check before activation" (`docs/process/review-first-delivery.md`, "Preparation follow-up").

## 2. Dependency status

"Merged" means that a `Unit: <id>` trailer exists on origin/master (the latest commit is given).

| Unit | Depends on (plan) | Merged | Not merged (blocker) |
|---|---|---|---|
| U81 | U80, U20, U22, U29, U61, U82a | U80 `8a13d266`, U20 `1b3d2a8a`, U22 `5e24d8a0`, U29 `31edf7a7`, U61 `52c4da01`, U82a `32746932` | none. **Ready.** |
| U82 | U80, U81, U82a, U23, U28, U29, U30, U40 | U80, U82a, U23 `bb546fc7`, U28 `70669146`, U29, U30 `67bf0166`, U40 `2448e115` | **U81** (in this batch) |
| U91 | U80, U81, U82, U84, U3, U4, U11 | U80, U3 `62c89b98`, U4 `3f3bab98`, U11 `f0607339` | **U81, U82** (in batch); **U84** (not in batch). U84 depends on U82, U83, U85a and others (plan Unit Index). |
| U90 | U85, U86, U87, U91, U26, U11, U12 | U26 `fb85a2c8`, U11, U12 `6a0ddfd8` | **U85, U86, U87** (not in batch), **U91**. U85 depends on U81, U82, U83, U84 (plan Unit Index). The plan's U90 execution note says to land U85's baseline benchmarks first. |

- U82a's four functions are on master: `meguri_card_f32`, `meguri_retrievability`, `meguri_relative_retrievability` and `meguri_original_position` (`core/crates/meguri-search/src/sqlfns.rs`). U81 and U82 add their functions to this module.
- `is:due` and `is:buried` already compile (`core/crates/meguri-search/src/plan/sql.rs`, from `152f1da4`, Spec `search/state-and-flags`). Every other term (`Term` in `src/ast.rs`) still returns `TermOutcome::NotYetSupported`.

## 3. Per unit

### 3.1 U81: text matching

- Spec trailers: `search/text-matching#T1`–`#T17` (T3.3, T7.2, T7.3, T9.4, T12.1–T12.3, T14, T15 and T16 are the ones most often missed), `search/fields#F1`–`#F10`, `search/case-folding#C1`–`#C7`. Also cited: `search/query-model#Q8`, `#Q9`, `search/errors#E3`, `#E4`, `#E5`, `#E7`, `text/strip-html#H1`, `text/unicode-primitives#U2`, `#U3`, `storage/open-and-connections#C5` (registration) and `search/card-extractors#X2.1` (shared registration).
- Meguri design docs to write: none. U81's three specs exist. Spec T14 (SQL functions) and T15 (the plan) already hold the Meguri design.
- SQL functions to add (T14): `meguri_like(text, pattern, mode)`, `meguri_transform(text, flags)`, `meguri_regex(pattern, text)` and `meguri_regex_fields(pattern, flds, ord, ...)`. N7 adds `meguri_regex_tags` (for U82). None of these may appear in a schema object (R2016).
- Committed scenarios (all inactive; `conformance/scenarios/search/area.yaml` does not list them; goldens are tracked):
  - `search/text/`: `plain-substring`, `html-and-sort-field`, `field-separator`, `normalization`, `ignore-accents`, `excluded-fields`, `excluded-all`, `no-searchable-field`, `nc`, `sc`, `sc-no-cloze-type`, `word`, `pattern-limit`.
  - `search/fields/`: `exact`, `names`, `glob-runs`, `modes`.
  - `search/folding/`: `umlaut`, `differential`, `ignore-accents`. Deviations `search-unicode-umlaut`, `search-unicode-differential` and `search-unicode-ignore-accents` apply.
  - `search/regex/`: `basics`, `invalid`, `invalid-unreached` (deviation `search-invalid-regex-unreached`).
  - Also gated on U81 (tag `needs-text`): `search/structure/precedence-results`. `card-note-modes` also needs state and template terms. `damaged-rows` also needs deck and state terms. Activate those two after U82.
- Prepared and held:
  - `search/text/nc-escapes` and `search/text/pattern-limit-unreached`: held until their goldens and the spec edits that drop "(no golden yet)" land on master. `pattern-limit-unreached` also waits for its deviation entry. Master T12.3 names one entry, `search-pattern-limit-unreached`, for both `/steps/0/search/0` and `/steps/3/search/0`, and says it is "still to be added before the scenario becomes active". The split into `search-pattern-limit-unreached` (`/steps/0/search/0`) and `search-pattern-limit-unreached-runs` (`/steps/3/search/0`) exists only in the uncommitted T12.3 rewrite and the uncommitted `deviations.yaml` (§3.5).
  - `search/regex/glob-size`: held for the same reason. It also needs U82's tag, deck, note type and template terms (tags `needs-deck`, `needs-tag`, `needs-notetype`, `needs-template`).
- Plan test scenarios mapped to scenarios or tests:

| Plan test scenario | Where |
|---|---|
| AE49 `über`: Unicode match | `search/folding/umlaut` (deviation) |
| AE49 `über`: AsciiCompat no-match | Meguri-only: `tests/matchers.rs` (AsciiCompat needs `test-hooks`) |
| `dog`, `do*g`, `d_g`, `\*` | `search/text/plain-substring` |
| `hotdog` through the sort field | `search/text/html-and-sort-field` |
| `&amp;` and `&` | `search/text/html-and-sort-field` |
| NFC `é` | `search/text/normalization` |
| `ignoreAccentsInSearch` `uber`/`über` | `search/text/ignore-accents` |
| excluded "Notes" (also as the sort field) | `search/text/excluded-fields` |
| `привет`/`ÜBER` with ignore accents | `search/folding/ignore-accents` |
| `w:dog`, `x<b>dog`, `w:dog*` | `search/text/word` |
| `(?-i)`, `re:a\"b` | `search/regex/basics` |
| `nc:は`, `nc:strasse` | `search/text/nc` |
| `sc:capital` | `search/text/sc` |
| `front:dog`, `front:*dog*`, `front:`, `front:_*`, `front:*` | `search/fields/exact` |
| `"my field:x"`, `my\ field:x`, `*:x` | `search/fields/names`, `search/fields/modes` |
| `fr*:x` | `search/fields/names` and `glob-runs` (glob names; the exact text `fr*:` is not in any scenario) |
| `front:re:^a`, `front:nc:uber`, `nosuchfield:x` | `search/fields/modes`, `search/fields/names` |
| property: Unicode result ⊇ AsciiCompat result | Meguri-only: `tests/fold_props.rs` (C7 property) |
| invalid `re:(`: error with a range, before `FirstPage` | `search/regex/invalid` (quoted `"re:("`). An unquoted `re:(` is the grammar error `unclosed_group` (T12.1). The range and the "before first page" order are Meguri-only plan tests (ranges are not observed, `conformance/search-ops#S3`). |

### 3.2 U82: names, ids, state, properties, history

- Spec trailers: `search/names-and-ids#N1`–`#N8`, `search/state-and-flags#S1`–`#S7` (S1 and S2 are already implemented), `search/properties#P1`–`#P17`, `search/history#H1`–`#H11`. Also cited: `search/card-extractors#X1`, `#X2`, `search/query-model#Q8`, `#Q9`, `search/grammar#G8`, `#G8.1`, `text/checksum-and-sort-field#C4`, `text/strip-html#H5` (for `dupe:`), `storage/ids-usn-mtime#I2`, and `sched/day-clock#D5`, `#D7`, `#D8` (for S7).
- Meguri design docs to write: none. The four specs exist. P15 (`meguri_custom_data`) and N7 (`meguri_regex_tags`) hold the Meguri design.
- Shared due expression: P4 and P4.1 ("The due-day rule outside search") define it. U84 reuses it for the Due sort and cell.
- Committed scenarios (all inactive): `search/tags/{hierarchy,regex-and-nc}`, `search/decks/{names,current,presets}`, `search/notetypes/names`, `search/ids/lists`, `search/state/{is,due,flags}`, `search/props/{scheduling,memory,custom-data}`, `search/history/{added-edited,answers,dst-paris,dst-new-york}`, `search/dupe/first-field`. After U82, also `search/structure/{card-note-modes,damaged-rows}` and the held `search/regex/glob-size`.
- Prepared and held:
  - `search/decks/preset-missing`, `search/props/due-extremes`, `search/history/odd-ease` and `search/dupe/entities`: held until their goldens and the master spec edits that drop "(no golden yet)" land (master: N8, P17, H11).
- Committed but not in this batch: `search/builder/names` (scenario and golden are in `git ls-files`; inactive; tags `needs-deck`, `needs-tag`, `needs-notetype`, `needs-text`). Besides U81/U82 terms it needs U88's adapter (`conformance/search-ops#S4`), so it stays inactive after this batch.
- Plan test scenarios mapped to scenarios:

| Plan test scenario | Where |
|---|---|
| AE49 tag part; `tag:none`, `tag:*`, `tag:_*`, `tag:parent::*`, `tag:a b`; the `tag:ani*` boundary | `search/tags/hierarchy` |
| `tag:re:^ani`, `tag:nc:ubung` | `search/tags/regex-and-nc` |
| `deck:french…`; `deck:x -deck:x::*` (the scenario uses `french`); `deck:filtered`; filtered card with home deck H | `search/decks/names` (N3.7) |
| `did:<H>` excludes children; a filtered card by its home deck | `search/ids/lists` (N3.5) |
| `deck:current` with a deleted deck | `search/decks/current` |
| `preset:"Japanese \"Core\""` | `search/decks/presets`; held `preset-missing` |
| `note:` glob and unicase, `card:2` cloze, `card:0` | `search/notetypes/names` (names are `Vocab`, not `basic`) |
| `nid:`, `cid:`, single-id `mid:` | `search/ids/lists` |
| `nid:1, 2` invalid id list | `search/errors/arguments` (U80, already active), with equivalent malformed lists: `nid:4,5,`, `nid:1,,2`, `nid:,5,6`, `nid:7,8,z`, `nid:4_5,6` (the exact text `nid:1, 2` is not in any scenario) |
| `is:` over every queue/type pair | `search/state/is` |
| `is:due` at next-day-at − 1 s with learn-ahead | `search/state/due` |
| `flag:0`…`flag:7`, `flag:8` | `search/state/flags` (also `errors/arguments`) |
| `prop:ivl`, `ease`, `due=-1`, `pos` | `search/props/scheduling`; held `due-extremes` |
| `prop:rated=1` error | `search/errors/{arguments,kinds}` (U80) |
| `prop:rated=-7:1`, `prop:resched=0` | `search/history/answers` |
| `prop:s`, `prop:d`, `prop:r` at a frozen now | `search/props/memory` |
| `prop:cdn`, `prop:cds`, `has-cd`, a key with `-` | `search/props/custom-data` (also `errors/arguments`) |
| `added:0`, `rated:2:3`, `resched:1`, `rated:1` excludes manual entries, `introduced:1`/`-introduced:1` around rollover | `search/history/{added-edited,answers}`; held `odd-ease` |
| DST in Europe/Paris and America/New_York | `search/history/dst-paris`, `dst-new-york` |
| `dupe:` with stripped first field and `[sound:]` kept | `search/dupe/first-field`; held `entities` |

### 3.3 U91: differential corpus and grammar fuzzer

- Spec trailers: no spec is written for U91. Cite `search/case-folding#C1`, `#C7`, `conformance/search-ops#S1`–`#S3`, `search/query-model#Q8`, `search/canonical-form` (canonical text), and R1085 and R1918 for Meguri-only tooling.
- Meguri design docs to write: none named.
- Scenarios: none exist yet. The paths `conformance/corpora/search/`, `conformance/scenarios/search/fixture/` and `conformance/scenarios/regressions/` do not exist.
- Plan test scenarios: all are Meguri-only tests or T2 runs, with no fixed scenario ids:
  - 10k queries per nightly run: T2 job (U247).
  - Shrinker on an injected `_`-as-`*` bug: Meguri-only test.
  - Harvested history corpus: see §5, item 6.
  - Invalid-query sample: differential run.
  - T0 replay: committed goldens.
- What exists: `FoldMode::AsciiCompat` behind `test-hooks` (`core/crates/meguri-search/src/context.rs`). Oracle worker protocol `step_request` "with step, run that ad-hoc step" (`conformance/schema/protocol.json`). The protocol has no `find_cards` request; differential queries go through `begin` and ad-hoc `observe` steps. Live chain and pool code: `core/crates/meguri-testkit/src/exec/{live,oracle}.rs`, `tests/oracle_pool.rs`.

### 3.4 U90: accelerators

- Spec trailers: `search/accelerators` (to write), `search/query-model#Q8`, `#Q9`, `#Q10` (the relaxed query is the cache key), `search/text-matching#T6`, `#T8` (paths that U90 must not pre-filter), `storage/derived-store` (the U26 lifecycle).
- Meguri design doc to write: `specs/search/accelerators.md` (plan Files: "(Meguri)"). Header `Anki: none` (`specs/AGENTS.md`). It must set the fallback bound for changed notes (plan U90 Approach, "Freshness").
- File paths in the plan that do not exist yet: `core/crates/meguri-search/benches/` and U85's `search.rs`/`browse.rs` benches. `core/crates/meguri-storage/src/derived/` exists (U26).
- Scenarios: none. All tests are Meguri-only: the equivalence property suite, benchmarks and budgets in `tools/perf/budgets.yaml` (group `browser-search`).

### 3.5 Prepared work in the worktree (not master)

- Eight held scenario/golden pairs under `conformance/{scenarios,goldens}/search/`: `decks/preset-missing`, `dupe/entities`, `errors/ae42` (not in this batch), `history/odd-ease`, `props/due-extremes`, `regex/glob-size`, `text/nc-escapes`, `text/pattern-limit-unreached`.
- Uncommitted spec edits:
  - drop "(no golden yet)" from text-matching T17, names-and-ids N8, properties P17 and history H11;
  - remove the intro clause "except those of `search/text/nc-escapes`, `search/text/pattern-limit-unreached` and `search/regex/glob-size`, which await their first recording" from text-matching, and the equivalent clause (`preset-missing`, `dupe/entities`, `glob-size`) from names-and-ids;
  - rewrite T12.3's second deviation into two entries (§3.1).
- Uncommitted `conformance/deviations.yaml` adds `search-pattern-limit-unreached` and `search-pattern-limit-unreached-runs`, and also an unrelated entry, `check-db-default-deck-filtered` (not this batch).
- Also untracked, not in this batch: the draft spec `specs/search/syntax-reference.md` and its draft scenario `conformance/scenarios/search/reference/examples.yaml` (no golden, not in `area.yaml`; R1093).
- Treat all of these as spec-side work that is held. Implement against master.

## 4. Testkit and adapter work

The scenario model already parses every form that the batch scenarios use. These are:

- Setup sections: `config`, `presets`, `decks` (name or `{name, preset}`), `notetypes` (`kind: cloze`), `notes` (with `deck` and `tags`).
- Steps: `add_note`, `db_exec`, `edit_note`, `legacy_update`, `set_clock`, `set_config`.
- Observations: `search`, `ids`, `timing`, and `canonical_search` (in `search/structure/precedence-results`, gated on U81).

`core_registry()` registers each of them (`core/crates/meguri-testkit/src/scenario/model.rs` `Setup`, `Step`; `src/exec/{decks,notes,notetypes,model,storage,clock,cards,search,sched}.rs`; `canonical_search` is in `exec/search.rs` from U80, `timing` in `exec/sched.rs`). These scenarios were not run for this brief. No batch scenario uses `order`.

What remains is in `core/crates/meguri-testkit/src/exec/search.rs`:

1. **Plan inputs.** `Plan::build(&query, mode, &context)` takes no collection. U81 and U82 must resolve names when the plan is built: field names (fields F2), exclusions (text-matching T7), cloze note types (T9.1), and deck, note type, template, preset and `dupe:` candidates (names-and-ids N1, N3, N4, N6, N7). This must happen "on one read transaction" with the statement, or the plan must be rebuilt (text-matching T15, names-and-ids N1, query-model Q10.5). Today the adapter opens `collection.reader()` separately for config reads and for `run()`.
2. **`curDeck` missing or non-integer.** The adapter marks `CurrentDeck` unknown and returns `unsupported_observation`. The testkit AGENTS.md says the same (`core/crates/meguri-testkit/AGENTS.md` line 5). Names-and-ids N3.4 says the id is then 1, and `"301"` (a string) also gives 1. `ConfigValue::as_i64` returns None for a string (`meguri-storage/src/rows/config.rs` line 254). Change the adapter and that AGENTS.md line together.
3. **`BuildError::NotYetSupported`.** Remove this branch with the last stub (`core/crates/meguri-search/AGENTS.md`).
4. **Fold mode.** Fold mode is fixed to `FoldMode::Unicode`. This is correct for T0 and T1 (case-folding C1). U91 needs an AsciiCompat path under `test-hooks`.
5. **Sorted search.** A request with `order` returns `unsupported_observation` ("sorted search is pending", U84). U91's "ordered ids … under a fixed sort" needs that ordering (`conformance/search-ops#S1`; `browser-ops` B1, B2).
6. **Day values.** Today and next-day-at are filled only when `plan.reads()` contains them. U82 terms must declare their reads exactly: `prop:due` reads Today and NextDayAt; `prop:r` reads Today and Now; history terms read NextDayAt only (properties P16, history H10, state-and-flags S3).
7. **U91 tooling.** No `search_gen.rs` exists. `core/tools/meguri-cli/src/` has no `cmd/` folder: its modules are top-level files (`conformance.rs`, `generate.rs`, …) plus one subfolder, `private_suite/` (`mod.rs`, `io_roundtrip.rs`). The plan's `src/cmd/search_fuzz.rs` path does not match this layout.

## 5. Open questions and decisions

1. **`nc:` letter case in Unicode mode.** The plan's Open Questions default is "simple folding … recorded with R1036's entry in deviations.yaml (U81)". Case-folding C2 adopts it. The `nc:` rows sit inside `search-unicode-differential`. **Not blocking.**
2. **R1881 provenance of the ported search scenarios.** Scan `similarity/20261028T013910Z-72520` flagged SQL regions in `history/answers.yaml`, `history/dst-new-york.yaml` and `history/dst-paris.yaml`. Scan `20261028T062248Z-42310` also flagged `goldens/search/builder/items.json` line 251. No disposition is approved (`docs/process/review-log/search-data-similarity-2026-10-28.md`). `docs/process/review-first-delivery.md` requires the R1881 check before activation. **Blocks marking those scenarios active, not implementation.** This is spec-side or infra work, not the implementer's.
3. **Held goldens and spec edits (§3.5).** **Blocks activation of the eight held scenarios only.**
4. **Deviation entry for `pattern-limit-unreached`.** Master T12.3 names one entry, `search-pattern-limit-unreached`, "still to be added before the scenario becomes active"; the held rewrite splits it in two (§3.1, §3.5). **Blocks that scenario only.**
5. **Where U91 saves regression scenarios.** The plan names `conformance/scenarios/regressions/search/`. No such area exists, and `specs/conformance/dsl.md` has no regression convention. **Not blocking until the first shrunk failure is saved.**
6. **U91's harvested query-history corpus.** It is anonymized from the user's history (R1915). Implementers never read `local-data/`, so another role must produce it, through tools such as `meguri-testkit/src/anonymize`. **Blocks that one U91 test.**
7. **Accelerator fallback bound.** U90's own spec must set it. **Blocks U90 (already blocked by §2).**
8. Settled, recorded for reference:
   - Fold modes: no golden comparison runs in AsciiCompat mode (case-folding C1, decided 2026-10-27). This overrides the plan's U81 Verification line, which says the `search/{text,fields,folding,regex}/` scenarios "match the goldens in AsciiCompat mode". Under C1, T0/T1 golden comparisons run in Unicode mode with the `search-unicode-*` deviations; AsciiCompat is only for U91's live differential runs and U81's matcher tests (`fold_props`). Follow the spec.
   - Build-time pattern-size check (T12.3, 2026-10-27).
   - `{notetype}` wording for "all fields excluded" (T16, 2026-10-27).
   - `meguri_custom_data` never raises an error (P15, 2026-10-27).
   - `rid:` is not supported; `rid:1:2` is a field search (plan "Deferred to Planning" list; grammar G6).

## 6. Known traps

- **Conformance mode.** Conformance runs always use Unicode mode, and only the three `search-unicode-*` deviations cover Unicode extras. Any other Unicode-vs-golden difference is a spec finding to escalate, never something to work around in the adapter (case-folding C1, C7; `conformance/AGENTS.md` deviations line).
- **Two folding rules.**
  - Plain, field and `sc:` text use simple case folding (C3).
  - Field names without wildcards use unicase full folding: `straße` = `STRASSE`. With wildcards they use simple folding: `straß*` ≠ `STRASSE` (fields F2; names-and-ids N1).
  - U+212A and U+017F fold to ASCII, so even an ASCII term needs folding on the stored side (C4).
- **Regex construction.** Use `regex` 1.11.2, `Regex::new` on `&str`, default options, size limit 10 × 2^20, nesting limit 250. With the `(?i)` prefix, 249 nested groups pass and 250 fail (T11.1, T12.1). Ignore the `regex::bytes` advice in `docs/plans/research/rust-stack.md`; the spec governs. Versions come from `core/pins.toml`.
- **Compile at plan time.** Compile every expression when the plan is built, glob and escaped ones included. Over the size limit is `search_failed (expression too large)`, never a panic (T12.1, T12.2, fields F2). Report plan errors in the order of errors E4 step 3: two passes over the terms, each in reading order, negated terms included. Pass 1 runs the statement checks of every term (negated operator word, non-ASCII id digit, `prop:` operand range, `sc:` with no cloze note type, no searchable field, all fields excluded, in that order within a term). Pass 2 runs the pattern checks (T3.3 size, then regex compilation). So an earlier term's pattern error loses to a later term's statement error: `re:[ sc:x` with no cloze note type is `search_failed (no cloze note type)` (E4, T12.2). The "one pass" in `meguri-search/AGENTS.md` means `build_with` raises these errors before any stub, not one interleaved pass.
- **Pattern limit.** It is 50,000 bytes and counts the added `*`, separators and field-position wildcards (T3.3; `search/text/pattern-limit`).
- **Field runs.** A field-name glob over neighbouring fields tests one span of the field string (fields F4.2; `search/fields/glob-runs`).
- **`-sc:x` versus `-(sc:x)`.** `-sc:x` negates only the cloze condition; `-(sc:x)` is ordinary negation. The tree keeps them apart (T9.4).
- **`sc:` cloze stripping.** No cloze-markup removal helper exists in `meguri-text` (grep at base), although the plan says U61 provides it. T9.2 defines the rule. Removal runs on the field string and the sort text separately (T9.3).
- **Sort-field affinity.** The sort field has integer affinity (`1.50` is stored as 1.5). INTEGER and REAL arguments read as SQLite text (T2, T14; `search/text/html-and-sort-field`).
- **`re:` and `w:` with excluded fields.** Once any note type excludes a field, `re:` and `w:` test that type's fields one by one. A type with all fields excluded makes every `re:` and `w:` search fail (T7.3).
- **Ignore accents.**
  - Ignore accents routes plain, `w:` and `sc:` through the `nc:` transform. It never applies to field searches (fields F9), and U90 must not pre-filter those paths (plan U90 Approach).
  - NFKD can turn characters into pattern escapes and wildcards (U+FF3C, U+FE68, U+FE33, U+FE4D, U+FE6A) (`search/text/nc-escapes`, held).
- **Tags.** Tag wildcards are `\S*` and `\S` and never cross a space. `tag:re:` tests each space-split piece, empty pieces included (names-and-ids N1, N2.3).
- **`deck:current`.** It compares names, not deck id 1: a deleted current deck searches the name `Default` (N3.4).
- **`is:` words.** `is:` words are exact and case-sensitive; the key is not (S5). `flag:` masks with `& 7` (S6).
- **Buried cards across a day change.** Search never unburies. Scenarios list `timing` before `search` after a day change (S7; `conformance/AGENTS.md` day-clock line).
- **`prop:r`.** It is false for type 0 cards (P15).
- **Non-finite operands.** `prop:` operands that are infinite or NaN (`prop:d>1e38` after mapping) are `search_failed` at plan time (P14).
- **History windows.** `rated:` and `resched:` include an entry exactly at the window start; `introduced:` and `added:` do not (H3–H7; `search/history/answers`).
- **Product term compilers.** They cannot run on `tests/canonical_roundtrip.rs`'s id-only tables; use a schema-18 collection (`meguri-search/AGENTS.md`; U80 review log).
- **Fixed-id rows.** Rows inserted with `db_exec` take fixed ids below 1,000,000. Clock-derived ids are pinned by `ids` observations (`conformance/AGENTS.md`; names-and-ids N8).
- **Never edit goldens by hand.** Only `tools/mg oracle record` writes them. Mark a scenario active in the change that lands its adapter (`conformance/AGENTS.md`).

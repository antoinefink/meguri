# Test-data similarity flags: classification and dispositions, 2026-11-21

Status: classified. The 4 findings of class (c) are rewritten: 3 in branch
`unit/similarity-flag-dispositions`, the studied-today one in
`unit/similarity-studied-today-values` (see "Hosted check" below). The 179
other flags are oracle output or generic overlap; their disposition waits for
Antoine (escalation `E-20261121T155116Z-246e`). A whole-tree scan stays red
until he decides.

- Scan: `tools/mg similarity --test-data -- conformance/scenarios conformance/goldens`
  on `20e6fc4a4` (master after PR #64): run `similarity/20261121T152759Z-7385`,
  2,508 files, 182 flagged (`lex`, `com`, `text`), 0 allowlisted, 0 errors.
- With all four rewrites: run `similarity/20261121T154232Z-66467`:
  2,508 files, 179 flagged. The 3 rewritten files that flagged
  are clear; `stats/studied-today/format-table.json` still flags on the
  zero line only, which is Anki output (class a).
- Author: spec-writer session `s-20261121T152737Z-af3fb04f`. It read Anki only
  at the pinned tag (`refs/anki-26.09.3`) and the reviewer view of the scan.
  This log names no Anki file and quotes no Anki text; the Anki side of each
  group is in `oracle/provenance/similarity-test-data-20261121.md`.

## Classes

- (a) oracle output: a golden whose matching text is what Anki emits (HTML,
  labels), or a scenario holding an Anki-emitted interface constant that a
  spec lists (the search texts of `specs/stats/browse-queries.md`).
- (b) our inputs that share a generic run with Anki code or tests: the
  schema-18 column list, the Image Occlusion field syntax with small
  values, counting lists such as `#c1, #c2`, common SQL subqueries and one
  character written many times over.
- (c) text taken from an Anki test or source into a scenario.

For (b) against (c) the check was: what the run is, whether it sits in an
Anki test or product code, whether the scenario's other inputs come from the
same Anki test, and whether a distinctive input (a number with many digits, a
word sequence) occurs verbatim in the matched Anki file or in any Anki test
file at the tag. Every flagged scenario's string and number inputs were
searched that way; the only verbatim hits beyond interop vocabulary (stock
note-type names, template syntax, config keys) are the (c) findings below.

| Area | (a) | (b) | (c) | Files |
|---|---|---|---|---|
| cardgen | 0 | 4 | 0 | 4 |
| editor | 1 | 3 | 0 | 4 |
| maintenance | 0 | 3 | 1 | 4 |
| media | 0 | 1 | 0 | 1 |
| model | 0 | 0 | 1 | 1 |
| notetypes | 0 | 1 | 1 | 2 |
| package | 0 | 2 | 0 | 2 |
| preservation | 1 | 1 | 0 | 2 |
| render | 51 | 9 | 0 | 60 |
| sched | 0 | 20 | 0 | 20 |
| stats | 12 | 63 | 1 | 76 |
| storage | 1 | 2 | 0 | 3 |
| sync | 0 | 2 | 0 | 2 |
| undo | 0 | 1 | 0 | 1 |
| **Total** | **66** | **112** | **4** | **182** |

search and the other areas have no flag.

## (c) findings, rewritten

All four are small, incidental reuse of Anki unit-test values or a test query,
not copied test code. Each scenario still tests the same behaviour.

1. `model/card-data/rewrite`: the first card's input was the four values of an
   Anki unit test of card-data rounding, and `specs/model/card-data-json.md`
   C5 listed that row. Now `{"s":87.65438,"d":4.567891,"dr":0.912345,"decay":0.345678}`,
   which the oracle writes back as `{"s":87.6544,"d":4.568,"dr":0.91,"decay":0.346}`
   (rounding up and down, as before). C5's row is the new pair.
   Open: `core/crates/meguri-model/tests/card_data_json.rs` (`spec_examples`)
   still has the old pair, copied from C5. A fixer must replace it with C5's
   new row; a spec writer cannot edit `core/`.
2. `stats/studied-today/format-table`: two of its 36 inputs, `[3, 13000]` and
   `[300, 5400000]`, were the two cases of an Anki unit test. Now
   `[3, 7000]` (2.33 s per card: rounding down to two decimals) and
   `[240, 5040000]` (84 minutes: the minutes form). They are not rows of
   `specs/stats/studied-today.md` T2.4, whose count (34 rows plus 2) holds.
3. `maintenance/check-db/all-records` and
4. `notetypes/structure/template-swap-replace`: the orphan-card count used
   an Anki test's query, and in the second scenario for the same purpose.
   Both now use a `LEFT JOIN` with the same result.

Goldens re-recorded with `oracle/20261121T153646Z-41957` (all four). Observations
unchanged except the two new rows above; `goldens-check/20261121T153851Z-50525`
passes (2 goldens changed). T0 `conformance/20261121T153848Z-49763`: 5 pass,
2 pending (inactive, their ops are not implemented yet), 0 fail.
`core-test/20261121T153920Z-57228` (card_data_json and sched_ops_conformance):
13 passed.

Seen and not flagged: `render/cloze/hints.yaml` uses the cloze word `shared`
for a two-number cloze, as an Anki cloze test does. One natural word; left
as is.

## (a) and (b): proposed dispositions

`provenance.md` allows a similarity exception only as an allowlist entry
written from Antoine's chat approval and recorded as an escalation. None is
added here. Escalation `E-20261121T155116Z-246e` (kind `approval`, unit
`similarity-test-data-allowlist`) asks Antoine to decide both:

1. (b), 112 files. Recommended: rewrite, no exception. 91 scenarios match only
   because their review-log insert names all nine columns in schema order;
   dropping that list (a positional insert in the same order) changes no
   observation, only `scenario_sha256`, as the search area did on
   2026-11-20. The other 21 (Image Occlusion coordinates, numbered `#c1`
   lists, three generic subqueries, a repeated letter) can take other values
   the same way, with their goldens re-recorded. Exceptions: `preservation/recipe/kitchen-sink`
   (a rewrite remakes the preservation fixture and every preservation golden)
   and the actor scenarios under `sync/` (recordings) would cost more than an
   entry; those 3 could take digest entries (below).
2. (a), 67 files (66 plus the residual zero line). These cannot be
   rewritten: Meguri must emit the same bytes. Line-range entries would need
   498 entries, and every re-record (each pin bump re-records every golden)
   moves the lines and fails the check again. Whole-file path entries would
   hide later findings. Proposed mechanism, not built: an allowlist entry
   that holds the reviewed file's whole SHA-256 and applies only while the
   bytes are exactly those (as the public-artifact map does for privacy). A
   re-record or edit voids it and the file is judged again. A blanket rule
   for goldens is not proposed: scanning goldens caught both value lifts
   above (the scenario inputs did not flag; Anki's asserted outputs did).

## Hosted check

Linux · similarity fails only on flags in files a change adds or modifies,
using the base's allowlist, so no allowlist entry has to land first. The
three rewrites modify 6 data files and none of them flags; their branch
passes the check without any exception. The studied-today golden still flags
after its rewrite, on the zero line (Anki's own output, class a, the same as
nine unmodified stats goldens). Any re-record of that golden fails the check
until the class (a) decision lands, so that rewrite is a separate branch and
pull request, which waits for that decision or for the coordinator to accept
the one red flag.

## Per-file classification

| File | Class | Channels | Regions | What matches | State |
|---|---|---|---|---|---|
| `scenarios/cardgen/empty/keep-notes.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/cardgen/empty/occlusion.yaml` | b | text | 2 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/cardgen/empty/recheck.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/cardgen/empty/undo.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `goldens/editor/cloze/output/read-back.json` | a | text | 1 | oracle output: cloze span HTML | open |
| `scenarios/editor/cloze/output/read-back.yaml` | b | text | 1 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/editor/image-occlusion/add-check.yaml` | b | text | 6 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/editor/image-occlusion/audio-fields.yaml` | b | text | 2 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `goldens/maintenance/check-db-notes/all-notes.json` | b | text | 1 | echo of the scenario's own Image Occlusion field (format keywords and coordinates .1/.1/.2) | open |
| `scenarios/maintenance/check-db-notes/all-notes.yaml` | b | text | 2 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/maintenance/check-db-notes/io-field-tags.yaml` | b | text | 1 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/maintenance/check-db/all-records.yaml` | c | text | 1 | orphan-card count query was an Anki test query (count() spelled count(*)) | rewritten; clear |
| `goldens/media/add/lowercase.json` | b | lex | 5 | lex: long runs of a repeated letter (case-folding names) against one repetitive Anki literal | open |
| `goldens/model/card-data/rewrite.json` | c | text | 1 | input row was the four values of an Anki card-data unit test; the golden shows Anki's asserted output | rewritten; clear |
| `scenarios/notetypes/change/template-map.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/notetypes/structure/template-swap-replace.yaml` | c | text | 1 | same orphan-card query as an Anki test with the same purpose (no card left without its note after removing templates) | rewritten; clear |
| `scenarios/package/apkg-export/selection/invalid-ids.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/package/apkg-import/guid-other-id.yaml` | b | text | 1 | generic orphan-card subquery (nid not in (select id from notes)) in a different statement | open |
| `goldens/preservation/render/every-card.json` | a | text | 6 | oracle output: hint-filter HTML | open |
| `scenarios/preservation/recipe/kitchen-sink.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `goldens/render/cloze/blank-check.json` | a | text | 1 | oracle output: cloze span HTML | open |
| `goldens/render/cloze/encoding.json` | a | text | 1 | oracle output: cloze span HTML | open |
| `goldens/render/cloze/markers.json` | a | text | 1 | oracle output: cloze span HTML | open |
| `goldens/render/cloze/stored-cards.json` | a | text | 4 | oracle output: cloze span HTML | open |
| `goldens/render/cloze/stored-text-chain.json` | a | text | 2 | oracle output: cloze span HTML | open |
| `goldens/render/corpus/templates-01.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-02.json` | a | text | 18 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-03.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-04.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-05.json` | a | text | 18 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-06.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-07.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-08.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-09.json` | a | text | 17 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-10.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-11.json` | a | text | 17 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-12.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-13.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-14.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-15.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-16.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-17.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-18.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-19.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/corpus/templates-20.json` | a | text | 20 | oracle output: hint-filter HTML | open |
| `goldens/render/filters/chains.json` | a | text | 1 | oracle output: hint-filter HTML | open |
| `goldens/render/filters/hint.json` | a | text | 12 | oracle output: hint-filter HTML | open |
| `goldens/render/image-occlusion/render/audio-tags.json` | a | text | 2 | oracle output: Image Occlusion div HTML | open |
| `goldens/render/image-occlusion/render/audio.json` | a | text | 1 | oracle output: Image Occlusion div HTML | open |
| `goldens/render/image-occlusion/render/cards.json` | a | text | 1 | oracle output: Image Occlusion div HTML | open |
| `goldens/render/image-occlusion/render/divs.json` | a | text | 1 | oracle output: Image Occlusion div HTML | open |
| `goldens/render/image-occlusion/review/fixtures.json` | a | text | 5 | oracle output: Image Occlusion div HTML | open |
| `goldens/render/pipeline/read-only.json` | a | text | 3 | oracle output: hint-filter HTML | open |
| `goldens/render/preview/type-in.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/ae17.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/answer.json` | a | text | 6 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/cloze.json` | a | text | 6 | oracle output: cloze span HTML | open |
| `goldens/render/type-answer/compare.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/corpus/compare-01.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/corpus/compare-02.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/corpus/compare-03.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/corpus/compare-04.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/corpus/compare-05.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/corpus/compare-06.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/corpus/compare-07.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/corpus/compare-08.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/corpus/compare-09.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/corpus/compare-10.json` | a | text | 1 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/meguri-modifiers.json` | a | text | 6 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/modifiers.json` | a | text | 6 | oracle output: type-answer comparison HTML | open |
| `goldens/render/type-answer/question.json` | a | text | 2 | oracle output: type-answer comparison HTML | open |
| `scenarios/render/cloze/nesting.yaml` | b | text | 1 | numbered sequence (c1, c2, ... or #c1, #c2, ...) against a numbered table literal | open |
| `scenarios/render/cloze/numbers.yaml` | b | text | 1 | numbered sequence (c1, c2, ... or #c1, #c2, ...) against a numbered table literal | open |
| `scenarios/render/image-occlusion/format/cards.yaml` | b | text | 2 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/render/image-occlusion/format/parse-escapes.yaml` | b | text | 1 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/render/image-occlusion/format/parse-numbers.yaml` | b | text | 1 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/render/image-occlusion/render/audio-tags.yaml` | b | text | 1 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/render/image-occlusion/render/audio.yaml` | b | text | 1 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/render/image-occlusion/render/divs.yaml` | b | text | 2 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/render/image-occlusion/review/fixtures.yaml` | b | text | 3 | Image Occlusion field syntax with coordinates .1/.1/.2 (format keywords plus three digits) | open |
| `scenarios/sched/answer/state-kinds.yaml` | b | lex,text | 4 | numbered sequence (c1, c2, ... or #c1, #c2, ...) against a numbered table literal | open |
| `scenarios/sched/compat/enable-fsrs.yaml` | b | text | 2 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/compat/re-enable.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/loadbalancer/crafted-days.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/loadbalancer/siblings.yaml` | b | text | 1 | generic subquery nid in (select id from notes where mid ...) | open |
| `scenarios/sched/memory/crafted-rows.yaml` | b | text | 8 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/memory/ignore-before-boundary.yaml` | b | text | 3 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/memory/ignore-before-count.yaml` | b | text | 8 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/memory/ignore-before.yaml` | b | text | 5 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/memory/lrt-repair.yaml` | b | text | 10 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/memory/no-history.yaml` | b | text | 3 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/memory/on-the-fly.yaml` | b | text | 4 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/memory/recompute.yaml` | b | text | 11 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/memory/reset.yaml` | b | text | 7 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/memory/skipped-rows.yaml` | b | text | 6 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/memory/sm2-history.yaml` | b | text | 8 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/options/missing-preset.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/options/remap-steps.yaml` | b | text | 5 | numbered sequence (c1, c2, ... or #c1, #c2, ...) against a numbered table literal | open |
| `scenarios/sched/options/undo-save.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sched/revlog/kept-rows.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `goldens/stats/studied-today/refresh.json` | a | text | 2 | oracle output: Studied-today line | open |
| `goldens/stats/studied-today/zero-line.json` | a | text | 1 | oracle output: Studied-today line | open |
| `goldens/stats/today/empty.json` | a | text | 1 | oracle output: Studied-today line | open |
| `goldens/stats/widget/day-start-fall-back.json` | a | text | 1 | oracle output: Studied-today line | open |
| `goldens/stats/widget/day-start-future-row.json` | a | text | 1 | oracle output: Studied-today line | open |
| `goldens/stats/widget/day-start-legacy.json` | a | text | 1 | oracle output: Studied-today line | open |
| `goldens/stats/widget/day-start-spring-forward-r2.json` | a | text | 1 | oracle output: Studied-today line | open |
| `goldens/stats/widget/day-start-spring-forward-r4.json` | a | text | 3 | oracle output: Studied-today line | open |
| `goldens/stats/widget/day-start-summer.json` | a | text | 1 | oracle output: Studied-today line | open |
| `scenarios/stats/browse-queries/ae60-young.yaml` | a | text | 1 | search texts that Anki's Card Counts graph opens in the browser (interface constants in specs/stats/browse-queries.md) | open |
| `scenarios/stats/browse-queries/card-counts-labels.yaml` | a | text | 6 | search texts that Anki's Card Counts graph opens in the browser (interface constants in specs/stats/browse-queries.md) | open |
| `scenarios/stats/browse-queries/parse-all.yaml` | a | text | 7 | search texts that Anki's Card Counts graph opens in the browser (interface constants in specs/stats/browse-queries.md) | open |
| `scenarios/stats/browse-queries/calendar-cells.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/card-info/ae46-no-state.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/card-info/edge-values.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/card-info/intraday-learning.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/card-info/mixed-history.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/card-info/no-lrt-elapsed.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/card-info/params.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/card-info/times.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/card-info/zone-dst.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/buttons/groups.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/buttons/scope-parts.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/buttons/tz-matrix/kathmandu-scopes.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/buttons/tz-matrix/kathmandu.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/buttons/tz-matrix/lord-howe-dst-scopes.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/buttons/tz-matrix/lord-howe-dst.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/buttons/tz-matrix/other-creation-zone-scopes.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/buttons/tz-matrix/other-creation-zone.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/buttons/tz-matrix/st-johns-scopes.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/buttons/tz-matrix/st-johns.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/calendar/ae58-paris-spring-forward.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/calendar/history-years.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/hourly/current-offset.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/hourly/kathmandu-edges.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/hourly/near-epoch.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/hourly/range-edges.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/hourly/row-kinds.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/retention/ae59-history-parts.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/retention/periods.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/retention/row-filter.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/reviews/day-edges.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/reviews/kinds-split.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/reviews/time-wrap.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/reviews/window-start.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/scope/deleted-card-rows.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/scope/filtered-deck-card.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/scope/history-window.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/scope/moved-card.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/scope/odd-row-values.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/scope/read-only.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/scope/scope-follows-retag.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/scope/unreadable-card.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/graphs/scope/unreadable-row.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/studied-today/time-units.yaml` | b | text | 4 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/ae53-deleted-card.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/day-start-chatham.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/day-start-kathmandu.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/day-start-los-angeles-dst.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/day-start-paris-dst.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/day-start-st-johns.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/day-start-utc.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/deck-subtree.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/empty.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/lines-mix.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/no-mature.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/odd-values.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/today/search-scopes.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/widget/day-start-fall-back.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/widget/day-start-future-row.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/widget/day-start-legacy.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/widget/day-start-spring-forward-r2-early-row.yaml` | b | text | 2 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/widget/day-start-spring-forward-r2.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/widget/day-start-spring-forward-r4.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/stats/widget/day-start-summer.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `goldens/stats/studied-today/format-table.json` | c | text | 1 | inputs [3, 13000] and [300, 5400000] were the two cases of an Anki unit test; the golden shows its asserted lines | rewritten; residual (a): the zero line |
| `goldens/storage/revlog/studied-today-window.json` | a | text | 1 | oracle output: Studied-today line | open |
| `scenarios/storage/revlog/last-review-pick.yaml` | b | text | 11 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/storage/revlog/studied-today-window.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sync/normal/many-objects.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/sync/wire/records-up.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |
| `scenarios/undo/revlog-taken-id.yaml` | b | text | 1 | revlog column list in schema-18 order (id, cid, usn, ease, ivl, lastIvl, factor, time, type) | open |

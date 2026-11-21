# Test-data similarity flags: Anki side (2026-11-21)

Spec-side evidence for `docs/process/review-log/similarity-test-data-dispositions-20261121.md`.
Read at tag 26.09.3 (`refs/anki-26.09.3`) and in the reviewer view of
`similarity/20261121T152759Z-7385` (182 flagged files). The index reports
each matching blob under the first tag that holds it, often 2.1.50; every
group below was checked against the same file at 26.09.3. No Anki text is
copied here.

## (c), rewritten

| Scenario | Anki file | What was taken |
|---|---|---|
| `model/card-data/rewrite` (and `specs/model/card-data-json.md` C5 row 1) | `rslib/src/storage/card/data.rs`, unit test of card-data float compaction | its four input values |
| `stats/studied-today/format-table` | `rslib/src/stats/today.rs`, unit test of the studied-today line | its two (cards, seconds) cases |
| `maintenance/check-db/all-records`, `notetypes/structure/template-swap-replace` | `pylib/tests/test_models.py`, template-removal test | its orphan-card count query |

## (a), oracle output

| Output | Anki file holding the same text |
|---|---|
| hint filter HTML (render corpus, filters, pipeline, preservation) | `rslib/src/template_filters.rs` (product code) |
| type-answer comparison HTML | `rslib/src/typeanswer.rs` (unit tests asserting the output) |
| cloze span HTML | `pylib/tests/test_models.py`, `rslib/src/template_filters.rs` tests |
| Image Occlusion div HTML | `rslib/src/cloze.rs` tests |
| studied-today zero line | `rslib/src/scheduler/service/mod.rs` test |
| Card Counts search texts (stats browse-queries scenarios) | `ts/routes/graphs/card-counts.ts` (product code; reported under 2.1.50's `ts/graphs/`) |

## (b), generic

| Our input | Anki file |
|---|---|
| review-log insert or select naming the nine columns in schema order | `rslib/src/dbcheck.rs` test |
| Image Occlusion field with coordinates .1/.1/.2 | `ts/tests/e2e/io-mask-save-race.spec.ts` (one seed note) |
| `c1, c2, ...` and `#c1, #c2, ...` sequences | `rslib/ascii_percent_encoding/src/lib.rs` (the index reports it under 2.1.56; the file is absent at 26.09.3, so it was not read; the match is the numbered words alone) |
| repeated `i` in media names (lex) | `rslib/src/deckconfig/schema11.rs` (24.04; product code that reads many fields of a variable named `i`) |
| `nid in (select id from notes where mid ...)`, `mid in (select id from notetypes where name ...)` | `rslib/src/storage/notetype/mod.rs` (product SQL), `rslib/src/search/sqlwriter.rs` (product SQL and its test module) |
| an update of orphan cards (`package/apkg-import/guid-other-id`) | `pylib/tests/test_models.py` (shares only the subquery) |

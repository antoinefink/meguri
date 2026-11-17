# Interface text sources (U167)

Spec-writer note for `specs/text/interface-text.md`. Session
s-20261110T085429Z-165d83d3 (2026-11-10), shell-platform item (U165, U167,
U168). Read at tag 26.09.3 (`refs/anki-26.09.3`). Only English interface
strings and the fact that names are taken from translations were read; no
code was copied.

| Spec section | Source | Read for |
|---|---|---|
| I7 | `ftl/core/exporting.ftl` | confirmation of `package/transfer-strings` S3: Anki's legacy-export label carries a parenthesised suffix after "Support older Anki versions", which R1618 and R1625 leave out |
| I5 (stock names) | `rslib/src/notetype/stock.rs`, `rslib/src/decks/remove.rs` | confirmation that Anki takes the stock note type and field names and the default deck name from its translations at creation; the rule itself is already in `notetypes/stock-kinds`, `decks/deck-presets` and `storage/new-collection` |
| I1 to I4, I6, I8 | no Anki file | an index of existing specs (`stats/studied-today`, `stats/card-info`, `sched/answer-labels`, `review/reviewer-strings`, `render/*`, `text/description-html`, every `*-strings` file, `editor/formatting` F19, `editor/paste` P21), the Product Contract's R118, R153, R156, R192, R1448, R1674 and R2029, the plan's U167 unit and its Open Questions default for Card Info display strings |

Review round 1 (2026-11-10): I3 gained `package/transfer-strings`,
`maintenance/maintenance-strings` and `editor/image-occlusion-strings` and
the full catalog list of `editor/editing-strings`; the value checks were
placed with each file's own catalog test; I4's language rule was split per
row, and the visible answer-label row was left open against R192 instead of
being decided here; I7's scope and its Product Contract rows were added.

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| Classification of h7e4c18df6974, h420d96663fe4, h74b7d2e31f18, hbcc38ba4659c as native-ui under text/interface-text I1-I8, render/display-transform D8/D10, review/reviewer-strings S1/S13; proposed I6.1 and I8 test cases | `pylib/tests/test_collection.py` (test_translate); `rslib/i18n/src/lib.rs` (test module: numbers, decimal_rounding, i18n) | What the rows require: English text for a template problem and count plurals; English fallback, argument substitution and plurals; the locale decimal separator, with English text keeping a full stop; the plural form of a rounded decimal count following the written number. Used only to name the obligations. The proposed spec text uses Meguri's own values, locale and nouns (0.9996, 1.25, 1.4, minutes, de-DE, 3.25); no value, locale pair or string from those tests is carried over. |

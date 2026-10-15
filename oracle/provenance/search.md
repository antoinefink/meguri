# Search grammar, errors, canonical text and modes (U80)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/search/grammar.md`,
`errors.md`, `canonical-form.md`, `query-model.md` and
`specs/conformance/search-ops.md`. No code was copied; every rule was restated
in the spec writer's words and checked as a black box against the oracle
image's Anki (about 800 search texts through the search-string builder, plus
card and note searches on small collections, damaged ones included).

| Spec section | Anki file | Read for |
|---|---|---|
| grammar G1 | `rslib/src/search/parser.rs`, `qt/aqt/browser/browser.py` | whitespace replacement and trimming; the desktop field's own whitespace pass (Python `\s`, which also covers U+001C–U+001F) |
| grammar G2–G5 | `rslib/src/search/parser.rs` | item sequence rules, implicit AND, operator words, negation (including a negated operator word), groups, the three term forms and what ends them |
| grammar G6–G8.1 | `rslib/src/search/parser.rs` | key split at the first unescaped colon, reserved keys compared before unescaping, per-key value readings, `prop:` property order and operator order |
| grammar G7 | `rslib/src/search/parser.rs` | the unescape check and replacement, the quote-only and dupe readings, raw values (`preset:`, `is:`, id lists, `prop:cds:`) |
| grammar G9 | Rust standard library `FromStr` for `u8`, `u16`, `u32`, `i32`, `i64`, `f32` | number forms (not Anki code) |
| errors E1–E2, E4 | `rslib/src/error/search.rs`, `rslib/src/search/parser.rs`, `ftl/core/search.ftl` | the error categories, the texts each reports (`provided`, `context`), backtick replacement in messages, detection order |
| errors E3 | `rslib/src/error/db.rs`, `rslib/src/error/mod.rs`, `rslib/src/backend/error.rs`, `rslib/src/search/sqlwriter.rs` | invalid regex mapped from the SQL function's failure (so only when a row reaches it); database errors from a negated operator word, non-ASCII id digits, expression depth |
| errors E6 | oracle runs | nesting depth at which Anki's process aborts (about 10 000 groups) |
| canonical-form F1–F6 | `rslib/src/search/writer.rs`, `rslib/src/search/service/mod.rs`, `rslib/src/search/service/search_node.rs`, `rslib/src/search/builder.rs`, `qt/aqt/browser/browser.py` | the writer per node, the quoting rule, the top-level group unwrapping of the builder used when the browser commits a search, f32 `Display` |
| canonical-form F7 | oracle runs | the three shapes whose canonical text does not round-trip |
| query-model Q4–Q6 | `rslib/src/search/sqlwriter.rs`, `rslib/src/search/mod.rs` | per-term table requirements and how they combine, the cards-only / notes-only / joined statements, distinct note ids |
| query-model Q8 | `rslib/src/search/sqlwriter.rs`, `rslib/src/config/bool.rs`, `rslib/src/config/mod.rs` | which outside values each term reads; config keys and their defaults |
| search-ops S1–S3 | `pylib/anki/collection.py`, `rslib/src/search/service/mod.rs`, `rslib/src/i18n` (Fluent isolation marks) | card and note searches without order, the browser's normalization call, how messages carry the reported texts |

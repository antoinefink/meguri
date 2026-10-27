# Search text matching, fields and case folding (U81)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/search/text-matching.md`,
`fields.md` and `case-folding.md`. No code was copied; every rule was restated in
the spec writer's words and checked as a black box against the oracle image's
Anki (probe scenarios over plain, field, `nc:`, `sc:`, `w:` and `re:` searches,
excluded fields, normalization, ignore accents, limits and fold-dependent
characters; the kept ones are under `conformance/scenarios/search/{text,fields,folding,regex}/`).

| Spec section | Anki file | Read for |
|---|---|---|
| text-matching T1, T4, T5 | `rslib/src/search/sqlwriter.rs` | which value each text term passes on, NFC of values (conditional) and field names (always), the substring pattern over the sort field and the field string |
| text-matching T2 | `rslib/src/storage/note`, `rslib/src/storage/schema11.sql` | the sort field column and its integer affinity (affinity conversions checked with SQLite) |
| text-matching T3, T13 | `rslib/src/text.rs` | glob to `LIKE` conversion, glob to regular expression conversion, the wildcard test |
| text-matching T3.3 | oracle runs, SQLite `SQLITE_MAX_LIKE_PATTERN_LENGTH` | the 50,000-byte pattern limit and that it counts the separators a field position adds |
| text-matching T6 | `rslib/src/search/sqlwriter.rs`, `rslib/src/config/bool.rs` | which terms read `ignoreAccentsInSearch` and where the transform applies (pattern after conversion, field text, `w:` expression) |
| text-matching T7 | `rslib/src/search/sqlwriter.rs` | per note type runs of included fields, sort field exclusion, per-field regular expressions, the statements that fail when a note type or the collection has no included field |
| text-matching T8 | `rslib/src/text.rs`, `rslib/src/storage/sqlite.rs` | NFKD, mark removal and the extra replacement table; the transform applied to the converted pattern |
| text-matching T9 | `rslib/src/cloze.rs`, `rslib/src/storage/sqlite.rs`, `rslib/src/search/sqlwriter.rs` | the cloze removal pattern, its use on the field string and sort field, the cloze note type restriction and why a directly negated `sc:` negates only that restriction |
| text-matching T10, T11 | `rslib/src/search/sqlwriter.rs`, `rslib/src/storage/sqlite.rs` | `w:` and `re:` expressions, whole field string versus per-field tests |
| text-matching T12 | `rslib/src/storage/sqlite.rs`, `rslib/src/error/db.rs`, `rslib/src/error/mod.rs`, `Cargo.lock` (`regex` 1.11.2) | lazy compilation in the SQL functions, parse errors versus other compile errors, default builder limits |
| fields F1–F7 | `rslib/src/search/parser.rs`, `rslib/src/search/sqlwriter.rs`, `rslib/src/text.rs` | mode prefixes, all-fields names, name resolution (unicase equality or case-insensitive pattern), runs of matching positions and their patterns, nc and regex modes |
| case-folding C2–C6 | `rslib/src/search/sqlwriter.rs`, `rslib/src/text.rs`, `regex-syntax` 0.8.6 tables | which comparisons use `LIKE` (ASCII) and which a case-insensitive regular expression; the simple folding table version |

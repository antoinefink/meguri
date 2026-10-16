# meguri-search

- Specs: `specs/search/` (grammar, errors, canonical-form, query-model; U81 and U82 add theirs). Messages come only from `errors.md` E7.
- Nothing may recurse on the input's nesting (E6): the query is an arena (`ast.rs`) and every walk (parser, canonical writer, plan, tests' deep cases) uses an explicit stack. A recursive helper or a `Box` tree overflows on `((((…))))`.
- Two position spaces: the lexer's indexes into the normalized, trimmed characters, and the caller's character offsets (`Lexer::input_range`; the only shift is the trimmed lead). Node spans and error ranges are the caller's; the FFI converts them to UTF-16.
- Canonical text is checked only against oracle goldens (`canonical_search` in T0) and the spec's examples, never against Meguri's own output.
- Terms compile in `plan/sql.rs` `compile_term`; U81 and U82 replace its `NotYetSupported` stubs (a `BuildError`, never a silent `0`). `build_with` raises E3's `search_failed` before any stub, left to right: keep plan-time errors in that one pass.
- `Term::scope` is Q3's table and decides the rows (Q5): a term must read only the tables its scope names.

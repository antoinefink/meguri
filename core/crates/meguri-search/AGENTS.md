# meguri-search

- Specs: `specs/search/` (grammar, errors, canonical-form, query-model; U81 and U82 add theirs). Messages come only from `errors.md` E7.
- Nothing may recurse on the input's nesting (E6): the query is an arena (`ast.rs`) and every walk (parser, canonical writer, plan, tests' deep cases) uses an explicit stack. A recursive helper or a `Box` tree overflows on `((((…))))`.
- Two position spaces: the lexer's indexes into the normalized, trimmed characters, and the caller's character offsets (`Lexer::input_range`; the only shift is the trimmed lead). Node spans and error ranges are the caller's; the FFI converts them to UTF-16.
- Canonical text is checked only against oracle goldens (`canonical_search` in T0) and the spec's examples, never against Meguri's own output.
- Terms compile in `plan/sql.rs` `compile_term`; U81 and U82 replace its `NotYetSupported` stubs (a `BuildError`, never a silent `0`). `build_with` raises E3's `search_failed` before any stub, left to right: keep plan-time errors in that one pass.
- `Term::scope` is Q3's table and decides the rows (Q5): a term must read only the tables its scope names.
- `tests/canonical_roundtrip.rs` runs plans through `Plan::build_with` (test-hooks, turned on by the testkit dev-dependency) with a stand-in term compiler, on in-memory tables that hold ids only. The product's term compilers (U81/U82) need a schema-18 collection instead.
- `config.rs` (U83, `specs/browser/config-keys.md`) only reads and computes: an action's planner returns the whole value to write, or `None` for "write nothing", and meguri-engine's `browser_config` applies it through C4. Its `Column` is K3's catalog; U84 extends it rather than keeping a second column list.

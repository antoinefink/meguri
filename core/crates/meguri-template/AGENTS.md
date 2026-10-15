# meguri-template

- Behavior is `specs/render/template-syntax.md`, `filters.md` and `special-fields.md`; where the Product Contract's wording differs (R953, R959, R960, R964, R968), the spec follows the oracle and wins (T14, F9). The oracle comparison is T0: the testkit's `template` adapter (`meguri-testkit/src/exec/render.rs`) over `conformance/scenarios/render/`.
- Nodes stay flat (open, contents, close) and every walk keeps its own stack: no recursion, so 5 MB templates and any nesting depth work (T13). Keep the tokenizer's closer checks on the last occurrence and its opener lookups cached; a rescan per tag is quadratic.
- Node spans tile the source and must stay exact in both delimiter modes: U34 rewrites references and U200 highlights tags from them (T12).
- A side renders on its own. Card composition (which side's error counts, the blank check, the FrontSide text) is the caller's: the testkit adapter does Q1.4, U62 passes the play-marker question as FrontSide.
- `cloze` and `cloze-only` are built-in names that leave the text unchanged until U61 implements them in `filters/`; the testkit adapter refuses them, and cloze note types, until then. Remove those refusals with U61.
- `filters/furigana.rs` relies on the `regex` crate's leftmost-first, lazy matching for F4's shortest base and reading. `hint` ids use the `=`-pinned `blake3`.

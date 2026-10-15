# meguri-text

- Parity code follows `specs/text/`; each function names its section. Field stripping, media tags and sound tags are `regex` patterns with leftmost-first semantics (strip-html H1). Never swap in an HTML parser there: `sfld`, `csum` and duplicate checks would drift. `description.rs` is the only parser use (html5ever), because its output is only displayed and has no oracle parity.
- Functions take one value. "As saved" (`checksum-and-sort-field.md` C1: `normalize::field_as_saved`, then C2, C4 or E1) is composed by callers: U23, U27 and the testkit `text` adapter (`meguri-testkit/src/exec/text.rs`, observations.md O9).
- `src/unicode10_unassigned.rs` is generated. Do not edit it. Regenerate it with `python3 -I gen/unicode10_unassigned.py <UnicodeData.txt>`; the script names the Unicode 10.0.0 file and checks its SHA-256.
- The outputs depend on the `=`-pinned crates (`regex`, `htmlescape`, `unicase`, `unicode-normalization`, `sha1`). A version change is an oracle re-pin, never a routine update. `html5ever` and `pulldown-cmark` are pinned too, so the description output changes only with a deliberate bump.
- `tests/corpora.rs` replays every corpus input against its golden. `tests/props.rs` restricts the IRI round trip to texts that meet M8's four conditions; the other texts do not round-trip, by design.

# Cloze specs and the `cloze` observation (U61)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/render/cloze.md` and
`specs/conformance/cloze-ops.md`. No code was copied; every rule was restated in
the spec writer's words and checked against the oracle image's Anki
(`meguri-oracle:26.9.3-34c3a09eb2d55813`) as a black box: all 137 scenario
inputs, plus the spec's examples, through the cloze-number query, type-in
extraction and the template renderer (a standard note type, `{{cloze:Front}}`
or `{{cloze-only:Front}}` followed by a sentinel field that the probe removed).

| Spec section | Anki file | Read for |
|---|---|---|
| cloze C2-C4 | `rslib/src/cloze.rs` | marker grammar, number-list parsing (u16, dedupe, sort), tokenizing order, depth limit 10, hint split at the first `::` per text run, stray closers, unclosed clozes dropped |
| cloze C5 | `rslib/src/cloze.rs`, `rslib/src/notetype/cardgen.rs`, `rslib/src/notes/mod.rs`, `rslib/src/notes/service.rs`, `rslib/src/template.rs` | numbers excluding 0 and nested clozes, top-level presence for the note field check, union over fields, missing-cloze use |
| cloze C6, C9, C10 | `rslib/src/cloze.rs`, `rslib/src/template_filters.rs`, `rslib/src/template.rs` | side from FrontSide presence, card number = ordinal + 1, span markup, empty output without an active cloze, cloze-only order and joiner, all-identical collapse for typing |
| cloze C7 | `rslib/src/cloze.rs`, `Cargo.lock` | attribute encoder used for `data-cloze` |
| cloze C8 | `rslib/src/cloze.rs`, `rslib/src/text.rs` | MathJax pattern (either closer), markup-only stripping inside it, applied to `cloze:` only |
| cloze C11 | `rslib/src/cloze.rs` | image-occlusion prefix, class choice, element frame; shape attributes left to U63/U64 |
| cloze C13 | `rslib/src/cloze.rs` at the tag | the tokenizer has no MathJax state at 26.09.3 (main-branch tests are supplementary rows) |
| cloze-ops Z3 | `proto/anki/card_rendering.proto`, `proto/anki/notes.proto`, `rslib/src/card_rendering/service.rs`, `rslib/src/notes/service.rs`, `rslib/src/notetype/render.rs`, `pylib/anki/collection.py`, `qt/aqt/reviewer.py` | backend calls that expose numbers and typing extraction, how the renderer adds blank-front and missing-cloze messages, how the reviewer passes ordinal + 1 |

Third-party crate source read for exact behavior (not Anki code): `htmlescape`
0.3.1 `encode_attribute` (minimal entity table, `&#xHH;` with upper-case hex for
other characters below U+0100, everything from U+0100 unchanged).

Harvest rows classified for U61: the `rslib/src/cloze.rs` tests, the `cloze` test
of `template_filters.rs`, the type-in extraction and cloze render tests of the
card-rendering service, and nine supplementary main-branch MathJax tests (not
applicable at the tag).

Driver `observe/cloze.py` (oracle maintainer, same tag): read
`rslib/src/template.rs` (the blank-front check looks only at whether a
referenced field is non-empty, hence the sentinel field),
`rslib/src/notetype/render.rs` (a template with a null `ord` renders a
synthesized card with `template_idx` = `card_ord`), `rslib/src/notetype/schema11.rs`
(legacy template JSON), `rslib/src/notes/service.rs` (cloze numbers come back
unordered) and `pylib/anki/template.py` (the legacy uncommitted-card call).

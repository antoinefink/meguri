# Render area specs and the `template` observation (U60)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/render/template-syntax.md`,
`filters.md`, `special-fields.md` and `specs/conformance/render-ops.md`. No code
was copied; every rule was re-derived in the spec writer's words and checked by
black-box probes of the oracle image (pylib's uncommitted render on a scratch
collection, nodes composed as below), which agree with every scenario under
`conformance/scenarios/render/`.

| Spec section | Anki file | Read for |
|---|---|---|
| template-syntax T2-T5 | `rslib/src/template.rs` | delimiter modes and the directive strip (leading white space, repeated copies), per-position tag-then-comment scan, first closing delimiter, tag classification (leading `{` strip, trim, byte length below 2, prefix after `#`/`/`/`^` trimmed at the start only), `rsplit(':')` key and filters |
| template-syntax T6-T9 | `rslib/src/template.rs`, `rslib/src/error/mod.rs` | nesting and close matching, the three structural errors, parse before render, hidden sections still rendered for errors, the unused no-closing-brackets variant |
| template-syntax T7 | `rslib/src/template.rs`, `pylib/anki/template.py` | FrontSide matched before the field lookup and filters dropped; empty key with filters deferred to add-on hooks (none in pylib); cloze-conditional test (`c` + u32 parse, so `+` is accepted) |
| template-syntax T10 | `rslib/src/template.rs` (render of both sides), `rslib/src/notetype/render.rs` | blank check after the question, answer not parsed when blank, notice appended as its own node |
| template-syntax T11 | `rslib/src/template.rs`, `rslib/src/notetype/mod.rs` (requirements on save, unparsable template gives none, ordinals sorted) | strict and legacy emptiness, referenced and cloze-referenced names, Any/All/None |
| filters F1-F2 | `rslib/src/template_filters.rs`, `pylib/anki/template.py` | the type rewrite on `[.., "type"]`, `["cloze","type"]`, `["nc","type"]`; unknown filter returns the remaining chain to Python, whose hooks are empty in pylib, so later built-ins never run |
| filters F3-F7 | `rslib/src/template_filters.rs`, `rslib/src/text.rs` | `strip_html`; the ruby pattern ` ?([^ >]+?)\[(.+?)\]` after `&nbsp;` replacement, sound readings kept; hint markup and BLAKE3 over text then field name, first 8 bytes hex; `tts ` prefix; marker formats |
| filters F9 | `qt/aqt/reviewer.py` (only `cloze:` and `nc:` prefixes read from the marker); Anki's main-branch changelog (`ci`, PR #5264, is after the tag) | why R964's `ci` has no oracle counterpart |
| special-fields P2-P6 | `rslib/src/notetype/render.rs`, `rslib/src/notetype/mod.rs` (special field list), `rslib/src/notetype/cardgen.rs`, `rslib/src/decks/mod.rs` (deck id fallback), `rslib/src/decks/name.rs`, `rslib/src/card/mod.rs` (flags byte unmasked) | values, `entry().or_insert` precedence, `c<ord+1>` = `1`, `(Deck)`, `flag` + whole flags byte, card generation's special set |
| render-ops Q1 | `proto/anki/card_rendering.proto`, `rslib/src/card_rendering/service.rs`, `rslib/src/notetype/schema11.rs` (template `ord` optional), `pylib/anki/template.py` | the uncommitted render call and how pylib completes partial nodes |

## Driver hints for the `template` kind (oracle maintainer)

Implemented in `driver/observe/template.py`; the driver's own reads are in `driver-render.md`.

- New-note cases: in a scratch collection per worker (as `observe/text.py`), save a note type with exactly `notetype.name`, `kind` and `fields` (remove any note type of that name first, stock ones included, since names must stay exact for the `Type` special; check the saved field names equal the given ones, else `invalid_scenario`), with one valid dummy template (`{{<first field>}}`, or `{{cloze:<first field>}}` for cloze). Per case, build a new note of it (fields and tags as given, unsaved) and call the backend's legacy uncommitted render with a schema-11 template object `{name, ord: null, qfmt, afmt}`, the case's `ord`, `fill_empty` false and `partial_render` true. Never save the case's template: a save validates it.
- Card cases: the stored note, `card_ord` = the card's ord, template object `{name: <stored template name at that ord, first template for cloze>, ord: <card ord>, qfmt, afmt}` so the stored card is found by ordinal.
- Composing nodes, as pylib does with no add-on hooks: text nodes as they are; a replacement node contributes its current text, except `FrontSide`, which contributes nothing on the question side and the composed question text on the answer side. When the response's empty flag is set, drop the last question node (the blank notice) and report `answer` null.
- Errors arrive as one template-error message (localized HTML). Classify it through the backend's own translations, never literal text (oracle/AGENTS.md): build each candidate message with sentinel arguments and match its fixed parts. Side: the front-side or back-side problem header. Kinds: the not-closed message (`unclosed_conditional`), the wrong-closed message (`mismatched_conditional`), the not-open message (`unopened_conditional`), and the no-such-field message, shared by `unknown_field` and `unknown_conditional`: it is `unknown_conditional` exactly when the quoted tag is `{{#` or `{{^`, at least one more character, then `}}` (a replacement's content never starts with those unless it is that single character, which gives `{{#}}`). The quoted tag is HTML-escaped and wrapped in Unicode isolation marks; it may contain line breaks.
- The value never carries message text.

## Harvest links

`conformance/catalog/harvest.csv` render rows classified with U60 (scenarios linked); the test names behind them are in `oracle/harvest/links.csv` only.

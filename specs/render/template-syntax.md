# Template syntax and evaluation

Spec: render/template-syntax
Requirements: R529, R953, R954, R955, R956, R957, R958, R959, R1805, R1907, R1908, R1909
Anki: 26.09.3
Scenarios: conformance/scenarios/render/template/, conformance/scenarios/render/corpus/

How a card template's text becomes card HTML: delimiters, tags, comments, conditionals, field replacements, errors and the blank-front check, plus the analysis that card generation and note type validation reuse. Filters are `filters.md`; the names and values of special fields are `special-fields.md`. Output must equal Anki's string for string (R1907): where this file and a requirement's wording differ, this file describes Anki 26.09.3 and wins (T14 lists the differences).

## T1. The engine's contract

- **Inputs** for one side: the template text; the field map (field name to text, built as `special-fields.md` P2 says); the card ordinal (0-based); the side, question or answer; on the answer side, the FrontSide text (`special-fields.md` P4). Whether the note type is standard or cloze matters only to the blank-front check (T10) and to the `cloze` filters (`filters.md` F8).
- **Output**: the side's text, or an error (T9).
- Field names, keys and filter names compare by exact code points: no case folding, no Unicode normalization, no trimming beyond what T4 and T5 say. `{{front}}` does not find a field named `Front`, and a key typed in decomposed form does not find a field whose name is stored composed (NFC).
- Nothing in the text is escaped or unescaped by the engine: field text, names and filter output are inserted as they are.

## T2. Delimiters

- Tags normally open with `{{` and close with `}}`.
- **Alternate delimiters.** When the template text, after removing Unicode White_Space at its start, begins with the exact 11 characters `{{=<% %>=}}` (one U+0020 on each side of `<% %>`), then:
  - the leading white space and that directive are removed, and so is every immediately repeated copy of the directive (`{{=<% %>=}}{{=<% %>=}}x` keeps `x`);
  - in the rest of that template, tags open with `<%` and close with `%>`, and `{{…}}` is plain text.
- The directive anywhere else, or with other spacing or delimiters (`{{=<%  %>=}}`, `{{=[[ ]]=}}`), is an ordinary tag: a replacement of a field with that strange name, so an `unknown_field` error (T9). In alternate mode a directive that was not removed (`{{=<% %>=}} {{=<% %>=}}x`: the space stops the removal) holds the tag `<% %>`, whose empty name is also `unknown_field`. The choice is per template text: a question in alternate mode says nothing about its answer.
- Comments (T3) are the same in both modes. Error reports describe tags with `{{ }}` in both modes; Meguri's own wording is free (T9).

## T3. Tags, comments and text

The template is split into tags, comments and text, left to right:

1. A **tag** can begin at a position that starts with the opening delimiter, provided the closing delimiter occurs somewhere after the opening delimiter. It then ends at the end of the first closing delimiter found after the opening one. Its **content** is what lies between. Content may be empty (`{{}}`) and may hold anything, including `{{`, `<!--`, `-->` and line breaks.
2. A **comment** can begin at a position that starts with `<!--`, provided `-->` occurs somewhere after those four characters. It ends at the end of the first such `-->`. Its content is what lies between and is never evaluated.
3. The next token is whichever of the two begins earliest. Everything before it is **text**. When no tag or comment can begin anywhere in the rest, the rest is text.

Consequences:

| Template | Tokens |
|---|---|
| `{{{Front}}}` | tag with content `{Front`, then text `}` |
| `{{{{Front}}}}` | tag `{{Front`, then text `}}` |
| `{{Front` (no `}}` later) | text: no tag, no error |
| `{{a{{Back}}` | one tag with content `a{{Back` |
| `{{Front <!-- }} -->` | tag `Front <!--`, then text ` -->` |
| `<!-- {{#X}} -->{{F}}` | comment ` {{#X}} `, then tag `F` |
| `<!-- {{Back}}` (no `-->` later) | text `<!-- `, then tag `Back` |
| `<!---->`, `<!-- a --> --> ` | a comment with empty content; a comment ` a `, then text ` --> ` |
| `<!-->x` | text (no `-->` after the opening `<!--`) |
| `text }} more` | text |

A position inside an earlier tag or comment is never a start. The scan is defined on characters, but every delimiter is ASCII, so byte offsets give the same tokens.

## T4. Kinds of tag

From a tag's content:

1. Remove every leading `{` (only `{`, and only at the very start: ` {Front` keeps its brace).
2. Remove Unicode White_Space at both ends (U+00A0, U+3000, line breaks and TAB included).
3. If what remains is shorter than 2 bytes (empty, or one ASCII character such as `#`, `/` or `^`), the tag is a **replacement** with that whole text as its name: `{{#}}`, `{{/}}`, `{{^}}` and `{{}}` are replacements of fields named `#`, `/`, `^` and the empty name.
4. Otherwise, if it starts with `#`, it is a **conditional** open; with `^`, a **negated conditional** open; with `/`, a **close**. The **key** is the rest after that character, with Unicode White_Space removed at its start. The key is not split on `:`: `{{#text:Front}}` tests a field named `text:Front`.
5. Otherwise it is a **replacement** (T5).

So `{{ Front }}`, `{{Front}}` and `{{{Front}}}` all name `Front`, but the third leaves a `}` of text after it (T3). `{{# A }}`, `{{#A}}` and `{{#  A}}` all open `A`; `{{/ A}}` closes `A`.

## T5. Replacements and filter chains

- A replacement name is split at every `:`. The last piece is the **key** (the field), and the pieces before it are **filter names**. Pieces are not trimmed: `{{text: Front}}` asks for the field ` Front` (leading space), which does not exist; `{{ text:Front }}` is fine because T4 trimmed the ends.
- Filters apply from the one nearest the key outward: `{{f2:f1:Front}}` applies `f1` to the field's text, then `f2` to the result. An empty piece (from `::` or a leading `:`) is a filter that changes nothing: `{{::Front}}` and `{{:Front}}` equal `{{Front}}`.
- How filters act, and what an unknown filter does to the rest of the chain, is `filters.md` F1.
- A key may contain any character except `:` (including spaces, `}`, `{`, `<`, non-Latin scripts and emoji). Field names never hold `:`, `{`, `}` or `"` and never start with `#`, `/`, `^` or white space (note type rules), so keys with those can only be unknown fields or special names.

## T6. Conditional structure

- An open (`#` or `^`) starts a section that runs to the matching close: the next close at the same nesting level. Sections nest to any depth.
- A close matches when its key equals the open key exactly.
- Structural errors, found before anything is rendered, in the order they are met reading the template from the start:
  - a close whose key differs from the innermost open section's key: `mismatched_conditional` (`{{#A}}x{{/B}}`, and `{{#A}}{{#B}}{{/A}}{{/B}}` fails at `{{/A}}`);
  - a close with no open section: `unopened_conditional` (`{{/B}}` at top level);
  - the end of the template inside an open section: `unclosed_conditional`.
- Only the first structural error counts. A structural error anywhere in a side wins over every T8 error of that side, even one earlier in the text: `{{Nope}}{{#Back}}` is `unclosed_conditional`.
- Tags inside comments take no part: `<!--{{#A}}-->{{/A}}` is `unopened_conditional`, and `<!--{{#A}}{{/B}}-->` is fine.

## T7. Rendering

The side's output is its tokens in order:

- **Text** is output unchanged, braces and all.
- **A comment** is output unchanged, delimiters included (`<!--` + content + `-->`). It is never evaluated, so names inside it never cause errors and never count as references.
- **A replacement** outputs, in this order of rules:
  1. key `FrontSide`, whatever its filters: the FrontSide text of T1 on the answer side, nothing on the question side. Filters on it are ignored (`{{text:FrontSide}}` and `{{type:FrontSide}}` equal `{{FrontSide}}`). This holds even when the note has a field named `FrontSide` (`special-fields.md` P4).
  2. empty key with at least one filter (`{{text:}}`, `{{:}}`, `{{type:}}`): nothing, and no error.
  3. a key in the field map: the field's text through the filter chain (`filters.md`).
  4. any other key, including the empty key with no filter (`{{}}`): `unknown_field` (T9).
- **A conditional section** (`#`) outputs its contents when its key's value is non-empty; a **negated section** (`^`) when it is empty. Emptiness is `text/field-emptiness#E1` applied to the value in the field map as given (for saved notes that is E2; a note not yet saved gets no save normalization). The key:
  - in the field map: its value decides;
  - not in the field map but `c` followed by text that reads as an unsigned 32-bit decimal integer (ASCII digits, optionally led by one `+`, value at most 4294967295): treated as empty, so `#` hides and `^` shows, without error. The map holds `c<ordinal+1>` (`special-fields.md` P2), so `{{#c2}}` shows on the card with ordinal 1 only. `{{#c02}}` and `{{#c+2}}` never match a map entry, so they always hide;
  - otherwise (`C2`, `c`, `c2x`, `c99999999999`, `FrontSide` without a field of that name, an unknown name): `unknown_conditional` (T9).
- Contents of a section that is not shown are still checked: an unknown field or unknown conditional inside a hidden section is an error too (`{{#Empty}}{{Nope}}{{/Empty}}` fails).

Examples (fields `A`, `B`; `x` stands for any non-empty value):

| `{{#A}}{{^B}}x{{/B}}{{/A}}` with A, B | Output |
|---|---|
| empty, empty | empty |
| empty, non-empty | empty |
| non-empty, empty | `x` |
| non-empty, non-empty | empty |

## T8. Render errors

While rendering (after T6 found no structural error), the first problem in template order, hidden sections included, is the side's error:

- `unknown_field`: a replacement whose key is not in the field map and none of T7's rules 1 and 2 applies;
- `unknown_conditional`: a section key that T7 rejects.

## T9. Errors of a card

| Kind | Raised by |
|---|---|
| `unclosed_conditional` | T6 |
| `unopened_conditional` | T6 |
| `mismatched_conditional` | T6 |
| `unknown_field` | T7, T8 |
| `unknown_conditional` | T7, T8 |

- There is no error for an unclosed tag: `{{Front` with no `}}` after it is text (T3).
- A card's error is the question side's error if it has one; otherwise, if the question is not blank (T10), the answer side's error. A blank question means the answer template is never examined, so its errors stay hidden.
- On an error, Meguri shows its own message in place of both sides (R959). Its wording is Meguri's; tests compare only the kind and the side (R1887). Anki's message says which side failed and repeats the offending tag; Meguri's should too.

## T10. Blank question

After a question renders without error, the card is **blank** when:

- standard note type: the question template's analysis (T11.2, strict form) says it renders nothing for the set of names whose field-map values are non-empty (E1). `FrontSide` is in that set only when the note has a non-empty field of that name (P4), so a question made only of text, comments, `{{FrontSide}}` and conditionals around such things is usually blank, whatever it outputs;
- cloze note type: no value of the field map, special values included, has ordinal + 1 among its cloze numbers; the templates play no part (`specs/render/cloze.md` C15, U61).

A blank card keeps its question output; its answer is not rendered. Anki then shows a "card is blank" notice with a help link on both sides; Meguri shows the same notice (`display-transform.md` D7, U62), and observations report `empty` (or the `render` kind's `blank`) instead of any notice text.

## T11. Analysis without rendering

The parse result (T12) answers these questions without rendering. Card generation (U27), note type validation (U29, R942) and the template editor (U200) use them.

### T11.1 Referenced names

- **Field references**: every replacement key, plus every section key that is not of the `c<number>` form of T7, from anywhere in the template outside comments, sections included. The result can hold special names, `FrontSide`, the empty key and names of no field: callers decide (validation ignores the empty name and special names).
- **Cloze references**: the keys of replacements whose filter chain contains a piece equal to `cloze`, at any position.

### T11.2 Would the template render something?

Given a set of non-empty names, a sequence of tokens **renders something** when, scanning it:

- a replacement whose key is in the set is met (filters do not matter); or
- a conditional section whose key is in the set has contents that render something; or
- a negated section whose key is **not** in the set has contents that render something.

Text, comments and sections that fail these tests contribute nothing. Two forms exist:

- **strict** (T10, card generation): as above;
- **legacy** (T11.3 only): a negated section's contents count whatever its key, as if every negated key were absent from the set.

Examples with non-empty names {A}: `{{B}}{{A}}` yes; `{{B}}` no; `{{#A}}{{^B}}{{A}}{{/B}}{{/A}}` yes; `{{^A}}{{B}}{{/A}}` no; `{{^A}}{{A}}{{/A}}` no (strict) and yes (legacy); `{{#A}}text{{/A}}` no; `{{text:A}}` yes; `{{FrontSide}}` no (it would count only for a non-empty note field named `FrontSide`).

### T11.3 Legacy card requirements

When a note type is saved, Anki stores per template a requirement (`reqs` in the note type config, `req` in schema-11 JSON: `specs/model/schema11-json.md` S4.1) computed from the question template with the legacy form of T11.2 and only the note type's own field names (no special names, no `c<N>`):

1. **any**: the fields that make the template render something when each is the only non-empty field. If there is at least one, the requirement is `any` with those field ordinals.
2. Otherwise **all**: start from every field ordinal; drop each field whose removal (all other fields non-empty) still lets the template render something. If some ordinals remain and the template renders something with every field non-empty, the requirement is `all` with the remaining ordinals.
3. Otherwise **none** with no ordinals. A question template that does not parse (T6) also gives `none`.

Ordinals are listed in ascending order. Examples for fields A, B, C (ordinals 0, 1, 2):

| Question template | Requirement |
|---|---|
| `{{A}}{{B}}` | any [0, 1] |
| `{{#A}}{{B}}{{/A}}` | all [0, 1] |
| `{{^A}}{{B}}{{/A}}` | any [1] |
| `{{^A}}{{#B}}{{C}}{{/B}}{{/A}}` | all [1, 2] |
| `{{^C}}{{#A}}{{B}}{{/A}}{{/C}}` | all [0, 1] |
| `{{^A}}{{A}}{{/A}}` | any [0] |
| `{{text:A}}{{hint:C}}` | any [0, 2] |
| `{{Tags}}`, `{{#A}}x{{/A}}`, `{{^A}}x{{/A}}`, `{{#c1}}{{A}}{{/c1}}`, `{{Nope}}` | none |
| `{{#A}}{{B}}` (does not parse) | none |
| `<!--{{A}}-->{{B}}` | any [1] |

## T12. Parse result and source spans (Meguri contract)

The parsed form is public: U34 rewrites field references in place and U200 highlights tags (R930), both from the source text.

- Nodes: text, comment, replacement (key and filter names in source order), section open (conditional or negated, with its key), close (with its key), and the alternate-delimiter prefix of T2 (leading white space plus directives) when present.
- Every node except text records its byte range in the template source: a tag from the first byte of its opening delimiter to the last byte of its closing delimiter (`{{{Front}}}` spans its first 10 bytes; the last `}` is text), a comment from `<!--` through `-->`. Ranges are exact in both delimiter modes.
- Concatenating, in order, the source bytes of every node gives back the template byte for byte, for every input, including inputs with structural errors.
- Parsing never fails on any byte sequence that is valid UTF-8: structural errors (T6) are reported beside the nodes, not instead of them.

## T13. Robustness

- Tokenizing, parsing and rendering take time close to linear in the template length: a 5 MB template (R1805) renders in well under a second. A literal reading of T3 (look for a closing delimiter after every opening one) is quadratic on inputs such as many `{{` without `}}`; find the next closing delimiter once and reuse it.
- Nesting depth is limited only by memory: deep nesting must not overflow the stack.
- No input panics.

## T14. Differences from the Product Contract's wording

The oracle shows these; Meguri follows the oracle, since R1907 asks for identical output.

- R953: `{{{Field}}}` gives the field's text followed by a literal `}` (T3, T4), not exactly what `{{Field}}` gives.
- R959: an unclosed `{{` is text, not an error (T9). Only the five kinds of T9 exist.
- R960: a field named `FrontSide` never replaces the FrontSide special in a replacement (T7 rule 1); for every other special name the note field wins (`special-fields.md` P2).
- R964 and R968 differ in their filter details: `filters.md` F9.

## T15. Scenarios

- `template/tags`: T2–T5 tokenization and tag kinds.
- `template/conditionals`: T6–T7 nesting, emptiness of `<br>`, ` <div></div> `, `&nbsp;`, `<span></span>` and white space, `c<N>` keys.
- `template/comments`, `template/delimiters`: T3 and T2.
- `template/errors`: every T9 kind, the precedence rules and the side.
- `template/blank`: T10 and T11.2 through the `empty` flag.
- `template/names`: exact matching, spaces, scripts, decomposed names.
- `template/legacy-req`: T11.3 through the stored `req` (`legacy_json`).
- `corpus/templates-*`: generated templates and field values (`conformance/corpora/templates/generate.py`); T12's round trip is checked on every corpus template by Meguri's own tests.
- Observation kind: `template` (`specs/conformance/render-ops.md` Q1).

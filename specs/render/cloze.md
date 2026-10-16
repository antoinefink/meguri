# Cloze parsing, rendering and cloze utilities

Spec: render/cloze
Requirements: R963, R969, R970, R971, R972, R973, R974, R975, R976 (the missing-cloze check, C15), R977 (extraction side), R563 (via C8); supports AE33 (C5, with U27)
Anki: 26.09.3
Scenarios: conformance/scenarios/render/cloze/

How field text with cloze deletions (`{{c1::...}}`) is read, what the `cloze:` and `cloze-only:` template filters output, which cloze numbers a text contains, what a cloze type-in field expects, and when a card of a cloze note type is blank. Every output here is compared with the oracle string for string (R1907). The observation that exposes these functions to scenarios is `specs/conformance/cloze-ops.md`; the `template` observation (`specs/conformance/render-ops.md`) runs the filters and the blank check inside templates.

Pattern conventions ("shortest", leftmost-first, "any character" includes line breaks) are `text/strip-html#H1`.

## C1. Terms

- **Text**: the string a function works on: a field's stored HTML, or the output of the filters applied before this one in a filter chain (filter order and chains are U60's `render/filters`).
- **Card number** N: the card's template ordinal plus one. For a cloze note type this is the cloze number the card was generated for (U27 caps generated ordinals, so a card for `c700` has number 500). For a standard note type the `cloze:` filter still works, with N = template ordinal + 1. N is an integer from 1 to 65535.
- **Side**: front (question) or back (answer). The template engine evaluates `{{cloze:F}}` with the front side in the front template and with the back side in the back template, `{{FrontSide}}` included (U60, U62).
- **Number list** L of a cloze: its numbers in ascending order, each written in decimal without leading zeros, joined by `,` with no spaces: `1,2,3`.

## C2. Markers

**Opening marker.** In this order, with nothing in between:

1. `{{c`: two `{`, then a lower-case `c`. `{{C1::`, `{{ c1::` and `{c1::` are not markers.
2. The number run: the longest run of ASCII digits and commas at that point. It may be empty.
3. `::`.

The run is split at every comma. Each piece that consists of one or more digits with a value from 0 to 65535 gives a number (leading zeros allowed: `007` is 7). Empty pieces and pieces above 65535 give nothing. The numbers are deduplicated and sorted ascending. When no number remains, the characters are not an opening marker and stay text.

| Text at that point | Marker? | Numbers |
|---|---|---|
| `{{c1::` | yes | 1 |
| `{{c3,1,3,2::` | yes | 1, 2, 3 |
| `{{c2,,5::`, `{{c,1::`, `{{c1,::` | yes | 2, 5 / 1 / 1 |
| `{{c01::` | yes | 1 (`data-ordinal="1"`) |
| `{{c0::`, `{{c0,2::` | yes | 0 / 0, 2 |
| `{{c65535::` | yes | 65535 |
| `{{c65536::`, `{{c99999::` | no | (the only piece is too large) |
| `{{c::`, `{{c,::` | no | (no piece) |
| `{{c1 ::`, `{{c1:x` | no | (`::` must follow the run directly) |

**Closing marker.** `}}`.

## C3. Tokens

The text is read from start to end. At each character position:

- an opening marker starting there is taken whole;
- otherwise a closing marker starting there is taken;
- otherwise the character joins a text run, which continues up to the next position where an opening or closing marker starts, or to the end.

So `}}}` is a closing marker followed by the text `}`, and `{{{c1::x}}` is the text `{` followed by an opening marker. A failed opening marker such as `{{c::` is ordinary text and does not interrupt the run.

## C4. The cloze tree

The tokens build a sequence of top-level items. An item is a text or a cloze. A cloze has a number list, a sequence of child items, and an optional hint. While reading, some clozes are open; the innermost one is the one opened last.

- **Opening marker.** When fewer than 10 clozes are open, it opens a new cloze, placed inside the innermost open cloze (or at top level when none is open). When 10 are already open, the marker is dropped: it creates no cloze and leaves no text behind (R972).
- **Text run.** With no cloze open, it is a top-level text. With a cloze open, it is a child of the innermost open cloze, after hint splitting: when the run does not start with `image-occlusion:` and contains `::`, it is split at its first `::`. The part before becomes the child text (possibly empty). The part after becomes the cloze's hint; it may be empty and may contain more `::`. It replaces any hint that an earlier run of the same cloze set.
- **Closing marker.** With a cloze open, the innermost open cloze is finished and becomes the next item of the cloze around it (or of the top level). With no cloze open, the marker is the top-level text `}}`.
- **End of text.** Clozes still open are discarded with everything they contain: their texts, their hints and the finished clozes inside them. They count for nothing in any output of this file.

Consequences:

| Text | Tree (top level) |
|---|---|
| `{{c1::Paris::capital}}` | cloze 1: child `Paris`, hint `capital` |
| `{{c1::a::b::c}}` | cloze 1: child `a`, hint `b::c` |
| `{{c1::a::}}` | cloze 1: child `a`, hint empty |
| `{{c1::x::h1 {{c2::y}} z::h2}}` | cloze 1: children `x`, cloze 2 (`y`), ` z`; hint `h2` |
| `{{c1::x::h{{c2::y}}z}}` | cloze 1: children `x`, cloze 2 (`y`), `z`; hint `h` |
| `{{c1::{{c2::y}}::h}}` | cloze 1: children cloze 2 (`y`), empty text; hint `h` |
| `{{c1::a {{c2::b::hb}} c}}` | cloze 1: children `a `, cloze 2 (`b`, hint `hb`), ` c` |
| `a }} b` | text `a `, text `}}`, text ` b` |
| `{{c1::x}}}` | cloze 1 (`x`), text `}` |
| `a {{c1::b {{c2::c}} d` | text `a ` only |
| `{{c1::a}} {{c2::b` | cloze 1 (`a`), text ` ` |

With eleven nested opening markers `{{c1::` … `{{c11::` followed by `x` and eleven `}}`: clozes 1 to 10 nest, the eleventh marker is dropped, `x` is a child of cloze 10, the first ten `}}` close clozes 10 down to 1, and the eleventh `}}` is the top-level text `}}`.

## C5. Cloze numbers

- The **cloze numbers** of a text are the numbers other than 0 in the number lists of all clozes of its tree, nested clozes included, each once. Listed, they are in ascending order. Dropped markers and discarded clozes contribute nothing.

  | Text | Cloze numbers |
  |---|---|
  | `{{c1::a}}{{c3::b}}{{c0::l}}{{c700::z}}` | 1, 3, 700 (AE33; U27 maps 700 to ordinal 499) |
  | `{{c2::ab{{c1::cd}}}}e{{` | 1, 2 |
  | `{{c0,3,4::x}}` | 3, 4 |
  | `{{c1::open` | none |
  | `ÄÖÜ öäü` | none |

- Card generation (over the note's fields only, U27) and the blank check of cloze cards (over every value of the card's field map, C15) work on the union of the numbers of several texts.
- **Top-level presence**: a text *has a cloze* when one of its top-level clozes has a number other than 0. Nested clozes are not looked at: `{{c0::a {{c1::b}}}}` has cloze numbers {1} but has no cloze in this sense. U27's note field check (missing cloze, cloze in a non-cloze field, cloze in a standard note type) uses this test.

## C6. The `cloze:` filter

A cloze is **active** on card N when N is in its number list. A cloze with number 0 alone is never active.

1. When no cloze of the tree is active, the output is the empty string (R975). Every cloze counts, nested ones included, except clozes inside an image-occlusion cloze (C11).
2. Otherwise the output is the items of the top level, rendered in order, then passed through C8. A text renders as itself. A cloze renders by this table, where L is its number list (C1) and *content* is its children rendered by these same rules on the same side:

   | Side | Active | Output |
   |---|---|---|
   | front | yes | `<span class="cloze" data-cloze="` E(*content*) `" data-ordinal="` L `">[` H `]</span>` |
   | back | yes | `<span class="cloze" data-ordinal="` L `">` *content* `</span>` |
   | front or back | no | `<span class="cloze-inactive" data-ordinal="` L `">` *content* `</span>` |

   - H is the cloze's hint when it has one, written as is (not encoded; an empty hint gives `[]`), and `...` otherwise.
   - E is the attribute encoding of C7.
   - The front of an active cloze shows only `[` H `]`; its content, nested clozes rendered as spans included, appears only encoded inside `data-cloze`. A nested cloze active on the same card renders its own front span inside that content, so its markup is encoded once per level.

Examples on the front of card 1 and on the back:

| Text | Front, card 1 | Back, card 1 |
|---|---|---|
| `{{c1::Paris::capital}} is in {{c2::France}}` | `<span class="cloze" data-cloze="Paris" data-ordinal="1">[capital]</span> is in <span class="cloze-inactive" data-ordinal="2">France</span>` | `<span class="cloze" data-ordinal="1">Paris</span> is in <span class="cloze-inactive" data-ordinal="2">France</span>` |
| `{{c1,2::x}}` | `<span class="cloze" data-cloze="x" data-ordinal="1,2">[...]</span>` | `<span class="cloze" data-ordinal="1,2">x</span>` |
| `{{c1::}}` | `<span class="cloze" data-cloze="" data-ordinal="1">[...]</span>` | `<span class="cloze" data-ordinal="1"></span>` |
| `{{c2::a {{c1::b}}}}` | `<span class="cloze-inactive" data-ordinal="2">a <span class="cloze" data-cloze="b" data-ordinal="1">[...]</span></span>` | `<span class="cloze-inactive" data-ordinal="2">a <span class="cloze" data-ordinal="1">b</span></span>` |
| `{{c1::a {{c2::b}}}}` | `<span class="cloze" data-cloze="a&#x20;&lt;span&#x20;class&#x3D;&quot;cloze&#x2D;inactive&quot;&#x20;data&#x2D;ordinal&#x3D;&quot;2&quot;&gt;b&lt;&#x2F;span&gt;" data-ordinal="1">[...]</span>` | `<span class="cloze" data-ordinal="1">a <span class="cloze-inactive" data-ordinal="2">b</span></span>` |
| `{{c2::x}}`, or `{{c0::x}}` | empty | empty |

## C7. Attribute encoding

E replaces each character of its input, in order:

| Character | Written as |
|---|---|
| `"` | `&quot;` |
| `&` | `&amp;` |
| `'` | `&#x27;` |
| `<` | `&lt;` |
| `>` | `&gt;` |
| ASCII letters `A`–`Z`, `a`–`z` and digits `0`–`9` | unchanged |
| any other character below U+0100: other ASCII punctuation, the space, every control character (NUL, TAB, LF, CR, …) and U+0080–U+00FF | `&#x` + its code point as two upper-case hex digits + `;` |
| U+0100 and above (CJK, emoji, `€`, U+2028, …) | unchanged |

Examples: space → `&#x20;`, LF → `&#x0A;`, NUL → `&#x00;`, `\` → `&#x5C;`, `(` → `&#x28;`, `[` → `&#x5B;`, `.` → `&#x2E;`, `é` → `&#xE9;`, U+00A0 → `&#xA0;`, `日本😀` unchanged. This is the attribute encoder of the pinned `htmlescape` 0.3.1 (KTD2).

Because `[`, `:` and `]` are encoded, a `[sound:…]` tag inside an active cloze is no sound tag on the front: the front's AV extraction finds nothing there, and the back plays it (R974, R1020, R545; U62, U64).

## C8. MathJax in the `cloze:` output

The output of C6 step 2 goes through one more pass before it is returned (R973, R563):

- A **MathJax match** is a backslash followed by `(` or `[`, then the shortest run of any characters (possibly empty), then a backslash followed by `)` or `]`. The opener and closer need not correspond: `\(…\]` matches. Matches are found left to right, leftmost first, never overlapping; scanning resumes after a match's closer. An opener with no closer anywhere after it is no match. `\\(x\)` matches from its second backslash.
- Each match is replaced by its opener, then the markup removal of `text/strip-html#H2` applied to the run between opener and closer alone, then its closer. Entities are not decoded: `\(a &lt; b\)` is unchanged, `\(<b>x</b>\)` becomes `\(x\)`, `\(a < b\)` is unchanged (no `>` after the `<` inside the run).
- The pass covers the whole output: top-level text, inactive spans, active spans, and runs that cross span boundaries. Cloze spans inside math therefore lose their tags: `\(a {{c1::b}} c\)` gives `\(a [...] c\)` on the front of card 1 and `\(a b c\)` on the back. Inside `data-cloze` the backslash and brackets are encoded (C7), so nothing there matches.
- A match can remove one tag of a span and not the other. On the back of card 1, `{{c1::\(a}}b\)}}` gives `<span class="cloze" data-ordinal="1">\(ab\)}}`: the match `\(a</span>b\)` lost its `</span>`. This unbalanced output is Anki's, and Meguri reproduces it.
- Only the `cloze:` filter applies this pass. `cloze-only:` (C9) and type-in extraction (C10) never do.
- Cloze parsing (C2–C4) does not look at MathJax: a `}}` inside `\(…\)` closes a cloze like any other. In the example above, cloze 1 holds only `\(a`; the front of card 1 is `<span class="cloze" data-cloze="&#x5C;&#x28;a" data-ordinal="1">[...]</span>b\)}}`. Likewise `{{c2::\(\frac{1}{x^{2}}\)}}` closes at the `}}` of `{2}}`: its back on card 2 is `<span class="cloze" data-ordinal="2">\(\frac{1}{x^{2\)}}`. See C13.

## C9. The `cloze-only:` filter

- The entries are taken from every active cloze on card N, in document order: a cloze comes before the clozes nested in it, and children are taken left to right. On the front an entry is the cloze's hint, or `...` when it has none (an empty hint gives an empty entry). On the back an entry is the cloze's **plain content**: the concatenation of all texts inside it, nested clozes' texts included, without markers and without any hint.
- The output is the entries joined with `, ` (comma, space), or the empty string when there are none (R963). Nothing is encoded, no span is written, and C8 does not apply.
- Image-occlusion clozes (C11) are ordinary clozes here.

| Text | Card | Front | Back |
|---|---|---|---|
| `{{c1::Paris::capital}} and {{c1::Rome}}` | 1 | `capital, ...` | `Paris, Rome` |
| `{{c1,2::Seine}} and {{c1::Loire}} or {{c2::Rhône}}` | 2 | `..., ...` | `Seine, Rhône` |
| `{{c1::a {{c1::b}}}}` | 1 | `..., ...` | `a b, b` |
| `no cloze here` | 1 | empty | empty |

## C10. Type-in extraction

What a cloze type-in field (`{{type:cloze:F}}`) expects on card N (R977; the marker is U60's, the comparison U64's):

- The entries are those of C9 on the back: the plain content of each active cloze, in document order.
- No entry: the empty string. All entries identical (exact string comparison): that single entry. Otherwise: all entries joined with `, `, in order, duplicates kept.

| Text | Card | Expected answer |
|---|---|---|
| `{{c1::a}} {{c1::a}}` | 1 | `a` |
| `{{c1::a}} {{c1::a}} {{c1::b}}` | 1 | `a, a, b` |
| `{{c1::a  }} {{c1::a}}` | 1 | `a  , a` (trailing spaces make them differ) |
| `{{c1::x {{c1::x}}}}` | 1 | `x x, x` |
| `{{c2::a}}` | 1 | empty |

The input is the field's stored text and the output keeps its HTML as written (`{{c1::<b>Paris</b>}}` → `<b>Paris</b>`). U64 decides what an empty expectation means for the reviewer.

## C11. Image-occlusion clozes

- A cloze whose first child is a text starting with `image-occlusion:` (case-sensitive) is an **image-occlusion cloze**. Hint splitting never applies to such runs (C4).
- In the `cloze:` filter (C6) such a cloze is not rendered by C6's table. It becomes a `div` built from its number list L, the side, whether it is active, and its **shape text**: its first child's text after the prefix. Its other children are not rendered, and clozes nested in it are not consulted for C6 step 1. Its own activity counts for step 1.
- The class is chosen by the first row that applies:

  | Condition | Class |
  |---|---|
  | the number list contains 0, or (front and active) | `cloze` |
  | not active | `cloze-inactive` |
  | back and active | `cloze-highlight` |

  The element is `<div class="` class `" data-ordinal="` L `" ` + the shape attributes + `></div>`. The shape attributes, produced from the shape text, are U64's (`render/image-occlusion-render#I2`).
- C8 still applies to the whole output afterwards.
- U61 provides this element through a hook that U64 fills in. Until then the hook returns the empty string. No U61 scenario renders an image-occlusion cloze with `cloze:`; `render/cloze/numbers` and `render/cloze/only` contain one, where it is an ordinary cloze.
- C5, C9 and C10 treat image-occlusion clozes as ordinary clozes: their numbers count, and their plain content is the text starting with `image-occlusion:`.

## C12. Limits and robustness

- Every text has an output for every function here: there is no error case. Inputs of any length and content are valid, including unpaired markers and invalid HTML.
- Nesting depth is at most 10 open clozes (C4).
- Output size: each level of active clozes nested on the front encodes the markup below it once more (C6), and every encoding of punctuation adds more punctuation, so a punctuation character ten active levels deep becomes about 50,000 characters. Ten nested `c1` clozes around a few words give about 700 KB on the front of card 1. Anki has no cap and neither does Meguri, since a cap would change the output. Fuzz targets keep inputs small (1 KiB or less) or bound the output they accept.
- The output of C6 step 2 always has balanced `span` tags. C8 can unbalance it (C8), so a balance property holds for the C6 step alone, or for inputs with no backslash followed by `(` or `[`.

## C13. Notes on the plan's U61 test scenarios

- R973's first clause ("cloze parsing ignores `}}` inside MathJax") is not the behavior of Anki 26.09.3, the pinned oracle: parsing ignores MathJax there (C8, last point). Tests on Anki's main branch cover MathJax-aware cloze parsing (supplementary harvest rows, classified not applicable at the pin), so a later oracle bump may bring it. This spec follows the oracle, so Meguri renders, generates and checks cards exactly as the user's Anki does; adopting R973 now would be a deliberate deviation (`conformance/deviations.yaml`) that also changes card generation for clozes left unclosed by it, and needs a product decision. The plan's scenario "`{{c1::\(a}}b\)}}` does not close inside math" is covered by `render/cloze/mathjax` with the oracle's result. The second clause (HTML inside MathJax stripped from the output) holds and is C8.
- The plan expects type-in extraction of `{{c1::a}} {{c1::a}} {{c1::b}}` to give `a, b`. The oracle gives `a, a, b`: identical answers collapse only when all of them are identical (C10). Tests follow this spec and the goldens.
- The plan's fuzz property "the output never contains an unbalanced span" holds only as stated in C12.

## C14. Scenarios

None needs collection setup. All but the last two use the `cloze` observation (`specs/conformance/cloze-ops.md`); `blank-check` and `templates` use the `template` observation's new-note cases (`specs/conformance/render-ops.md` Q1.2), so they run the filters and the blank check through the template engine.

| Scenario | Covers |
|---|---|
| `render/cloze/basic` | C6 and C9 on single clozes, hints, inactive clozes and cards with no cloze (R969, R974, R975) |
| `render/cloze/multi-number` | number lists on several cards, shared and separate clozes (R970) |
| `render/cloze/repeated` | one number used several times (R971) and its type-in answer |
| `render/cloze/nesting` | nested clozes, same and different numbers, depth 10 and 11, stray and extra closers (R972) |
| `render/cloze/markers` | C2 and C3: marker syntax that is and is not a cloze, unclosed clozes |
| `render/cloze/hints` | C4 hint splitting |
| `render/cloze/encoding` | C7 on every class of character, sound tags in hidden clozes |
| `render/cloze/mathjax` | C8, and C2–C4 ignoring MathJax |
| `render/cloze/numbers` | C5 |
| `render/cloze/only` | C9 (R963) |
| `render/cloze/typing` | C10 (R977) |
| `render/cloze/blank-check` | C15 on cloze note types: numbers in used and unused fields, empty and cloze-free question templates, nested, zero, unclosed and listed numbers, the top card number 500, special values that count, a note field that hides a special |
| `render/cloze/templates` | C1, C6, C8, C9 inside templates: side by template, FrontSide carrying the front, card number from the ordinal, chains with `text`, `cloze`, `cloze-only` and an unknown filter (`filters.md` F1), `{{type:cloze:F}}`, the filters on other fields, sections and specials, and on a standard note type |

## C15. Blank cards of cloze note types

What `template-syntax.md` T10 decides for a card of a cloze note type (R976's missing-cloze case; U62 shows the notice and its text):

- The card with number N (C1) is **blank** when N is not among the cloze numbers (C5) of any value of the card's field map (`special-fields.md` P2). The map holds the note's fields and the special values `Tags`, `Type`, `Deck`, `Subdeck`, `Card`, `CardFlag`, `CardID` and `c<N>`, each unless a note field of the same name replaced it. `FrontSide` is never in the map and never counts.
- Special values count like fields. A note whose tags, note type name, template name or deck name hold `{{c3::…}}` has a card 3 that is not blank, even with no cloze 3 in any field. A note field named like a special replaces it here too: with a field `Tags`, that field's text counts and the note's tags do not.
- The templates play no part, and T11.2's analysis is not used. A card is not blank when its number appears only in a field that no template uses, or when its question template is empty or renders nothing. It is blank when its question template shows a field in full but no value holds N.
- Numbers are C5's: nested clozes count, 0 matches no card (N ≥ 1), and unclosed clozes and dropped markers count for nothing.
- The check runs after the question rendered without error (T9, T10). The question output is kept as rendered: often the empty string, since `cloze:` renders nothing without an active cloze (C6).
- Card generation (U27) reads the note's fields only (C5), not the special values.

| Field map (cloze note type, fields `Text`, `Extra`) | Question template | Card | Blank? | Question output |
|---|---|---|---|---|
| Text `{{c1::a}} {{c2::b}}` | `{{cloze:Text}}` | 3 | yes | empty |
| Text `plain`, Extra `{{c2::x}}` | `{{cloze:Text}}` | 2 | no | empty |
| Text `{{c1::a}}` | empty template, or `{{Extra}}` with Extra empty | 1 | no | empty |
| Text `no clozes` | `{{Text}}` | 1 | yes | `no clozes` |
| Text `{{c0::a {{c2::b}}}}` | `{{cloze:Text}}` | 2 | no | the C6 front |
| Text `{{c1::open` or `{{c0::label}}` | `{{cloze:Text}}` | 1 | yes | empty |
| Text `{{c500::x}}` | `{{cloze:Text}}` | 500 | no | the C6 front |
| Text `{{c1::a}}`, tags `{{c3::t}}` | `{{cloze:Text}}` | 3 | no | empty |
| Text `{{c1::a}}`, template name `{{c4::card}}` | `{{cloze:Text}}` | 4 | no | empty |
| Text `{{c1::a}}`, note type name `Gap {{c5::type}}` | `{{cloze:Text}}` | 5 / 6 | no / yes | empty |
| fields `Text`, `Tags`: Text `{{c1::a}}`, field Tags empty, note tags `{{c3::t}}` | `{{cloze:Text}}` | 3 | yes | empty |
| the same with field Tags `{{c3::f}}` | `{{cloze:Text}}` | 3 | no | empty |
